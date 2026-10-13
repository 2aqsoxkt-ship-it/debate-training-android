import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { emptySnapshot } from "../session/types";
import type { CrossExamSnapshot } from "../arena/CrossExamPanel";
import { MatchPage } from "./MatchPage";

const invoke = vi.fn();
// jsdom does not implement the browser's element scrolling API.
HTMLElement.prototype.scrollTo = vi.fn();
// Fake IPC at the app boundary: concurrent dynamic core imports in jsdom can
// resolve the native implementation instead of the module mock.
vi.mock("../../lib/tauri", () => ({
  call: (...args: unknown[]) => invoke(...args),
}));
// A real native event subscription is unavailable in jsdom. Keep it isolated
// from the command mock, especially during simultaneous AI start/takeover.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

const crossBase: CrossExamSnapshot = {
  eventId: "cross-stable",
  phase: "question",
  questioner: "AFF_4",
  respondent: "NEG_1",
  questionerHuman: false,
  respondentHuman: false,
  question: "",
  visiblePrefix: "",
  interruptible: true,
  answerPrepared: false,
  revealedUnits: 0,
  totalUnits: 0,
  roundNumber: 1,
  nextSide: "affirmative",
  totalMs: 180_000,
  usedMs: 0,
  remainingMs: 180_000,
  meteringMode: "words",
  remainingUnits: 4320,
  questionWordCap: 720,
  answerWordCap: 720,
  selectionSummary: "合法派位",
  rolesSelected: true,
  eligibleQuestioners: ["AFF_4"],
  history: [],
};
function crossView(human = false) {
  return {
    ...emptySnapshot,
    matchId: "match-stable",
    phase: "in_progress",
    currentHuman: human,
    currentWordCap: 720,
    remainingUnits: 4320,
    seats: [
      {
        index: 3,
        name: "正方四辩",
        duty: "质询",
        side: "affirmative",
        controller: human ? "human" : "ai",
        speaking: true,
        claimable: true,
      },
    ],
    schedule: [
      {
        id: "cross-stable",
        kind: "cross_exam",
        speaker: "AFF_4",
        durationSeconds: 180,
        wordCap: 720,
        interruptible: true,
      },
    ],
  };
}
function renderStableMatch() {
  render(
    <MemoryRouter initialEntries={["/match/match-stable"]}>
      <Routes>
        <Route path="/match/:matchId" element={<MatchPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("当前发言控制权来回切换", () => {
  it("问题提交后不再保存已关闭的问题草稿，仍可把回答切给AI", async () => {
    invoke.mockReset();
    let cross = { ...crossBase, questionerHuman: true, respondentHuman: true };
    const view = () => ({
      ...crossView(true),
      currentWindowId: "draft-window-stable",
    });
    invoke.mockImplementation(
      (
        command: string,
        args?: { key?: { fieldKind: string }; question?: string },
      ) => {
        if (command === "workspace_snapshot") return Promise.resolve(view());
        if (command === "cross_exam_snapshot") return Promise.resolve(cross);
        if (command === "load_draft") return Promise.resolve(null);
        if (command === "save_draft")
          return Promise.resolve(
            args?.key?.fieldKind !== "question" || cross.phase === "question",
          );
        if (command === "cross_exam_submit_question") {
          cross = {
            ...cross,
            phase: "preparing",
            question: args?.question ?? "",
          };
          return Promise.resolve(view());
        }
        if (command === "set_current_controller") {
          cross = { ...cross, respondentHuman: false };
          return Promise.resolve(view());
        }
        return Promise.resolve(undefined);
      },
    );
    renderStableMatch();
    fireEvent.change(await screen.findByRole("textbox", { name: "真人提问" }), {
      target: { value: "电脑也有这些能力吗？" },
    });
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(([command]) => command === "save_draft"),
      ).toBe(true),
    );
    fireEvent.click(screen.getByRole("button", { name: "提交问题" }));
    fireEvent.click(await screen.findByRole("button", { name: "AI 回答" }));
    expect(
      await screen.findByRole("button", { name: "AI 准备回答" }),
    ).toBeEnabled();
    expect(screen.queryByText("草稿窗口已关闭或版本已更新")).toBeNull();
  });
  it("陈词人工→AI→人工保留草稿，不提交也不推进赛程", async () => {
    invoke.mockReset();
    let human = true;
    const view = () => ({
      ...crossView(human),
      meteringMode: "words",
      mode: "training",
      seats: [
        {
          index: 0,
          name: "正方一辩",
          duty: "陈词",
          side: "affirmative",
          controller: human ? "human" : "ai",
          speaking: true,
          claimable: true,
        },
      ],
      schedule: [
        {
          id: "opening",
          kind: "opening",
          speaker: "AFF_1",
          durationSeconds: 180,
          wordCap: 720,
          interruptible: false,
        },
      ],
    });
    invoke.mockImplementation(
      (command: string, args?: { toHuman?: boolean }) => {
        if (command === "set_current_controller")
          human = Boolean(args?.toHuman);
        return Promise.resolve(view());
      },
    );
    renderStableMatch();
    const input = await screen.findByLabelText("本环节回答");
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.input(input, { target: { value: "保留这份尚未提交的完整草稿" } });
    fireEvent.click(screen.getByRole("button", { name: "交给 AI" }));
    await waitFor(() => expect(input).toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "我来发言" }));
    await waitFor(() => expect(input).toBeEnabled());
    expect(input).toHaveValue("保留这份尚未提交的完整草稿");
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "submit_turn" || command === "advance_ai",
      ),
    ).toHaveLength(0);
  });

  it("质询人工提问可交回AI再接回人工", async () => {
    invoke.mockReset();
    let human = true;
    invoke.mockImplementation(
      (command: string, args?: { toHuman?: boolean }) => {
        if (command === "set_current_controller")
          human = Boolean(args?.toHuman);
        if (command === "cross_exam_snapshot")
          return Promise.resolve({ ...crossBase, questionerHuman: human });
        return Promise.resolve(crossView(human));
      },
    );
    renderStableMatch();
    const input = await screen.findByLabelText("真人提问");
    fireEvent.input(input, { target: { value: "这份提问草稿要保留" } });
    fireEvent.click(screen.getByRole("button", { name: "交给 AI 提问" }));
    await screen.findByRole("button", { name: "AI 生成并提交问题" });
    fireEvent.click(screen.getByRole("button", { name: "我来提问" }));
    expect(await screen.findByLabelText("真人提问")).toHaveValue(
      "这份提问草稿要保留",
    );
  });

  it("AI 回答等待中可直接接管，并反复交回AI而不丢失回答草稿", async () => {
    invoke.mockReset();
    let human = false;
    let resolveAnswer: (snapshot: CrossExamSnapshot) => void = () => {};
    const cross = () => ({
      ...crossBase,
      phase: "preparing" as const,
      question: "当前问题",
      respondentHuman: human,
    });
    invoke.mockImplementation(
      (command: string, args?: { toHuman?: boolean }) => {
        if (command === "workspace_snapshot")
          return Promise.resolve(crossView(human));
        if (command === "cross_exam_snapshot") return Promise.resolve(cross());
        if (command === "cross_exam_generate_answer")
          return new Promise<CrossExamSnapshot>((resolve) => {
            resolveAnswer = resolve;
          });
        if (command === "set_current_controller") {
          human = Boolean(args?.toHuman);
          return Promise.resolve(crossView(human));
        }
        return Promise.resolve(undefined);
      },
    );
    renderStableMatch();
    fireEvent.click(await screen.findByRole("button", { name: "AI 准备回答" }));
    expect(screen.getByRole("button", { name: "真人回答" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "真人回答" }));
    const input = await screen.findByLabelText("质询回答");
    fireEvent.input(input, { target: { value: "这份回答仍由我修改" } });
    fireEvent.click(screen.getByRole("button", { name: "AI 回答" }));
    await screen.findByRole("button", { name: "AI 准备回答" });
    fireEvent.click(screen.getByRole("button", { name: "真人回答" }));
    expect(await screen.findByLabelText("质询回答")).toHaveValue(
      "这份回答仍由我修改",
    );
    await act(async () => {
      resolveAnswer({
        ...cross(),
        phase: "revealing",
        answerPrepared: true,
        totalUnits: 10,
      });
    });
    expect(screen.getByLabelText("质询回答")).toHaveValue("这份回答仍由我修改");
    expect(screen.queryByText(/已展示/)).toBeNull();
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "set_current_controller",
      ),
    ).toHaveLength(3);
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "cross_exam_prepare_answer",
      ),
    ).toBe(false);
  });
});

