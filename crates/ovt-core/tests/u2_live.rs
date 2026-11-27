//! Live U2-ASR integration test (opt-in; requires network + credentials).
//!
//! Run explicitly after exporting `U2_ASR_TOKEN_PLAN_API_KEY`:
//!   cargo test -p ovt-core --test u2_live -- --ignored --nocapture
//!
//! What it proves: authentication, multipart upload, task creation with
//! hotwords/context, polling to terminal state, and transcript extraction —
//! the whole async U2 workflow end to end.

use ovt_core::providers::u2::U2Asr;
use ovt_core::providers::{AsrProvider, TranscribeRequest};
use std::time::Duration;

fn tone_wav() -> Vec<u8> {
    // ~1.2s of faint 500 Hz tone; enough to exercise the pipeline. The
    // transcript may be empty — that still validates every stage.
    let rate = 16_000u32;
    let samples: Vec<f32> = (0..rate * 12 / 10)
        .map(|i| ((i as f32) * 500.0 * std::f32::consts::TAU / rate as f32).sin() * 0.05)
        .collect();
    ovt_core::wav::encode_wav_16k_mono(&samples, rate).unwrap()
}

#[tokio::test]
#[ignore = "live network + credentials; run with --ignored"]
async fn live_u2_transcription_workflow() {
    let Some(key) = std::env::var("U2_ASR_TOKEN_PLAN_API_KEY")
        .or_else(|_| std::env::var("U2_ASR_PLATFORM_API_KEY"))
        .ok()
        .filter(|k| !k.trim().is_empty())
    else {
        eprintln!("skipped: no U2 credential in environment");
        return;
    };

    let asr = U2Asr::new(key).with_timing(Duration::from_millis(700), Duration::from_secs(90));

    // Prove capabilities arrive for this engine.
    let caps = asr.capabilities();
    assert_eq!(caps.max_context_chars, Some(500));
    assert_eq!(caps.languages.len(), 15);

    let req = TranscribeRequest {
        audio: tone_wav(),
        language: Some(caps.resolve_language(Some("en-US")).unwrap()),
        hotwords: vec!["K8s".to_string(), "Redis".to_string()],
        context: Some(
            "App: Slack — #eng\nVocabulary (use these exact spellings): Kubernetes".to_string(),
        ),
    };

    let result = asr.transcribe(req).await;
    match result {
        Ok(transcript) => {
            println!(
                "live U2 OK: {} chars, duration_ms={:?}, language={:?}",
                transcript.text.chars().count(),
                transcript.duration_ms,
                transcript.language
            );
        }
        Err(ovt_core::Error::ProviderResponse(msg))
            if msg.contains("finished without transcript text") =>
        {
            // Tone carries no speech; empty transcript means upload+task+poll
            // all succeeded through to the final payload.
            println!("live U2 OK (empty transcript on tone input): {msg}");
        }
        Err(other) => panic!("live U2 workflow failed before completion: {other}"),
    }
}
