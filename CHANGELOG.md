# Changelog

All notable changes to `oh-my-voice-type` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-11-27

A bug-fix release found by testing 0.1.0 on real Windows and macOS desktops.

### Added
- The "include text near the caret" setting now works: the text before the caret is read (never from password fields) and given to AI cleanup as context.
- Consecutive dictations into Latin-script text are separated by a space, as typing would be; CJK text, opening quotes and existing whitespace never get one.

### Fixed
- **Hotkey and tray sessions**: sessions started from the hotkey or tray now register properly, so the pill shows and the key can stop the take; starting again after a cancelled or failed take no longer takes two presses; the hotkey still stops a take after settings are saved; a chord cancels the take it interrupted.
- **Delivery**: stopping from the tray or the pill hands focus back to your app and types the text instead of only copying it; dictating into an app running as administrator is reported and copied instead of silently dropped; a lone `Alt` tap no longer opens the target app's menu bar.
- **Keyboard remappers**: keys sent by PowerToys Keyboard Manager, AutoHotkey or the On-Screen Keyboard drive the hotkey again.
- **Tray menu**: "Stop and transcribe" works while recording, and "Open" restores a minimized main window.
- **Recording pill**: it hides after each session on Windows and no longer shows a frame around it; on macOS it is placed correctly for hotkey starts.
- **Clipboard**: the previous clipboard content (including images) is restored reliably, and the temporary copy stays out of `Win+V` history and cloud clipboard sync.
- **Recovery**: retries resample audio correctly, show why a task failed, discard silent takes, recover sessions interrupted by a crash, and no longer add a spurious "Could not insert text" warning; pending counts refresh after a delete.
- **AI output**: reasoning models' `<think>` blocks are never typed into documents; translation translates questions instead of answering them; Anthropic requests no longer fail with a temperature above 1; "Preserve wording" and "no list formatting" work together.
- **Settings and UI**: dictionary rename and history date filters work; config export/import and audio export no longer freeze the app; hotkey pickers reject keys the platform cannot bind and duplicate bindings; a blank tone row no longer blocks saving; the main window no longer stays stuck on "processing" after a failed session.
- **Chinese interface**: session warnings and errors from the backend are translated instead of shown in English.
- **Logging**: the app writes logs again (the default log filter matched nothing).

### Installation notes
- **Windows**: Download the `*-setup.exe` installer (per-user NSIS install, no admin privileges required). If SmartScreen warns about an unrecognized app, click **More info → Run anyway**.
- **macOS**: Download the `*-universal.dmg` image and drag the app to **Applications**. Because this release is ad-hoc signed and not yet notarized by Apple, run `xattr -rd com.apple.quarantine "/Applications/oh-my-voice-type.app"` if macOS reports the app as damaged, then grant Microphone, Accessibility, and Input Monitoring permissions when prompted.

## [0.1.0] - 2026-11-27

### Added
- Global hotkey voice typing on Windows (`Right Alt`) and macOS (`Right Option`) with hold-to-talk, tap-to-latch, and chord passthrough.
- Speech recognition (ASR) and optional AI cleanup (LLM) provider profiles with live `/models` discovery and OS credential-vault API key storage.
- Floating recording pill, dictation history with search/pin/reprocess/audio export, custom dictionary hotwords, and durable interrupted-session recovery queue.
- Signed in-app updates via GitHub Releases with cryptographic signature verification.
- English and Simplified Chinese (`zh-CN`) interface.

### Installation notes
- **Windows**: Download the `*-setup.exe` installer (per-user NSIS install, no admin privileges required). If SmartScreen warns about an unrecognized app, click **More info → Run anyway**.
- **macOS**: Download the `*-universal.dmg` image and drag the app to **Applications**. Because this release is ad-hoc signed and not yet notarized by Apple, run `xattr -rd com.apple.quarantine "/Applications/oh-my-voice-type.app"` if macOS reports the app as damaged, then grant Microphone, Accessibility, and Input Monitoring permissions when prompted.
