import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CrossExamPanel, type CrossExamSnapshot } from "./CrossExamPanel";

const base: CrossExamSnapshot = {
  eventId: "event-1",
  phase: "question",
  questioner: "AFF_3",
  respondent: "NEG_3",
  questionerHuman: true,
  respondentHuman: true,
  question: "",
  visiblePrefix: "",
  interruptible: true,
  answerPrepared: false,
  revealedUnits: 0,
  totalUnits: 0,
  roundNumber: 1,
  nextSide: "affirmative",
  totalMs: 90_000,
  usedMs: 0,
  remainingMs: 90_000,
  selectionSummary: "系统按公开实录派位",
  rolesSelected: true,
  eligibleQuestioners: ["AFF_1", "AFF_2", "AFF_3", "AFF_4"],
  history: [],
};

function renderPanel(
  snapshot: CrossExamSnapshot,
  options: { questionText?: string; answerLength?: number } = {},
) {
  const handlers = {
    onSubmitQuestion: vi.fn((event) => event.preventDefault()),
    onGenerateQuestion: vi.fn(),
    onInsertQuestioner: vi.fn(),
    onPrepareHumanAnswer: vi.fn((event) => event.preventDefault()),
    onGenerateAnswer: vi.fn(),
    onSetRespondentHuman: vi.fn(),
    onReveal: vi.fn<
      (units: number, elapsedMs: number) => Promise<CrossExamSnapshot>
    >(async () => snapshot),
    onComplete: vi.fn(),
    onInterrupt: vi.fn(),
    onTimeout: vi.fn(),
    onEndSession: vi.fn(),
    onCancel: vi.fn(),
  };
  render(
    <CrossExamPanel
      snapshot={snapshot}
      questionTextareaProps={{
        value: options.questionText ?? "",
        onChange: vi.fn(),
      }}
      answerTextareaProps={{ value: "", onChange: vi.fn() }}
      answerLength={options.answerLength ?? 0}
      wordCap={180}
      busy={false}
      {...handlers}
    />,
  );
  return handlers;
}

