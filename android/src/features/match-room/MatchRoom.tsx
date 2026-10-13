import {
  FormEvent,
  useEffect,
  useState,
  type ReactNode,
  type RefObject,
  type TextareaHTMLAttributes,
} from "react";
import { readableRoleText, speakerLabel } from "../../lib/presentation";
import { ConfirmDialog, Drawer } from "../../components/ui/Overlay";
import {
  ProgressBar,
  StatusBadge,
  TeamClock,
} from "../../components/ui/Primitives";
import { EventControlPanel } from "../arena/EventControlPanel";
import {
  floorSide,
  FreeDebatePanel,
  TeamWordPool,
} from "../arena/FreeDebatePanel";
import { useAiStream } from "../arena/useAiStream";
import { useAuthoritativeClock } from "../arena/useAuthoritativeClock";
import { TranscriptList } from "../transcript/TranscriptList";
import { MatchTimeline } from "../transcript/MatchTimeline";

export type Seat = {
  defaultController?: string;
  index: number;
  name: string;
  duty: string;
  side: string;
  controller: string;
  speaking: boolean;
  claimable: boolean;
};

export type MatchView = {
  phase: string;
  topic: string;
  mode: string;
  schedule: {
    id: string;
    kind: string;
    speaker: string;
    durationSeconds: number;
    wordCap: number;
    interruptible: boolean;
    controller?: string;
    started?: boolean;
  }[];
  cursor: number;
  meteringMode?: "words" | "time";
  remainingUnits?: number;
  currentWordCap?: number;
  affirmativeMainUnits?: number;
  negativeMainUnits?: number;
  affirmativeFreeUnits?: number;
  negativeFreeUnits?: number;
  remainingMs: number;
  affirmativeMs: number;
  negativeMs: number;
  affirmativeMainMs?: number;
  negativeMainMs?: number;
  ruleDraft?: object | null;
  floorHolder: string;
  transcripts: {
    id: string;
    slot: string;
    effectiveText: string;
    overflowText: string;
    interruptedTail: string;
    respondsTo?: string;
    responseTargetText?: string;
  }[];
  ownStrategy: string | null;
  opponentStrategy: string | null;
  materials: string;
  currentHuman: boolean;
  currentInterruptible: boolean;
  awaitingSeat: boolean;
  usesLiveModel: boolean;
  seats: Seat[];
  standard: boolean;
  currentSpeaker?: string;
};

const KIND: Record<string, string> = {
  opening: "陈词",
  cross_exam: "质询",
  free_debate: "自由辩",
  summary: "总结",
};

