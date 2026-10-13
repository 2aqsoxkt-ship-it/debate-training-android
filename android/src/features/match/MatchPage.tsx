import { FormEvent, useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { ConfirmDialog } from "../../components/ui/Overlay";
import { call } from "../../lib/tauri";
import {
  CrossExamPanel,
  type CrossExamSnapshot,
} from "../arena/CrossExamPanel";
import { MatchRoom } from "../match-room/MatchRoom";
import { speakerLabel } from "../../lib/presentation";
import {
  emptySnapshot,
  routeForSnapshot,
  type Snapshot,
} from "../session/types";
import { useInputTelemetry } from "../telemetry/useInputTelemetry";
import { countEffectiveUnits } from "../telemetry/units";
import { useDurableDraft } from "./useDurableDraft";

class SupersededOperation extends Error {
  constructor() {
    super("此操作已被新的控制操作替代");
    this.name = "SupersededOperation";
  }
}

export function MatchPage() {
  const { matchId = "" } = useParams();
  const navigate = useNavigate();
  const [view, setView] = useState<Snapshot>(emptySnapshot);
  const [error, setError] = useState("");
  const [aiFailure, setAiFailure] = useState("");
  const [thinking, setThinking] = useState<string | null>(null);
  const [aiGenerating, setAiGenerating] = useState(false);
  const [humanSubmitting, setHumanSubmitting] = useState(false);
  const [crossExam, setCrossExam] = useState<CrossExamSnapshot | null>(null);
  const [crossRecoveryPending, setCrossRecoveryPending] = useState(false);
  const [windowStart, setWindowStart] = useState(0);
  const [controlScope, setControlScope] = useState<"turn" | "match">("turn");
  const [pendingConfirmation, setPendingConfirmation] = useState<
    | { kind: "controller"; index: number; toHuman: boolean }
    | { kind: "current"; index: number; toHuman: boolean }
    | { kind: "claim"; index: number }
    | null
  >(null);
  const submittingRef = useRef(false);
  const crossHumanWrite = useRef(false);
  const switchingControl = useRef(false);
  // Every explicit action revokes older UI responses, even within one event.
  const operation = useRef(0);
  const alive = useRef(true);
  const finishingCross = useRef(false);
  const uncertainCross = useRef<CrossExamSnapshot | null>(null);
  const currentEvent = view.schedule[view.cursor];
  const wordCap =
    view.meteringMode === "words"
      ? (view.currentWordCap ??
        currentEvent?.wordCap ??
        Number.POSITIVE_INFINITY)
      : (currentEvent?.wordCap ?? Number.POSITIVE_INFINITY);
  const isCrossExam = currentEvent?.kind === "cross_exam";
  const crossIsCurrent = isCrossExam && crossExam?.eventId === currentEvent?.id;

  function isCurrent(epoch: number) {
    return alive.current && epoch === operation.current;
  }

  function humanWriteInFlight() {
    return submittingRef.current || crossHumanWrite.current;
  }

  async function adoptSnapshot(
    next: Snapshot,
    epoch: number,
    knownCross?: CrossExamSnapshot,
  ) {
    if (!isCurrent(epoch)) return;
    const event = next.schedule[next.cursor];
    let cross: CrossExamSnapshot | null = null;
    if (event?.kind === "cross_exam") {
      cross =
        knownCross?.eventId === event.id
          ? knownCross
          : await call<CrossExamSnapshot>("cross_exam_snapshot");
      if (!isCurrent(epoch)) return;
      // A just-ended session can return its terminal snapshot once.
      if (cross.eventId !== event.id)
        cross = await call<CrossExamSnapshot>("cross_exam_snapshot");
    }
    if (!isCurrent(epoch)) return;
    setView(next);
    setCrossExam(cross);
    const target = routeForSnapshot(next);
    if (target !== `/match/${matchId}`) navigate(target, { replace: true });
    return { view: next, cross };
  }

  async function refreshAuthority(epoch: number) {
    const next = await call<Snapshot>("workspace_snapshot");
    return adoptSnapshot(next, epoch);
  }

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      operation.current += 1;
    };
  }, [matchId]);
  const telemetry = useInputTelemetry({
    wordCap,
    active:
      view.phase === "in_progress" &&
      view.currentHuman &&
      !view.awaitingSeat &&
      !isCrossExam &&
      thinking === null,
    sessionKey: `${matchId}:${currentEvent?.id ?? ""}:${view.transcripts.length}:${view.currentSpeaker || currentEvent?.speaker || ""}`,
  });
  const crossTelemetry = useInputTelemetry({
    wordCap:
      view.meteringMode === "words"
        ? (crossExam?.answerWordCap ?? wordCap)
        : wordCap,
    active: Boolean(
      crossIsCurrent &&
      crossExam?.phase === "preparing" &&
      crossExam.respondentHuman &&
      thinking === null,
    ),
    sessionKey: `${matchId}:${crossExam?.eventId ?? ""}:${crossExam?.roundNumber ?? 0}:${crossExam?.respondent ?? ""}:answer`,
  });
  const crossQuestionTelemetry = useInputTelemetry({
    wordCap:
      view.meteringMode === "words"
        ? (crossExam?.questionWordCap ?? wordCap)
        : wordCap,
    active: Boolean(
      crossIsCurrent &&
      crossExam?.phase === "question" &&
      crossExam.questionerHuman &&
      thinking === null,
    ),
    sessionKey: `${matchId}:${crossExam?.eventId ?? ""}:${crossExam?.roundNumber ?? 0}:${crossExam?.questioner ?? ""}:question`,
  });

  function draftKey(slot: string, fieldKind: "speech" | "question" | "answer") {
    return view.currentWindowId && slot
      ? { matchId, windowId: view.currentWindowId, slot, fieldKind }
      : null;
  }
  const speechDraft = useDurableDraft(
    !isCrossExam
      ? draftKey(view.currentSpeaker || currentEvent?.speaker || "", "speech")
      : null,
    telemetry,
  );
  const questionDraft = useDurableDraft(
    crossIsCurrent && crossExam?.phase === "question"
      ? draftKey(crossExam.questioner, "question")
      : null,
    crossQuestionTelemetry,
  );
  const answerDraft = useDurableDraft(
    crossIsCurrent && crossExam?.phase === "preparing"
      ? draftKey(crossExam.respondent, "answer")
      : null,
    crossTelemetry,
  );
  async function flushDrafts() {
    await Promise.all([
      speechDraft.flush(),
      questionDraft.flush(),
      answerDraft.flush(),
    ]);
  }
  const flushDraftsRef = useRef(flushDrafts);
  flushDraftsRef.current = flushDrafts;
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let closing = false;
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        const window = getCurrentWindow();
        const unlisten = await window.onCloseRequested(async (event) => {
          event.preventDefault();
          if (closing) return;
          closing = true;
          try {
            await flushDraftsRef.current();
            await call("cancel_ai_stream");
            await window.destroy();
          } catch (caught) {
            closing = false;
            setError(`关闭前草稿保存失败：${String(caught)}`);
          }
        });
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        /* Browser tests have no native close lifecycle. */
      });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    let active = true;
    const epoch = operation.current;
    async function load() {
      try {
        let next = await call<Snapshot>("workspace_snapshot");
        if (next.matchId !== matchId) {
          next = await call<Snapshot>("resume_match", { id: matchId });
        }
        if (!active || !isCurrent(epoch)) return;
        const target = routeForSnapshot(next);
        if (target !== `/match/${matchId}`) {
          navigate(target, { replace: true });
          return;
        }
        setView(next);
      } catch (caught) {
        if (!active || !isCurrent(epoch)) return;
        setError(caught instanceof Error ? caught.message : String(caught));
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, [matchId, navigate]);

  useEffect(() => {
    if (currentEvent?.kind !== "cross_exam") {
      setCrossExam(null);
      return;
    }
    crossTelemetry.reset();
    crossQuestionTelemetry.reset();
    let active = true;
    const epoch = operation.current;
    void call<CrossExamSnapshot>("cross_exam_snapshot")
      .then((next) => {
        if (active && isCurrent(epoch)) setCrossExam(next);
      })
      .catch((caught) => {
        if (active && isCurrent(epoch))
          setError(caught instanceof Error ? caught.message : String(caught));
      });
    return () => {
      active = false;
    };
    // The event identity is the reset boundary; live input must not refetch it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentEvent?.id, currentEvent?.kind]);

  async function run(command: string, args?: Record<string, unknown>) {
    if (humanWriteInFlight() && command !== "submit_turn") return undefined;
    const epoch = ++operation.current;
    setAiGenerating(command === "advance_ai");
    setError("");
    if (command === "advance_ai") setAiFailure("");
    if (command === "advance_ai" || command === "finish_match") {
      setThinking(command === "advance_ai" ? "正在思考" : "正在评分");
      await new Promise((resolve) =>
        window.requestAnimationFrame(() =>
          window.requestAnimationFrame(() => resolve(undefined)),
        ),
      );
    }
    try {
      if (!isCurrent(epoch)) return undefined;
      const next = await call<Snapshot>(command, args);
      if (!isCurrent(epoch)) return undefined;
      await adoptSnapshot(next, epoch);
      if (!isCurrent(epoch)) return undefined;
      return next;
    } catch (caught) {
      if (!isCurrent(epoch)) return undefined;
      const message = caught instanceof Error ? caught.message : String(caught);
      setError(message);
      if (command === "advance_ai" && !message.includes("已取消")) {
        setAiFailure(message);
      }
      return undefined;
    } finally {
      if (isCurrent(epoch)) {
        setThinking(null);
        setAiGenerating(false);
      }
    }
  }

  async function submitTelemetry() {
    if (telemetry.isComposing()) {
      setError("请先确认输入法候选词，再提交发言。");
      return;
    }
    if (submittingRef.current) return;
    submittingRef.current = true;
    setHumanSubmitting(true);
    try {
      const payload = telemetry.snapshot();
      setThinking("正在提交发言");
      const pauseIntervals = payload.events.flatMap((item) =>
        item.kind === "pause" ? [item.ms] : [],
      );
      const submitted = await run("submit_turn", {
        text: payload.text,
        inputMs: payload.claimed.inputElapsedMs,
        pauseIntervals,
        reverseUnits: payload.claimed.reverseUnits,
        reverseActions: payload.claimed.reverseActions,
        pasted: payload.claimed.pasteUnits > 0,
        events: payload.events,
        claimed: payload.claimed,
      });
      if (submitted) telemetry.reset();
    } finally {
      submittingRef.current = false;
      if (alive.current) setHumanSubmitting(false);
    }
  }

  async function crossCall(
    command: string,
    args?: Record<string, unknown>,
    quiet = false,
  ) {
    if (uncertainCross.current)
      throw new Error("请先同步质询现场，确认回答提交状态");
    const epoch = quiet ? operation.current : ++operation.current;
    setError("");
    if (!quiet) {
      setThinking("正在处理质询");
      setAiGenerating(
        command === "cross_exam_generate_question" ||
          command === "cross_exam_generate_answer",
      );
    }
    try {
      const next = await call<CrossExamSnapshot>(command, args);
      if (!isCurrent(epoch)) throw new SupersededOperation();
      if (quiet) setCrossExam(next);
      else {
        const workspace = await call<Snapshot>("workspace_snapshot");
        await adoptSnapshot(workspace, epoch, next);
      }
      if (!isCurrent(epoch)) throw new SupersededOperation();
      return next;
    } catch (caught) {
      if (isCurrent(epoch)) {
        const message =
          caught instanceof Error ? caught.message : String(caught);
        setError(message);
        if (!quiet) {
          try {
            await refreshAuthority(epoch);
          } catch (syncError) {
            if (isCurrent(epoch))
              setError(
                `${message}；现场同步失败：${syncError instanceof Error ? syncError.message : String(syncError)}`,
              );
          }
        }
      }
      throw caught;
    } finally {
      if (!quiet && isCurrent(epoch)) {
        setThinking(null);
        setAiGenerating(false);
      }
    }
  }

  async function submitCrossQuestion(event: FormEvent) {
    event.preventDefault();
    if (crossQuestionTelemetry.isComposing()) {
      setError("请先确认输入法候选词，再提交问题。");
      return;
    }
    if (humanWriteInFlight()) return;
    const payload = crossQuestionTelemetry.snapshot();
    if (!payload.text.trim()) return;
    crossHumanWrite.current = true;
    setHumanSubmitting(true);
    const epoch = ++operation.current;
    setThinking("正在提交问题");
    try {
      const next = await call<Snapshot>("cross_exam_submit_question", {
        question: payload.text,
        inputMs: payload.claimed.inputElapsedMs,
        events: payload.events,
        claimed: payload.claimed,
      });
      await adoptSnapshot(next, epoch);
      if (isCurrent(epoch)) crossQuestionTelemetry.reset();
    } catch (caught) {
      if (isCurrent(epoch))
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      crossHumanWrite.current = false;
      if (alive.current) setHumanSubmitting(false);
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  async function prepareCrossHumanAnswer(event: FormEvent) {
    event.preventDefault();
    if (crossTelemetry.isComposing()) {
      setError("请先确认输入法候选词，再提交回答。");
      return;
    }
    if (humanWriteInFlight()) return;
    crossHumanWrite.current = true;
    setHumanSubmitting(true);
    const payload = crossTelemetry.snapshot();
    try {
      await crossCall("cross_exam_prepare_answer", {
        fullText: payload.text,
        events: payload.events,
        claimed: payload.claimed,
      })
        .then(() => crossTelemetry.reset())
        .catch(() => undefined);
    } finally {
      crossHumanWrite.current = false;
      if (alive.current) setHumanSubmitting(false);
    }
  }

  async function finishCrossExam(interrupt: boolean) {
    if (finishingCross.current) return;
    finishingCross.current = true;
    const epoch = ++operation.current;
    const attempted = uncertainCross.current ?? crossExam;
    setThinking(interrupt ? "正在提交打断" : "正在提交回答");
    try {
      // A failed command may already have committed. Read before any retry.
      if (uncertainCross.current) {
        const authority = await refreshAuthority(epoch);
        if (!isCurrent(epoch)) return;
        const current = authority?.cross;
        if (
          !current ||
          !attempted ||
          current.eventId !== attempted.eventId ||
          current.roundNumber !== attempted.roundNumber ||
          current.phase !== "revealing"
        ) {
          uncertainCross.current = null;
          setCrossRecoveryPending(false);
          setError("");
          return;
        }
      }
      const next = await call<Snapshot>(
        interrupt ? "cross_exam_interrupt" : "cross_exam_complete",
        interrupt
          ? {
              reason: "问方打断",
              humanInputMs: 0,
              displayedUnits: attempted?.revealedUnits,
              eventId: attempted?.eventId,
              roundNumber: attempted?.roundNumber,
            }
          : { humanInputMs: 0 },
      );
      await adoptSnapshot(next, epoch);
      if (!isCurrent(epoch)) return;
      uncertainCross.current = null;
      setCrossRecoveryPending(false);
      setError("");
      crossTelemetry.reset();
      crossQuestionTelemetry.reset();
    } catch (caught) {
      if (!isCurrent(epoch)) return;
      uncertainCross.current = attempted;
      setCrossRecoveryPending(true);
      const message = caught instanceof Error ? caught.message : String(caught);
      setError(message);
      try {
        const authority = await refreshAuthority(epoch);
        if (!isCurrent(epoch)) return;
        const current = authority?.cross;
        if (
          !current ||
          !attempted ||
          current.eventId !== attempted.eventId ||
          current.roundNumber !== attempted.roundNumber ||
          current.phase !== "revealing"
        ) {
          uncertainCross.current = null;
          setCrossRecoveryPending(false);
          setError(
            `回答已在后台处理，未重复提交；现场已同步。原错误：${message}`,
          );
          return;
        }
      } catch {
        // Keep the uncertainty marker: retry must read authority successfully.
      }
      if (isCurrent(epoch)) throw new Error(message);
    } finally {
      finishingCross.current = false;
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  async function finishCrossExamSession(timeout: boolean) {
    if (uncertainCross.current || humanWriteInFlight()) return;
    const epoch = ++operation.current;
    setThinking(timeout ? "质询时间已到" : "正在结束质询");
    try {
      const next = await call<Snapshot>(
        timeout ? "cross_exam_timeout" : "cross_exam_end_session",
      );
      await adoptSnapshot(next, epoch);
      if (!isCurrent(epoch)) return;
      uncertainCross.current = null;
      setCrossRecoveryPending(false);
      crossTelemetry.reset();
      crossQuestionTelemetry.reset();
    } catch (caught) {
      if (isCurrent(epoch))
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  async function setController(
    index: number,
    toHuman: boolean,
    second = false,
  ) {
    if (humanWriteInFlight()) return;
    if (uncertainCross.current) {
      setError("请先同步质询现场，确认回答提交状态");
      return;
    }
    const epoch = ++operation.current;
    setThinking("正在更新控制权");
    try {
      const next = await call<Snapshot>("set_event_controller", {
        index,
        toHuman,
        secondConfirmed: second,
      });
      await adoptSnapshot(next, epoch);
    } catch (caught) {
      if (!isCurrent(epoch)) return;
      const message = caught instanceof Error ? caught.message : String(caught);
      if (!second && message.includes("二次确认")) {
        setPendingConfirmation({ kind: "controller", index, toHuman });
      } else setError(message);
    } finally {
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  async function claim(index: number, second = false) {
    if (humanWriteInFlight()) return;
    if (uncertainCross.current) {
      setError("请先同步质询现场，确认回答提交状态");
      return;
    }
    const epoch = ++operation.current;
    setThinking("正在接管辩位");
    try {
      const next = await call<Snapshot>("take_seat", {
        slotIndex: index,
        secondConfirmed: second,
      });
      await adoptSnapshot(next, epoch);
    } catch (caught) {
      if (!isCurrent(epoch)) return;
      const message = caught instanceof Error ? caught.message : String(caught);
      if (!second && message.includes("再确认")) {
        setPendingConfirmation({ kind: "claim", index });
      } else setError(message);
    } finally {
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  async function setCurrentController(
    index: number,
    toHuman: boolean,
    second = false,
  ) {
    if (
      humanWriteInFlight() ||
      switchingControl.current ||
      finishingCross.current ||
      uncertainCross.current
    )
      return;
    switchingControl.current = true;
    let epoch = operation.current;
    const previousThinking = thinking;
    const previousGenerating = aiGenerating;
    let accepted = false;
    setThinking("正在切换本轮控制权");
    setError("");
    setAiFailure("");
    try {
      // Validation, cancellation and revision change are one backend action.
      // Failed scoring confirmation must not cancel a still-valid request.
      await flushDrafts();
      const next = await call<Snapshot>("set_current_controller", {
        slotIndex: index,
        toHuman,
        secondConfirmed: second,
        scope: controlScope,
      });
      if (!isCurrent(epoch)) return;
      epoch = ++operation.current;
      accepted = true;
      setAiGenerating(false);
      await adoptSnapshot(next, epoch);
    } catch (caught) {
      if (!isCurrent(epoch)) return;
      const message = caught instanceof Error ? caught.message : String(caught);
      if (
        !second &&
        (message.includes("再确认") || message.includes("二次确认"))
      ) {
        setPendingConfirmation({ kind: "current", index, toHuman });
      } else setError(message);
      await refreshAuthority(epoch).catch(() => undefined);
    } finally {
      switchingControl.current = false;
      if (isCurrent(epoch)) {
        setThinking(accepted ? null : previousThinking);
        if (!accepted) setAiGenerating(previousGenerating);
      }
    }
  }

  function crossSlotIndex(slot: string) {
    const match = /^(AFF|NEG)_([1-4])$/.exec(slot);
    return match ? (match[1] === "NEG" ? 4 : 0) + Number(match[2]) - 1 : -1;
  }

  async function cancelGeneration() {
    if (humanWriteInFlight() || !aiGenerating) return;
    if (finishingCross.current || uncertainCross.current) {
      setError("回答正在确认提交状态，不能当作 AI 生成取消");
      return;
    }
    const epoch = ++operation.current;
    setThinking("正在取消，请等待请求退出");
    try {
      await call("cancel_ai_stream");
      setThinking(null);
      setAiGenerating(false);
      await refreshAuthority(epoch);
    } catch (caught) {
      if (isCurrent(epoch))
        setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  async function synchronizeCross() {
    if (humanWriteInFlight()) return;
    const epoch = ++operation.current;
    setThinking("正在同步质询现场");
    try {
      await refreshAuthority(epoch);
      if (!isCurrent(epoch)) return;
      uncertainCross.current = null;
      setCrossRecoveryPending(false);
      setError("");
    } catch (caught) {
      if (isCurrent(epoch))
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  async function abortMatch() {
    if (
      humanWriteInFlight() ||
      finishingCross.current ||
      uncertainCross.current
    )
      return;
    const epoch = ++operation.current;
    setThinking("正在保存并中止");
    try {
      await flushDrafts();
      await call<Snapshot>("abort_match");
      if (!isCurrent(epoch)) return;
      navigate("/workspace", { replace: true });
    } catch (caught) {
      if (isCurrent(epoch))
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (isCurrent(epoch)) setThinking(null);
    }
  }

  const crossWorkspace =
    currentEvent?.kind === "cross_exam" && crossExam ? (
      <CrossExamPanel
        snapshot={crossExam}
        questionTextareaProps={crossQuestionTelemetry.bindTextareaProps}
        answerTextareaProps={crossTelemetry.bindTextareaProps}
        answerLength={countEffectiveUnits(crossTelemetry.text)}
        wordCap={wordCap}
        speechRatePerMinute={view.speechRatePerMinute}
        busy={thinking !== null}
        aiCancellable={aiGenerating}
        humanWritePending={humanSubmitting}
        synchronizationBlocked={crossRecoveryPending}
        onSynchronize={() => {
          void synchronizeCross();
        }}
        onSubmitQuestion={(event) => void submitCrossQuestion(event)}
        onSetQuestionerHuman={(human) => {
          void setCurrentController(
            crossSlotIndex(crossExam.questioner),
            human,
          );
        }}
        onGenerateQuestion={() => {
          void crossCall("cross_exam_generate_question").catch(() => undefined);
        }}
        onInsertQuestioner={(slotIndex) => {
          void (async () => {
            if (humanWriteInFlight()) return;
            const epoch = ++operation.current;
            if (aiGenerating) {
              await call("cancel_ai_stream").catch(() => undefined);
              if (!isCurrent(epoch)) return;
              setThinking(null);
              setAiGenerating(false);
            }
            await crossCall("cross_exam_insert_human_questioner", {
              slotIndex,
            }).catch(() => undefined);
          })();
        }}
        onPrepareHumanAnswer={(event) => void prepareCrossHumanAnswer(event)}
        onGenerateAnswer={() => {
          void crossCall("cross_exam_generate_answer").catch(() => undefined);
        }}
        onSetRespondentHuman={(human) => {
          void setCurrentController(
            crossSlotIndex(crossExam.respondent),
            human,
          );
        }}
        onReveal={(units, elapsedMs) =>
          crossCall(
            "cross_exam_reveal",
            {
              units,
              elapsedMs,
              eventId: crossExam.eventId,
              roundNumber: crossExam.roundNumber,
            },
            true,
          )
        }
        onComplete={() => finishCrossExam(false)}
        onInterrupt={() => {
          void finishCrossExam(true).catch(() => undefined);
        }}
        onTimeout={() => void finishCrossExamSession(true)}
        onEndSession={() => void finishCrossExamSession(false)}
        onCancel={() => {
          void cancelGeneration();
        }}
      />
    ) : undefined;

  return (
    <main className="match-page">
      {[speechDraft, questionDraft, answerDraft].map((draft, index) => (
        <section key={index} className="draft-recovery" aria-live="polite">
          {draft.error && <p className="error">{draft.error}</p>}
          {draft.pending && (
            <>
              <p>
                发现此辩位未提交草稿（
                {countEffectiveUnits(draft.pending.payload.text)}{" "}
                字），不会自动发言。
              </p>
              {draft.pending.telemetryTrusted ? (
                <button type="button" onClick={draft.restore}>
                  恢复草稿和输入记录
                </button>
              ) : (
                <>
                  <p>输入记录损坏：仅可复制文本，不作为标准评分输入。</p>
                  <pre>{draft.pending.payload.text}</pre>
                </>
              )}
              <button type="button" onClick={draft.discard}>
                不恢复，重新输入
              </button>
            </>
          )}
        </section>
      ))}
      <label className="control-scope">
        人工／AI 切换范围
        <select
          aria-label="控制范围"
          value={controlScope}
          onChange={(event) =>
            setControlScope(event.target.value as "turn" | "match")
          }
          disabled={humanSubmitting}
        >
          <option value="turn">仅本轮（下轮恢复原分配）</option>
          <option value="match">此辩位的本轮及后续比赛</option>
        </select>
      </label>
      {error ? <p className="error match-error">{error}</p> : null}
      {view.publicIssues && view.publicIssues.length > 0 && (
        <details className="public-issues">
          <summary>
            公开交锋记录 · {view.publicIssues.length} 条（不自动判胜负）
          </summary>
          <div>
            {view.publicIssues.slice(-24).map((issue) => (
              <article key={issue.sourceTurnId}>
                <strong>{speakerLabel(issue.slot)}</strong>
                <p>{issue.publicClaim}</p>
                <small>
                  {issue.status} · 回应 {issue.responseTurnIds.length} 条 · 来源{" "}
                  {issue.sourceTurnId}
                </small>
              </article>
            ))}
          </div>
        </details>
      )}
      <MatchRoom
        view={view}
        textareaProps={telemetry.bindTextareaProps}
        draftLength={countEffectiveUnits(telemetry.text)}
        onSubmit={(event) => {
          event.preventDefault();
          void submitTelemetry();
        }}
        onClaim={(index) => void setCurrentController(index, true)}
        onAdvance={() => void run("advance_ai")}
        onCancel={() => {
          void cancelGeneration();
        }}
        onFinish={() => void run("finish_match")}
        onTimeoutSubmit={() => void submitTelemetry()}
        onSetController={(index, toHuman) => void setController(index, toHuman)}
        onSetCurrentController={(index, toHuman) =>
          void setCurrentController(index, toHuman)
        }
        thinking={thinking}
        aiCancellable={aiGenerating}
        interactionLocked={
          Boolean(
            speechDraft.pending || questionDraft.pending || answerDraft.pending,
          ) ||
          humanSubmitting ||
          crossRecoveryPending ||
          (thinking !== null && !aiGenerating)
        }
        aiFailure={aiFailure}
        onRetryAi={() => void run("advance_ai")}
        onTakeoverCurrent={() => {
          const seat = view.seats.find(
            (item) => item.speaking && item.claimable,
          );
          if (seat) void setCurrentController(seat.index, true);
        }}
        onDismissAiFailure={() => setAiFailure("")}
        windowStart={windowStart}
        onMore={() => setWindowStart((value) => value + 12)}
        crossExamActive={currentEvent?.kind === "cross_exam"}
        stageWorkspace={crossWorkspace}
        onAbort={() => void abortMatch()}
        onWaiveFreeDebate={() => void run("waive_free_debate_time")}
      />
      <ConfirmDialog
        open={pendingConfirmation !== null}
        title="确认临时接管"
        description="这次接管会把评分场次标记为非标准，但不会改变已经保存的发言。"
        confirmLabel="确认接管"
        onClose={() => setPendingConfirmation(null)}
        onConfirm={() => {
          const pending = pendingConfirmation;
          setPendingConfirmation(null);
          if (!pending) return;
          if (pending.kind === "controller") {
            void setController(pending.index, pending.toHuman, true);
          } else if (pending.kind === "current") {
            void setCurrentController(pending.index, pending.toHuman, true);
          } else {
            void claim(pending.index, true);
          }
        }}
      />
    </main>
  );
}
