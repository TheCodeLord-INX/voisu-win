//! Win32 layered acrylic floating pill overlay (`ui::overlay`).
//!
//! Renders a non-activating, click-through, floating capsule window positioned bottom-center.
//! Uses Windows 11 DWM transient acrylic backdrop (`DWMSBT_TRANSIENTWINDOW`) with Windows 10
//! layered alpha fallback. Displays live reactive RMS audio meters during recording and
//! state transitions for processing and delivery.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use tracing::warn;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateRoundRectRgn, CreateSolidBrush, DT_SINGLELINE, DT_VCENTER, DeleteObject,
    DrawTextW, EndPaint, FillRect, HDC, InvalidateRect, PAINTSTRUCT, SetBkMode, SetTextColor,
    SetWindowRgn, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, GetSystemMetrics, HMENU, LWA_ALPHA, MSG, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

// Dimensions per design.md §4.1
pub const OVERLAY_WIDTH: i32 = 180;
pub const OVERLAY_HEIGHT: i32 = 40;
pub const OVERLAY_CORNER_RADIUS: i32 = 20; // Perfect capsule
pub const OVERLAY_BOTTOM_MARGIN: i32 = 80;

// DWM Windows 11 attributes
pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
pub const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
pub const DWMWCP_ROUND: u32 = 2;
pub const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
pub const DWMSBT_TRANSIENTWINDOW: u32 = 3; // Transient Acrylic

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayState {
    Hidden,
    Recording { rms: f32 },
    Processing,
    Done,
}

#[derive(Debug)]
enum OverlayCommand {
    SetState(OverlayState),
    Close,
}

pub struct OverlayController {
    cmd_tx: std_mpsc::Sender<OverlayCommand>,
    is_alive: Arc<AtomicBool>,
}

impl OverlayController {
    /// Spawns the dedicated Win32 UI overlay thread and initializes the floating pill window.
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = std_mpsc::channel();
        let is_alive = Arc::new(AtomicBool::new(true));
        let alive_clone = Arc::clone(&is_alive);

        std::thread::Builder::new()
            .name("voisu-pill-overlay".to_string())
            .spawn(move || {
                run_overlay_window_thread(cmd_rx, alive_clone);
            })
            .expect("Failed to spawn overlay thread");

        Self { cmd_tx, is_alive }
    }

    pub fn set_recording(&self, rms: f32) {
        let _ = self
            .cmd_tx
            .send(OverlayCommand::SetState(OverlayState::Recording { rms }));
    }

    pub fn set_processing(&self) {
        let _ = self
            .cmd_tx
            .send(OverlayCommand::SetState(OverlayState::Processing));
    }

    pub fn set_done(&self) {
        let _ = self
            .cmd_tx
            .send(OverlayCommand::SetState(OverlayState::Done));
    }

    pub fn hide(&self) {
        let _ = self
            .cmd_tx
            .send(OverlayCommand::SetState(OverlayState::Hidden));
    }

    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }

    pub fn close(&self) {
        let _ = self.cmd_tx.send(OverlayCommand::Close);
    }
}

impl Default for OverlayController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OverlayController {
    fn drop(&mut self) {
        self.close();
    }
}

static CURRENT_STATE: std::sync::Mutex<OverlayState> = std::sync::Mutex::new(OverlayState::Hidden);

fn run_overlay_window_thread(
    cmd_rx: std_mpsc::Receiver<OverlayCommand>,
    is_alive: Arc<AtomicBool>,
) {
    let class_name: Vec<u16> = OsStr::new("VoisuWinFloatingPillClass")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let hwnd = unsafe {
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(overlay_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: null_mut(),
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wnd_class);

        // Screen positioning: Bottom-center
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = (screen_w - OVERLAY_WIDTH) / 2;
        let pos_y = screen_h - OVERLAY_HEIGHT - OVERLAY_BOTTOM_MARGIN;

        let window_title: Vec<u16> = OsStr::new("Voisu Overlay")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let ex_style =
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;

        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_POPUP,
            pos_x,
            pos_y,
            OVERLAY_WIDTH,
            OVERLAY_HEIGHT,
            null_mut(),
            null_mut() as HMENU,
            null_mut(),
            null_mut(),
        );

        if hwnd.is_null() {
            warn!("Failed to create overlay HWND. Running without visual pill.");
            is_alive.store(false, Ordering::Relaxed);
            return;
        }

        // 1. Create rounded capsule window region
        let rgn = CreateRoundRectRgn(
            0,
            0,
            OVERLAY_WIDTH + 1,
            OVERLAY_HEIGHT + 1,
            OVERLAY_CORNER_RADIUS,
            OVERLAY_CORNER_RADIUS,
        );
        SetWindowRgn(hwnd, rgn, 1);

        // 2. Modern DWM Acrylic styling for Windows 11
        let dark_mode = 1u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );

        let backdrop_type = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop_type as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );

        // 3. Fallback alpha transparency for Windows 10
        SetLayeredWindowAttributes(hwnd, 0, 235, LWA_ALPHA);

        hwnd
    };

    // Spawn command processor channel
    let hwnd_val = hwnd as usize;
    std::thread::spawn(move || {
        let hwnd = hwnd_val as HWND;
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                OverlayCommand::SetState(state) => {
                    if let Ok(mut guard) = CURRENT_STATE.lock() {
                        *guard = state;
                    }
                    match state {
                        OverlayState::Hidden => unsafe {
                            ShowWindow(hwnd, SW_HIDE);
                        },
                        _ => unsafe {
                            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            SetWindowPos(
                                hwnd,
                                -1 as _,
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                            InvalidateRect(hwnd, null_mut(), 1);
                            UpdateWindow(hwnd);
                        },
                    }
                }
                OverlayCommand::Close => unsafe {
                    DestroyWindow(hwnd);
                    break;
                },
            }
        }
    });

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }

    is_alive.store(false, Ordering::Relaxed);
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            windows_sys::Win32::UI::WindowsAndMessaging::WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);

                paint_overlay(hwnd, hdc);

                EndPaint(hwnd, &ps);
                0
            }
            windows_sys::Win32::UI::WindowsAndMessaging::WM_DESTROY => {
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }
}

