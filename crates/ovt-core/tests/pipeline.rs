//! End-to-end pipeline tests with mock providers: verifies the session engine
//! wires dictionary context into ASR requests (hotwords/context packing),
//! cleanup, insertion and history — including fallbacks.

use async_trait::async_trait;
use ovt_core::config::Settings;
use ovt_core::dictation::{prepare_audio, DictationEngine, DictationInput};
use ovt_core::dictionary::Dictionary;
use ovt_core::history::HistoryRepo;
use ovt_core::prompt::Mode;
use ovt_core::providers::{
    AsrCapabilities, AsrProvider, CleanupRequest, HotwordRules, LlmProvider, TranscribeRequest,
    Transcript,
};
use ovt_core::wav::{self, TARGET_SAMPLE_RATE};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Records what the engine actually sends to the ASR provider.
#[derive(Default, Clone)]
struct AsrCapture {
    pub language: Option<String>,
    pub hotwords: Vec<String>,
    pub context: Option<String>,
}

struct MockAsr {
    text: String,
    calls: AtomicUsize,
    capture: Mutex<AsrCapture>,
    caps: AsrCapabilities,
}

#[async_trait]
impl AsrProvider for MockAsr {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn capabilities(&self) -> AsrCapabilities {
        self.caps
    }

    async fn transcribe(&self, req: TranscribeRequest) -> ovt_core::Result<Transcript> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.capture.lock().unwrap() = AsrCapture {
            language: req.language.clone(),
            hotwords: req.hotwords.clone(),
            context: req.context.clone(),
        };
        Ok(Transcript {
            text: self.text.clone(),
            language: Some("en".into()),
            duration_ms: Some(1000),
        })
    }
}

struct MockLlm {
    reply: Result<String, String>,
    calls: AtomicUsize,
    cleanup: Mutex<Option<CleanupRequest>>,
}

#[async_trait]
impl LlmProvider for MockLlm {
    async fn complete(&self, req: CleanupRequest) -> ovt_core::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(!req.system.is_empty());
        *self.cleanup.lock().unwrap() = Some(req);
        match &self.reply {
            Ok(text) => Ok(text.clone()),
            Err(e) => Err(ovt_core::Error::Network(e.clone())),
        }
    }
}

fn engine(
    asr: Arc<MockAsr>,
    llm: Arc<MockLlm>,
    history: Option<Arc<HistoryRepo>>,
) -> DictationEngine {
    DictationEngine::new(
        asr,
        Some(llm as Arc<dyn LlmProvider>),
        Settings::default(),
        Dictionary::default(),
        history,
    )
}

fn engine_with_dict(
    asr: Arc<MockAsr>,
    llm: Arc<MockLlm>,
    settings: Settings,
    dictionary: Dictionary,
) -> DictationEngine {
    DictationEngine::new(
        asr,
        Some(llm as Arc<dyn LlmProvider>),
        settings,
        dictionary,
        None,
    )
}

fn mock_asr(text: &str) -> Arc<MockAsr> {
    Arc::new(MockAsr {
        text: text.to_string(),
        calls: AtomicUsize::new(0),
        capture: Mutex::new(AsrCapture::default()),
        caps: ovt_core::providers::default_capabilities(),
    })
}

fn u2_like_asr(text: &str) -> Arc<MockAsr> {
    Arc::new(MockAsr {
        text: text.to_string(),
        calls: AtomicUsize::new(0),
        capture: Mutex::new(AsrCapture::default()),
        caps: AsrCapabilities {
            hotwords: HotwordRules::List {
                max_items: 200,
                max_item_chars: 5,
            },
            max_context_chars: Some(500),
            languages: &[("en-US", "English (US)"), ("zh-CN", "Chinese (Mandarin)")],
        },
    })
}

fn speech_wav() -> Vec<u8> {
    let samples: Vec<f32> = (0..TARGET_SAMPLE_RATE)
        .map(|i| ((i as f32) * 0.02).sin() * 0.3)
        .collect();
    wav::encode_wav_16k_mono(&samples, TARGET_SAMPLE_RATE).unwrap()
}

