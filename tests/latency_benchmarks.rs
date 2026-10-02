//! Empirical Latency & Performance Benchmarks for Voisu Windows.
//!
//! Validates:
//! - Levenshtein DP Alignment throughput (< 5ms for 100 words)
//! - Spoken Punctuation Formatting latency (< 1ms)
//! - Rubato Sinc Resampling 48kHz -> 16kHz throughput

use std::time::Instant;
use voisu_win::core::arbitration::ArbitrationEngine;
use voisu_win::core::audio::AudioResampler;
use voisu_win::core::formatting::FormattingEngine;
use voisu_win::core::types::{ProviderId, SourceTranscript, WordToken};

#[test]
fn bench_levenshtein_arbitration_throughput() {
    // Generate realistic 100-word transcripts
    let base_words = [
        "the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog", "and", "then",
        "runs", "across", "the", "wide", "open", "green", "field", "towards", "the",
    ];

    let mut dg_tokens = Vec::new();
    let mut groq_tokens = Vec::new();

    for i in 0..100 {
        let w = base_words[i % base_words.len()];
        // Deepgram tokens: some lower confidence
        let dg_conf = if i % 7 == 0 { 0.45 } else { 0.95 };
        let dg_word = if i % 7 == 0 { "cat" } else { w };
        dg_tokens.push(WordToken::new(
            dg_word,
            (i * 200) as u32,
            ((i + 1) * 200) as u32,
            dg_conf,
        ));

        // Groq tokens: high confidence
        groq_tokens.push(WordToken::new(
            w,
            (i * 200) as u32,
            ((i + 1) * 200) as u32,
            0.98,
        ));
    }

    let dg_transcript = SourceTranscript {
        provider: ProviderId::Deepgram,
        raw_text: dg_tokens
            .iter()
            .map(|t| t.word.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        words: dg_tokens,
        duration_ms: 20000,
        latency_ms: 120,
    };

    let groq_transcript = SourceTranscript {
        provider: ProviderId::Groq,
        raw_text: groq_tokens
            .iter()
            .map(|t| t.word.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        words: groq_tokens,
        duration_ms: 20000,
        latency_ms: 320,
    };

    // Warm up
    let _ =
        ArbitrationEngine::arbitrate(Some(dg_transcript.clone()), Some(groq_transcript.clone()));

    // Benchmark 100 iterations
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        let arb = ArbitrationEngine::arbitrate(
            Some(dg_transcript.clone()),
            Some(groq_transcript.clone()),
        )
        .expect("Arbitration must succeed");
        assert!(!arb.flipped_regions.is_empty());
    }
    let elapsed = start.elapsed();
    let per_op_us = elapsed.as_micros() / iterations;

    println!("\n============================================================");
    println!("  BENCHMARK: Levenshtein DP Word Alignment (100 words)");
    println!("  Total iterations : {}", iterations);
    println!("  Total elapsed    : {:?}", elapsed);
    println!(
        "  Latency per run  : {} µs ({} ms)",
        per_op_us,
        per_op_us as f64 / 1000.0
    );
    println!("============================================================");

    // Hard invariant: Must complete in < 5ms in release (< 15ms in debug unoptimized)
    let threshold_us = if cfg!(debug_assertions) {
        15_000
    } else {
        5_000
    };
    assert!(
        per_op_us < threshold_us,
        "Levenshtein arbitration took too long: {} µs (threshold: {} µs)",
        per_op_us,
        threshold_us
    );
}

#[test]
fn bench_formatting_engine_latency() {
    let raw_text = "hello world period new paragraph testing open parenthesis advanced closed parenthesis comma next question mark";

    // Warm up
    let _ = FormattingEngine::format(raw_text);

    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let result = FormattingEngine::format(raw_text);
        assert!(result.contains('.'));
    }
    let elapsed = start.elapsed();
    let per_op_us = elapsed.as_micros() / iterations;

    println!("\n============================================================");
    println!("  BENCHMARK: Spoken Punctuation Formatting");
    println!("  Total iterations : {}", iterations);
    println!("  Total elapsed    : {:?}", elapsed);
    println!(
        "  Latency per run  : {} µs ({} ms)",
        per_op_us,
        per_op_us as f64 / 1000.0
    );
    println!("============================================================");

    // Hard invariant: Must complete in < 1ms (1,000 µs)
    assert!(
        per_op_us < 1000,
        "Formatting took too long: {} µs",
        per_op_us
    );
}

#[test]
fn bench_audio_resampler_throughput() {
    // 5 seconds of 48kHz mono float samples
    let sample_rate = 48000;
    let duration_secs = 5;
    let total_samples = sample_rate * duration_secs;
    let samples: Vec<f32> = (0..total_samples)
        .map(|i| (i as f32 * 0.05).sin() * 0.3)
        .collect();

    let mut resampler = AudioResampler::new(48000, 16000).expect("Resampler init");

    let start = Instant::now();
    let resampled = resampler
        .process(&samples)
        .expect("Resampling must succeed");
    let elapsed = start.elapsed();

    // 5 seconds @ 16kHz = ~80,000 samples
    let expected_16k = total_samples / 3;
    assert!((resampled.len() as i64 - expected_16k as i64).abs() < 1000);

    println!("\n============================================================");
    println!("  BENCHMARK: Sinc Resampling 48kHz -> 16kHz (5 seconds audio)");
    println!("  Input samples    : {}", total_samples);
    println!("  Output samples   : {}", resampled.len());
    println!("  Processing time  : {:?}", elapsed);
    println!(
        "  Speedup factor   : {:.1}x real-time",
        duration_secs as f64 / elapsed.as_secs_f64()
    );
    println!("============================================================");

    let max_allowed_ms = if cfg!(debug_assertions) { 250 } else { 100 };
    assert!(
        elapsed.as_millis() < max_allowed_ms,
        "Resampling was slower than expected: {:?}",
        elapsed
    );
}
