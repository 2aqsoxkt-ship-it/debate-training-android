import { countEffectiveUnits, splitEffective } from "./units";
import type {
  ClaimedMetrics,
  Fragment,
  FragmentSource,
  InputRound,
  ProvenanceSpan,
  SourceEvent,
  TurnTelemetry,
} from "./types";
import { PAUSE_GRACE_MS } from "./types";

export function excessPause(intervalMs: number): number {
  return Math.max(0, intervalMs - PAUSE_GRACE_MS);
}

export function toClaimed(telemetry: TurnTelemetry): ClaimedMetrics {
  return {
    inputElapsedMs: telemetry.inputElapsedMs,
    forwardTypedUnits: telemetry.forwardTypedUnits,
    effectivePositiveUnits: telemetry.effectivePositiveUnits,
    reverseUnits: telemetry.reverseEditedUnits,
    reverseActions: telemetry.reverseEditActions,
    pauseMs: telemetry.pauseMs,
    focusLossMs: telemetry.focusLossMs,
    pasteUnits: telemetry.pasteUnits,
  };
}

export function fold(events: SourceEvent[], inputElapsedMs = 0): InputRound {
  const fragments: Fragment[] = [];
  let forward = 0;
  let reverseUnits = 0;
  let reverseActions = 0;
  let pauseMs = 0;
  let focusLossMs = 0;
  let unverifiedInput = false;

  for (const event of events) {
    switch (event.kind) {
      case "composition":
        break;
      case "insert": {
        unverifiedInput ||= event.source === "unverified";
        const units = countEffectiveUnits(event.text);
        if (event.source === "typed") forward += units;
        fragments.push({ text: event.text, source: event.source });
        break;
      }
      case "replace": {
        const removed = replaceRange(
          fragments,
          event.start,
          event.end,
          event.text,
          event.source,
        );
        if (removed !== null) {
          if (event.source === "typed")
            forward += countEffectiveUnits(event.text);
          if (event.end > event.start) {
            reverseUnits += removed;
            reverseActions += 1;
          }
        }
        unverifiedInput ||= event.source === "unverified";
        break;
      }
      case "delete": {
        const removed = removeUnits(fragments, event.units);
        reverseUnits += removed;
        reverseActions += 1;
        break;
      }
      case "pause":
        pauseMs += excessPause(event.ms);
        break;
      case "focusLoss":
        focusLossMs += event.ms;
        break;
    }
  }

  const pasteUnits = sumSourceUnits(fragments, "pasted");
  const effectivePositiveUnits = sumSourceUnits(fragments, "typed");
  const finalText = fragments.map((f) => f.text).join("");

  return {
    telemetry: {
      forwardTypedUnits: forward,
      effectivePositiveUnits,
      reverseEditedUnits: reverseUnits,
      reverseEditActions: reverseActions,
      pasteUnits,
      pauseMs,
      focusLossMs,
      inputElapsedMs,
      fragments,
      provenance: provenanceSpans(fragments),
      finalText,
      ...(unverifiedInput ? { unverifiedInput: true } : {}),
    },
    timeline: events.slice(),
  };
}

/** Preserve the sources on both sides of a real DOM edit, including punctuation. */
function replaceRange(
  fragments: Fragment[],
  start: number,
  end: number,
  text: string,
  source: FragmentSource,
): number | null {
  const original = fragments.map((fragment) => fragment.text).join("");
  const boundaries = new Set([0]);
  let offset = 0;
  for (const character of original) {
    offset += character.length;
    boundaries.add(offset);
  }
  if (start > end || !boundaries.has(start) || !boundaries.has(end))
    return null;
  const before: Fragment[] = [];
  const after: Fragment[] = [];
  offset = 0;
  for (const fragment of fragments) {
    const length = fragment.text.length;
    const head = fragment.text.slice(
      0,
      Math.max(0, Math.min(length, start - offset)),
    );
    const tail = fragment.text.slice(
      Math.max(0, Math.min(length, end - offset)),
    );
    if (head) before.push({ text: head, source: fragment.source });
    if (tail) after.push({ text: tail, source: fragment.source });
    offset += length;
  }
  const next = [...before, ...(text ? [{ text, source }] : []), ...after];
  fragments.length = 0;
  for (const fragment of next) {
    const last = fragments[fragments.length - 1];
    if (last?.source === fragment.source) last.text += fragment.text;
    else fragments.push({ ...fragment });
  }
  return countEffectiveUnits(original.slice(start, end));
}

export function applyWordCap(round: InputRound, wordCap: number): void {
  const [, overflow] = splitEffective(round.telemetry.finalText, wordCap);
  if (!overflow) return;

  const kept: Fragment[] = [];
  let used = 0;
  for (const fragment of round.telemetry.fragments) {
    const units = countEffectiveUnits(fragment.text);
    if (used >= wordCap) {
      kept.push({ text: fragment.text, source: "overflow" });
      continue;
    }
    if (used + units <= wordCap) {
      kept.push({ ...fragment });
      used += units;
    } else {
      const keep = wordCap - used;
      const [head, tail] = splitEffective(fragment.text, keep);
      if (head) kept.push({ text: head, source: fragment.source });
      if (tail) kept.push({ text: tail, source: "overflow" });
      used = wordCap;
    }
  }

  round.telemetry.fragments = kept;
  round.telemetry.finalText = kept.map((f) => f.text).join("");
  round.telemetry.effectivePositiveUnits = sumSourceUnits(kept, "typed");
  round.telemetry.pasteUnits = sumSourceUnits(kept, "pasted");
  round.telemetry.provenance = provenanceSpans(kept);
}

/** Clears the round timeline; returns claimed metrics from folded telemetry. */
export function commitRound(round: InputRound): ClaimedMetrics {
  const claimed = toClaimed(round.telemetry);
  round.timeline.length = 0;
  return claimed;
}

export function timelineIsEmpty(round: InputRound): boolean {
  return round.timeline.length === 0;
}

function sumSourceUnits(fragments: Fragment[], source: FragmentSource): number {
  return fragments
    .filter((f) => f.source === source)
    .reduce((sum, f) => sum + countEffectiveUnits(f.text), 0);
}

function provenanceSpans(fragments: Fragment[]): ProvenanceSpan[] {
  let cursor = 0;
  const spans: ProvenanceSpan[] = [];
  for (const fragment of fragments) {
    const units = countEffectiveUnits(fragment.text);
    if (units === 0) continue;
    spans.push({
      startUnits: cursor,
      endUnits: cursor + units,
      source: fragment.source,
    });
    cursor += units;
  }
  return spans;
}

function removeUnits(fragments: Fragment[], units: number): number {
  let remaining = units;
  let removed = 0;
  while (remaining > 0) {
    const fragment = fragments[fragments.length - 1];
    if (!fragment) break;
    const available = countEffectiveUnits(fragment.text);
    if (available === 0) {
      fragments.pop();
      continue;
    }
    if (available <= remaining) {
      remaining -= available;
      removed += available;
      fragments.pop();
    } else {
      const keep = available - remaining;
      const [prefix] = splitEffective(fragment.text, keep);
      fragment.text = prefix;
      removed += remaining;
      remaining = 0;
    }
  }
  return removed;
}
