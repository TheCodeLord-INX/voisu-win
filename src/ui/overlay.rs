//! Win32 layered acrylic floating pill overlay.

pub struct OverlayController;

impl OverlayController {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OverlayController {
    fn default() -> Self {
        Self::new()
    }
}
