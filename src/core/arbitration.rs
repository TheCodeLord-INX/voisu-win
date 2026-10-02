//! Positional Levenshtein alignment and asymmetric confidence arbitration engine (Slice B4).
//!
//! Reconciles Deepgram Nova-2 (incumbent, per-word confidence) and Groq Whisper Large v3
//! (challenger, segment proxy confidence) without LLMs, hallucinations, or added latency.
//!
//! Invariant: Every delivered token MUST originate from at least one source transcript.

use crate::core::types::{
    ArbitratedTranscript, ArbitrationMode, FlippedRegion, ProviderId, SourceTranscript, WordToken,
};
use tracing::debug;

/// Threshold below which Deepgram incumbent token is considered uncertain.
pub const DEEPGRAM_LOW_CONFIDENCE_THRESHOLD: f64 = 0.50;

/// Threshold above which Groq segment proxy confidence is strong enough to trigger substitution.
pub const GROQ_HIGH_CONFIDENCE_THRESHOLD: f64 = 0.75;

/// Threshold below which an unmatched Deepgram token is deemed a hallucination/noise and dropped.
pub const UNMATCHED_DEEPGRAM_DROP_THRESHOLD: f64 = 0.40;

/// Threshold above which an unmatched Groq token is accepted as a genuine missed word.
pub const UNMATCHED_GROQ_ACCEPT_THRESHOLD: f64 = 0.85;

/// Normalizes a token string for edit-distance comparison (lowercased, alphanumeric only).
pub fn normalize_token(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Identifies whether a word is meaning-critical (negation, question word, or number)
/// which must never be arbitrarily substituted across providers (fail-closed guard).
pub fn is_meaning_critical(word: &str) -> bool {
    let norm = normalize_token(word);

    // 1. Negations
    const NEGATIONS: &[&str] = &[
        "not", "no", "never", "none", "neither", "nor", "cant", "cannot", "wont", "dont", "isnt",
        "arent", "wasnt", "werent", "hasnt", "havent", "hadnt",
    ];
    if NEGATIONS.contains(&norm.as_str()) {
        return true;
    }

    // 2. Question words
    const QUESTION_WORDS: &[&str] = &["why", "what", "where", "who", "when", "how", "which"];
    if QUESTION_WORDS.contains(&norm.as_str()) {
        return true;
    }

    // 3. Numbers & Digits
    if norm.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    const NUMBER_WORDS: &[&str] = &[
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "hundred", "thousand", "million", "billion", "first", "second", "third",
    ];
    if NUMBER_WORDS.contains(&norm.as_str()) {
        return true;
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlignmentOp {
    Match(usize, usize),
    Substitute(usize, usize),
    Delete(usize),
    Insert(usize),
}

/// Computes optimal sequence alignment between Deepgram and Groq tokens using Levenshtein distance.
fn align_tokens(deepgram: &[WordToken], groq: &[WordToken]) -> Vec<AlignmentOp> {
    let m = deepgram.len();
    let n = groq.len();

    if m == 0 {
        return (0..n).map(AlignmentOp::Insert).collect();
    }
    if n == 0 {
        return (0..m).map(AlignmentOp::Delete).collect();
    }

    // DP cost table
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(n + 1) {
        *cell = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost =
                if normalize_token(&deepgram[i - 1].word) == normalize_token(&groq[j - 1].word) {
                    0
                } else {
                    1
                };

            dp[i][j] = (dp[i - 1][j] + 1) // deletion
                .min(dp[i][j - 1] + 1) // insertion
                .min(dp[i - 1][j - 1] + cost); // match or substitution
        }
    }

    // Backtrack to extract alignment path
    let mut ops = Vec::new();
    let mut i = m;
    let mut j = n;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let cost =
                if normalize_token(&deepgram[i - 1].word) == normalize_token(&groq[j - 1].word) {
                    0
                } else {
                    1
                };

            if dp[i][j] == dp[i - 1][j - 1] + cost {
                if cost == 0 {
                    ops.push(AlignmentOp::Match(i - 1, j - 1));
                } else {
                    ops.push(AlignmentOp::Substitute(i - 1, j - 1));
                }
                i -= 1;
                j -= 1;
                continue;
            }
        }

        if i > 0 && dp[i][j] == dp[i - 1][j] + 1 {
            ops.push(AlignmentOp::Delete(i - 1));
            i -= 1;
        } else if j > 0 {
            ops.push(AlignmentOp::Insert(j - 1));
            j -= 1;
        }
    }

    ops.reverse();
    ops
}

