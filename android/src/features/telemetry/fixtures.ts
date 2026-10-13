import type { ClaimedMetrics, SourceEvent } from "./types";

export type FixtureCase = {
  name: string;
  events: SourceEvent[];
  expected: Partial<ClaimedMetrics> & { finalText?: string };
  /** Cap applied after fold in overflow fixture tests. */
  wordCap?: number;
};

export const pinyinComposition: FixtureCase = {
  name: "pinyinComposition",
  events: [
    { kind: "composition" },
    { kind: "composition" },
    { kind: "insert", text: "人工智能", source: "typed" },
  ],
  expected: {
    forwardTypedUnits: 4,
    effectivePositiveUnits: 4,
    reverseUnits: 0,
    reverseActions: 0,
  },
};

export const backspaceHan: FixtureCase = {
  name: "backspaceHan",
  events: [
    { kind: "insert", text: "你好", source: "typed" },
    { kind: "delete", units: 1 },
  ],
  expected: {
    forwardTypedUnits: 2,
    effectivePositiveUnits: 1,
    reverseUnits: 1,
    reverseActions: 1,
    finalText: "你",
  },
};

export const selectReplaceThree: FixtureCase = {
  name: "selectReplaceThree",
  events: [
    { kind: "insert", text: "甲乙丙丁", source: "typed" },
    { kind: "delete", units: 3 },
    { kind: "insert", text: "替换词", source: "typed" },
  ],
  expected: {
    reverseUnits: 3,
    reverseActions: 1,
    finalText: "甲替换词",
  },
};

export const pasteThenDeleteHalf: FixtureCase = {
  name: "pasteThenDeleteHalf",
  events: [
    { kind: "insert", text: "一二三四", source: "pasted" },
    { kind: "delete", units: 2 },
  ],
  expected: {
    pasteUnits: 2,
    effectivePositiveUnits: 0,
    forwardTypedUnits: 0,
    finalText: "一二",
  },
};

export const pausesInitialMidSubmit: FixtureCase = {
  name: "pausesInitialMidSubmit",
  events: [
    { kind: "pause", ms: 3_000 },
    { kind: "insert", text: "点", source: "typed" },
    { kind: "pause", ms: 5_000 },
    { kind: "insert", text: "评", source: "typed" },
    { kind: "pause", ms: 4_000 },
  ],
  expected: {
    // (3000-2500) + (5000-2500) + (4000-2500) = 4500
    pauseMs: 4_500,
  },
};

export const focusLossReturn: FixtureCase = {
  name: "focusLossReturn",
  events: [
    { kind: "insert", text: "开", source: "typed" },
    { kind: "focusLoss", ms: 1_200 },
    { kind: "insert", text: "始", source: "typed" },
  ],
  expected: {
    focusLossMs: 1_200,
    forwardTypedUnits: 2,
  },
};

/** Insert 6 Han; tests apply wordCap 4 then assert overflow provenance. */
export const overflowBeyondCap: FixtureCase = {
  name: "overflowBeyondCap",
  events: [{ kind: "insert", text: "一二三四五六", source: "typed" }],
  wordCap: 4,
  expected: {
    forwardTypedUnits: 6,
  },
};

export const compositionNotDoubleCount: FixtureCase = {
  name: "compositionNotDoubleCount",
  events: [
    { kind: "composition" },
    { kind: "composition" },
    { kind: "insert", text: "你好", source: "typed" },
  ],
  expected: {
    forwardTypedUnits: 2,
    effectivePositiveUnits: 2,
    reverseActions: 0,
  },
};

export const ALL_FIXTURES: FixtureCase[] = [
  pinyinComposition,
  backspaceHan,
  selectReplaceThree,
  pasteThenDeleteHalf,
  pausesInitialMidSubmit,
  focusLossReturn,
  overflowBeyondCap,
  compositionNotDoubleCount,
];
