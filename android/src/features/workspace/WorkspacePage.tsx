import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate } from "react-router";
import { ConfirmDialog } from "../../components/ui/Overlay";
import { InlineNotice, StatusBadge } from "../../components/ui/Primitives";
import { call } from "../../lib/tauri";
import {
  emptySnapshot,
  routeForSnapshot,
  type Snapshot,
} from "../session/types";
import { SeatSelector, SETUP_STEPS, SetupStepper } from "./SetupWizard";
import { RulesEditor } from "./RulesEditor";
import { WorkspaceHelp } from "./WorkspaceHelp";
import { SchedulePreview } from "./SchedulePreview";
import { speakerLabel } from "../../lib/presentation";
import {
  draftFromSnapshot,
  humanSlots,
  preparationArguments,
  preparationChanged,
  readSetupDraft,
  rememberSetupDraft,
  type SetupDraft,
} from "./setupDraft";

const SPEAKERS: [string, string][] = [
  ["AFF_1", "正方一辩"],
  ["AFF_2", "正方二辩"],
  ["AFF_3", "正方三辩"],
  ["AFF_4", "正方四辩"],
  ["NEG_1", "反方一辩"],
  ["NEG_2", "反方二辩"],
  ["NEG_3", "反方三辩"],
  ["NEG_4", "反方四辩"],
];

const SLOT_NAMES = SPEAKERS.map(([, label]) => label);

function StepPanel({
  step,
  eyebrow,
  title,
  description,
  children,
}: {
  step: number;
  eyebrow: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className="setup-panel" aria-labelledby={`setup-step-${step}`}>
      <header className="setup-panel-header">
        <div className="step-kicker">{String(step + 1).padStart(2, "0")}</div>
        <div>
          <p className="eyebrow">{eyebrow}</p>
          <h2 id={`setup-step-${step}`}>{title}</h2>
          <p>{description}</p>
        </div>
      </header>
      <div className="setup-panel-body">{children}</div>
    </section>
  );
}