describe("质询权威快照与异步恢复", () => {
  afterEach(() => vi.restoreAllMocks());

  it("同一质询环节点辩席接管后立即刷新提问控制台", async () => {
    invoke.mockReset();
    let human = false;
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve(crossView(human));
      if (command === "cross_exam_snapshot")
        return Promise.resolve({ ...crossBase, questionerHuman: human });
      if (command === "set_current_controller") {
        human = true;
        return Promise.resolve(crossView(true));
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    await screen.findByRole("button", { name: "AI 生成并提交问题" });
    fireEvent.click(screen.getByRole("button", { name: /正方四辩，AI/ }));
    expect(await screen.findByLabelText("真人提问")).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "AI 生成并提交问题" }),
    ).toBeNull();
  });

  it("取消 AI 并人工插入后，迟到的生成响应不能覆盖人工提问", async () => {
    invoke.mockReset();
    let resolveGeneration: (snapshot: CrossExamSnapshot) => void = () => {};
    let cross = { ...crossBase };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve(crossView(cross.questionerHuman));
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "cross_exam_generate_question")
        return new Promise<CrossExamSnapshot>((resolve) => {
          resolveGeneration = resolve;
        });
      if (command === "cross_exam_insert_human_questioner") {
        cross = { ...crossBase, questionerHuman: true };
        return Promise.resolve(cross);
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.click(
      await screen.findByRole("button", { name: "AI 生成并提交问题" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "正方四辩" }));
    await screen.findByLabelText("真人提问");
    await act(async () => {
      resolveGeneration({
        ...crossBase,
        phase: "preparing",
        question: "旧问题",
      });
    });
    expect(screen.getByLabelText("真人提问")).toBeEnabled();
    expect(screen.queryByText("问：旧问题")).toBeNull();
  });

  it("取消 AI 回答并切换真人后，迟到回答不能重新进入展示", async () => {
    invoke.mockReset();
    let resolveAnswer: (snapshot: CrossExamSnapshot) => void = () => {};
    let cross = {
      ...crossBase,
      phase: "preparing" as const,
      question: "当前问题",
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") return Promise.resolve(crossView());
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "cross_exam_generate_answer")
        return new Promise<CrossExamSnapshot>((resolve) => {
          resolveAnswer = resolve;
        });
      if (command === "set_current_controller") {
        cross = { ...cross, respondentHuman: true };
        return Promise.resolve(crossView(true));
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.click(await screen.findByRole("button", { name: "AI 准备回答" }));
    fireEvent.click(screen.getAllByRole("button", { name: "取消生成" })[0]);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "真人回答" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "真人回答" }));
    await screen.findByLabelText("质询回答");
    await act(async () => {
      resolveAnswer({
        ...cross,
        respondentHuman: false,
        phase: "revealing",
        answerPrepared: true,
        totalUnits: 2,
      });
    });
    expect(screen.getByLabelText("质询回答")).toBeEnabled();
    expect(screen.queryByText(/已展示/)).toBeNull();
  });

  it("回答已入账但返回失败时协调权威快照，不重扣字数", async () => {
    invoke.mockReset();
    let committed = false;
    const revealing = {
      ...crossBase,
      phase: "revealing" as const,
      question: "问题",
      answerPrepared: true,
      visiblePrefix: "答案",
      revealedUnits: 2,
      totalUnits: 2,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve({
          ...crossView(),
          remainingUnits: committed ? 4318 : 4320,
          affirmativeMainUnits: committed ? 4318 : 4320,
        });
      if (command === "cross_exam_snapshot")
        return Promise.resolve(
          committed
            ? { ...crossBase, roundNumber: 2, remainingUnits: 4318 }
            : revealing,
        );
      if (command === "cross_exam_complete") {
        committed = true;
        return Promise.reject(new Error("保存响应失败"));
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    await waitFor(() =>
      expect(
        invoke.mock.calls.filter(
          ([command]) => command === "cross_exam_complete",
        ),
      ).toHaveLength(1),
    );
    expect(
      await screen.findByRole("button", { name: "AI 生成并提交问题" }),
    ).toBeEnabled();
    expect(screen.getByText("正方主字池").parentElement).toHaveTextContent(
      "4,318 字",
    );
    expect(screen.queryByRole("button", { name: "重试提交回答" })).toBeNull();
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "cross_exam_complete",
      ),
    ).toHaveLength(1);
  });

  it("未入账的回答失败后人工重试，先读取权威状态再提交一次", async () => {
    invoke.mockReset();
    let committed = false;
    let completes = 0;
    const revealing = {
      ...crossBase,
      phase: "revealing" as const,
      question: "问题",
      answerPrepared: true,
      visiblePrefix: "答案",
      revealedUnits: 2,
      totalUnits: 2,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") return Promise.resolve(crossView());
      if (command === "cross_exam_snapshot")
        return Promise.resolve(
          committed ? { ...crossBase, roundNumber: 2 } : revealing,
        );
      if (command === "cross_exam_complete") {
        completes += 1;
        if (completes === 1) return Promise.reject(new Error("回答尚未保存"));
        committed = true;
        return Promise.resolve(crossView());
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    const retry = await screen.findByRole("button", { name: "重试提交回答" });
    expect(completes).toBe(1);
    fireEvent.click(retry);
    await screen.findByRole("button", { name: "AI 生成并提交问题" });
    expect(completes).toBe(2);
    const secondComplete = invoke.mock.calls
      .map(([command]) => command)
      .lastIndexOf("cross_exam_complete");
    expect(invoke.mock.calls[secondComplete - 1][0]).toBe(
      "cross_exam_snapshot",
    );
  });
});

