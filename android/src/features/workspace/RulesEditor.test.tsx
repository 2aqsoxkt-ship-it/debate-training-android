import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RuleDraft } from "../session/types";
import { RulesEditor } from "./RulesEditor";

const draft: RuleDraft = {
  schemaVersion: "ruleset.v2",
  planVersion: "match-plan.v3",
  rulesetName: "测试赛制",
  sourceText: "正方一辩立论三分钟",
  affirmativeDuties: ["立论", "攻辩", "攻辩", "总结"],
  negativeDuties: ["立论", "攻辩", "攻辩", "总结"],
  migrationReviewRequired: false,
  clockPools: [
    {
      id: "affirmative-main",
      label: "正方主时间",
      side: "affirmative",
      kind: "main",
      initialSeconds: 1080,
      warningSeconds: 30,
      expirySignalCount: 2,
      allowWaiveRemaining: false,
    },
    {
      id: "negative-main",
      label: "反方主时间",
      side: "negative",
      kind: "main",
      initialSeconds: 1080,
      warningSeconds: 30,
      expirySignalCount: 2,
      allowWaiveRemaining: false,
    },
    {
      id: "affirmative-free",
      label: "正方自由辩",
      side: "affirmative",
      kind: "free_debate",
      initialSeconds: 240,
      warningSeconds: 30,
      expirySignalCount: 2,
      allowWaiveRemaining: true,
    },
    {
      id: "negative-free",
      label: "反方自由辩",
      side: "negative",
      kind: "free_debate",
      initialSeconds: 240,
      warningSeconds: 30,
      expirySignalCount: 2,
      allowWaiveRemaining: true,
    },
  ],
  sourceClauses: [
    { id: "clause-1", text: "正方一辩立论三分钟", start: 0, end: 9 },
  ],
  coverageEntries: [
    {
      clauseId: "clause-1",
      status: "mapped",
      stageIds: ["opening"],
      note: "已核对",
    },
  ],
  stages: [
    {
      id: "opening",
      displayName: "正方一辩立论",
      semanticType: "opening_statement",
      participationMode: "single",
      turnMode: "continuous_speech",
      clockMode: "individual",
      interruptPolicy: "forbidden",
      fixedSpeakers: ["AFF_1"],
      affirmativeCandidates: [],
      negativeCandidates: [],
      questioner: null,
      respondent: null,
      openingSide: null,
      durationSeconds: 180,
      affirmativeSeconds: null,
      negativeSeconds: null,
      questionSeconds: null,
      answerSeconds: null,
      turnSeconds: null,
      followUpAllowed: false,
      respondentMayCounterQuestion: false,
      maxRounds: null,
      aiObjective: "建立本方立场",
      scoringFocus: ["切题性"],
      sourceEvidence: [{ quote: "正方一辩立论三分钟", start: 0, end: 9 }],
      confidence: 100,
      unresolvedFields: [],
    },
  ],
};

describe("RulesEditor", () => {
  it("does not treat a restored but equal buffer as an unsaved ruleset", () => {
    render(
      <RulesEditor
        draft={draft}
        editedDraft={structuredClone(draft)}
        issues={[]}
        disabled={false}
        onReplace={vi.fn()}
        onDraftChange={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "保存赛程修改" })).toBeDisabled();
  });
  it("edits word quotas as exact integers without rounding through seconds", () => {
    const onReplace = vi.fn();
    render(
      <RulesEditor
        draft={draft}
        issues={[]}
        disabled={false}
        meteringMode="words"
        speechRatePerMinute={240}
        onReplace={onReplace}
      />,
    );
    const main = screen.getByLabelText("正方主额度（字）");
    expect(main).toHaveValue(4320);
    expect(screen.getByLabelText("正方自由辩（字）")).toHaveValue(960);
    expect(
      screen.getByRole("button", { name: /正方一辩立论/ }),
    ).toHaveTextContent("单轮上限 720字");
    fireEvent.change(main, { target: { value: "1003" } });
    fireEvent.click(screen.getByRole("button", { name: "保存赛程修改" }));
    expect(onReplace.mock.calls[0][0].clockPools[0]).toMatchObject({
      unitBudget: 1003,
      initialSeconds: 1080,
    });
  });
  it("keeps stage details collapsed until the user opens the card", () => {
    render(
      <RulesEditor
        draft={draft}
        issues={[]}
        disabled={false}
        onReplace={() => undefined}
      />,
    );

    expect(screen.queryByLabelText("环节名称")).toBeNull();
    const toggle = screen.getByRole("button", { name: /正方一辩立论/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(screen.getByLabelText("环节名称")).toHaveValue("正方一辩立论");
  });

  it("keeps edits local until the user explicitly saves the whole draft", () => {
    const onReplace = vi.fn();
    render(
      <RulesEditor
        draft={draft}
        issues={[]}
        disabled={false}
        onReplace={onReplace}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "添加环节" }));
    expect(onReplace).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "保存赛程修改" }));
    expect(onReplace).toHaveBeenCalledTimes(1);
    expect(onReplace.mock.calls[0][0].stages).toHaveLength(2);
  });
});