describe("CrossExamPanel", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("字数模式默认舒缓展示而非900字/分钟，速度可切慢且不会提前吐字", async () => {
    vi.useFakeTimers();
    const handlers = renderPanel({
      ...base,
      meteringMode: "words",
      phase: "revealing",
      answerPrepared: true,
      revealedUnits: 1,
      totalUnits: 100,
      visiblePrefix: "答",
    });
    expect(screen.getByRole("combobox", { name: "回答展示速度" })).toHaveValue(
      "180",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(handlers.onReveal).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("combobox", { name: "回答展示速度" }), {
      target: { value: "120" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(handlers.onReveal).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(110);
    });
    expect(handlers.onReveal).toHaveBeenCalledTimes(1);
    expect(handlers.onReveal.mock.calls[0][0]).toBe(2);
    expect(screen.getByRole("button", { name: "打断" })).toBeEnabled();
  });

  it("字数模式提问显示上限与草稿预测，并禁止倒计时自动超时", () => {
    vi.useFakeTimers();
    const wordSnapshot = {
      ...base,
      meteringMode: "words" as const,
      remainingUnits: 900,
      questionWordCap: 80,
      answerWordCap: 160,
      remainingMs: 500,
    };
    const handlers = renderPanel(wordSnapshot, {
      questionText: "请定义风险。",
    });
    expect(screen.getByText("质询方共享字池")).toBeInTheDocument();
    expect(
      screen.getByText("900 字", { selector: "strong" }),
    ).toBeInTheDocument();
    expect(screen.getByText("已输入 5 / 80 字")).toBeInTheDocument();
    expect(screen.getByText("草稿提交后预计剩余 895 字")).toBeInTheDocument();
    expect(screen.getByText(/问答均扣当前质询方字池/)).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(120_000);
    });
    expect(handlers.onTimeout).not.toHaveBeenCalled();
    expect(
      screen.getByText("900 字", { selector: "strong" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("1s")).toBeNull();
  });

  it("字数模式回答使用后端上限并只预测可公开文本的扣字", () => {
    const wordSnapshot = {
      ...base,
      phase: "preparing" as const,
      meteringMode: "words" as const,
      remainingUnits: 75,
      questionWordCap: 75,
      answerWordCap: 60,
    };
    renderPanel(wordSnapshot, { answerLength: 80 });
    expect(screen.getByText("已输入 80，上限 60 字")).toHaveClass("over-cap");
    expect(screen.getByText("草稿提交后预计剩余 15 字")).toBeInTheDocument();
    expect(screen.getByText(/打断时仅扣已公开前缀/)).toBeInTheDocument();
  });

  it.each(["preparing", "revealing"] as const)(
    "字数模式在%s阶段也不会时间超时",
    (phase) => {
      vi.useFakeTimers();
      const wordSnapshot = {
        ...base,
        phase,
        meteringMode: "words" as const,
        remainingUnits: 30,
        remainingMs: 250,
        answerPrepared: phase === "revealing",
        totalUnits: 10,
      };
      const handlers = renderPanel(wordSnapshot);
      act(() => {
        vi.advanceTimersByTime(10_000);
      });
      expect(handlers.onTimeout).not.toHaveBeenCalled();
      expect(
        screen.getByText("30 字", { selector: "strong" }),
      ).toBeInTheDocument();
    },
  );

  it("额度耗尽后的终态快照显示环节结束", () => {
    const completedSnapshot = {
      ...base,
      phase: "completed" as const,
      meteringMode: "words" as const,
      remainingUnits: 0,
    };
    renderPanel(completedSnapshot);
    expect(screen.getByText("本环节质询已经结束。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "提交问题" })).toBeNull();
    expect(screen.getByText("4. 本轮完成")).toHaveClass("current");
    expect(screen.getByText("1. 提问")).toHaveClass("complete");
    expect(
      screen.getByRole("button", { name: "结束整个质询环节" }),
    ).toBeDisabled();
  });

  it.each([
    [true, "真人提问"],
    [false, "AI 生成并提交问题"],
  ])("按问方控制权渲染操作", (questionerHuman, expected) => {
    renderPanel({ ...base, questionerHuman });
    expect(screen.getByText(expected)).toBeInTheDocument();
  });

  it.each([
    [true, "质询回答"],
    [false, "AI 准备回答"],
  ])("按答方控制权渲染操作", (respondentHuman, expected) => {
    renderPanel({
      ...base,
      phase: "preparing",
      question: "请定义风险",
      respondentHuman,
    });
    if (respondentHuman) {
      expect(screen.getByLabelText(expected)).toBeInTheDocument();
    } else {
      expect(
        screen.getByRole("button", { name: expected }),
      ).toBeInTheDocument();
    }
  });

  it("只在展示出前缀后允许打断", () => {
    const handlers = renderPanel({
      ...base,
      phase: "revealing",
      question: "请定义风险",
      visiblePrefix: "风险是",
      answerPrepared: true,
      revealedUnits: 3,
      totalUnits: 10,
    });
    fireEvent.click(screen.getByRole("button", { name: "打断" }));
    expect(handlers.onInterrupt).toHaveBeenCalledOnce();
  });

  it("显示多角色派位、共享计时并允许只选择本方提问辩位", () => {
    const handlers = renderPanel(base);
    expect(screen.getByText(/第 1 轮/)).toHaveTextContent("当前质询方：正方");
    expect(screen.getByText(/派位说明/)).toHaveTextContent(
      "系统按公开实录派位",
    );
    fireEvent.click(screen.getByRole("button", { name: "正方二辩" }));
    expect(handlers.onInsertQuestioner).toHaveBeenCalledWith(1);
    expect(screen.queryByRole("button", { name: "反方一辩" })).toBeNull();
  });

  it("问题提交后可以在回答开始前切换真人或 AI", () => {
    const handlers = renderPanel({
      ...base,
      phase: "preparing",
      question: "请定义风险",
      respondentHuman: false,
    });
    fireEvent.click(screen.getByRole("button", { name: "真人回答" }));
    expect(handlers.onSetRespondentHuman).toHaveBeenCalledWith(true);
  });
});
