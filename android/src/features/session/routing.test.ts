import { describe, expect, it } from "vitest";
import { emptySnapshot, routeForSnapshot } from "./types";

describe("比赛页面路由", () => {
  it("赛前阶段留在工作台", () => {
    expect(
      routeForSnapshot({
        ...emptySnapshot,
        matchId: "m-1",
        phase: "lineup_confirmed",
      }),
    ).toBe("/workspace");
  });

  it("正式比赛阶段进入独立赛场", () => {
    expect(
      routeForSnapshot({
        ...emptySnapshot,
        matchId: "m-1",
        phase: "in_progress",
      }),
    ).toBe("/match/m-1");
    expect(
      routeForSnapshot({
        ...emptySnapshot,
        matchId: "m-1",
        phase: "recovery_required",
      }),
    ).toBe("/match/m-1");
  });

  it("报告与中止阶段去往各自安全页面", () => {
    expect(
      routeForSnapshot({
        ...emptySnapshot,
        matchId: "m-1",
        phase: "report_ready",
      }),
    ).toBe("/report/m-1");
    expect(
      routeForSnapshot({
        ...emptySnapshot,
        matchId: "m-1",
        phase: "aborted",
      }),
    ).toBe("/workspace");
  });
});
