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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc as std_mpsc;
use tracing::warn;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush,
    DEFAULT_CHARSET, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
    Ellipse, EndPaint, FW_SEMIBOLD, GetStockObject, HDC, InvalidateRect, NULL_BRUSH,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SRCCOPY, SelectObject, SetBkMode,
    SetTextColor, SetWindowRgn, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, GetSystemMetrics, HMENU, KillTimer, LWA_ALPHA, MSG, RegisterClassW, SM_CXSCREEN,
    SM_CYSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

// Dimensions derived from StitchMCP Obsidian Flow design
pub const OVERLAY_WIDTH: i32 = 290;
pub const OVERLAY_HEIGHT: i32 = 44;
pub const OVERLAY_CORNER_RADIUS: i32 = 44; // Full capsule semicircular caps
pub const OVERLAY_BOTTOM_MARGIN: i32 = 90;

// Animation tick counter (driven by 40 FPS Win32 timer)
static ANIM_TICK: AtomicU32 = AtomicU32::new(0);

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
        SetLayeredWindowAttributes(hwnd, 0, 245, LWA_ALPHA);

        // 4. Start 40 FPS animation timer (25ms interval)
        SetTimer(hwnd, 1, 25, None);

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
                            InvalidateRect(hwnd, null_mut(), 0);
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
            WM_TIMER => {
                ANIM_TICK.fetch_add(1, Ordering::Relaxed);
                let is_visible = {
                    if let Ok(state) = CURRENT_STATE.lock() {
                        *state != OverlayState::Hidden
                    } else {
                        false
                    }
                };
                if is_visible {
                    InvalidateRect(hwnd, null_mut(), 0);
                }
                0
            }
            windows_sys::Win32::UI::WindowsAndMessaging::WM_DESTROY => {
                KillTimer(hwnd, 1);
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }
}

