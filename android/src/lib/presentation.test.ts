import { describe, expect, it } from "vitest";
import { readableRoleText, speakerLabel } from "./presentation";

describe("中文辩位展示", () => {
  it("八个辩位及旧的 aff1/Neg4 表示均可读，未知值不冒充合法辩位", () => {
    for (const side of ["AFF", "NEG"])
      for (let number = 1; number <= 4; number++) {
        expect(speakerLabel(`${side}_${number}`)).toBe(
          `${side === "AFF" ? "正方" : "反方"}${["一", "二", "三", "四"][number - 1]}辩`,
        );
      }
    expect(speakerLabel("aff1")).toBe("正方一辩");
    expect(speakerLabel("Neg4")).toBe("反方四辩");
    expect(speakerLabel("AFF_9")).toBe("待核对辩位");
    expect(speakerLabel("")).toBe("待选择");
  });
  it("只替换系统说明里的辩位，不改动标识符的片段", () => {
    expect(
      readableRoleText("系统安排 Neg1 质询 AFF_2；记录 id-aff1-extra"),
    ).toBe("系统安排 反方一辩 质询 正方二辩；记录 id-aff1-extra");
    expect(readableRoleText("NONAFF1")).toBe("NONAFF1");
  });
});