pub struct ArbitrationEngine;

impl ArbitrationEngine {
    /// Reconciles transcripts from dual providers or passes through a single available provider.
    pub fn arbitrate(
        deepgram: Option<SourceTranscript>,
        groq: Option<SourceTranscript>,
    ) -> Option<ArbitratedTranscript> {
        match (deepgram, groq) {
            (Some(dg), Some(gq)) => Some(Self::arbitrate_dual(dg, gq)),
            (Some(dg), None) => Some(Self::arbitrate_single(dg, ArbitrationMode::SingleDeepgram)),
            (None, Some(gq)) => Some(Self::arbitrate_single(gq, ArbitrationMode::SingleGroq)),
            (None, None) => None,
        }
    }

    /// Single provider pass-through.
    pub fn arbitrate_single(
        transcript: SourceTranscript,
        mode: ArbitrationMode,
    ) -> ArbitratedTranscript {
        ArbitratedTranscript {
            selected_text: transcript.raw_text,
            primary_source: transcript.provider,
            arbitration_mode: mode,
            flipped_regions: Vec::new(),
            is_source_derived: true,
        }
    }

    /// Full Asymmetric Slice B4 Arbitration between Deepgram and Groq.
    pub fn arbitrate_dual(
        deepgram: SourceTranscript,
        groq: SourceTranscript,
    ) -> ArbitratedTranscript {
        let ops = align_tokens(&deepgram.words, &groq.words);
        let mut final_tokens: Vec<String> = Vec::new();
        let mut flipped_regions: Vec<FlippedRegion> = Vec::new();

        for op in ops {
            match op {
                AlignmentOp::Match(d_idx, g_idx) => {
                    let d = &deepgram.words[d_idx];
                    let g = &groq.words[g_idx];
                    // Keep Deepgram word token, prefer whichever has cleaner punctuation
                    let word_str = d
                        .punctuated
                        .as_deref()
                        .or(g.punctuated.as_deref())
                        .unwrap_or(&d.word);
                    final_tokens.push(word_str.to_string());
                }

                AlignmentOp::Substitute(d_idx, g_idx) => {
                    let d = &deepgram.words[d_idx];
                    let g = &groq.words[g_idx];

                    // Check meaning-critical guard
                    let is_critical = is_meaning_critical(&d.word) || is_meaning_critical(&g.word);

                    let should_flip = !is_critical
                        && d.confidence < DEEPGRAM_LOW_CONFIDENCE_THRESHOLD
                        && g.confidence >= GROQ_HIGH_CONFIDENCE_THRESHOLD;

                    if should_flip {
                        let replacement = g.punctuated.as_deref().unwrap_or(&g.word).to_string();
                        let conf_delta = g.confidence - d.confidence;

                        debug!(
                            "Slice B4 flip: '{}' (conf: {:.2}) -> '{}' (conf: {:.2})",
                            d.word, d.confidence, g.word, g.confidence
                        );

                        flipped_regions.push(FlippedRegion {
                            start_index: final_tokens.len(),
                            original_tokens: vec![d.clone()],
                            replacement_tokens: vec![g.clone()],
                            confidence_delta: conf_delta,
                            arbitration_reason: format!(
                                "Deepgram confidence ({:.2}) < 0.50 and Groq proxy ({:.2}) >= 0.75",
                                d.confidence, g.confidence
                            ),
                        });

                        final_tokens.push(replacement);
                    } else {
                        // Retain incumbent Deepgram token
                        let word_str = d.punctuated.as_deref().unwrap_or(&d.word);
                        final_tokens.push(word_str.to_string());
                    }
                }

                AlignmentOp::Delete(d_idx) => {
                    let d = &deepgram.words[d_idx];
                    // If Deepgram token has extremely low confidence and was not in Groq at all, drop it
                    if d.confidence < UNMATCHED_DEEPGRAM_DROP_THRESHOLD {
                        debug!(
                            "Dropping low-confidence unmatched Deepgram token: '{}' ({:.2})",
                            d.word, d.confidence
                        );
                    } else {
                        let word_str = d.punctuated.as_deref().unwrap_or(&d.word);
                        final_tokens.push(word_str.to_string());
                    }
                }

                AlignmentOp::Insert(g_idx) => {
                    let g = &groq.words[g_idx];
                    // If Groq has high confidence in an extra word that Deepgram missed entirely, include it
                    if g.confidence >= UNMATCHED_GROQ_ACCEPT_THRESHOLD {
                        let word_str = g.punctuated.as_deref().unwrap_or(&g.word);
                        final_tokens.push(word_str.to_string());
                    }
                }
            }
        }

        let selected_text = final_tokens.join(" ");

        ArbitratedTranscript {
            selected_text,
            primary_source: ProviderId::Deepgram,
            arbitration_mode: ArbitrationMode::DualProvider,
            flipped_regions,
            is_source_derived: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_transcripts_match() {
        let dg = SourceTranscript {
            provider: ProviderId::Deepgram,
            raw_text: "Hello world.".to_string(),
            words: vec![
                WordToken::new("hello", 0, 100, 0.95).with_punctuation("Hello"),
                WordToken::new("world", 100, 200, 0.98).with_punctuation("world."),
            ],
            duration_ms: 200,
            latency_ms: 50,
        };

        let gq = SourceTranscript {
            provider: ProviderId::Groq,
            raw_text: "Hello world.".to_string(),
            words: vec![
                WordToken::new("Hello", 0, 100, 0.90),
                WordToken::new("world.", 100, 200, 0.90),
            ],
            duration_ms: 200,
            latency_ms: 180,
        };

        let result = ArbitrationEngine::arbitrate_dual(dg, gq);
        assert_eq!(result.arbitration_mode, ArbitrationMode::DualProvider);
        assert_eq!(result.selected_text, "Hello world.");
        assert_eq!(result.flipped_regions.len(), 0);
        assert!(result.is_source_derived);
    }

    #[test]
    fn test_substitution_flip_when_deepgram_uncertain() {
        // Deepgram heard "weather", confidence 0.35
        let dg = SourceTranscript {
            provider: ProviderId::Deepgram,
            raw_text: "check the weather".to_string(),
            words: vec![
                WordToken::new("check", 0, 50, 0.95),
                WordToken::new("the", 50, 100, 0.95),
                WordToken::new("weather", 100, 200, 0.35),
            ],
            duration_ms: 200,
            latency_ms: 50,
        };

        // Groq heard "whether", segment proxy confidence 0.88
        let gq = SourceTranscript {
            provider: ProviderId::Groq,
            raw_text: "check the whether".to_string(),
            words: vec![
                WordToken::new("check", 0, 50, 0.88),
                WordToken::new("the", 50, 100, 0.88),
                WordToken::new("whether", 100, 200, 0.88),
            ],
            duration_ms: 200,
            latency_ms: 180,
        };

        let result = ArbitrationEngine::arbitrate_dual(dg, gq);
        assert_eq!(result.selected_text, "check the whether");
        assert_eq!(result.flipped_regions.len(), 1);
        assert_eq!(result.flipped_regions[0].original_tokens[0].word, "weather");
        assert_eq!(
            result.flipped_regions[0].replacement_tokens[0].word,
            "whether"
        );
        assert!(result.is_source_derived);
    }

    #[test]
    fn test_meaning_guard_prevents_negation_flip() {
        // Deepgram heard "no", low confidence 0.40
        let dg = SourceTranscript {
            provider: ProviderId::Deepgram,
            raw_text: "no problem".to_string(),
            words: vec![
                WordToken::new("no", 0, 50, 0.40),
                WordToken::new("problem", 50, 150, 0.95),
            ],
            duration_ms: 150,
            latency_ms: 50,
        };

        // Groq heard "so", proxy confidence 0.90
        let gq = SourceTranscript {
            provider: ProviderId::Groq,
            raw_text: "so problem".to_string(),
            words: vec![
                WordToken::new("so", 0, 50, 0.90),
                WordToken::new("problem", 50, 150, 0.90),
            ],
            duration_ms: 150,
            latency_ms: 180,
        };

        let result = ArbitrationEngine::arbitrate_dual(dg, gq);
        // "no" is in the negation guard; MUST NOT FLIP!
        assert_eq!(result.selected_text, "no problem");
        assert_eq!(result.flipped_regions.len(), 0);
    }
}
