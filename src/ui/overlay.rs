//! Win32 layered Neo-Brutalist floating pill overlay (`ui::overlay`).
//!
//! Renders a non-activating, click-through, floating pill window positioned bottom-center.
//! Uses Neo-Brutalist design: bold solid colors, thick black borders, hard drop-shadows,
//! chunky typography. Color-key transparency (magenta) enables the hard shadow effect.
//! Displays live reactive RMS audio meters during recording and state transitions.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc as std_mpsc;
use tracing::warn;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush,
    DEFAULT_CHARSET, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
    EndPaint, FillRect, FW_BOLD, FW_SEMIBOLD, HDC, InvalidateRect,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SRCCOPY, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, GetSystemMetrics, HMENU, KillTimer, MSG, RegisterClassW, SM_CXSCREEN,
    SM_CYSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

// Neo-Brutalist Pill Dimensions
// Main pill: 280 x 48, hard shadow offset 5px right + 5px down
// Total window canvas includes shadow space
pub const PILL_WIDTH: i32 = 280;
pub const PILL_HEIGHT: i32 = 48;
const SHADOW_OFFSET: i32 = 5;
pub const OVERLAY_WIDTH: i32 = PILL_WIDTH + SHADOW_OFFSET;
pub const OVERLAY_HEIGHT: i32 = PILL_HEIGHT + SHADOW_OFFSET;
pub const OVERLAY_CORNER_RADIUS: i32 = 14; // Slight rounding, not full capsule
pub const OVERLAY_BOTTOM_MARGIN: i32 = 90;

// Neo-Brutalist color keying: magenta = transparent
const COLORKEY_R: u8 = 255;
const COLORKEY_G: u8 = 0;
const COLORKEY_B: u8 = 255;

// LWA_COLORKEY flag for SetLayeredWindowAttributes
const LWA_COLORKEY: u32 = 0x00000001;

// Animation tick counter (driven by 40 FPS Win32 timer)
static ANIM_TICK: AtomicU32 = AtomicU32::new(0);

