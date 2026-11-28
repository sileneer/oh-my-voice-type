# oh-my-voice-type

Voice typing app for macOS and Windows: hold a hotkey anywhere, speak, release — cleaned, polished text appears at your cursor in any app.

Phase 1 of the plan in [`docs/project-plan.md`](docs/project-plan.md); research in [`docs/research-report.md`](docs/research-report.md).

## Download and install

Download the latest installer from the [GitHub Releases](https://github.com/sileneer/oh-my-voice-type/releases) page:

- **Windows (x64)**: `oh-my-voice-type_*_x64-setup.exe` (per-user NSIS installer — installs under `%LOCALAPPDATA%` with no administrator privileges or UAC prompts on install or update).
  - Because Windows code signing is not yet configured, Microsoft Defender SmartScreen may show *"Windows protected your PC"*. Click **More info → Run anyway** to proceed.
- **macOS (Universal — Apple Silicon & Intel)**: `oh-my-voice-type_*_universal.dmg` — open the disk image and drag `oh-my-voice-type.app` into **Applications**.
  - Because Apple Developer ID signing and notarization are not yet configured (the bundle uses an ad-hoc signature), macOS Gatekeeper may warn that the app is damaged or from an unidentified developer after downloading from the web. Clear the quarantine attribute once in Terminal:
    ```bash
    xattr -rd com.apple.quarantine "/Applications/oh-my-voice-type.app"
    ```
  - On first use, macOS will prompt for **Microphone** access. Global hotkeys and pasting into other apps also require **Accessibility** and **Input Monitoring** in **System Settings → Privacy & Security**.

Verify installer checksums against `SHA256SUMS.txt` attached to each release.

## Privacy and security

- **Bring your own provider**: Recorded audio and transcripts are sent directly from your machine to the speech recognition (ASR) and optional AI cleanup (LLM) providers you configure. There is no intermediary backend or telemetry service.
- **Local credential vault**: API keys are stored only in your operating system's credential vault (Windows Credential Manager / macOS Keychain) under service `io.github.sileneer.ohmyvoicetype` and are never written to settings files or config exports.
- **Signed updates**: In-app updates are cryptographically signed with Ed25519 (minisign) and verified against the public key embedded in the app before installation.

## First-run setup

1. **Settings → Providers**: choose a speech recognition provider (Groq, Mistral, OpenAI, OpenRouter, SiliconFlow, StepFun, Unisound, or Zhipu GLM), then enter its API key. Endpoints and models are prefilled, and once a valid key is entered the model picker loads live models from the provider's `/models` endpoint. Use **Advanced options** for any custom endpoint.
2. **AI cleanup** uses the same flow with built-in presets or any custom OpenAI Chat Completions, OpenAI Responses, or Anthropic Messages endpoint. It is optional; without it, dictation inserts the raw transcript.
3. **Home**: pick the dictation hotkey (Windows default `Right Alt`; macOS default `Right Option`, with Fn/Globe also available via an IOHIDManager tap), choose hold/tap smart gestures, click *Test a dictation* and speak.

## Stack

- **`crates/ovt-core`** — framework-free Rust engine: dictation pipeline, prompt builders, dictionary/hotwords, OpenAI-compatible / Responses / Anthropic / native ASR + LLM provider adapters, live `/models` discovery, SQLite history, pending recovery model, settings model, hotkey gesture decoder, WAV/silence utilities.
- **`apps/desktop`** — Tauri 2 host (Rust) + React/Vite/Tailwind UI:
  - Platform layer (`src-tauri/src/platform/`): Windows low-level keyboard hook + SendInput paste + per-app paste routing + UIPI detection; macOS CGEventTap + CGEvent paste + Accessibility selection support + permission probes.
  - Audio: capture on a dedicated thread (WASAPI streams aren't `Send`), level metering, 16 kHz WAV prep with VAD trim.
  - Session glue: hotkey → capture → durable pending queue → ASR → LLM cleanup → insertion → history, streaming state to the UI (main window + floating pill).
  - Commands: settings, history, interrupted-session recovery (`pending_*`), dictation control, API keys (OS vault via keyring), updater control, permission/device status.

## Repository layout

```
crates/ovt-core/          # shared engine (used by desktop today, future mobile/Linux hosts)
apps/desktop/
  src/                    # React UI (Tauri IPC via src/lib/api.ts)
  src-tauri/
    src/
      lib.rs main.rs      # app bootstrap, command registry
      commands.rs         # settings/history/session/pending/updater commands + AppState
      dictation.rs        # hotkey wiring + session pipeline glue
      pending.rs          # SQLite-backed interrupted-session recovery queue
      audio.rs            # audio thread + AudioHandle
      keys.rs             # API keys → OS credential vault
      updater.rs          # signed GitHub Releases updater state machine
      platform/           # windows.rs / macos.rs OS adapters
    capabilities/ icons/ Info.plist tauri.conf.json
docs/                     # research report, project plan, dev guide, acceptance checklist
scripts/                  # release.mjs, github-release.mjs, release.test.mjs, gen-icons.mjs
```

## Development

Prereqs: Node 20+, Rust stable, and on Windows the MSVC build tools; on macOS Xcode CLT.

> **Where to run commands.** The repo root is a Cargo workspace only — there is **no `package.json` at the root**. Every `npm` command must run inside `apps/desktop`, and every `cargo` command from the repo root.

```bash
# 1. install frontend deps (once)
cd apps/desktop
npm ci

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
| `npm run dev` | `apps/desktop` | UI only in a browser (`http://localhost:5173/?mock=1` uses mocked IPC; add `&update=1` or `&locale=zh-CN`) |
| `npm run build` | `apps/desktop` | `tsc --noEmit` + Vite production build into `dist/` |
| `cargo test -p ovt-core -p ovt-desktop` | repo root | Rust engine and desktop host test suites |
| `cargo clippy -p ovt-core -p ovt-desktop -- -D warnings` | repo root | Rust lint checks |

## Verification

```bash
cargo fmt --all --check
cargo clippy -p ovt-core -p ovt-desktop -- -D warnings
cargo test -p ovt-core -p ovt-desktop
cd apps/desktop && npm test && npm run build
node --test scripts/release.test.mjs
```

## Releases and updates

1. Add release notes for the target version under `## [X.Y.Z] - YYYY-MM-DD` in [`CHANGELOG.md`](CHANGELOG.md).
2. From an up-to-date `main` branch, cut a release:

```powershell
# Initial release (tag the version already in manifests):
node scripts/release.mjs tag --push

# Subsequent releases (sync all 7 version fields, commit, tag, and push):
node scripts/release.mjs patch --push    # or minor, major, or an exact version like 0.3.0
```

Leave out `--push` to review the commit/tag first; `--dry-run` validates branch, sync, and changelog without modifying anything; `--check` runs the local test/lint suite before tagging. `node scripts/release.mjs check` fails if manifests disagree, and CI runs it on every push.

Pushing a `v*` tag (or running **Actions → release → Run workflow** from `main`) triggers the [release workflow](.github/workflows/release.yml):

1. **ci** runs the full reusable CI suite (`release-scripts`, `core`, `desktop-windows`, `desktop-macos`) before anything is built or released.
2. **prepare** checks that the commit is on `main`, verifies manifest versions, validates `CHANGELOG.md` notes, checks the signing secret, and creates (or resets) one draft GitHub Release.
3. **build** compiles Windows x64 (per-user NSIS `-setup.exe`) and macOS Universal (DMG + `.app.tar.gz`) in parallel, signs the updater bundles, generates build provenance attestations, and uploads the artifacts to the draft.
4. **publish** (protected by the `release` GitHub Environment for optional manual draft review) downloads every updater artifact and `.sig`, cryptographically verifies every minisign signature against `plugins.updater.pubkey` in `tauri.conf.json`, uploads `SHA256SUMS.txt` and a single `latest.json` covering every platform, publishes the release, and confirms that `https://github.com/sileneer/oh-my-voice-type/releases/latest/download/latest.json` serves the new version.

If any build or verification step fails, the release stays a draft and installed apps never see it. Re-running the workflow reuses the draft and replaces its assets. Versions with a prerelease suffix (`0.3.0-beta.1`) or manual dispatches marked as prerelease are published as GitHub prereleases, which the updater ignores.

To verify the end-to-end updater flow on first launch, publish `0.1.0`, install it on Windows and macOS, then cut `0.1.1` and confirm in-app update download, signature verification, and restart on both platforms.

In the app, **Settings → About and updates** checks the manifest, shows the release notes, downloads the update, and verifies its signature before installing. By default the app checks 20 s after launch and every 6 hours after that, and never installs without confirmation. On Windows the NSIS installer updates in place without UAC and reopens the app afterwards; on macOS the bundle is replaced in place and the app offers **Restart now**. To test against a staged manifest, start the app with `OVT_UPDATER_ENDPOINT=https://…/latest.json`.

Signing: the updater public key is in `tauri.conf.json` and the private key lives in the `TAURI_SIGNING_PRIVATE_KEY` Actions secret (backed up separately at `~/.tauri/oh-my-voice-type.key`). Losing the private key means installed copies can no longer be updated, so keep that backup safe.

## Status / known limits (v1)

- Dictate, Translate, and Edit Selection modes are wired end-to-end, backed by a crash-safe SQLite pending recovery queue (`pending_*`).
- Insertion: direct AX (macOS `AXSelectedText`, verified) / Win32 `EM_REPLACESEL` or UIA value pattern (Windows) writes at the caret when the target supports them, falling back to clipboard + synthetic paste with clipboard restore and per-app terminal routing; the transcript is never lost.
- Wayland/Linux and mobile hosts are phase 2/3 (the core crate is already host-agnostic).
- The global hotkey is observed, not swallowed: it still reaches the focused app, so function-key bindings can collide with app shortcuts (F8 is "Go to Next Problem" in VS Code). Prefer a modifier key or one your apps don't use.
- Windows code signing (Azure Trusted Signing / SignPath) and macOS Developer ID signing + notarization are not configured yet; updater artifacts are signed separately.

## License

[MIT](LICENSE)
