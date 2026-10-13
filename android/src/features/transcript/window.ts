import type { SourceEvent } from "../telemetry/types";

export type { SourceEvent } from "../telemetry/types";
export type { ClaimedMetrics, FragmentSource } from "../telemetry/types";

export function visibleWindow(total: number, start: number, size: number) {
  const from = Math.max(0, Math.min(start, total));
  const to = Math.min(total, from + size);
  return { from, to, count: to - from };
}

/**
 * @deprecated Prefer `useInputTelemetry().commit()` which clears the live timeline.
 * Kept for WorkspacePage until parent wires the telemetry hook.
 */
export function finishRound(events: SourceEvent[]) {
  const snapshot = events.slice();
  events.length = 0;
  return snapshot;
}
