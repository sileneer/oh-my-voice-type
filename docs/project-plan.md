# Project Plan — oh-my-voice-type

Voice typing (dictation) app like Typeless: hold a hotkey anywhere, speak, release, get clean polished text inserted at your caret on macOS and Windows (Linux next; mobile phase 3).

Source research: [`docs/research-report.md`](./research-report.md). This plan supersedes it where the two disagree.

---

## 1. Product definition (MVP, macOS + Windows)

One interaction, done obsessively well:

```
hotkey down → pill appears, recording → hotkey up → transcript (ASR) → cleanup (LLM) → insert at caret
```

- **Push-to-talk** (hold) and **toggle** (tap once to start, again to stop) modes.
- Works in any app; global hotkey.
- Three modes (matching Typeless):
  - **Dictate** — clean fillers/repetitions/self-corrections, format lists, keep the user's wording and intent.
  - **Translate** — speak language A, insert polished language B.
  - **Edit selection** — select text + speak a command → replacement text (shorten, tone, fix grammar, translate).
- Small floating pill (recording level + state), tray/menu-bar icon, settings window (hotkey, mic, modes, cleanup, providers, history), history (local SQLite) with copy/retry.
- **Providers v1**: our ASR API + our LLM API (OpenAI-compatible adapters), fully configurable endpoints/models/keys in Settings; no keys compiled in.

Non-goals for v1 (explicit): meetings/notes, cloud sync, teams, mobile, local models, marketplace. Keep seams so these land later without rewrites.

## 2. Architecture

```
┌──────────────────────────── apps/desktop ────────────────────────────┐
│  Tauri 2 host (Rust)                     React UI (Vite + Tailwind)  │
│  ├─ commands (config, history, dictation control, permissions)  ◄──► │
│  ├─ platform input adapters                   settings / history /   │
│  │   macOS: CGEventTap + AX + clipboard+CGEvent ⌘V      pill overlay │
│  │   Windows: WH_KEYBOARD_LL hook + clipboard+SendInput Ctrl+V       │
│  ├─ audio capture (cpal) + level stream + silence watchdog           │
│  ├─ dictation engine glue                                            │
│  └─────────────── uses ──────────────► ovt-core (shared Rust core)  │
└──────────────────────────────────────────────────────────────────────┘
crates/ovt-core: session state machine, provider adapters, cleanup prompts,
dictionary/hotwords, history repository (sqlite), config model, errors.
```

- **Shared core is framework-free.** It knows nothing about Tauri/OS. Hosts implement ports (`AudioSink`-like callbacks, inserters, key capture) and drive `DictationEngine`.
- This mirrors the proven OpenLess pattern and keeps mobile (phase 3, native Swift/Kotlin over UniFFI) and Linux (phase 2, Tauri or egui host) additive.

### Hotkey abstraction
- Gesture decoder in core-agnostic desktop helper: hold ≥350ms = push-to-talk; quick tap = toggle latch; Esc cancels; modifier-only keys (Fn on macOS, RightAlt Windows) need "held alone" disambiguation (chord passthrough).
- macOS: `CGEventTap` at `kCGHIDEventTap` (works for modifier-only + Fn via `IOHIDManager` tap when possible); Accessibility permission required.
- Windows: `SetWindowsHookEx(WH_KEYBOARD_LL)` (modifier-only combos supported; elevated apps need elevated app run).

### Insertion abstraction (the hard part — budget accordingly)
Strategy ladder per platform, resolved per foreground app:
1. AX direct set (`kAXSelectedTextAttribute`) on macOS / UI Automation for supported Windows controls.
2. Clipboard + synthesized paste (macOS ⌘V via CGEvent; Windows Ctrl+V via SendInput), with:
   - save/restore user clipboard (multi-type on macOS; text+HTML on Windows), restore only if untouched (change-count check),
   - transient/clipboard-manager-hiding markers per platform,
   - per-app paste-key overrides (terminals: Shift+Insert / Ctrl+Shift+V variants),
3. Clipboard-only fallback with explicit "pressed paste for you" / "copied — press Ctrl+V" status in the pill.
- Secure fields skipped (macOS `AXSecureTextField`, Windows UIA IsPassword; browsers best-effort, documented).
- Never silently no-op; failures always visible and never lose the transcript (auto-copy).

### Pipeline (core)
```
press → open session (audio device already warm; stream started)
release → stop capture → VAD trim → encode (WAV 16k mono, Opus optional behind feature flag)
       → ASR (batch POST; streaming later behind same trait) → transcript
       → LLM cleanup/rewrite (mode + app context + dictionary + selected text)
       → insertion (host) → history row saved
```
- Streaming: v1 sends whole take (batch) like Typeless observed behavior; trait allows adding streaming ASR (`StreamingAsrProvider`) without touching session code. Pill shows live level; text appears at end (matches v1 scope; partial-preview phase 2).
- Context: v1 ships foreground app name + (opt-in) caret-adjacent text via AX/UIA when the user enables "Context awareness". Default ON? No — default OFF with a one-time explainer (trust), matching our privacy posture; app name + tone mapping ON by default (not sensitive).
- Latency budget: capture→insert p90 ≤ 1.2s on good network (ASR ~200-400ms batch small files + LLM ~300-600ms + insertion ~50ms + overhead). Warm TLS (keep-alive), preload mic stream at hotkey down, model choose small.

### Prompts (v1)
- `cleanup` (dictate): remove fillers/repetitions, keep final intent, punctuation, list formatting, preserve names via dictionary hints ("never insert unseen words"), no answering, no chat-preamble output; empty input → `EMPTY`.
- `translate`: translate to target, natural phrasing, formatting, keep tone.
- `edit_selection`: apply spoken instruction to selected text; output replacement only.
- Golden test fixtures committed in `crates/ovt-core/tests/golden/` (input/expected pairs via mock LLM echo harness + real-provider integration test behind env key).

