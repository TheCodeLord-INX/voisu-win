//! Groq LPU Fast Transcript Reconciler
//!
//! Powered by `qwen/qwen3.8-27b` with `reasoning_effort: "none"` on Groq LPUs (~400ms).
//! Semantically reconciles phonetic disagreements, proper nouns, acronyms, and code-switched
//! Hinglish between Deepgram and Groq Whisper without manual dictionaries.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub const GROQ_CHAT_COMPLETIONS_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
pub const DEFAULT_RECONCILIATION_MODEL: &str = "qwen/qwen3.8-27b";
pub const RECONCILIATION_TIMEOUT_MS: u64 = 1500;

#[derive(Debug, thiserror::Error)]
pub enum ReconcilerError {
    #[error("Network error communicating with Groq LPU: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Groq API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("Reconciliation timed out after {0}ms")]
    Timeout(u64),
    #[error("Groq response omitted message content")]
    EmptyResponse,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

/// Reconciled transcript result with execution metrics.
#[derive(Debug, Clone)]
pub struct ReconciledResult {
    pub text: String,
    pub latency_ms: u64,
    pub model: String,
}

#[derive(Clone)]
pub struct GroqReconciler {
    api_key: String,
    client: Client,
    model: String,
}

impl GroqReconciler {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_model(api_key, DEFAULT_RECONCILIATION_MODEL)
    }

    pub fn with_model(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(RECONCILIATION_TIMEOUT_MS))
            .build()
            .unwrap_or_default();

        Self {
            api_key: api_key.into(),
            client,
            model: model.into(),
        }
    }

    /// Reconciles two ASR source transcripts using ultra-fast Groq LPU inference.
    pub async fn reconcile(
        &self,
        deepgram_text: &str,
        groq_text: &str,
    ) -> Result<ReconciledResult, ReconcilerError> {
        let start_time = Instant::now();
        let prompt_task = format!(
            "Deepgram: {}\nGroq Whisper: {}",
            deepgram_text.trim(),
            groq_text.trim()
        );

        let system_prompt = "You are Voisu's real-time transcript reconciliation model. Given dual ASR inputs (Deepgram and Whisper), output the single faithful, correct transcript. Specialize in English, Indian English, names, and conversational Hinglish written in Latin script (e.g. 'kya haal hai', 'theek hai', 'bilkul', 'yaar', 'bhai', 'IIT', 'BS'). Never invent facts or output Devanagari script. Return ONLY the final text without quotes, markdown, or explanations.";

        let req_body = ChatCompletionRequest {
            model: self.model.clone(),
            temperature: 0.0,
            reasoning_effort: if self.model == DEFAULT_RECONCILIATION_MODEL {
                Some("none".to_string())
            } else {
                None
            },
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: prompt_task,
                },
            ],
        };

        let response = self
            .client
            .post(GROQ_CHAT_COMPLETIONS_URL)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .json(&req_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("Groq LPU reconciliation API error ({}): {}", status, body);
            return Err(ReconcilerError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let resp_json: ChatCompletionResponse = response.json().await?;
        let content = resp_json
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .ok_or(ReconcilerError::EmptyResponse)?;

        // Strip any surrounding quotes or markdown formatting if returned
        let clean_content = content
            .trim_matches('"')
            .trim_matches('\'')
            .trim_matches('`')
            .trim()
            .to_string();

        let latency_ms = start_time.elapsed().as_millis() as u64;
        info!(
            "Groq LPU reconciliation finished in {}ms: '{}'",
            latency_ms, clean_content
        );

        Ok(ReconciledResult {
            text: clean_content,
            latency_ms,
            model: self.model.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_completion_request_serialization() {
        let req = ChatCompletionRequest {
            model: DEFAULT_RECONCILIATION_MODEL.to_string(),
            temperature: 0.0,
            reasoning_effort: Some("none".to_string()),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "test system".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: "Deepgram: hello\nGroq Whisper: hello".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&req).expect("should serialize");
        assert!(json.contains("\"reasoning_effort\":\"none\""));
        assert!(json.contains("qwen3.8-27b"));
    }

    #[test]
    fn test_chat_completion_response_deserialization() {
        let raw_json = r#"{
            "id": "chatcmpl-test",
            "choices": [
                {
                    "message": {
                        "content": "\"My name is Aditya Wadia.\""
                    }
                }
            ]
        }"#;

        let resp: ChatCompletionResponse =
            serde_json::from_str(raw_json).expect("should deserialize");
        assert_eq!(resp.choices.len(), 1);
        let cleaned = resp.choices[0].message.content.trim_matches('"').trim();
        assert_eq!(cleaned, "My name is Aditya Wadia.");
    }
}
