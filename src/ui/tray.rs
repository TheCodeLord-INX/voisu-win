//! Windows system tray notification icon and context menu.
//!
//! Provides an unobtrusive presence in the Windows taskbar system tray with:
//! - Visual icon and status tooltip
//! - Right-click context menu (Doctor diagnostics, Config folder, Exit)
//! - Clean removal on shutdown via `Shell_NotifyIconW(NIM_DELETE, ...)`

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use tracing::{info, warn};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, HMENU, IDI_APPLICATION, LoadIconW,
    MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_DESTROY, WM_LBUTTONDBLCLK, WM_RBUTTONUP, WM_USER,
    WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAYICON: u32 = WM_USER + 101;
const TRAY_ICON_ID: u32 = 1;

// Menu Command IDs
const CMD_TITLE: usize = 0;
const CMD_AUTOSTART: usize = 100;
const CMD_TOGGLE_CONSOLE: usize = 101;
const CMD_DOCTOR: usize = 102;
const CMD_CONFIG: usize = 103;
const CMD_ABOUT: usize = 104;
const CMD_EXIT: usize = 105;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    ToggleAutostart,
    ToggleConsole,
    RunDoctor,
    OpenConfig,
    About,
    Exit,
}

enum TrayCommand {
    UpdateTooltip(String),
    Close,
}

pub struct TrayManager {
    cmd_tx: std_mpsc::Sender<TrayCommand>,
    event_rx: std::sync::Mutex<std_mpsc::Receiver<TrayEvent>>,
    is_alive: Arc<AtomicBool>,
}

impl TrayManager {
    /// Initializes and starts the system tray notification icon on a dedicated Win32 message loop thread.
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = std_mpsc::channel();
        let (event_tx, event_rx) = std_mpsc::channel();
        let is_alive = Arc::new(AtomicBool::new(true));
        let alive_clone = Arc::clone(&is_alive);

        std::thread::Builder::new()
            .name("voisu-tray-thread".to_string())
            .spawn(move || {
                run_tray_thread(cmd_rx, event_tx, alive_clone);
            })
            .expect("Failed to spawn system tray thread");

        Self {
            cmd_tx,
            event_rx: std::sync::Mutex::new(event_rx),
            is_alive,
        }
    }

    /// Try to receive any pending tray event without blocking.
    pub fn try_recv_event(&self) -> Option<TrayEvent> {
        self.event_rx.lock().ok()?.try_recv().ok()
    }

    /// Update the tooltip text displayed when hovering over the tray icon.
    pub fn set_tooltip(&self, text: &str) {
        let _ = self
            .cmd_tx
            .send(TrayCommand::UpdateTooltip(text.to_string()));
    }

    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }

    /// Cleanly remove the notification icon and terminate the message loop.
    pub fn close(&self) {
        let _ = self.cmd_tx.send(TrayCommand::Close);
    }
}

impl Default for TrayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        self.close();
    }
}

// Global thread-local or static state for the tray window proc
static EVENT_SENDER: std::sync::Mutex<Option<std_mpsc::Sender<TrayEvent>>> =
    std::sync::Mutex::new(None);

fn run_tray_thread(
    cmd_rx: std_mpsc::Receiver<TrayCommand>,
    event_tx: std_mpsc::Sender<TrayEvent>,
    is_alive: Arc<AtomicBool>,
) {
    unsafe {
        let _ = windows_sys::Win32::System::Com::CoInitializeEx(
            null_mut(),
            windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
        );
    }

    if let Ok(mut guard) = EVENT_SENDER.lock() {
        *guard = Some(event_tx);
    }

    let class_name: Vec<u16> = OsStr::new("VoisuWinTrayMessageClass")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let hwnd = unsafe {
        let h_instance = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(null_mut());
        let wnd_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(tray_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wnd_class);

        let window_title: Vec<u16> = OsStr::new("Voisu Tray Message Receiver")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Top-level hidden window (parent must be NULL for Shell_NotifyIcon to deliver messages)
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut() as HMENU,
            h_instance,
            null_mut(),
        )
    };

    if hwnd.is_null() {
        warn!("Failed to create system tray message receiver HWND.");
        is_alive.store(false, Ordering::Relaxed);
        return;
    }

    // Load default application icon
    let hicon: HICON = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };

    // Register tray icon
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ICON_ID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = hicon;

    let tip_text = "Voisu - Ultra-Fast STT Dictation (Caps Lock)";
    encode_tip(&mut nid.szTip, tip_text);

    let success = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
    if success == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        eprintln!("[TRAY ERROR] Shell_NotifyIconW failed! GetLastError: {}, cbSize: {}, hwnd: {:?}, hicon: {:?}", err, nid.cbSize, hwnd, hicon);
        warn!("Failed to add icon to system tray via Shell_NotifyIconW (err: {}).", err);
    } else {
        info!("System tray notification icon active.");
    }

    // Spawn command processing thread for incoming tooltip updates or exit
    let hwnd_val = hwnd as usize;
    std::thread::spawn(move || {
        let hwnd = hwnd_val as HWND;
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                TrayCommand::UpdateTooltip(text) => unsafe {
                    let mut update_nid: NOTIFYICONDATAW = std::mem::zeroed();
                    update_nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                    update_nid.hWnd = hwnd;
                    update_nid.uID = TRAY_ICON_ID;
                    update_nid.uFlags = NIF_TIP;
                    encode_tip(&mut update_nid.szTip, &text);
                    Shell_NotifyIconW(NIM_MODIFY, &update_nid);
                },
                TrayCommand::Close => unsafe {
                    let mut del_nid: NOTIFYICONDATAW = std::mem::zeroed();
                    del_nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                    del_nid.hWnd = hwnd;
                    del_nid.uID = TRAY_ICON_ID;
                    Shell_NotifyIconW(NIM_DELETE, &del_nid);
                    DestroyWindow(hwnd);
                    break;
                },
            }
        }
    });

    // Run Win32 message pump for tray notifications
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }

        // Ensure icon is deleted when loop terminates
        let mut del_nid: NOTIFYICONDATAW = std::mem::zeroed();
        del_nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        del_nid.hWnd = hwnd;
        del_nid.uID = TRAY_ICON_ID;
        Shell_NotifyIconW(NIM_DELETE, &del_nid);

        windows_sys::Win32::System::Com::CoUninitialize();
    }

    is_alive.store(false, Ordering::Relaxed);
}

