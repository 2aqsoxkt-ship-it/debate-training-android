import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FreeDebatePanel, whistleCount } from "./FreeDebatePanel";

describe("FreeDebatePanel", () => {
  it("shows word pools without time warnings or whistles", () => {
    const audioContext = vi.fn();
    vi.stubGlobal("AudioContext", audioContext);
    try {
      const wordPools = {
        meteringMode: "words" as const,
        affirmativeUnits: 0,
        negativeUnits: 120,
      };
      const { rerender } = render(
        <FreeDebatePanel
          {...wordPools}
          floorHolder="negative"
          affirmativeMs={31_000}
          negativeMs={31_000}
          currentSpeaker="NEG_3"
          onWaiveRequest={() => {}}
        />,
      );
      rerender(
        <FreeDebatePanel
          {...wordPools}
          floorHolder="negative"
          affirmativeMs={0}
          negativeMs={29_000}
          currentSpeaker="NEG_3"
          onWaiveRequest={() => {}}
        />,
      );
      expect(screen.getByText("正方共享字池").parentElement).toHaveTextContent(
        "0 字",
      );
      expect(screen.getByText("反方共享字池").parentElement).toHaveTextContent(
        "120 字",
      );
      expect(screen.getByText(/正方字数额度已用尽/)).toBeInTheDocument();
      expect(screen.getByText("反方剩余 120 字。")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "当前方放弃剩余字数" }),
      ).toBeInTheDocument();
      expect(screen.queryByText(/30 秒|共享时间/)).toBeNull();
      expect(audioContext).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("shows team pools, no-interrupt rule, and an exhausted-side handoff", () => {
    render(
      <FreeDebatePanel
        floorHolder="NEG_3"
        affirmativeMs={0}
        negativeMs={90_000}
        currentSpeaker="NEG_3"
        lastRespondsTo="对方刚才的核心主张"
      />,
    );

    expect(screen.getByText("不可打断")).toBeInTheDocument();
    expect(screen.getByText(/正方时间已用尽/)).toBeInTheDocument();
    expect(screen.getByText(/反方可继续/)).toBeInTheDocument();
    expect(screen.getByText(/本轮应回应/)).toHaveTextContent(
      "对方刚才的核心主张",
    );
  });

  it("uses one whistle at 30 seconds and two when time expires", () => {
    expect(whistleCount(31_000, 30_000)).toBe(1);
    expect(whistleCount(30_000, 29_000)).toBe(0);
    expect(whistleCount(1_000, 0)).toBe(2);
    expect(whistleCount(0, 0)).toBe(0);
  });

  it("keeps a visual warning when audio is unavailable", () => {
    render(
      <FreeDebatePanel
        floorHolder="AFF_2"
        affirmativeMs={29_000}
        negativeMs={80_000}
        currentSpeaker="AFF_2"
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("正方剩余 30 秒内");
  });

  it("recognizes the authoritative team floor rather than requiring a seat label", () => {
    render(
      <FreeDebatePanel
        floorHolder="negative"
        affirmativeMs={120_000}
        negativeMs={90_000}
        currentSpeaker="NEG_3"
      />,
    );
    expect(screen.getByText("反方持权")).toBeInTheDocument();
    expect(screen.getByText("反方共享").closest(".team-clock")).toHaveClass(
      "active",
    );
    expect(screen.getByText("正方共享").closest(".team-clock")).not.toHaveClass(
      "active",
    );
  });
});
