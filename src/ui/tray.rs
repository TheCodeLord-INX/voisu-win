//! Windows system tray icon and context menu.

pub struct TrayManager;

impl TrayManager {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TrayManager {
    fn default() -> Self {
        Self::new()
    }
}
