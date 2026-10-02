use serde::{Deserialize, Serialize};

/// Indicates which STT provider generated a transcript or served as the primary source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    Deepgram,
    Groq,
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deepgram => write!(f, "Deepgram"),
            Self::Groq => write!(f, "Groq"),
        }
    }
}

/// Indicates which provider combination was used during transcript arbitration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbitrationMode {
    DualProvider,
    SingleDeepgram,
    SingleGroq,
}

impl std::fmt::Display for ArbitrationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DualProvider => write!(f, "DualProvider"),
            Self::SingleDeepgram => write!(f, "SingleDeepgram"),
            Self::SingleGroq => write!(f, "SingleGroq"),
        }
    }
}

/// Individual transcribed token with timing and confidence metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordToken {
    /// Normalized or verbatim token string.
    pub word: String,
    /// Start time offset in milliseconds relative to utterance start.
    pub start_ms: u32,
    /// End time offset in milliseconds relative to utterance start.
    pub end_ms: u32,
    /// Normalized confidence score between 0.0 and 1.0.
    pub confidence: f64,
    /// Verbatim word including trailing punctuation if provided.
    pub punctuated: Option<String>,
}

impl WordToken {
    pub fn new(word: impl Into<String>, start_ms: u32, end_ms: u32, confidence: f64) -> Self {
        Self {
            word: word.into(),
            start_ms,
            end_ms,
            confidence: confidence.clamp(0.0, 1.0),
            punctuated: None,
        }
    }

    pub fn with_punctuation(mut self, punctuated: impl Into<String>) -> Self {
        self.punctuated = Some(punctuated.into());
        self
    }
}

/// Complete transcription payload emitted by an individual STT provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceTranscript {
    /// Provider that produced this transcript.
    pub provider: ProviderId,
    /// Full raw concatenated text.
    pub raw_text: String,
    /// Positional sequence of recognized words with confidence.
    pub words: Vec<WordToken>,
    /// Total audio duration processed in milliseconds.
    pub duration_ms: u32,
    /// Total elapsed time from recording stop until transcript availability (ms).
    pub latency_ms: u32,
}

/// Log of a substitution made during arbitration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlippedRegion {
    pub start_index: usize,
    pub original_tokens: Vec<WordToken>,
    pub replacement_tokens: Vec<WordToken>,
    pub confidence_delta: f64,
    pub arbitration_reason: String,
}

/// Result of divergence alignment and confidence arbitration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArbitratedTranscript {
    /// Final assembled transcript text.
    pub selected_text: String,
    /// The incumbent backbone provider.
    pub primary_source: ProviderId,
    /// Which provider combination was used during arbitration.
    pub arbitration_mode: ArbitrationMode,
    /// Detailed log of word substitutions made.
    pub flipped_regions: Vec<FlippedRegion>,
    /// Invariant check confirming all tokens originated from source transcripts.
    pub is_source_derived: bool,
}

/// Resampled PCM audio slice produced by the audio capture pipeline.
/// 16-bit signed PCM samples at 16,000 Hz, mono.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// 16-bit signed PCM samples at 16,000 Hz, mono.
    pub samples: Vec<i16>,
    /// Monotonic millisecond timestamp from recording start.
    pub timestamp_ms: u64,
    /// Root-mean-square amplitude normalized between 0.0 and 1.0.
    pub rms_level: f32,
}

impl AudioFrame {
    pub fn new(samples: Vec<i16>, timestamp_ms: u64) -> Self {
        let rms_level = Self::calculate_rms(&samples);
        Self {
            samples,
            timestamp_ms,
            rms_level,
        }
    }

    /// Calculate RMS normalized level between 0.0 and 1.0.
    pub fn calculate_rms(samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f64 = samples
            .iter()
            .map(|&s| {
                let norm = s as f64 / 32768.0;
                norm * norm
            })
            .sum();
        let mean_sq = sum_sq / (samples.len() as f64);
        (mean_sq.sqrt() as f32).clamp(0.0, 1.0)
    }
}

/// Hotkey trigger selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TriggerKey {
    #[default]
    CapsLock,
    RightAlt,
    F8,
    Custom(u32),
}

/// Interaction mode for triggering speech recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum InteractionMode {
    #[default]
    Hybrid,
    PushToTalk,
    Toggle,
}

/// Method used to deliver the final text to the target focused application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DeliveryMode {
    #[default]
    SmartClipboard,
    SendInputUnicode,
}

/// Policy for Deep Punctuation & Formatting (DPR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DprPolicy {
    #[default]
    Adaptive,
    Natural,
    Structured,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_silence() {
        let samples = vec![0i16; 320];
        let rms = AudioFrame::calculate_rms(&samples);
        assert_eq!(rms, 0.0);
    }

    #[test]
    fn test_rms_max_amplitude() {
        let samples = vec![32767i16; 320];
        let rms = AudioFrame::calculate_rms(&samples);
        assert!((rms - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_word_token_confidence_clamp() {
        let token_high = WordToken::new("test", 0, 100, 1.5);
        assert_eq!(token_high.confidence, 1.0);

        let token_low = WordToken::new("test", 0, 100, -0.5);
        assert_eq!(token_low.confidence, 0.0);
    }

    #[test]
    fn test_arbitrated_transcript_serialization() {
        let transcript = ArbitratedTranscript {
            selected_text: "Hello world.".to_string(),
            primary_source: ProviderId::Deepgram,
            arbitration_mode: ArbitrationMode::DualProvider,
            flipped_regions: vec![FlippedRegion {
                start_index: 0,
                original_tokens: vec![WordToken::new("hello", 0, 100, 0.4)],
                replacement_tokens: vec![WordToken::new("Hello", 0, 100, 0.9)],
                confidence_delta: 0.5,
                arbitration_reason: "Higher confidence".to_string(),
            }],
            is_source_derived: true,
        };

        let json = serde_json::to_string(&transcript).unwrap();
        let decoded: ArbitratedTranscript = serde_json::from_str(&json).unwrap();
        assert_eq!(transcript, decoded);
    }
}
