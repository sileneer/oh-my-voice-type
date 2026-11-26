# Voice Typing Application — Research Report

_Compiled 2026-11-21. Based on public sources: Typeless's own site/docs/privacy pages, App Store listings, published third-party static analyses of the Typeless desktop app, competitor documentation, and open-source projects on GitHub._

---

## 1. What Typeless is

**Typeless** is an AI voice-typing product by Simply CA LLC (Palo Alto). Its pitch: "Speak, don't type" — dictation that turns messy spoken thoughts into polished writing in any app, claiming ~4x typing speed (45 wpm typical typing vs 220 wpm on the "Typeless keyboard").

### Platforms
- **Desktop**: macOS (Apple Silicon + Intel), Windows, Linux
- **Mobile**: iOS (iPhone, a custom "AI voice keyboard"), Android (keyboard + floating bubble)

An account links all devices; cloud sync of history/dictionary is optional (Pro).

### Core interaction (desktop)
- Works system-wide (no browser extension): Google Docs, Notion, Gmail, Slack, IDEs, terminals, etc.
- **Hold (or tap) Fn** on macOS / **Right Alt** on Windows to dictate with push-to-talk; a floating "voice bar" shows state. Tap again to stop; text is inserted at the caret.
- Same pattern with dedicated hotkeys for **Translate** (speak language A → get language B typed) and **Ask anything** (speech command on selected text, or a question with answer in a pop-up card).

### Core interaction (mobile)
- Custom **iOS keyboard extension**: a "Speak" button; requires "Allow Full Access" (network + shared container) since dictation audio must reach servers.
- **Android keyboard** plus a **floating bubble** that works alongside the user's preferred keyboard (needs overlay, accessibility, background permissions). Android keyboard supports dictation insertion via `InputConnection`.

### Feature set
- **Dictate**: removes filler words ("um", "uh"), repetitions, self-corrections (keeps final intent), spoken lists → structured text, punctuation/formating, "find the perfect words".
- **Personalization**: learns style/tone over time (vendor says only abstract patterns are learned, not content).
- **Personal dictionary**: automatic (from user corrections) and manual; CSV bulk import; hot-instruction to ASR when supported by the provider.
- **Tone per app**: different tone in email vs chat vs docs.
- **100+ languages**, auto language detection, code-switching (language mixing mid-sentence).
- **Translate mode** with preconfigured target languages.
- **Ask anything** (desktop): select text + speak instructions (shorten, lengthen, tone, translate, explain, summarize); web-backed quick answers/actions.
- **Whisper mode** (added as plan feature).
- **History**: on-device by default; retain 24h–forever or never; feedback/retry; download original audio (desktop).

### Pricing & plan structure (as of Oct–Nov 2026)
| Plan | Price | Notable |
|---|---|---|
| Free | $0 | limited words/wk (number unpublished), standard accuracy |
| Pro | $12/mo (yearly) or $30/mo (monthly) | unlimited words, enhanced accuracy, priority queues, cloud sync |
| Ultra | $200/mo | max accuracy/intelligence tiers |
| Enterprise | custom | SSO/SAML, SCIM, domain capture, audit logs, HIPAA + BAA |

Students get 50% off on desktop checkout. Cloud sync, team management, and analytics are Pro+ features.

### Limits
- **9-minute cap per dictation** (60-second countdown warning from minute 8; auto-saves to History at the limit).
- **Cloud-only architecture**: no offline mode; requires connectivity.
- **Cloud processing disclosure** (official data-controls page): audio + limited context (active app, relevant text) sent to servers, "processed in real time and immediately discarded"; third-party LLM providers named as Microsoft and Google under zero-retention agreements; no model training on data; no audio stored in the cloud; optional History Sync stores transcript text only.

### Ratings & user reception
- App Store ≈4.5 (761 ratings), Google Play ≈4.2 with 500k+ downloads.
- Common praise: output reads like polished writing even when you ramble; translation; electron dictation into AI tools.
- Common complaints: aggressively restructures content (plain sentences → unrequested numbered lists); wants a "stay closer to my words" dial; occasional "high demand" throttling.

---

## 2. How Typeless actually works (from public static analyses of the desktop app)

Two independent reverse-engineering write-ups (one on v1.0.2 / Electron 33, one on v2.8.1–2.8.2 / Electron + Swift helpers) agree on the architecture. Summary of observed design:

### Process model
```
┌─ Renderer (Chromium) ──────────┐   IPC   ┌─ Node main process ──────────┐  FFI (koffi)  ┌─ Swift dylibs ──────────────┐
│ React 18 UI, multiple windows: │◄───────►│ SQLite (DrizzleORM/libSQL),  │◄─────────────►│ libKeyboardHelper (CGEventTap│
│ hub / floating-bar /           │         │ electron-store, updater,     │               │  HID-level Fn detection)     │
│ onboarding / login / sidebar / │         │ undici HTTPS, Opus workers   │               │ libInputHelper (AX insert /  │
│ interactive-card windows       │         │                              │               │  clipboard paste, selection) │
└────────────────────────────────┘         └──────────────────────────────┘               │ libContextHelper (AX cursor/ │
                                                                                           │  visible text/context)       │
                                                                                           │ libUtilHelper (audio devices,│
                                                                                           │  muting, permissions)        │
                                                                                           └──────────────────────────────┘
```