// Smoothed activity envelope (stored as f32 bits in AtomicU32 for ballistics)
static SMOOTHED_ACTIVITY: AtomicU32 = AtomicU32::new(0);

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

        // Neo-Brutalist: Use color-key transparency so magenta pixels become invisible.
        // This lets the hard black shadow float over the desktop.
        SetLayeredWindowAttributes(
            hwnd,
            rgb(COLORKEY_R, COLORKEY_G, COLORKEY_B),
            0,
            LWA_COLORKEY,
        );

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
        // Double buffering for flicker-free Neo-Brutalist rendering
        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bmp = CreateCompatibleBitmap(hdc, OVERLAY_WIDTH, OVERLAY_HEIGHT);
        let old_bmp = SelectObject(mem_dc, mem_bmp as _);

        // Fill entire canvas with magenta (color-key = transparent)
        let key_brush = CreateSolidBrush(rgb(COLORKEY_R, COLORKEY_G, COLORKEY_B));
        let canvas = RECT { left: 0, top: 0, right: OVERLAY_WIDTH, bottom: OVERLAY_HEIGHT };
        FillRect(mem_dc, &canvas, key_brush);
        DeleteObject(key_brush as _);

        // --- Neo-Brutalist palette per state ---
        let (pill_bg, pill_text_color, accent_color, label, chip_label) = match state {
            OverlayState::Recording { .. } => (
                rgb(255, 229, 0),   // Electric Yellow
                rgb(0, 0, 0),       // Black text
                rgb(0, 0, 0),       // Black bars
                "LISTENING",
                "F8 STOP",
            ),
            OverlayState::Processing => (
                rgb(255, 51, 102),  // Hot Pink
                rgb(255, 255, 255), // White text
                rgb(0, 0, 0),       // Black bars
                "PROCESSING",
                "DUAL LPU",
            ),
            OverlayState::Done => (
                rgb(170, 255, 0),   // Acid Lime
                rgb(0, 0, 0),       // Black text
                rgb(0, 0, 0),       // Black bars
                "DELIVERED",
                "PASTED",
            ),
            OverlayState::Hidden => return,
        };

        // 1. Hard drop-shadow (solid black, offset right+down)
        let shadow_brush = CreateSolidBrush(rgb(0, 0, 0));
        let shadow_pen = CreatePen(PS_SOLID, 1, rgb(0, 0, 0));
        let old_p = SelectObject(mem_dc, shadow_pen as _);
        let old_b = SelectObject(mem_dc, shadow_brush as _);
        RoundRect(
            mem_dc,
            SHADOW_OFFSET, SHADOW_OFFSET,
            SHADOW_OFFSET + PILL_WIDTH, SHADOW_OFFSET + PILL_HEIGHT,
            OVERLAY_CORNER_RADIUS, OVERLAY_CORNER_RADIUS,
        );
        SelectObject(mem_dc, old_p);
        SelectObject(mem_dc, old_b);
        DeleteObject(shadow_brush as _);
        DeleteObject(shadow_pen as _);

        // 2. Main pill body (bold color fill + thick 3px black border)
        let body_brush = CreateSolidBrush(pill_bg);
        let body_pen = CreatePen(PS_SOLID, 3, rgb(0, 0, 0));
        let old_p = SelectObject(mem_dc, body_pen as _);
        let old_b = SelectObject(mem_dc, body_brush as _);
        RoundRect(
            mem_dc,
            0, 0,
            PILL_WIDTH, PILL_HEIGHT,
            OVERLAY_CORNER_RADIUS, OVERLAY_CORNER_RADIUS,
        );
        SelectObject(mem_dc, old_p);
        SelectObject(mem_dc, old_b);
        DeleteObject(body_brush as _);
        DeleteObject(body_pen as _);

        SetBkMode(mem_dc, TRANSPARENT as _);

        // Font setup (chunky, bold)
        let font_name: Vec<u16> = OsStr::new("Segoe UI")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Helper: draw bold label text
        let draw_label = |dc: HDC, text: &str, rect: RECT, color: COLORREF| {
            let hfont = CreateFontW(
                -14, 0, 0, 0, FW_BOLD as i32, 0, 0, 0,
                DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32, 0, font_name.as_ptr(),
            );
            let old_f = SelectObject(dc, hfont as _);
            SetTextColor(dc, color);
            let wtext: Vec<u16> = OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect();
            let mut r = rect;
            DrawTextW(dc, wtext.as_ptr(), wtext.len() as i32 - 1, &mut r, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            SelectObject(dc, old_f);
            DeleteObject(hfont as _);
        };

        // Helper: draw chip text (smaller)
        let draw_chip_text = |dc: HDC, text: &str, rect: RECT, color: COLORREF| {
            let hfont = CreateFontW(
                -10, 0, 0, 0, FW_SEMIBOLD as i32, 0, 0, 0,
                DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32, 0, font_name.as_ptr(),
            );
            let old_f = SelectObject(dc, hfont as _);
            SetTextColor(dc, color);
            let wtext: Vec<u16> = OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect();
            let mut r = rect;
            DrawTextW(dc, wtext.as_ptr(), wtext.len() as i32 - 1, &mut r, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            SelectObject(dc, old_f);
            DeleteObject(hfont as _);
        };

        // 3. State label (left side of pill)
        draw_label(
            mem_dc, label,
            RECT { left: 16, top: 0, right: 120, bottom: PILL_HEIGHT },
            pill_text_color,
        );

        // 4. Equalizer bars (center of pill)
        match state {
            OverlayState::Recording { rms } => {
                let active_level = (rms - 0.0012).max(0.0);
                // Compressive power curve: whisper / low pitch jumps up to 0.4-0.6, normal voice to 0.8-1.0
                let target_activity = (active_level * 50.0).clamp(0.0, 1.0).powf(0.45);

                // Ballistics: fast attack (responsive jump), smooth musical decay (~150ms half-life)
                let prev_activity = f32::from_bits(SMOOTHED_ACTIVITY.load(Ordering::Relaxed));
                let activity = if target_activity > prev_activity {
                    prev_activity + (target_activity - prev_activity) * 0.80
                } else {
                    (prev_activity * 0.88).max(0.0)
                };
                SMOOTHED_ACTIVITY.store(activity.to_bits(), Ordering::Relaxed);

                let num_bars = 7;
                for i in 0..num_bars {
                    let i_f = i as f32;

                    // Frequency weighting profile (bars 0..7 from lowest to highest frequency).
                    // Substantially boost lower frequencies so deep tones and bass voice resonate high:
                    let freq_weight = match i {
                        0 => 1.45, // Sub-bass / chest resonance
                        1 => 1.50, // Low bass
                        2 => 1.35, // Low-mids
                        3 => 1.20, // Mid body
                        4 => 1.05, // Upper-mids
                        5 => 0.90, // Presence
                        _ => 0.80, // Treble
                    };

                    let speed = 0.18;
                    let t = tick as f32 * speed;
                    let phase = i_f * 0.85;

                    // Wave component 1: traveling sine wave
                    let w1 = (t + phase).sin();
                    // Wave component 2: counter-harmonic wave
                    let w2 = (t * 0.65 - phase * 0.7 + (i_f * 1.3)).cos() * 0.5;
                    // Lower-frequency rhythmic bass swell
                    let low_freq_pulse = if i < 3 {
                        (t * 0.35 + i_f * 0.4).sin().abs() * 0.35
                    } else {
                        0.0
                    };

                    let wave_motion = ((w1 + w2 + 1.5) / 3.0) + low_freq_pulse;

                    // Max dynamic range of 32px (resting 4px -> active up to 36px tall)
                    let max_dynamic = 32.0;
                    let bar_amp = (activity * freq_weight * wave_motion * max_dynamic).clamp(0.0, max_dynamic);
                    let bar_h = (4.0 + bar_amp) as i32;
                    let bx = 128 + (i as i32) * 10;
                    let by = (PILL_HEIGHT - bar_h) / 2;

                    let bar_brush = CreateSolidBrush(accent_color);
                    let bar_pen = CreatePen(PS_SOLID, 1, accent_color);
                    let old_p = SelectObject(mem_dc, bar_pen as _);
                    let old_b = SelectObject(mem_dc, bar_brush as _);
                    RoundRect(mem_dc, bx, by, bx + 6, by + bar_h, 3, 3);
                    SelectObject(mem_dc, old_p);
                    SelectObject(mem_dc, old_b);
                    DeleteObject(bar_brush as _);
                    DeleteObject(bar_pen as _);
                }
            }
            OverlayState::Processing => {
                SMOOTHED_ACTIVITY.store(0, Ordering::Relaxed);
                // Bouncing blocks animation
                for i in 0..5i32 {
                    let phase = ((tick as f32 * 0.25) + (i as f32 * 0.9)).sin().abs();
                    let bar_h = (5.0 + phase * 25.0) as i32;
                    let bx = 138 + i * 10;
                    let by = (PILL_HEIGHT - bar_h) / 2;

                    let bar_brush = CreateSolidBrush(accent_color);
                    let bar_pen = CreatePen(PS_SOLID, 1, accent_color);
                    let old_p = SelectObject(mem_dc, bar_pen as _);
                    let old_b = SelectObject(mem_dc, bar_brush as _);
                    RoundRect(mem_dc, bx, by, bx + 6, by + bar_h, 3, 3);
                    SelectObject(mem_dc, old_p);
                    SelectObject(mem_dc, old_b);
                    DeleteObject(bar_brush as _);
                    DeleteObject(bar_pen as _);
                }
            }
            OverlayState::Done => {
                // Settled flat bars
                for i in 0..5i32 {
                    let bx = 138 + i * 10;
                    let bar_h = 5;
                    let by = (PILL_HEIGHT - bar_h) / 2;

                    let bar_brush = CreateSolidBrush(accent_color);
                    let bar_pen = CreatePen(PS_SOLID, 1, accent_color);
                    let old_p = SelectObject(mem_dc, bar_pen as _);
                    let old_b = SelectObject(mem_dc, bar_brush as _);
                    RoundRect(mem_dc, bx, by, bx + 6, by + bar_h, 3, 3);
                    SelectObject(mem_dc, old_p);
                    SelectObject(mem_dc, old_b);
                    DeleteObject(bar_brush as _);
                    DeleteObject(bar_pen as _);
                }
            }
            OverlayState::Hidden => {}
        }

        // 5. Chip button (right side) — black bg with bold white text, thick border
        let chip_bg = CreateSolidBrush(rgb(0, 0, 0));
        let chip_border = CreatePen(PS_SOLID, 2, rgb(0, 0, 0));
        let old_p = SelectObject(mem_dc, chip_border as _);
        let old_b = SelectObject(mem_dc, chip_bg as _);
        RoundRect(mem_dc, 204, 12, 268, 36, 8, 8);
        SelectObject(mem_dc, old_p);
        SelectObject(mem_dc, old_b);
        DeleteObject(chip_bg as _);
        DeleteObject(chip_border as _);

        draw_chip_text(
            mem_dc, chip_label,
            RECT { left: 204, top: 12, right: 268, bottom: 36 },
            rgb(255, 255, 255),
        );

        // Blit back-buffer to screen
        BitBlt(
            hdc, 0, 0, OVERLAY_WIDTH, OVERLAY_HEIGHT,
            mem_dc, 0, 0, SRCCOPY,
        );

        // Cleanup
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
