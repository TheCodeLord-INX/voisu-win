//! Groq Whisper Large v3 REST LPU client.
//!
//! Posts in-memory WAV audio buffers to `https://api.groq.com/openai/v1/audio/transcriptions`.
//! Emits `SourceTranscript` with word-level timestamps and segment-level confidence proxy.
//! Tracks RPM/RPD rate limits returned in response headers.

use crate::core::types::{ProviderId, SourceTranscript, WordToken};
use reqwest::Client;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use thiserror::Error;
use tracing::{debug, warn};

pub const GROQ_TRANSCRIPTION_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

pub const GROQ_WHISPER_MODEL: &str = "whisper-large-v3-turbo";

#[derive(Error, Debug)]
pub enum GroqError {
    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Groq API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("Rate limited by Groq API (429): {message}")]
    RateLimited { message: String },
}

#[derive(Debug, Deserialize)]
pub struct GroqWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Deserialize)]
pub struct GroqSegment {
    pub id: usize,
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default)]
    pub avg_logprob: Option<f64>,
    #[serde(default)]
    pub no_speech_prob: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct GroqVerboseResponse {
    pub text: String,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub segments: Option<Vec<GroqSegment>>,
    #[serde(default)]
    pub words: Option<Vec<GroqWord>>,
}

#[derive(Debug, Default)]
pub struct RateLimitTracker {
    pub remaining_requests: AtomicU32,
    pub remaining_tokens: AtomicU32,
}

pub struct GroqClient {
    api_key: String,
    client: Client,
    pub language: String,
    pub rate_limits: Arc<RateLimitTracker>,
}

