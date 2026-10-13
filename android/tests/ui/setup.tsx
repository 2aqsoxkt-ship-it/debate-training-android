// Presentation/DOM-input regressions only; not shipped in the APK's app route.
import { createRoot } from "react-dom/client";
import { useState } from "react";
import { MobileViewport } from "../../src/platform/MobileViewport";
import { SchedulePreview } from "../../src/features/workspace/SchedulePreview";
import { RulesEditor } from "../../src/features/workspace/RulesEditor";
import {
  emptySnapshot,
  type RuleDraft,
  type RuleStage,
} from "../../src/features/session/types";
import { useInputTelemetry } from "../../src/features/telemetry/useInputTelemetry";
import { fold } from "../../src/features/telemetry/fold";
import "../../src/styles/global.css";
import "../../src/platform/mobile.css";

const stage: RuleStage = {
  id: "opening",
  displayName: "正方一辩陈词",
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
  aiObjective: "说明本方判断标准与核心论证",
  scoringFocus: ["逻辑"],
  sourceEvidence: [],
  confidence: 60,
  unresolvedFields: [],
};
const draft: RuleDraft = {
  schemaVersion: "ruleset.v2",
  planVersion: "match-plan.v3",
  rulesetName: "竖屏布局合成赛制",
  sourceText: "陈词后质询，然后自由辩。",
  affirmativeDuties: ["立论", "申论", "小结", "总结"],
  negativeDuties: ["立论", "申论", "小结", "总结"],
  stages: [
    stage,
    {
      ...stage,
      id: "cross",
      displayName: "反方质询正方一辩",
      semanticType: "cross_examination",
      participationMode: "whole_team",
      turnMode: "one_way_question_answer",
      clockMode: "questioner_pool",
      interruptPolicy: "questioner_may_stop_answer",
      fixedSpeakers: [],
      questioner: "NEG_4",
      respondent: "AFF_1",
      openingSide: "negative",
      affirmativeCandidates: ["AFF_1"],
      negativeCandidates: ["NEG_1", "NEG_2", "NEG_3", "NEG_4"],
    },
    {
      ...stage,
      id: "free",
      displayName: "自由辩论",
      semanticType: "free_debate",
      participationMode: "whole_team",
      turnMode: "alternating_teams",
      clockMode: "team_pool",
      fixedSpeakers: [],
      affirmativeCandidates: ["AFF_1", "AFF_2", "AFF_3", "AFF_4"],
      negativeCandidates: ["NEG_1", "NEG_2", "NEG_3", "NEG_4"],
      openingSide: "affirmative",
      durationSeconds: null,
      affirmativeSeconds: 240,
      negativeSeconds: 240,
    },
  ],
  clockPools: [],
  sourceClauses: [
    {
      id: "clause",
      text: "质询双方共同参与，提问方可打断回答。",
      start: 0,
      end: 20,
    },
  ],
  coverageEntries: [
    { clauseId: "clause", status: "mapped", stageIds: ["cross"], note: "映射" },
  ],
  migrationReviewRequired: false,
};

function SetupFixture() {
  const [schedule, setSchedule] = useState([
    {
      id: "opening",
      kind: "opening",
      speaker: "AFF_1",
      durationSeconds: 180,
      wordCap: 720,
      interruptible: false,
    },
    {
      id: "cross",
      kind: "cross_exam",
      speaker: "NEG_4",
      durationSeconds: 180,
      wordCap: 720,
      interruptible: true,
    },
    {
      id: "free",
      kind: "free_debate",
      speaker: "AFF_1",
      durationSeconds: 240,
      wordCap: 960,
      interruptible: false,
    },
  ]);
  const [edits, setEdits] = useState(0);
  const telemetry = useInputTelemetry({ wordCap: 720 });
  const mode = new URLSearchParams(location.search).get("mode") ?? "review";
  return (
    <MobileViewport>
      <main className="workspace pre-match-page">
        {mode === "input" ? (
          <>
            <textarea
              aria-label="手机输入回放测试"
              {...telemetry.bindTextareaProps}
            />
            <output aria-label="回放结果">
              {JSON.stringify({
                ...telemetry.snapshot(),
                replayed: fold(telemetry.snapshot().events).telemetry.finalText,
              })}
            </output>
          </>
        ) : (
          <section className="setup-panel">
            <h1>
              {mode === "editor" ? "赛制编辑" : "检查赛程顺序与八位辩手任务"}
            </h1>
            {mode === "editor" ? (
              <RulesEditor
                draft={draft}
                issues={[]}
                disabled={false}
                meteringMode="words"
                onReplace={() => setEdits(edits + 1)}
              />
            ) : (
              <SchedulePreview
                busy={false}
                view={{
                  ...emptySnapshot,
                  meteringMode: "words",
                  schedule,
                  ruleDraft: draft,
                }}
                onUpdate={(index, speaker) => {
                  setSchedule(
                    schedule.map((event, at) =>
                      at === index ? { ...event, speaker } : event,
                    ),
                  );
                  setEdits(edits + 1);
                }}
              />
            )}
            <output aria-label="人工赛程修改次数">{edits}</output>
          </section>
        )}
      </main>
    </MobileViewport>
  );
}
createRoot(document.getElementById("root")!).render(<SetupFixture />);
