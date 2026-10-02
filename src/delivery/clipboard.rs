//! Windows Smart Clipboard Injector with prior content restoration and Win+V history suppression.
//!
//! Delivers formatted text to the currently focused Windows application via synthetic `Ctrl+V`
//! while preserving the user's prior clipboard text and preventing pollution of Windows 11's
//! `Win+V` clipboard history. Includes a fallback to direct `SendInput` Unicode typing.

use crate::core::types::DeliveryMode;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};
use windows_sys::Win32::Foundation::{HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VK_CONTROL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CURSORINFO, GUI_CARETBLINKING, GUITHREADINFO, GetClassNameW, GetCursorInfo,
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IDC_IBEAM, LoadCursorW,
};

/// Win32 standard clipboard format for Unicode text (CF_UNICODETEXT = 13).
pub const CF_UNICODETEXT: u32 = 13;

// Windows 11 format to prevent transient dictation from polluting Win+V history
const EXCLUDE_FROM_MONITOR_PROCESSING_FORMAT: &str = "ExcludeClipboardContentFromMonitorProcessing";

#[derive(Error, Debug)]
pub enum DeliveryError {
    #[error("Clipboard open timed out after retries")]
    ClipboardLocked,
    #[error("Failed to allocate global memory for clipboard data")]
    MemoryAllocationFailed,
    #[error("Failed to lock global memory buffer")]
    MemoryLockFailed,
    #[error("SendInput failed to inject keystrokes")]
    SendInputFailed,
}

pub struct ClipboardInjector {
    restore_timeout_ms: u32,
    exclude_format_id: u32,
}

/// Outcome of a transcription delivery operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// Injected text directly into the focused text box (and restored prior clipboard).
    Injected,
    /// Cursor was not focused on any text box; transcribed text was copied to the clipboard.
    CopiedToClipboard,
}

impl ClipboardInjector {
    pub fn new(restore_timeout_ms: u32) -> Self {
        // Register Windows 11 clipboard history exclusion format
        let wide: Vec<u16> = OsStr::new(EXCLUDE_FROM_MONITOR_PROCESSING_FORMAT)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let format_id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };

