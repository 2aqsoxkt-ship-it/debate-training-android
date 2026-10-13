import type { RuleDraft, Snapshot } from "../session/types";
import { setupStepForPhase } from "./SetupWizard";

export type SetupDraft = {
  name: string;
  topic: string;
  materials: string;
  rulesText: string;
  mode: string;
  side: string;
  difficulty: string;
  selectedSlots: number[];
  activeStep: number;
  ruleBuffer: RuleDraft | null;
};

type CachedDraft = { authority: string; draft: SetupDraft };
// Route-navigation retention only. Nothing is written to browser storage or disk.
const drafts = new Map<string, CachedDraft>();

function scope(snapshot: Snapshot) {
  return JSON.stringify([snapshot.activeProfileId, snapshot.matchId]);
}

function authority(snapshot: Snapshot) {
  return JSON.stringify([
    snapshot.phase,
    snapshot.mode,
    snapshot.topic,
    snapshot.materials,
    snapshot.userSide,
    snapshot.difficulty,
    snapshot.ruleDraft,
    snapshot.rulesConfirmed,
    snapshot.lineupConfirmed,
    snapshot.preparationReady,
    snapshot.rolePackets,
    snapshot.schedule,
    humanSlots(snapshot),
  ]);
}

export function humanSlots(snapshot: Snapshot) {
  return snapshot.seats
    .filter((seat) => seat.controller.toLowerCase() === "human")
    .map((seat) => seat.index)
    .sort((left, right) => left - right);
}

export function draftFromSnapshot(snapshot: Snapshot): SetupDraft {
  return {
    name: "我的档案",
    topic: snapshot.topic || "人工智能应当受到限制",
    materials: snapshot.materials || "",
    rulesText: snapshot.ruleDraft?.sourceText || "",
    mode: snapshot.mode || "training",
    side: snapshot.matchId
      ? (snapshot.userSide ?? "affirmative")
      : snapshot.userSide || "affirmative",
    difficulty: snapshot.difficulty || "standard",
    selectedSlots: snapshot.matchId ? humanSlots(snapshot) : [0],
    activeStep: snapshot.profiles.length
      ? Math.min(setupStepForPhase(snapshot.phase), 8)
      : 0,
    ruleBuffer: snapshot.ruleDraft ?? null,
  };
}

export function readSetupDraft(snapshot: Snapshot): SetupDraft {
  const cached = drafts.get(scope(snapshot));
  return cached?.authority === authority(snapshot)
    ? cached.draft
    : draftFromSnapshot(snapshot);
}

export function rememberSetupDraft(snapshot: Snapshot, draft: SetupDraft) {
  const key = scope(snapshot);
  drafts.delete(key);
  drafts.set(key, { authority: authority(snapshot), draft });
  if (drafts.size > 20) drafts.delete(drafts.keys().next().value!);
}

export function preparationArguments(draft: SetupDraft) {
  return {
    mode: draft.mode,
    topic: draft.topic.trim(),
    materials: draft.materials,
    userSide: draft.side,
    difficulty: draft.difficulty,
  };
}

export function preparationChanged(snapshot: Snapshot, draft: SetupDraft) {
  if (!snapshot.matchId) return false;
  return (
    snapshot.mode !== draft.mode ||
    snapshot.topic.trim() !== draft.topic.trim() ||
    snapshot.materials !== draft.materials ||
    (snapshot.userSide ?? "affirmative") !== draft.side ||
    (snapshot.difficulty || "standard") !== draft.difficulty
  );
}
