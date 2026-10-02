//! Positional Levenshtein alignment and asymmetric confidence arbitration engine (Slice B4).

use crate::core::types::{ArbitratedTranscript, SourceTranscript};

pub struct ArbitrationEngine;

impl ArbitrationEngine {
    pub fn arbitrate_single(transcript: SourceTranscript) -> ArbitratedTranscript {
        let is_deepgram = transcript.provider == crate::core::types::ProviderId::Deepgram;
        ArbitratedTranscript {
            selected_text: transcript.raw_text,
            primary_source: transcript.provider,
            arbitration_mode: if is_deepgram {
                crate::core::types::ArbitrationMode::SingleDeepgram
            } else {
                crate::core::types::ArbitrationMode::SingleGroq
            },
            flipped_regions: Vec::new(),
            is_source_derived: true,
        }
    }
}
