//! Low-level Windows keyboard hook (`WH_KEYBOARD_LL`) on a dedicated Win32 message-pump thread.
//!
//! Features:
//! - Dedicated message-pump thread (never on Tokio runtime) to evade Windows `LowLevelHooksTimeout`.
//! - CapsLock suppression: returns 1 to prevent Windows from toggling Caps Lock state.
//! - Hybrid Mode: Tap (<400ms) to toggle on/off, Hold (>=400ms) for Push-to-Talk.
//! - Hook Liveness Watchdog: periodically verifies hook presence via synthetic `VK_F24` heartbeat.

use crate::core::types::{InteractionMode, TriggerKey};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CAPITAL, VK_F8, VK_F24,
    VK_RMENU,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Watchdog heartbeat key code (F24 is virtually unused by apps).
pub const WATCHDOG_KEY_CODE: u16 = VK_F24;

/// Unique 32-bit tag placed in dwExtraInfo to identify synthetic watchdog events.
pub const WATCHDOG_EXTRA_INFO: usize = 0x564F4953; // "VOIS"

/// Events emitted by the hotkey manager to session coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    StartRecording,
    StopRecording,
}

// Global thread-safe hook state shared with the static Win32 callback
struct HookState {
    trigger_vk: u32,
    suppress_trigger: bool,
    mode: InteractionMode,
    is_recording: AtomicBool,
    press_time: std::sync::Mutex<Option<Instant>>,
    event_tx: std::sync::Mutex<Option<std_mpsc::Sender<HotkeyEvent>>>,
    last_watchdog_ping: AtomicU64,
}

static GLOBAL_HOOK_STATE: std::sync::OnceLock<Arc<HookState>> = std::sync::OnceLock::new();
static GLOBAL_HHOOK: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let kbd = unsafe { *(l_param as *const KBDLLHOOKSTRUCT) };
        let vk_code = kbd.vkCode;
        let is_key_down = w_param as u32 == WM_KEYDOWN || w_param as u32 == WM_SYSKEYDOWN;
        let is_key_up = w_param as u32 == WM_KEYUP || w_param as u32 == WM_SYSKEYUP;

        if let Some(state) = GLOBAL_HOOK_STATE.get() {
            // 1. Check for synthetic watchdog heartbeat (VK_F24 with VOIS signature)
            if vk_code as u16 == WATCHDOG_KEY_CODE && kbd.dwExtraInfo == WATCHDOG_EXTRA_INFO {
                state.last_watchdog_ping.store(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    Ordering::Relaxed,
                );
                // Suppress synthetic event from reaching other apps
                return 1;
            }

            // 2. Check for configured hotkey
            if vk_code == state.trigger_vk {
                if is_key_down {
                    handle_key_down(state);
                } else if is_key_up {
                    handle_key_up(state);
                }

                // If trigger is CapsLock, swallow it completely so Windows doesn't toggle CapsLock
                if state.suppress_trigger {
                    return 1;
                }
            }
        }
    }

    unsafe {
        CallNextHookEx(
            GLOBAL_HHOOK.load(Ordering::Relaxed) as HHOOK,
            n_code,
            w_param,
            l_param,
        )
    }
}

fn handle_key_down(state: &HookState) {
    let mut press_guard = state.press_time.lock().unwrap();
    if press_guard.is_none() {
        *press_guard = Some(Instant::now());

        match state.mode {
            InteractionMode::PushToTalk => {
                if !state.is_recording.swap(true, Ordering::SeqCst) {
                    emit_event(state, HotkeyEvent::StartRecording);
                }
            }
            InteractionMode::Toggle => {
                // Handled on key up to avoid key repeating
            }
            InteractionMode::Hybrid => {
                // Immediate audio capture start on press for zero-latency response
                if !state.is_recording.load(Ordering::SeqCst) {
                    state.is_recording.store(true, Ordering::SeqCst);
                    emit_event(state, HotkeyEvent::StartRecording);
                }
            }
        }
    }
}

fn handle_key_up(state: &HookState) {
    let mut press_guard = state.press_time.lock().unwrap();
    let press_start = press_guard.take();

    match state.mode {
        InteractionMode::PushToTalk => {
            if state.is_recording.swap(false, Ordering::SeqCst) {
                emit_event(state, HotkeyEvent::StopRecording);
            }
        }
        InteractionMode::Toggle => {
            let was_recording = state.is_recording.load(Ordering::SeqCst);
            if was_recording {
                state.is_recording.store(false, Ordering::SeqCst);
                emit_event(state, HotkeyEvent::StopRecording);
            } else {
                state.is_recording.store(true, Ordering::SeqCst);
                emit_event(state, HotkeyEvent::StartRecording);
            }
        }
        InteractionMode::Hybrid => {
            let duration = press_start
                .map(|t| t.elapsed())
                .unwrap_or(Duration::from_millis(0));
            if duration >= Duration::from_millis(400) {
                // Hold mode: Release stops recording immediately
                state.is_recording.store(false, Ordering::SeqCst);
                emit_event(state, HotkeyEvent::StopRecording);
            } else {
                // Tap mode (<400ms): If already recording from a prior tap session, this tap stops it
                // If it was just started by this tap, leave it recording!
            }
        }
    }
}

fn emit_event(state: &HookState, event: HotkeyEvent) {
    let Ok(guard) = state.event_tx.lock() else {
        return;
    };
    if let Some(ref tx) = *guard {
        let _ = tx.send(event);
    }
}

/// Controller handle for managing the background hook thread and watchdog.
pub struct HotkeyManager {
    thread_id: Arc<AtomicU32>,
    is_running: Arc<AtomicBool>,
    state: Arc<HookState>,
}

