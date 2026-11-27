# Changelog

All notable changes to `oh-my-voice-type` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
