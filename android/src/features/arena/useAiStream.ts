import { useEffect, useRef, useState } from "react";

export type AiStreamEvent = {
  kind: string;
  requestId?: string;
  matchId?: string;
  eventId?: string;
  text?: string;
  label?: string;
  category?: string;
  message?: string;
};

/** Pure filter: bind on `started`, drop late events from other requestIds. */
export function acceptsStreamEvent(
  payload: AiStreamEvent,
  activeEventId: string,
  boundRequestId: string,
): { accept: boolean; nextBoundRequestId: string } {
  if (activeEventId && payload.eventId && payload.eventId !== activeEventId) {
    return { accept: false, nextBoundRequestId: boundRequestId };
  }
  if (payload.kind === "started" && payload.requestId) {
    return { accept: true, nextBoundRequestId: payload.requestId };
  }
  if (
    boundRequestId &&
    payload.requestId &&
    payload.requestId !== boundRequestId
  ) {
    return { accept: false, nextBoundRequestId: boundRequestId };
  }
  return { accept: true, nextBoundRequestId: boundRequestId };
}

export function useAiStream(activeEventId: string, enabled = true) {
  const [draft, setDraft] = useState("");
  const [status, setStatus] = useState("");
  const [requestId, setRequestId] = useState("");
  const boundRequestId = useRef("");
  const startedAt = useRef<number | null>(null);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);

  useEffect(() => {
    boundRequestId.current = "";
    setDraft("");
    setStatus("");
    setRequestId("");
    startedAt.current = null;
    setElapsedSeconds(0);
    if (!enabled) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const stop = await listen<AiStreamEvent>("ai-stream", (event) => {
          if (cancelled) return;
          const payload = event.payload;
          const decision = acceptsStreamEvent(
            payload,
            activeEventId,
            boundRequestId.current,
          );
          if (!decision.accept) {
            return;
          }
          boundRequestId.current = decision.nextBoundRequestId;
          if (payload.requestId) {
            setRequestId(payload.requestId);
          }
          switch (payload.kind) {
            case "started":
              startedAt.current = performance.now();
              setDraft("");
              setStatus("正在思考");
              break;
            case "status":
              setStatus(payload.label ?? "正在思考");
              break;
            case "text_delta":
              setStatus("接收候选稿");
              setDraft((previous) => previous + (payload.text ?? ""));
              break;
            case "completed":
              startedAt.current = null;
              setDraft(payload.text ?? "");
              setStatus("");
              break;
            case "failed":
              startedAt.current = null;
              setStatus(payload.message ?? "模型失败");
              break;
            case "cancelled":
              startedAt.current = null;
              setStatus("已取消");
              setDraft("");
              break;
            default:
              break;
          }
        });
        if (cancelled) {
          stop();
        } else {
          unlisten = stop;
        }
      } catch {
        // Browser preview has no Tauri events.
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [activeEventId, enabled]);

  useEffect(() => {
    if (!enabled) return;
    const timer = window.setInterval(() => {
      if (startedAt.current !== null)
        setElapsedSeconds(
          Math.floor((performance.now() - startedAt.current) / 1000),
        );
    }, 1000);
    return () => window.clearInterval(timer);
  }, [enabled]);
  return { draft, status, requestId, elapsedSeconds, setDraft, setStatus };
}
