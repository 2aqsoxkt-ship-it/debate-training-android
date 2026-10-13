import { fold, toClaimed } from "./fold";
import type { ClaimedMetrics, SourceEvent, TurnTelemetry } from "./types";

export function replayFixture(events: SourceEvent[]): ClaimedMetrics {
  return toClaimed(fold(events).telemetry);
}

export function assertReplayStable(events: SourceEvent[]): TurnTelemetry {
  const first = fold(events).telemetry;
  const second = fold(events).telemetry;
  if (!telemetryEqual(first, second)) {
    throw new Error("replay fold is not stable");
  }
  return first;
}

function telemetryEqual(a: TurnTelemetry, b: TurnTelemetry): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
