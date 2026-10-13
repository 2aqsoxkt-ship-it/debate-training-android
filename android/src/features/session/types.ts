export type Report = {
  formulaVersion: string;
  modelId: string;
  difficulty: string;
  speechRate: number;
  personal: number;
  program: number;
  semantic: number;
  teamWinner: string;
  standard: boolean;
  nonStandardReason: string;
  length: number;
  inputControl: number;
  execution: number;
  trace: string;
  scoringStatus: string;
  lowConfidence: boolean;
  dimensions: {
    name: string;
    raw: number;
    points: number;
    transcriptId: string;
  }[];
  stageEvaluations: {
    transcriptId: string;
    strength: string;
    improvement: string;
    materialSupport: string;
    confidence: number;
  }[];
  reviewSummary: string;
  teamSummary: string;
};

export type SemanticStageType =
  | "opening_statement"
  | "case_extension"
  | "rebuttal"
  | "cross_examination"
  | "paired_debate"
  | "cross_exam_summary"
  | "free_debate"
  | "closing_summary"
  | "custom";

export type RuleStage = {
  id: string;
  displayName: string;
  semanticType: SemanticStageType;
  participationMode: "single" | "fixed_pair" | "candidate_pool" | "whole_team";
  turnMode:
    | "continuous_speech"
    | "one_way_question_answer"
    | "alternating_pair"
    | "alternating_teams";
  clockMode:
    | "individual"
    | "questioner_pool"
    | "separate_sides"
    | "team_pool"
    | "shared_stage";
  interruptPolicy:
    "forbidden" | "questioner_may_stop_answer" | "clock_expiry_only";
  fixedSpeakers: string[];
  affirmativeCandidates: string[];
  negativeCandidates: string[];
  questioner: string | null;
  respondent: string | null;
  openingSide: "affirmative" | "negative" | null;
  durationSeconds: number | null;
  affirmativeSeconds: number | null;
  negativeSeconds: number | null;
  questionSeconds: number | null;
  answerSeconds: number | null;
  turnSeconds: number | null;
  followUpAllowed: boolean;
  respondentMayCounterQuestion: boolean;
  maxRounds: number | null;
  aiObjective: string;
  scoringFocus: string[];
  sourceEvidence: { quote: string; start: number | null; end: number | null }[];
  confidence: number;
  unresolvedFields: string[];
};

export type RuleDraft = {
  schemaVersion: "ruleset.v2";
  planVersion: string;
  rulesetName: string;
  sourceText: string;
  affirmativeDuties: [string, string, string, string];
  negativeDuties: [string, string, string, string];
  stages: RuleStage[];
  clockPools: {
    id: string;
    label: string;
    side: "affirmative" | "negative";
    kind: "main" | "free_debate";
    initialSeconds: number;
    unitBudget?: number | null;
    warningSeconds: number;
    expirySignalCount: number;
    allowWaiveRemaining: boolean;
  }[];
  sourceClauses: { id: string; text: string; start: number; end: number }[];
  coverageEntries: {
    clauseId: string;
    status: "mapped" | "explanatory" | "ambiguous" | "unmapped";
    stageIds: string[];
    note: string;
  }[];
  migrationReviewRequired: boolean;
};

export type RuleIssue = {
  severity: "blocking" | "warning" | "suggestion";
  stageId: string;
  field: string;
  message: string;
};

