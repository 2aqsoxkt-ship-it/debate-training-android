import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EventControlPanel } from "./EventControlPanel";

const props = {
  schedule: [
    {
      id: "current",
      kind: "opening",
      speaker: "AFF_1",
      durationSeconds: 180,
      wordCap: 720,
      interruptible: false,
    },
    {
      id: "next",
      kind: "opening",
      speaker: "NEG_1",
      durationSeconds: 180,
      wordCap: 720,
      interruptible: false,
    },
  ],
  cursor: 0,
  mode: "training",
  onSetController: vi.fn(),
};

describe("EventControlPanel", () => {
  it("字数模式隐藏时限，当前行使用动态上限", () => {
    const wordProps = { meteringMode: "words" as const, currentWordCap: 60 };
    render(<EventControlPanel {...props} {...wordProps} />);
    expect(screen.queryByRole("columnheader", { name: "时限" })).toBeNull();
    expect(
      screen.getByRole("columnheader", { name: "字数上限" }),
    ).toBeInTheDocument();
    const rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("60 字")).toBeInTheDocument();
    expect(within(rows[2]).getByText("720 字")).toBeInTheDocument();
    expect(screen.queryByText("180s")).toBeNull();
  });

  it("字池耗尽后保留零上限", () => {
    const wordProps = { meteringMode: "words" as const, currentWordCap: 0 };
    render(<EventControlPanel {...props} {...wordProps} />);
    expect(screen.getByText("0 字")).toBeInTheDocument();
  });

  it("旧时间比赛保留时限列和静态字数上限", () => {
    render(<EventControlPanel {...props} />);
    expect(
      screen.getByRole("columnheader", { name: "时限" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("180s")).toHaveLength(2);
    expect(screen.getAllByText("720")).toHaveLength(2);
  });
});
