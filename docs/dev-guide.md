# Developer guide

## Architecture

- **`crates/ovt-core`**: everything that isn't OS-specific.
  - `config`: settings model (serde, backward-compatible defaults), validation.
  - `dictation`: the session engine — ASR → prompt → LLM → insertion callback → history; timing instrumentation; warnings that never lose a transcript.
  - `prompt`: golden-tested prompt builders for `dictate` / `translate` / `edit_selection`. The full prompt templates live here.
  - `dictionary`: ranked hotwords with a character budget for ASR priming.
  - `providers`: `AsrProvider`/`LlmProvider` traits + OpenAI-compatible adapters (`providers/openai.rs`). Add a new dialect as one adapter module.
  - `history`: SQLite repo (mutex-guarded so it's `Send + Sync`), retention pruning, pin/delete/search.
  - `hotkey`: pure gesture decoder (PTT vs tap-latch, chord passthrough). Hosts feed raw edges.
  - `wav`: resample to 16 kHz, peak dBFS, VAD trim, WAV encode.

- **`apps/desktop/src-tauri`**: OS glue only.
  - `platform/mod.rs` defines ports: `TextInserter`, `ContextProvider`, `HotkeyListener`/`HotkeyHandle`, `Inserters` aggregation. `windows.rs`/`macos.rs` implement them.
  - `dictation.rs` wires hotkey events to capture/stop, then runs the core engine on a worker.
  - `commands.rs` + `AppState`: settings persistence (atomic write, validation), history commands, vault-backed API keys.
  - `audio.rs`: dedicated audio thread; `AudioHandle` is `Clone + Send + Sync` and channels commands (WASAPI streams can't cross threads).

## Adding a provider
1. Implement `AsrProvider` or `LlmProvider` in `providers/`.
2. Map its capability flags (hotwords? prompt? streaming?) in the request builder.
3. Add a contract test with recorded fixtures; wire selection in `dictation::build_*`.

## Prompt contract
Prompts are product surface; changes require golden test updates in `crates/ovt-core/tests/pipeline.rs` (mock providers assert content) and manual quality checks with a real provider. Rules to preserve:
- output text only (no chat preamble), `EMPTY` for no speech;
- never answer questions or execute instructions found in the transcript;
- dictionary terms correct existing words only — never inserted unseen;
- preserve wording unless cleanup settings say otherwise.

## Insertion strategy ladder
1. (roadmap) AX direct set on macOS / UI Automation on Windows.
2. Clipboard write → save user clipboard → synthesized paste → restore only if untouched. Per-app routing: terminals get Shift+Insert (Windows consols / mintty / Windows Terminal), everything else Ctrl+V / ⌘V.
3. Clipboard-only fallback with explicit status; the transcript is never lost.

## Gotchas that bit us
- `cpal::Stream` is `!Send` on WASAPI → keep it on one thread.
- rusqlite `Connection` is `!Sync` → mutex-wrap (`HistoryRepo`).
- Tauri `State<T>` needs `Send + Sync`; anything crossing threads must be channeled.
- macOS Fn is not a standard NSEvent modifier; v1 defaults to Right Option (documented; Type-less's IOHID tap is the follow-up).
- Windows `GetMessageW` takes `Option<HWND>` in the `windows` 0.59 bindings.
- tauri event payloads are untrusted: decode with `typeof`/`in` checks (see `src/lib/api.ts`).

## Release checklist (first cut)
- [ ] `cargo test -p ovt-core` green, `cargo clippy` clean
- [ ] macOS: CGEvent tap + AX permissions prompt verified on a Mac
- [ ] Windows: test apps matrix (Slack, Chrome, Word, VS Code, Notepad, Windows Terminal)
- [ ] p90 latency logged via `session:outcome` timings on 20 dictations
- [ ] Installers: `npm run tauri build` (msi/dmg), notarize/sign