function clock(ms: number) {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function sideLabel(side: string) {
  return side === "affirmative" ? "正方" : "反方";
}

function SeatCard({
  seat,
  thinking,
  disabled = false,
  onClaim,
}: {
  seat: Seat;
  thinking: boolean;
  disabled?: boolean;
  onClaim: (index: number) => void;
}) {
  const available = seat.claimable;
  return (
    <button
      type="button"
      className={`seat ${seat.side} ${seat.speaking ? "speaking" : ""} ${
        seat.speaking && thinking ? "thinking-seat" : ""
      }`}
      aria-label={`${seat.name}，${seat.controller === "human" ? "真人" : "AI"}${seat.claimable ? "，可接管" : ""}`}
      disabled={!available || disabled}
      onClick={() => onClaim(seat.index)}
    >
      <span className="avatar" aria-hidden="true">
        {seat.name.slice(2, 3)}
      </span>
      <span className="seat-copy">
        <strong>{seat.name}</strong>
        <small>{seat.duty || "辩位"}</small>
        {seat.defaultController && (
          <small>
            长期：{seat.defaultController === "human" ? "人工" : "AI"} · 当前：
            {seat.controller === "human" ? "人工" : "AI"}
          </small>
        )}
      </span>
      <span className="seat-status">
        {seat.speaking
          ? "当前发言"
          : seat.claimable
            ? "可接管"
            : seat.controller === "human"
              ? "真人"
              : "AI"}
      </span>
    </button>
  );
}

type DrawerKind = "transcript" | "context" | "schedule" | "seats" | null;

export function MatchRoom({
  view,
  textareaProps,
  draftLength,
  onSubmit,
  onClaim,
  onAdvance,
  onCancel,
  onFinish,
  onTimeoutSubmit,
  onSetController,
  onSetCurrentController,
  thinking,
  aiCancellable = true,
  interactionLocked = false,
  windowStart,
  onMore,
  crossExamActive,
  stageWorkspace,
  onAbort,
  onWaiveFreeDebate,
  aiFailure,
  onRetryAi,
  onTakeoverCurrent,
  onDismissAiFailure,
}: {
  view: MatchView;
  textareaProps: TextareaHTMLAttributes<HTMLTextAreaElement> & {
    ref?: RefObject<HTMLTextAreaElement | null>;
  };
  draftLength: number;
  onSubmit: (event: FormEvent) => void;
  onClaim: (index: number) => void;
  onAdvance: () => void;
  onCancel: () => void;
  onFinish: () => void;
  onTimeoutSubmit: () => void;
  onSetController: (index: number, toHuman: boolean) => void;
  onSetCurrentController?: (index: number, toHuman: boolean) => void;
  thinking: string | null;
  aiCancellable?: boolean;
  interactionLocked?: boolean;
  windowStart: number;
  onMore: () => void;
  crossExamActive?: boolean;
  stageWorkspace?: ReactNode;
  onAbort: () => void;
  onWaiveFreeDebate?: () => void;
  aiFailure?: string;
  onRetryAi?: () => void;
  onTakeoverCurrent?: () => void;
  onDismissAiFailure?: () => void;
}) {
  const [drawer, setDrawer] = useState<DrawerKind>(null);
  const [confirmAbort, setConfirmAbort] = useState(false);
  const [confirmWaive, setConfirmWaive] = useState(false);
  const current = view.schedule[view.cursor];
  const words = view.meteringMode === "words";
  const remainingUnits = Math.max(0, view.remainingUnits ?? 0);
  const cap = words
    ? Math.min(view.currentWordCap ?? current?.wordCap ?? 0, remainingUnits)
    : (current?.wordCap ?? 0);
  const draftRemainingUnits = Math.max(
    0,
    remainingUnits - Math.min(draftLength, cap),
  );
  const stream = useAiStream(
    current?.id ?? "",
    thinking !== null && aiCancellable,
  );
  const waitingModel = thinking !== null;
  const [waitingSeconds, setWaitingSeconds] = useState(0);
  useEffect(() => {
    setWaitingSeconds(0);
    if (!waitingModel) return;
    const started = performance.now();
    const timer = window.setInterval(
      () => setWaitingSeconds(Math.floor((performance.now() - started) / 1000)),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [waitingModel]);
  const isFreeDebate = current?.kind === "free_debate";
  const shownMs = useAuthoritativeClock(view.remainingMs, {
    frozen: words || view.awaitingSeat || waitingModel,
    cursor: view.cursor,
    onTimeout: () => {
      if (!words && view.currentHuman && !waitingModel && !view.awaitingSeat) {
        onTimeoutSubmit();
      }
    },
  });

  const affirmative = view.seats.filter((seat) => seat.side === "affirmative");
  const negative = view.seats.filter((seat) => seat.side === "negative");
  const speaker = view.seats.find((seat) => seat.speaking);
  const thinkingText = thinking
    ? `${isFreeDebate ? "当前方 AI 团队" : (speaker?.name ?? "模型")} · ${stream.status || thinking} · 已等待 ${stream.requestId ? stream.elapsedSeconds : waitingSeconds} 秒（不扣字）`
    : "";
  const streamDraft = stream.draft;
  const upcoming = view.schedule.slice(view.cursor, view.cursor + 4);
  const lastTranscript = view.transcripts[view.transcripts.length - 1];
  const lastRespondsTo = lastTranscript?.effectiveText.slice(0, 70);
  const progress = view.schedule.length
    ? (Math.min(view.cursor, view.schedule.length) / view.schedule.length) * 100
    : 0;
  const holderSide = isFreeDebate
    ? floorSide(view.floorHolder)
    : floorSide(current?.speaker ?? view.currentSpeaker ?? "");
  const floorIsAffirmative = holderSide === "affirmative";
  const floorIsNegative = holderSide === "negative";
  const freeAffirmativeMs =
    isFreeDebate && floorIsAffirmative && !view.awaitingSeat
      ? Math.min(view.affirmativeMs, shownMs)
      : view.affirmativeMs;
  const freeNegativeMs =
    isFreeDebate && floorIsNegative && !view.awaitingSeat
      ? Math.min(view.negativeMs, shownMs)
      : view.negativeMs;
  const usesMainPool = view.ruleDraft != null;
  const projectMainClock = (
    side: "affirmative" | "negative",
    stored: number,
  ) => {
    if (isFreeDebate || holderSide !== side || !current) return stored;
    return usesMainPool
      ? Math.min(stored, shownMs)
      : Math.max(
          0,
          stored - Math.max(0, current.durationSeconds * 1000 - shownMs),
        );
  };

  return (
    <section className="room match-console" aria-label="赛场">
      <header className="match-command-bar">
        <div className="match-identity">
          <p className="eyebrow">正式赛场</p>
          <strong>{view.topic || "辩论"}</strong>
          <span>
            第 {Math.min(view.cursor + 1, view.schedule.length || 1)} /{" "}
            {view.schedule.length || 1} 环节 ·{" "}
            {KIND[current?.kind ?? ""] ?? "赛程"}
          </span>
        </div>
        <div className="match-team-clocks">
          {words ? (
            <TeamWordPool
              side="affirmative"
              label="正方主字池"
              units={view.affirmativeMainUnits ?? 0}
              active={!isFreeDebate && floorIsAffirmative}
            />
          ) : (
            <TeamClock
              side="affirmative"
              label="正方主时间"
              ms={projectMainClock(
                "affirmative",
                view.affirmativeMainMs ?? view.affirmativeMs,
              )}
              active={!isFreeDebate && floorIsAffirmative}
            />
          )}
          <div className="round-clock">
            <span>
              {words
                ? view.awaitingSeat
                  ? "点选辩位后发言"
                  : "当前字池剩余"
                : view.awaitingSeat
                  ? "点选辩位后开始计时"
                  : "本轮剩余"}
            </span>
            <strong>
              {words
                ? `${remainingUnits.toLocaleString("zh-CN")} 字`
                : view.awaitingSeat
                  ? "--:--"
                  : clock(shownMs)}
            </strong>
          </div>
          {words ? (
            <TeamWordPool
              side="negative"
              label="反方主字池"
              units={view.negativeMainUnits ?? 0}
              active={!isFreeDebate && floorIsNegative}
            />
          ) : (
            <TeamClock
              side="negative"
              label="反方主时间"
              ms={projectMainClock(
                "negative",
                view.negativeMainMs ?? view.negativeMs,
              )}
              active={!isFreeDebate && floorIsNegative}
            />
          )}
        </div>
        <div className="match-tools">
          <StatusBadge tone={view.usesLiveModel ? "success" : "neutral"}>
            {view.usesLiveModel ? "DeepSeek 在线" : "本地模拟"}
          </StatusBadge>
          <button
            type="button"
            aria-label="打开双方辩位"
            onClick={() => setDrawer("seats")}
          >
            辩位
          </button>
          <button
            type="button"
            aria-label="打开实录"
            onClick={() => setDrawer("transcript")}
          >
            实录
          </button>
          <button
            type="button"
            aria-label="打开策略与材料"
            onClick={() => setDrawer("context")}
          >
            策略
          </button>
          <button
            type="button"
            aria-label="打开赛程与控制"
            onClick={() => setDrawer("schedule")}
          >
            赛程
          </button>
          {view.phase === "in_progress" ||
          view.phase === "recovery_required" ? (
            <button
              type="button"
              className="danger-ghost"
              disabled={interactionLocked}
              onClick={() => setConfirmAbort(true)}
            >
              中止比赛
            </button>
          ) : null}
        </div>
        <ProgressBar value={progress} label="全场进度" />
      </header>

      <div className="match-arena-grid">
        <main className="stage-surface">
          <header className="current-stage-header">
            <div>
              <StatusBadge tone={floorIsNegative ? "negative" : "affirmative"}>
                {holderSide
                  ? `${sideLabel(floorIsAffirmative ? "affirmative" : "negative")}持权`
                  : "等待持权"}
              </StatusBadge>
              <h1>
                {current ? (KIND[current.kind] ?? current.kind) : "赛程完成"}
              </h1>
              <p>
                {current
                  ? words
                    ? `${speakerLabel(view.currentSpeaker || current.speaker)} · ${isFreeDebate ? "全队共享自由辩字池" : current.kind === "cross_exam" ? "使用质询方主字池" : "使用本方主字池"} · 本轮上限 ${cap} 字`
                    : isFreeDebate
                      ? `${speakerLabel(view.currentSpeaker || current.speaker)} · 全队共享自由辩时间 · 单轮不限时`
                      : usesMainPool
                        ? `${speakerLabel(view.currentSpeaker || current.speaker)} · 使用本方主时间 · 建议上限 ${cap}`
                        : `${speakerLabel(view.currentSpeaker || current.speaker)} · ${current.durationSeconds}s · 字数上限 ${cap}`
                  : "所有正式环节已经完成"}
              </p>
            </div>
            <div className="stage-rule-chips">
              <span>{view.currentHuman ? "真人发言" : "AI 发言"}</span>
              <span>
                {view.currentInterruptible ? "质询可打断" : "不可打断"}
              </span>
              <span>{view.standard ? "标准场次" : "非标准场次"}</span>
            </div>
          </header>

          <MatchTimeline items={view.transcripts} />

          {lastTranscript && !crossExamActive ? (
            <details className="response-target">
              <summary>
                上一条公开发言 · {speakerLabel(lastTranscript.slot)}
              </summary>
              <p>{lastTranscript.effectiveText}</p>
            </details>
          ) : null}

          {isFreeDebate ? (
            <FreeDebatePanel
              floorHolder={view.floorHolder}
              affirmativeMs={freeAffirmativeMs}
              negativeMs={freeNegativeMs}
              meteringMode={view.meteringMode}
              affirmativeUnits={view.affirmativeFreeUnits}
              negativeUnits={view.negativeFreeUnits}
              currentSpeaker={view.currentSpeaker ?? current?.speaker ?? ""}
              lastRespondsTo={lastRespondsTo}
              onWaiveRequest={() => setConfirmWaive(true)}
              waiveDisabled={thinking !== null}
            />
          ) : null}

          {stageWorkspace}

          {thinking ? (
            <div className="thinking stage-thinking" role="status">
              <span className="avatar" aria-hidden="true">
                {(speaker?.name ?? "模").slice(2, 3) || "模"}
              </span>
              <span>
                {thinkingText}
                <span className="thinking-dots" aria-hidden="true" />
              </span>
              {aiCancellable ? (
                <button type="button" onClick={onCancel}>
                  取消生成
                </button>
              ) : null}
            </div>
          ) : null}
          {streamDraft && aiCancellable && thinking ? (
            <div className="stream-draft" aria-live="polite">
              <small>AI 草稿 · 尚未通过字数校验，未提交</small>
              {streamDraft}
            </div>
          ) : null}

          {aiFailure ? (
            <aside className="ai-recovery" role="alert">
              <div>
                <strong>本轮 AI 没有完成</strong>
                <p>{aiFailure}</p>
                <small>
                  {words
                    ? "实录和字池均未回滚，模型等待不会扣字。"
                    : "实录和计时均未回滚，也未扣除模型等待时间。"}
                </small>
              </div>
              <div className="action-row">
                <button type="button" onClick={onRetryAi}>
                  重试本轮
                </button>
                <button type="button" onClick={onTakeoverCurrent}>
                  人工接管
                </button>
                <button type="button" onClick={onDismissAiFailure}>
                  保留现场
                </button>
              </div>
            </aside>
          ) : null}

          {!current ? (
            <div className="finish-stage">
              <h2>正式赛程已完成</h2>
              <p>确认后将生成个人评分与团队复盘报告。</p>
              <button
                type="button"
                className="primary-action"
                onClick={onFinish}
                disabled={thinking !== null}
              >
                生成评分报告
              </button>
            </div>
          ) : !crossExamActive ? (
            <form className="stage-composer" onSubmit={onSubmit}>
              {onSetCurrentController && speaker ? (
                <div
                  className="current-control"
                  role="group"
                  aria-label="本轮控制权"
                >
                  <span>本轮由你选择：</span>
                  <button
                    type="button"
                    aria-pressed={view.currentHuman}
                    disabled={
                      interactionLocked ||
                      !speaker.claimable ||
                      (view.currentHuman && !view.awaitingSeat)
                    }
                    onClick={() => onSetCurrentController(speaker.index, true)}
                  >
                    我来发言
                  </button>
                  <button
                    type="button"
                    aria-pressed={!view.currentHuman}
                    disabled={
                      interactionLocked ||
                      !speaker.claimable ||
                      !view.currentHuman
                    }
                    onClick={() => onSetCurrentController(speaker.index, false)}
                  >
                    交给 AI
                  </button>
                  <small>切换保留未提交草稿，不自动生成或推进赛程。</small>
                </div>
              ) : null}
              <label htmlFor="match-answer">本轮发言</label>
              <textarea
                id="match-answer"
                aria-label="本环节回答"
                placeholder={
                  view.currentHuman
                    ? "输入你的发言。提交后进入下一轮。"
                    : "当前由 AI 发言，你可以在合法辩位上接管。"
                }
                disabled={
                  !view.currentHuman || thinking !== null || view.awaitingSeat
                }
                {...textareaProps}
              />
              <div className="composer-meta">
                <span
                  className={
                    draftLength > cap && (words || cap > 0) ? "over-cap" : ""
                  }
                >
                  已输入 {draftLength} / {words ? cap : cap || "不限"}
                  {words ? " 字" : ""}
                </span>
                <span>
                  {view.awaitingSeat
                    ? "请先从当前方选择发言辩位"
                    : words
                      ? `草稿提交后预计剩余 ${draftRemainingUnits.toLocaleString("zh-CN")} 字`
                      : `本轮剩余 ${clock(shownMs)}`}
                </span>
              </div>
              {words ? (
                <p>仅正式公开文本扣字；超出上限的部分不计入比赛。</p>
              ) : null}
              <div className="composer-actions">
                {thinking === null ? (
                  <button type="button" onClick={onCancel} disabled>
                    取消生成
                  </button>
                ) : null}
                <button
                  type="button"
                  onClick={onAdvance}
                  disabled={view.currentHuman || thinking !== null}
                >
                  让模型完成本轮
                </button>
                <button
                  type="submit"
                  className="primary-action"
                  disabled={!view.currentHuman || thinking !== null}
                >
                  提交本轮发言
                </button>
              </div>
            </form>
          ) : null}
        </main>

        <aside className="arena-seats-panel" aria-label="双方辩席">
          {[affirmative, negative].map((team, index) => (
            <section
              key={index}
              className={`team-rail ${index === 0 ? "affirmative" : "negative"}-rail`}
              aria-label={`${index === 0 ? "正方" : "反方"}辩席`}
            >
              <div className="rail-heading">
                <span>{index === 0 ? "正方" : "反方"}</span>
                <small>四人团队</small>
              </div>
              {team.map((seat) => (
                <SeatCard
                  key={seat.index}
                  seat={seat}
                  thinking={thinking !== null}
                  disabled={interactionLocked}
                  onClaim={onClaim}
                />
              ))}
            </section>
          ))}
          <section className="arena-next" aria-label="接下来的环节">
            <h3>接下来</h3>
            <ol>
              {upcoming.map((event, index) => (
                <li key={event.id}>
                  {index === 0 ? "当前：" : ""}
                  {KIND[event.kind] ?? event.kind} ·{" "}
                  {speakerLabel(event.speaker)}
                </li>
              ))}
            </ol>
          </section>
        </aside>
      </div>

      <Drawer
        open={drawer === "seats"}
        title="双方辩位"
        onClose={() => setDrawer(null)}
      >
        <p>
          选择当前合法辩位可人工接管；切换不会自动生成、提交或进入下一环节。评分模式的队伍限制仍然生效。
        </p>
        {[affirmative, negative].map((team, index) => (
          <section
            key={index}
            className="seat-drawer-team"
            aria-label={`${index === 0 ? "正方" : "反方"}辩位`}
          >
            <h3>{index === 0 ? "正方" : "反方"} · 四个辩位</h3>
            <div className="seat-drawer-grid">
              {team.map((seat) => (
                <SeatCard
                  key={seat.index}
                  seat={seat}
                  thinking={thinking !== null}
                  disabled={interactionLocked}
                  onClaim={(seatIndex) => {
                    setDrawer(null);
                    onClaim(seatIndex);
                  }}
                />
              ))}
            </div>
          </section>
        ))}
      </Drawer>

      <Drawer
        open={drawer === "transcript"}
        title="比赛实录"
        onClose={() => setDrawer(null)}
      >
        <TranscriptList
          items={view.transcripts.map((entry) => ({
            id: entry.id,
            text: `${speakerLabel(entry.slot)}：${entry.effectiveText}${entry.interruptedTail ? "〔打断尾部不计入〕" : ""}${entry.overflowText ? "（超出部分不计入）" : ""}${entry.responseTargetText ? ` ←回答「${entry.responseTargetText}」` : entry.respondsTo ? ` ←回应 ${readableRoleText(entry.respondsTo)}` : ""}`,
          }))}
          start={windowStart}
          size={12}
        />
        {view.transcripts.length > 12 ? (
          <button type="button" onClick={onMore}>
            更早的发言
          </button>
        ) : null}
      </Drawer>

      <Drawer
        open={drawer === "context"}
        title="策略与材料"
        onClose={() => setDrawer(null)}
      >
        <section className="drawer-section">
          <h3>本方策略</h3>
          <p>
            {view.ownStrategy
              ? `本方策略：${view.ownStrategy}`
              : "暂无本方策略"}
          </p>
        </section>
        <section className="drawer-section">
          <h3>对方方向</h3>
          <p>
            {view.opponentStrategy
              ? `对方策略：${view.opponentStrategy}`
              : "暂无对方方向"}
          </p>
        </section>
        <section className="drawer-section">
          <h3>参考材料</h3>
          <p>{view.materials || "本场未提供材料"}</p>
        </section>
      </Drawer>

      <Drawer
        open={drawer === "schedule"}
        title="赛程与控制"
        onClose={() => setDrawer(null)}
      >
        <EventControlPanel
          schedule={view.schedule}
          cursor={view.cursor}
          mode={view.mode}
          meteringMode={view.meteringMode}
          currentWordCap={cap}
          disabled={thinking !== null}
          onSetController={onSetController}
        />
        <section className="drawer-section">
          <h3>接下来</h3>
          <ol>
            {upcoming.map((event, index) => (
              <li key={event.id}>
                {index === 0 ? "当前：" : ""}
                {KIND[event.kind] ?? event.kind} · {speakerLabel(event.speaker)}
              </li>
            ))}
          </ol>
        </section>
      </Drawer>

      <ConfirmDialog
        open={confirmWaive}
        title={`放弃当前方自由辩剩余${words ? "字数" : "时间"}`}
        description={
          words
            ? "当前持权方的自由辩字池会立即归零；若对方还有字数额度，对方仍可继续发言。"
            : "当前持权方的自由辩时间会立即归零；若对方还有时间，对方仍可继续发言。"
        }
        confirmLabel="确认放弃"
        onClose={() => setConfirmWaive(false)}
        onConfirm={() => {
          setConfirmWaive(false);
          onWaiveFreeDebate?.();
        }}
      />

      <ConfirmDialog
        open={confirmAbort}
        title="确认中止这场比赛"
        description="当前进度会保存为“已中止”，之后不能继续作答。"
        confirmLabel="确认中止"
        onClose={() => setConfirmAbort(false)}
        onConfirm={() => {
          setConfirmAbort(false);
          onAbort();
        }}
      />
    </section>
  );
}