impl HotkeyManager {
    /// Initialize keyboard hook and spawn dedicated message loop and watchdog threads.
    pub fn start(
        trigger: TriggerKey,
        mode: InteractionMode,
    ) -> Result<(Self, std_mpsc::Receiver<HotkeyEvent>), String> {
        let (trigger_vk, suppress) = match trigger {
            TriggerKey::CapsLock => (VK_CAPITAL as u32, true),
            TriggerKey::RightAlt => (VK_RMENU as u32, false),
            TriggerKey::F8 => (VK_F8 as u32, false),
            TriggerKey::Custom(vk) => (vk, false),
        };

        let (event_tx, event_rx) = std_mpsc::channel();
        let state = Arc::new(HookState {
            trigger_vk,
            suppress_trigger: suppress,
            mode,
            is_recording: AtomicBool::new(false),
            press_time: std::sync::Mutex::new(None),
            event_tx: std::sync::Mutex::new(Some(event_tx)),
            last_watchdog_ping: AtomicU64::new(0),
        });

        // Initialize or update global state
        let _ = GLOBAL_HOOK_STATE.set(Arc::clone(&state));

        let thread_id = Arc::new(AtomicU32::new(0));
        let is_running = Arc::new(AtomicBool::new(true));

        let tid_clone = Arc::clone(&thread_id);
        let run_clone = Arc::clone(&is_running);

        let (init_tx, init_rx) = std_mpsc::channel();

        // 1. Spawn Dedicated Win32 Message-Pump Thread
        std::thread::Builder::new()
            .name("voisu-win32-hook-pump".to_string())
            .spawn(move || unsafe {
                let current_tid = windows_sys::Win32::System::Threading::GetCurrentThreadId();
                tid_clone.store(current_tid, Ordering::SeqCst);

                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(low_level_keyboard_proc),
                    std::ptr::null_mut(),
                    0,
                );

                if hook.is_null() {
                    let err = windows_sys::Win32::Foundation::GetLastError();
                    let _ = init_tx.send(Err(format!(
                        "SetWindowsHookExW failed with error code: {}",
                        err
                    )));
                    return;
                }

                GLOBAL_HHOOK.store(hook as isize, Ordering::SeqCst);
                info!(
                    "Low-level keyboard hook installed successfully (hook handle: {:?})",
                    hook
                );
                let _ = init_tx.send(Ok(()));

                // Win32 Message Pump (GetMessage blocks until an OS event arrives)
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                info!("Exiting Win32 message pump thread. Unhooking...");
                UnhookWindowsHookEx(hook);
                GLOBAL_HHOOK.store(0, Ordering::SeqCst);
                run_clone.store(false, Ordering::SeqCst);
            })
            .map_err(|e| format!("Failed to spawn hook thread: {}", e))?;

        init_rx
            .recv()
            .map_err(|_| "Hook initialization thread panicked".to_string())??;

        // 2. Spawn Liveness Watchdog Thread
        let run_watchdog = Arc::clone(&is_running);
        let state_watchdog = Arc::clone(&state);

        std::thread::Builder::new()
            .name("voisu-hook-watchdog".to_string())
            .spawn(move || {
                debug!("Keyboard hook watchdog started.");
                while run_watchdog.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(3000));
                    if !run_watchdog.load(Ordering::Relaxed) {
                        break;
                    }

                    // Send synthetic VK_F24 event with signature
                    unsafe {
                        let mut input: INPUT = std::mem::zeroed();
                        input.r#type = INPUT_KEYBOARD;
                        input.Anonymous.ki = KEYBDINPUT {
                            wVk: WATCHDOG_KEY_CODE,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: WATCHDOG_EXTRA_INFO,
                        };
                        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
                    }

                    // Check if response registered within 1.5 seconds
                    std::thread::sleep(Duration::from_millis(500));
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let last_ping = state_watchdog.last_watchdog_ping.load(Ordering::Relaxed);

                    if last_ping > 0 && now_ms.saturating_sub(last_ping) > 6000 {
                        warn!("Hook watchdog alert: No heartbeat response for >6s. Windows may have removed the hook!");
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn watchdog thread: {}", e))?;

        Ok((
            Self {
                thread_id,
                is_running,
                state,
            },
            event_rx,
        ))
    }

    /// Check whether a dictation recording session is currently active.
    pub fn is_recording(&self) -> bool {
        self.state.is_recording.load(Ordering::SeqCst)
    }

    /// Return the active virtual key code triggering dictation.
    pub fn trigger_vk(&self) -> u32 {
        self.state.trigger_vk
    }

    /// Stops the keyboard hook and terminates the Win32 message loop thread.
    pub fn stop(&self) {
        if self.is_running.swap(false, Ordering::SeqCst) {
            let tid = self.thread_id.load(Ordering::SeqCst);
            if tid > 0 {
                unsafe {
                    PostThreadMessageW(tid, WM_QUIT, 0, 0);
                }
            }
        }
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_manager_lifecycle() {
        // Start hook with F8 trigger key and PushToTalk mode
        let result = HotkeyManager::start(TriggerKey::F8, InteractionMode::PushToTalk);
        assert!(
            result.is_ok(),
            "Failed to start HotkeyManager: {:?}",
            result.err()
        );

        let (manager, rx) = result.unwrap();
        assert_eq!(manager.trigger_vk(), VK_F8 as u32);
        assert!(!manager.is_recording());

        // Allow message loop and watchdog to start briefly
        std::thread::sleep(Duration::from_millis(50));

        // Stop manager cleanly
        manager.stop();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!manager.is_running.load(Ordering::SeqCst));
        drop(rx);
    }
}