export type Snapshot = {
  publicIssues?: {
    sourceTurnId: string;
    slot: string;
    publicClaim: string;
    responseTurnIds: string[];
    status: string;
  }[];
  matchId: string;
  profiles: { id: string; displayName: string }[];
  activeProfileId: string;
  recent: { id: string; topic: string; phase: string }[];
  phase: string;
  topic: string;
  mode: string;
  userSide: string;
  difficulty: string;
  preparationReady: boolean;
  rulesError: string;
  rulesConfirmed: boolean;
  lineupConfirmed: boolean;
  schedule: {
    id: string;
    kind: string;
    speaker: string;
    durationSeconds: number;
    wordCap: number;
    interruptible: boolean;
    controller?: string;
    started?: boolean;
  }[];
  ruleDraft?: RuleDraft | null;
  ruleIssues?: RuleIssue[];
  rulesCanConfirm?: boolean;
  cursor: number;
  meteringMode?: "words" | "time";
  speechRatePerMinute?: number;
  remainingUnits?: number;
  currentWordCap?: number;
  currentWindowId?: string;
  controlRevision?: number;
  affirmativeMainUnits?: number;
  negativeMainUnits?: number;
  affirmativeFreeUnits?: number;
  negativeFreeUnits?: number;
  remainingMs: number;
  chargedMs: number;
  aiWaitMs: number;
  transcripts: {
    controller?: "human" | "ai" | null;
    id: string;
    slot: string;
    effectiveText: string;
    overflowText: string;
    interruptedTail: string;
    respondsTo: string;
    responseTargetText: string;
  }[];
  ownStrategy: string | null;
  opponentStrategy: string | null;
  rolePackets: {
    slot: string;
    identity: string;
    duty: string;
    mainArgument: string;
    backupRebuttal: string;
    crossExamTarget: string;
    defenseFloor: string;
    summaryCallback: string;
  }[];
  message: string;
  report: Report | null;
  standard: boolean;
  currentSpeaker: string;
  currentHuman: boolean;
  currentInterruptible: boolean;
  usesLiveModel: boolean;
  affirmativeMs: number;
  negativeMs: number;
  affirmativeMainMs: number;
  negativeMainMs: number;
  floorHolder: string;
  calibrationPassage: string;
  baselineCpm: number;
  calibrationRoundsCompleted: number;
  lowConfidence: boolean;
  materials: string;
  awaitingSeat: boolean;
  seats: {
    defaultController?: string;
    index: number;
    name: string;
    duty: string;
    side: string;
    controller: string;
    speaking: boolean;
    claimable: boolean;
  }[];
  modelSettings: {
    modelId: string;
    baseUrl: string;
    reasoningLevel: string;
    connectionStatus: string;
    lastTestedAt: string;
    lastErrorCategory: string;
    hasApiKey: boolean;
  };
};

export const emptySnapshot: Snapshot = {
  matchId: "",
  profiles: [],
  activeProfileId: "",
  recent: [],
  phase: "draft_setup",
  topic: "",
  mode: "training",
  userSide: "affirmative",
  difficulty: "standard",
  preparationReady: false,
  rulesError: "",
  rulesConfirmed: false,
  lineupConfirmed: false,
  schedule: [],
  ruleDraft: null,
  ruleIssues: [],
  rulesCanConfirm: false,
  cursor: 0,
  meteringMode: "words",
  speechRatePerMinute: 240,
  remainingUnits: 0,
  currentWordCap: 0,
  affirmativeMainUnits: 4_320,
  negativeMainUnits: 4_320,
  affirmativeFreeUnits: 960,
  negativeFreeUnits: 960,
  remainingMs: 0,
  chargedMs: 0,
  aiWaitMs: 0,
  transcripts: [],
  ownStrategy: null,
  opponentStrategy: null,
  rolePackets: [],
  message: "",
  report: null,
  standard: true,
  currentSpeaker: "",
  currentHuman: false,
  currentInterruptible: false,
  usesLiveModel: false,
  affirmativeMs: 0,
  negativeMs: 0,
  affirmativeMainMs: 18 * 60 * 1_000,
  negativeMainMs: 18 * 60 * 1_000,
  floorHolder: "",
  calibrationPassage: "",
  baselineCpm: 0,
  calibrationRoundsCompleted: 0,
  lowConfidence: false,
  materials: "",
  awaitingSeat: false,
  seats: [],
  modelSettings: {
    modelId: "deepseek-flash",
    baseUrl: "https://api.deepseek.com",
    reasoningLevel: "high",
    connectionStatus: "not_configured",
    lastTestedAt: "",
    lastErrorCategory: "",
    hasApiKey: false,
  },
};

const MATCH_PHASES = new Set([
  "in_progress",
  "match_complete",
  "scoring",
  "recovery_required",
]);

export function routeForSnapshot(snapshot: Snapshot): string {
  if (!snapshot.matchId || snapshot.phase === "aborted") return "/workspace";
  if (snapshot.phase === "report_ready") {
    return `/report/${snapshot.matchId}`;
  }
  if (MATCH_PHASES.has(snapshot.phase)) {
    return `/match/${snapshot.matchId}`;
  }
  return "/workspace";
}

export function isFormalMatchPhase(phase: string): boolean {
  return MATCH_PHASES.has(phase);
}
