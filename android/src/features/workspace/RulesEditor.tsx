import { useEffect, useState } from "react";
import type {
  RuleDraft,
  RuleIssue,
  RuleStage,
  SemanticStageType,
} from "../session/types";

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

const TYPES: [SemanticStageType, string][] = [
  ["opening_statement", "立论陈词"],
  ["case_extension", "申论 / 展开论证"],
  ["rebuttal", "驳论"],
  ["cross_examination", "质询 / 攻辩"],
  ["paired_debate", "对辩"],
  ["cross_exam_summary", "质询小结"],
  ["free_debate", "自由辩"],
  ["closing_summary", "总结陈词"],
  ["custom", "自定义"],
];

const AFFIRMATIVE = SPEAKERS.slice(0, 4).map(([value]) => value);
const NEGATIVE = SPEAKERS.slice(4).map(([value]) => value);

function stageTemplate(type: SemanticStageType, index: number): RuleStage {
  const common: RuleStage = {
    id: `manual-${Date.now()}-${index}`,
    displayName: "新环节",
    semanticType: type,
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
    aiObjective: "围绕本环节职责完成发言",
    scoringFocus: ["切题性", "论证质量", "逻辑"],
    sourceEvidence: [],
    confidence: 100,
    unresolvedFields: [],
  };
  if (type === "cross_examination") {
    return {
      ...common,
      displayName: "新增质询",
      participationMode: "whole_team",
      turnMode: "one_way_question_answer",
      clockMode: "questioner_pool",
      interruptPolicy: "questioner_may_stop_answer",
      fixedSpeakers: [],
      affirmativeCandidates: AFFIRMATIVE,
      negativeCandidates: NEGATIVE,
      questioner: "AFF_2",
      respondent: "NEG_2",
      openingSide: "affirmative",
      durationSeconds: 180,
      questionSeconds: null,
      answerSeconds: null,
      aiObjective: "连续提问并检验对方论证",
      scoringFocus: ["质询质量", "临场应对", "逻辑"],
    };
  }
  if (type === "free_debate") {
    return {
      ...common,
      displayName: "自由辩",
      participationMode: "whole_team",
      turnMode: "alternating_teams",
      clockMode: "team_pool",
      fixedSpeakers: [],
      affirmativeCandidates: AFFIRMATIVE,
      negativeCandidates: NEGATIVE,
      openingSide: "affirmative",
      durationSeconds: null,
      affirmativeSeconds: 240,
      negativeSeconds: 240,
      turnSeconds: null,
      aiObjective: "回应对方上一轮并推进本方争点",
      scoringFocus: ["反驳", "临场应对", "团队协作"],
    };
  }
  if (type === "paired_debate") {
    return {
      ...common,
      displayName: "新增对辩",
      participationMode: "fixed_pair",
      turnMode: "alternating_pair",
      clockMode: "separate_sides",
      fixedSpeakers: ["AFF_2", "NEG_2"],
      affirmativeSeconds: 90,
      negativeSeconds: 90,
      durationSeconds: null,
    };
  }
  return common;
}

type Props = {
  draft: RuleDraft;
  issues: RuleIssue[];
  disabled: boolean;
  onReplace: (draft: RuleDraft) => void;
  meteringMode?: "words" | "time";
  speechRatePerMinute?: number;
  editedDraft?: RuleDraft;
  onDraftChange?: (draft: RuleDraft) => void;
};

