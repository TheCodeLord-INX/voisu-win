//! Low-level Windows keyboard hook module.
//!
//! Captures CapsLock (or configured hotkey) on a dedicated Win32 message-pump thread.
//! Implements Hybrid Mode (Tap-to-Toggle or Hold-to-Talk) and suppresses CapsLock state changes.

use crate::core::types::TriggerKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    StartRecording,
    StopRecording,
}

pub struct HotkeyManager {
    trigger_key: TriggerKey,
}

impl HotkeyManager {
    pub fn new(trigger_key: TriggerKey) -> Self {
        Self { trigger_key }
    }

    pub fn trigger_key(&self) -> TriggerKey {
        self.trigger_key
    }
}
