import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MatchRoom, type MatchView } from "./MatchRoom";

const originalScrollTo = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "scrollTo",
);

beforeEach(() => {
  // jsdom does not implement the scroll method used when restoring the timeline.
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    writable: true,
    value: vi.fn(),
  });
});

afterEach(() => {
  if (originalScrollTo) {
    Object.defineProperty(HTMLElement.prototype, "scrollTo", originalScrollTo);
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  }
});

const view: MatchView = {
  phase: "in_progress",
  topic: "人工智能应当受到限制",
  mode: "training",
  schedule: [
    {
      id: "1",
      kind: "cross_exam",
      speaker: "AFF_3",
      durationSeconds: 90,
      wordCap: 360,
      interruptible: true,
      controller: "ai",
      started: true,
    },
    {
      id: "2",
      kind: "opening",
      speaker: "NEG_1",
      durationSeconds: 180,
      wordCap: 720,
      interruptible: false,
      controller: "ai",
      started: false,
    },
  ],
  cursor: 0,
  remainingMs: 90_000,
  affirmativeMs: 0,
  negativeMs: 0,
  floorHolder: "",
  transcripts: [
    {
      id: "t1",
      slot: "AFF_1",
      effectiveText: "我方先立论。",
      overflowText: "",
      interruptedTail: "",
    },
  ],
  ownStrategy: "正方立论",
  opponentStrategy: "反方拆解",
  materials: "",
  currentHuman: false,
  currentInterruptible: false,
  awaitingSeat: true,
  usesLiveModel: true,
  standard: true,
  seats: [0, 1, 2, 3, 4, 5, 6, 7].map((index) => ({
    index,
    name: [
      "正方一辩",
      "正方二辩",
      "正方三辩",
      "正方四辩",
      "反方一辩",
      "反方二辩",
      "反方三辩",
      "反方四辩",
    ][index],
    duty: ["立论", "展开", "攻辩", "总结", "立论", "展开", "攻辩", "总结"][
      index
    ],
    side: index < 4 ? "affirmative" : "negative",
    controller: "ai",
    speaking: index === 2,
    claimable: index === 2,
  })),
};

const roomProps = {
  draftLength: 0,
  textareaProps: { value: "", onChange: () => {} },
  onSubmit: (event: { preventDefault: () => void }) => event.preventDefault(),
  onClaim: () => {},
  onAdvance: () => {},
  onCancel: () => {},
  onInterrupt: () => {},
  onFinish: () => {},
  onTimeoutSubmit: () => {},
  onSetController: () => {},
  thinking: null as string | null,
  windowStart: 0,
  onMore: () => {},
  onAbort: () => {},
};

it("辩位代码只留在数据中，赛场、上一条发言和赛程抽屉显示中文辩位", () => {
  const { container } = render(<MatchRoom view={view} {...roomProps} />);
  expect(container.textContent).not.toMatch(/\b(?:AFF|NEG)_?[1-4]\b/i);
  expect(screen.getByText(/上一条公开发言 · 正方一辩/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "打开赛程与控制" }));
  expect(container.textContent).not.toMatch(/\b(?:AFF|NEG)_?[1-4]\b/i);
});

