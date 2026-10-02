//! WASAPI audio capture engine with sinc resampling to 16kHz mono.

pub struct AudioEngineConfig {
    pub target_sample_rate: u32,
    pub channels: u16,
    pub native_sample_rate_override: Option<u32>,
}

impl Default for AudioEngineConfig {
    fn default() -> Self {
        Self {
            target_sample_rate: 16_000,
            channels: 1,
            native_sample_rate_override: None,
        }
    }
}

pub struct AudioCaptureEngine {
    config: AudioEngineConfig,
}

impl AudioCaptureEngine {
    pub fn new(config: AudioEngineConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &AudioEngineConfig {
        &self.config
    }
}
