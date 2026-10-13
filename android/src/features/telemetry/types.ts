/** Source events matching Rust `InputEvent` / fragment sources. */

export type FragmentSource = "typed" | "pasted" | "overflow" | "unverified";

export type SourceEvent =
  | { kind: "composition" }
  | { kind: "insert"; text: string; source: FragmentSource }
  | { kind: "delete"; units: number }
  // DOM selection offsets are UTF-16, not effective debate units.
  | {
      kind: "replace";
      start: number;
      end: number;
      text: string;
      source: FragmentSource;
    }
  | { kind: "pause"; ms: number }
  | { kind: "focusLoss"; ms: number };

export type Fragment = {
  text: string;
  source: FragmentSource;
};

export type ProvenanceSpan = {
  startUnits: number;
  endUnits: number;
  source: FragmentSource;
};

export type TurnTelemetry = {
  forwardTypedUnits: number;
  effectivePositiveUnits: number;
  reverseEditedUnits: number;
  reverseEditActions: number;
  pasteUnits: number;
  pauseMs: number;
  focusLossMs: number;
  inputElapsedMs: number;
  fragments: Fragment[];
  provenance: ProvenanceSpan[];
  finalText: string;
  unverifiedInput?: boolean;
};

export type ClaimedMetrics = {
  inputElapsedMs: number;
  forwardTypedUnits: number;
  effectivePositiveUnits: number;
  reverseUnits: number;
  reverseActions: number;
  pauseMs: number;
  focusLossMs: number;
  pasteUnits: number;
};

export type InputRound = {
  telemetry: TurnTelemetry;
  timeline: SourceEvent[];
};

export const PAUSE_GRACE_MS = 2_500;
