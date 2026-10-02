//! Deepgram Nova-2 streaming WebSocket client.
//!
//! Streams real-time 16kHz 16-bit linear PCM frames over `wss://api.deepgram.com/v1/listen`.
//! Emits incremental and final `SourceTranscript` messages with word-level timestamps and confidence scores.

use crate::core::types::{AudioFrame, ProviderId, SourceTranscript, WordToken};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::time::Instant;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{debug, warn};

pub const DEEPGRAM_WS_URL: &str = "wss://api.deepgram.com/v1/listen?model=nova-2&smart_format=true&encoding=linear16&sample_rate=16000&channels=1&endpointing=300&utterance_end_ms=1000&interim_results=true";

#[derive(Error, Debug)]
pub enum DeepgramError {
    #[error("WebSocket connection failed: {0}")]
    Connection(Box<tokio_tungstenite::tungstenite::Error>),
    #[error("HTTP header or URI parse error: {0}")]
    Http(#[from] tokio_tungstenite::tungstenite::http::Error),
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Channel communication error: {0}")]
    Channel(String),
    #[error("Timeout waiting for transcription results")]
    Timeout,
}

impl From<tokio_tungstenite::tungstenite::Error> for DeepgramError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Connection(Box::new(err))
    }
}

#[derive(Debug, Deserialize)]
pub struct DeepgramWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f64,
    #[serde(default)]
    pub punctuated_word: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeepgramAlternative {
    pub transcript: String,
    pub confidence: f64,
    #[serde(default)]
    pub words: Vec<DeepgramWord>,
}

#[derive(Debug, Deserialize)]
pub struct DeepgramChannel {
    #[serde(default)]
    pub alternatives: Vec<DeepgramAlternative>,
}

#[derive(Debug, Deserialize)]
pub struct DeepgramResponse {
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub speech_final: bool,
    #[serde(default)]
    pub duration: Option<f64>,
    pub channel: Option<DeepgramChannel>,
}

pub struct DeepgramClient {
    api_key: String,
}

impl DeepgramClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Establish streaming WebSocket connection to Deepgram Nova-2.
    pub async fn connect(
        &self,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, DeepgramError> {
        let uri = DEEPGRAM_WS_URL;
        let request = Request::builder()
            .uri(uri)
            .header("Host", "api.deepgram.com")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Authorization", format!("Token {}", self.api_key.trim()))
            .body(())?;

        let (ws_stream, response) = connect_async(request).await?;
        debug!(
            "Deepgram WebSocket connected successfully (Status: {})",
            response.status()
        );
        Ok(ws_stream)
    }

    /// Streams audio frames from `audio_rx` to Deepgram and waits for final `SourceTranscript`.
    pub async fn stream_session(
        &self,
        mut audio_rx: mpsc::Receiver<AudioFrame>,
    ) -> Result<SourceTranscript, DeepgramError> {
        let start_time = Instant::now();
        let ws_stream = self.connect().await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        // 1. Task to send binary PCM audio frames to Deepgram
        let send_task = tokio::spawn(async move {
            let mut total_samples = 0usize;
            while let Some(frame) = audio_rx.recv().await {
                total_samples += frame.samples.len();
                // Convert Vec<i16> into little-endian byte array
                let mut bytes = Vec::with_capacity(frame.samples.len() * 2);
                for sample in frame.samples {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
                if let Err(e) = ws_tx.send(Message::Binary(bytes.into())).await {
                    warn!("Error sending audio frame to Deepgram: {}", e);
                    break;
                }
            }

            // Send close stream message to notify Deepgram audio is finished
            let close_msg = serde_json::json!({ "type": "CloseStream" }).to_string();
            let _ = ws_tx.send(Message::Text(close_msg.into())).await;
            total_samples
        });

        // 2. Collect and assemble transcription results
        let mut final_transcript = String::new();
        let mut all_words = Vec::new();
        let mut total_duration_sec = 0.0;

        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(resp) = serde_json::from_str::<DeepgramResponse>(&text) {
                        if resp.msg_type.as_deref() == Some("Metadata") {
                            debug!("Deepgram Metadata received. Session complete.");
                            break;
                        }
                        if let Some(dur) = resp.duration {
                            total_duration_sec = dur;
                        }
                        if let Some(channel) = resp.channel {
                            for alt in channel.alternatives {
                                if resp.is_final && !alt.transcript.is_empty() {
                                    if !final_transcript.is_empty() {
                                        final_transcript.push(' ');
                                    }
                                    final_transcript.push_str(alt.transcript.trim());

                                    for w in alt.words {
                                        all_words.push(WordToken {
                                            word: w.word,
                                            start_ms: (w.start * 1000.0) as u32,
                                            end_ms: (w.end * 1000.0) as u32,
                                            confidence: w.confidence,
                                            punctuated: w.punctuated_word,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    debug!("Deepgram WebSocket closed by server.");
                    break;
                }
                Err(e) => {
                    warn!("Deepgram WebSocket receive error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        let _ = send_task.await;
        let latency_ms = start_time.elapsed().as_millis() as u32;

        Ok(SourceTranscript {
            provider: ProviderId::Deepgram,
            raw_text: final_transcript,
            words: all_words,
            duration_ms: (total_duration_sec * 1000.0) as u32,
            latency_ms,
        })
    }
}

/// Helper function to parse Deepgram JSON message for testing and arbitration.
pub fn parse_deepgram_json(json_str: &str) -> Result<Option<SourceTranscript>, DeepgramError> {
    let resp: DeepgramResponse = serde_json::from_str(json_str)?;
    let Some(channel) = resp.channel else {
        return Ok(None);
    };
    let Some(alt) = channel.alternatives.into_iter().next() else {
        return Ok(None);
    };

    let words = alt
        .words
        .into_iter()
        .map(|w| WordToken {
            word: w.word,
            start_ms: (w.start * 1000.0) as u32,
            end_ms: (w.end * 1000.0) as u32,
            confidence: w.confidence,
            punctuated: w.punctuated_word,
        })
        .collect();

    Ok(Some(SourceTranscript {
        provider: ProviderId::Deepgram,
        raw_text: alt.transcript,
        words,
        duration_ms: (resp.duration.unwrap_or(0.0) * 1000.0) as u32,
        latency_ms: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_deepgram_response() {
        let json_data = r#"{
            "type": "Results",
            "channel_index": [0, 1],
            "duration": 2.45,
            "start": 0.0,
            "is_final": true,
            "speech_final": true,
            "channel": {
                "alternatives": [
                    {
                        "transcript": "Hello world.",
                        "confidence": 0.982,
                        "words": [
                            { "word": "hello", "start": 0.12, "end": 0.45, "confidence": 0.985, "punctuated_word": "Hello" },
                            { "word": "world", "start": 0.48, "end": 0.82, "confidence": 0.978, "punctuated_word": "world." }
                        ]
                    }
                ]
            }
        }"#;

        let parsed = parse_deepgram_json(json_data)
            .unwrap()
            .expect("Expected transcript");
        assert_eq!(parsed.provider, ProviderId::Deepgram);
        assert_eq!(parsed.raw_text, "Hello world.");
        assert_eq!(parsed.words.len(), 2);
        assert_eq!(parsed.words[0].word, "hello");
        assert_eq!(parsed.words[0].punctuated.as_deref(), Some("Hello"));
        assert!((parsed.words[0].confidence - 0.985).abs() < 1e-4);
        assert_eq!(parsed.words[1].word, "world");
        assert_eq!(parsed.words[1].punctuated.as_deref(), Some("world."));
        assert_eq!(parsed.duration_ms, 2450);
    }
}
