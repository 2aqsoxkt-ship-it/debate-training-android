import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type RefObject,
  type TextareaHTMLAttributes,
} from "react";
import { StatusBadge } from "../../components/ui/Primitives";
import { countEffectiveUnits } from "../telemetry/units";
import { PreparedAnswerReveal } from "./PreparedAnswerReveal";
import {
  readableRoleText,
  speakerLabel as slotLabel,
} from "../../lib/presentation";

export type CrossExamUiPhase =
  "question" | "preparing" | "revealing" | "done" | "completed";

export type CrossExamSnapshot = {
  eventId: string;
  phase: CrossExamUiPhase;
  questioner: string;
  respondent: string;
  questionerHuman: boolean;
  respondentHuman: boolean;
  question: string;
  visiblePrefix: string;
  interruptible: boolean;
  answerPrepared: boolean;
  revealedUnits: number;
  totalUnits: number;
  roundNumber: number;
  nextSide: "affirmative" | "negative";
  totalMs: number;
  usedMs: number;
  remainingMs: number;
  meteringMode?: "words" | "time";
  remainingUnits?: number;
  questionWordCap?: number;
  answerWordCap?: number;
  selectionSummary: string;
  rolesSelected: boolean;
  eligibleQuestioners: string[];
  history: {
    roundNumber: number;
    questioner: string;
    respondent: string;
    question: string;
    answer: string;
    status: string;
  }[];
};

const slotIndex = (slot: string) => {
  const match = /^(AFF|NEG)_([1-4])$/.exec(slot);
  if (!match) return -1;
  return (match[1] === "NEG" ? 4 : 0) + Number(match[2]) - 1;
};