impl GroqClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_language(api_key, "en")
    }

    pub fn with_language(api_key: impl Into<String>, language: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_millis(6000))
            .build()
            .unwrap_or_default();

        Self {
            api_key: api_key.into(),
            client,
            language: language.into(),
            rate_limits: Arc::new(RateLimitTracker::default()),
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Transcribe an in-memory WAV buffer using Groq Whisper LPU.
    pub async fn transcribe_wav(&self, wav_bytes: Vec<u8>) -> Result<SourceTranscript, GroqError> {
        let start_time = Instant::now();

        let audio_part = Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| GroqError::Api {
                status: 400,
                message: e.to_string(),
            })?;

        let form = Form::new()
            .part("file", audio_part)
            .text("model", GROQ_WHISPER_MODEL)
            .text("response_format", "verbose_json")
            .text("timestamp_granularities[]", "word")
            .text("language", self.language.clone())
            .text("temperature", "0.0");

        let response = self
            .client
            .post(GROQ_TRANSCRIPTION_URL)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .multipart(form)
            .send()
            .await?;

        let status = response.status();

        // Capture rate limit headers
        if let Some(rem_req) = response
            .headers()
            .get("x-ratelimit-remaining-requests")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
        {
            self.rate_limits
                .remaining_requests
                .store(rem_req, Ordering::Relaxed);
        }

        if let Some(rem_tok) = response
            .headers()
            .get("x-ratelimit-remaining-tokens")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
        {
            self.rate_limits
                .remaining_tokens
                .store(rem_tok, Ordering::Relaxed);
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let body = response.text().await.unwrap_or_default();
            warn!("Groq rate limit exceeded (HTTP 429): {}", body);
            return Err(GroqError::RateLimited { message: body });
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(GroqError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let body_text = response.text().await?;
        let resp_json: GroqVerboseResponse = serde_json::from_str(&body_text).map_err(|e| {
            warn!(
                "Groq JSON parsing error: {}. Raw response: {}",
                e, body_text
            );
            GroqError::Json(e)
        })?;
        let latency_ms = start_time.elapsed().as_millis() as u32;

        debug!(
            "Groq Whisper transcription complete in {}ms (words: {}, segments: {})",
            latency_ms,
            resp_json.words.as_ref().map(|w| w.len()).unwrap_or(0),
            resp_json.segments.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        let transcript = assemble_source_transcript(resp_json, latency_ms);
        Ok(transcript)
    }
}

/// Convert avg_logprob to normalized confidence proxy: e^(avg_logprob)
pub fn logprob_to_confidence(avg_logprob: f64) -> f64 {
    avg_logprob.exp().clamp(0.0, 1.0)
}

/// Returns true if text matches known Whisper silence/outro hallucinations or non-target scripts.
pub fn is_whisper_hallucination(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();

    const ASR_OUTROS: &[&str] = &[
        "thank you for watching",
        "thanks for watching",
        "thank you for watching.",
        "thanks for watching.",
        "thank you.",
        "thank you!",
        "please subscribe",
        "subscribe to my channel",
        "subtitles by",
        "subtitles by the amara.org community",
        "translated by",
        "you",
        "bye",
    ];

    if ASR_OUTROS
        .iter()
        .any(|&o| lower == o || lower == format!("{}.", o))
    {
        return true;
    }

    // Detect non-English Nordic/Icelandic hallucinated characters (þ, ð, æ)
    let has_foreign = trimmed
        .chars()
        .any(|c| matches!(c, 'þ' | 'Þ' | 'ð' | 'Ð' | 'æ' | 'Æ'));
    if has_foreign {
        return true;
    }

    false
}

/// Assembles a `SourceTranscript` from Groq's verbose JSON structure,
/// applying the asymmetric segment-level confidence proxy to all words within that segment.
pub fn assemble_source_transcript(resp: GroqVerboseResponse, latency_ms: u32) -> SourceTranscript {
    if is_whisper_hallucination(&resp.text) {
        debug!("Filtered out Whisper hallucination: '{}'", resp.text);
        return SourceTranscript {
            provider: ProviderId::Groq,
            raw_text: String::new(),
            words: Vec::new(),
            duration_ms: (resp.duration.unwrap_or(0.0) * 1000.0) as u32,
            latency_ms,
        };
    }

    let words_list = resp.words.unwrap_or_default();
    let segments_list = resp.segments.unwrap_or_default();
    let mut words = Vec::with_capacity(words_list.len());

    for w in words_list {
        // Find matching segment by time overlap
        let segment_confidence = segments_list
            .iter()
            .find(|seg| w.start >= seg.start - 0.05 && w.end <= seg.end + 0.05)
            .and_then(|seg| seg.avg_logprob)
            .map(logprob_to_confidence)
            .unwrap_or(0.85); // Fallback confidence proxy if segments missing

        words.push(WordToken {
            word: w.word.trim().to_string(),
            start_ms: (w.start * 1000.0) as u32,
            end_ms: (w.end * 1000.0) as u32,
            confidence: segment_confidence,
            punctuated: Some(w.word),
        });
    }

    SourceTranscript {
        provider: ProviderId::Groq,
        raw_text: resp.text,
        words,
        duration_ms: (resp.duration.unwrap_or(0.0) * 1000.0) as u32,
        latency_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logprob_to_confidence() {
        assert_eq!(logprob_to_confidence(0.0), 1.0);
        let conf = logprob_to_confidence(-0.15);
        assert!((conf - 0.8607).abs() < 0.001);
        let low_conf = logprob_to_confidence(-2.0);
        assert!((low_conf - 0.1353).abs() < 0.001);
    }

    #[test]
    fn test_assemble_groq_transcript() {
        let resp = GroqVerboseResponse {
            text: "Hello world.".to_string(),
            duration: Some(2.45),
            segments: Some(vec![GroqSegment {
                id: 0,
                start: 0.0,
                end: 2.45,
                text: "Hello world.".to_string(),
                avg_logprob: Some(-0.15),
                no_speech_prob: Some(0.01),
            }]),
            words: Some(vec![
                GroqWord {
                    word: "Hello".to_string(),
                    start: 0.12,
                    end: 0.45,
                },
                GroqWord {
                    word: "world.".to_string(),
                    start: 0.48,
                    end: 0.82,
                },
            ]),
        };

        let transcript = assemble_source_transcript(resp, 180);
        assert_eq!(transcript.provider, ProviderId::Groq);
        assert_eq!(transcript.raw_text, "Hello world.");
        assert_eq!(transcript.latency_ms, 180);
        assert_eq!(transcript.words.len(), 2);

        // Word confidence inherits segment avg_logprob proxy
        let expected_conf = logprob_to_confidence(-0.15);
        assert!((transcript.words[0].confidence - expected_conf).abs() < 1e-4);
        assert!((transcript.words[1].confidence - expected_conf).abs() < 1e-4);
        assert_eq!(transcript.words[0].start_ms, 120);
        assert_eq!(transcript.words[1].end_ms, 820);
    }
}