fn paint_overlay(_hwnd: HWND, hdc: HDC) {
    let state = *CURRENT_STATE.lock().unwrap();
    if state == OverlayState::Hidden {
        return;
    }

    let tick = ANIM_TICK.load(Ordering::Relaxed);

    unsafe {
        // Double buffering: Create compatible DC and memory bitmap for flicker-free rendering
        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bmp = CreateCompatibleBitmap(hdc, OVERLAY_WIDTH, OVERLAY_HEIGHT);
        let old_bmp = SelectObject(mem_dc, mem_bmp as _);

        // 1. Draw outer capsule background & specular glow border
        let (border_color, base_bg) = match state {
            OverlayState::Recording { .. } => (rgb(0, 240, 255), rgb(15, 17, 23)), // Electric Cyan rim + Obsidian Glass
            OverlayState::Processing => (rgb(168, 85, 247), rgb(15, 17, 23)), // Neon Violet rim + Obsidian Glass
            OverlayState::Done => (rgb(16, 185, 129), rgb(15, 17, 23)), // Matrix Emerald rim + Obsidian Glass
            OverlayState::Hidden => (rgb(40, 44, 56), rgb(15, 17, 23)),
        };

        let pen = CreatePen(PS_SOLID, 1, border_color);
        let brush = CreateSolidBrush(base_bg);
        let old_pen = SelectObject(mem_dc, pen as _);
        let old_brush = SelectObject(mem_dc, brush as _);

        RoundRect(
            mem_dc,
            0,
            0,
            OVERLAY_WIDTH,
            OVERLAY_HEIGHT,
            OVERLAY_CORNER_RADIUS,
            OVERLAY_CORNER_RADIUS,
        );

        SelectObject(mem_dc, old_pen);
        SelectObject(mem_dc, old_brush);
        DeleteObject(pen as _);
        DeleteObject(brush as _);

        SetBkMode(mem_dc, TRANSPARENT as _);

        let font_name: Vec<u16> = OsStr::new("Segoe UI")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        match state {
            OverlayState::Hidden => {}
            OverlayState::Recording { rms } => {
                // A. Animated Pulsating Ruby Radar Dot
                let cycle = ((tick % 36) as f32) / 36.0;
                let radar_r = 5.0 + cycle * 9.0;
                let radar_r_int = radar_r as i32;

                let null_brush = GetStockObject(NULL_BRUSH as _);
                let radar_pen = CreatePen(PS_SOLID, 1, rgb(180, 30, 55));
                let old_p = SelectObject(mem_dc, radar_pen as _);
                let old_b = SelectObject(mem_dc, null_brush);
                Ellipse(
                    mem_dc,
                    20 - radar_r_int,
                    22 - radar_r_int,
                    20 + radar_r_int,
                    22 + radar_r_int,
                );
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(radar_pen as _);

                // Breathing Ruby Core Dot
                let breathe = (tick as f32 * 0.15).sin();
                let core_r = if breathe > 0.0 { 4 } else { 3 };
                let ruby_brush = CreateSolidBrush(rgb(255, 45, 85));
                let ruby_pen = CreatePen(PS_SOLID, 1, rgb(255, 90, 120));
                let old_p = SelectObject(mem_dc, ruby_pen as _);
                let old_b = SelectObject(mem_dc, ruby_brush as _);
                Ellipse(mem_dc, 20 - core_r, 22 - core_r, 20 + core_r, 22 + core_r);
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(ruby_brush as _);
                DeleteObject(ruby_pen as _);

                // B. 9-Bar Reactive Neon Equalizer (Electric Cyan -> Neon Violet -> Hot Pink)
                let bar_colors = [
                    rgb(0, 240, 255),   // Electric Cyan
                    rgb(34, 211, 238),  // Cyan
                    rgb(56, 189, 248),  // Sky
                    rgb(99, 102, 241),  // Indigo
                    rgb(139, 92, 246),  // Violet
                    rgb(168, 85, 247),  // Purple
                    rgb(192, 132, 252), // Lavender
                    rgb(232, 121, 249), // Fuchsia
                    rgb(244, 63, 150),  // Hot Pink
                ];

                for (i, &bar_color) in bar_colors.iter().enumerate() {
                    let i_f = i as f32;
                    let bell = 1.0 - ((i_f - 4.0).abs() / 5.0) * 0.35; // Center emphasis
                    let wave1 = ((tick as f32 * 0.22) + (i_f * 0.70)).sin();
                    let wave2 = ((tick as f32 * 0.14) - (i_f * 0.45)).cos() * 0.5;
                    let wave_norm = (wave1 + wave2 + 1.5) / 3.0;

                    let speech_boost = (rms * 2.8).clamp(0.0, 1.0);
                    let amp = (speech_boost * 18.0 * bell) + (wave_norm * 5.0);
                    let bar_h = (4.0 + amp).clamp(4.0, 26.0) as i32;
                    let bx = 36 + (i as i32) * 7;
                    let by = (OVERLAY_HEIGHT - bar_h) / 2;

                    let bar_brush = CreateSolidBrush(bar_color);
                    let bar_pen = CreatePen(PS_SOLID, 1, bar_color);
                    let old_p = SelectObject(mem_dc, bar_pen as _);
                    let old_b = SelectObject(mem_dc, bar_brush as _);
                    RoundRect(mem_dc, bx, by, bx + 4, by + bar_h, 4, 4);
                    SelectObject(mem_dc, old_p);
                    SelectObject(mem_dc, old_b);
                    DeleteObject(bar_brush as _);
                    DeleteObject(bar_pen as _);
                }

                // C. Typography: "LISTENING" in crisp Segoe UI SemiBold
                let hfont = CreateFontW(
                    -12,
                    0,
                    0,
                    0,
                    FW_SEMIBOLD as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                );
                let old_f = SelectObject(mem_dc, hfont as _);
                SetTextColor(mem_dc, rgb(240, 245, 255));
                let text: Vec<u16> = OsStr::new("LISTENING")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut text_rect = RECT {
                    left: 104,
                    top: 0,
                    right: 198,
                    bottom: OVERLAY_HEIGHT,
                };
                DrawTextW(
                    mem_dc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut text_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
                SelectObject(mem_dc, old_f);
                DeleteObject(hfont as _);

                // D. Trailing Hotkey Chip: [F8 STOP]
                let chip_brush = CreateSolidBrush(rgb(26, 30, 42));
                let chip_pen = CreatePen(PS_SOLID, 1, rgb(48, 56, 78));
                let old_p = SelectObject(mem_dc, chip_pen as _);
                let old_b = SelectObject(mem_dc, chip_brush as _);
                RoundRect(mem_dc, 204, 11, 276, 33, 10, 10);
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(chip_brush as _);
                DeleteObject(chip_pen as _);

                let chip_font = CreateFontW(
                    -10,
                    0,
                    0,
                    0,
                    FW_SEMIBOLD as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                );
                let old_f = SelectObject(mem_dc, chip_font as _);
                SetTextColor(mem_dc, rgb(160, 175, 205));
                let chip_text: Vec<u16> = OsStr::new("F8 STOP")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut chip_rect = RECT {
                    left: 204,
                    top: 11,
                    right: 276,
                    bottom: 33,
                };
                DrawTextW(
                    mem_dc,
                    chip_text.as_ptr(),
                    chip_text.len() as i32 - 1,
                    &mut chip_rect,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                );
                SelectObject(mem_dc, old_f);
                DeleteObject(chip_font as _);
            }
            OverlayState::Processing => {
                // Animated Pulsing Violet Beacon
                let wave = (tick as f32 * 0.2).sin().abs();
                let beacon_r = (4.0 + wave * 3.0) as i32;

                let beacon_brush = CreateSolidBrush(rgb(168, 85, 247));
                let beacon_pen = CreatePen(PS_SOLID, 1, rgb(216, 180, 254));
                let old_p = SelectObject(mem_dc, beacon_pen as _);
                let old_b = SelectObject(mem_dc, beacon_brush as _);
                Ellipse(
                    mem_dc,
                    20 - beacon_r,
                    22 - beacon_r,
                    20 + beacon_r,
                    22 + beacon_r,
                );
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(beacon_brush as _);
                DeleteObject(beacon_pen as _);

                // 5 Dancing Sweep Bars (Violet to Cyan)
                let proc_colors = [
                    rgb(168, 85, 247),
                    rgb(139, 92, 246),
                    rgb(99, 102, 241),
                    rgb(56, 189, 248),
                    rgb(0, 240, 255),
                ];
                for (i, &proc_color) in proc_colors.iter().enumerate() {
                    let wave = ((tick as f32 * 0.28) + (i as f32 * 0.8)).sin().abs();
                    let bar_h = (4.0 + wave * 16.0) as i32;
                    let bx = 36 + (i as i32) * 7;
                    let by = (OVERLAY_HEIGHT - bar_h) / 2;

                    let bar_brush = CreateSolidBrush(proc_color);
                    let bar_pen = CreatePen(PS_SOLID, 1, proc_color);
                    let old_p = SelectObject(mem_dc, bar_pen as _);
                    let old_b = SelectObject(mem_dc, bar_brush as _);
                    RoundRect(mem_dc, bx, by, bx + 4, by + bar_h, 4, 4);
                    SelectObject(mem_dc, old_p);
                    SelectObject(mem_dc, old_b);
                    DeleteObject(bar_brush as _);
                    DeleteObject(bar_pen as _);
                }

                // Typography: "RACING LPUs..."
                let hfont = CreateFontW(
                    -12,
                    0,
                    0,
                    0,
                    FW_SEMIBOLD as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                );
                let old_f = SelectObject(mem_dc, hfont as _);
                SetTextColor(mem_dc, rgb(0, 240, 255));
                let text: Vec<u16> = OsStr::new("RACING LPUs...")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut text_rect = RECT {
                    left: 78,
                    top: 0,
                    right: 198,
                    bottom: OVERLAY_HEIGHT,
                };
                DrawTextW(
                    mem_dc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut text_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
                SelectObject(mem_dc, old_f);
                DeleteObject(hfont as _);

                // Trailing Chip: [DUAL LPU]
                let chip_brush = CreateSolidBrush(rgb(38, 22, 54));
                let chip_pen = CreatePen(PS_SOLID, 1, rgb(98, 48, 140));
                let old_p = SelectObject(mem_dc, chip_pen as _);
                let old_b = SelectObject(mem_dc, chip_brush as _);
                RoundRect(mem_dc, 204, 11, 276, 33, 10, 10);
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(chip_brush as _);
                DeleteObject(chip_pen as _);

                let chip_font = CreateFontW(
                    -10,
                    0,
                    0,
                    0,
                    FW_SEMIBOLD as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                );
                let old_f = SelectObject(mem_dc, chip_font as _);
                SetTextColor(mem_dc, rgb(216, 180, 254));
                let chip_text: Vec<u16> = OsStr::new("DUAL LPU")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut chip_rect = RECT {
                    left: 204,
                    top: 11,
                    right: 276,
                    bottom: 33,
                };
                DrawTextW(
                    mem_dc,
                    chip_text.as_ptr(),
                    chip_text.len() as i32 - 1,
                    &mut chip_rect,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                );
                SelectObject(mem_dc, old_f);
                DeleteObject(chip_font as _);
            }
            OverlayState::Done => {
                // Emerald Success Badge Dot
                let emerald_brush = CreateSolidBrush(rgb(16, 185, 129));
                let emerald_pen = CreatePen(PS_SOLID, 1, rgb(52, 211, 153));
                let old_p = SelectObject(mem_dc, emerald_pen as _);
                let old_b = SelectObject(mem_dc, emerald_brush as _);
                Ellipse(mem_dc, 15, 17, 25, 27);
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(emerald_brush as _);
                DeleteObject(emerald_pen as _);

                // 5 Settled Emerald Accent Bars
                for i in 0..5i32 {
                    let bx = 36 + i * 7;
                    let bar_h = 5;
                    let by = (OVERLAY_HEIGHT - bar_h) / 2;

                    let bar_brush = CreateSolidBrush(rgb(16, 185, 129));
                    let bar_pen = CreatePen(PS_SOLID, 1, rgb(52, 211, 153));
                    let old_p = SelectObject(mem_dc, bar_pen as _);
                    let old_b = SelectObject(mem_dc, bar_brush as _);
                    RoundRect(mem_dc, bx, by, bx + 4, by + bar_h, 4, 4);
                    SelectObject(mem_dc, old_p);
                    SelectObject(mem_dc, old_b);
                    DeleteObject(bar_brush as _);
                    DeleteObject(bar_pen as _);
                }

                // Typography: "DELIVERED"
                let hfont = CreateFontW(
                    -12,
                    0,
                    0,
                    0,
                    FW_SEMIBOLD as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                );
                let old_f = SelectObject(mem_dc, hfont as _);
                SetTextColor(mem_dc, rgb(52, 211, 153));
                let text: Vec<u16> = OsStr::new("DELIVERED")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut text_rect = RECT {
                    left: 78,
                    top: 0,
                    right: 198,
                    bottom: OVERLAY_HEIGHT,
                };
                DrawTextW(
                    mem_dc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut text_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
                SelectObject(mem_dc, old_f);
                DeleteObject(hfont as _);

                // Trailing Chip: [PASTED]
                let chip_brush = CreateSolidBrush(rgb(16, 44, 30));
                let chip_pen = CreatePen(PS_SOLID, 1, rgb(24, 96, 60));
                let old_p = SelectObject(mem_dc, chip_pen as _);
                let old_b = SelectObject(mem_dc, chip_brush as _);
                RoundRect(mem_dc, 204, 11, 276, 33, 10, 10);
                SelectObject(mem_dc, old_p);
                SelectObject(mem_dc, old_b);
                DeleteObject(chip_brush as _);
                DeleteObject(chip_pen as _);

                let chip_font = CreateFontW(
                    -10,
                    0,
                    0,
                    0,
                    FW_SEMIBOLD as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                );
                let old_f = SelectObject(mem_dc, chip_font as _);
                SetTextColor(mem_dc, rgb(110, 231, 183));
                let chip_text: Vec<u16> = OsStr::new("PASTED")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut chip_rect = RECT {
                    left: 204,
                    top: 11,
                    right: 276,
                    bottom: 33,
                };
                DrawTextW(
                    mem_dc,
                    chip_text.as_ptr(),
                    chip_text.len() as i32 - 1,
                    &mut chip_rect,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                );
                SelectObject(mem_dc, old_f);
                DeleteObject(chip_font as _);
            }
        }

        // Blit back-buffer to screen
        BitBlt(
            hdc,
            0,
            0,
            OVERLAY_WIDTH,
            OVERLAY_HEIGHT,
            mem_dc,
            0,
            0,
            SRCCOPY,
        );

        // Cleanup double buffering objects
        SelectObject(mem_dc, old_bmp);
        DeleteObject(mem_bmp as _);
        DeleteDC(mem_dc);
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
