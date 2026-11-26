import { describe, expect, it } from "vitest";
import { decodeLevel, decodeOutcome, decodeSessionState } from "./api";

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
