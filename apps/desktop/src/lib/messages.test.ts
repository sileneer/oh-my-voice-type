import { describe, expect, it } from "vitest";
import { makeT } from "../i18n";
import { localizeMessage } from "./messages";

describe("backend message localization", () => {
  const zh = makeT("zh-CN");

  it("translates known session messages for the Chinese UI", () => {
    expect(localizeMessage("No microphone activity for 3 seconds — check the input device.", "zh-CN", zh))
      .toBe("已有 3 秒没有检测到麦克风声音，请检查输入设备。");
    expect(localizeMessage("text insertion failed: focus the destination app before starting a session; text is never inserted into oh-my-voice-type itself", "zh-CN", zh))
      .toContain("请先切换到要输入文字的应用");
  });

  it("keeps provider details and prefers the most specific rule", () => {
    expect(localizeMessage("AI cleanup unavailable (provider error (429): llm: rate limited — retry shortly); inserted raw transcript", "zh-CN", zh))
      .toBe("AI 润色不可用（provider error (429): llm: rate limited — retry shortly），已使用原始转写。");
    expect(localizeMessage("AI cleanup unavailable (network error: timeout); used the raw transcript", "zh-CN", zh))
      .toBe("AI 润色不可用（network error: timeout），已使用原始转写。");
    expect(localizeMessage("Could not insert text: text insertion failed: the focused target changed before delivery", "zh-CN", zh))
      .toBe("插入前焦点应用已改变，文字已复制到剪贴板。");
  });

  it("leaves English and unknown messages unchanged", () => {
    const en = makeT("en");
    expect(localizeMessage("Nothing heard — try again.", "en", en)).toBe("Nothing heard — try again.");
    expect(localizeMessage("provider error (500): boom", "zh-CN", zh)).toBe("provider error (500): boom");
  });
});