fn encode_tip(sz_tip: &mut [u16; 128], text: &str) {
    let wide: Vec<u16> = OsStr::new(text).encode_wide().collect();
    let len = wide.len().min(127);
    sz_tip[..len].copy_from_slice(&wide[..len]);
    sz_tip[len] = 0;
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAYICON => {
                let event = l_param as u32;
                match event {
                    WM_RBUTTONUP => {
                        show_context_menu(hwnd);
                        0
                    }
                    WM_LBUTTONDBLCLK => {
                        // Double-click triggers diagnostics by default
                        if let Some(tx) = EVENT_SENDER.lock().ok().and_then(|g| g.clone()) {
                            let _ = tx.send(TrayEvent::RunDoctor);
                        }
                        0
                    }
                    _ => 0,
                }
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }
}

fn show_context_menu(hwnd: HWND) {
    let cmd = unsafe {
        let menu: HMENU = CreatePopupMenu();
        if menu.is_null() {
            return;
        }

        // Helper to append a wide string menu item
        let append_item = |hmenu: HMENU, id: usize, text: &str, flags: u32| {
            let wide: Vec<u16> = OsStr::new(text)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            AppendMenuW(hmenu, flags | MF_STRING, id, wide.as_ptr());
        };

        // Title (disabled item)
        append_item(
            menu,
            CMD_TITLE,
            "Voisu Dictation v0.1.0 (Active)",
            MF_DISABLED | MF_GRAYED,
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());

        // Autostart toggle with live checkmark
        let autostart_enabled = crate::platform::autostart::is_autostart_enabled();
        let autostart_flags = if autostart_enabled {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        append_item(
            menu,
            CMD_AUTOSTART,
            "&Start with Windows",
            autostart_flags,
        );

        append_item(
            menu,
            CMD_TOGGLE_CONSOLE,
            "Show / Hide &Console Window",
            MF_STRING,
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());

        // Action items
        append_item(
            menu,
            CMD_DOCTOR,
            "&Run System Diagnostics (Doctor)",
            MF_STRING,
        );
        append_item(menu, CMD_CONFIG, "&Open Configuration Folder", MF_STRING);
        append_item(menu, CMD_ABOUT, "&About Voisu", MF_STRING);
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
        append_item(menu, CMD_EXIT, "E&xit Voisu", MF_STRING);

        let mut cursor: POINT = std::mem::zeroed();
        GetCursorPos(&mut cursor);

        // Required Win32 pattern for popup menu to dismiss when clicking away
        SetForegroundWindow(hwnd);

        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            cursor.x,
            cursor.y,
            0,
            hwnd,
            null_mut(),
        );

        DestroyMenu(menu);
        cmd
    };

    if cmd > 0
        && let Some(tx) = EVENT_SENDER.lock().ok().and_then(|g| g.clone())
    {
        match cmd as usize {
            CMD_AUTOSTART => {
                let _ = tx.send(TrayEvent::ToggleAutostart);
            }
            CMD_TOGGLE_CONSOLE => {
                let _ = tx.send(TrayEvent::ToggleConsole);
            }
            CMD_DOCTOR => {
                let _ = tx.send(TrayEvent::RunDoctor);
            }
            CMD_CONFIG => {
                let _ = tx.send(TrayEvent::OpenConfig);
            }
            CMD_ABOUT => {
                let _ = tx.send(TrayEvent::About);
            }
            CMD_EXIT => {
                let _ = tx.send(TrayEvent::Exit);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_tray_manager_lifecycle() {
        let tray = TrayManager::new();
        assert!(tray.is_alive());

        tray.set_tooltip("Testing Voisu Tray...");
        std::thread::sleep(Duration::from_millis(100));

        let event = tray.try_recv_event();
        assert!(event.is_none());

        tray.close();
    }
}
