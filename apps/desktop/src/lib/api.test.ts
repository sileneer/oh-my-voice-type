import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { api, decodeLevel, decodeOutcome, decodeSessionState } from "./api";

describe("IPC argument names", () => {
  afterEach(() => clearMocks());

  it("sends camelCase keys for multi-word command parameters", async () => {
    // Tauri 2 ignores (or rejects, when required) snake_case argument keys:
    // dictionary rename failed and the history date filters did nothing.
    const calls: Array<[string, Record<string, unknown>]> = [];
    mockIPC((command, payload) => {
      calls.push([command, (payload ?? {}) as Record<string, unknown>]);
      return command === "history_list" ? { entries: [], total: 0 } : [];
    });
    await api.historyList({ limit: 1, offset: 0, from_ms: 5, to_ms: 9, newest_first: true });
    await api.dictionaryRename("old", "new");
    await api.historyReprocess("id", "translate", "German");
    await api.saveAsrProfile({} as never, "key", true);

    expect(calls[0][1]).toMatchObject({ fromMs: 5, toMs: 9, newestFirst: true });
    expect(calls[1][1]).toEqual({ oldTerm: "old", newTerm: "new" });
    expect(calls[2][1]).toMatchObject({ targetLanguage: "German" });
    for (const [command, args] of calls) {
      for (const key of Object.keys(args)) expect(key, `${command}.${key}`).not.toContain("_");
    }
  });
});

describe("IPC payload decoding", () => {
  it("keeps valid session metadata and rejects invalid modes", () => {
    expect(decodeSessionState({
      state: "recording",
      session_id: "session-1",
      mode: "translate",
      elapsed_ms: 1200,
      pending_id: null,
    })).toEqual({
      state: "recording",
      session_id: "session-1",
      mode: "translate",
      elapsed_ms: 1200,
      pending_id: null,
    });
    expect(decodeSessionState({ state: "recording", mode: "unknown" }).mode).toBeUndefined();
    expect(decodeSessionState(null).state).toBe("error");
  });

  it("clamps audio levels and ignores non-finite values", () => {
    expect(decodeLevel({ level: 2 })).toBe(1);
    expect(decodeLevel({ level: -1 })).toBe(0);
    expect(decodeLevel({ level: Number.NaN })).toBe(0);
    expect(decodeLevel("loud")).toBe(0);
  });

  it("decodes outcomes without trusting nested fields", () => {
    expect(decodeOutcome({
      id: 42,
      raw_text: "hello",
      final_text: "Hello.",
      mode: "edit_selection",
      inserted: "yes",
      timings: { asr_ms: "fast", cleanup_ms: 20, total_ms: 40 },
      warnings: ["warning", 1],
    })).toEqual({
      id: "",
      raw_text: "hello",
      final_text: "Hello.",
      mode: "edit_selection",
      timings: { asr_ms: null, cleanup_ms: 20, total_ms: 40 },
      inserted: false,
      warnings: ["warning"],
    });
    expect(decodeOutcome({ final_text: 1 })).toBeNull();
  });
});
