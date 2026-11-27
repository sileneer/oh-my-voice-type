import type { Locale, MessageKey, TFunc } from "../i18n";

/**
 * Session warnings and errors are produced by the Rust backend in English.
 * They were shown verbatim, so the Chinese UI popped up English toasts and
 * HUD errors ("No microphone activity for 3 seconds…"). Map the known ones,
 * most specific first; anything unknown (provider details) passes through.
 */
const rules: Array<{ match: RegExp; key: MessageKey; detail?: number }> = [
  { match: /focused target changed before delivery/i, key: "msg.targetChanged" },
  { match: /runs as administrator/i, key: "msg.elevated" },
  { match: /focus the destination app/i, key: "msg.focusTarget" },
  { match: /another (voice )?session is (already )?active/i, key: "msg.sessionActive" },
  { match: /No microphone activity/i, key: "msg.noMicActivity" },
  { match: /nearing the configured time limit/i, key: "msg.timeLimit" },
  { match: /nothing was heard in this recording/i, key: "msg.retryNothingHeard" },
  { match: /Nothing heard/i, key: "msg.nothingHeard" },
  { match: /No sound from the microphone/i, key: "msg.noSound" },
  { match: /copied to the clipboard — paste it manually/i, key: "msg.copied" },
  { match: /AI cleanup unavailable \((.*)\); inserted raw transcript/i, key: "msg.cleanupUnavailable", detail: 1 },
  { match: /AI cleanup is off/i, key: "msg.cleanupOff" },
  { match: /select an ASR profile|ASR API key missing|ASR profile is not configured/i, key: "msg.asrMissing" },
  { match: /LLM is required|LLM API key missing|bound LLM API key is missing|LLM profile is not configured/i, key: "msg.llmRequired" },
  { match: /Could not insert text: (.*)/i, key: "msg.insertFailed", detail: 1 },
];

export function localizeMessage(message: string, locale: Locale, t: TFunc): string {
  if (locale !== "zh-CN") return message;
  for (const rule of rules) {
    const found = message.match(rule.match);
    if (found) return t(rule.key, { detail: rule.detail ? found[rule.detail] ?? "" : "" });
  }
  return message;
}