- **Electron** (consistent across versions: Electron 33 → later), React renderer, Node main process, native Swift helpers loaded via `koffi` FFI (7-8 windows/entry points: hub, floating bar, onboarding, login, sidebar, interactive card).
- **No local ASR models** in the app bundle → ASR + LLM rewriting are done server-side.
- **Global hotkeys**: not Electron's `globalShortcut`; instead a native HID-level keyboard helper (CGEventTap / IOHIDManager) because macOS's Fn/Globe key and external keyboard layouts aren't reachable through standard APIs. Defaults: Dictate = Fn (mac) / RightAlt (win), Ask anything = +Space, Translate = +Shift.
- **Audio pipeline**: mic captured in renderer via `getUserMedia` → AudioWorklet at 16 kHz mono Float32, batches of 1024 samples → main process encodes Opus (e.g., 32 kbps VBR) in a worker pool → single multipart POST `https://api.typeless.com/ai/voice_flow` with fields: `audio_file`, `mode` (`voice_transcript` / `voice_command` / `voice_translation`), `audio_context` (JSON), `parameters` (selected_text / output_language), plus usage metadata. Timeout 40s, abortable.
- **One server round-trip does ASR + intent + rewrite + formatting + translation**: response `{ refine_text, delivery, external_action, web_metadata, ... }`. No prompt templates in the client; "personalized style", "different tones for each app", dictionary and app context are all server-side functions conditioned on the uploaded `audio_context`.
- **Context sent with audio**: focused app name/bundle id/window title, whether native or browser (+context), text cursor state (`text_before_cursor`, `text_after_cursor`, selected text), surrounding visible text (truncated to token budgets, e.g., ~10k tokens visible text / ~5k selection / 1k+1k surroundings). A remote **blacklist of apps/domains** is fetched (AES-encrypted) and checked *before* the accessibility read for sensitive apps (password managers etc. are not automatically excluded — notably, cursor state was observed to still be read in blacklisted apps). There's also a hardcoded Notion/AXWebArea exception.
- **Text insertion**: primary = save clipboard → write transcript → synthesize ⌘V via CGEvent → restore clipboard; fallback = direct AXUIElement set-value; a `deleteBackward` primitive lets the server-driven flow "rewrite" what was already typed. Rich-text (HTML) insertion computed natively for fields that support it.
- **Retry/cancel controls**, history DB (local), optional history sync endpoints, updater (Squirrel/embedded feed), analytics, Sentry.
- One analysis notes: no evidence of a streaming ASR WebSocket in the client; requests are whole-recording multipart (latency-aware but not "live" transcription). Another notes a renderer-side WebSocket construction path, so newer builds may stream. Treat exact protocol as version-dependent; the pattern (single "voice flow" call, cloud-side comprehension+polish) is consistent.

### Windows / Linux clients
Public analyses only cover macOS; company docs confirm native apps on Windows and Linux with the same permissions model. Assumed equivalents: low-level keyboard hook (`WH_KEYBOARD_LL`) or input hook for hotkeys; UI Automation for context/cursor; SendInput/clipboard for insertion. (Consistent with what competing projects implement — see §4.)

### Mobile
- **iOS**: custom keyboard extension + "Allow Full Access"; the app converts the keyboard into a dictation-focused surface with swipe-to-type, Speak-to-edit (select text → speak instruction), language swipes, history. Constraint: keyboard extensions are sandboxed; network + shared container need open access; mic permission quirks (need to open host app first on iOS).
- **Android**: keyboard extension and later a floating bubble app-overlay + Accessibility service to insert into any focused editable field while the user keeps their preferred keyboard. Uses `InputConnection` when acting as IME (best capability: exact caret context read/write) or `ACTION_PASTE`/clipboard fallback outside IME.

---

## 3. Competitive landscape (commercial)

| Product | Platforms | Positioning / notable traits |
|---|---|---|
| **Wispr Flow** | Mac, Windows, iPhone, Android (no official Linux; community port exists) | Closest parallel. Cloud-first, custom in-house ASR models + fine-tuned Llama cleanup; markets <700ms p99 E2E latency; freemium (free plan, Pro $12–15/mo); Notetaker product line; API/WS service. Engineering blog describes context-conditioned ASR, personalized LLM formatting, correction-learning; 3.3k stars-tier competitor set. |
| **Superwhisper** | macOS, Windows, iOS, Android (Linux "coming soon") | Local-first: Parakeet/Whisper on-device, cloud models optional; "modes" with custom system prompts for polish; meeting notes on macOS; coding-agent integrations; paid yearly (~$85/yr reported), 3,000 free words. |
| **Aqua Voice** | desktop (mac/win) | Similar dictation + edit-by-voice; closed source; ~$8-10/mo tiers. |
| **Apple / Windows / Google built-in dictation** | native | Free, but verbatim transcription (fillers remain) and weaker formatting; no cross-app polish. |
| Others: **Monologue**, **Willow**, **Lazy**, **Voibe**, **Blazing Transcribe**, **Speechify-style** dictation apps, **Talon** (voice coding, X11-only) | — | Long tail; most single-platform, mac-centric; several leverage local Whisper/Parakeet. |

