import { describe, expect, it } from "vitest";
import { formatWhen } from "./format";

const now = new Date(2026, 10, 26, 15, 0);

describe("formatWhen", () => {
  it("names today and yesterday", () => {
    expect(formatWhen(new Date(2026, 10, 26, 13, 45).getTime(), "en", now)).toMatch(/^Today 0?1:45|^Today 13:45/);
    expect(formatWhen(new Date(2026, 10, 25, 7, 5).getTime(), "en", now)).toMatch(/^Yesterday /);
    expect(formatWhen(new Date(2026, 10, 26, 9, 0).getTime(), "zh-CN", now)).toMatch(/^今天 /);
  });

  it("shows the year only for older entries", () => {
    expect(formatWhen(new Date(2026, 9, 3, 9, 0).getTime(), "en", now)).toMatch(/^Oct 3 /);
    expect(formatWhen(new Date(2025, 9, 3, 9, 0).getTime(), "en", now)).toMatch(/2025/);
  });
});
