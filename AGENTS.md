# oh-my-voice-type: agent notes

Project rules that add to the user-level `AGENTS.md`. See `README.md` and `docs/dev-guide.md` for architecture.

## Checks before committing

- `cargo fmt --all --check`, `cargo clippy -p ovt-core -p ovt-desktop`, `cargo test -p ovt-core -p ovt-desktop`
- `cd apps/desktop && npm test && npm run build` (vitest, then `tsc --noEmit` + vite)
- `node --test scripts/release.test.mjs` (a directory argument such as `node --test scripts/` fails on Windows)
- Browser preview of the UI: `http://localhost:5173/?mock=1` uses the IPC mock in `src/lib/tauri-mock.ts`; add `&update=1` to simulate an available update, `&locale=zh-CN` for Chinese, and `&view=pill` for the recording pill (size the viewport to 340x64). Run Vite with `cwd` `apps/desktop` (`.claude/launch.json`: `"runtimeArgs": ["node_modules/vite/bin/vite.js"], "cwd": "apps/desktop"`); started from the repo root, Tailwind cannot find its config and every page shows a `bg-background` PostCSS error.
- The pill window is shown with raw Win32 `ShowWindow(SW_SHOWNOACTIVATE)` (`platform::show_inactive`), so tao's cached visibility never changes and Tauri `hide()` is a no-op on Windows. Hide it only via `platform::hide_window` / the `pill_hide` command.
- Tauri 2 maps every Rust command parameter to a camelCase `invoke` key (`old_term` → `oldTerm`); snake_case keys are silently dropped (optional) or rejected (required). Struct parameters (`settings`, `request`, `args`) keep their serde field names. `src/lib/api.test.ts` pins the multi-word keys, and the IPC mock reads camelCase so regressions fail in tests.
- Plain sync `#[tauri::command]`s run on the main thread: a command that calls a `blocking_*` dialog must be `#[tauri::command(async)]` or the app freezes. Session start blocks too (keychain, UI Automation) and must never run on an async worker or the hook thread (`blocking_recv` there panicked).
- Tray menu: build it once and update items in place (`set_text`/`set_enabled`); menus swapped in with `TrayIcon::set_menu` from worker threads never delivered clicks on Windows.
- The Windows keyboard hook ignores only this app's own `SendInput` events (tagged `OVT_INJECTED_TAG` in `dwExtraInfo`); other injected keys (PowerToys/AutoHotkey remaps, On-Screen Keyboard) must still drive the hotkey.
- Real-device testing: run `node node_modules/@tauri-apps/cli/tauri.js dev` from `apps/desktop` with `OVT_DATA_DIR` set to a scratch folder (debug builds only) so real history is untouched. Saving any Rust file restarts the app and kills an in-flight session. Desktop automation cannot press side-specific Right Alt, so bind F8 in the scratch settings; Windows popup menus need a hover before a synthetic click registers.
- Commands that change the pending queue must emit `pending:changed` (and update the tray); the sidebar badge, home banner and settings count only refresh on that event.

## Providers

- This is a general voice-typing app: treat every ASR and LLM provider equally. Do not special-case Unisound (or any other provider) in presets, UI flows, defaults, or local skills.
- Model lists are fetched live from each provider's `/models` endpoint via the `list_models` command (`crates/ovt-core/src/providers/models.rs`) once a valid API key is provided, using `apps/desktop/src/lib/provider-presets.ts` as fallback.

## Releases and the updater

- Never hand-edit versions. Use `node scripts/release.mjs <patch|minor|major|x.y.z|tag> [--push] [--dry-run] [--check]`; `check` verifies all seven version fields agree, and CI runs it. Update `CHANGELOG.md` with a `## [X.Y.Z]` section before cutting a release.
- Windows ships a per-user NSIS installer (`-setup.exe`, `installMode: "currentUser"`), not MSI.
- `.github/workflows/release.yml` runs the reusable `ci.yml` first, creates one draft release from `main`, builds both platforms into it with `uploadUpdaterJson: false`, then `scripts/github-release.mjs publish` cryptographically verifies every `.sig` against `tauri.conf.json`'s `pubkey`, uploads `SHA256SUMS.txt` and a single `latest.json`, and publishes (via the `release` GitHub Environment). Do not switch back to tauri-action's own `latest.json`: two parallel jobs race on it, and the app looks up the `darwin-universal` key (set in `lib.rs`), which tauri-action does not write.
- Updater state lives in `src-tauri/src/updater.rs` and is serialised in kebab-case (`up-to-date`), matching `UpdaterStatus` in `src/lib/api.ts`.
- Pushing a release or a `v*` tag publishes a public GitHub Release; get the user's go-ahead first.
- GitHub retired the `macos-14` runner image; use `macos-latest`.