Pricing landscape for reference: Wispr Flow Free / Pro $12 monthly or $15 monthly-per-user tiers; our target should beat "sharp paywall + cloud-only" with either generous free tier or local-first option; research shows a large segment (offline/privacy-conscious, corporate, regulated) is under-served by all cloud-only players.

---

## 4. Open-source projects and their stacks (current state of the art)

| Project | Stars (approx) | Platforms | Stack | Notes |
|---|---|---|---|---|
| **OpenWhispr** | ~12k | macOS, Win, Linux + Expo mobile (iOS-focused) | Electron 41 + React 19 + TS + Tailwind, whisper.cpp + sherpa-onnx (Parakeet/Orukeet/Nemotron/Cohere), better-sqlite3; mobile: Expo SDK 55, expo-router, NativeWind, Zustand, Drizzle/expo-sqlite, whisper.rn | Full product: dictation, translation hotkey, AI agent, meeting transcription, notes; local or cloud BYOK or its own hosted plan ($2k words/wk free). Great reference for packaging/meet/tab features. MIT. |
| **Handy** | ~39k | macOS, Win, Linux | Tauri 2 (Rust) + React front; whisper.cpp + Parakeet GGUF via transcribe.cpp; Silero VAD; cpal audio | Offline-only, minimal UX; well-known known-issues (Wayland limitations, Fn only on Apple keyboards, Windows UIPI). MIT. |
| **Parler** | small but complete | macOS, Win, Linux | Tauri 2 + React; Rust backend modules (dictation loop thread, audio, providers, paste, overlay, shortcut via modifier event tap); local via transcribe.cpp (Parakeet/Whisper) or OpenAI-compatible cloud; own cloud plan | Superb docs (architecture.md) — covers clipboard restore rules, silence detection, pending retries, device-code auth. |
| **OpenLess** | ~4k | macOS, Win, Linux, Android, iOS, HarmonyOS | Shared Rust core (`openless-core`) with per-platform hosts: Tauri 2 + React (mac/win/android), egui native (Linux), SwiftUI/UIKit + KeyboardKit (iOS), ArkTS (HarmonyOS); Kotlin IME (Android), TSF IME (Windows) | Best example of a truly multi-platform architecture; ~30 ASR providers + ~20 polish providers; streaming insertion; style packs. AGPL. |
| **SayIt** | ~0.5k | Win, macOS | Tauri 2 + React/TS; GGUF local ASR (Parakeet etc.) via C++ engine, CUDA/Metal; FastAPI WebSocket server option (Qwen3-ASR + vLLM) | Shows server-side streaming option; per-app rules, hotwords, context-aware, overlay. AGPL. |
| **Voquill** | ~1k | mac, win, linux | Tauri 2 + React (TS), sidecar transcription, Whisper local or cloud BYOK, glossary/replacement rules, enterprise gateway | AGPL; focus on AI polish editing presets. |
| **Voz** | small | macOS 26+ (Apple Silicon) | **Native Swift** app, extension system in TypeScript (run in sandboxed JS runtime), Parakeet on-device (25 languages) + Apple's on-device cleanup; CLI + MCP | Shows single-platform native depth vs the price of no cross-platform coverage. |
| **FreeFlow** | ~3k | macOS only | **Native Swift** (AppKit/SwiftUI) | Clean Swift reference for hotkeys (modifier event taps), context capture, insertion, custom embeddings. MIT. |
| **Blazing Transcribe (open)** | small | macOS | Native Swift, Parakeet local | Offline-first mac dictation. |
| Others: **Scriva**, **SpeakoFlow**, **vocalinux/VOXD** (linux X11/wayland), **WhisperDictation**, **whisper_streaming / SimulStreaming (UFAL)**, **Talon** (voice programming) | — | — | Mostly Whisper.cpp wrappers; teach llama/ASR integration patterns | whisper_streaming implements LocalAgreement-2 policy for incremental decoding. |
| **rustype / Arion / gaojunbin/Typeless (clones)** | — | — | Electron/Tauri attempts to rebuild Typeless; documents included useful reverse-engineering methodology | Read their docs as "what to clone and what actually matters". |

### What the stack choice landscape tells us
- **Cross-platform desktop dictation** converged on **Tauri 2 + Rust backend** (Handy, Parler, SayIt, OpenLess, Voquill, +FreeFlow choice only failed for lack of cross-platform). Reasons:
  - Small installer, low memory vs Electron (Electron ~250-400MB+ baseline; Tauri ~40-80MB).
  - First-class Rust ecosystem for native FFI-free access to OS APIs or clean crates: `arboard` (clipboard), `enigo` (UGI), `global-hotkey`, `cpal` (audio).
  - Native-sidecars less painful than in Electron.
  - Still supports web-tech UI (React/Vue/Svelte), which is the UI productivity sweet spot.
