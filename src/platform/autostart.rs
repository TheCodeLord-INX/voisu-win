//! Windows startup registry manager (`platform::autostart`).
//!
//! Manages automatic startup of `voisu-win` on Windows login using the standard
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` registry key.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use tracing::info;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const REG_APP_NAME: &str = "VoisuDictation";

/// Check if Voisu is currently configured to run on Windows startup.
pub fn is_autostart_enabled() -> bool {
    let subkey_w: Vec<u16> = OsStr::new(RUN_SUBKEY).encode_wide().chain(std::iter::once(0)).collect();
    let name_w: Vec<u16> = OsStr::new(REG_APP_NAME).encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        let mut hkey: HKEY = std::mem::zeroed();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return false;
        }

        let mut val_type = 0u32;
        let mut data_len = 0u32;
        let status = RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            null_mut(),
            &mut val_type,
            null_mut(),
            &mut data_len,
        );

        RegCloseKey(hkey);
        status == 0
    }
}

/// Enable Voisu autostart on Windows login (with `--tray` flag for silent background attachment).
pub fn enable_autostart() -> Result<PathBuf, String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Failed to get current executable path: {}", e))?;

    let cmd_line = format!("\"{}\" run --tray", exe_path.display());
    let subkey_w: Vec<u16> = OsStr::new(RUN_SUBKEY).encode_wide().chain(std::iter::once(0)).collect();
    let name_w: Vec<u16> = OsStr::new(REG_APP_NAME).encode_wide().chain(std::iter::once(0)).collect();
    let val_w: Vec<u16> = OsStr::new(&cmd_line).encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        let mut hkey: HKEY = std::mem::zeroed();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_WRITE, &mut hkey);
        if open_res != 0 {
            return Err(format!("Failed to open registry Run key (error code {})", open_res));
        }

        let byte_len = (val_w.len() * std::mem::size_of::<u16>()) as u32;
        let set_res = RegSetValueExW(
            hkey,
            name_w.as_ptr(),
            0,
            REG_SZ,
            val_w.as_ptr() as *const u8,
            byte_len,
        );

        RegCloseKey(hkey);

        if set_res != 0 {
            return Err(format!("Failed to set registry value (error code {})", set_res));
        }
    }

    info!("Enabled Windows autostart: {}", cmd_line);
    Ok(exe_path)
}

/// Disable Voisu autostart from Windows login.
pub fn disable_autostart() -> Result<(), String> {
    let subkey_w: Vec<u16> = OsStr::new(RUN_SUBKEY).encode_wide().chain(std::iter::once(0)).collect();
    let name_w: Vec<u16> = OsStr::new(REG_APP_NAME).encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        let mut hkey: HKEY = std::mem::zeroed();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_WRITE, &mut hkey);
        if open_res != 0 {
            return Err(format!("Failed to open registry Run key (error code {})", open_res));
        }

        let del_res = RegDeleteValueW(hkey, name_w.as_ptr());
        RegCloseKey(hkey);

        // 2 is ERROR_FILE_NOT_FOUND, which means it wasn't registered anyway (success)
        if del_res != 0 && del_res != 2 {
            return Err(format!("Failed to remove registry value (error code {})", del_res));
        }
    }

    info!("Disabled Windows autostart.");
    Ok(())
}

/// Toggle autostart status and return the new status (true = enabled, false = disabled).
pub fn toggle_autostart() -> Result<bool, String> {
    if is_autostart_enabled() {
        disable_autostart()?;
        Ok(false)
    } else {
        enable_autostart()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_toggle_roundtrip() {
        let initial = is_autostart_enabled();
        // Enable
        let _ = enable_autostart();
        assert!(is_autostart_enabled());
        // Disable
        let _ = disable_autostart();
        assert!(!is_autostart_enabled());
        // Restore initial state
        if initial {
            let _ = enable_autostart();
        }
    }
}
