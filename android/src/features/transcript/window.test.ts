import { describe, expect, it } from "vitest";
import { finishRound, visibleWindow } from "./window";

describe("记录窗口", () => {
  it("一万条记录只暴露当前窗口", () => {
    const window = visibleWindow(10_000, 200, 20);
    expect(window.count).toBe(20);
    expect(window.from).toBe(200);
    expect(window.to).toBe(220);
  });

  it("提交后清空按键时间线", () => {
    const events = [
      { kind: "composition" as const },
      { kind: "insert" as const, text: "你好", source: "typed" as const },
    ];
    const saved = finishRound(events);
    expect(saved).toHaveLength(2);
    expect(events).toHaveLength(0);
  });
});