- **Electron is where you inherit mature stable patterns** (OpenWhispr) and where many teams land first because JS+Swift/C# helpers via koffi works; but you pay installer/update/memory and need 5+ native dylibs just for keyboard/clipboard/context.
- **Mobile** never uses Tauri's WebView semantics for the typing surface: you always need a native keyboard extension (iOS) or IME/overlay (Android), because you need `InputConnection`/`textDocumentProxy`-level integration plus offline/standby behavior; thick mobile UI frameworks: OpenWhispr chose Expo (RN), OpenLess native Swift(iOS)/Kotlin(Android)/ArkTS. Native core APIs (KeyboardKit on iOS) are the fastest path.
- Rendering heavy UI in WebViews on mobile is a liability; keep UI-native there.
- **Shared-core architecture** (OpenLess's `openless-core`) is the most credible answer to "same behavior on 5+ platforms": one Rust business core; thin per-platform hosts.

---

## 5. Technical building blocks (the pipeline, de-jargon'd)

```
mic → VAD/endpointing → (streaming?) ASR → transcript
   → LLM cleanup & rewriting (context: app, caret text, dictionary, style, formality)
   → insertion into focused app (clipboard-paste or AX/IME direct write)
   → history (local SQLite) → optional sync
```

### 5.1 Audio capture
- 16 kHz mono is the canonical ASR input (decoders resample anyway; Whisper-family models ingest 16k).
- Always **device change handling** (mic unplugged, AirPods switch to HFP making quality drop), level meter for UI, and "no sound watchdog" (a muted/unplugged mic must not let users talk for 30s thinking it's working — Parler/Handy both implement this).
- Disable **echo cancellation / noise suppression / AGC in capture constraints** for dictation (Typeless observed doing this; otherwise phones-style processing degrades ASR). Do optional software-level ducking of system output ("Mute when dictating").
- **Trim silence** (VAD-based head/tail trim) is mandatory: Whisper hallucinates text ("Thank you.") on silent audio; VAD also saves bandwidth/billing.

### 5.2 Speech recognition — batch vs streaming
Three viable modes:
1. **Record-then-send (batch)**: record up to N minutes, encode Opus/WAV, one HTTP multipart call to an ASR API (OpenAI-compatible `/v1/audio/transcriptions`). Typeless v2.8.x observed doing this in a single "voice flow" call; Parler/OpenWhispr/FreeFlow do this per take. Simplest; latency = network + decode of whole take (seconds). Good enough if your ASR provider is fast (Groq/OpenAI batch models) and recordings short.
2. **True streaming** (WebSocket PCM chunks or AudioWorklet chunk cadence): partial hypotheses while speaking; needed for "text flows as you talk" UX and sub-second tail latencies. Whisper-family models stream badly; purpose-built streaming models (NVIDIA Nemotron Microsoft/VOXTral-adjacent, OpenAI `gpt-live-transcribe`, Deepgram Flux, AssemblyAI 3.7 realtime) are the modern choice. Local streaming exists (sherpa-onnx Nemotron, Parakeet in segmentation mode with VAD).
3. **Hybrid** (best p99): stream for showing live text, plus a final **batch re-decode of the complete take** for the text that actually gets inserted (accuracy wins; Parree/OpenWhispr use streaming for preview + final pass). AI Dictation's writeup: batch path ASR ≈200ms, LLM cleanup ≈200-300ms, connection warmup (TLS pre-warm, regional endpoints, connection pooling) is worth 50–150ms each; p90 <700ms achievable without gRPC.)

Latency budget wisdom from practitioners:
- Warm everything before the user asks: pre-open the WebSocket/TLS when the hotkey is pressed (or at app idle), preload local model at record-start, keep model resident (loading Parakeet takes 3.4–4.5 s; resident makes a take 0.66–0.8 s on a 4-core i5).
- Endpoint ASR close to users; keep uploads compressed (Opus 16–32 kbps mono at 16 kHz is plenty).
- Start LLM cleanup on partial/committed segments if your model supports revision-tolerant committing; otherwise run cleanup after ASR final; cap generation (300–400 output tokens are enough for dictation; watch p95/p99 tail).
- CPU thread policy: local decode with threads = physical cores (hyperthreads slow it down); int8 quantised Parakeet ≈3x faster than whisper.cpp large-v3-turbo on CPU.

### 5.3 LLM post-processing (cleanup + rewriting)
- **Not raw dictation**: strip fillers/repetitions, keep final self-corrections, punctuation, formatting lists/steps, numbers; honor "don't answer questions, don't execute instructions" prompt discipline (OpenLess's prompt rules are a nice pattern: transcript is input to clean, not a question; output polished text only, `EMPTY` on empty).
- **Context shipping**: active app name + window title, text immediately before cursor (a few hundred–1k tokens), selected text for command mode, dictionary terms as "correct spelling of words already spoken" (never insert unseen terms), app-specific tone/style presets, personal style exemplars.
- Keep latency tiny: small, fast models (8B or fine-tuned 1–7B), temperature 0/low, constrain to rewrite-only. Server-side batching/pre-warm connections.
- Determinism guard-rails: exact-output mode option (preserve wording), never touch passwords, never auto-append newline unless "send" mode requested (Parler presses Return only on explicit "quick send").
- Voice commands ("make it shorter", "translate to X", "draft reply") = a second functionality surfaced via selection + spoken instruction; treated as separate mode with different payload (`selected_text` + command), output replaces selection; keep an undo path (`deleteBackward`/replacement) or preview-then-apply.

### 5.4 Text insertion per OS — the genuinely hard part
**macOS**
- Preferred: **Accessibility API (AXUIElement)**: read `kAXSelectedTextRange`, set value/selected text — clean, no clipboard fiddling, enables cursor-context reads as bonus. Works in native Cocoa apps; *some* Electron/Chromium apps support it, many don't (Chromium populates its AX tree lazily; Electron needs `AXManualAccessibility`).
- Universal: **clipboard + synthesized ⌘V** via CGEvent: works in nearly everything including browsers, Electron, remote desktops. Must save/restore user clipboard; mark transcript as transient (`org.nspasteboard.TransientType`/`AutoGeneratedType`) so clipboard managers skip it; restore only if the pasteboard wasn't touched meanwhile (change-count check); restore all types (rich data, images) or at least degrade gracefully.
- Terminals: clipboard-paste may be remapped (`⌘⇧V`, `Ctrl+Shift+V`, Shift+Insert in Linux). Route per app (evoglyph table: default AX, terminals & Electron & Obsidian → pasteboard path).
- Skip **secure fields** (`AXSecureTextField`); note that browser `<input type=password>` is invisible to AX-shaped detection — don't offer dictation into password managers as a promise.
- Permissions: **Accessibility** (AX reads + CGEvent injection + event taps), **Input Monitoring** (CGEventTap in some cases), **Microphone**. App Store distribution is effectively blocked for the AX-dependent experience (sandbox forbids it) — must distribute notarized/Developer ID outside the Mac App Store, or ship a degraded App Store build. Cursor-caret positioning: `kAXBoundsForRangeParameterizedAttribute` for overlays.
- Event-tap gotcha: macOS disables slow taps ("disabled by timeout/user input") → watchdog restart; Fn/Globe key requires IOHIDManager-level tap; third-party keyboards' Fn sends nothing at HID level (Apple-only behavior) → don't promise Fn works everywhere.

**Windows**
- Clipboard + SendInput of paste shortcut (`Ctrl+V`, or Shift+Insert/Ctrl+Shift+V in terminals CMD/PowerShell/Windows Terminal variations) — same save/restore discipline; clipboard format types: also HTML format for rich text; `SendInput` Unicode typing possible but layout/IME-dependent.
- **UI Automation** for cursor context (`TextPattern2` caret ranges) and for direct value setting in some apps; skip password controls; terminals often expose little.
- **UIPI (integrity levels)**: can't inject into elevated/admin apps from a non-elevated process — handle by re-running elevated copy or reporting "copied to clipboard, paste manually".
- Hotkeys: low-level keyboard hook (`WH_KEYBOARD_LL`) or `global-hotkey` crate; Fn is firmware-only on many keyboards (untouchable); RightAlt is a safe default. Watch UAC prompts, remote sessions.
- Distribution: not MS-signed by default → SmartScreen warnings; use `tauri.conf` updater or Squirrel/WiX; prefer `.msi` + winget packaging later.

**Linux**
- X11: hotkeys via X11 grabs; injection via XTEST/clipboard (xclip/xsel holding selection is required for clipboard persistence!); `enigo` crate works.
- Wayland: compositors intentionally block injection and global shortcut grabs. Real options:
  - **XDG Desktop Portal RemoteDesktop** ("type" permission): works on GNOME 48+, KDE 6 (consent dialog once; grant token survives restarts; KDE forgets on reboot). Zero setup on stock desktops now.
  - Fallback tools: `wtype` (wlroots/KDE), `ydotool` (any compositor, needs uinput/`input` group + daemon) — document as guided setup.
  - Clipboard needs `wl-copy`/`wl-paste` for persistence.
  - Global hotkeys: **GlobalShortcuts portal** (GNOME 48+, KDE 6+) or ask desktop/WM config to bind CLI (`--toggle-dictation`) or send a unix signal; document per-DE instructions (GNOME custom shortcuts, KDE custom shortcuts, sway/hyprland binds).
  - Focus detection is impossible by design → keep an "insert where caret is" guarantee narrow; per-app paste-key routing table helps (terminals want Shift+Insert etc.). VS Code/Cursor integrated terminals have image-paste quirks (Ctrl+V reads image) — prefer Shift+Insert / explicit settings.
- Dependency-sensitivity of the whole injection layer is the single biggest platform risk on Linux; plan to ship helper-utility auto-install ("guided setup"), or accept clipboard fallback with a clear "press Ctrl+V" toast.

**Mobile insertion**
- **Android**: as an IME you get `InputConnection` → `commitText` (and `commitTextAt`)/`setComposingText` and can read the field around the caret (`getTextBeforeCursor`/`getTextAfterCursor` — great for context, gate it for passwords/`IME_FLAG_NO_PERSONALIZED_LEARNING`). Outside IME: Accessibility node injection (`ACTION_SET_TEXT` unsupported in many apps; use `ACTION_PASTE` with clipboard fallback) or floating bubble + accessibility clipboard paste. Mark: users increasingly keep their own keyboard; bubble approach is a better product wedge (with overlay + accessibility permissions), while a full "keyboard replacement" is the most robust insertion path.
- **iOS**: custom keyboard extension (with Allow Full Access) using `textDocumentProxy` (`insertText`, `documentContextBeforeInput`, `selectedText`) — the only way to type into arbitrary apps; sandbox forbids network by default; Apple reviews networked-keyboard guidelines strictly ("don't retain keystrokes/voice beyond need"). Handlers: dictation start from keyboard → host app or extension does audio+network; need shared App Group container, entitlement for mic; iOS may show dictation lockscreen quirk; users must enable Full Access repeatedly. Also consider widget/Shortcuts/Action button entry (record in host app, copy to clipboard, user pastes or use `UIPasteboard` then advise paste) — less seamless but reaches apps where keyboard disabled.
- Android IME also gives system dictionary integration, autofill interplay, and input type handling; decode inputType flags carefully (number pad, email, url adjust UI); hardware keyboard events can bypass soft keyboard entirely (then bubble/accessibility path is needed).

### 5.5 Global hotkey capture (matrix)
| Platform | Best mechanism | Constraints |
|---|---|---|
| macOS | CGEventTap (Input Monitoring) or Carbon `RegisterEventHotKey` (no modifiers API); IOHIDManager tap for Fn/Globe | Taps auto-disable on slow callback; modifier-only chords need custom work if you want Fn or single-key push-to-talk; AX permission needed for App Store-blockers |
| Windows | `WH_KEYBOARD_LL` hook or `RegisterHotKey` (no modifier-only combos, collides with apps) | Admin-elevated apps unobservable by non-elevated process; bare modifier combos unsupported by `RegisterHotKey` → low-level hook required |
| Linux X11 | XGrabKey / grabs; `global-hotkey` ok | global grab fights with other apps; use recorder to pick non-conflicting; Wayland session: X11 grabs don't reach native Wayland apps |
| Linux Wayland | GlobalShortcuts portal; CLI/signal-triggered (user-configures) | GNOME 48+/KDE6 have portal; others need WM config |
| iOS | none (no global hotkey on system) | in-app only, widget/hardware keyboard avoided |
| Android | none (no global hotkey) | use bubble/overlay or IME button |

Gesture design lesson from Parler: a bare modifier (Fn/right-⌘) key needs a 150ms "alone" hold, or clean tap, to avoid hijacking chords; double-tap = latch; Esc×2 = cancel; recording continues if you switch windows; the text goes where focus is *when you release* unless "original field lock" opt-in.

### 5.6 Model options for ASR (local and cloud) — reference points
- **Local**: NVIDIA Parakeet TDT 0.6B v3 (int8 ONNX via sherpa-onnx or transcribe.cpp, ~660–800MB): 25 EU languages, fast on CPU (~0.17 RTF warm). **Nemotron Streaming** variants (live partials, single-pass commit). Whisper variants (whisper.cpp / faster-whisper): widest languages, prompt-biasing for vocabularies, but slower and batch-y; large-v3-turbo ≈30s CPU for a 24s clip. Newer: Cohere Transcribe (2.7GB, 14 langs, best WER per OpenWhispr), Moondream Parakeet-redux (178MB, 113x real-time claims), Orukeet (multilingual Parakeet fine-tune). Voz uses Parakeet 25 langs fully on-device + Apple's on-device cleanup — proving the offline product is real.
- **Cloud (some observed prices, Nov 2026)**: OpenAI `gpt-live-transcribe` streaming $0.017/min (~$1.02/hr); AssemblyAI realtime $0.30–0.45/hr; ElevenLabs Scribe v3 Live $0.22/hr; Soniox realtime $0.12/hr; Deepgram Nova-3 ~$0.46/hr list; Groq whisper-large-v3-turbo much cheaper per hour (~$0.04–0.11/hr from Groq's own rates); Azure/AWS standard ~$1-1.5/hr; Google STT $0.96/hr. Translation/other API usage on top.
- **LLM cleanup** cost/latency: use small/fast model (distillation fine-tune on dictation-cleanup pairs is the Wispr approach; you can accept a good general model like GPT-4o-mini class or a local llama-family 8B at low temp). Output is tiny (≤400 tokens), so per-dictation cost is negligible at ~$0.000X.

### 5.7 Data & privacy architecture (what to copy, what to improve)
Typeless: zero-retention cloud, context blacklist by app/domain, on-device history by default, optional history sync encrypted; audio never stored cloud-side. Their blacklist check-before-read pattern is right; the fact that cursor text is still read for blacklisted apps is a known weak spot. For our product:
- Default **on-device or user-key** processing (like OpenWhispr/Voz/Handy positioning) plus optional paid cloud; or cloud-first like Typeless/Wispr. Choose based on target audience; be aware that Regulated/HIPAA audiences require no-audio-retention + BAA, and "your app reads all my text" trust barrier is real (it IS what makes the product magic — you must communicate why and gate password fields).
- Encrypt API keys in OS vault (Keychain/DPAPI/libsecret) — never plaintext JSON; Parler/OpenLess patterns.
- Provide pseudonymous/opt-out telemetry; keep history local by default with retention control.

### 5.8 Research references worth linking
- **Whisper-Streaming (UFAL, ACL 2023, "Turning Whisper into Real-Time Transcription System")** — LocalAgreement-2 policy for stable incremental decoding of non-streaming models; also SimulStreaming successor.
- **Typeless official docs** — dictate/translate/ask-anything/personalization/history-dictionary/settings pages (behavior + interaction spec source).
- **Wispr Flow engineering blog + Baseten case study** — <700ms p99, Llama fine-tune cleanup, TensorRT-LLM, chains.
- **Apple: Configuring open access for a custom keyboard**; **AXUIElement & secure-input documentation (Watch out for AXSecureTextField, IsSecureEventInputEnabled)**.
- **XDG DesktopPortal: GlobalShortcuts & RemoteDesktop compat specs** (Wayland strategy).
- Reverse-analysis write-ups of Typeless v2.8.x and v1.0.2 (process model, dylibs, audio pipeline, `ai/voice_flow` payload observations).

---

## 6. Cross-platform tech-stack decision

### The realistic shape of a multi-platform voice typing product
You need two very different kinds of software:
1. **The desktop dictation client** — hotkey capture, audio, insertion, context, tray/menubar, updater. Native OS integration is 70% of interface risk.
2. **Mobile companions** — keyboard/IME extensions and floating bubble; these inherit mobile-OS rules that have nothing to do with the desktop stack.

### Options

**Option A — Tauri 2 (Rust core) + React UI on desktop; native mobile**
- Evidence: Handy, Parler, SayIt, OpenLess, Voquill all took this route for new projects; it is the ecosystem default for low-footprint dictation apps in 2026. Tight binaries (~10–20MB), low RAM, direct FFI to platform APIs (arboard, enigo, global-hotkey, cpal, tauri plugins), updater built-in (signed JSON manifests). Rust also gives us a **shared business core** option a la OpenLess (`openless-core`): dictation engine, providers, prompts, dictionary, history behind ports, with hosts for macOS/Windows/Linux/Android (Tauri mobile)/iOS (native SwiftUI + KeyboardKit).
- Risks: smaller hiring pool vs JS; you still write ~4 native glue layers (macOS AX/CGEvent tap, Windows LL/UI Automation, Linux portals/tools); Tauri mobile is early for keyboard extensions (they are unspecified interfaces; you can't use WebView as IME — you'd write native keyboards anyway).
- Verdict: **strong best-fit** if team is comfortable with Rust; matches what the best OSS products have proven.

**Option B — Electron + Node + helpers (Typeless/OpenWhispr model)**
- Pros: fastest iteration, huge ecosystem, proven dictation patterns (OpenWhispr ~12k stars ships functionally complete), web audio pipeline simplicity (AudioWorklet + Opus), native helpers via Swift/C#/DLLs.
- Cons: 300–400MB+ memory, ~100MB+ installers, need many native helpers anyway, macOS AX/self-tap caveats identical; no shared core across mobile.
- Verdict: reasonable if team is all TS and time-to-market is priority #1; OpenWhispr is proof it works at scale.

**Option C — Fully native per platform (Swift on macOS/iOS, WinUI/C# on Windows, GTK/Qt or Rust on Linux, Kotlin/Swift on Android)**
- Pros: best possible integration & performance; Voz/FreeFlow show what native speed feels like.
- Cons: 5 codebases; UI/behavior drift; you'll rebuild the dictation engine 5 times. Only sensible for a narrow premium mac-first product.
- Verdict: not for a cross-platform v1 unless shipping mac-only first.

**Option D — Hybrid (recommended):**
- Rust shared core crate (dictation session state machine, provider adapters, prompt pipeline, history, dictionary, settings, sync crypto) + thin hosts.
- Desktop hosts:
  - macOS/Windows/Linux: Tauri 2 host for main UI/settings/tray + native adapters for the input path — this mirrors **OpenLess's exact split** and avoids the WebView-in-IME trap; Linux native host (egui or gtk) if WebKitGTK keeps hurting (Handy's WebKitGTK 2.54 DMA-BUF pain is documented; OpenLess reverted to egui on Linux).
  - iOS: **native SwiftUI/UIKit app + custom keyboard extension (KeyboardKit) calling the shared core via a thin bridge** (OpenLess pattern; OpenWhispr uses Expo keyboard plugin — also viable but more glue).
  - Android: **native keyboard/IME (Kotlin) + optional floating bubble overlay** calling shared core via JNI/UniFFI bridge (OpenLess pattern) or Tauri Android host for the app shell.
- Frontends: React + Tailwind (shadcn) for desktop windows; native UI on mobile.
- Access the LLM/ASR provider APIs through provider adapters behind an interface so users can BYOK (OpenAI-compatible + others) and we can host a managed option.
- Use the shared core to implement: streaming manager (VAD + streaming ASR + commit), cleanup prompt pipeline with tests, dictionary hotwords injection per-provider, insertion strategies per platform, clipboard hygiene, retry/pending queue, history schema, sync.

### Cross-platform caveats to design for from day one
1. **Insertion is the product.** Build the insertion abstraction (clipboard restore, per-app paste-key routing, AX/TSF/InputConnection paths, secure-field skip, clipboard-manager transient marking) first; everything else is replaceable.
2. **Hotkey abstraction with per-backend capture** (asset-tap vs hook vs portal) plus gesture decoder (hold/tap/double-tap/Esc-cancel/chord passthrough) — shared across platforms.
3. **Permissions & onboarding flows per OS** are a big chunk of work (mac AX toggles needing restart, Win injected-vs-elevated, Linux helper install, iOS full-access enabling, Android special permissions) — plan UX for graceful degradation and a "copied to clipboard" fallback, never silent no-op.
4. **Latency budget**: target end-to-end (stop-speaking → text inserted) p90 ~1s desktop with cloud ASR+LLM; stream previews if you want the "alive" feel. Do connection warm-up and model residency; measure per release.
5. **Wayland-first Linux plan** (GNOME 48+ / KDE 6 portals; helper-tool ladder) and honest docs; Fedora/Ubuntu are Wayland-only now.
6. **Distribution**: macOS notarized DMG (AX requires non-sandboxed → no MAS), Windows MSI+SmartScreen signing + winget later, Linux AppImage/deb/Homebrew cask; Tauri updater or Sparkle/Squirrel equivalents; auto-update signed manifests.
7. **LLM/ASR provider matrix** and BYOK vs managed; our APIs are the default, but the abstraction is the moat.

---

## 7. Recommended direction (one paragraph, opinionated)

**Build a Tauri 2 + Rust shared-core product:**
- Shared Rust core: session state machine, audio capture/VAD/streaming manager, provider adapters (our ASR + LLM APIs as defaults, BYOK later), cleanup/rewrite prompt pipeline (tested with golden cases), dictionary, history, sync (encrypted, opt-in), CLI + MCP surface.
- Desktop: React/Tailwind windows in Tauri 2 for macOS/Windows (tray, overlay pill, settings, history, onboarding); a Linux host reusing the core (start Tauri; keep an egui/native escape hatch if WebKitGTK proves hostile). Native adapters: macOS AX+CGevent+IOHID tap; Windows keyboard hook+SendInput+UIA; Linux XTest/clipboard and Wayland portals.
- Mobile phase 2: native iOS app + custom keyboard (Swift + KeyboardKit) and Android IME + bubble (Kotlin), both on the shared core via UniFFI.
- Model strategy: **cloud-first with our APIs** for v1 (fastest accuracy/latency story; matches Typeless/Wispr experience we're cloning), with an offline local mode added later (Parakeet/Nemotron via sherpa-onnx) as a privacy differentiator — exactly the gap the market shows.
- Trust: zero-retention audio, context honored on-device for sensitive apps, on-device history by default, transparent context-in-use indicators.

This closely mirrors what actually works in the wild (OpenLess architecture + Parler operational rigor + OpenWhispr product scope) while avoiding Electron's footprint and multi-codebase native's duplication.

---

## 8. Open questions to settle before build
1. **Which ASR/LLM APIs do we have?** (Provider choice determines streaming strategy, languages, hints/hotwords support, and per-dictation cost model; needs API keys/scopes to benchmark p90 latency before committing.)
2. Target platforms for v1 (recommend: macOS + Windows first; Linux in 4–8 weeks with portal work; mobile in phase 2).
3. Positioning: cloud-only like Typeless vs hybrid/local-first like Handy/Voz (affects model & privacy investment).
4. Monorepo layout: single Cargo workspace + `apps/desktop` (Tauri) + `apps/mobile-*` + `crates/core` + `crates/adapters-*`.
5. Team languages: if Rust is weak, fall back to Electron (Option B) — but keep the shared-core discipline via TS packages + native helpers.

## 9. Source index (primary)
- Typeless: typeless.com (home, downloads, pricing, data-controls, use-cases, help/quickstart/*, help/troubleshooting/dictation-limit, help/faqs, help/installation-and-setup, android bubble release note)
- Typeless app store listing; reviews (blazingfasttranscription, laxis, voz.tools comparison)
- Reverse analyses: "Architecture Behind Typeless" (v1.0.2), rustype REVERSE_ENGINEERING.md + THINKING_PROCESS (v2.8.2), Arion research doc (v2.8.1)
- Wispr Flow: technical-challenges post, Baseten case study, pricing, product site; reddit latency write-up
- Superwhisper docs (get-started, models, privacy)
- OSS: OpenWhispr README/docs/mobile; Handy README (incl. Wayland/Fn/limitations); Parler README + docs/architecture.md; OpenLess README + docs/architecture.md + docs/android-ime.md + insertion.rs; SayIt README; Voquill README; Voz site + README; FreeFlow README
- Platform docs: Apple custom keyboard open access; AXUIElement / AXObserver articles; evoglyph text-injection doc; XDG Portal GlobalShortcuts/RemoteDesktop; whisper_streaming paper (arXiv 2307.14743); OpenAI realtime transcription + gpt-live-transcribe model pages; AssemblyAI realtime STT API comparison; Windows SendInput/UIPI references
