import { describe, expect, it } from "vitest";
import bundle from "../../../tests/fixtures/contracts/bundle-v1.json";
import { contractBundleV1, crossExamSessionV1 } from "./contracts";

describe("领域契约", () => {
  it("用同一份夹具校验 Rust 与 Zod 镜像", () => {
    expect(contractBundleV1.parse(bundle)).toEqual(bundle);
  });
  it("保留提交当时的控制者，不由后续接管重判历史", () => {
    const withController = {
      ...bundle,
      transcriptEntry: { ...bundle.transcriptEntry, controller: "ai" },
    };
    expect(contractBundleV1.parse(withController)).toEqual(withController);
  });
  it("保留实际提交字数上限，不丢弃新增字段", () => {
    const withWordBudget = {
      ...bundle,
      transcriptEntry: { ...bundle.transcriptEntry, wordCapAtSubmission: 3 },
    };
    expect(contractBundleV1.parse(withWordBudget)).toEqual(withWordBudget);
  });
  it("保留质询会话共享字池", () => {
    const session = {
      schemaVersion: "cross-exam-session.v1",
      eventId: bundle.crossExamExchange.eventId,
      phase: "question_pending",
      nextSide: "affirmative",
      totalMs: 180000,
      usedMs: 0,
      totalUnits: 1003,
      usedUnits: 7,
      roundNumber: 1,
      activeExchangeId: null,
      exchangeIds: [],
      endReason: null,
      revision: 1,
    };
    expect(crossExamSessionV1.parse(session)).toEqual(session);
  });
});
