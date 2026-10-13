import { z } from "zod";

const schemaVersion = (value: string) => z.literal(value);

const identifier = z
  .string()
  .regex(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);

export const slotIdV1 = z.enum([
  "AFF_1",
  "AFF_2",
  "AFF_3",
  "AFF_4",
  "NEG_1",
  "NEG_2",
  "NEG_3",
  "NEG_4",
]);

export const stageKindV1 = z.enum([
  "opening",
  "cross_exam",
  "free_debate",
  "summary",
  "custom",
]);

export const clockOwnerV1 = z.enum([
  "individual",
  "team",
  "questioner",
  "respondent",
  "shared",
]);

export const stageDefinitionV1 = z.object({
  id: z.string().min(1),
  name: z.string(),
  kind: stageKindV1,
  speakers: z.array(slotIdV1).min(1),
  speakerDurations: z.array(z.number().int().nonnegative()),
  clockOwner: clockOwnerV1,
  durationSeconds: z.number().int().positive(),
  earlyEndAllowed: z.boolean(),
  followUpAllowed: z.boolean(),
  interruptible: z.boolean(),
  interruptLimit: z.number().int().nonnegative(),
  questioner: slotIdV1.nullable(),
  respondent: slotIdV1.nullable(),
  eligibleQuestioners: z.array(slotIdV1).optional(),
  eligibleRespondents: z.array(slotIdV1).optional(),
  initialQuestioningSide: z
    .enum(["affirmative", "negative"])
    .nullable()
    .optional(),
  maxCrossExamRounds: z.number().int().positive().nullable().optional(),
  weight: z.number().int().nonnegative(),
  repeat: z.number().int().positive(),
});

export const rulesetV1 = z.object({
  schemaVersion: schemaVersion("ruleset.v1"),
  rulesetName: z.string(),
  sourceText: z.string(),
  affirmativeDuties: z.tuple([z.string(), z.string(), z.string(), z.string()]),
  negativeDuties: z.tuple([z.string(), z.string(), z.string(), z.string()]),
  stages: z.array(stageDefinitionV1).min(1),
});

export const matchSetupV1 = z.object({
  schemaVersion: schemaVersion("match-setup.v1"),
  mode: z.enum(["training", "scoring"]),
  topic: z.string(),
  materials: z.string(),
  speechRatePerMinute: z.number().int().positive(),
  ruleset: rulesetV1,
});

export const debaterAssignmentV1 = z.object({
  schemaVersion: schemaVersion("debater-assignment.v1"),
  slot: slotIdV1,
  controller: z.enum(["human", "ai"]),
});

export const matchEventV1 = z.object({
  schemaVersion: schemaVersion("match-event.v1"),
  id: identifier,
  sequence: z.number().int().nonnegative(),
  kind: z.enum([
    "state_changed",
    "speech_submitted",
    "ai_wait",
    "interrupt_rejected",
    "takeover",
  ]),
  slot: slotIdV1.nullable(),
  summary: z.string(),
});

export const transcriptEntryV1 = z.object({
  schemaVersion: schemaVersion("transcript-entry.v1"),
  id: identifier,
  eventId: identifier,
  controller: z.enum(["human", "ai"]).nullable().optional(),
  slot: slotIdV1,
  fullText: z.string(),
  effectiveText: z.string(),
  effectiveUnits: z.number().int().nonnegative(),
  wordCapAtSubmission: z.number().int().nonnegative().optional(),
  overflowText: z.string(),
  crossExamExchangeId: identifier.nullable(),
  responseTargetText: z.string(),
  crossExamTurnKind: z.enum(["question", "answer"]).nullable().optional(),
  crossExamRoundNumber: z.number().int().nonnegative().optional(),
});

export const crossExamExchangeV1 = z.object({
  schemaVersion: schemaVersion("cross-exam-exchange.v1"),
  id: identifier,
  eventId: identifier,
  questioner: slotIdV1,
  respondent: slotIdV1,
  questionText: z.string().min(1),
  answerTranscriptId: identifier.nullable(),
  status: z.enum([
    "selected",
    "question_submitted",
    "answered",
    "interrupted",
    "timed_out",
  ]),
  displayedAnswerUnits: z.number().int().nonnegative(),
  interruptedReason: z.string(),
  roundNumber: z.number().int().nonnegative().optional(),
  questioningSide: z.enum(["affirmative", "negative"]).nullable().optional(),
  questionerController: z.enum(["human", "ai"]).nullable().optional(),
  respondentController: z.enum(["human", "ai"]).nullable().optional(),
  previousExchangeId: identifier.nullable().optional(),
  questionTranscriptId: identifier.nullable().optional(),
  questionInputMs: z.number().int().nonnegative().optional(),
  answerInputMs: z.number().int().nonnegative().optional(),
  selectionSummary: z.string().optional(),
});

export const crossExamSessionV1 = z.object({
  schemaVersion: schemaVersion("cross-exam-session.v1"),
  eventId: identifier,
  phase: z.enum([
    "selecting",
    "question_pending",
    "answer_pending",
    "revealing",
    "between_exchanges",
    "completed",
  ]),
  nextSide: z.enum(["affirmative", "negative"]),
  totalMs: z.number().int().positive(),
  usedMs: z.number().int().nonnegative(),
  totalUnits: z.number().int().nonnegative().optional(),
  usedUnits: z.number().int().nonnegative().optional(),
  roundNumber: z.number().int().positive(),
  activeExchangeId: identifier.nullable(),
  exchangeIds: z.array(identifier),
  endReason: z
    .enum(["timeout", "user_ended", "round_limit", "legacy_single_round"])
    .nullable(),
  revision: z.number().int().nonnegative(),
});

export const turnMetricSummaryV1 = z.object({
  schemaVersion: schemaVersion("turn-metric-summary.v1"),
  turnId: identifier,
  forwardTypedUnits: z.number().int().nonnegative(),
  effectivePositiveUnits: z.number().int().nonnegative(),
  reverseEditedUnits: z.number().int().nonnegative(),
  pasteUnits: z.number().int().nonnegative(),
  pauseMs: z.number().int().nonnegative(),
});

export const stageEvaluationV1 = z.object({
  schemaVersion: schemaVersion("stage-evaluation.v1"),
  stageId: z.string(),
  transcriptIds: z.array(identifier),
  summary: z.string(),
});

export const matchReportV1 = z.object({
  schemaVersion: schemaVersion("match-report.v1"),
  matchId: identifier,
  formulaVersion: z.string(),
  standard: z.boolean(),
});

export const contractBundleV1 = z.object({
  ruleset: rulesetV1,
  matchSetup: matchSetupV1,
  assignment: debaterAssignmentV1,
  matchEvent: matchEventV1,
  transcriptEntry: transcriptEntryV1,
  crossExamExchange: crossExamExchangeV1,
  turnMetricSummary: turnMetricSummaryV1,
  stageEvaluation: stageEvaluationV1,
  matchReport: matchReportV1,
});