#[tokio::test]
async fn full_pipeline_dictate() {
    let history = Arc::new(HistoryRepo::open_in_memory().unwrap());
    let asr = mock_asr("um so yeah i think we should ship it friday");
    let llm = Arc::new(MockLlm {
        reply: Ok("I think we should ship it Friday.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let e = engine(asr.clone(), llm.clone(), Some(history.clone()));

    let mut inserted = String::new();
    let input = DictationInput {
        app_name: Some("Slack".into()),
        ..DictationInput::new(speech_wav(), Mode::Dictate)
    };
    let outcome = e
        .process(input, |text| {
            inserted = text.to_string();
            Ok(())
        })
        .await
        .unwrap();

    assert_eq!(outcome.final_text, "I think we should ship it Friday.");
    assert_eq!(inserted, outcome.final_text);
    assert!(outcome.inserted);
    assert_eq!(asr.calls.load(Ordering::SeqCst), 1);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    assert!(outcome.warnings.is_empty());

    let page = history.list(10, 0, None).unwrap();
    assert_eq!(page.total, 1);
    assert!(page.entries[0].insert_ok);
}

#[tokio::test]
async fn translate_requires_llm_even_when_cleanup_is_disabled() {
    let asr = mock_asr("Bonjour tout le monde");
    let llm = Arc::new(MockLlm {
        reply: Ok("Hello everyone.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let mut settings = Settings::default();
    settings.dictation.cleanup_enabled = false;
    let engine = DictationEngine::new(
        asr,
        Some(llm.clone()),
        settings,
        Dictionary::default(),
        None,
    );
    let mut input = DictationInput::new(speech_wav(), Mode::Translate);
    input.target_language = Some("English".into());
    let outcome = engine.process(input, |_| Ok(())).await.unwrap();
    assert_eq!(outcome.final_text, "Hello everyone.");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn translate_without_llm_never_inserts_source_text() {
    let asr = mock_asr("Bonjour");
    let engine = DictationEngine::new(asr, None, Settings::default(), Dictionary::default(), None);
    let mut input = DictationInput::new(speech_wav(), Mode::Translate);
    input.target_language = Some("English".into());
    let inserted = Arc::new(AtomicUsize::new(0));
    let inserted_for_callback = inserted.clone();
    let result = engine
        .process(input, move |_| {
            inserted_for_callback.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await;
    assert!(result.is_err());
    assert_eq!(inserted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn edit_prompt_contains_selection_exactly_once() {
    let asr = mock_asr("make this shorter");
    let llm = Arc::new(MockLlm {
        reply: Ok("Short text.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let engine = engine(asr, llm.clone(), None);
    let mut input = DictationInput::new(speech_wav(), Mode::EditSelection);
    input.selection = Some("unique-selection-marker".into());
    engine.process(input, |_| Ok(())).await.unwrap();
    let request = llm.cleanup.lock().unwrap().clone().unwrap();
    assert_eq!(request.user.matches("unique-selection-marker").count(), 1);
}

#[tokio::test]
async fn u2_style_provider_splits_short_and_long_terms() {
    let asr = u2_like_asr("ship it");
    let llm = Arc::new(MockLlm {
        reply: Ok("Ship it.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let mut dictionary = Dictionary::default();
    for term in ["K8s", "Claude", "Kubernetes", "Codestral-CLI", "Redis"] {
        dictionary.add_term(term);
    }
    let mut settings = Settings::default();
    settings.dictation.language = Some("en-US".into());

    let e = engine_with_dict(asr.clone(), llm, settings, dictionary);
    let input = DictationInput {
        app_name: Some("Slack".into()),
        window_title: Some("#eng".into()),
        ..DictationInput::new(speech_wav(), Mode::Dictate)
    };
    e.process(input, |_| Ok(())).await.unwrap();

    let capture = asr.capture.lock().unwrap().clone();
    // ≤5-char terms become hotwords; long ones ride in context.
    assert!(capture.hotwords.contains(&"K8s".to_string()));
    assert!(capture.hotwords.contains(&"Redis".to_string()));
    assert!(!capture.hotwords.contains(&"Kubernetes".to_string()));
    let context = capture
        .context
        .expect("context must be sent for U2-like providers");
    assert!(
        context.contains("Kubernetes"),
        "long terms must reach context: {context}"
    );
    assert!(context.contains("Codestral-CLI"));
    assert!(context.contains("App: Slack — #eng"));
    // Language resolved to the provider-native code.
    assert_eq!(capture.language.as_deref(), Some("en-US"));
}

#[tokio::test]
async fn whisper_style_provider_joins_terms_into_prompt_field() {
    let asr = mock_asr("ship it");
    let llm = Arc::new(MockLlm {
        reply: Ok("Ship it.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let mut dictionary = Dictionary::default();
    dictionary.add_term("K8s");
    let mut settings = Settings::default();
    settings.dictation.language = Some("en-US".into());

    let e = engine_with_dict(asr.clone(), llm, settings, dictionary);
    e.process(DictationInput::new(speech_wav(), Mode::Dictate), |_| Ok(()))
        .await
        .unwrap();

    let capture = asr.capture.lock().unwrap().clone();
    // Prompt-field providers get all terms as hotwords, ISO-639-1 language.
    assert_eq!(capture.hotwords, vec!["K8s".to_string()]);
    assert_eq!(capture.language.as_deref(), Some("en"));
}

#[tokio::test]
async fn dictation_language_auto_detect_sends_none() {
    let asr = u2_like_asr("hello");
    let llm = Arc::new(MockLlm {
        reply: Ok("Hello.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let mut settings = Settings::default();
    settings.dictation.language = Some("auto".into());
    let e = engine_with_dict(asr.clone(), llm, settings, Dictionary::default());
    e.process(DictationInput::new(speech_wav(), Mode::Dictate), |_| Ok(()))
        .await
        .unwrap();
    assert!(asr.capture.lock().unwrap().language.is_none());
}

#[tokio::test]
async fn cleanup_failure_falls_back_to_raw() {
    let asr = mock_asr("hello there");
    let llm = Arc::new(MockLlm {
        reply: Err("boom".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let e = engine(asr, llm, None);

    let outcome = e
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(outcome.final_text, "hello there");
    assert_eq!(outcome.warnings.len(), 1);
    assert!(outcome.warnings[0].contains("raw transcript"));
}

#[tokio::test]
async fn reasoning_blocks_are_never_inserted() {
    let asr = mock_asr("hello world");
    let llm = Arc::new(MockLlm {
        reply: Ok(
            "<think>The user wants punctuation.\nAdd a comma.</think>\n\nHello, world.".into(),
        ),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let e = engine(asr, llm, None);
    let typed = Mutex::new(String::new());
    let outcome = e
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |text| {
            *typed.lock().unwrap() = text.to_string();
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(outcome.final_text, "Hello, world.");
    assert_eq!(*typed.lock().unwrap(), "Hello, world.");

    // Cut off inside the reasoning: dictation falls back to the transcript.
    assert_eq!(
        ovt_core::dictation::strip_reasoning("<THINK>still going"),
        ""
    );
    assert_eq!(
        ovt_core::dictation::strip_reasoning("  plain answer "),
        "plain answer"
    );
}

#[tokio::test]
async fn empty_transcript_never_inserts_or_records_history() {
    // The desktop host relies on this: an empty outcome must not reach the
    // inserter or history, so it can discard the take without a clipboard copy.
    let asr = mock_asr("   ");
    let llm = Arc::new(MockLlm {
        reply: Ok("unused".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let history = Arc::new(HistoryRepo::open_in_memory().unwrap());
    let e = engine(asr, llm, Some(history.clone()));
    let mut inserted = false;
    let outcome = e
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |_| {
            inserted = true;
            Ok(())
        })
        .await
        .unwrap();
    assert!(!inserted);
    assert!(outcome.final_text.is_empty());
    assert!(outcome.warnings[0].contains("Nothing heard"));
    assert_eq!(history.count().unwrap(), 0);
}

#[test]
fn recovered_native_rate_audio_is_normalised_to_16k() {
    // Pending recordings are stored at the microphone's rate; recovery must
    // send the provider the same 16 kHz payload as a live session.
    let samples: Vec<f32> = (0..48_000)
        .map(|i| ((i as f32) * 0.01).sin() * 0.3)
        .collect();
    let stored = wav::encode_wav_mono(&samples, 48_000);
    let (decoded, rate) = wav::decode_pcm16_wav(&stored).unwrap();
    assert_eq!(rate, 48_000);
    let (payload, peak) = ovt_core::dictation::prepare_audio(&decoded, rate);
    let payload_rate = u32::from_le_bytes(payload[24..28].try_into().unwrap());
    assert_eq!(payload_rate, TARGET_SAMPLE_RATE);
    assert!(peak > -20.0);
    ovt_core::dictation::ensure_has_audio(peak).unwrap();
}

#[tokio::test]
async fn insert_failure_reports_not_inserted() {
    let asr = mock_asr("hi");
    let llm = Arc::new(MockLlm {
        reply: Ok("Hi.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let e = engine(asr, llm, None);
    let outcome = e
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |_| {
            Err(ovt_core::Error::Insert("no target".into()))
        })
        .await
        .unwrap();
    assert!(!outcome.inserted);
    assert!(!outcome.warnings.is_empty());
    assert_eq!(outcome.final_text, "Hi.");
}

#[tokio::test]
async fn empty_transcript_short_circuits() {
    let asr = mock_asr("");
    let llm = Arc::new(MockLlm {
        reply: Ok("should not run".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let e = engine(asr, llm.clone(), None);
    let outcome = e
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |_| Ok(()))
        .await
        .unwrap();
    assert!(!outcome.inserted);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    assert!(outcome.final_text.is_empty());
}

#[tokio::test]
async fn missing_cleanup_provider_inserts_raw_text_with_warning() {
    // Setup where no AI cleanup endpoint exists: dictation must still work
    // (raw transcript inserted), not fail with a dead take.
    let asr = mock_asr("raw transcript text");
    let engine = DictationEngine::new(asr, None, Settings::default(), Dictionary::default(), None);
    let mut inserted = String::new();
    let outcome = engine
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |text| {
            inserted = text.to_string();
            Ok(())
        })
        .await
        .unwrap();
    assert!(outcome.inserted);
    assert_eq!(inserted, "raw transcript text");
    assert_eq!(outcome.final_text, "raw transcript text");
    assert!(outcome
        .warnings
        .iter()
        .any(|w| w.contains("AI cleanup is off")));
}

#[tokio::test]
async fn cleanup_disabled_skips_llm_and_inserts_raw() {
    // The user preference "AI cleanup off" must bypass the LLM even when a
    // cleanup provider is configured and available.
    let asr = mock_asr("um, so, hello, uh, world");
    let llm = Arc::new(MockLlm {
        reply: Ok("Hello, world.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let mut settings = Settings::default();
    settings.dictation.cleanup_enabled = false;
    let mut inserted = String::new();
    let engine = DictationEngine::new(
        asr,
        Some(llm.clone() as Arc<dyn LlmProvider>),
        settings,
        Dictionary::default(),
        None,
    );
    let outcome = engine
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |text| {
            inserted = text.to_string();
            Ok(())
        })
        .await
        .unwrap();

    assert_eq!(
        llm.calls.load(Ordering::SeqCst),
        0,
        "LLM must not be called"
    );
    assert_eq!(outcome.final_text, "um, so, hello, uh, world");
    assert_eq!(inserted, outcome.final_text);
    assert!(
        outcome.warnings.is_empty(),
        "turn-off is intentional; no warning"
    );

    // And with the preference on, the same setup does call the LLM.
    let asr = mock_asr("um, so, hello, uh, world");
    let llm2 = Arc::new(MockLlm {
        reply: Ok("Hello, world.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let engine = DictationEngine::new(
        asr,
        Some(llm2.clone() as Arc<dyn LlmProvider>),
        Settings::default(),
        Dictionary::default(),
        None,
    );
    let outcome = engine
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(llm2.calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.final_text, "Hello, world.");
}

#[tokio::test]
async fn cleanup_request_uses_cleanup_settings() {
    let asr = mock_asr("hello");
    let llm = Arc::new(MockLlm {
        reply: Ok("Hello.".into()),
        calls: AtomicUsize::new(0),
        cleanup: Mutex::new(None),
    });
    let mut settings = Settings::default();
    settings.cleanup.max_tokens = 2048;
    settings.cleanup.temperature = 0.7;
    let engine = DictationEngine::new(
        asr,
        Some(llm.clone() as Arc<dyn LlmProvider>),
        settings,
        Dictionary::default(),
        None,
    );

    engine
        .process(DictationInput::new(speech_wav(), Mode::Dictate), |_| Ok(()))
        .await
        .unwrap();

    let request = llm.cleanup.lock().unwrap().clone().unwrap();
    assert_eq!(request.max_tokens, 2048);
    assert_eq!(request.temperature, 0.7);
}

#[test]
fn prepare_audio_trims_and_resamples() {
    let rate = 48_000u32;
    let mut samples = vec![0.0f32; rate as usize / 4];
    samples.extend((0..rate / 2).map(|i| ((i as f32) * 0.01).sin() * 0.25));
    samples.extend(vec![0.0f32; rate as usize / 4]);
    let (wav_bytes, peak) = prepare_audio(&samples, rate);
    assert_eq!(&wav_bytes[0..4], b"RIFF");
    assert!(peak > -20.0);
    let data_len = u32::from_le_bytes(wav_bytes[40..44].try_into().unwrap()) as usize;
    let secs = data_len as f32 / (TARGET_SAMPLE_RATE as f32 * 2.0);
    assert!(secs < 0.95, "expected trimming, got {secs}s");
}
