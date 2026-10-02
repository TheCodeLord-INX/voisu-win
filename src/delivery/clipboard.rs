//! Windows Smart Clipboard Injector with prior content restoration.

pub struct ClipboardInjector {
    restore_timeout_ms: u32,
}

impl ClipboardInjector {
    pub fn new(restore_timeout_ms: u32) -> Self {
        Self { restore_timeout_ms }
    }

    pub fn restore_timeout_ms(&self) -> u32 {
        self.restore_timeout_ms
    }
}
