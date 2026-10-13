import { useEffect, useRef, useState } from "react";
import type { CrossExamSnapshot } from "./CrossExamPanel";

/** One acknowledged unit per tick. IPC/background delays never create reveal bursts. */
export function PreparedAnswerReveal({
  visiblePrefix,
  revealedUnits,
  totalUnits,
  unitsPerMinute = 180,
  allowPacingControls = false,
  interruptible,
  synchronizationBlocked = false,
  paused = false,
  onReveal,
  onComplete,
  onInterrupt,
}: {
  visiblePrefix: string;
  revealedUnits: number;
  totalUnits: number;
  unitsPerMinute?: number;
  allowPacingControls?: boolean;
  interruptible: boolean;
  synchronizationBlocked?: boolean;
  paused?: boolean;
  onReveal: (units: number, elapsedMs: number) => Promise<CrossExamSnapshot>;
  onComplete: () => void | Promise<void>;
  onInterrupt: () => void;
}) {
  const initialUnits = useRef(revealedUnits);
  const clock = useRef({
    elapsed: 0,
    started: performance.now(),
    running: true,
  });
  const completing = useRef(false);
  const pending = useRef(false);
  const mounted = useRef(true);
  const [busy, setBusy] = useState(false);
  const [chosenRate, setChosenRate] = useState<number | null>(null);
  const [hidden, setHidden] = useState(document.visibilityState === "hidden");
  const textPanel = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const rate = chosenRate ?? unitsPerMinute;
  useEffect(() => {
    if (following.current && textPanel.current) {
      textPanel.current.scrollTop = textPanel.current.scrollHeight;
    }
  }, [visiblePrefix]);
  const [failure, setFailure] = useState<{
    kind: "reveal" | "complete";
    message: string;
  } | null>(null);
  const onRevealRef = useRef(onReveal);
  const onCompleteRef = useRef(onComplete);
  const retryComplete = useRef<(() => Promise<void>) | null>(null);
  onRevealRef.current = onReveal;
  onCompleteRef.current = onComplete;

  function activeElapsed() {
    return (
      clock.current.elapsed +
      (clock.current.running ? performance.now() - clock.current.started : 0)
    );
  }

  useEffect(() => {
    const blocked =
      paused || synchronizationBlocked || failure !== null || hidden;
    if (blocked && clock.current.running) {
      clock.current.elapsed = activeElapsed();
      clock.current.running = false;
    } else if (!blocked && !clock.current.running) {
      clock.current.started = performance.now();
      clock.current.running = true;
    }
  }, [paused, synchronizationBlocked, failure, hidden]);

  useEffect(() => {
    const update = () => setHidden(document.visibilityState === "hidden");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    async function attempt(kind: "reveal" | "complete") {
      if (pending.current || (kind === "complete" && completing.current))
        return;
      pending.current = true;
      setBusy(true);
      try {
        if (kind === "complete") {
          completing.current = true;
          await onCompleteRef.current();
          if (mounted.current) setFailure(null);
        } else {
          const elapsed = Math.round(activeElapsed());
          const target = Math.min(totalUnits, revealedUnits + 1);
          await onRevealRef.current(target, elapsed);
        }
      } catch (caught) {
        if (caught instanceof Error && caught.name === "SupersededOperation")
          return;
        if (mounted.current) {
          if (kind === "complete") completing.current = false;
          // A failed reveal was not acknowledged. Retry from the confirmed
          // prefix, without treating the offline interval as spoken text.
          clock.current = {
            elapsed:
              (Math.max(0, revealedUnits - initialUnits.current) * 60_000) /
              Math.max(1, rate),
            started: performance.now(),
            running: false,
          };
          setFailure({
            kind,
            message: caught instanceof Error ? caught.message : String(caught),
          });
        }
      } finally {
        pending.current = false;
        if (mounted.current) setBusy(false);
      }
    }
    // Explicit retry enters MatchPage's authority-read recovery path; a blocked
    // snapshot never triggers an automatic completion write.
    retryComplete.current = () => attempt("complete");
    if (
      failure ||
      pending.current ||
      paused ||
      synchronizationBlocked ||
      hidden
    )
      return;
    if (totalUnits <= 0 || revealedUnits >= totalUnits) {
      if (totalUnits > 0 && !completing.current) {
        void attempt("complete");
      }
      return;
    }
    const tickMs = Math.max(40, Math.round(60_000 / Math.max(1, rate)));
    const timer = window.setTimeout(() => {
      void attempt("reveal");
    }, tickMs);
    return () => window.clearTimeout(timer);
  }, [
    revealedUnits,
    totalUnits,
    rate,
    failure,
    busy,
    synchronizationBlocked,
    paused,
    hidden,
  ]);

  return (
    <div className="prepared-reveal">
      <div className="reveal-controls">
        {allowPacingControls ? (
          <label>
            回答展示速度
            <select
              aria-label="回答展示速度"
              value={rate}
              disabled={busy && completing.current}
              onChange={(event) => setChosenRate(Number(event.target.value))}
            >
              <option value={120}>慢速 · 2 字/秒</option>
              <option value={180}>舒缓 · 3 字/秒</option>
              <option value={240}>正常 · 4 字/秒</option>
            </select>
          </label>
        ) : null}
        {interruptible ? (
          <button
            type="button"
            className="reveal-interrupt"
            disabled={
              revealedUnits <= 0 ||
              (busy && completing.current) ||
              synchronizationBlocked ||
              paused ||
              hidden
            }
            onClick={onInterrupt}
          >
            打断
          </button>
        ) : null}
      </div>
      {allowPacingControls ? (
        <small>只调整展示节奏，不改变字池；打断只保留已公开前缀。</small>
      ) : null}
      <div
        ref={textPanel}
        className="revealed-text"
        role="log"
        aria-label="已公开回答"
        aria-live="off"
        onScroll={(event) => {
          const panel = event.currentTarget;
          following.current =
            panel.scrollHeight - panel.scrollTop - panel.clientHeight < 32;
        }}
      >
        <p>{visiblePrefix || "即将开始展示…"}</p>
      </div>
      <p>
        已展示 {revealedUnits}/{totalUnits} 有效单位
        {interruptible ? " · 可打断" : " · 不可打断"}
      </p>
      {failure ? (
        <div role="alert">
          <p>{failure.message}</p>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              if (failure.kind === "complete") void retryComplete.current?.();
              else setFailure(null);
            }}
          >
            {failure.kind === "reveal" ? "重试展示" : "重试提交回答"}
          </button>
        </div>
      ) : null}
    </div>
  );
}