### Data & privacy
- Audio goes only to configured providers; never stored by default. Optional "keep audio in history" (default OFF; opt-in bytes on disk).
- History (transcript text) in SQLite under app data; retention setting (forever/30d/7d/never).
- API keys in OS vault (Keychain / DPAPI via `keyring` crate) — never in JSON.
- No telemetry in v1 unless explicitly added; deps audited for network calls.
- Password managers / secure fields skipped; context gated and marked in settings with a live indicator when it's being used.

## 3. Contracts (Rust)

```rust
// ovt-core
pub struct DictationConfig { mode, target_lang, app_context: Option<AppContext>, dictionary, ... }
pub struct AppContext { app_name, bundle_id/window_class, window_title, text_before_caret?, selection? }
pub trait AsrProvider { async fn transcribe(&self, wav: Vec<u8>, lang_hints, hotwords) -> Result<Transcript>; }
pub trait LlmProvider { async fn cleanup(&self, req: CleanupRequest) -> Result<String>; }
pub trait HistoryRepo { save/list/delete/retention; }
pub struct DictationEngine { state: Idle|Recording{..}|Processing{..}, ... }
```

Host ports in desktop: `insert_text(strategy)`, `capture_hotkey(callback)`, `audio_start/stop + level stream`, `context_collect`.

## 4. Milestones

| # | Milestone | Exit criteria |
|---|-----------|---------------|
| M1 | Scaffold | workspaces compile; tray app shows; hotkey + mic + insert skeleton on both OS (behind settings) |
| M2 | Capture & insert done right | hold/tap/toggle gestures; clipboard save/restore + transient markers; terminal paste-key routing; no-audio watchdog; insert-failure fallbacks with pill states |
| M3 | Providers & cleanup | both APIs wired via Settings; dictation cleanup prompt with golden tests; latency instrumentation (ms per stage) |
| M4 | Context & dictionary | app-tone mapping; opt-in caret context (AX/UIA); dictionary injected as hotwords + prompt hints |
| M5 | History & polish | SQLite history with copy/retry + retention; onboarding (permissions walkthrough); installer + updater path; crash-safe pending queue |
| M6 | Beta hardening | measure p90 latency corpus; edge app matrix (Slack/Chrome/Word/VS Code/Terminal/Notepad); docs |
| later | Phase 2 Linux | portal hotkeys/insertion, deb/AppImage |
| later | Phase 3 mobile | iOS keyboard (Swift), Android IME + bubble (Kotlin), shared core via UniFFI |
| later | Phase 4 | streaming ASR previews, local Parakeet mode, translation provider expansion |

## 5. Repo layout

```
crates/ovt-core/          # shared engine, providers, prompts, history, config
apps/desktop/
  src/                    # React UI (Vite, TS, Tailwind)
  src-tauri/
    src/
      main.rs, lib.rs     # Tauri app setup, commands, state
      dictation.rs        # engine glue (session state), integrates core
      audio.rs            # cpal capture, level meter, device watch, WAV encode
      hotkey.rs           # gesture state machine (shared logic)
      platform/
        windows.rs        # LL hook, SendInput paste, clipboard (windows crate)
        macos.rs          # CGEventTap, AXUIElement, NSPasteboard+CGEvent (objc2)
    icons/, capabilities/, tauri.conf.json
docs/
scripts/
.github/workflows/        # CI: cargo check/clippy/test + frontend build (phase 1 target)
```

## 6. Risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Insertion inconsistency across apps (Electron lazily enables AX tree; terminals remap paste keys) | core UX failures | per-app routing table (evoglyph-style), paste fallback always works, status surfaced; test matrix in M2/M6 |
| macOS AX/Input Monitoring permission churn (rebuilds invalidate trust) vs notarization | onboarding friction | guided onboarding, re-check on launch + on insert failure, `tccutil` docs for dev |
| Windows UIPI (elevated apps) | dictation into admin apps fails | detect foreground elevated process → copied-to-clipboard path with clear message; doc "run elevated" |
| Hotkey conflicts / bare-modifier quirks | hotkey triggers chords, steals Fn | hold-alone detection, chord passthrough, Esc cancel, user-rebindable, Fn optional (RightAlt/RightOpt defaults) |
| LLM latency variance / over-cleaning complaints (Typeless's top complaint: unwanted restructuring) | perceived latency, wrong output | small fast model, temperature 0, "preserve wording" toggle per-app, output restraint prompt; p99 instrumentation |
| Provider API differences (streaming/hotwords support, auth) | schedule | adapters isolate; provider field mapping + capability flags; contract tests with recorded fixtures |
| Supply-chain (macro-heavy crates) | security | deny/audit in CI phase 2, pinned versions, minimal features (chosen crates are mainstream: cpal, arboard, windows, objc2, reqwest) |

## 7. Verification strategy
- Core: `cargo test -p ovt-core` — prompt golden cases (mock provider), history repo, dictionary hotword budget, config validation, session state machine transitions.
- Desktop host: `cargo check/clippy` both OS in CI; Windows local run for hotkey/clipboard (real machine), macOS smoke on Mac before release; unit tests for gesture decoder and paste-key router.
- Frontend: `tsc --noEmit` + `vite build` in CI.
- Manual acceptance checklist per release (docs/acceptance.md) — the app matrix.

## 8. Open questions (need product/engineering answers before M3)
1. Which ASR/LLM APIs are "our APIs" (endpoints/models)? — required for M3 wiring & latency budget.
2. Default free-vs-paid boundaries? (v1 can ship all-pro, billing later.)
3. Brand naming for the app/crates (`ovt` placeholder) and distribution identity (bundle id, signing).