describe("赛场房间", () => {
  it("八个辩位可从移动面板访问，合法接管不自动发言且关闭保留草稿", () => {
    const onClaim = vi.fn();
    const onAdvance = vi.fn();
    render(
      <MatchRoom
        {...roomProps}
        view={view}
        textareaProps={{ value: "未提交草稿", onChange: () => {} }}
        onClaim={onClaim}
        onAdvance={onAdvance}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "打开双方辩位" }));
    const panel = screen.getByRole("dialog", { name: "双方辩位" });
    for (const seat of view.seats) {
      const button = within(panel).getByRole("button", {
        name: new RegExp(seat.name),
      });
      if (seat.claimable) expect(button).toBeEnabled();
      else expect(button).toBeDisabled();
    }
    fireEvent.click(within(panel).getByRole("button", { name: /正方三辩/ }));
    expect(onClaim).toHaveBeenCalledWith(2);
    expect(onAdvance).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "双方辩位" })).toBeNull();
    expect(screen.getByRole("textbox", { name: "本环节回答" })).toHaveValue(
      "未提交草稿",
    );
  });

  describe("字数额度", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    const wordView = {
      ...view,
      meteringMode: "words" as const,
      schedule: [{ ...view.schedule[1], speaker: "AFF_1", started: true }],
      remainingMs: 500,
      remainingUnits: 900,
      currentWordCap: 240,
      affirmativeMainUnits: 900,
      negativeMainUnits: 1_200,
      affirmativeFreeUnits: 960,
      negativeFreeUnits: 800,
      currentHuman: true,
      awaitingSeat: false,
    };

    it("显示字池和当前可提交上限，草稿只预测余额且不会时间超时", () => {
      const onTimeoutSubmit = vi.fn();
      render(
        <MatchRoom
          {...roomProps}
          view={wordView}
          draftLength={300}
          onTimeoutSubmit={onTimeoutSubmit}
        />,
      );
      const affirmativePool = screen.getByText("正方主字池").parentElement;
      expect(affirmativePool).toHaveClass("active");
      expect(affirmativePool).toHaveTextContent("900 字");
      expect(screen.getByText("反方主字池").parentElement).toHaveTextContent(
        "1,200 字",
      );
      expect(screen.getByText("已输入 300 / 240 字")).toHaveClass("over-cap");
      expect(screen.getByText("草稿提交后预计剩余 660 字")).toBeInTheDocument();
      expect(screen.getByText(/仅正式公开文本扣字/)).toBeInTheDocument();
      expect(screen.queryByText(/本轮剩余.*:/)).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "打开赛程与控制" }));
      const schedule = screen.getByRole("dialog", { name: "赛程与控制" });
      expect(
        within(schedule).queryByRole("columnheader", { name: "时限" }),
      ).toBeNull();
      expect(
        within(schedule).getByRole("cell", { name: "240 字" }),
      ).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "关闭赛程与控制" }));
      act(() => {
        vi.advanceTimersByTime(120_000);
      });
      expect(onTimeoutSubmit).not.toHaveBeenCalled();
      expect(affirmativePool).toHaveTextContent("900 字");
    });

    it("字数上限为零时不会显示不限", () => {
      const exhaustedView = {
        ...wordView,
        remainingUnits: 0,
        currentWordCap: 0,
        affirmativeMainUnits: 0,
      };
      render(<MatchRoom {...roomProps} view={exhaustedView} draftLength={1} />);
      expect(screen.getByText("已输入 1 / 0 字")).toHaveClass("over-cap");
      expect(screen.queryByText(/不限/)).toBeNull();
    });

    it("自由辩使用双方共享字池，选席和放弃操作不显示时间", () => {
      const freeWordView = {
        ...wordView,
        schedule: [{ ...wordView.schedule[0], kind: "free_debate" }],
        floorHolder: "negative",
        awaitingSeat: true,
      };
      render(<MatchRoom {...roomProps} view={freeWordView} />);
      expect(screen.getByText("正方共享字池").parentElement).toHaveTextContent(
        "960 字",
      );
      expect(screen.getByText("反方共享字池").parentElement).toHaveTextContent(
        "800 字",
      );
      expect(screen.getByText("点选辩位后发言")).toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", { name: "当前方放弃剩余字数" }),
      );
      expect(
        screen.getByRole("dialog", { name: "放弃当前方自由辩剩余字数" }),
      ).toHaveTextContent("自由辩字池会立即归零");
      expect(screen.queryByText(/30 秒/)).toBeNull();
    });
  });

  describe("主时间显示", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    const speechView = {
      ...view,
      ruleDraft: { planVersion: "match-plan.v3" },
      schedule: [
        {
          ...view.schedule[1],
          speaker: "AFF_1",
          started: true,
        },
      ],
      remainingMs: 1_070_000,
      affirmativeMainMs: 1_080_000,
      negativeMainMs: 1_050_000,
      currentHuman: true,
      awaitingSeat: false,
    };

    it("没有自由辩持权字段时仍按当前陈词方实时显示主池", () => {
      render(<MatchRoom {...roomProps} view={speechView} />);
      const affirmativeClock = screen.getByText("正方主时间").parentElement;
      const negativeClock = screen.getByText("反方主时间").parentElement;
      expect(affirmativeClock).toHaveClass("active");
      expect(negativeClock).not.toHaveClass("active");
      expect(affirmativeClock).toHaveTextContent("17:50");
      expect(negativeClock).toHaveTextContent("17:30");
      expect(screen.getByText("正方持权")).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(affirmativeClock).toHaveTextContent("17:48");
      expect(negativeClock).toHaveTextContent("17:30");
    });

    it("切换到反方陈词时忽略上一环节残留的自由辩持权方", () => {
      render(
        <MatchRoom
          {...roomProps}
          view={{
            ...speechView,
            floorHolder: "affirmative",
            schedule: [{ ...speechView.schedule[0], speaker: "NEG_1" }],
            remainingMs: 1_040_000,
          }}
        />,
      );
      const affirmativeClock = screen.getByText("正方主时间").parentElement;
      const negativeClock = screen.getByText("反方主时间").parentElement;
      expect(affirmativeClock).not.toHaveClass("active");
      expect(negativeClock).toHaveClass("active");
      expect(affirmativeClock).toHaveTextContent("18:00");
      expect(negativeClock).toHaveTextContent("17:20");
      expect(screen.getByText("反方持权")).toBeInTheDocument();
    });

    it("自定义单环节时限按本轮用时投影主池，保留单轮倒计时", () => {
      render(
        <MatchRoom
          {...roomProps}
          view={{ ...speechView, ruleDraft: null, remainingMs: 170_000 }}
        />,
      );
      const affirmativeClock = screen.getByText("正方主时间").parentElement;
      expect(affirmativeClock).toHaveTextContent("17:50");
      expect(screen.getByText(/正方一辩 · 180s/)).toBeInTheDocument();
      expect(
        screen.getByText("02:50", { selector: "strong" }),
      ).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(affirmativeClock).toHaveTextContent("17:48");
      expect(
        screen.getByText("02:48", { selector: "strong" }),
      ).toBeInTheDocument();
    });

    it.each([
      { awaitingSeat: true, thinking: null },
      { awaitingSeat: false, thinking: "正在思考" },
    ])(
      "等待选席或模型生成时主池不会继续扣时：%j",
      ({ awaitingSeat, thinking }) => {
        render(
          <MatchRoom
            {...roomProps}
            thinking={thinking}
            view={{ ...speechView, awaitingSeat }}
          />,
        );
        const affirmativeClock = screen.getByText("正方主时间").parentElement;
        act(() => {
          vi.advanceTimersByTime(2_000);
        });
        expect(affirmativeClock).toHaveTextContent("17:50");
      },
    );

    it("自由辩只推进持权方的自由辩池，双方主池暂停", () => {
      render(
        <MatchRoom
          {...roomProps}
          view={{
            ...speechView,
            schedule: [{ ...speechView.schedule[0], kind: "free_debate" }],
            floorHolder: "negative",
            remainingMs: 110_000,
            affirmativeMs: 120_000,
            negativeMs: 120_000,
          }}
        />,
      );
      const affirmativeClock = screen.getByText("正方主时间").parentElement;
      const negativeClock = screen.getByText("反方主时间").parentElement;
      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(affirmativeClock).not.toHaveClass("active");
      expect(negativeClock).not.toHaveClass("active");
      expect(affirmativeClock).toHaveTextContent("18:00");
      expect(negativeClock).toHaveTextContent("17:30");
      expect(screen.getByText("反方共享").parentElement).toHaveTextContent(
        "01:48",
      );
      expect(screen.getByText("正方共享").parentElement).toHaveTextContent(
        "02:00",
      );
    });
  });

  it("按席位展示对话，并只让本环节辩位可点", () => {
    const onClaim = vi.fn();
    render(<MatchRoom {...roomProps} view={view} onClaim={onClaim} />);
    expect(
      screen.getByRole("region", { name: "比赛时间线" }),
    ).toHaveTextContent("我方先立论");
    expect(screen.getByRole("log")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "记录" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "打开实录" }));
    expect(
      within(screen.getByRole("dialog", { name: "比赛实录" })).getByText(
        /我方先立论/,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭比赛实录" }));
    expect(screen.getByRole("button", { name: /取消生成/ })).toBeDisabled();
    expect(screen.getByText(/点选辩位后开始计时/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开策略与材料" }));
    expect(screen.getByText(/本方策略：正方立论/)).toBeInTheDocument();
    expect(screen.getByText(/对方策略：反方拆解/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭策略与材料" }));
    fireEvent.click(screen.getByRole("button", { name: /正方三辩/ }));
    expect(onClaim).toHaveBeenCalledWith(2);
    expect(screen.getByRole("button", { name: /反方一辩/ })).toBeDisabled();
  });

  it("模型生成时显示当前辩位正在思考，并停住操作", () => {
    const onCancel = vi.fn();
    render(
      <MatchRoom
        {...roomProps}
        view={view}
        thinking="正在思考"
        onCancel={onCancel}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "正方三辩 · 正在思考 · 已等待 0 秒（不扣字）",
    );
    expect(
      screen.getByRole("button", { name: /让模型完成本轮/ }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: /正方三辩/ })).toBeEnabled();
    const cancel = screen.getByRole("button", { name: /取消生成/ });
    expect(cancel).toBeEnabled();
    fireEvent.click(cancel);
    expect(onCancel).toHaveBeenCalled();
  });

  it("模型失败后保留赛场并提供重试、接管和留在现场", () => {
    const onRetryAi = vi.fn();
    const onTakeoverCurrent = vi.fn();
    const onDismissAiFailure = vi.fn();
    render(
      <MatchRoom
        {...roomProps}
        view={view}
        aiFailure="请求超时"
        onRetryAi={onRetryAi}
        onTakeoverCurrent={onTakeoverCurrent}
        onDismissAiFailure={onDismissAiFailure}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("实录和计时均未回滚");
    fireEvent.click(within(alert).getByRole("button", { name: "重试本轮" }));
    fireEvent.click(within(alert).getByRole("button", { name: "人工接管" }));
    fireEvent.click(within(alert).getByRole("button", { name: "保留现场" }));
    expect(onRetryAi).toHaveBeenCalled();
    expect(onTakeoverCurrent).toHaveBeenCalled();
    expect(onDismissAiFailure).toHaveBeenCalled();
  });

  it("自由辩不渲染打断按钮", () => {
    const freeView: MatchView = {
      ...view,
      schedule: [
        {
          id: "fd",
          kind: "free_debate",
          speaker: "AFF_1",
          durationSeconds: 240,
          wordCap: 0,
          interruptible: false,
          controller: "human",
          started: true,
        },
      ],
      currentHuman: true,
      currentInterruptible: false,
      awaitingSeat: false,
    };
    render(<MatchRoom {...roomProps} view={freeView} />);
    expect(screen.queryByRole("button", { name: /^打断$/ })).toBeNull();
    expect(screen.getByLabelText("自由辩")).toBeInTheDocument();
  });

  it("质询期间隐藏普通回答和普通 AI 完成本轮", () => {
    const crossView: MatchView = {
      ...view,
      schedule: [
        {
          id: "cross",
          kind: "cross_exam",
          speaker: "AFF_3",
          durationSeconds: 120,
          wordCap: 180,
          interruptible: true,
          controller: "human",
          started: true,
        },
      ],
    };
    render(
      <MatchRoom
        {...roomProps}
        view={crossView}
        crossExamActive
        stageWorkspace={<div>质询控制台位于中央</div>}
      />,
    );
    expect(screen.queryByLabelText("本环节回答")).toBeNull();
    expect(screen.queryByRole("button", { name: /让模型完成本轮/ })).toBeNull();
    expect(screen.getByText("质询控制台位于中央")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /结束并评分/ })).toBeNull();
  });

  it("中止比赛必须由页面显式处理", () => {
    const onAbort = vi.fn();
    render(<MatchRoom {...roomProps} view={view} onAbort={onAbort} />);
    fireEvent.click(screen.getByRole("button", { name: "中止比赛" }));
    expect(onAbort).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "确认中止" }));
    expect(onAbort).toHaveBeenCalledOnce();
  });
});
