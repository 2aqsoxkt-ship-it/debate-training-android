import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Link, MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  emptySnapshot,
  type RuleDraft,
  type RuleStage,
  type Snapshot,
} from "../session/types";
import { WorkspacePage } from "./WorkspacePage";

const invoke = vi.fn();
let profileSequence = 0;

const ruleDraft: RuleDraft = {
  schemaVersion: "ruleset.v2",
  planVersion: "match-plan.v3",
  rulesetName: "已保存赛制",
  sourceText: "完整赛制原文",
  affirmativeDuties: ["立论", "论证", "质询", "总结"],
  negativeDuties: ["立论", "论证", "质询", "总结"],
  stages: [],
  clockPools: [],
  sourceClauses: [],
  coverageEntries: [],
  migrationReviewRequired: false,
};

function setupSnapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  const profileId = `setup-profile-${++profileSequence}`;
  return {
    ...emptySnapshot,
    profiles: [{ id: profileId, displayName: "训练档案" }],
    activeProfileId: profileId,
    ...overrides,
  };
}

function preparedSnapshot(): Snapshot {
  return setupSnapshot({
    matchId: `setup-match-${profileSequence}`,
    phase: "ready",
    mode: "training",
    topic: "已保存辩题",
    materials: "已保存材料",
    userSide: "negative",
    difficulty: "contest",
    preparationReady: true,
    rulesConfirmed: true,
    lineupConfirmed: true,
    ruleDraft,
    ownStrategy: "已保存策略",
    rolePackets: [
      {
        slot: "NEG_2",
        identity: "反方二辩",
        duty: "申论",
        mainArgument: "已保存论证",
        backupRebuttal: "备用反驳",
        crossExamTarget: "追问目标",
        defenseFloor: "防守",
        summaryCallback: "总结",
      },
    ],
    seats: Array.from({ length: 8 }, (_, index) => ({
      index,
      name: `辩手${index}`,
      duty: "职责",
      side: index < 4 ? "affirmative" : "negative",
      controller: index === 5 ? "human" : "ai",
      speaking: false,
      claimable: false,
    })),
  });
}

