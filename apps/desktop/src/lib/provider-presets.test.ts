import { describe, expect, it } from "vitest";
import { asrPresets, findLlmPreset, llmPresets, resolvePresetKeyUrl } from "./provider-presets";

describe("provider presets", () => {
  it("have unique stable ids within each role", () => {
    expect(new Set(asrPresets.map((preset) => preset.id)).size).toBe(asrPresets.length);
    expect(new Set(llmPresets.map((preset) => preset.id)).size).toBe(llmPresets.length);
  });

  it("sorts providers alphabetically without prioritizing Unisound", () => {
    const labels = (presets: { label: string }[]) => presets.map((preset) => preset.label);
    expect(labels(asrPresets)).toEqual([...labels(asrPresets)].sort((a, b) => a.localeCompare(b)));
    expect(labels(llmPresets)).toEqual([...labels(llmPresets)].sort((a, b) => a.localeCompare(b)));
    expect(asrPresets[0]?.id).not.toBe("unisound-u2");
    expect(llmPresets[0]?.id).not.toBe("unisound");
  });

  it("provide complete HTTPS defaults, valid regional endpoints, and a model suggestion", () => {
    for (const preset of [...asrPresets, ...llmPresets]) {
      expect(preset.baseUrl.startsWith("https://")).toBe(true);
      expect(preset.models).toContain(preset.model);
      expect(preset.keyUrl.startsWith("https://")).toBe(true);
      if (preset.endpoints) {
        expect(preset.endpoints.length).toBeGreaterThanOrEqual(2);
        expect(preset.endpoints.some((option) => option.baseUrl === preset.baseUrl)).toBe(true);
        for (const option of preset.endpoints) {
          expect(option.baseUrl.startsWith("https://")).toBe(true);
          expect(option.keyUrl.startsWith("https://")).toBe(true);
        }
      }
    }
  });

  it("configures provider-specific ASR request options", () => {
    const openai = asrPresets.find((preset) => preset.id === "openai");
    const siliconflow = asrPresets.find((preset) => preset.id === "siliconflow");
    const stepfun = asrPresets.find((preset) => preset.id === "stepfun");
    const zhipu = asrPresets.find((preset) => preset.id === "zhipu");
    expect(openai?.extraOptions).toContainEqual(["response_format", "json"]);
    expect(siliconflow?.extraOptions).toContainEqual(["response_format", ""]);
    expect(siliconflow?.extraOptions).toContainEqual(["send_temperature", "false"]);
    expect(stepfun?.extraOptions).toContainEqual(["response_format", "json"]);
    expect(stepfun?.extraOptions).toContainEqual(["send_temperature", "false"]);
    expect(zhipu?.extraOptions).toContainEqual(["response_format", ""]);
  });

  it("maps providers to supported protocols uniformly without special-casing Unisound", () => {
    expect(asrPresets.every((preset) => ["unisound_u2", "openai_compatible"].includes(preset.engine))).toBe(true);
    expect(llmPresets.every((preset) => preset.engine === "custom")).toBe(true);
    expect(llmPresets.every((preset) => ["open_ai_chat", "open_ai_responses", "anthropic_messages"].includes(preset.format))).toBe(true);
  });

  it("resolves regional API key URLs when switching between China and Global endpoints", () => {
    const minimax = findLlmPreset("minimax");
    expect(minimax?.baseUrl).toBe("https://api.minimaxi.com/v1");
    expect(resolvePresetKeyUrl(minimax, "https://api.minimaxi.com/v1")).toContain("platform.minimaxi.com");
    expect(resolvePresetKeyUrl(minimax, "https://api.minimax.io/v1")).toContain("platform.minimax.io");

    const moonshot = findLlmPreset("moonshot");
    expect(resolvePresetKeyUrl(moonshot, "https://api.moonshot.cn/v1")).toContain("platform.moonshot.cn");
    expect(resolvePresetKeyUrl(moonshot, "https://api.moonshot.ai/v1")).toContain("platform.moonshot.ai");
  });
});