describe("异步响应与未知提交状态边界", () => {
  afterEach(() => vi.restoreAllMocks());

  it.each([
    ["question", "AFF_2", 1, "正方二辩", "真人提问"],
    ["preparing", "NEG_2", 5, "反方二辩", "质询回答"],
  ] as const)(
    "动态%s角色接管后使用当前问答控制快照",
    async (phase, slot, index, name, inputLabel) => {
      invoke.mockReset();
      let human = false;
      const stageView = () => ({
        ...crossView(human),
        seats: [
          {
            index,
            name,
            duty: "质询",
            side: index < 4 ? "affirmative" : "negative",
            controller: human ? "human" : "ai",
            speaking: true,
            claimable: true,
          },
        ],
      });
      invoke.mockImplementation((command: string) => {
        if (command === "workspace_snapshot")
          return Promise.resolve(stageView());
        if (command === "cross_exam_snapshot")
          return Promise.resolve({
            ...crossBase,
            phase,
            rolesSelected: true,
            questioner: phase === "question" ? slot : "AFF_4",
            respondent: phase === "preparing" ? slot : "NEG_1",
            questionerHuman: phase === "question" && human,
            respondentHuman: phase === "preparing" && human,
          });
        if (command === "set_current_controller") {
          human = true;
          return Promise.resolve(stageView());
        }
        return Promise.resolve(undefined);
      });
      renderStableMatch();
      await screen.findByText("派位说明：合法派位");
      fireEvent.click(
        screen.getByRole("button", { name: new RegExp(`${name}，AI`) }),
      );
      expect(await screen.findByLabelText(inputLabel)).toBeEnabled();
      const take = invoke.mock.calls.find(
        ([command]) => command === "set_current_controller",
      );
      expect(take?.[1].slotIndex).toBe(index);
      expect(take?.[1].scope).toBe("turn");
    },
  );

  it("提交回答尚未返回时点击接管不会取消提交或丢弃成功快照", async () => {
    invoke.mockReset();
    let resolveComplete: (
      snapshot: ReturnType<typeof crossView>,
    ) => void = () => {};
    let committed = false;
    const revealing = {
      ...crossBase,
      phase: "revealing" as const,
      question: "问题",
      answerPrepared: true,
      visiblePrefix: "答案",
      revealedUnits: 2,
      totalUnits: 2,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") return Promise.resolve(crossView());
      if (command === "cross_exam_snapshot")
        return Promise.resolve(
          committed ? { ...crossBase, roundNumber: 2 } : revealing,
        );
      if (command === "cross_exam_complete")
        return new Promise<ReturnType<typeof crossView>>((resolve) => {
          resolveComplete = resolve;
        });
      if (command === "set_current_controller")
        return Promise.resolve(crossView(true));
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(
          ([command]) => command === "cross_exam_complete",
        ),
      ).toBe(true),
    );
    fireEvent.click(screen.getByRole("button", { name: /正方四辩，AI/ }));
    await act(async () => {});
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "set_current_controller",
      ),
    ).toBe(false);
    await act(async () => {
      committed = true;
      resolveComplete(crossView());
    });
    expect(
      await screen.findByRole("button", { name: "AI 生成并提交问题" }),
    ).toBeEnabled();
  });

  it("展示中接管等待期间逐字请求不会撤销接管", async () => {
    invoke.mockReset();
    let resolveTakeover: (
      snapshot: ReturnType<typeof crossView>,
    ) => void = () => {};
    let cross = {
      ...crossBase,
      phase: "revealing" as const,
      question: "问题",
      answerPrepared: true,
      visiblePrefix: "答",
      revealedUnits: 1,
      totalUnits: 100,
    };
    invoke.mockImplementation((command: string, args?: { units: number }) => {
      if (command === "workspace_snapshot") return Promise.resolve(crossView());
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "set_current_controller")
        return new Promise<ReturnType<typeof crossView>>((resolve) => {
          resolveTakeover = resolve;
        });
      if (command === "cross_exam_reveal") {
        cross = { ...cross, revealedUnits: args?.units ?? 1 };
        return Promise.resolve(cross);
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    await screen.findByText(/已展示 1\/100/);
    fireEvent.click(screen.getByRole("button", { name: /正方四辩，AI/ }));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 400));
    });
    expect(
      invoke.mock.calls.filter(([command]) => command === "cross_exam_reveal"),
    ).toHaveLength(0);
    await act(async () => {
      resolveTakeover(crossView(true));
    });
    expect(
      screen.getByRole("button", { name: /正方四辩，真人/ }),
    ).toBeInTheDocument();
  });

  it("完成失败且权威读取失败时，重试和结束环节都不能盲写", async () => {
    invoke.mockReset();
    let firstRead = true;
    const revealing = {
      ...crossBase,
      phase: "revealing" as const,
      question: "问题",
      answerPrepared: true,
      visiblePrefix: "答案",
      revealedUnits: 2,
      totalUnits: 2,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") {
        if (firstRead) {
          firstRead = false;
          return Promise.resolve(crossView());
        }
        return Promise.reject(new Error("现场读取失败"));
      }
      if (command === "cross_exam_snapshot") return Promise.resolve(revealing);
      if (command === "cross_exam_complete")
        return Promise.reject(new Error("提交状态未知"));
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.click(
      await screen.findByRole("button", { name: "重试提交回答" }),
    );
    await screen.findByRole("button", { name: "重试提交回答" });
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "cross_exam_complete",
      ),
    ).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "结束整个质询环节" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "打断" })).toBeDisabled();
  });

  it("普通 AI 发言迟到响应也不能覆盖同辩位人工接管", async () => {
    invoke.mockReset();
    let resolveAi: (snapshot: ReturnType<typeof crossView>) => void = () => {};
    let human = false;
    const speech = () => {
      const next = crossView(human);
      return { ...next, schedule: [{ ...next.schedule[0], kind: "opening" }] };
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") return Promise.resolve(speech());
      if (command === "advance_ai")
        return new Promise<ReturnType<typeof crossView>>((resolve) => {
          resolveAi = resolve;
        });
      if (command === "set_current_controller") {
        human = true;
        return Promise.resolve(speech());
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.click(
      await screen.findByRole("button", { name: "让模型完成本轮" }),
    );
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(([command]) => command === "advance_ai"),
      ).toBe(true),
    );
    fireEvent.click(screen.getByRole("button", { name: /正方四辩，AI/ }));
    await waitFor(() =>
      expect(screen.getByLabelText("本环节回答")).toBeEnabled(),
    );
    await act(async () => {
      resolveAi({ ...speech(), currentHuman: false });
    });
    expect(screen.getByLabelText("本环节回答")).toBeEnabled();
  });
});

