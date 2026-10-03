//! Dual-Provider Race Coordinator with bounded deadlines and rate-limit guardrails.
//!
//! Concurrently queries Deepgram Nova-2 streaming WebSocket and Groq Whisper LPU REST endpoints,
//! applying an 800ms deadline on post-speech transcription completion and graceful single-provider
//! fallback when one provider times out or approaches API rate limits.

use crate::config::AppConfig;
use crate::core::types::{AudioFrame, SourceTranscript};
use crate::providers::deepgram::{DeepgramClient, DeepgramError};
use crate::providers::groq::GroqClient;
use crate::providers::reconciler::GroqReconciler;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Bounded deadline to wait for both providers to complete after recording stops.
pub const DUAL_PROVIDER_DEADLINE: Duration = Duration::from_millis(800);

/// Minimum Groq remaining requests buffer before temporarily degrading to Deepgram-only.
pub const GROQ_MIN_REQUEST_BUFFER: u32 = 2;

#[derive(Error, Debug)]
pub enum CoordinatorError {
    #[error("No STT providers configured with valid API keys")]
    NoProvidersConfigured,
    #[error(
        "Both STT providers failed or timed out: Deepgram({deepgram_err:?}), Groq({groq_err:?})"
    )]
    BothProvidersFailed {
        deepgram_err: Option<String>,
        groq_err: Option<String>,
    },
}

/// Transcription payload output by the race coordinator.
#[derive(Debug, Clone, PartialEq)]
pub struct DualProviderResult {
    pub deepgram: Option<SourceTranscript>,
    pub groq: Option<SourceTranscript>,
}

impl DualProviderResult {
    pub fn is_empty(&self) -> bool {
        self.deepgram.is_none() && self.groq.is_none()
    }

    pub fn has_both(&self) -> bool {
        self.deepgram.is_some() && self.groq.is_some()
    }
}

pub struct DualProviderCoordinator {
    deepgram_client: Option<Arc<DeepgramClient>>,
    groq_client: Option<Arc<GroqClient>>,
    reconciler: Option<Arc<GroqReconciler>>,
}

impl DualProviderCoordinator {
    pub fn new(config: &AppConfig) -> Self {
        let deepgram_client = config
            .deepgram_api_key
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .map(|k| Arc::new(DeepgramClient::with_language(k, &config.language)));

        let groq_client = config
            .groq_api_key
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .map(|k| Arc::new(GroqClient::with_language(k, &config.language)));

        let reconciler = config
            .groq_api_key
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .map(|k| Arc::new(GroqReconciler::new(k)));

        Self {
            deepgram_client,
            groq_client,
            reconciler,
        }
    }

    pub fn has_deepgram(&self) -> bool {
        self.deepgram_client.is_some()
    }

    pub fn has_groq(&self) -> bool {
        self.groq_client.is_some()
    }

    pub fn reconciler(&self) -> Option<Arc<GroqReconciler>> {
        self.reconciler.clone()
    }

    /// Check if Groq rate limits are nearing exhaustion.
    pub fn should_skip_groq_due_to_rate_limits(&self) -> bool {
        if let Some(groq) = &self.groq_client {
            let rem_requests = groq.rate_limits.remaining_requests.load(Ordering::Relaxed);
            if rem_requests > 0 && rem_requests < GROQ_MIN_REQUEST_BUFFER {
                warn!(
                    "Groq rate limit near capacity ({} remaining). Skipping Groq for this utterance.",
                    rem_requests
                );
                return true;
            }
        }
        false
    }

    /// Spawn real-time Deepgram streaming task as speech begins.
    pub fn start_deepgram_stream(
        &self,
        audio_rx: mpsc::Receiver<AudioFrame>,
    ) -> Option<JoinHandle<Result<SourceTranscript, DeepgramError>>> {
        self.deepgram_client.as_ref().map(|dg| {
            let client = Arc::clone(dg);
            tokio::spawn(async move { client.stream_session(audio_rx).await })
        })
    }