export function CrossExamPanel({
  snapshot,
  questionTextareaProps,
  answerTextareaProps,
  answerLength,
  wordCap,
  speechRatePerMinute = 240,
  busy,
  aiCancellable = true,
  humanWritePending = false,
  synchronizationBlocked = false,
  onSynchronize,
  onSubmitQuestion,
  onGenerateQuestion,
  onInsertQuestioner,
  onSetQuestionerHuman,
  onPrepareHumanAnswer,
  onGenerateAnswer,
  onSetRespondentHuman,
  onReveal,
  onComplete,
  onInterrupt,
  onTimeout,
  onEndSession,
  onCancel,
}: {
  snapshot: CrossExamSnapshot;
  questionTextareaProps: TextareaHTMLAttributes<HTMLTextAreaElement> & {
    ref?: RefObject<HTMLTextAreaElement | null>;
  };
  answerTextareaProps: TextareaHTMLAttributes<HTMLTextAreaElement> & {
    ref?: RefObject<HTMLTextAreaElement | null>;
  };
  answerLength: number;
  wordCap: number;
  speechRatePerMinute?: number;
  busy: boolean;
  aiCancellable?: boolean;
  humanWritePending?: boolean;
  synchronizationBlocked?: boolean;
  onSynchronize?: () => void;
  onSubmitQuestion: (event: FormEvent) => void;
  onGenerateQuestion: () => void;
  onInsertQuestioner: (slotIndex: number) => void;
  onSetQuestionerHuman?: (human: boolean) => void;
  onPrepareHumanAnswer: (event: FormEvent) => void;
  onGenerateAnswer: () => void;
  onSetRespondentHuman: (human: boolean) => void;
  onReveal: (units: number, elapsedMs: number) => Promise<CrossExamSnapshot>;
  onComplete: () => void | Promise<void>;
  onInterrupt: () => void;
  onTimeout: () => void;
  onEndSession: () => void;
  onCancel: () => void;
}) {
  const words = snapshot.meteringMode === "words";
  const controlsDisabled = busy || synchronizationBlocked;
  const switchingDisabled =
    synchronizationBlocked || humanWritePending || (busy && !aiCancellable);
  const currentPhase = snapshot.phase === "completed" ? "done" : snapshot.phase;
  const remainingUnits = Math.max(0, snapshot.remainingUnits ?? 0);
  const questionCap = words
    ? Math.min(snapshot.questionWordCap ?? wordCap, remainingUnits)
    : wordCap;
  const answerCap = words
    ? Math.min(snapshot.answerWordCap ?? wordCap, remainingUnits)
    : wordCap;
  const questionLength = countEffectiveUnits(
    String(questionTextareaProps.value ?? ""),
  );
  const [localRemainingMs, setLocalRemainingMs] = useState(
    snapshot.remainingMs,
  );
  const timedOut = useRef(false);
  const onTimeoutRef = useRef(onTimeout);
  onTimeoutRef.current = onTimeout;

  useEffect(() => {
    setLocalRemainingMs(snapshot.remainingMs);
    timedOut.current = false;
    const consumesTime =
      (snapshot.phase === "question" && snapshot.questionerHuman) ||
      (snapshot.phase === "preparing" && snapshot.respondentHuman) ||
      snapshot.phase === "revealing";
    if (words || !consumesTime || controlsDisabled || snapshot.remainingMs <= 0)
      return;
    const started = performance.now();
    const timer = window.setInterval(() => {
      const remaining = Math.max(
        0,
        snapshot.remainingMs - Math.round(performance.now() - started),
      );
      setLocalRemainingMs(remaining);
      if (remaining === 0 && !timedOut.current) {
        timedOut.current = true;
        window.clearInterval(timer);
        onTimeoutRef.current();
      }
    }, 250);
    return () => window.clearInterval(timer);
  }, [
    snapshot.eventId,
    snapshot.roundNumber,
    snapshot.phase,
    snapshot.remainingMs,
    snapshot.questionerHuman,
    snapshot.respondentHuman,
    words,
    controlsDisabled,
  ]);

  return (
    <section className="cross-exam-panel" aria-label="质询">
      <header className="cross-header">
        <div>
          <p className="eyebrow">
            第 {snapshot.roundNumber} 轮 · 当前质询方：
            {snapshot.nextSide === "affirmative" ? "正方" : "反方"}
          </p>
          <h2>质询控制台</h2>
        </div>
        <div className="cross-clock">
          <span>{words ? "质询方共享字池" : "共享剩余"}</span>
          <strong>
            {words
              ? `${remainingUnits.toLocaleString("zh-CN")} 字`
              : `${Math.ceil(localRemainingMs / 1000)}s`}
          </strong>
        </div>
      </header>
      <div className="cross-phase-line" aria-label="质询阶段">
        {(["question", "preparing", "revealing", "done"] as const).map(
          (phase, index) => (
            <span
              key={phase}
              className={
                currentPhase === phase
                  ? "current"
                  : ["question", "preparing", "revealing", "done"].indexOf(
                        currentPhase,
                      ) > index
                    ? "complete"
                    : ""
              }
            >
              {index + 1}.{" "}
              {phase === "question"
                ? "提问"
                : phase === "preparing"
                  ? "准备回答"
                  : phase === "revealing"
                    ? "回答展示"
                    : "本轮完成"}
            </span>
          ),
        )}
      </div>
      <div className="cross-roles">
        <article
          className={
            snapshot.nextSide === "affirmative" ? "affirmative" : "negative"
          }
        >
          <span>提问者</span>
          <strong>{slotLabel(snapshot.questioner)}</strong>
          <StatusBadge>{snapshot.questionerHuman ? "真人" : "AI"}</StatusBadge>
        </article>
        <div className="cross-versus" aria-hidden="true">
          问 → 答
        </div>
        <article
          className={
            snapshot.respondent.startsWith("AFF") ? "affirmative" : "negative"
          }
        >
          <span>回答者</span>
          <strong>{slotLabel(snapshot.respondent)}</strong>
          <StatusBadge>{snapshot.respondentHuman ? "真人" : "AI"}</StatusBadge>
        </article>
      </div>
      <p className="selection-summary">
        派位说明：{readableRoleText(snapshot.selectionSummary)}
      </p>
      {words ? (
        <p>
          问答均扣当前质询方字池；仅正式公开文本扣字，打断时仅扣已公开前缀。
        </p>
      ) : null}
      {busy && aiCancellable ? (
        <button type="button" onClick={onCancel}>
          取消生成
        </button>
      ) : null}
      {synchronizationBlocked ? (
        <div role="status">
          <p>回答提交状态尚未确认，已暂停操作；同步后台现场后才能继续。</p>
          <button type="button" disabled={busy} onClick={onSynchronize}>
            同步质询现场
          </button>
        </div>
      ) : null}

      {snapshot.phase === "question" ? (
        <div>
          {onSetQuestionerHuman ? (
            <div
              className="current-control"
              role="group"
              aria-label="本轮提问控制权"
            >
              <button
                type="button"
                aria-pressed={snapshot.questionerHuman}
                disabled={switchingDisabled || snapshot.questionerHuman}
                onClick={() => onSetQuestionerHuman(true)}
              >
                我来提问
              </button>
              <button
                type="button"
                aria-pressed={!snapshot.questionerHuman}
                disabled={switchingDisabled || !snapshot.questionerHuman}
                onClick={() => onSetQuestionerHuman(false)}
              >
                交给 AI 提问
              </button>
              <small>保留问题草稿；交给 AI 后仍需点击生成。</small>
            </div>
          ) : null}
          <fieldset>
            <legend>人工插入（只选择本方提问辩位）</legend>
            {snapshot.eligibleQuestioners.map((slot) => (
              <button
                key={slot}
                type="button"
                disabled={
                  synchronizationBlocked ||
                  switchingDisabled ||
                  (slot === snapshot.questioner && snapshot.questionerHuman)
                }
                onClick={() => onInsertQuestioner(slotIndex(slot))}
              >
                {slotLabel(slot)}
              </button>
            ))}
          </fieldset>
          {snapshot.questionerHuman ? (
            <form onSubmit={onSubmitQuestion}>
              <label>
                真人提问
                <textarea
                  name="question"
                  rows={3}
                  disabled={controlsDisabled}
                  {...questionTextareaProps}
                />
              </label>
              {words ? (
                <div className="composer-meta">
                  <span
                    className={questionLength > questionCap ? "over-cap" : ""}
                  >
                    已输入 {questionLength} / {questionCap} 字
                  </span>
                  <span>
                    草稿提交后预计剩余{" "}
                    {Math.max(
                      0,
                      remainingUnits - Math.min(questionLength, questionCap),
                    ).toLocaleString("zh-CN")}{" "}
                    字
                  </span>
                </div>
              ) : null}
              <button type="submit" disabled={controlsDisabled}>
                提交问题
              </button>
            </form>
          ) : (
            <button
              type="button"
              onClick={onGenerateQuestion}
              disabled={controlsDisabled}
            >
              AI 生成并提交问题
            </button>
          )}
        </div>
      ) : null}

      {snapshot.question ? (
        <p className="cross-question">问：{snapshot.question}</p>
      ) : null}

      {snapshot.phase === "preparing" ? (
        <div>
          <p>
            回答控制：
            <button
              type="button"
              disabled={switchingDisabled || snapshot.respondentHuman}
              onClick={() => onSetRespondentHuman(true)}
            >
              真人回答
            </button>
            <button
              type="button"
              disabled={switchingDisabled || !snapshot.respondentHuman}
              onClick={() => onSetRespondentHuman(false)}
            >
              AI 回答
            </button>
          </p>
          {snapshot.respondentHuman ? (
            <form onSubmit={onPrepareHumanAnswer}>
              <label>
                真人回答（提交后按比赛语速展示）
                <textarea
                  aria-label="质询回答"
                  rows={5}
                  disabled={controlsDisabled}
                  {...answerTextareaProps}
                />
              </label>
              <p className={answerLength > answerCap ? "over-cap" : ""}>
                已输入 {answerLength}，上限 {answerCap}
                {words ? " 字" : ""}
              </p>
              {words ? (
                <p>
                  草稿提交后预计剩余{" "}
                  {Math.max(
                    0,
                    remainingUnits - Math.min(answerLength, answerCap),
                  ).toLocaleString("zh-CN")}{" "}
                  字
                </p>
              ) : null}
              <button
                type="submit"
                disabled={controlsDisabled || answerLength === 0}
              >
                准备并开始展示
              </button>
            </form>
          ) : (
            <button
              type="button"
              onClick={onGenerateAnswer}
              disabled={controlsDisabled}
            >
              AI 准备回答
            </button>
          )}
        </div>
      ) : null}

      {snapshot.phase === "revealing" && snapshot.answerPrepared ? (
        <PreparedAnswerReveal
          key={`${snapshot.eventId}:${snapshot.roundNumber}`}
          visiblePrefix={snapshot.visiblePrefix}
          revealedUnits={snapshot.revealedUnits}
          totalUnits={snapshot.totalUnits}
          unitsPerMinute={words ? 180 : speechRatePerMinute}
          allowPacingControls={words}
          interruptible={snapshot.interruptible}
          synchronizationBlocked={synchronizationBlocked}
          paused={busy}
          onReveal={onReveal}
          onComplete={onComplete}
          onInterrupt={onInterrupt}
        />
      ) : null}
      {snapshot.phase === "done" ? <p>本轮质询已经提交。</p> : null}
      {snapshot.phase === "completed" ? <p>本环节质询已经结束。</p> : null}
      {snapshot.history.length > 0 ? (
        <details className="cross-history">
          <summary>本环节已完成 {snapshot.history.length} 轮问答</summary>
          <ol>
            {snapshot.history.map((item) => (
              <li
                key={`${item.roundNumber}-${item.questioner}-${item.respondent}`}
              >
                第 {item.roundNumber} 轮：{slotLabel(item.questioner)}问“
                {item.question}”；{slotLabel(item.respondent)}答“
                {item.answer || "未作答"}” （{item.status}）
              </li>
            ))}
          </ol>
        </details>
      ) : null}
      <button
        type="button"
        className="danger-ghost end-cross-session"
        disabled={controlsDisabled || snapshot.phase === "completed"}
        onClick={onEndSession}
      >
        结束整个质询环节
      </button>
    </section>
  );
}
