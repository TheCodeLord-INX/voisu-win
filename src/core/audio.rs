//! WASAPI audio capture engine with sinc resampling to 16kHz mono and memory WAV encoding.
//!
//! Captures hardware input at native sample rate (typically 48kHz), downmixes to mono,
//! applies real-time sinc resampling via `rubato` to 16kHz 16-bit PCM, calculates RMS,
//! and streams `AudioFrame` chunks to consumers (Deepgram) while buffering for batch WAV (Groq).

use crate::core::types::AudioFrame;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig, SupportedStreamConfig};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::io::Cursor;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Target sample rate required by both Deepgram Nova-2 and Groq Whisper.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Standard chunk size emitted to streaming consumers: ~20ms at 16kHz = 320 samples.
pub const TARGET_CHUNK_SIZE_16K: usize = 320;

#[derive(Error, Debug)]
pub enum AudioError {
    #[error("No audio input device found")]
    NoInputDevice,
    #[error("Device query error: {0}")]
    DeviceQuery(#[from] cpal::DevicesError),
    #[error("Unsupported audio format or config: {0}")]
    DefaultConfig(#[from] cpal::DefaultStreamConfigError),
    #[error("Audio stream build error: {0}")]
    StreamBuild(#[from] cpal::BuildStreamError),
    #[error("Audio stream playback/play error: {0}")]
    PlayStream(#[from] cpal::PlayStreamError),
    #[error("Resampler initialization error: {0}")]
    ResamplerInit(String),
    #[error("Resampling processing error: {0}")]
    ResampleProcess(String),
    #[error("WAV encoding error: {0}")]
    WavEncode(#[from] hound::Error),
}

/// Downmix multi-channel interleaved float samples to mono.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    if channels == 0 || interleaved.is_empty() {
        return Vec::new();
    }

    let frames = interleaved.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    let channels_f = channels as f32;

    for frame_idx in 0..frames {
        let base = frame_idx * channels;
        let mut sum = 0.0f32;
        for ch in 0..channels {
            sum += interleaved[base + ch];
        }
        mono.push(sum / channels_f);
    }
    mono
}

/// Convert float samples in range `[-1.0, 1.0]` to 16-bit signed PCM `i16`.
pub fn float_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            if clamped >= 0.0 {
                (clamped * 32767.0) as i16
            } else {
                (clamped * 32768.0) as i16
            }
        })
        .collect()
}

