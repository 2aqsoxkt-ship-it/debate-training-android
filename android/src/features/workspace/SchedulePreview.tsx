import type { RuleStage, Snapshot } from "../session/types";
import { speakerLabel } from "../../lib/presentation";

const SPEAKERS: [string, string][] = [
  ["AFF_1", "正方一辩"],
  ["AFF_2", "正方二辩"],
  ["AFF_3", "正方三辩"],
  ["AFF_4", "正方四辩"],
  ["NEG_1", "反方一辩"],
  ["NEG_2", "反方二辩"],
  ["NEG_3", "反方三辩"],
  ["NEG_4", "反方四辩"],
];

function stageKind(stage: RuleStage) {
  if (stage.semanticType === "free_debate") return "free_debate";
  if (stage.semanticType === "cross_examination") return "cross_exam";
  if (stage.semanticType === "closing_summary") return "summary";
  if (["custom", "paired_debate"].includes(stage.semanticType)) return "custom";
  return "opening";
}

function participants(kind: string, stage?: RuleStage) {
  if (kind === "free_debate") {
    const opening = stage?.openingSide;
    const names = (slots: string[]) => slots.map(speakerLabel).join("、");
    const candidates =
      stage &&
      (stage.affirmativeCandidates.length || stage.negativeCandidates.length)
        ? `；合法辩位：${names(stage.affirmativeCandidates) || "正方请核对赛制"} / ${names(stage.negativeCandidates) || "反方请核对赛制"}`
        : "";
    return `双方团队交替${opening ? ` · ${opening === "negative" ? "反方" : "正方"}先发` : " · 首发方请在赛制中核对"}；每方全队共用额度，不可打断${candidates}`;
  }
  if (!stage)
    return "按赛制由合法候选辩位参与问答；在赛制编辑器核对提问权和候选人";
  const side =
    stage.openingSide ??
    (stage.questioner?.startsWith("NEG") ? "negative" : "affirmative");
  const questioners =
    side === "negative"
      ? stage.negativeCandidates
      : stage.affirmativeCandidates;
  const respondents =
    side === "negative"
      ? stage.affirmativeCandidates
      : stage.negativeCandidates;
  const labels = (slots: string[], fallback: string | null) =>
    (slots.length ? slots : fallback ? [fallback] : [])
      .map(speakerLabel)
      .join("、") || "按赛制候选人";
  const direction =
    stage.turnMode === "alternating_teams"
      ? `双方轮流提问 · ${side === "negative" ? "反方" : "正方"}先问`
      : `${side === "negative" ? "反方" : "正方"}提问 → ${side === "negative" ? "正方" : "反方"}回答`;
  return `${direction}；${stage.turnMode === "alternating_teams" ? "首轮提问" : "提问"}：${labels(questioners, stage.questioner)}；${stage.turnMode === "alternating_teams" ? "首轮回答" : "回答"}：${labels(respondents, stage.respondent)}`;
}

export function SchedulePreview({
  view,
  busy,
  onUpdate,
}: {
  view: Snapshot;
  busy: boolean;
  onUpdate: (index: number, speaker: string, durationSeconds: number) => void;
}) {
  // V2 compilation creates one event per fixed speaker, except one team free
  // session. Use order only when the entire expansion matches; never attribute
  // another stage's roles to a legacy/mismatched schedule.
  const expanded =
    view.ruleDraft?.stages.flatMap((stage) =>
      Array.from(
        {
          length:
            stage.semanticType === "free_debate"
              ? 1
              : Math.max(1, stage.fixedSpeakers.length),
        },
        () => stage,
      ),
    ) ?? [];
  const mapped =
    expanded.length === view.schedule.length &&
    expanded.every(
      (stage, index) => stageKind(stage) === view.schedule[index].kind,
    );
  return (
    <div className="schedule-preview compact-table">
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th>环节</th>
            <th>参与安排</th>
            {view.meteringMode !== "words" ? <th>时限</th> : null}
            <th>{view.meteringMode === "words" ? "字数约束" : "上限"}</th>
          </tr>
        </thead>
        <tbody>
          {view.schedule.map((event, index) => {
            const stage = mapped ? expanded[index] : undefined;
            const teamEvent = ["free_debate", "cross_exam"].includes(
              event.kind,
            );
            const fallbackName =
              {
                opening: "陈词",
                cross_exam: "质询",
                free_debate: "自由辩",
                summary: "总结",
              }[event.kind] ?? event.kind;
            return (
              <tr key={event.id}>
                <td data-label="序号">{index + 1}</td>
                <td data-label="环节">{stage?.displayName ?? fallbackName}</td>
                <td data-label="参与安排">
                  {teamEvent ? (
                    <>
                      <span>{participants(event.kind, stage)}</span>
                      <small> · 修改参与规则请返回赛制编辑器</small>
                    </>
                  ) : (
                    <select
                      aria-label={`第${index + 1}行辩位`}
                      value={event.speaker}
                      disabled={busy}
                      onChange={(change) =>
                        onUpdate(
                          index,
                          change.target.value,
                          event.durationSeconds,
                        )
                      }
                    >
                      {SPEAKERS.map(([value, label]) => (
                        <option key={value} value={value}>
                          {label}
                        </option>
                      ))}
                    </select>
                  )}
                </td>
                {view.meteringMode !== "words" ? (
                  <td data-label="时限">{event.durationSeconds}s</td>
                ) : null}
                <td
                  data-label={
                    view.meteringMode === "words" ? "字数约束" : "上限"
                  }
                >
                  {view.meteringMode === "words" && event.kind === "free_debate"
                    ? `正 ${view.affirmativeFreeUnits ?? 960} / 反 ${view.negativeFreeUnits ?? 960} 字，全队共享`
                    : `${event.wordCap}${view.meteringMode === "words" ? (event.kind === "cross_exam" ? " 字（单次上限，共用提问方主额度）" : " 字（受主额度约束）") : ""}`}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
