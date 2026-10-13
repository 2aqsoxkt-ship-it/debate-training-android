import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { emptySnapshot } from "../session/types";
import { ReportPage } from "./ReportPage";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

const reportSnapshot = {
  ...emptySnapshot,
  matchId: "m-1",
  phase: "report_ready",
  topic: "测试辩题",
  transcripts: [
    {
      id: "t-1",
      slot: "AFF_1",
      effectiveText: "这是有效发言。",
      overflowText: "",
      interruptedTail: "",
      respondsTo: "",
      responseTargetText: "",
    },
  ],
  report: {
    formulaVersion: "v1",
    modelId: "deepseek",
    difficulty: "standard",
    speechRate: 240,
    personal: 82,
    program: 40,
    semantic: 42,
    teamWinner: "affirmative",
    standard: true,
    nonStandardReason: "",
    length: 9,
    inputControl: 8,
    execution: 10,
    trace: "可核对评分轨迹",
    scoringStatus: "READY",
    lowConfidence: false,
    dimensions: [{ name: "论证", raw: 8, points: 16, transcriptId: "t-1" }],
    stageEvaluations: [],
    reviewSummary: "论点清楚。",
    teamSummary: "协作完整。",
  },
};

describe("独立报告页", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("展示报告证据，并能释放已完成比赛返回工作台", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve(reportSnapshot);
      if (command === "release_completed_match")
        return Promise.resolve(emptySnapshot);
      return Promise.reject(new Error(`unexpected ${command}`));
    });

    render(
      <MemoryRouter initialEntries={["/report/m-1"]}>
        <Routes>
          <Route path="/report/:matchId" element={<ReportPage />} />
          <Route path="/workspace" element={<div>赛前工作台已打开</div>} />
        </Routes>
      </MemoryRouter>,
    );

    expect(
      await screen.findByRole("heading", { name: "测试辩题" }),
    ).toBeInTheDocument();
    expect(screen.getByText("这是有效发言。")).toBeInTheDocument();
    expect(
      screen.getByRole("group", { name: "完整发言证据" }),
    ).not.toHaveAttribute("open");
    fireEvent.click(screen.getByRole("button", { name: "返回赛前工作台" }));
    await waitFor(() =>
      expect(screen.getByText("赛前工作台已打开")).toBeInTheDocument(),
    );
    expect(invoke).toHaveBeenCalledWith("release_completed_match", undefined);
  });
});