function renderWorkspace() {
  return render(
    <MemoryRouter initialEntries={["/workspace"]}>
      <Routes>
        <Route path="/workspace" element={<WorkspacePage />} />
        <Route
          path="/settings"
          element={<Link to="/workspace">返回赛前工作台</Link>}
        />
      </Routes>
    </MemoryRouter>,
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

describe("赛前分步工作台", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(setupSnapshot());
  });

  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("一次只显示一个主要任务并保留步骤导航", async () => {
    render(
      <MemoryRouter initialEntries={["/workspace"]}>
        <Routes>
          <Route path="/workspace" element={<WorkspacePage />} />
        </Routes>
      </MemoryRouter>,
    );

    expect(
      await screen.findByRole("heading", { name: "选择档案与训练模式" }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("闭卷材料")).toBeNull();
    await screen.findByText("当前档案");
    fireEvent.click(screen.getByRole("button", { name: "保存并继续" }));
    expect(
      screen.getByRole("heading", { name: "输入辩题与参考材料" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("闭卷材料")).toBeInTheDocument();
  });

  it("restores the saved side, difficulty, source, materials and actual human seats", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /档案与模式/ }));
    expect(screen.getByLabelText("参赛阵营")).toHaveValue("negative");
    expect(screen.getByLabelText("AI 难度")).toHaveValue("contest");
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    expect(screen.getByLabelText("闭卷材料")).toHaveValue("已保存材料");
    fireEvent.click(screen.getByRole("button", { name: /赛制来源/ }));
    expect(screen.getByLabelText("赛制说明")).toHaveValue("完整赛制原文");
    fireEvent.click(screen.getByRole("button", { name: /队伍与辩位/ }));
    expect(
      screen.getByRole("button", { name: /反方二辩.*人工控制/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: /正方一辩.*AI 控制/ }),
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("preserves unsaved preparation fields and the active step across model settings navigation", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /档案与模式/ }));
    fireEvent.change(screen.getByLabelText("AI 难度"), {
      target: { value: "beginner" },
    });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.change(screen.getByLabelText("辩题"), {
      target: { value: "未保存辩题" },
    });
    fireEvent.change(screen.getByLabelText("闭卷材料"), {
      target: { value: "未保存材料" },
    });
    fireEvent.click(screen.getByRole("button", { name: /赛制来源/ }));
    fireEvent.change(screen.getByLabelText("赛制说明"), {
      target: { value: "未保存赛制说明" },
    });
    fireEvent.click(screen.getByRole("link", { name: "模型设置" }));
    fireEvent.click(screen.getByRole("link", { name: "返回赛前工作台" }));
    await screen.findByLabelText("赛制说明");
    expect(screen.getByLabelText("赛制说明")).toHaveValue("未保存赛制说明");
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    expect(screen.getByLabelText("辩题")).toHaveValue("未保存辩题");
    expect(screen.getByLabelText("闭卷材料")).toHaveValue("未保存材料");
    fireEvent.click(screen.getByRole("button", { name: /档案与模式/ }));
    expect(screen.getByLabelText("AI 难度")).toHaveValue("beginner");
  });

  it("preserves an unsaved semantic rules buffer across steps and settings", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /赛制编辑与确认/ }));
    fireEvent.change(screen.getByLabelText("赛制名称"), {
      target: { value: "我的未保存赛制" },
    });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.click(screen.getByRole("button", { name: /赛制编辑与确认/ }));
    expect(screen.getByLabelText("赛制名称")).toHaveValue("我的未保存赛制");
    fireEvent.click(screen.getByRole("link", { name: "模型设置" }));
    fireEvent.click(screen.getByRole("link", { name: "返回赛前工作台" }));
    expect(await screen.findByLabelText("赛制名称")).toHaveValue(
      "我的未保存赛制",
    );
  });

  it("blocks launch immediately when preparation or the human lineup changes", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    expect(screen.getByRole("button", { name: "正式开始比赛" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: /队伍与辩位/ }));
    fireEvent.click(screen.getByRole("button", { name: /反方三辩.*AI 控制/ }));
    fireEvent.click(screen.getByRole("button", { name: /开赛确认/ }));
    expect(screen.getByRole("button", { name: "正式开始比赛" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.change(screen.getByLabelText("辩题"), {
      target: { value: "修改后的辩题" },
    });
    fireEvent.click(screen.getByRole("button", { name: /开赛确认/ }));
    expect(screen.getByRole("button", { name: "正式开始比赛" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "确认阵容" })).toBeDisabled();
  });

  it("saves changed preparation before continuing and clears the outdated strategy display", async () => {
    const saved = preparedSnapshot();
    invoke.mockImplementation((command) =>
      Promise.resolve(
        command === "update_preparation"
          ? {
              ...saved,
              topic: "更新的辩题",
              preparationReady: false,
              phase: "rules_confirmed",
              lineupConfirmed: false,
              ownStrategy: null,
              rolePackets: [],
            }
          : saved,
      ),
    );
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.change(screen.getByLabelText("辩题"), {
      target: { value: "更新的辩题" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存并继续" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_preparation", {
        mode: "training",
        topic: "更新的辩题",
        materials: "已保存材料",
        userSide: "negative",
        difficulty: "contest",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: /拆题与策略/ }));
    expect(screen.queryByText("已保存策略")).toBeNull();
    expect(
      screen.getByRole("button", { name: "开始拆题并分配任务" }),
    ).toBeEnabled();
  });

  it("uses a synchronous single-flight gate and leaves help readable while a command is pending", async () => {
    const initial = setupSnapshot();
    const pending = deferred<Snapshot>();
    invoke.mockImplementation((command) =>
      command === "create_profile" ? pending.promise : Promise.resolve(initial),
    );
    renderWorkspace();
    await screen.findByText("当前档案");
    const create = screen.getByRole("button", { name: "建立档案" });
    fireEvent.click(create);
    fireEvent.click(create);
    await waitFor(() =>
      expect(
        invoke.mock.calls.filter(([command]) => command === "create_profile"),
      ).toHaveLength(1),
    );
    expect(screen.getByLabelText("AI 难度")).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存并继续" })).toBeDisabled();
    fireEvent.click(screen.getByRole("link", { name: "模型设置" }));
    expect(screen.queryByRole("link", { name: "返回赛前工作台" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "使用说明" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await act(async () => pending.resolve(initial));
  });

  it("reports a real desktop loading error with retry instead of mislabelling it browser preview", async () => {
    Reflect.set(window, "__TAURI_INTERNALS__", {});
    invoke.mockRejectedValueOnce(new Error("读取本机会话失败"));
    renderWorkspace();
    expect(await screen.findByText("读取本机会话失败")).toBeInTheDocument();
    expect(screen.queryByText("浏览器预览")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "重新加载本机会话" }));
    await screen.findByText("当前档案");
    expect(screen.queryByText("读取本机会话失败")).toBeNull();
  });

  it("hydrates an explicitly resumed preparation rather than retaining another draft", async () => {
    const initial = preparedSnapshot();
    initial.recent = [
      { id: "restored-match", topic: "恢复的辩题", phase: "rules_confirmed" },
    ];
    const restored = {
      ...initial,
      matchId: "restored-match",
      phase: "rules_confirmed",
      topic: "恢复的辩题",
      materials: "恢复的材料",
      difficulty: "standard",
      userSide: "affirmative",
      preparationReady: false,
      lineupConfirmed: false,
      ownStrategy: null,
      rolePackets: [],
    };
    invoke.mockImplementation((command) =>
      Promise.resolve(command === "resume_match" ? restored : initial),
    );
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.change(screen.getByLabelText("闭卷材料"), {
      target: { value: "不应污染恢复比赛" },
    });
    fireEvent.click(screen.getByRole("button", { name: "恢复" }));
    await screen.findByRole("heading", { name: "拆解辩题并分配任务" });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    expect(screen.getByLabelText("闭卷材料")).toHaveValue("恢复的材料");
    expect(screen.getByLabelText("辩题")).toHaveValue("恢复的辩题");
  });

  it("adopts a chosen template's source so it is not treated as an unsaved competing ruleset", async () => {
    const initial = setupSnapshot();
    const opened = {
      ...initial,
      matchId: "opened-template",
      phase: "rules_parsed",
      ruleDraft,
    };
    invoke.mockImplementation((command) =>
      Promise.resolve(command === "open_template" ? opened : initial),
    );
    renderWorkspace();
    await screen.findByText("当前档案");
    fireEvent.click(screen.getByRole("button", { name: "保存并继续" }));
    fireEvent.click(screen.getByRole("button", { name: "保存并继续" }));
    fireEvent.click(screen.getByRole("button", { name: "使用并编辑预设" }));
    await screen.findByLabelText("赛制名称");
    expect(screen.queryByText("赛制有未保存修改")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /赛制来源/ }));
    expect(screen.getByLabelText("赛制说明")).toHaveValue("完整赛制原文");
  });

  it("retains a name and initial config across settings even before creating the first profile", async () => {
    invoke.mockResolvedValue({
      ...emptySnapshot,
      topic: "首次设置辩题",
      userSide: "",
    });
    renderWorkspace();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "建立档案" })).toBeEnabled(),
    );
    fireEvent.change(screen.getByLabelText("新档案名称"), {
      target: { value: "未保存的新档案" },
    });
    fireEvent.change(screen.getByLabelText("AI 难度"), {
      target: { value: "contest" },
    });
    fireEvent.click(screen.getByRole("link", { name: "模型设置" }));
    fireEvent.click(screen.getByRole("link", { name: "返回赛前工作台" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "建立档案" })).toBeEnabled(),
    );
    expect(screen.getByLabelText("新档案名称")).toHaveValue("未保存的新档案");
    expect(screen.getByLabelText("AI 难度")).toHaveValue("contest");
  });

  it("normalizes an unspecified training side when switching to scoring", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /档案与模式/ }));
    fireEvent.change(screen.getByLabelText("参赛阵营"), {
      target: { value: "" },
    });
    fireEvent.change(screen.getByLabelText("模式"), {
      target: { value: "scoring" },
    });
    expect(screen.getByLabelText("参赛阵营")).toHaveValue("affirmative");
    fireEvent.click(screen.getByRole("button", { name: /队伍与辩位/ }));
    expect(
      screen.getByRole("button", { name: /正方一辩.*人工控制/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: /反方二辩.*AI 控制/ }),
    ).toBeDisabled();
  });

  it("refreshes authoritative preparation after a failed reanalysis instead of allowing an outdated launch", async () => {
    const initial = preparedSnapshot();
    const invalidated = {
      ...initial,
      phase: "rules_confirmed",
      preparationReady: false,
      lineupConfirmed: false,
      rolePackets: [],
      ownStrategy: null,
    };
    let reads = 0;
    invoke.mockImplementation((command) => {
      if (command === "analyze_topic")
        return Promise.reject(new Error("模型拆题失败"));
      return Promise.resolve(
        command === "workspace_snapshot" && reads++ > 0 ? invalidated : initial,
      );
    });
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /拆题与策略/ }));
    fireEvent.click(screen.getByRole("button", { name: "重新拆题并分配" }));
    await screen.findByText("模型拆题失败");
    await waitFor(() =>
      expect(
        invoke.mock.calls.filter(
          ([command]) => command === "workspace_snapshot",
        ),
      ).toHaveLength(2),
    );
    expect(screen.queryByText("已保存策略")).toBeNull();
    expect(screen.getByRole("button", { name: /开赛确认/ })).toBeDisabled();
  });

  it("locks writes until reload when a failed mutation cannot be synchronized", async () => {
    const initial = preparedSnapshot();
    let reads = 0;
    invoke.mockImplementation((command) => {
      if (command === "analyze_topic")
        return Promise.reject(new Error("模型拆题失败"));
      if (command === "workspace_snapshot" && reads++ > 0)
        return Promise.reject(new Error("现场读取失败"));
      return Promise.resolve(initial);
    });
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /拆题与策略/ }));
    fireEvent.click(screen.getByRole("button", { name: "重新拆题并分配" }));
    const retry = await screen.findByRole("button", {
      name: "重新加载本机会话",
    });
    await waitFor(() => expect(retry).toBeEnabled());
    expect(
      screen.getByRole("button", { name: "开始拆题并分配任务" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: /开赛确认/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "使用说明" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("shows team participation instead of single-speaker controls for cross examination and free debate", async () => {
    const initial = preparedSnapshot();
    const stage = (kind: "cross_examination" | "free_debate"): RuleStage => ({
      id: kind,
      displayName: kind === "free_debate" ? "团队自由辩" : "团队质询",
      semanticType: kind,
      participationMode: "whole_team",
      turnMode:
        kind === "free_debate"
          ? "alternating_teams"
          : "one_way_question_answer",
      clockMode: kind === "free_debate" ? "team_pool" : "questioner_pool",
      interruptPolicy:
        kind === "free_debate" ? "forbidden" : "questioner_may_stop_answer",
      fixedSpeakers: [],
      affirmativeCandidates: ["AFF_1", "AFF_2", "AFF_3", "AFF_4"],
      negativeCandidates: ["NEG_1", "NEG_2", "NEG_3", "NEG_4"],
      questioner: kind === "cross_examination" ? "AFF_3" : null,
      respondent: kind === "cross_examination" ? "NEG_2" : null,
      openingSide: "affirmative",
      durationSeconds: kind === "free_debate" ? null : 180,
      affirmativeSeconds: 240,
      negativeSeconds: 240,
      questionSeconds: null,
      answerSeconds: null,
      turnSeconds: null,
      followUpAllowed: false,
      respondentMayCounterQuestion: false,
      maxRounds: null,
      aiObjective: "围绕争点回应",
      scoringFocus: [],
      sourceEvidence: [],
      confidence: 100,
      unresolvedFields: [],
    });
    initial.ruleDraft = {
      ...ruleDraft,
      stages: [stage("cross_examination"), stage("free_debate")],
    };
    initial.schedule = [
      {
        id: "cross-event",
        kind: "cross_exam",
        speaker: "AFF_3",
        durationSeconds: 180,
        wordCap: 720,
        interruptible: true,
      },
      {
        id: "free-event",
        kind: "free_debate",
        speaker: "AFF_1",
        durationSeconds: 240,
        wordCap: 960,
        interruptible: false,
      },
    ];
    invoke.mockResolvedValue(initial);
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /阵容与赛程核对/ }));
    expect(screen.queryByRole("combobox", { name: "第1行辩位" })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "第2行辩位" })).toBeNull();
    expect(screen.getByText(/正方提问.*反方回答/)).toBeInTheDocument();
    expect(screen.getByText(/双方团队交替.*正方先发/)).toBeInTheDocument();
  });

  it("uses the same trimmed topic semantics as the backend instead of blocking launch for surrounding whitespace", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.change(screen.getByLabelText("辩题"), {
      target: { value: "  已保存辩题  " },
    });
    fireEvent.click(screen.getByRole("button", { name: /开赛确认/ }));
    expect(screen.getByRole("button", { name: "正式开始比赛" })).toBeEnabled();
  });

  it("does not start or submit calibration against an unsaved scoring mode or outdated preparation", async () => {
    invoke.mockResolvedValue(preparedSnapshot());
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /档案与模式/ }));
    fireEvent.change(screen.getByLabelText("模式"), {
      target: { value: "scoring" },
    });
    fireEvent.click(screen.getByRole("button", { name: /打字校准/ }));
    expect(screen.getByRole("button", { name: "开始计时" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "提交测速" })).toBeDisabled();
    expect(screen.getByLabelText("测速输入")).toBeDisabled();
  });

  it("hydrates profile switches and discards the previous profile's unsaved configuration", async () => {
    const initial = preparedSnapshot();
    initial.profiles.push({
      id: "other-setup-profile",
      displayName: "另一个档案",
    });
    const selected = {
      ...emptySnapshot,
      profiles: initial.profiles,
      activeProfileId: "other-setup-profile",
      userSide: "",
      topic: "另一档案的新辩题",
    };
    invoke.mockImplementation((command) =>
      Promise.resolve(command === "select_profile" ? selected : initial),
    );
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    fireEvent.change(screen.getByLabelText("闭卷材料"), {
      target: { value: "旧档案未保存材料" },
    });
    fireEvent.click(screen.getByRole("button", { name: /档案与模式/ }));
    fireEvent.click(screen.getAllByRole("button", { name: "选用" })[1]);
    await waitFor(() =>
      expect(screen.getByLabelText("AI 难度")).toHaveValue("standard"),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存并继续" }));
    expect(screen.getByLabelText("闭卷材料")).toHaveValue("");
    expect(screen.getByLabelText("辩题")).toHaveValue("另一档案的新辩题");
  });

  it("ignores an initial snapshot arriving after unmount and does not disturb a newer workspace", async () => {
    const oldRead = deferred<Snapshot>();
    invoke.mockReturnValueOnce(oldRead.promise);
    const first = renderWorkspace();
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    first.unmount();
    const current = preparedSnapshot();
    current.topic = "新页面的有效辩题";
    invoke.mockResolvedValue(current);
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    await act(async () =>
      oldRead.resolve(setupSnapshot({ topic: "过期结果" })),
    );
    expect(screen.getByText("新页面的有效辩题")).toBeInTheDocument();
    expect(screen.queryByText("过期结果")).toBeNull();
  });

  it("does not synchronize or display a failed command after its workspace has unmounted", async () => {
    const firstSnapshot = preparedSnapshot();
    const analysis = deferred<Snapshot>();
    invoke.mockImplementation((command) =>
      command === "analyze_topic"
        ? analysis.promise
        : Promise.resolve(firstSnapshot),
    );
    const first = renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    fireEvent.click(screen.getByRole("button", { name: /拆题与策略/ }));
    fireEvent.click(screen.getByRole("button", { name: "重新拆题并分配" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("analyze_topic", expect.any(Object)),
    );
    first.unmount();
    const current = preparedSnapshot();
    current.topic = "新的准备现场";
    invoke.mockResolvedValue(current);
    renderWorkspace();
    await screen.findByRole("heading", { name: "确认阵容并进入正式赛场" });
    await act(async () => analysis.reject(new Error("过期的 AI 错误")));
    expect(screen.queryByText("过期的 AI 错误")).toBeNull();
    expect(
      invoke.mock.calls.filter(([command]) => command === "workspace_snapshot"),
    ).toHaveLength(2);
    expect(screen.getByText("新的准备现场")).toBeInTheDocument();
  });
});
