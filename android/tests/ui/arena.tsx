// Dev-only presentation fixture. Not an app route, model call or acceptance of AI quality.
import { createRoot } from "react-dom/client";
import { useState } from "react";
import {
  MatchRoom,
  type MatchView,
} from "../../src/features/match-room/MatchRoom";
import { MobileViewport } from "../../src/platform/MobileViewport";
import {
  CrossExamPanel,
  type CrossExamSnapshot,
} from "../../src/features/arena/CrossExamPanel";
import "../../src/styles/global.css";
import "../../src/platform/mobile.css";

const mode = new URLSearchParams(location.search).get("mode") ?? "speech";
const fixture: MatchView = {
  phase: "in_progress",
  topic: "人工智能的发展，应当优先追求效率还是人的自主选择？",
  mode: "training",
  schedule: [
    {
      id: "current",
      kind:
        mode === "cross"
          ? "cross_exam"
          : mode === "free"
            ? "free_debate"
            : "opening",
      speaker: "AFF_1",
      durationSeconds: 180,
      wordCap: 480,
      interruptible: mode === "cross",
      started: true,
    },
  ],
  cursor: 0,
  meteringMode: "words",
  remainingUnits: 3000,
  currentWordCap: 480,
  affirmativeMainUnits: 3000,
  negativeMainUnits: 3200,
  affirmativeFreeUnits: 960,
  negativeFreeUnits: 880,
  remainingMs: 0,
  affirmativeMs: 0,
  negativeMs: 0,
  floorHolder: "affirmative",
  currentSpeaker: "AFF_1",
  transcripts: Array.from({ length: 30 }, (_, index) => ({
    id: String(index),
    slot: index % 2 ? "NEG_2" : "AFF_1",
    effectiveText: `第 ${index + 1} 条发言：${"我们需要讨论标准、事实、比较与可能的代价，回应对方刚才提出的论证。".repeat(4)}`,
    overflowText: "",
    interruptedTail: "",
  })),
  ownStrategy: "澄清判断标准，以公开证据回应对方。",
  opponentStrategy: "对比人的自主选择与效率。",
  materials: "合成布局测试，不包含真实参赛记录。",
  currentHuman: true,
  currentInterruptible: false,
  awaitingSeat: false,
  usesLiveModel: false,
  standard: true,
  seats: Array.from({ length: 8 }, (_, index) => ({
    index,
    name: `${index < 4 ? "正方" : "反方"}${["一", "二", "三", "四"][index % 4]}辩`,
    duty: ["立论", "展开", "质询", "总结"][index % 4],
    side: index < 4 ? "affirmative" : "negative",
    controller: index === 0 ? "human" : "ai",
    defaultController: "ai",
    speaking: index === 0,
    claimable: index < 4,
  })),
};
const cross: CrossExamSnapshot = {
  eventId: "current",
  phase: "question",
  questioner: "AFF_3",
  respondent: "NEG_2",
  questionerHuman: true,
  respondentHuman: false,
  question: "",
  visiblePrefix: "",
  interruptible: false,
  answerPrepared: false,
  revealedUnits: 0,
  totalUnits: 0,
  roundNumber: 1,
  nextSide: "affirmative",
  totalMs: 0,
  usedMs: 0,
  remainingMs: 0,
  meteringMode: "words",
  remainingUnits: 3000,
  questionWordCap: 120,
  answerWordCap: 360,
  selectionSummary: "合法辩位由赛制约束，当前可人工插入本方提问者。",
  rolesSelected: true,
  eligibleQuestioners: ["AFF_1", "AFF_2", "AFF_3", "AFF_4"],
  history: [],
};

function ArenaFixture() {
  const [draft, setDraft] = useState("切换人工与 AI 时保留这段草稿。");
  const [human, setHuman] = useState(true);
  const [thinking, setThinking] = useState<string | null>(null);
  const [actions, setActions] = useState(0);
  const textareaProps = {
    value: draft,
    onChange: (event: React.ChangeEvent<HTMLTextAreaElement>) =>
      setDraft(event.target.value),
  };
  const next = {
    ...fixture,
    currentHuman: human,
    seats: fixture.seats.map((seat) =>
      seat.index === 0 ? { ...seat, controller: human ? "human" : "ai" } : seat,
    ),
  };
  return (
    <MobileViewport>
      <main className="match-page">
        <output aria-label="合成测试提交次数">{actions}</output>
        <MatchRoom
          view={next}
          textareaProps={textareaProps}
          draftLength={draft.length}
          onSubmit={(event) => {
            event.preventDefault();
            setActions(actions + 1);
          }}
          onClaim={() => setHuman(true)}
          onAdvance={() => setThinking("合成等待，请继续查看历史")}
          onCancel={() => setThinking(null)}
          onFinish={() => setActions(actions + 1)}
          onTimeoutSubmit={() => {}}
          onSetController={() => {}}
          onSetCurrentController={(_, toHuman) => {
            setThinking(null);
            setHuman(toHuman);
          }}
          thinking={thinking}
          windowStart={0}
          onMore={() => {}}
          onAbort={() => {}}
          crossExamActive={mode === "cross"}
          stageWorkspace={
            mode === "cross" ? (
              <CrossExamPanel
                snapshot={cross}
                questionTextareaProps={textareaProps}
                answerTextareaProps={textareaProps}
                answerLength={draft.length}
                wordCap={480}
                busy={false}
                onSubmitQuestion={(event) => event.preventDefault()}
                onGenerateQuestion={() => {}}
                onInsertQuestioner={() => {}}
                onPrepareHumanAnswer={(event) => event.preventDefault()}
                onGenerateAnswer={() => {}}
                onSetRespondentHuman={() => {}}
                onReveal={async () => cross}
                onComplete={() => {}}
                onInterrupt={() => {}}
                onTimeout={() => {}}
                onEndSession={() => {}}
                onCancel={() => {}}
              />
            ) : undefined
          }
        />
      </main>
    </MobileViewport>
  );
}
createRoot(document.getElementById("root")!).render(<ArenaFixture />);