describe("比赛输入提交", () => {
  afterEach(() => vi.restoreAllMocks());

  it("自由辩同一环节的真人回合分别计时，不计入两轮间AI等待", async () => {
    invoke.mockReset();
    let now = 100000;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    let transcriptCount = 0;
    const snapshot = (human: boolean) => ({
      ...emptySnapshot,
      matchId: "match-1",
      phase: "in_progress",
      currentHuman: human,
      currentWordCap: 960,
      remainingUnits: 960,
      floorHolder: "affirmative",
      transcripts: Array.from({ length: transcriptCount }, (_, index) => ({
        id: `t-${index}`,
        slot: "NEG_1",
        effectiveText: "对方发言",
        overflowText: "",
        interruptedTail: "",
      })),
      schedule: [
        {
          id: "same-free-event",
          kind: "free_debate",
          speaker: "AFF_1",
          durationSeconds: 240,
          wordCap: 960,
          interruptible: false,
        },
      ],
    });
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve(snapshot(false));
      if (command === "advance_ai") {
        now += 300000;
        transcriptCount += 1;
        return Promise.resolve(snapshot(true));
      }
      if (command === "submit_turn") {
        transcriptCount += 1;
        return Promise.resolve(snapshot(false));
      }
      return Promise.resolve(undefined);
    });
    render(
      <MemoryRouter initialEntries={["/match/match-1"]}>
        <Routes>
          <Route path="/match/:matchId" element={<MatchPage />} />
        </Routes>
      </MemoryRouter>,
    );
    const input = await screen.findByLabelText("本环节回答");
    for (const elapsed of [2000, 1000]) {
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "让模型完成本轮" }),
        ).toBeEnabled(),
      );
      fireEvent.click(screen.getByRole("button", { name: "让模型完成本轮" }));
      await waitFor(() => expect(input).toBeEnabled());
      now += elapsed;
      act(() => {
        input.dispatchEvent(
          new InputEvent("beforeinput", {
            bubbles: true,
            inputType: "insertText",
            data: "真人回应",
          }),
        );
        fireEvent.input(input, { target: { value: "真人回应" } });
      });
      fireEvent.click(screen.getByRole("button", { name: "提交本轮发言" }));
      await waitFor(() => expect(input).toBeDisabled());
    }
    const submissions = invoke.mock.calls.filter(
      ([command]) => command === "submit_turn",
    );
    expect(submissions.map(([, args]) => args.claimed.inputElapsedMs)).toEqual([
      2000, 1000,
    ]);
    expect(
      submissions.map(([, args]) => args.claimed.forwardTypedUnits),
    ).toEqual([4, 4]);
  });

  it("质询真人回答只从可回答阶段计时，不包括AI提问等待", async () => {
    invoke.mockReset();
    let now = 100000;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const snapshot = {
      ...emptySnapshot,
      matchId: "match-1",
      phase: "in_progress",
      currentWordCap: 720,
      remainingUnits: 4320,
      schedule: [
        {
          id: "cross-1",
          kind: "cross_exam",
          speaker: "AFF_4",
          durationSeconds: 180,
          wordCap: 720,
          interruptible: true,
        },
      ],
    };
    let cross = {
      eventId: "cross-1",
      phase: "question",
      questioner: "AFF_4",
      respondent: "NEG_1",
      questionerHuman: false,
      respondentHuman: true,
      question: "",
      visiblePrefix: "",
      interruptible: true,
      answerPrepared: false,
      revealedUnits: 0,
      totalUnits: 0,
      roundNumber: 1,
      nextSide: "affirmative",
      totalMs: 180000,
      usedMs: 0,
      remainingMs: 180000,
      meteringMode: "words",
      remainingUnits: 4320,
      questionWordCap: 720,
      answerWordCap: 720,
      selectionSummary: "合法派位",
      rolesSelected: true,
      eligibleQuestioners: ["AFF_4"],
      history: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") return Promise.resolve(snapshot);
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "cross_exam_generate_question") {
        now += 300000;
        cross = { ...cross, phase: "preparing", question: "你如何证明？" };
        return Promise.resolve(cross);
      }
      if (command === "cross_exam_prepare_answer")
        return Promise.resolve({
          ...cross,
          phase: "revealing",
          answerPrepared: true,
        });
      return Promise.resolve(undefined);
    });
    render(
      <MemoryRouter initialEntries={["/match/match-1"]}>
        <Routes>
          <Route path="/match/:matchId" element={<MatchPage />} />
        </Routes>
      </MemoryRouter>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "AI 生成并提交问题" }),
    );
    const input = await screen.findByLabelText("质询回答");
    await waitFor(() => expect(input).toBeEnabled());
    now += 2000;
    act(() => {
      input.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          inputType: "insertText",
          data: "回答依据",
        }),
      );
      fireEvent.input(input, { target: { value: "回答依据" } });
    });
    fireEvent.click(screen.getByRole("button", { name: "准备并开始展示" }));
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(
          ([command]) => command === "cross_exam_prepare_answer",
        ),
      ).toBe(true),
    );
    const submission = invoke.mock.calls.find(
      ([command]) => command === "cross_exam_prepare_answer",
    );
    expect(submission?.[1].claimed.inputElapsedMs).toBe(2000);
    expect(submission?.[1].claimed.forwardTypedUnits).toBe(4);
  });
  it.each(["words", "time"] as const)(
    "%s 模式：后端拒绝提交时保留草稿和遥测，重试仍发送原始输入记录",
    async (meteringMode) => {
      invoke.mockReset();
      const snapshot = {
        ...emptySnapshot,
        matchId: "match-1",
        phase: "in_progress",
        currentHuman: true,
        meteringMode,
        currentWordCap: 480,
        remainingUnits: 4320,
        remainingMs: 120_000,
        schedule: [
          {
            id: "event-1",
            kind: "opening",
            speaker: "AFF_1",
            durationSeconds: 120,
            wordCap: 480,
            interruptible: false,
          },
        ],
      };
      invoke.mockImplementation((command: string) => {
        if (command === "workspace_snapshot") return Promise.resolve(snapshot);
        if (command === "submit_turn")
          return Promise.reject(new Error("提交尚未保存"));
        return Promise.resolve(undefined);
      });
      render(
        <MemoryRouter initialEntries={["/match/match-1"]}>
          <Routes>
            <Route path="/match/:matchId" element={<MatchPage />} />
          </Routes>
        </MemoryRouter>,
      );
      const input = await screen.findByLabelText("本环节回答");
      await waitFor(() => expect(input).toBeEnabled());
      act(() => {
        input.dispatchEvent(
          new InputEvent("beforeinput", {
            bubbles: true,
            inputType: "insertText",
            data: "完整草稿",
          }),
        );
        fireEvent.input(input, { target: { value: "完整草稿" } });
      });
      fireEvent.click(screen.getByRole("button", { name: "提交本轮发言" }));
      await screen.findByText("提交尚未保存");
      expect(input).toHaveValue("完整草稿");
      fireEvent.click(screen.getByRole("button", { name: "提交本轮发言" }));
      await waitFor(() =>
        expect(
          invoke.mock.calls.filter(([command]) => command === "submit_turn"),
        ).toHaveLength(2),
      );
      const submissions = invoke.mock.calls.filter(
        ([command]) => command === "submit_turn",
      );
      expect(submissions[1][1].text).toBe("完整草稿");
      expect(submissions[1][1].events).toEqual(submissions[0][1].events);
      expect(submissions[1][1].claimed.forwardTypedUnits).toBe(4);
    },
  );
});