/// Encodes 16kHz 16-bit signed mono PCM samples into an in-memory RIFF WAV buffer.
pub fn encode_to_wav(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &sample in samples {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

/// Real-time sinc resampler converting native hardware rate to 16,000 Hz mono.
pub struct AudioResampler {
    resampler: SincFixedIn<f32>,
    input_chunk_size: usize,
    buffer: Vec<f32>,
}

impl AudioResampler {
    pub fn new(from_rate: u32, to_rate: u32) -> Result<Self, AudioError> {
        let ratio = to_rate as f64 / from_rate as f64;
        // Input chunk size proportional to 20ms at target rate
        let input_chunk_size = ((TARGET_CHUNK_SIZE_16K as f64) / ratio).round() as usize;

        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };

        let resampler = SincFixedIn::<f32>::new(
            ratio,
            2.0,
            params,
            input_chunk_size,
            1, // 1 channel (mono)
        )
        .map_err(|e| AudioError::ResamplerInit(e.to_string()))?;

        Ok(Self {
            resampler,
            input_chunk_size,
            buffer: Vec::with_capacity(input_chunk_size * 2),
        })
    }

    /// Feeds mono float samples at native rate, processes full chunks, and returns resampled 16kHz float samples.
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>, AudioError> {
        self.buffer.extend_from_slice(input);
        let mut output = Vec::new();

        while self.buffer.len() >= self.input_chunk_size {
            let chunk: Vec<f32> = self.buffer.drain(..self.input_chunk_size).collect();
            let wave_in = vec![chunk];
            let resampled = self
                .resampler
                .process(&wave_in, None)
                .map_err(|e| AudioError::ResampleProcess(e.to_string()))?;

            if let Some(channel_out) = resampled.into_iter().next() {
                output.extend(channel_out);
            }
        }

        Ok(output)
    }

    /// Flush remaining buffered audio padding with zeros if necessary.
    pub fn flush(&mut self) -> Result<Vec<f32>, AudioError> {
        if self.buffer.is_empty() {
            return Ok(Vec::new());
        }
        let needed = self.input_chunk_size - self.buffer.len();
        self.buffer.resize(self.input_chunk_size, 0.0);
        let chunk: Vec<f32> = std::mem::take(&mut self.buffer);
        let wave_in = vec![chunk];
        let resampled = self
            .resampler
            .process(&wave_in, None)
            .map_err(|e| AudioError::ResampleProcess(e.to_string()))?;

        let mut output = Vec::new();
        if let Some(channel_out) = resampled.into_iter().next() {
            // Trim padding proportionally
            let valid_len =
                (channel_out.len() * (self.input_chunk_size - needed)) / self.input_chunk_size;
            output.extend_from_slice(&channel_out[..valid_len]);
        }
        Ok(output)
    }
}

/// Active recording session handle to stop recording and retrieve all audio data.
pub struct RecordingSession {
    stream: Stream,
    resampler_handle: Option<std::thread::JoinHandle<()>>,
    sample_accumulator: Arc<std::sync::Mutex<Vec<i16>>>,
    start_instant: Instant,
}

impl RecordingSession {
    /// Stop recording, wait for audio pipeline to flush, and return normalized 16kHz PCM samples and WAV buffer.
    pub fn stop(mut self) -> Result<(Vec<i16>, Vec<u8>), AudioError> {
        // 1. Pause and drop stream so input callback finishes and raw_tx is dropped
        let _ = self.stream.pause();
        // Dropping stream triggers raw_rx channel disconnect in worker thread

        // 2. Wait for resampler worker thread to drain all remaining chunks and flush
        if let Some(handle) = self.resampler_handle.take() {
            let _ = handle.join();
        }

        // 3. Extract accumulated samples
        let mut samples = {
            let guard = self.sample_accumulator.lock().unwrap();
            guard.clone()
        };

        // 4. Auto-Gain Normalization: boost quiet laptop microphones into optimal speech range
        let max_val = samples.iter().map(|&s| s.abs()).max().unwrap_or(0);
        if max_val > 50 && max_val < 16000 {
            let boost = (24000.0 / max_val as f32).min(8.0);
            debug!(
                "Applying auto-gain boost: {:.2}x (peak: {})",
                boost, max_val
            );
            for s in &mut samples {
                *s = (*s as f32 * boost).clamp(-32767.0, 32767.0) as i16;
            }
        }

        let wav_bytes = encode_to_wav(&samples, TARGET_SAMPLE_RATE)?;
        info!(
            "Recording session stopped: {} samples ({}ms), peak: {}, WAV payload: {} bytes",
            samples.len(),
            self.start_instant.elapsed().as_millis(),
            max_val,
            wav_bytes.len()
        );
        Ok((samples, wav_bytes))
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start_instant.elapsed().as_millis() as u64
    }
}

/// Keywords commonly present in external headsets, earphones, and USB/Bluetooth microphones.
pub const EXTERNAL_DEVICE_KEYWORDS: &[&str] = &[
    "headset",
    "earphone",
    "earphones",
    "headphone",
    "headphones",
    "earbud",
    "earbuds",
    "airpod",
    "airpods",
    "galaxy buds",
    "pixel buds",
    "usb",
    "bluetooth",
    "wireless",
    "external",
    "mic in",
    "line in",
    "front mic",
    "jack",
    "3.5mm",
    "hands-free",
    "handsfree",
    "bose",
    "sony",
    "jabra",
    "sennheiser",
    "hyperx",
    "corsair",
    "razer",
    "steelseries",
    "blue yeti",
    "rode",
    "shure",
    "fifine",
    "elgato",
    "samson",
    "focusrite",
];

/// Keywords indicative of built-in laptop microphones.
pub const BUILTIN_DEVICE_KEYWORDS: &[&str] = &[
    "array",
    "internal",
    "integrated",
    "built-in",
    "builtin",
    "intel(r) smart sound",
    "intel® smart sound",
];

/// Scores an audio device name to determine priority.
/// External earphones and headsets rank highest (100 or 90).
/// Built-in microphones rank lower (50 or 10).
pub fn score_audio_device(name: &str, is_default: bool) -> (u32, bool) {
    let lower = name.to_lowercase();
    let is_external = EXTERNAL_DEVICE_KEYWORDS.iter().any(|&k| lower.contains(k));
    let is_builtin = BUILTIN_DEVICE_KEYWORDS.iter().any(|&k| lower.contains(k));

    let score = if is_external {
        if is_default {
            100 // External device AND marked as Windows default
        } else {
            90 // External device plugged in (even if Windows didn't auto-switch default)
        }
    } else if is_default && !is_builtin {
        70 // Default device that isn't explicitly an internal mic array
    } else if is_default {
        50 // System default internal microphone array
    } else if !is_builtin {
        30 // Other available device
    } else {
        10 // Non-default internal device
    };

    (score, is_external)
}

struct ActiveDeviceState {
    name: String,
    device: Arc<Device>,
    config: SupportedStreamConfig,
    is_external: bool,
}

/// Core WASAPI audio capture engine with smart device selection and dynamic hot-plug detection.
pub struct AudioCaptureEngine {
    state: RwLock<ActiveDeviceState>,
    prefer_external_mic: bool,
    device_id_override: Option<String>,
}

impl AudioCaptureEngine {
    /// Resolves the optimal input device according to smart priority and configuration.
    pub fn resolve_best_device(
        host: &cpal::Host,
        device_override: Option<&str>,
        prefer_external: bool,
    ) -> Result<(Device, SupportedStreamConfig, String, bool), AudioError> {
        let default_device = host.default_input_device();
        let default_name = default_device
            .as_ref()
            .and_then(|d| d.name().ok())
            .unwrap_or_default();

        // 1. If explicit device override is provided, search for match
        if let Some(target) = device_override.filter(|t| !t.trim().is_empty()) {
            let target_lower = target.trim().to_lowercase();
            if let Ok(devices) = host.input_devices() {
                for dev in devices {
                    let Ok(name) = dev.name() else { continue };
                    if !name.to_lowercase().contains(&target_lower) {
                        continue;
                    }
                    if let Ok(cfg) = dev.default_input_config() {
                        let (_, is_ext) = score_audio_device(&name, name == default_name);
                        return Ok((dev, cfg, name, is_ext));
                    }
                }
            }
        }

        // 2. If smart external prioritization is enabled, score all enumerated devices
        if prefer_external && let Ok(devices) = host.input_devices() {
            let mut candidates = Vec::new();
            for dev in devices {
                let Ok(name) = dev.name() else { continue };
                let is_def = name == default_name;
                let (score, is_ext) = score_audio_device(&name, is_def);
                candidates.push((score, is_ext, name, dev));
            }

            // Sort descending by score
            candidates.sort_by_key(|c| std::cmp::Reverse(c.0));

            for (score, is_ext, name, dev) in candidates {
                if let Ok(cfg) = dev.default_input_config() {
                    debug!(
                        "Evaluated audio candidate: '{}' (score: {}, external: {})",
                        name, score, is_ext
                    );
                    return Ok((dev, cfg, name, is_ext));
                }
            }
        }

        // 3. Fall back to system default input device
        let dev = default_device.ok_or(AudioError::NoInputDevice)?;
        let name = dev.name().unwrap_or_else(|_| "Default Device".to_string());
        let cfg = dev.default_input_config()?;
        let (_, is_ext) = score_audio_device(&name, true);
        Ok((dev, cfg, name, is_ext))
    }

    /// Initialize audio engine using smart device detection (prioritizing external earphones/microphones).
    pub fn new() -> Result<Self, AudioError> {
        Self::with_options(None, true)
    }

    /// Initialize audio engine with user preferences from AppConfig.
    pub fn with_config(config: &crate::config::AppConfig) -> Result<Self, AudioError> {
        Self::with_options(
            config.audio_device_id.as_deref(),
            config.prefer_external_mic,
        )
    }

    /// Initialize audio engine with specific override and smart prioritization options.
    pub fn with_options(
        device_id_override: Option<&str>,
        prefer_external_mic: bool,
    ) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let (device, config, name, is_external) =
            Self::resolve_best_device(&host, device_id_override, prefer_external_mic)?;

        info!(
            "Audio engine initialized with device: '{}' (external: {}), native sample rate: {} Hz, channels: {}",
            name,
            is_external,
            config.sample_rate().0,
            config.channels()
        );

        Ok(Self {
            state: RwLock::new(ActiveDeviceState {
                name,
                device: Arc::new(device),
                config,
                is_external,
            }),
            prefer_external_mic,
            device_id_override: device_id_override.map(|s| s.to_string()),
        })
    }

    pub fn device_name(&self) -> String {
        self.state.read().unwrap().name.clone()
    }

    pub fn is_external(&self) -> bool {
        self.state.read().unwrap().is_external
    }

    pub fn sample_rate(&self) -> u32 {
        self.state.read().unwrap().config.sample_rate().0
    }

    pub fn channels(&self) -> u16 {
        self.state.read().unwrap().config.channels()
    }

    /// Begin a recording session, emitting real-time 16kHz `AudioFrame` chunks to the returned channel.
    /// Dynamically re-evaluates input devices: if earphones or an external mic was recently plugged in
    /// or unplugged, switches to the optimal device seamlessly without restarting the application.
    pub fn start_session(
        &self,
    ) -> Result<(mpsc::Receiver<AudioFrame>, RecordingSession), AudioError> {
        // Dynamic re-evaluation of audio topology on each session start
        if self.prefer_external_mic {
            let host = cpal::default_host();
            if let Ok((new_dev, new_cfg, new_name, is_ext)) =
                Self::resolve_best_device(&host, self.device_id_override.as_deref(), true)
            {
                let mut guard = self.state.write().unwrap();
                if guard.name != new_name {
                    if is_ext {
                        info!(
                            "Smart Audio: External microphone detected ('{}'). Switching active input.",
                            new_name
                        );
                        println!(
                            "\n[AUDIO] External microphone detected: '{}' (Active)",
                            new_name
                        );
                    } else if guard.is_external {
                        info!(
                            "Smart Audio: External microphone unplugged. Reverting to ('{}').",
                            new_name
                        );
                        println!(
                            "\n[AUDIO] External mic unplugged, reverted to: '{}'",
                            new_name
                        );
                    }
                    guard.name = new_name;
                    guard.device = Arc::new(new_dev);
                    guard.config = new_cfg;
                    guard.is_external = is_ext;
                }
            }
        }

        let (device, channels, native_rate, sample_format, stream_config) = {
            let guard = self.state.read().unwrap();
            let cfg: StreamConfig = guard.config.clone().into();
            (
                Arc::clone(&guard.device),
                guard.config.channels() as usize,
                guard.config.sample_rate().0,
                guard.config.sample_format(),
                cfg,
            )
        };

        let (tx, rx) = mpsc::channel::<AudioFrame>(100);
        let sample_accumulator = Arc::new(std::sync::Mutex::new(Vec::new()));

        let acc = Arc::clone(&sample_accumulator);
        let start_time = Instant::now();

        // Dedicated ring buffer channel between audio thread and resampling worker
        let (raw_tx, raw_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(256);

        // Spawn high-priority resampling worker thread
        let resampler_handle = std::thread::Builder::new()
            .name("voisu-audio-resampler".to_string())
            .spawn(move || {
                let mut resampler = match AudioResampler::new(native_rate, TARGET_SAMPLE_RATE) {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to create sinc resampler: {}", e);
                        return;
                    }
                };

                let mut out_buffer: Vec<i16> = Vec::with_capacity(TARGET_CHUNK_SIZE_16K * 2);

                while let Ok(raw_mono) = raw_rx.recv() {
                    match resampler.process(&raw_mono) {
                        Ok(resampled_floats) => {
                            let i16_samples = float_to_i16(&resampled_floats);
                            out_buffer.extend(i16_samples);

                            // Emit ~20ms chunks to live consumers
                            while out_buffer.len() >= TARGET_CHUNK_SIZE_16K {
                                let chunk: Vec<i16> =
                                    out_buffer.drain(..TARGET_CHUNK_SIZE_16K).collect();
                                let ts_ms = start_time.elapsed().as_millis() as u64;
                                let frame = AudioFrame::new(chunk.clone(), ts_ms);

                                // Append to full session buffer
                                if let Ok(mut guard) = acc.lock() {
                                    guard.extend(chunk);
                                }

                                if tx.blocking_send(frame).is_err() {
                                    // Receiver closed
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Resampling chunk error: {}", e);
                        }
                    }
                }

                // Flush remaining samples at stop
                if let Ok(flushed) = resampler.flush() {
                    let flushed_i16 = float_to_i16(&flushed);
                    if !flushed_i16.is_empty() {
                        let ts_ms = start_time.elapsed().as_millis() as u64;
                        let frame = AudioFrame::new(flushed_i16.clone(), ts_ms);
                        if let Ok(mut guard) = acc.lock() {
                            guard.extend(flushed_i16);
                        }
                        let _ = tx.blocking_send(frame);
                    }
                }
            })
            .expect("Failed to spawn audio resampler worker thread");

        let err_fn = |err| {
            error!("WASAPI stream error: {}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono = downmix_to_mono(data, channels);
                    let _ = raw_tx.try_send(mono);
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let float_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono = downmix_to_mono(&float_data, channels);
                    let _ = raw_tx.try_send(mono);
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let float_data: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 - 32768.0) / 32768.0)
                        .collect();
                    let mono = downmix_to_mono(&float_data, channels);
                    let _ = raw_tx.try_send(mono);
                },
                err_fn,
                None,
            )?,
            _ => {
                return Err(AudioError::StreamBuild(
                    cpal::BuildStreamError::StreamConfigNotSupported,
                ));
            }
        };

        stream.play()?;

        let session = RecordingSession {
            stream,
            resampler_handle: Some(resampler_handle),
            sample_accumulator,
            start_instant: start_time,
        };

        Ok((rx, session))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downmix_mono() {
        let stereo = vec![0.5f32, 0.5, -0.2, 0.4, 1.0, 0.0];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.5).abs() < 1e-5);
        assert!((mono[1] - 0.1).abs() < 1e-5);
        assert!((mono[2] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_float_to_i16_clamping() {
        let input = vec![0.0f32, 1.0, -1.0, 1.5, -1.5];
        let output = float_to_i16(&input);
        assert_eq!(output[0], 0);
        assert_eq!(output[1], 32767);
        assert_eq!(output[2], -32768);
        assert_eq!(output[3], 32767);
        assert_eq!(output[4], -32768);
    }

    #[test]
    fn test_wav_encoding_roundtrip() {
        let samples = vec![0i16, 1000, 2000, -1000, -2000, 0];
        let wav_bytes = encode_to_wav(&samples, 16000).expect("Failed to encode WAV");
        assert!(wav_bytes.len() > 44);

        // Verify with hound reader
        let cursor = Cursor::new(wav_bytes);
        let mut reader = hound::WavReader::new(cursor).expect("Failed to read encoded WAV");
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().bits_per_sample, 16);

        let decoded: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn test_resampler_48k_to_16k_ratio() {
        let mut resampler = AudioResampler::new(48000, 16000).expect("Failed to create resampler");

        // 48,000 samples = 1.0 second of 48kHz audio
        let input_48k = vec![0.1f32; 48000];
        let mut output_16k = resampler.process(&input_48k).expect("Resampling failed");
        let flushed = resampler.flush().expect("Flush failed");
        output_16k.extend(flushed);

        // Expected roughly 16,000 samples (within small sinc filter delay window ±100)
        let diff = (output_16k.len() as isize - 16000).abs();
        assert!(
            diff < 150,
            "Expected ~16000 samples, got {}, diff: {}",
            output_16k.len(),
            diff
        );
    }

    #[test]
    fn test_score_audio_device_earphones_and_headsets() {
        let (score1, is_ext1) = score_audio_device("Headset Microphone (Realtek Audio)", true);
        assert_eq!(score1, 100);
        assert!(is_ext1);

        let (score2, is_ext2) = score_audio_device("USB Audio Device", false);
        assert_eq!(score2, 90);
        assert!(is_ext2);

        let (score3, is_ext3) = score_audio_device("AirPods Hands-Free AG Audio", false);
        assert_eq!(score3, 90);
        assert!(is_ext3);

        let (score4, is_ext4) = score_audio_device("Sony WH-1000XM4 Hands-Free AG Audio", false);
        assert_eq!(score4, 90);
        assert!(is_ext4);
    }

    #[test]
    fn test_score_audio_device_builtin_array() {
        let (score1, is_ext1) = score_audio_device(
            "Microphone Array (2- Intel® Smart Sound Technology for Digital Microphones)",
            true,
        );
        assert_eq!(score1, 50);
        assert!(!is_ext1);

        let (score2, is_ext2) = score_audio_device("Internal Microphone", false);
        assert_eq!(score2, 10);
        assert!(!is_ext2);
    }

    #[test]
    fn test_score_audio_device_ranking_priority() {
        let (score_ext_def, _) = score_audio_device("Headset Microphone", true);
        let (score_ext_non_def, _) = score_audio_device("Headset Microphone", false);
        let (score_generic_def, _) = score_audio_device("Line In Audio", true);
        let (score_builtin_def, _) = score_audio_device("Microphone Array", true);
        let (score_builtin_non_def, _) = score_audio_device("Microphone Array", false);

        assert!(score_ext_def > score_ext_non_def);
        assert!(score_ext_non_def > score_builtin_def);
        assert!(score_generic_def > score_builtin_def);
        assert!(score_builtin_def > score_builtin_non_def);
    }
}
