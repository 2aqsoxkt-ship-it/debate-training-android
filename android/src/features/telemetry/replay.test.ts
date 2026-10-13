import { describe, expect, it } from "vitest";
import {
  ALL_FIXTURES,
  backspaceHan,
  compositionNotDoubleCount,
  focusLossReturn,
  overflowBeyondCap,
  pasteThenDeleteHalf,
  pausesInitialMidSubmit,
  pinyinComposition,
  selectReplaceThree,
} from "./fixtures";
import { applyWordCap, commitRound, fold, timelineIsEmpty } from "./fold";
import { assertReplayStable, replayFixture } from "./replay";
import { countEffectiveUnits, splitEffective } from "./units";
import mobileEdits from "../../../tests/fixtures/telemetry/mobile-edits.json";
import type { SourceEvent } from "./types";

describe("input telemetry replay fixtures", () => {
  for (const fixture of mobileEdits) {
    it(`手机共享回放夹具: ${fixture.name}`, () => {
      const events = fixture.events as SourceEvent[];
      const round = fold(events, fixture.claimed.inputElapsedMs);
      expect(round.telemetry.finalText).toBe(fixture.text);
      expect(replayFixture(events)).toEqual({
        ...fixture.claimed,
        inputElapsedMs: 0,
      });
    });
  }
  it("1. pinyin composition commits once", () => {
    const claimed = replayFixture(pinyinComposition.events);
    expect(claimed.forwardTypedUnits).toBe(4);
    expect(claimed.effectivePositiveUnits).toBe(4);
  });

  it("2. backspace deletes one Han unit", () => {
    const round = fold(backspaceHan.events);
    expect(round.telemetry.reverseEditedUnits).toBe(1);
    expect(round.telemetry.reverseEditActions).toBe(1);
    expect(round.telemetry.effectivePositiveUnits).toBe(1);
    expect(round.telemetry.finalText).toBe("你");
  });

  it("3. select-replace three Han", () => {
    const round = fold(selectReplaceThree.events);
    expect(round.telemetry.reverseEditActions).toBe(1);
    expect(round.telemetry.reverseEditedUnits).toBe(3);
    expect(round.telemetry.finalText).toBe("甲替换词");
  });

  it("4. paste then delete half keeps paste provenance", () => {
    const claimed = replayFixture(pasteThenDeleteHalf.events);
    expect(claimed.pasteUnits).toBe(2);
    expect(claimed.effectivePositiveUnits).toBe(0);
    expect(fold(pasteThenDeleteHalf.events).telemetry.finalText).toBe("一二");
  });

  it("5. initial / mid / submit pauses keep excess only", () => {
    expect(replayFixture(pausesInitialMidSubmit.events).pauseMs).toBe(4_500);
  });

  it("6. focus loss is tracked on return", () => {
    expect(replayFixture(focusLossReturn.events).focusLossMs).toBe(1_200);
  });

  it("7. overflow beyond cap marks provenance after fold", () => {
    const round = fold(overflowBeyondCap.events);
    applyWordCap(round, overflowBeyondCap.wordCap!);
    expect(
      round.telemetry.provenance.some((s) => s.source === "overflow"),
    ).toBe(true);
    const [prefix] = splitEffective(round.telemetry.finalText, 4);
    expect(countEffectiveUnits(prefix)).toBe(4);
    expect(round.telemetry.forwardTypedUnits).toBe(6);
  });

  it("8. composition updates do not double-count", () => {
    const claimed = replayFixture(compositionNotDoubleCount.events);
    expect(claimed.forwardTypedUnits).toBe(2);
    expect(claimed.reverseActions).toBe(0);
  });

  it("9. same fixture twice yields identical stats", () => {
    for (const fixture of ALL_FIXTURES) {
      const a = replayFixture(fixture.events);
      const b = replayFixture(fixture.events);
      expect(a).toEqual(b);
      assertReplayStable(fixture.events);
    }
  });

  it("10. commitRound clears the timeline", () => {
    const round = fold(pinyinComposition.events);
    expect(round.timeline.length).toBeGreaterThan(0);
    const claimed = commitRound(round);
    expect(timelineIsEmpty(round)).toBe(true);
    expect(claimed.forwardTypedUnits).toBe(4);
  });
});
