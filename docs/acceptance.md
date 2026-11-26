# Release acceptance checklist (v1)

Run on each target machine before shipping.

## macOS
- [ ] Fresh install: Accessibility prompt appears; granting takes effect without restart (or a restart prompt appears)
- [ ] Input Monitoring granted when the CGEventTap starts
- [ ] Microphone prompt on first dictation
- [ ] Hold Right Option → speaks → release → text lands at caret in: TextEdit, Slack, Chrome, VS Code, Notes
- [ ] Quick tap latches; next tap stops
- [ ] Fn+Space chord passes through to the focused app instead of starting a dictation
- [ ] Clipboard restore: copy something first, dictate, paste → original clipboard content present
- [ ] Password field (native secure field): text is not inserted; copied fallback only
- [ ] Cancel (Esc ×2 via UI button) discards the take
- [ ] 9-minute scenario: no crash at long takes; silence at start/end trimmed (no "Thank you." hallucination)
- [ ] Muted mic: "No sound from the microphone" surfaced within ~3 s

## Windows
- [ ] `Right Alt` hold → speak → release → insert into Notepad, Chrome, Slack, Word, VS Code
- [ ] Windows Terminal: Shift+Insert routing used (no `^V` artifacts)
- [ ] Elevated app target (admin Notepad): clear error + copied fallback, no text loss
- [ ] UAC prompt while recording: takes degrade gracefully
- [ ] Ctrl+V in browser text fields; Enter not auto-pressed
- [ ] Settings: changing hotkey persists across restart; invalid base URL rejected with message
- [ ] API keys survive restart and show "stored" status
- [ ] History: list, search, copy, pin, delete, retention prune after restart
- [ ] Pill shows on the focused monitor, never steals focus, disappears after done

## Both
- [ ] 20 dictations: record p50/p90 stop→insert time from `session:outcome` timings
- [ ] Provider failure paths: wrong key (401 message), wrong URL, offline → raw transcript inserted + warning
- [ ] No dictation ever results in an empty caret without a warning