fn paint_overlay(_hwnd: HWND, hdc: HDC) {
    let state = *CURRENT_STATE.lock().unwrap();

    let rect = RECT {
        left: 0,
        top: 0,
        right: OVERLAY_WIDTH,
        bottom: OVERLAY_HEIGHT,
    };

    unsafe {
        // 1. Dark Acrylic background brush
        let bg_color = rgb(18, 18, 22);
        let bg_brush = CreateSolidBrush(bg_color);
        FillRect(hdc, &rect, bg_brush);
        DeleteObject(bg_brush as _);

        SetBkMode(hdc, TRANSPARENT as _);

        match state {
            OverlayState::Hidden => {}
            OverlayState::Recording { rms } => {
                // Red recording dot
                let dot_brush = CreateSolidBrush(rgb(255, 75, 75));
                let dot_rect = RECT {
                    left: 18,
                    top: 15,
                    right: 28,
                    bottom: 25,
                };
                FillRect(hdc, &dot_rect, dot_brush);
                DeleteObject(dot_brush as _);

                // 5-bar reactive audio waveform meter
                let base_x = 36;
                let bar_w = 3;
                let spacing = 2;
                let max_h = 18;
                let meter_brush = CreateSolidBrush(rgb(255, 120, 120));

                for i in 0..5 {
                    let factor = match i {
                        0 | 4 => 0.5,
                        1 | 3 => 0.8,
                        _ => 1.0,
                    };
                    let h = ((rms * factor * max_h as f32) as i32).clamp(3, max_h);
                    let x = base_x + i * (bar_w + spacing);
                    let y = (OVERLAY_HEIGHT - h) / 2;
                    let bar_rect = RECT {
                        left: x,
                        top: y,
                        right: x + bar_w,
                        bottom: y + h,
                    };
                    FillRect(hdc, &bar_rect, meter_brush);
                }
                DeleteObject(meter_brush as _);

                // Text "Listening..."
                SetTextColor(hdc, rgb(255, 255, 255));
                let text: Vec<u16> = OsStr::new("Listening...")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut text_rect = RECT {
                    left: 68,
                    top: 0,
                    right: OVERLAY_WIDTH - 12,
                    bottom: OVERLAY_HEIGHT,
                };
                DrawTextW(
                    hdc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut text_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
            }
            OverlayState::Processing => {
                // Amber processing dot
                let dot_brush = CreateSolidBrush(rgb(245, 166, 35));
                let dot_rect = RECT {
                    left: 20,
                    top: 15,
                    right: 30,
                    bottom: 25,
                };
                FillRect(hdc, &dot_rect, dot_brush);
                DeleteObject(dot_brush as _);

                // Text "Racing LPUs..."
                SetTextColor(hdc, rgb(245, 166, 35));
                let text: Vec<u16> = OsStr::new("Racing LPUs...")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut text_rect = RECT {
                    left: 40,
                    top: 0,
                    right: OVERLAY_WIDTH - 12,
                    bottom: OVERLAY_HEIGHT,
                };
                DrawTextW(
                    hdc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut text_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
            }
            OverlayState::Done => {
                // Green check dot
                let dot_brush = CreateSolidBrush(rgb(39, 174, 96));
                let dot_rect = RECT {
                    left: 20,
                    top: 15,
                    right: 30,
                    bottom: 25,
                };
                FillRect(hdc, &dot_rect, dot_brush);
                DeleteObject(dot_brush as _);

                // Text "Done!"
                SetTextColor(hdc, rgb(39, 174, 96));
                let text: Vec<u16> = OsStr::new("Pasted!")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut text_rect = RECT {
                    left: 40,
                    top: 0,
                    right: OVERLAY_WIDTH - 12,
                    bottom: OVERLAY_HEIGHT,
                };
                DrawTextW(
                    hdc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut text_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
            }
        }
    }
}

#[inline]
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_overlay_controller_lifecycle() {
        let overlay = OverlayController::new();
        assert!(overlay.is_alive());

        overlay.set_recording(0.5);
        overlay.set_processing();
        overlay.set_done();
        overlay.hide();

        std::thread::sleep(Duration::from_millis(100));
        overlay.close();
    }
}