export function RulesEditor({
  draft: sourceDraft,
  issues,
  disabled,
  onReplace,
  meteringMode = "time",
  speechRatePerMinute = 240,
  editedDraft,
  onDraftChange,
}: Props) {
  const [localDraft, setLocalDraft] = useState(sourceDraft);
  const draft = editedDraft ?? localDraft;
  function setDraft(next: RuleDraft) {
    if (onDraftChange) onDraftChange(next);
    else setLocalDraft(next);
  }
  const [expandedStages, setExpandedStages] = useState<Set<string>>(new Set());

  useEffect(() => {
    setLocalDraft(sourceDraft);
    setExpandedStages(
      new Set(
        issues
          .filter((issue) => issue.severity === "blocking")
          .map((issue) => issue.stageId),
      ),
    );
  }, [sourceDraft, issues]);

  function replaceStage(index: number, stage: RuleStage) {
    const stages = draft.stages.slice();
    stages[index] = stage;
    setDraft({ ...draft, stages });
  }

  function move(index: number, offset: number) {
    const target = index + offset;
    if (target < 0 || target >= draft.stages.length) return;
    const stages = draft.stages.slice();
    [stages[index], stages[target]] = [stages[target], stages[index]];
    setDraft({ ...draft, stages });
  }

  function toggleStage(id: string) {
    setExpandedStages((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <div className="rules-editor">
      <div className="rules-editor-heading">
        <label>
          赛制名称
          <input
            value={draft.rulesetName}
            disabled={disabled}
            onChange={(event) =>
              setDraft({ ...draft, rulesetName: event.target.value })
            }
          />
        </label>
        <button
          type="button"
          disabled={disabled}
          onClick={() =>
            setDraft({
              ...draft,
              stages: [
                ...draft.stages,
                stageTemplate("opening_statement", draft.stages.length),
              ],
            })
          }
        >
          添加环节
        </button>
        <button
          type="button"
          className="primary-action"
          disabled={
            disabled || JSON.stringify(draft) === JSON.stringify(sourceDraft)
          }
          onClick={() => onReplace(draft)}
        >
          保存赛程修改
        </button>
      </div>

      {issues.length > 0 ? (
        <ul className="rule-issues" aria-label="赛制问题">
          {issues.map((issue, index) => (
            <li key={`${issue.stageId}-${issue.field}-${index}`}>
              <button
                type="button"
                onClick={() => {
                  setExpandedStages(
                    (current) => new Set([...current, issue.stageId]),
                  );
                  window.requestAnimationFrame(() =>
                    document
                      .getElementById(`rule-stage-${issue.stageId}`)
                      ?.focus(),
                  );
                }}
              >
                {issue.severity === "blocking" ? "必须修正" : "提示"}：
                {issue.message}
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="rule-ready">语义和额度检查通过，可以确认规则。</p>
      )}

      {draft.planVersion === "match-plan.v3" ? (
        <section className="rule-coverage" aria-label="条款覆盖核对">
          <header>
            <div>
              <p className="eyebrow">强制核对</p>
              <h3>原文条款 → 赛程</h3>
            </div>
            <strong>
              {
                draft.coverageEntries.filter((entry) =>
                  ["mapped", "explanatory"].includes(entry.status),
                ).length
              }
              /{draft.sourceClauses.length} 已覆盖
            </strong>
          </header>
          <ol>
            {draft.sourceClauses.map((clause) => {
              const entry = draft.coverageEntries.find(
                (item) => item.clauseId === clause.id,
              ) ?? {
                clauseId: clause.id,
                status: "unmapped" as const,
                stageIds: [],
                note: "等待核对",
              };
              return (
                <li key={clause.id} id={`rule-stage-${clause.id}`}>
                  <p>{clause.text}</p>
                  <div>
                    <label>
                      处理方式
                      <select
                        value={entry.status}
                        disabled={disabled}
                        onChange={(event) => {
                          const status = event.target.value as
                            "mapped" | "explanatory" | "ambiguous" | "unmapped";
                          setDraft({
                            ...draft,
                            coverageEntries: [
                              ...draft.coverageEntries.filter(
                                (item) => item.clauseId !== clause.id,
                              ),
                              { ...entry, status },
                            ],
                          });
                        }}
                      >
                        <option value="mapped">映射到环节</option>
                        <option value="explanatory">说明性条款</option>
                        <option value="ambiguous">存在歧义</option>
                        <option value="unmapped">尚未处理</option>
                      </select>
                    </label>
                    {entry.status === "mapped" ? (
                      <label>
                        对应环节
                        <select
                          value={entry.stageIds[0] ?? ""}
                          disabled={disabled}
                          onChange={(event) =>
                            setDraft({
                              ...draft,
                              coverageEntries: [
                                ...draft.coverageEntries.filter(
                                  (item) => item.clauseId !== clause.id,
                                ),
                                {
                                  ...entry,
                                  stageIds: event.target.value
                                    ? [event.target.value]
                                    : [],
                                },
                              ],
                            })
                          }
                        >
                          <option value="">计时池或全局规则</option>
                          {draft.stages.map((stage) => (
                            <option value={stage.id} key={stage.id}>
                              {stage.displayName}
                            </option>
                          ))}
                        </select>
                      </label>
                    ) : null}
                  </div>
                </li>
              );
            })}
          </ol>
        </section>
      ) : null}

      {draft.clockPools.length > 0 ? (
        <section
          className="clock-pool-editor"
          aria-label={meteringMode === "words" ? "字数额度设置" : "计时池设置"}
        >
          <header>
            <p className="eyebrow">
              {meteringMode === "words" ? "按有效字数推进" : "独立计时"}
            </p>
            <h3>
              {meteringMode === "words" ? "四个团队字数池" : "四个团队计时池"}
            </h3>
            {meteringMode === "words" ? (
              <p>
                未另设字数时，按每分钟 {speechRatePerMinute}{" "}
                有效字换算赛制。停顿、打字和 AI
                思考不扣比赛额度；下方原赛制秒数仅用于换算单轮上限，不是倒计时。
              </p>
            ) : null}
          </header>
          <div>
            {draft.clockPools.map((pool, index) => (
              <label key={pool.id}>
                {meteringMode === "words"
                  ? pool.label.replace("主时间", "主额度")
                  : pool.label}
                （{meteringMode === "words" ? "字" : "秒"}）
                <input
                  type="number"
                  min={1}
                  max={meteringMode === "words" ? 100000 : 7200}
                  step={1}
                  value={
                    meteringMode === "words"
                      ? (pool.unitBudget ??
                        Math.floor(
                          (pool.initialSeconds * speechRatePerMinute) / 60,
                        ))
                      : pool.initialSeconds
                  }
                  disabled={disabled}
                  onChange={(event) => {
                    const clockPools = draft.clockPools.slice();
                    clockPools[index] = {
                      ...pool,
                      ...(meteringMode === "words"
                        ? { unitBudget: Number(event.target.value) }
                        : { initialSeconds: Number(event.target.value) }),
                    };
                    setDraft({ ...draft, clockPools });
                  }}
                />
              </label>
            ))}
          </div>
        </section>
      ) : null}

      <ol className="rule-stage-list">
        {draft.stages.map((stage, index) => {
          const stageIssues = issues.filter(
            (issue) => issue.stageId === stage.id,
          );
          const questioningSide =
            stage.openingSide ??
            (stage.questioner?.startsWith("NEG") ? "negative" : "affirmative");
          const questionerCandidates =
            questioningSide === "affirmative"
              ? stage.affirmativeCandidates
              : stage.negativeCandidates;
          const respondentCandidates =
            questioningSide === "affirmative"
              ? stage.negativeCandidates
              : stage.affirmativeCandidates;
          const expanded = expandedStages.has(stage.id);
          return (
            <li
              key={stage.id}
              id={`rule-stage-${stage.id}`}
              tabIndex={-1}
              className={
                stageIssues.length > 0 ? "rule-stage has-issue" : "rule-stage"
              }
            >
              <button
                type="button"
                className="rule-stage-summary"
                aria-expanded={expanded}
                onClick={() => toggleStage(stage.id)}
              >
                <span className="rule-stage-index">{index + 1}</span>
                <span className="rule-stage-title">
                  <strong>{stage.displayName}</strong>
                  <small>
                    {TYPES.find(([type]) => type === stage.semanticType)?.[1] ??
                      stage.semanticType}
                    {" · "}
                    {stageDurationLabel(
                      stage,
                      meteringMode,
                      speechRatePerMinute,
                      draft,
                    )}
                  </small>
                </span>
                <span
                  className={`confidence-chip ${stage.confidence < 70 ? "low" : ""}`}
                >
                  {stage.confidence < 70 ? "低置信" : `${stage.confidence}%`}
                </span>
                {stageIssues.length > 0 ? (
                  <span className="issue-chip">
                    {stageIssues.length} 项需处理
                  </span>
                ) : null}
                <span className="disclosure" aria-hidden="true">
                  {expanded ? "收起" : "编辑"}
                </span>
              </button>
              {expanded ? (
                <div className="rule-stage-details">
                  <div className="rule-stage-toolbar">
                    <button
                      type="button"
                      disabled={disabled || index === 0}
                      onClick={() => move(index, -1)}
                    >
                      上移
                    </button>
                    <button
                      type="button"
                      disabled={disabled || index === draft.stages.length - 1}
                      onClick={() => move(index, 1)}
                    >
                      下移
                    </button>
                    <button
                      type="button"
                      disabled={disabled}
                      onClick={() => {
                        const stages = draft.stages.slice();
                        stages.splice(index + 1, 0, {
                          ...stage,
                          id: `copy-${Date.now()}-${index}`,
                          displayName: `${stage.displayName}（副本）`,
                          sourceEvidence: [],
                        });
                        setDraft({ ...draft, stages });
                      }}
                    >
                      复制
                    </button>
                    <button
                      type="button"
                      disabled={disabled}
                      onClick={() =>
                        setDraft({
                          ...draft,
                          stages: draft.stages.filter((_, at) => at !== index),
                        })
                      }
                    >
                      删除
                    </button>
                  </div>
                  <div className="rule-stage-grid">
                    <label>
                      环节名称
                      <input
                        value={stage.displayName}
                        disabled={disabled}
                        onChange={(event) =>
                          replaceStage(index, {
                            ...stage,
                            displayName: event.target.value,
                          })
                        }
                      />
                    </label>
                    <label>
                      环节职责
                      <select
                        value={stage.semanticType}
                        disabled={disabled}
                        onChange={(event) => {
                          const replacement = stageTemplate(
                            event.target.value as SemanticStageType,
                            index,
                          );
                          replaceStage(index, {
                            ...replacement,
                            id: stage.id,
                            displayName: stage.displayName,
                          });
                        }}
                      >
                        {TYPES.map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </label>
                    {stage.semanticType === "free_debate" ? (
                      <>
                        {meteringMode !== "words" ? (
                          <>
                            <NumberField
                              label="正方共享时间（秒）"
                              value={stage.affirmativeSeconds}
                              disabled={disabled}
                              onChange={(value) =>
                                replaceStage(index, {
                                  ...stage,
                                  affirmativeSeconds: value,
                                })
                              }
                            />
                            <NumberField
                              label="反方共享时间（秒）"
                              value={stage.negativeSeconds}
                              disabled={disabled}
                              onChange={(value) =>
                                replaceStage(index, {
                                  ...stage,
                                  negativeSeconds: value,
                                })
                              }
                            />
                          </>
                        ) : null}
                        <SideField
                          value={stage.openingSide}
                          disabled={disabled}
                          onChange={(openingSide) =>
                            replaceStage(index, { ...stage, openingSide })
                          }
                        />
                        {meteringMode !== "words" ? (
                          <NumberField
                            label="单轮建议上限（秒）"
                            value={stage.turnSeconds}
                            disabled={disabled}
                            onChange={(turnSeconds) =>
                              replaceStage(index, { ...stage, turnSeconds })
                            }
                          />
                        ) : null}
                        <p className="rule-behavior-note">
                          {meteringMode === "words"
                            ? "全队共用顶部自由辩字数池，双方交替，不能打断；某方额度耗尽后，另一方可以继续发言或放弃剩余额度。"
                            : "全队可发言，双方交替；不能打断。某方时间耗尽后，另一方可以继续用完剩余时间。"}
                        </p>
                      </>
                    ) : stage.semanticType === "cross_examination" ? (
                      <>
                        <SpeakerField
                          label="提问辩位"
                          value={stage.questioner ?? "AFF_2"}
                          disabled={disabled}
                          onChange={(questioner) => {
                            const next = {
                              ...stage,
                              questioner,
                              openingSide: questioner.startsWith("AFF")
                                ? ("affirmative" as const)
                                : ("negative" as const),
                            };
                            const candidates = questioner.startsWith("AFF")
                              ? next.affirmativeCandidates
                              : next.negativeCandidates;
                            if (!candidates.includes(questioner)) {
                              if (questioner.startsWith("AFF")) {
                                next.affirmativeCandidates = [
                                  ...candidates,
                                  questioner,
                                ];
                              } else {
                                next.negativeCandidates = [
                                  ...candidates,
                                  questioner,
                                ];
                              }
                            }
                            replaceStage(index, next);
                          }}
                        />
                        <CandidatePoolField
                          label="AI 可派出的提问辩位"
                          side={questioningSide}
                          values={questionerCandidates}
                          disabled={disabled}
                          onChange={(values) => {
                            const next = {
                              ...stage,
                              questioner: values.includes(
                                stage.questioner ?? "",
                              )
                                ? stage.questioner
                                : (values[0] ?? null),
                            };
                            if (questioningSide === "affirmative") {
                              next.affirmativeCandidates = values;
                            } else {
                              next.negativeCandidates = values;
                            }
                            replaceStage(index, next);
                          }}
                        />
                        <CandidatePoolField
                          label="AI 可选择的回答辩位"
                          side={
                            questioningSide === "affirmative"
                              ? "negative"
                              : "affirmative"
                          }
                          values={respondentCandidates}
                          disabled={disabled}
                          onChange={(values) => {
                            const next = {
                              ...stage,
                              respondent: values.includes(
                                stage.respondent ?? "",
                              )
                                ? stage.respondent
                                : (values[0] ?? null),
                            };
                            if (questioningSide === "affirmative") {
                              next.negativeCandidates = values;
                            } else {
                              next.affirmativeCandidates = values;
                            }
                            replaceStage(index, next);
                          }}
                        />
                        <SpeakerField
                          label="初始答方"
                          value={stage.respondent ?? "NEG_2"}
                          disabled={disabled}
                          onChange={(respondent) => {
                            const next = { ...stage, respondent };
                            const candidates = respondent.startsWith("AFF")
                              ? next.affirmativeCandidates
                              : next.negativeCandidates;
                            if (!candidates.includes(respondent)) {
                              if (respondent.startsWith("AFF")) {
                                next.affirmativeCandidates = [
                                  ...candidates,
                                  respondent,
                                ];
                              } else {
                                next.negativeCandidates = [
                                  ...candidates,
                                  respondent,
                                ];
                              }
                            }
                            replaceStage(index, next);
                          }}
                        />
                        <NumberField
                          label={
                            meteringMode === "words"
                              ? "原赛制质询时长（秒，换算单次上限）"
                              : "质询总时长（秒）"
                          }
                          value={stage.durationSeconds}
                          disabled={disabled}
                          onChange={(durationSeconds) =>
                            replaceStage(index, { ...stage, durationSeconds })
                          }
                        />
                        <NumberField
                          label="单次提问建议（秒）"
                          value={stage.questionSeconds}
                          disabled={disabled}
                          onChange={(questionSeconds) =>
                            replaceStage(index, { ...stage, questionSeconds })
                          }
                        />
                        <NumberField
                          label="单次回答建议（秒）"
                          value={stage.answerSeconds}
                          disabled={disabled}
                          onChange={(answerSeconds) =>
                            replaceStage(index, { ...stage, answerSeconds })
                          }
                        />
                        <label className="checkbox-label">
                          <input
                            type="checkbox"
                            checked={
                              stage.interruptPolicy ===
                              "questioner_may_stop_answer"
                            }
                            disabled={disabled}
                            onChange={(event) =>
                              replaceStage(index, {
                                ...stage,
                                interruptPolicy: event.target.checked
                                  ? "questioner_may_stop_answer"
                                  : "forbidden",
                              })
                            }
                          />
                          允许提问方打断回答
                        </label>
                        <label className="checkbox-label">
                          <input
                            type="checkbox"
                            checked={stage.turnMode === "alternating_teams"}
                            disabled={disabled}
                            onChange={(event) =>
                              replaceStage(index, {
                                ...stage,
                                turnMode: event.target.checked
                                  ? "alternating_teams"
                                  : "one_way_question_answer",
                              })
                            }
                          />
                          本环节由双方轮流取得提问权
                        </label>
                      </>
                    ) : stage.semanticType === "paired_debate" ? (
                      <>
                        <SpeakerField
                          label="第一位辩手"
                          value={stage.fixedSpeakers[0] ?? "AFF_2"}
                          disabled={disabled}
                          onChange={(value) =>
                            replaceStage(index, {
                              ...stage,
                              fixedSpeakers: [
                                value,
                                stage.fixedSpeakers[1] ?? "NEG_2",
                              ],
                            })
                          }
                        />
                        <SpeakerField
                          label="第二位辩手"
                          value={stage.fixedSpeakers[1] ?? "NEG_2"}
                          disabled={disabled}
                          onChange={(value) =>
                            replaceStage(index, {
                              ...stage,
                              fixedSpeakers: [
                                stage.fixedSpeakers[0] ?? "AFF_2",
                                value,
                              ],
                            })
                          }
                        />
                        <NumberField
                          label="正方时间（秒）"
                          value={stage.affirmativeSeconds}
                          disabled={disabled}
                          onChange={(affirmativeSeconds) =>
                            replaceStage(index, {
                              ...stage,
                              affirmativeSeconds,
                            })
                          }
                        />
                        <NumberField
                          label="反方时间（秒）"
                          value={stage.negativeSeconds}
                          disabled={disabled}
                          onChange={(negativeSeconds) =>
                            replaceStage(index, { ...stage, negativeSeconds })
                          }
                        />
                      </>
                    ) : (
                      <>
                        <SpeakerField
                          label="发言辩位"
                          value={stage.fixedSpeakers[0] ?? "AFF_1"}
                          disabled={disabled}
                          onChange={(value) =>
                            replaceStage(index, {
                              ...stage,
                              fixedSpeakers: [value],
                            })
                          }
                        />
                        <NumberField
                          label={
                            meteringMode === "words"
                              ? "原赛制时长（秒，换算单轮字数）"
                              : "时长（秒）"
                          }
                          value={stage.durationSeconds}
                          disabled={disabled}
                          onChange={(durationSeconds) =>
                            replaceStage(index, { ...stage, durationSeconds })
                          }
                        />
                      </>
                    )}
                  </div>
                  <p className="rule-source-note">
                    识别置信度 {stage.confidence}%
                    {stage.sourceEvidence[0]?.quote
                      ? ` · 原文依据：${stage.sourceEvidence[0].quote}`
                      : " · 人工新增"}
                  </p>
                  {stageIssues.map((issue) => (
                    <p
                      className="error"
                      key={`${issue.field}-${issue.message}`}
                    >
                      {issue.message}
                    </p>
                  ))}
                </div>
              ) : null}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function stageDurationLabel(
  stage: RuleStage,
  meteringMode: "words" | "time",
  speechRatePerMinute: number,
  draft: RuleDraft,
) {
  if (meteringMode === "words") {
    const units = (seconds: number | null) =>
      Math.floor(((seconds ?? 0) * speechRatePerMinute) / 60);
    if (stage.semanticType === "free_debate") {
      const quota = (
        side: "affirmative" | "negative",
        seconds: number | null,
      ) => {
        const pool = draft.clockPools?.find(
          (pool) => pool.side === side && pool.kind === "free_debate",
        );
        return pool?.unitBudget ?? units(pool?.initialSeconds ?? seconds);
      };
      return `正 ${quota("affirmative", stage.affirmativeSeconds)}字 / 反 ${quota("negative", stage.negativeSeconds)}字（全队共享）`;
    }
    if (stage.semanticType === "paired_debate") {
      return `正 ${units(stage.affirmativeSeconds)}字 / 反 ${units(stage.negativeSeconds)}字`;
    }
    return stage.semanticType === "cross_examination"
      ? `单次上限 ${units(stage.durationSeconds)}字 · 共用提问方主额度`
      : `单轮上限 ${units(stage.durationSeconds)}字`;
  }
  if (
    stage.semanticType === "free_debate" ||
    stage.semanticType === "paired_debate"
  ) {
    return `正 ${stage.affirmativeSeconds ?? 0}s / 反 ${stage.negativeSeconds ?? 0}s`;
  }
  return `${stage.durationSeconds ?? 0}s`;
}

function NumberField({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: number | null;
  disabled: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <label>
      {label}
      <input
        type="number"
        min={1}
        max={3600}
        value={value ?? ""}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  );
}

function SpeakerField({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label>
      {label}
      <select
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      >
        {SPEAKERS.map(([speaker, name]) => (
          <option value={speaker} key={speaker}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
}

function SideField({
  value,
  disabled,
  onChange,
}: {
  value: "affirmative" | "negative" | null;
  disabled: boolean;
  onChange: (value: "affirmative" | "negative") => void;
}) {
  return (
    <label>
      先发方
      <select
        value={value ?? "affirmative"}
        disabled={disabled}
        onChange={(event) =>
          onChange(event.target.value as "affirmative" | "negative")
        }
      >
        <option value="affirmative">正方</option>
        <option value="negative">反方</option>
      </select>
    </label>
  );
}

function CandidatePoolField({
  label,
  side,
  values,
  disabled,
  onChange,
}: {
  label: string;
  side: "affirmative" | "negative";
  values: string[];
  disabled: boolean;
  onChange: (values: string[]) => void;
}) {
  const options =
    side === "affirmative" ? SPEAKERS.slice(0, 4) : SPEAKERS.slice(4);
  return (
    <fieldset className="candidate-pool">
      <legend>{label}</legend>
      {options.map(([speaker, name]) => (
        <label key={speaker}>
          <input
            type="checkbox"
            checked={values.includes(speaker)}
            disabled={disabled}
            onChange={(event) =>
              onChange(
                event.target.checked
                  ? [...values, speaker]
                  : values.filter((value) => value !== speaker),
              )
            }
          />
          {name}
        </label>
      ))}
    </fieldset>
  );
}