describe("真人写入期间不接受 AI 取消或接管", () => {
  afterEach(() => vi.restoreAllMocks());

  it.each(["取消", "接管", "中止"])(
    "普通发言提交期间禁止%s，成功响应不会丢失",
    async (action) => {
      invoke.mockReset();
      let resolveSubmit: (
        snapshot: ReturnType<typeof crossView>,
      ) => void = () => {};
      const start = {
        ...crossView(true),
        schedule: [{ ...crossView().schedule[0], kind: "opening" }],
      };
      invoke.mockImplementation((command: string) => {
        if (command === "workspace_snapshot") return Promise.resolve(start);
        if (command === "submit_turn")
          return new Promise<ReturnType<typeof crossView>>((resolve) => {
            resolveSubmit = resolve;
          });
        if (command === "set_current_controller") return Promise.resolve(start);
        return Promise.resolve(undefined);
      });
      renderStableMatch();
      const input = await screen.findByLabelText("本环节回答");
      fireEvent.input(input, { target: { value: "已经发出的正文" } });
      fireEvent.click(screen.getByRole("button", { name: "提交本轮发言" }));
      await waitFor(() =>
        expect(
          invoke.mock.calls.some(([command]) => command === "submit_turn"),
        ).toBe(true),
      );
      if (action === "取消")
        expect(screen.queryByRole("button", { name: "取消生成" })).toBeNull();
      if (action === "接管") {
        const seat = screen.getByRole("button", { name: /正方四辩，真人/ });
        fireEvent.click(seat);
        await act(async () => {});
        expect(
          invoke.mock.calls.some(
            ([command]) => command === "set_current_controller",
          ),
        ).toBe(false);
      }
      if (action === "中止")
        expect(screen.getByRole("button", { name: "中止比赛" })).toBeDisabled();
      await act(async () => {
        resolveSubmit({
          ...start,
          currentHuman: false,
          transcripts: [
            {
              id: "saved-human",
              slot: "AFF_4",
              effectiveText: "已经发出的正文",
              overflowText: "",
              interruptedTail: "",
              respondsTo: "",
              responseTargetText: "",
            },
          ],
        });
      });
      expect(input).toHaveValue("");
      expect(input).toBeDisabled();
      expect(screen.getByRole("log")).toHaveTextContent("已经发出的正文");
      expect(
        invoke.mock.calls.filter(([command]) => command === "submit_turn"),
      ).toHaveLength(1);
    },
  );

  it("真人问题提交期间不能插入其他提问辩位或取消写入", async () => {
    invoke.mockReset();
    let resolveQuestion: (
      snapshot: ReturnType<typeof crossView>,
    ) => void = () => {};
    let cross: CrossExamSnapshot = {
      ...crossBase,
      questionerHuman: true,
      eligibleQuestioners: ["AFF_2", "AFF_4"],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve(crossView(true));
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "cross_exam_submit_question")
        return new Promise<ReturnType<typeof crossView>>((resolve) => {
          resolveQuestion = resolve;
        });
      if (command === "cross_exam_insert_human_questioner")
        return Promise.resolve({ ...cross, questioner: "AFF_2" });
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.input(await screen.findByLabelText("真人提问"), {
      target: { value: "真实问题" },
    });
    fireEvent.click(screen.getByRole("button", { name: "提交问题" }));
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(
          ([command]) => command === "cross_exam_submit_question",
        ),
      ).toBe(true),
    );
    expect(screen.queryByRole("button", { name: "取消生成" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "正方二辩" }));
    await act(async () => {});
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "cross_exam_insert_human_questioner",
      ),
    ).toBe(false);
    await act(async () => {
      cross = { ...cross, phase: "preparing", question: "真实问题" };
      resolveQuestion(crossView(true));
    });
    expect(
      await screen.findByRole("button", { name: "AI 准备回答" }),
    ).toBeEnabled();
    expect(screen.getByText("问：真实问题")).toBeInTheDocument();
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "cross_exam_submit_question",
      ),
    ).toHaveLength(1);
  });

  it("真人回答准备期间没有取消生成，成功后进入原回答展示", async () => {
    invoke.mockReset();
    let resolvePrepared: (snapshot: CrossExamSnapshot) => void = () => {};
    const cross = {
      ...crossBase,
      phase: "preparing" as const,
      respondentHuman: true,
      question: "问题",
    };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot") return Promise.resolve(crossView());
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "cross_exam_prepare_answer")
        return new Promise<CrossExamSnapshot>((resolve) => {
          resolvePrepared = resolve;
        });
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.input(await screen.findByLabelText("质询回答"), {
      target: { value: "真实答案" },
    });
    fireEvent.click(screen.getByRole("button", { name: "准备并开始展示" }));
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(
          ([command]) => command === "cross_exam_prepare_answer",
        ),
      ).toBe(true),
    );
    expect(screen.queryByRole("button", { name: "取消生成" })).toBeNull();
    expect(screen.getByRole("button", { name: /正方四辩，AI/ })).toBeDisabled();
    await act(async () => {
      resolvePrepared({
        ...cross,
        phase: "revealing",
        answerPrepared: true,
        totalUnits: 4,
      });
    });
    expect(await screen.findByText(/已展示 0\/4/)).toBeInTheDocument();
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "cross_exam_prepare_answer",
      ),
    ).toHaveLength(1);
  });

  it("AI 派位选中真人后生成拒绝，仍同步实际派位并显示真人提问", async () => {
    invoke.mockReset();
    let cross = { ...crossBase, rolesSelected: false };
    invoke.mockImplementation((command: string) => {
      if (command === "workspace_snapshot")
        return Promise.resolve(crossView(cross.questionerHuman));
      if (command === "cross_exam_snapshot") return Promise.resolve(cross);
      if (command === "cross_exam_generate_question") {
        cross = {
          ...cross,
          questioner: "AFF_2",
          questionerHuman: true,
          rolesSelected: true,
          selectionSummary: "AI 派位选择了真人二辩",
        };
        return Promise.reject(new Error("当前提问者由真人控制"));
      }
      return Promise.resolve(undefined);
    });
    renderStableMatch();
    fireEvent.click(
      await screen.findByRole("button", { name: "AI 生成并提交问题" }),
    );
    expect(await screen.findByLabelText("真人提问")).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "AI 生成并提交问题" }),
    ).toBeNull();
    expect(
      screen.getByText(/派位说明：AI 派位选择了真人二辩/),
    ).toBeInTheDocument();
  });
});