    /// Concurrently executes or resolves Deepgram streaming and Groq batch transcription.
    pub async fn resolve_race(
        &self,
        dg_task: Option<JoinHandle<Result<SourceTranscript, DeepgramError>>>,
        wav_bytes: Vec<u8>,
    ) -> Result<DualProviderResult, CoordinatorError> {
        let has_deepgram = dg_task.is_some();
        let skip_groq = self.should_skip_groq_due_to_rate_limits();
        let has_groq = self.groq_client.is_some() && !skip_groq;

        if !has_deepgram && !has_groq {
            return Err(CoordinatorError::NoProvidersConfigured);
        }

        // Launch Groq batch task
        let groq_task = if has_groq {
            self.groq_client.as_ref().map(|groq| {
                let client = Arc::clone(groq);
                tokio::spawn(async move { client.transcribe_wav(wav_bytes).await })
            })
        } else {
            None
        };

        let mut deepgram_result: Option<SourceTranscript> = None;
        let mut deepgram_err: Option<String> = None;

        let mut groq_result: Option<SourceTranscript> = None;
        let mut groq_err: Option<String> = None;

        let mut dg_fut = std::pin::pin!(async {
            if let Some(handle) = dg_task {
                match handle.await {
                    Ok(Ok(transcript)) => Some(Ok(transcript)),
                    Ok(Err(e)) => {
                        let msg = e.to_string();
                        warn!("Deepgram transcription failed: {}", msg);
                        Some(Err(msg))
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        error!("Deepgram task join error: {}", msg);
                        Some(Err(msg))
                    }
                }
            } else {
                None
            }
        });

        let mut groq_fut = std::pin::pin!(async {
            if let Some(handle) = groq_task {
                match handle.await {
                    Ok(Ok(transcript)) => Some(Ok(transcript)),
                    Ok(Err(e)) => {
                        let msg = e.to_string();
                        warn!("Groq transcription failed: {}", msg);
                        Some(Err(msg))
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        error!("Groq task join error: {}", msg);
                        Some(Err(msg))
                    }
                }
            } else {
                None
            }
        });

        let overall_timeout = tokio::time::sleep(Duration::from_millis(5000));
        let mut overall_timeout = std::pin::pin!(overall_timeout);

        let mut grace_timer: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;

        let mut dg_done = !has_deepgram;
        let mut groq_done = !has_groq;

        while !dg_done || !groq_done {
            tokio::select! {
                res = &mut dg_fut, if !dg_done => {
                    dg_done = true;
                    if let Some(r) = res {
                        match r {
                            Ok(t) => {
                                deepgram_result = Some(t);
                                if grace_timer.is_none() && !groq_done {
                                    grace_timer = Some(Box::pin(tokio::time::sleep(Duration::from_millis(150))));
                                }
                            }
                            Err(e) => deepgram_err = Some(e),
                        }
                    }
                }
                res = &mut groq_fut, if !groq_done => {
                    groq_done = true;
                    if let Some(r) = res {
                        match r {
                            Ok(t) => {
                                groq_result = Some(t);
                                if grace_timer.is_none() && !dg_done {
                                    grace_timer = Some(Box::pin(tokio::time::sleep(Duration::from_millis(150))));
                                }
                            }
                            Err(e) => groq_err = Some(e),
                        }
                    }
                }
                _ = async {
                    if let Some(ref mut timer) = grace_timer {
                        timer.as_mut().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    info!("Dual-provider grace window (600ms) expired. Evaluating available result.");
                    break;
                }
                _ = &mut overall_timeout => {
                    warn!("Overall STT provider deadline (5000ms) expired. Evaluating partial results...");
                    break;
                }
            }
        }

        if deepgram_result.is_none() && groq_result.is_none() {
            return Err(CoordinatorError::BothProvidersFailed {
                deepgram_err,
                groq_err,
            });
        }

        info!(
            "Dual-provider race resolved: Deepgram={}, Groq={}",
            if deepgram_result.is_some() {
                "OK"
            } else {
                "None"
            },
            if groq_result.is_some() { "OK" } else { "None" },
        );

        Ok(DualProviderResult {
            deepgram: deepgram_result,
            groq: groq_result,
        })
    }

    /// Convenience wrapper starting Deepgram stream and running race upon receiving WAV bytes.
    pub async fn execute_race(
        &self,
        audio_rx: mpsc::Receiver<AudioFrame>,
        wav_bytes: Vec<u8>,
    ) -> Result<DualProviderResult, CoordinatorError> {
        let dg_task = self.start_deepgram_stream(audio_rx);
        self.resolve_race(dg_task, wav_bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_dual_provider_result() {
        let res = DualProviderResult {
            deepgram: None,
            groq: None,
        };
        assert!(res.is_empty());
        assert!(!res.has_both());
    }

    #[test]
    fn test_unconfigured_coordinator() {
        let config = AppConfig::default();
        let coordinator = DualProviderCoordinator::new(&config);
        assert!(!coordinator.has_deepgram());
        assert!(!coordinator.has_groq());
        assert!(!coordinator.should_skip_groq_due_to_rate_limits());
    }
}