        Self {
            restore_timeout_ms,
            exclude_format_id: format_id,
        }
    }

    pub fn restore_timeout_ms(&self) -> u32 {
        self.restore_timeout_ms
    }

    /// Check whether the foreground application has an active text box or blinking caret focused.
    pub fn is_text_box_focused(&self) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() {
                return false;
            }

            // Check if foreground window is desktop or taskbar
            let mut fg_class_buf = [0u16; 256];
            let fg_len = GetClassNameW(fg, fg_class_buf.as_mut_ptr(), 256);
            if fg_len > 0 {
                let fg_class =
                    String::from_utf16_lossy(&fg_class_buf[..fg_len as usize]).to_lowercase();
                if fg_class == "progman"
                    || fg_class == "workerw"
                    || fg_class == "shell_traywnd"
                    || fg_class == "shell_secondarytraywnd"
                {
                    return false;
                }
            }

            let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            let mut gui: GUITHREADINFO = std::mem::zeroed();
            gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

            if GetGUIThreadInfo(tid, &mut gui) != 0 {
                // 1. If an actual Win32 caret window exists
                if !gui.hwndCaret.is_null() {
                    return true;
                }

                // 2. If the caret is blinking
                if (gui.flags & GUI_CARETBLINKING) != 0 {
                    return true;
                }

                // 3. If the caret rectangle has positive height (reported by Chromium, Electron, WPF, Java)
                if gui.rcCaret.bottom > gui.rcCaret.top {
                    return true;
                }

                // 4. Check the focused control window class name
                let focus = if !gui.hwndFocus.is_null() {
                    gui.hwndFocus
                } else {
                    gui.hwndActive
                };
                if !focus.is_null() {
                    let mut class_buf = [0u16; 256];
                    let len = GetClassNameW(focus, class_buf.as_mut_ptr(), 256);
                    if len > 0 {
                        let class_name =
                            String::from_utf16_lossy(&class_buf[..len as usize]).to_lowercase();
                        if class_name.contains("edit")
                            || class_name.contains("scintilla")
                            || class_name.contains("terminal")
                            || class_name.contains("console")
                            || class_name.contains("textbox")
                            || class_name.contains("textarea")
                            || class_name.contains("rich")
                        {
                            return true;
                        }
                    }
                }
            }

            // 5. Check mouse cursor icon (I-beam cursor indicates text field hover/focus)
            let mut cursor_info: CURSORINFO = std::mem::zeroed();
            cursor_info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
            if GetCursorInfo(&mut cursor_info) != 0 {
                let ibeam = LoadCursorW(null_mut(), IDC_IBEAM);
                if cursor_info.hCursor == ibeam {
                    return true;
                }
            }

            false
        }
    }

    /// Primary delivery entry point:
    /// - If cursor is focused on a text box: injects via SmartClipboard or direct typing and restores prior clipboard.
    /// - If cursor is NOT focused on any text box: copies transcribed text to Windows clipboard and leaves it there.
    pub fn deliver(
        &self,
        text: &str,
        mode: DeliveryMode,
    ) -> Result<DeliveryOutcome, DeliveryError> {
        let text_box_focused = self.is_text_box_focused();

        if !text_box_focused {
            // Cursor is not focused on any text box:
            // Copy transcript to clipboard without exclusion format so it's in standard clipboard and history.
            // Do NOT synthesize Ctrl+V, and do NOT restore prior clipboard.
            self.set_clipboard_text(text, false)?;
            info!("No text box focused; transcribed text copied to Windows clipboard.");
            return Ok(DeliveryOutcome::CopiedToClipboard);
        }

        // Text box is focused: perform injection
        match mode {
            DeliveryMode::SmartClipboard => {
                self.deliver_smart_clipboard(text)?;
                Ok(DeliveryOutcome::Injected)
            }
            DeliveryMode::SendInputUnicode => {
                self.deliver_unicode_direct(text)?;
                Ok(DeliveryOutcome::Injected)
            }
        }
    }

    /// Smart Clipboard: Back up prior clipboard, write transcript, synthesize Ctrl+V, restore.
    pub fn deliver_smart_clipboard(&self, text: &str) -> Result<(), DeliveryError> {
        // 1. Try backing up current clipboard text
        let prior_clipboard = self.get_clipboard_text();

        // 2. Write new transcript to clipboard
        if let Err(e) = self.set_clipboard_text(text, true) {
            warn!(
                "Failed to set clipboard text: {}. Falling back to direct SendInput typing.",
                e
            );
            return self.deliver_unicode_direct(text);
        }

        // 3. Synthesize Ctrl+V paste
        self.synthesize_ctrl_v()?;

        // 4. Wait for focused target application to consume clipboard paste
        sleep(Duration::from_millis(self.restore_timeout_ms as u64));

        // 5. Restore previous clipboard content if there was any
        if let Some(prior) = prior_clipboard {
            if let Err(e) = self.set_clipboard_text(&prior, false) {
                warn!("Failed to restore prior clipboard content: {}", e);
            } else {
                debug!("Prior clipboard content successfully restored.");
            }
        }

        Ok(())
    }

    /// Fallback: Injects text directly character-by-character using SendInput Unicode packets.
    pub fn deliver_unicode_direct(&self, text: &str) -> Result<(), DeliveryError> {
        let wide: Vec<u16> = OsStr::new(text).encode_wide().collect();
        if wide.is_empty() {
            return Ok(());
        }

        let mut inputs = Vec::with_capacity(wide.len() * 2);
        for &ch in &wide {
            // Key down
            let mut down: INPUT = unsafe { std::mem::zeroed() };
            down.r#type = windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD;
            down.Anonymous.ki = KEYBDINPUT {
                wVk: 0,
                wScan: ch,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            };
            inputs.push(down);

            // Key up
            let mut up: INPUT = unsafe { std::mem::zeroed() };
            up.r#type = windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD;
            up.Anonymous.ki = KEYBDINPUT {
                wVk: 0,
                wScan: ch,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            };
            inputs.push(up);
        }

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };

        if sent != inputs.len() as u32 {
            return Err(DeliveryError::SendInputFailed);
        }

        Ok(())
    }

    /// Read plain unicode text from Windows clipboard with retry.
    pub fn get_clipboard_text(&self) -> Option<String> {
        unsafe {
            if !self.open_clipboard_retry(null_mut(), 5) {
                return None;
            }

            if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                CloseClipboard();
                return None;
            }

            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                CloseClipboard();
                return None;
            }

            let ptr = GlobalLock(handle as _) as *const u16;
            if ptr.is_null() {
                CloseClipboard();
                return None;
            }

            // Find null terminator
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }

            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);

            GlobalUnlock(handle as _);
            CloseClipboard();
            Some(text)
        }
    }

    /// Write unicode text to Windows clipboard with optional Win+V exclusion.
    pub fn set_clipboard_text(
        &self,
        text: &str,
        exclude_history: bool,
    ) -> Result<(), DeliveryError> {
        let wide: Vec<u16> = OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let bytes_len = wide.len() * std::mem::size_of::<u16>();

        unsafe {
            if !self.open_clipboard_retry(null_mut(), 6) {
                return Err(DeliveryError::ClipboardLocked);
            }

            EmptyClipboard();

            let h_mem = GlobalAlloc(GMEM_MOVEABLE, bytes_len);
            if h_mem.is_null() {
                CloseClipboard();
                return Err(DeliveryError::MemoryAllocationFailed);
            }

            let p_mem = GlobalLock(h_mem) as *mut u16;
            if p_mem.is_null() {
                CloseClipboard();
                return Err(DeliveryError::MemoryLockFailed);
            }

            std::ptr::copy_nonoverlapping(wide.as_ptr(), p_mem, wide.len());
            GlobalUnlock(h_mem);

            SetClipboardData(CF_UNICODETEXT, h_mem as HANDLE);

            // Windows 11 Win+V history exclusion flag
            if exclude_history && self.exclude_format_id != 0 {
                let h_flag = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>());
                if !h_flag.is_null() {
                    let p_flag = GlobalLock(h_flag) as *mut u32;
                    if !p_flag.is_null() {
                        *p_flag = 1;
                        GlobalUnlock(h_flag);
                        SetClipboardData(self.exclude_format_id, h_flag as HANDLE);
                    }
                }
            }

            CloseClipboard();
        }

        Ok(())
    }

    /// Synthesizes synthetic Ctrl+V key combination via SendInput.
    fn synthesize_ctrl_v(&self) -> Result<(), DeliveryError> {
        const VK_V_CODE: u16 = 0x56;

        let mut inputs = [
            // 1. Ctrl down
            create_key_input(VK_CONTROL, 0),
            // 2. V down
            create_key_input(VK_V_CODE, 0),
            // 3. V up
            create_key_input(VK_V_CODE, KEYEVENTF_KEYUP),
            // 4. Ctrl up
            create_key_input(VK_CONTROL, KEYEVENTF_KEYUP),
        ];

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };

        if sent != inputs.len() as u32 {
            return Err(DeliveryError::SendInputFailed);
        }

        Ok(())
    }

    /// Helper to attempt opening clipboard with backoff retry.
    unsafe fn open_clipboard_retry(&self, hwnd: HWND, max_retries: usize) -> bool {
        for attempt in 0..max_retries {
            if unsafe { OpenClipboard(hwnd) } != 0 {
                return true;
            }
            sleep(Duration::from_millis((5 * (1 << attempt)).min(50)));
        }
        false
    }
}

fn create_key_input(vk: u16, flags: u32) -> INPUT {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_injector_initialization() {
        let injector = ClipboardInjector::new(250);
        assert_eq!(injector.restore_timeout_ms(), 250);
    }

    #[test]
    fn test_clipboard_text_roundtrip() {
        let injector = ClipboardInjector::new(100);
        let original = injector.get_clipboard_text();

        let test_phrase = "VoisuWin_Test_Arbitration_12345";
        let write_res = injector.set_clipboard_text(test_phrase, false);
        if write_res.is_ok() {
            let read_back = injector.get_clipboard_text();
            assert_eq!(read_back.as_deref(), Some(test_phrase));
        }

        // Restore original clipboard so we don't mess up user's clipboard
        if let Some(orig) = original {
            let _ = injector.set_clipboard_text(&orig, false);
        }
    }

    #[test]
    fn test_is_text_box_focused_safety() {
        let injector = ClipboardInjector::new(100);
        // Call should run safely without crash regardless of environment
        let _ = injector.is_text_box_focused();
    }

    #[test]
    fn test_delivery_outcome_types() {
        assert_ne!(
            DeliveryOutcome::Injected,
            DeliveryOutcome::CopiedToClipboard
        );
    }
}
