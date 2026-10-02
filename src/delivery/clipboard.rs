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
use tracing::{debug, warn};
use windows_sys::Win32::Foundation::{HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VK_CONTROL,
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

    /// Primary delivery entry point dispatching either Smart Clipboard or direct SendInput typing.
    pub fn deliver(&self, text: &str, mode: DeliveryMode) -> Result<(), DeliveryError> {
        match mode {
            DeliveryMode::SmartClipboard => self.deliver_smart_clipboard(text),
            DeliveryMode::SendInputUnicode => self.deliver_unicode_direct(text),
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
}
