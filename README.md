# oh-my-voice-type

Voice typing app (macOS + Windows): hold a hotkey anywhere, speak, release — cleaned, polished text appears at your cursor, in any app.

Phase 1 of the plan in [`docs/project-plan.md`](docs/project-plan.md); research in [`docs/research-report.md`](docs/research-report.md).

## Stack

- **`crates/ovt-core`** — framework-free Rust engine: dictation pipeline, prompt builders, dictionary/hotwords, OpenAI-compatible ASR + LLM provider adapters, SQLite history, settings model, hotkey gesture decoder, WAV/silence utilities. Fully unit- and integration-tested.
- **`apps/desktop`** — Tauri 2 host (Rust) + React/Vite/Tailwind UI:
  - Platform layer (`src-tauri/src/platform/`): Windows low-level keyboard hook + SendInput paste + per-app paste routing + UIPI detection; macOS CGEventTap + CGEvent paste + permission probes.
  - Audio: capture on a dedicated thread (WASAPI streams aren't `Send`), level metering, 16 kHz WAV prep with VAD trim.
  - Session glue: hotkey → capture → ASR → LLM cleanup → insertion → history, streaming state to the UI (main window + floating pill).
  - AI cleanup: Unisound text generation by default (`u2-flash`, `https://maas-api.unisound.com`, OpenAI Chat / OpenAI Responses / Anthropic Messages formats), or a custom endpoint in any of the three formats. User-toggleable on/off.
  - Commands: settings, history, dictation control, API keys (OS vault via keyring), permission/device status.

## Repository layout

```
crates/ovt-core/          # shared engine (used by desktop today, future mobile/Linux hosts)
apps/desktop/
  src/                    # React UI (Tauri IPC via src/lib/api.ts)
  src-tauri/
    src/
      lib.rs main.rs      # app bootstrap, command registry
      commands.rs         # settings/history/session commands + AppState
      dictation.rs        # hotkey wiring + session pipeline glue
      audio.rs            # audio thread + AudioHandle
      keys.rs             # API keys → OS credential vault
      platform/           # windows.rs / macos.rs OS adapters
    capabilities/ icons/ tauri.conf.json
docs/                     # research report, project plan, dev guide, acceptance checklist
scripts/gen-icons.mjs     # regenerates app icons (zero deps)
```

## Getting started

Prereqs: Node 20+, Rust stable, and on Windows the MSVC build tools; on macOS Xcode CLT.

> **Where to run commands.** The repo root is a Cargo workspace only — there is **no `package.json` at the root**. Every `npm` command must run inside `apps/desktop`, and every `cargo` command from the repo root.

```bash
# 1. install frontend deps (once)
cd apps/desktop
npm install

# 2. start the dev app (run from apps/desktop — same terminal as above)
npm run tauri dev
```

If you prefer to stay at the repo root, use:

```bash
npm --prefix apps/desktop run tauri dev
```

`npm run tauri dev` starts Vite on `http://localhost:5173` (via `beforeDevCommand`), compiles the Rust workspace (`crates/ovt-core` + `apps/desktop/src-tauri`), then opens the Tauri window with hot reload for the UI.

Other entry points:

| Command | Run from | Purpose |
| --- | --- | --- |
| `npm run dev` | `apps/desktop` | UI only in a browser (no Tauri APIs) |
| `npm run build` | `apps/desktop` | `tsc --noEmit` + Vite production build into `dist/` |
| `cargo test -p ovt-core` | repo root | core engine test suite |
| `cargo check -p ovt-desktop` | repo root | compile the Tauri host |

First run setup:

1. **Settings → Speech recognition**: base URL (`https://…/v1`), model, and API key (stored in Windows Credential Manager / macOS Keychain).
2. **Settings → AI cleanup**: same for the chat completions endpoint.
3. **Home**: pick the dictation hotkey (Windows default `Right Alt`; macOS default `Right Option`), choose hold/tap smart gestures, click *Test a dictation* and speak.

## Verification

```bash
cargo test -p ovt-core      # 28 tests: pipeline, prompts, hotkey decoder, dictionary, history, WAV
cargo check -p ovt-desktop  # host compile (Windows verified locally; macOS in CI)
cd apps/desktop && npm run build   # tsc + vite production build
```

## Releases

Update the version in `Cargo.toml`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`, then push a matching `vX.Y.Z` tag. The release workflow builds Windows x64 and macOS Universal installers, signs the updater artifacts, and uploads them to a draft GitHub Release with `latest.json`.

Publish the draft release manually after checking both installers. The in-app updater then reads `https://github.com/sileneer/oh-my-voice-type/releases/latest/download/latest.json`. The repository must keep the `TAURI_SIGNING_PRIVATE_KEY` Actions secret configured, and the private key must be backed up separately.

## Status / known limits (v1)

- Dictate mode end-to-end. Translate + Edit Selection prompts exist in core; UI wiring is next.
- Insertion: clipboard + synthetic paste with clipboard restore; AX (macOS)/UIA (Windows) direct insertion and opt-in caret context are milestone 2.
- Wayland/Linux and mobile hosts are phase 2/3 (the core crate is already host-agnostic).
- Windows code signing and macOS notarization are not configured yet; updater artifacts are signed separately.
- Crash-safe pending queue is still pending (see plan milestones M4–M6).