export function WorkspacePage() {
  const navigate = useNavigate();
  const [view, setView] = useState<Snapshot>(emptySnapshot);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<SetupDraft>(() =>
    draftFromSnapshot(emptySnapshot),
  );
  const {
    name,
    topic,
    materials,
    rulesText,
    mode,
    side,
    difficulty,
    selectedSlots,
    activeStep,
  } = draft;
  const [calibrationText, setCalibrationText] = useState("");
  const [calibrationStarted, setCalibrationStarted] = useState(0);
  const [calibrationPasted, setCalibrationPasted] = useState(false);
  const [desktop, setDesktop] = useState<boolean | null>(null);
  const [thinking, setThinking] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const alive = useRef(false);
  const operation = useRef(0);
  const pending = useRef(false);
  const current = useRef({ view, draft, loaded });
  current.current = { view, draft, loaded };
  const [deleteProfile, setDeleteProfile] = useState<{
    id: string;
    name: string;
  } | null>(null);

  function edit<K extends keyof SetupDraft>(key: K, value: SetupDraft[K]) {
    if (pending.current) return;
    setDraft((currentDraft) => ({ ...currentDraft, [key]: value }));
  }
  const setName = (value: string) => edit("name", value);
  const setTopic = (value: string) => edit("topic", value);
  const setMaterials = (value: string) => edit("materials", value);
  const setRulesText = (value: string) => edit("rulesText", value);
  const setMode = (value: string) => {
    if (pending.current) return;
    setDraft((currentDraft) => ({
      ...currentDraft,
      mode: value,
      side:
        value === "scoring" && !currentDraft.side
          ? "affirmative"
          : currentDraft.side,
    }));
  };
  const setSide = (value: string) => edit("side", value);
  const setDifficulty = (value: string) => edit("difficulty", value);
  const setSelectedSlots = (value: number[]) => edit("selectedSlots", value);
  function setActiveStep(value: number | ((step: number) => number)) {
    if (pending.current) return;
    setDraft((currentDraft) => ({
      ...currentDraft,
      activeStep:
        typeof value === "number" ? value : value(currentDraft.activeStep),
    }));
  }

  useEffect(() => {
    alive.current = true;
    const requestRevision = operation;
    const ticket = ++operation.current;
    pending.current = true;
    setBusy(true);
    void call<Snapshot>("workspace_snapshot")
      .then((next) => {
        if (!alive.current || operation.current !== ticket) return;
        setView(next);
        setDraft(readSetupDraft(next));
        setDesktop(true);
        setLoadFailed(false);
        setLoaded(true);
      })
      .catch((caught) => {
        if (!alive.current || operation.current !== ticket) return;
        const isDesktop = "__TAURI_INTERNALS__" in window;
        setDesktop(isDesktop);
        setLoadFailed(isDesktop);
        setError(
          isDesktop
            ? caught instanceof Error
              ? caught.message
              : String(caught)
            : "",
        );
        setLoaded(true);
      })
      .finally(() => {
        if (!alive.current || operation.current !== ticket) return;
        pending.current = false;
        setBusy(false);
      });
    return () => {
      alive.current = false;
      ++requestRevision.current;
      const state = current.current;
      if (state.loaded) rememberSetupDraft(state.view, state.draft);
    };
  }, []);

  useEffect(() => {
    if (loaded && desktop && !loadFailed) rememberSetupDraft(view, draft);
  }, [view, draft, loaded, desktop, loadFailed]);

  useEffect(() => {
    if (!loaded) return;
    const target = routeForSnapshot(view);
    if (target !== "/workspace") navigate(target, { replace: true });
  }, [loaded, navigate, view]);

  useEffect(() => {
    if (mode !== "scoring") return;
    const allowed = side === "negative" ? [4, 5, 6, 7] : [0, 1, 2, 3];
    setDraft((currentDraft) => {
      const kept = currentDraft.selectedSlots
        .filter((index) => allowed.includes(index))
        .slice(0, 4);
      const selected = kept.length > 0 ? kept : [allowed[0]];
      return selected.join(",") === currentDraft.selectedSlots.join(",")
        ? currentDraft
        : { ...currentDraft, selectedSlots: selected };
    });
  }, [mode, side]);

  function adoptSnapshot(next: Snapshot, command: string, failed = false) {
    const previous = current.current;
    const changedScope =
      next.matchId !== previous.view.matchId ||
      next.activeProfileId !== previous.view.activeProfileId;
    const hydrate =
      [
        "resume_match",
        "select_profile",
        "delete_profile",
        "create_profile",
      ].includes(command) &&
      (!failed || changedScope);
    setView(next);
    setDraft((currentDraft) => {
      if (command === "workspace_snapshot" && !failed)
        return readSetupDraft(next);
      if (hydrate) return draftFromSnapshot(next);
      return {
        ...currentDraft,
        topic:
          next.topic.trim() === currentDraft.topic.trim()
            ? next.topic
            : currentDraft.topic,
        rulesText: ["open_template", "recognize_rules"].includes(command)
          ? (next.ruleDraft?.sourceText ?? currentDraft.rulesText)
          : currentDraft.rulesText,
        ruleBuffer:
          JSON.stringify(next.ruleDraft) !==
          JSON.stringify(previous.view.ruleDraft)
            ? (next.ruleDraft ?? null)
            : currentDraft.ruleBuffer,
      };
    });
  }

  async function run(command: string, args?: Record<string, unknown>) {
    if (pending.current || !alive.current) return undefined;
    pending.current = true;
    const ticket = ++operation.current;
    setBusy(true);
    setError("");
    const waiting =
      command === "analyze_topic"
        ? "正在拆题"
        : command === "recognize_rules"
          ? "正在识别赛制"
          : command === "workspace_snapshot"
            ? "正在重新加载本机会话"
            : "正在保存操作";
    setThinking(waiting);
    try {
      const next = await call<Snapshot>(command, args);
      if (!alive.current || operation.current !== ticket) return undefined;
      adoptSnapshot(next, command);
      if (command === "workspace_snapshot") {
        setDesktop(true);
        setLoadFailed(false);
      }
      return next;
    } catch (caught) {
      if (!alive.current || operation.current !== ticket) return undefined;
      const failure = caught instanceof Error ? caught.message : String(caught);
      if (command === "workspace_snapshot") {
        setLoadFailed(true);
        setError(failure);
        return undefined;
      }
      // A command may already have changed memory before persistence or AI failed.
      // Read authority without releasing the single-flight gate; never imply a
      // successful read made the preceding failed write durable.
      try {
        const synchronized = await call<Snapshot>("workspace_snapshot");
        if (!alive.current || operation.current !== ticket) return undefined;
        adoptSnapshot(synchronized, command, true);
        setLoadFailed(false);
        setError(failure);
      } catch (syncError) {
        if (!alive.current || operation.current !== ticket) return undefined;
        setLoadFailed(true);
        setError(
          `${failure}；无法确认本机会话：${syncError instanceof Error ? syncError.message : String(syncError)}。重新加载后再操作。`,
        );
      }
      return undefined;
    } finally {
      if (alive.current && operation.current === ticket) {
        pending.current = false;
        setBusy(false);
        setThinking(null);
      }
    }
  }

  async function resume(id: string) {
    const next = await run("resume_match", { id });
    if (next) navigate(routeForSnapshot(next));
  }

  async function submitCalibration() {
    if (!calibrationAllowed) return;
    const next = await run("submit_calibration", {
      typed: calibrationText,
      elapsedMs: calibrationStarted ? Date.now() - calibrationStarted : 0,
      pasted: calibrationPasted,
    });
    if (!next) return;
    setCalibrationText("");
    setCalibrationStarted(0);
    setCalibrationPasted(false);
  }

  const connection =
    desktop === null
      ? { tone: "neutral" as const, label: "正在连接本机会话" }
      : loadFailed
        ? { tone: "danger" as const, label: "本机会话加载失败" }
        : desktop && view.modelSettings.connectionStatus === "ready"
          ? { tone: "success" as const, label: "DeepSeek 已验证" }
          : desktop && view.modelSettings.hasApiKey
            ? { tone: "warning" as const, label: "密钥尚未验证" }
            : desktop
              ? { tone: "neutral" as const, label: "本地模拟预览" }
              : { tone: "warning" as const, label: "浏览器预览" };

  const hasRuleIssues = Boolean(
    view.rulesError ||
    view.ruleIssues?.some((issue) => issue.severity === "blocking"),
  );
  const preparationDirty = preparationChanged(view, draft);
  const rulesDirty =
    JSON.stringify(draft.ruleBuffer) !== JSON.stringify(view.ruleDraft ?? null);
  const ruleSourceDirty = Boolean(
    view.ruleDraft && rulesText !== view.ruleDraft.sourceText,
  );
  const preparationReady =
    view.preparationReady &&
    !loadFailed &&
    !preparationDirty &&
    !rulesDirty &&
    !ruleSourceDirty;
  const lineupReady =
    preparationReady &&
    view.lineupConfirmed &&
    selectedSlots.join(",") === humanSlots(view).join(",");
  const calibrationAllowed =
    mode === "scoring" && view.mode === "scoring" && preparationReady;
  useEffect(() => {
    if (calibrationAllowed) return;
    setCalibrationText("");
    setCalibrationStarted(0);
    setCalibrationPasted(false);
  }, [calibrationAllowed]);
  const canMoveNext =
    activeStep === 0
      ? Boolean(view.activeProfileId)
      : activeStep === 1
        ? topic.trim().length > 0
        : activeStep === 2
          ? false
          : activeStep === 3
            ? view.rulesConfirmed && !rulesDirty
            : activeStep === 4
              ? preparationReady
              : activeStep === 5
                ? mode === "training" || selectedSlots.length > 0
                : activeStep === 6
                  ? mode !== "scoring" ||
                    (calibrationAllowed && view.baselineCpm > 0)
                  : activeStep === 7
                    ? preparationReady
                    : false;

  async function nextStep() {
    if (!canMoveNext) return;
    if (preparationDirty) {
      const next = await run("update_preparation", preparationArguments(draft));
      if (!next) return;
    }
    setActiveStep((current) => Math.min(8, current + 1));
  }

  const selectedNames = selectedSlots.map((index) => SLOT_NAMES[index]);

  return (
    <main className="workspace pre-match-page">
      <header className="workspace-header">
        <div>
          <p className="eyebrow">赛前工作台</p>
          <h1>构建一场完整辩论</h1>
          <p>一次完成一个任务。正式开赛后将进入独立赛事控制台。</p>
        </div>
        <div className="workspace-header-actions">
          <StatusBadge tone={connection.tone}>{connection.label}</StatusBadge>
          <nav aria-label="工作台导航">
            <button type="button" onClick={() => setHelpOpen(true)}>
              使用说明
            </button>
            <Link
              to="/settings"
              aria-disabled={busy}
              onClick={(event) => {
                if (pending.current) event.preventDefault();
              }}
            >
              模型设置
            </Link>
          </nav>
        </div>
      </header>

      {error ? (
        <InlineNotice tone="danger" title="操作未完成">
          <p>{error}</p>
          {loadFailed ? (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run("workspace_snapshot")}
            >
              重新加载本机会话
            </button>
          ) : null}
        </InlineNotice>
      ) : null}
      {thinking ? (
        <p className="thinking workspace-thinking" role="status">
          {thinking}
          <span className="thinking-dots" aria-hidden="true" />
        </p>
      ) : null}
      {view.message ? (
        <InlineNotice tone="info">
          <p>{view.message}</p>
        </InlineNotice>
      ) : null}
      {preparationDirty ? (
        <InlineNotice tone="warning" title="比赛配置已修改">
          <p>请保存配置并重新拆题；旧策略、辩手任务和阵容确认不能用于开赛。</p>
          <button
            type="button"
            disabled={busy || loadFailed || desktop !== true || !topic.trim()}
            onClick={() =>
              void run("update_preparation", preparationArguments(draft))
            }
          >
            保存比赛配置
          </button>
        </InlineNotice>
      ) : rulesDirty || ruleSourceDirty ? (
        <InlineNotice tone="warning" title="赛制有未保存修改">
          <p>
            {ruleSourceDirty
              ? "赛制原文已修改，请重新识别并确认规则。"
              : "请先保存赛程修改，再确认规则并重新拆题。"}
          </p>
        </InlineNotice>
      ) : null}

      <fieldset
        className="workspace-layout"
        aria-label="赛前配置"
        style={{ border: 0, padding: 0, minWidth: 0 }}
        disabled={busy || !loaded || desktop !== true || loadFailed}
      >
        <SetupStepper
          phase={view.phase}
          activeStep={activeStep}
          mode={mode}
          hasRuleIssues={hasRuleIssues}
          onStepChange={setActiveStep}
        />

        <div className="workspace-stage">
          {activeStep === 0 ? (
            <StepPanel
              step={0}
              eyebrow="基础设置"
              title="选择档案与训练模式"
              description="档案保存你的测速基线和历史比赛；模式决定本场接管与评分规则。"
            >
              <div className="form-grid">
                <label>
                  模式
                  <select
                    value={mode}
                    onChange={(event) => setMode(event.target.value)}
                  >
                    <option value="training">训练模式</option>
                    <option value="scoring">评分模式</option>
                  </select>
                </label>
                <label>
                  参赛阵营
                  <select
                    value={side}
                    onChange={(event) => setSide(event.target.value)}
                  >
                    <option value="affirmative">正方</option>
                    <option value="negative">反方</option>
                    {mode === "training" ? (
                      <option value="">不指定</option>
                    ) : null}
                  </select>
                </label>
                <label>
                  AI 难度
                  <select
                    value={difficulty}
                    onChange={(event) => setDifficulty(event.target.value)}
                  >
                    <option value="beginner">入门</option>
                    <option value="standard">标准</option>
                    <option value="contest">竞赛级</option>
                  </select>
                </label>
              </div>
              <div className="profile-workspace">
                <form
                  className="profile-create"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void run("create_profile", { name });
                  }}
                >
                  <label>
                    新档案名称
                    <input
                      value={name}
                      onChange={(event) => setName(event.target.value)}
                    />
                  </label>
                  <button type="submit">建立档案</button>
                </form>
                <ul className="profile-list">
                  {view.profiles.map((profile) => {
                    const current = profile.id === view.activeProfileId;
                    return (
                      <li
                        key={profile.id}
                        className={current ? "selected-row" : ""}
                      >
                        <span>
                          <strong>{profile.displayName}</strong>
                          <small>{current ? "当前档案" : "可切换"}</small>
                        </span>
                        <div>
                          <button
                            type="button"
                            disabled={current}
                            onClick={() =>
                              void run("select_profile", { id: profile.id })
                            }
                          >
                            选用
                          </button>
                          <button
                            type="button"
                            onClick={() =>
                              void run("rename_profile", {
                                id: profile.id,
                                name,
                              })
                            }
                          >
                            改名
                          </button>
                          <button
                            type="button"
                            className="danger-ghost"
                            onClick={() =>
                              setDeleteProfile({
                                id: profile.id,
                                name: profile.displayName,
                              })
                            }
                          >
                            删除
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </div>
            </StepPanel>
          ) : null}

          {activeStep === 1 ? (
            <StepPanel
              step={1}
              eyebrow="比赛内容"
              title="输入辩题与参考材料"
              description="辩题决定双方立场，材料只放背景、数据和证据，不要混入赛制。"
            >
              <label>
                辩题
                <input
                  value={topic}
                  onChange={(event) => setTopic(event.target.value)}
                />
              </label>
              <label>
                参考材料
                <textarea
                  className="large-textarea"
                  value={materials}
                  aria-label="闭卷材料"
                  placeholder="可选：粘贴背景、数据、案例与证据来源"
                  onChange={(event) => setMaterials(event.target.value)}
                />
              </label>
            </StepPanel>
          ) : null}

          {activeStep === 2 ? (
            <StepPanel
              step={2}
              eyebrow="赛制来源"
              title="选择预设或识别你的赛制"
              description="内置预设可以继续编辑；识别失败时也会保留原文并生成可修正草稿。"
            >
              <div className="source-choice-grid">
                <article className="source-choice recommended">
                  <StatusBadge tone="success">推荐起点</StatusBadge>
                  <h3>传统四对四攻辩制</h3>
                  <p>
                    包含立论、攻辩、攻辩小结、团队自由辩和总结，可逐环节调整。
                  </p>
                  <button
                    type="button"
                    className="primary-action"
                    onClick={() =>
                      void run("open_template", {
                        mode,
                        topic,
                        materials,
                        userSide: side,
                        humanSlots: selectedSlots,
                        difficulty,
                      }).then((next) => next && setActiveStep(3))
                    }
                  >
                    使用并编辑预设
                  </button>
                </article>
                <article className="source-choice">
                  <StatusBadge>AI 识别</StatusBadge>
                  <h3>粘贴比赛赛制</h3>
                  <label>
                    赛制原文
                    <textarea
                      value={rulesText}
                      aria-label="赛制说明"
                      placeholder="写明环节、辩位、方向和时长"
                      onChange={(event) => setRulesText(event.target.value)}
                    />
                  </label>
                  <button
                    type="button"
                    disabled={thinking !== null || !rulesText.trim()}
                    onClick={() =>
                      void run("recognize_rules", {
                        rulesText,
                        mode,
                        topic,
                        materials,
                        userSide: side,
                        difficulty,
                      }).then((next) => next && setActiveStep(3))
                    }
                  >
                    识别并生成草稿
                  </button>
                </article>
              </div>
            </StepPanel>
          ) : null}

          {activeStep === 3 ? (
            <StepPanel
              step={3}
              eyebrow="赛制确认"
              title="逐项核对环节语义"
              description="修改后先保存，再确认整套规则。阻断问题必须解决后才能继续。"
            >
              {view.rulesError ? (
                <InlineNotice tone="danger" title="规则尚不能确认">
                  <p>{view.rulesError}</p>
                </InlineNotice>
              ) : null}
              {view.ruleDraft ? (
                <RulesEditor
                  draft={view.ruleDraft}
                  editedDraft={draft.ruleBuffer ?? view.ruleDraft}
                  onDraftChange={(ruleBuffer) => edit("ruleBuffer", ruleBuffer)}
                  meteringMode={view.meteringMode ?? "words"}
                  speechRatePerMinute={view.speechRatePerMinute ?? 240}
                  issues={view.ruleIssues ?? []}
                  disabled={thinking !== null || view.phase === "in_progress"}
                  onReplace={(ruleset) =>
                    void run("replace_semantic_rules", { ruleset })
                  }
                />
              ) : (
                <InlineNotice tone="warning" title="还没有赛制草稿">
                  <p>请返回上一步选择预设或识别赛制。</p>
                </InlineNotice>
              )}
              <div className="panel-primary-action">
                <button
                  className="primary-action"
                  type="button"
                  disabled={
                    thinking !== null ||
                    rulesDirty ||
                    ruleSourceDirty ||
                    (view.ruleDraft
                      ? view.ruleDraft.stages.length === 0 ||
                        view.rulesCanConfirm === false
                      : view.schedule.length === 0)
                  }
                  onClick={() => void run("confirm_rules")}
                >
                  {view.rulesConfirmed ? "规则已确认" : "确认整套规则"}
                </button>
              </div>
            </StepPanel>
          ) : null}

          {activeStep === 4 ? (
            <StepPanel
              step={4}
              eyebrow="AI 读题"
              title="拆解辩题并分配任务"
              description="模型将生成双方方向和八位辩手的职责包，生成后仍需你核对。"
            >
              <div className="analysis-brief">
                <div>
                  <span>辩题</span>
                  <strong>{topic}</strong>
                </div>
                <div>
                  <span>模式</span>
                  <strong>{mode === "scoring" ? "评分" : "训练"}</strong>
                </div>
                <div>
                  <span>难度</span>
                  <strong>{difficulty}</strong>
                </div>
              </div>
              <button
                type="button"
                className="primary-action"
                disabled={
                  !view.rulesConfirmed ||
                  rulesDirty ||
                  ruleSourceDirty ||
                  thinking !== null
                }
                onClick={() =>
                  void run("analyze_topic", {
                    mode,
                    topic,
                    materials,
                    userSide: side,
                    difficulty,
                  })
                }
              >
                {preparationReady ? "重新拆题并分配" : "开始拆题并分配任务"}
              </button>
              {preparationReady && view.ownStrategy ? (
                <InlineNotice tone="success" title="本方策略">
                  <p>{view.ownStrategy}</p>
                </InlineNotice>
              ) : null}
              {preparationReady && view.opponentStrategy ? (
                <InlineNotice tone="info" title="对方方向">
                  <p>{view.opponentStrategy}</p>
                </InlineNotice>
              ) : null}
            </StepPanel>
          ) : null}

          {activeStep === 5 ? (
            <StepPanel
              step={5}
              eyebrow="控制分配"
              title="选择你要扮演的辩位"
              description="训练模式可以控制任意一个或多个辩位，也可以全部交给 AI。"
            >
              <SeatSelector
                mode={mode}
                side={side}
                selected={selectedSlots}
                onChange={setSelectedSlots}
              />
            </StepPanel>
          ) : null}

          {activeStep === 6 ? (
            <StepPanel
              step={6}
              eyebrow="输入基线"
              title={
                mode === "scoring" ? "完成赛前打字测速" : "训练模式无需测速"
              }
              description={
                mode === "scoring"
                  ? "测速只用于校正编辑与思考成本，不直接计入分数。"
                  : "本场不会使用输入速度校正，你可以直接继续。"
              }
            >
              {mode === "scoring" ? (
                <div className="calibration-card">
                  {!calibrationAllowed ? (
                    <InlineNotice tone="warning" title="请先完成本场准备">
                      <p>
                        保存比赛配置并重新拆题后，再开始本场测速，避免使用过期配置建立基线。
                      </p>
                    </InlineNotice>
                  ) : null}
                  <p className="calibration-passage">
                    {view.calibrationPassage}
                  </p>
                  <textarea
                    value={calibrationText}
                    disabled={!calibrationAllowed}
                    aria-label="测速输入"
                    placeholder="点击开始计时后，在这里连续输入上方文字"
                    onChange={(event) => setCalibrationText(event.target.value)}
                    onPaste={(event) => {
                      event.preventDefault();
                      setCalibrationPasted(true);
                    }}
                  />
                  <div className="action-row">
                    <button
                      type="button"
                      disabled={!calibrationAllowed}
                      onClick={() => setCalibrationStarted(Date.now())}
                    >
                      {calibrationStarted ? "重新开始计时" : "开始计时"}
                    </button>
                    <button
                      type="button"
                      className="primary-action"
                      disabled={
                        !calibrationAllowed ||
                        !calibrationStarted ||
                        !calibrationText
                      }
                      onClick={() => void submitCalibration()}
                    >
                      提交测速
                    </button>
                  </div>
                  <p>
                    已完成 {Math.min(view.calibrationRoundsCompleted, 2)}/2 轮；
                    {view.baselineCpm > 0
                      ? `当前基线：${view.baselineCpm} 字/分钟`
                      : "尚未形成有效基线"}
                    {view.lowConfidence ? "（低置信）" : ""}
                  </p>
                </div>
              ) : (
                <InlineNotice tone="info" title="已自动跳过">
                  <p>训练模式允许随时接管辩位，不需要先建立评分输入基线。</p>
                </InlineNotice>
              )}
            </StepPanel>
          ) : null}

          {activeStep === 7 ? (
            <StepPanel
              step={7}
              eyebrow="最终核对"
              title="检查赛程顺序与八位辩手任务"
              description="这是正式开赛前最后一次核对；开赛后不能返回修改。"
            >
              <SchedulePreview
                view={view}
                busy={busy}
                onUpdate={(index, speaker, durationSeconds) =>
                  void run("update_schedule", {
                    index,
                    speaker,
                    durationSeconds,
                  })
                }
              />
              <div className="role-packets">
                <h3>八位辩手任务</h3>
                <ul>
                  {(preparationReady ? view.rolePackets : []).map((packet) => (
                    <li key={packet.slot}>
                      <strong>{speakerLabel(packet.slot)}</strong>
                      <span>{packet.duty}</span>
                      <p>{packet.mainArgument}</p>
                    </li>
                  ))}
                </ul>
              </div>
            </StepPanel>
          ) : null}

          {activeStep === 8 ? (
            <StepPanel
              step={8}
              eyebrow="准备就绪"
              title="确认阵容并进入正式赛场"
              description="正式开始前系统会冻结当前赛制、任务和辩位控制安排。"
            >
              <div className="launch-summary">
                <div>
                  <span>辩题</span>
                  <strong>{topic}</strong>
                </div>
                <div>
                  <span>模式</span>
                  <strong>
                    {mode === "scoring" ? "评分模式" : "训练模式"}
                  </strong>
                </div>
                <div>
                  <span>你的阵营</span>
                  <strong>
                    {side === "negative"
                      ? "反方"
                      : side === "affirmative"
                        ? "正方"
                        : "不指定"}
                  </strong>
                </div>
                <div>
                  <span>人工辩位</span>
                  <strong>{selectedNames.join("、") || "全 AI 模拟"}</strong>
                </div>
              </div>
              <div className="launch-actions">
                <button
                  type="button"
                  disabled={
                    !preparationReady ||
                    (mode === "scoring" && view.baselineCpm <= 0)
                  }
                  onClick={() =>
                    void run("ready_match", {
                      userSide: side,
                      humanSlots: selectedSlots,
                    })
                  }
                >
                  {lineupReady ? "阵容已确认" : "确认阵容"}
                </button>
                <button
                  className="primary-action launch-button"
                  type="button"
                  disabled={!lineupReady}
                  onClick={() => void run("start_match")}
                >
                  正式开始比赛
                </button>
              </div>
            </StepPanel>
          ) : null}

          <footer className="workspace-step-actions">
            <button
              type="button"
              disabled={activeStep === 0}
              onClick={() => setActiveStep((step) => Math.max(0, step - 1))}
            >
              上一步
            </button>
            <span>
              第 {activeStep + 1} 步，共 {SETUP_STEPS.length} 步
            </span>
            {activeStep < 8 ? (
              <button
                type="button"
                className="primary-action"
                disabled={!canMoveNext}
                onClick={() => void nextStep()}
              >
                保存并继续
              </button>
            ) : (
              <span />
            )}
          </footer>
        </div>

        <aside className="setup-summary" aria-label="当前准备状态">
          <p className="eyebrow">当前准备状态</p>
          <h2>{SETUP_STEPS[activeStep]}</h2>
          <dl>
            <div>
              <dt>档案</dt>
              <dd>
                {view.profiles.find(
                  (profile) => profile.id === view.activeProfileId,
                )?.displayName ?? "未选择"}
              </dd>
            </div>
            <div>
              <dt>模式</dt>
              <dd>{mode === "scoring" ? "评分" : "训练"}</dd>
            </div>
            <div>
              <dt>阵营</dt>
              <dd>
                {side === "negative"
                  ? "反方"
                  : side === "affirmative"
                    ? "正方"
                    : "不指定"}
              </dd>
            </div>
            <div>
              <dt>规则</dt>
              <dd>
                {view.rulesConfirmed
                  ? "已确认"
                  : view.ruleDraft
                    ? "编辑中"
                    : "未建立"}
              </dd>
            </div>
            <div>
              <dt>真人辩位</dt>
              <dd>
                {selectedNames.length ? `${selectedNames.length} 人` : "全 AI"}
              </dd>
            </div>
            <div>
              <dt>模型</dt>
              <dd>{connection.label}</dd>
            </div>
          </dl>
          {hasRuleIssues ? (
            <InlineNotice tone="danger">
              <p>赛制还有阻断问题，请返回第 4 步修正。</p>
            </InlineNotice>
          ) : null}
          <div className="history-compact">
            <h3>最近比赛</h3>
            {view.recent.length === 0 ? (
              <p className="muted">暂无历史比赛</p>
            ) : null}
            <ul>
              {view.recent.slice(0, 4).map((item) => (
                <li key={item.id}>
                  <span>{item.topic}</span>
                  {item.phase === "aborted" ? (
                    <small>已中止</small>
                  ) : (
                    <button type="button" onClick={() => void resume(item.id)}>
                      {item.phase === "report_ready" ? "报告" : "恢复"}
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </div>
        </aside>
      </fieldset>

      <WorkspaceHelp open={helpOpen} onClose={() => setHelpOpen(false)} />

      <ConfirmDialog
        open={deleteProfile !== null}
        title={`删除档案「${deleteProfile?.name ?? ""}」`}
        description="档案下的比赛也会一并删除，这项操作无法撤销。"
        confirmLabel="确认删除"
        onClose={() => setDeleteProfile(null)}
        onConfirm={() => {
          if (deleteProfile)
            void run("delete_profile", {
              id: deleteProfile.id,
              confirmed: true,
            });
          setDeleteProfile(null);
        }}
      />
    </main>
  );
}
