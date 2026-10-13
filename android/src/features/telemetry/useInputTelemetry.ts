import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
  type TextareaHTMLAttributes,
} from "react";
import { applyWordCap, fold, toClaimed } from "./fold";
import { inputSource, observedEdit } from "./domEdit";
import type { ClaimedMetrics, FragmentSource, SourceEvent } from "./types";
import { PAUSE_GRACE_MS } from "./types";

export type UseInputTelemetryOptions = {
  wordCap?: number;
  initialText?: string;
  active?: boolean;
  sessionKey?: string;
};

export type TelemetryCommit = {
  text: string;
  events: SourceEvent[];
  claimed: ClaimedMetrics;
};

export type UseInputTelemetry = {
  text: string;
  setText: (next: string) => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  bindTextareaProps: TextareaHTMLAttributes<HTMLTextAreaElement> & {
    ref: RefObject<HTMLTextAreaElement | null>;
  };
  snapshot: () => TelemetryCommit;
  commit: () => TelemetryCommit;
  reset: () => void;
  restore: (saved: TelemetryCommit) => void;
  isComposing: () => boolean;
};

export function useInputTelemetry(
  options: UseInputTelemetryOptions = {},
): UseInputTelemetry {
  const wordCap = options.wordCap ?? Number.POSITIVE_INFINITY;
  const active = options.active ?? true;
  const [text, setTextState] = useState(options.initialText ?? "");
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const timelineRef = useRef<SourceEvent[]>(
    options.initialText
      ? [{ kind: "insert", text: options.initialText, source: "unverified" }]
      : [],
  );
  const composingRef = useRef(false);
  const lastActivityRef = useRef<number | null>(null);
  const focusLostAtRef = useRef<number | null>(null);
  const startedAtRef = useRef<number | null>(active ? performance.now() : null);
  const elapsedBeforeWindowRef = useRef(0);
  const activeOptionRef = useRef(active);
  activeOptionRef.current = active;
  const timingActiveRef = useRef(active);
  const sessionKeyRef = useRef(options.sessionKey);
  const committedTextRef = useRef(text);
  const sourceHintRef = useRef<FragmentSource | null>(null);
  const textRef = useRef(text);
  textRef.current = text;

  const pushPauseIfNeeded = useCallback((now: number) => {
    const last = lastActivityRef.current;
    if (last != null) {
      const gap = now - last;
      if (gap > PAUSE_GRACE_MS) {
        timelineRef.current.push({ kind: "pause", ms: Math.round(gap) });
      }
    }
    lastActivityRef.current = now;
  }, []);

  const observe = useCallback(
    (next: string, source: FragmentSource) => {
      const edit = observedEdit(committedTextRef.current, next, source);
      if (edit) {
        pushPauseIfNeeded(performance.now());
        timelineRef.current.push(edit);
        committedTextRef.current = next;
      }
      textRef.current = next;
      setTextState(next);
    },
    [pushPauseIfNeeded],
  );

  useEffect(() => {
    const el = textareaRef.current;
    if (!el || !active) return;

    const onBeforeInput = (event: InputEvent) => {
      // Intent/source only. The browser may cancel or replace this operation.
      if (!event.defaultPrevented)
        sourceHintRef.current = inputSource(event.inputType);
    };

    const onInput = (event: Event) => {
      const input = event as InputEvent;
      const target = event.target as HTMLTextAreaElement;
      if (
        input.isComposing ||
        (composingRef.current && input.inputType === "insertCompositionText")
      ) {
        // Some Android keyboards omit compositionstart. A late duplicate of
        // the already committed value must not reopen an unfinished candidate.
        if (target.value !== committedTextRef.current)
          composingRef.current = true;
        textRef.current = target.value;
        setTextState(target.value);
        return;
      }
      if (composingRef.current) {
        // A non-composing input is a commit even when compositionend is omitted.
        composingRef.current = false;
        observe(target.value, "typed");
        sourceHintRef.current = null;
        return;
      }
      observe(
        target.value,
        input.inputType
          ? inputSource(input.inputType)
          : (sourceHintRef.current ?? "unverified"),
      );
      sourceHintRef.current = null;
    };

    const onCompositionStart = () => {
      composingRef.current = true;
      sourceHintRef.current = null;
      pushPauseIfNeeded(performance.now());
      timelineRef.current.push({ kind: "composition" });
    };

    const onCompositionUpdate = () => {
      timelineRef.current.push({ kind: "composition" });
    };

    const onCompositionEnd = (event: CompositionEvent) => {
      composingRef.current = false;
      const target = event.target as HTMLTextAreaElement;
      // event.data is not an edit: OEMs may repeat it or send the entire candidate.
      observe(target.value, "typed");
      sourceHintRef.current = "typed";
    };

    const onPaste = () => {
      sourceHintRef.current = "pasted";
    };
    const onDrop = () => {
      sourceHintRef.current = "pasted";
    };

    const markFocusLost = () => {
      if (focusLostAtRef.current == null)
        focusLostAtRef.current = performance.now();
    };

    const markFocusReturn = () => {
      const lostAt = focusLostAtRef.current;
      if (lostAt == null) return;
      const ms = Math.round(performance.now() - lostAt);
      focusLostAtRef.current = null;
      if (ms > 0) timelineRef.current.push({ kind: "focusLoss", ms });
      lastActivityRef.current = performance.now();
    };

    const onFocus = () => markFocusReturn();
    const onBlur = () => {
      markFocusLost();
      sourceHintRef.current = null;
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") markFocusLost();
      else markFocusReturn();
    };

    el.addEventListener("beforeinput", onBeforeInput);
    el.addEventListener("input", onInput);
    el.addEventListener("compositionstart", onCompositionStart);
    el.addEventListener("compositionupdate", onCompositionUpdate);
    el.addEventListener("compositionend", onCompositionEnd);
    el.addEventListener("paste", onPaste);
    el.addEventListener("drop", onDrop);
    el.addEventListener("focus", onFocus);
    el.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      el.removeEventListener("beforeinput", onBeforeInput);
      el.removeEventListener("input", onInput);
      el.removeEventListener("compositionstart", onCompositionStart);
      el.removeEventListener("compositionupdate", onCompositionUpdate);
      el.removeEventListener("compositionend", onCompositionEnd);
      el.removeEventListener("paste", onPaste);
      el.removeEventListener("drop", onDrop);
      el.removeEventListener("focus", onFocus);
      el.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibility);
    };
    // Match and cross-exam fields can mount after an async snapshot arrives.
    // Rebind after render so a previously absent or replaced textarea is covered.
  });

  // Programmatic text is not evidence of typing. Restored events use restore().
  const setText = useCallback(
    (next: string) => observe(next, "unverified"),
    [observe],
  );

  const buildCommit = useCallback(
    (clear: boolean): TelemetryCommit => {
      const events = timelineRef.current.slice();
      const elapsed = Math.round(
        elapsedBeforeWindowRef.current +
          (startedAtRef.current === null
            ? 0
            : performance.now() - startedAtRef.current),
      );
      const round = fold(events, elapsed);
      applyWordCap(round, wordCap);
      const claimed = toClaimed(round.telemetry);
      // Candidate/preedit text is displayed but is not a committed input event.
      const payload = {
        text: composingRef.current ? committedTextRef.current : textRef.current,
        events,
        claimed,
      };
      if (clear) {
        timelineRef.current = [];
        committedTextRef.current = "";
        textRef.current = "";
        setTextState("");
        elapsedBeforeWindowRef.current = 0;
        startedAtRef.current = activeOptionRef.current
          ? performance.now()
          : null;
        lastActivityRef.current = null;
      }
      return payload;
    },
    [wordCap],
  );

  const snapshot = useCallback(() => buildCommit(false), [buildCommit]);
  const commit = useCallback(() => buildCommit(true), [buildCommit]);

  const reset = useCallback(() => {
    timelineRef.current = [];
    committedTextRef.current = "";
    composingRef.current = false;
    lastActivityRef.current = null;
    focusLostAtRef.current = null;
    sourceHintRef.current = null;
    elapsedBeforeWindowRef.current = 0;
    startedAtRef.current = activeOptionRef.current ? performance.now() : null;
    setTextState("");
    textRef.current = "";
  }, []);

  const restore = useCallback((saved: TelemetryCommit) => {
    timelineRef.current = saved.events.slice();
    committedTextRef.current = saved.text;
    elapsedBeforeWindowRef.current = saved.claimed.inputElapsedMs;
    startedAtRef.current = activeOptionRef.current ? performance.now() : null;
    lastActivityRef.current = null;
    focusLostAtRef.current = null;
    composingRef.current = false;
    sourceHintRef.current = null;
    setTextState(saved.text);
    textRef.current = saved.text;
  }, []);

  useEffect(() => {
    if (sessionKeyRef.current !== options.sessionKey) {
      sessionKeyRef.current = options.sessionKey;
      reset();
    }
  }, [options.sessionKey, reset]);

  useEffect(() => {
    if (timingActiveRef.current === active) return;
    const now = performance.now();
    if (active) {
      startedAtRef.current = now;
      if (lastActivityRef.current !== null) lastActivityRef.current = now;
    } else {
      if (startedAtRef.current !== null)
        elapsedBeforeWindowRef.current += now - startedAtRef.current;
      startedAtRef.current = null;
    }
    focusLostAtRef.current = null;
    timingActiveRef.current = active;
  }, [active]);

  return {
    text,
    setText,
    textareaRef,
    bindTextareaProps: {
      ref: textareaRef,
      value: text,
      onChange: (event) => {
        // Native input already observed the edit. Fallback has no source proof.
        if (composingRef.current) {
          textRef.current = event.target.value;
          setTextState(event.target.value);
        } else if (activeOptionRef.current) {
          observe(event.target.value, sourceHintRef.current ?? "unverified");
          sourceHintRef.current = null;
        }
      },
    },
    snapshot,
    commit,
    reset,
    restore,
    isComposing: () => composingRef.current,
  };
}
