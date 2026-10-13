use serde::{Deserialize, Serialize};

use crate::domain::ruleset::{
    ClockOwner, RulesetV1, Side, SlotId, StageDefinitionV1, StageKind, RULESET_SCHEMA_VERSION,
};

pub const RULESET_V2_SCHEMA_VERSION: &str = "ruleset.v2";
pub const MATCH_PLAN_V3_VERSION: &str = "match-plan.v3";

fn default_plan_version() -> String {
    "ruleset.v2".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockPoolKindV3 {
    Main,
    FreeDebate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockPoolV3 {
    pub id: String,
    pub label: String,
    pub side: Side,
    pub kind: ClockPoolKindV3,
    pub initial_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_budget: Option<u32>,
    pub warning_seconds: u32,
    pub expiry_signal_count: u32,
    pub allow_waive_remaining: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatusV3 {
    Mapped,
    Explanatory,
    Ambiguous,
    Unmapped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceClauseV3 {
    pub id: String,
    pub text: String,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageEntryV3 {
    pub clause_id: String,
    pub status: CoverageStatusV3,
    #[serde(default)]
    pub stage_ids: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStageType {
    OpeningStatement,
    CaseExtension,
    Rebuttal,
    CrossExamination,
    PairedDebate,
    CrossExamSummary,
    FreeDebate,
    ClosingSummary,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipationMode {
    Single,
    FixedPair,
    CandidatePool,
    WholeTeam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    ContinuousSpeech,
    OneWayQuestionAnswer,
    AlternatingPair,
    AlternatingTeams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockMode {
    Individual,
    QuestionerPool,
    SeparateSides,
    TeamPool,
    SharedStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptPolicyV2 {
    Forbidden,
    QuestionerMayStopAnswer,
    ClockExpiryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Blocking,
    Warning,
    Suggestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEvidenceV2 {
    pub quote: String,
    pub start: Option<u32>,
    pub end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageDefinitionV2 {
    pub id: String,
    pub display_name: String,
    pub semantic_type: SemanticStageType,
    pub participation_mode: ParticipationMode,
    pub turn_mode: TurnMode,
    pub clock_mode: ClockMode,
    pub interrupt_policy: InterruptPolicyV2,
    #[serde(default)]
    pub fixed_speakers: Vec<SlotId>,
    #[serde(default)]
    pub affirmative_candidates: Vec<SlotId>,
    #[serde(default)]
    pub negative_candidates: Vec<SlotId>,
    pub questioner: Option<SlotId>,
    pub respondent: Option<SlotId>,
    pub opening_side: Option<Side>,
    pub duration_seconds: Option<u32>,
    pub affirmative_seconds: Option<u32>,
    pub negative_seconds: Option<u32>,
    pub question_seconds: Option<u32>,
    pub answer_seconds: Option<u32>,
    #[serde(default)]
    pub turn_seconds: Option<u32>,
    pub follow_up_allowed: bool,
    pub respondent_may_counter_question: bool,
    pub max_rounds: Option<u32>,
    pub ai_objective: String,
    pub scoring_focus: Vec<String>,
    #[serde(default)]
    pub source_evidence: Vec<SourceEvidenceV2>,
    pub confidence: u32,
    #[serde(default)]
    pub unresolved_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesetV2 {
    pub schema_version: String,
    #[serde(default = "default_plan_version")]
    pub plan_version: String,
    pub ruleset_name: String,
    pub source_text: String,
    pub affirmative_duties: [String; 4],
    pub negative_duties: [String; 4],
    pub stages: Vec<StageDefinitionV2>,
    #[serde(default)]
    pub clock_pools: Vec<ClockPoolV3>,
    #[serde(default)]
    pub source_clauses: Vec<SourceClauseV3>,
    #[serde(default)]
    pub coverage_entries: Vec<CoverageEntryV3>,
    #[serde(default)]
    pub migration_review_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleIssueV2 {
    pub severity: IssueSeverity,
    pub stage_id: String,
    pub field: String,
    pub message: String,
}

impl RuleIssueV2 {
    fn blocking(stage: &StageDefinitionV2, field: &str, message: &str) -> Self {
        Self {
            severity: IssueSeverity::Blocking,
            stage_id: stage.id.clone(),
            field: field.to_owned(),
            message: message.to_owned(),
        }
    }
}

fn default_clock_pools() -> Vec<ClockPoolV3> {
    vec![
        ClockPoolV3 {
            id: "affirmative-main".to_owned(),
            label: "正方主时间".to_owned(),
            side: Side::Affirmative,
            kind: ClockPoolKindV3::Main,
            initial_seconds: 18 * 60,
            warning_seconds: 30,
            expiry_signal_count: 2,
            unit_budget: None,
            allow_waive_remaining: false,
        },
        ClockPoolV3 {
            id: "negative-main".to_owned(),
            label: "反方主时间".to_owned(),
            side: Side::Negative,
            kind: ClockPoolKindV3::Main,
            initial_seconds: 18 * 60,
            warning_seconds: 30,
            expiry_signal_count: 2,
            unit_budget: None,
            allow_waive_remaining: false,
        },
        ClockPoolV3 {
            id: "affirmative-free".to_owned(),
            label: "正方自由辩".to_owned(),
            side: Side::Affirmative,
            kind: ClockPoolKindV3::FreeDebate,
            initial_seconds: 4 * 60,
            warning_seconds: 30,
            expiry_signal_count: 2,
            unit_budget: None,
            allow_waive_remaining: true,
        },
        ClockPoolV3 {
            id: "negative-free".to_owned(),
            label: "反方自由辩".to_owned(),
            side: Side::Negative,
            kind: ClockPoolKindV3::FreeDebate,
            initial_seconds: 4 * 60,
            warning_seconds: 30,
            expiry_signal_count: 2,
            unit_budget: None,
            allow_waive_remaining: true,
        },
    ]
}

fn source_clauses(source: &str) -> Vec<SourceClauseV3> {
    let mut clauses = Vec::new();
    let mut buffer = String::new();
    let mut start = 0usize;
    let mut position = 0usize;
    for character in source.chars() {
        let boundary = matches!(character, '。' | '；' | ';' | '\n');
        if boundary {
            let text = buffer.trim().to_owned();
            if !text.is_empty() {
                clauses.push(SourceClauseV3 {
                    id: format!("clause-{}", clauses.len() + 1),
                    text,
                    start: start as u32,
                    end: position as u32,
                });
            }
            buffer.clear();
            start = position + 1;
        } else {
            buffer.push(character);
        }
        position += 1;
    }
    let text = buffer.trim().to_owned();
    if !text.is_empty() {
        clauses.push(SourceClauseV3 {
            id: format!("clause-{}", clauses.len() + 1),
            text,
            start: start as u32,
            end: position as u32,
        });
    }
    clauses
}

fn stage_ids_for_clause(clause: &str, stages: &[StageDefinitionV2]) -> Vec<String> {
    let semantic_match = |stage: &StageDefinitionV2| {
        (clause.contains("自由辩") && stage.semantic_type == SemanticStageType::FreeDebate)
            || (clause.contains("质询小结")
                && stage.semantic_type == SemanticStageType::CrossExamSummary)
            || (clause.contains("总结陈词")
                && stage.semantic_type == SemanticStageType::ClosingSummary)
            || (clause.contains("质询")
                && !clause.contains("质询小结")
                && stage.semantic_type == SemanticStageType::CrossExamination)
            || ((clause.contains("陈词") || clause.contains("发言") || clause.contains("立论"))
                && matches!(
                    stage.semantic_type,
                    SemanticStageType::OpeningStatement | SemanticStageType::CaseExtension
                ))
    };
    stages
        .iter()
        .filter(|stage| clause.contains(&stage.display_name) || semantic_match(stage))
        .map(|stage| stage.id.clone())
        .collect()
}

fn populate_coverage(ruleset: &mut RulesetV2, authoritative_default: bool) {
    ruleset.source_clauses = source_clauses(&ruleset.source_text);
    ruleset.coverage_entries = ruleset
        .source_clauses
        .iter()
        .map(|clause| {
            let stage_ids = stage_ids_for_clause(&clause.text, &ruleset.stages);
            let pool_clause = clause.text.contains("双方各18分钟")
                || (clause.text.contains("自由辩") && clause.text.contains("各4分钟"))
                || ((clause.text.contains("正方") || clause.text.contains("反方"))
                    && (clause.text.contains("分钟") || clause.text.contains('秒')));
            let status = if authoritative_default || pool_clause || !stage_ids.is_empty() {
                CoverageStatusV3::Mapped
            } else {
                CoverageStatusV3::Ambiguous
            };
            CoverageEntryV3 {
                clause_id: clause.id.clone(),
                status,
                stage_ids,
                note: if status == CoverageStatusV3::Mapped {
                    "已核对".to_owned()
                } else {
                    "未找到可靠对应关系，请人工选择环节或标记为说明条款".to_owned()
                },
            }
        })
        .collect();
}

pub fn traditional_four_on_four_v2() -> RulesetV2 {
    let duties = ["陈词", "陈词与回应", "质询小结", "总结"].map(str::to_owned);
    let mut ruleset = RulesetV2 {
        schema_version: RULESET_V2_SCHEMA_VERSION.to_owned(),
        plan_version: MATCH_PLAN_V3_VERSION.to_owned(),
        ruleset_name: "十八分钟团队计时四对四赛制".to_owned(),
        source_text: "双方各18分钟，自由辩各4分钟另外计时。陈词1：正方一辩发言，反方四辩质询；反方一辩陈词，正方四辩质询。质询方可以进行打断，被质询方不能打断、反问，质询回答时间计入质询者时间。陈词2：正方二辩发言，反方三辩质询正方二辩；反方二辩发言，正方三辩质询反方二辩。质询小结由双方三辩完成，先反方再正方。自由辩正方先开始，此后正反自动轮流发言，每方四位辩手的发言次数、时间及次序均无限制，但一方落座后在对方发言前不得再次发言。双方自由辩各4分钟，剩30秒一声提示，时间用尽两声提示。总结陈词由双方四辩完成，反方先发言。".to_owned(),
        affirmative_duties: duties.clone(),
        negative_duties: duties,
        stages: vec![
            speech_stage(
                "aff-opening",
                "正方一辩陈词",
                SemanticStageType::OpeningStatement,
                SlotId::Aff1,
                180,
            ),
            cross_stage(
                "neg-cross-aff-opening",
                "反方集体质询正方",
                SlotId::Neg4,
                SlotId::Aff1,
            ),
            speech_stage(
                "neg-opening",
                "反方一辩陈词",
                SemanticStageType::OpeningStatement,
                SlotId::Neg1,
                180,
            ),
            cross_stage(
                "aff-cross-neg-opening",
                "正方集体质询反方",
                SlotId::Aff4,
                SlotId::Neg1,
            ),
            speech_stage(
                "aff-second-statement",
                "正方二辩陈词",
                SemanticStageType::CaseExtension,
                SlotId::Aff2,
                180,
            ),
            cross_stage(
                "neg-cross-aff-second",
                "反方集体质询正方",
                SlotId::Neg3,
                SlotId::Aff2,
            ),
            speech_stage(
                "neg-second-statement",
                "反方二辩陈词",
                SemanticStageType::CaseExtension,
                SlotId::Neg2,
                180,
            ),
            cross_stage(
                "aff-cross-neg-second",
                "正方集体质询反方",
                SlotId::Aff3,
                SlotId::Neg2,
            ),
            speech_stage(
                "neg-cross-summary",
                "反方三辩质询小结",
                SemanticStageType::CrossExamSummary,
                SlotId::Neg3,
                180,
            ),
            speech_stage(
                "aff-cross-summary",
                "正方三辩质询小结",
                SemanticStageType::CrossExamSummary,
                SlotId::Aff3,
                180,
            ),
            free_debate_stage(),
            speech_stage(
                "neg-closing",
                "反方四辩总结陈词",
                SemanticStageType::ClosingSummary,
                SlotId::Neg4,
                180,
            ),
            speech_stage(
                "aff-closing",
                "正方四辩总结陈词",
                SemanticStageType::ClosingSummary,
                SlotId::Aff4,
                180,
            ),
        ],
        clock_pools: default_clock_pools(),
        source_clauses: Vec::new(),
        coverage_entries: Vec::new(),
        migration_review_required: false,
    };
    populate_coverage(&mut ruleset, true);
    ruleset
}

pub fn team_bank_ruleset_v3_from_source(source: &str) -> RulesetV2 {
    let mut ruleset = traditional_four_on_four_v2();
    ruleset.source_text = source.trim().to_owned();
    populate_coverage(&mut ruleset, true);
    ruleset
}

pub fn manual_ruleset_v2(source: &str) -> RulesetV2 {
    let mut stages = source
        .split(['。', '；', ';', '\n'])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .take(24)
        .enumerate()
        .map(|(index, clause)| StageDefinitionV2 {
            id: format!("manual-source-{index}"),
            display_name: clause.chars().take(36).collect(),
            semantic_type: SemanticStageType::Custom,
            participation_mode: ParticipationMode::Single,
            turn_mode: TurnMode::ContinuousSpeech,
            clock_mode: ClockMode::Individual,
            interrupt_policy: InterruptPolicyV2::Forbidden,
            fixed_speakers: vec![SlotId::Aff1],
            affirmative_candidates: Vec::new(),
            negative_candidates: Vec::new(),
            questioner: None,
            respondent: None,
            opening_side: None,
            duration_seconds: Some(180),
            affirmative_seconds: None,
            negative_seconds: None,
            question_seconds: None,
            answer_seconds: None,
            turn_seconds: None,
            follow_up_allowed: false,
            respondent_may_counter_question: false,
            max_rounds: None,
            ai_objective: "人工确认本环节职责后生成目标".to_owned(),
            scoring_focus: vec!["切题性".to_owned(), "逻辑".to_owned()],
            source_evidence: vec![SourceEvidenceV2 {
                quote: clause.to_owned(),
                start: None,
                end: None,
            }],
            confidence: 0,
            unresolved_fields: vec!["semanticType".to_owned()],
        })
        .collect::<Vec<_>>();
    if stages.is_empty() {
        stages.clear();
    }
    let duties = ["待确认", "待确认", "待确认", "待确认"].map(str::to_owned);
    RulesetV2 {
        schema_version: RULESET_V2_SCHEMA_VERSION.to_owned(),
        plan_version: MATCH_PLAN_V3_VERSION.to_owned(),
        ruleset_name: "人工补救草稿".to_owned(),
        source_text: source.to_owned(),
        affirmative_duties: duties.clone(),
        negative_duties: duties,
        stages,
        clock_pools: default_clock_pools(),
        source_clauses: source_clauses(source),
        coverage_entries: source_clauses(source)
            .into_iter()
            .map(|clause| CoverageEntryV3 {
                clause_id: clause.id,
                status: CoverageStatusV3::Unmapped,
                stage_ids: Vec::new(),
                note: "等待人工映射到环节或标记为说明条款".to_owned(),
            })
            .collect(),
        migration_review_required: true,
    }
}

fn speech_stage(
    id: &str,
    name: &str,
    semantic_type: SemanticStageType,
    speaker: SlotId,
    seconds: u32,
) -> StageDefinitionV2 {
    StageDefinitionV2 {
        id: id.to_owned(),
        display_name: name.to_owned(),
        semantic_type,
        participation_mode: ParticipationMode::Single,
        turn_mode: TurnMode::ContinuousSpeech,
        clock_mode: ClockMode::Individual,
        interrupt_policy: InterruptPolicyV2::Forbidden,
        fixed_speakers: vec![speaker],
        affirmative_candidates: Vec::new(),
        negative_candidates: Vec::new(),
        questioner: None,
        respondent: None,
        opening_side: Some(speaker.side()),
        duration_seconds: Some(seconds),
        affirmative_seconds: None,
        negative_seconds: None,
        question_seconds: None,
        answer_seconds: None,
        turn_seconds: None,
        follow_up_allowed: false,
        respondent_may_counter_question: false,
        max_rounds: None,
        ai_objective: match semantic_type {
            SemanticStageType::OpeningStatement => "建立定义、标准和本方论证框架",
            SemanticStageType::CrossExamSummary => "总结此前攻辩交锋并推进本方论证",
            SemanticStageType::ClosingSummary => "回收全场争点并比较双方论证责任",
            _ => "完成当前单人发言任务",
        }
        .to_owned(),
        scoring_focus: vec![
            "切题性".to_owned(),
            "论证质量".to_owned(),
            "逻辑".to_owned(),
        ],
        source_evidence: vec![preset_evidence(name)],
        confidence: 100,
        unresolved_fields: Vec::new(),
    }
}

fn cross_stage(id: &str, name: &str, questioner: SlotId, respondent: SlotId) -> StageDefinitionV2 {
    let affirmative_candidates = SlotId::ALL
        .into_iter()
        .filter(|slot| slot.side() == Side::Affirmative)
        .collect();
    let negative_candidates = SlotId::ALL
        .into_iter()
        .filter(|slot| slot.side() == Side::Negative)
        .collect();
    StageDefinitionV2 {
        id: id.to_owned(),
        display_name: name.to_owned(),
        semantic_type: SemanticStageType::CrossExamination,
        participation_mode: ParticipationMode::WholeTeam,
        turn_mode: TurnMode::OneWayQuestionAnswer,
        clock_mode: ClockMode::QuestionerPool,
        interrupt_policy: InterruptPolicyV2::QuestionerMayStopAnswer,
        fixed_speakers: vec![questioner],
        affirmative_candidates,
        negative_candidates,
        questioner: Some(questioner),
        respondent: Some(respondent),
        opening_side: Some(questioner.side()),
        duration_seconds: Some(180),
        affirmative_seconds: None,
        negative_seconds: None,
        question_seconds: None,
        answer_seconds: None,
        turn_seconds: None,
        follow_up_allowed: true,
        respondent_may_counter_question: false,
        max_rounds: None,
        ai_objective: "连续提问、追问并检验对方论证".to_owned(),
        scoring_focus: vec![
            "质询质量".to_owned(),
            "临场应对".to_owned(),
            "逻辑".to_owned(),
        ],
        source_evidence: vec![preset_evidence(name)],
        confidence: 100,
        unresolved_fields: Vec::new(),
    }
}

fn free_debate_stage() -> StageDefinitionV2 {
    StageDefinitionV2 {
        id: "free-debate".to_owned(),
        display_name: "自由辩".to_owned(),
        semantic_type: SemanticStageType::FreeDebate,
        participation_mode: ParticipationMode::WholeTeam,
        turn_mode: TurnMode::AlternatingTeams,
        clock_mode: ClockMode::TeamPool,
        interrupt_policy: InterruptPolicyV2::Forbidden,
        fixed_speakers: Vec::new(),
        affirmative_candidates: SlotId::ALL
            .into_iter()
            .filter(|slot| slot.side() == Side::Affirmative)
            .collect(),
        negative_candidates: SlotId::ALL
            .into_iter()
            .filter(|slot| slot.side() == Side::Negative)
            .collect(),
        questioner: None,
        respondent: None,
        opening_side: Some(Side::Affirmative),
        duration_seconds: None,
        affirmative_seconds: Some(240),
        negative_seconds: Some(240),
        question_seconds: None,
        answer_seconds: None,
        turn_seconds: None,
        follow_up_allowed: false,
        respondent_may_counter_question: false,
        max_rounds: None,
        ai_objective: "回应上一轮对方发言并推进本方未解决争点".to_owned(),
        scoring_focus: vec![
            "反驳".to_owned(),
            "临场应对".to_owned(),
            "团队协作".to_owned(),
        ],
        source_evidence: vec![preset_evidence("自由辩双方各 4 分钟，正方先发言")],
        confidence: 100,
        unresolved_fields: Vec::new(),
    }
}

fn preset_evidence(quote: &str) -> SourceEvidenceV2 {
    SourceEvidenceV2 {
        quote: quote.to_owned(),
        start: None,
        end: None,
    }
}

pub fn validate_ruleset_v2(ruleset: &RulesetV2) -> Vec<RuleIssueV2> {
    let mut issues = Vec::new();
    if ruleset.stages.is_empty() {
        issues.push(RuleIssueV2 {
            severity: IssueSeverity::Blocking,
            stage_id: String::new(),
            field: "stages".to_owned(),
            message: "赛制至少需要一个环节".to_owned(),
        });
        return issues;
    }
    if ruleset.plan_version == MATCH_PLAN_V3_VERSION {
        let unique_pools = ruleset
            .clock_pools
            .iter()
            .map(|pool| pool.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if ruleset.clock_pools.len() != 4
            || unique_pools.len() != 4
            || ruleset.clock_pools.iter().any(|pool| {
                pool.initial_seconds == 0
                    || pool
                        .unit_budget
                        .is_some_and(|units| units == 0 || units > 100_000)
            })
        {
            issues.push(RuleIssueV2 {
                severity: IssueSeverity::Blocking,
                stage_id: String::new(),
                field: "clockPools".to_owned(),
                message: "V3 赛制必须保留正反方主额度和自由辩额度四个有效池；字数额度须为 1–100000 的整数".to_owned(),
            });
        }
        let coverage = ruleset
            .coverage_entries
            .iter()
            .map(|entry| (entry.clause_id.as_str(), entry))
            .collect::<std::collections::HashMap<_, _>>();
        for clause in &ruleset.source_clauses {
            let valid = coverage.get(clause.id.as_str()).is_some_and(|entry| {
                matches!(
                    entry.status,
                    CoverageStatusV3::Mapped | CoverageStatusV3::Explanatory
                )
            });
            if !valid {
                issues.push(RuleIssueV2 {
                    severity: IssueSeverity::Blocking,
                    stage_id: clause.id.clone(),
                    field: "coverage".to_owned(),
                    message: format!("原文条款未完成映射：{}", clause.text),
                });
            }
        }
    }
    let mut ids = std::collections::HashSet::new();
    for stage in &ruleset.stages {
        if stage.id.trim().is_empty() || !ids.insert(stage.id.clone()) {
            issues.push(RuleIssueV2::blocking(stage, "id", "环节编号不能为空或重复"));
        }
        for field in &stage.unresolved_fields {
            issues.push(RuleIssueV2::blocking(stage, field, "该字段需要人工确认"));
        }
        if stage.display_name.contains("攻辩")
            && stage.semantic_type == SemanticStageType::Custom
            && !stage
                .unresolved_fields
                .iter()
                .any(|field| field == "turnMode")
        {
            issues.push(RuleIssueV2::blocking(
                stage,
                "turnMode",
                "攻辩必须确认是单向质询还是双方对辩",
            ));
        }
        match stage.semantic_type {
            SemanticStageType::FreeDebate => {
                if stage.participation_mode != ParticipationMode::WholeTeam {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "participationMode",
                        "自由辩必须允许全队参与",
                    ));
                }
                if stage.turn_mode != TurnMode::AlternatingTeams {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "turnMode",
                        "自由辩必须由双方团队交替",
                    ));
                }
                if stage.clock_mode != ClockMode::TeamPool
                    || stage.affirmative_seconds.unwrap_or(0) == 0
                    || stage.negative_seconds.unwrap_or(0) == 0
                {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "clock",
                        "自由辩必须设置双方团队共享时长",
                    ));
                }
                if stage.interrupt_policy != InterruptPolicyV2::Forbidden {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "interruptPolicy",
                        "自由辩不能被对手打断",
                    ));
                }
            }
            SemanticStageType::CrossExamination => {
                if stage.questioner.is_none() || stage.respondent.is_none() {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "participants",
                        "质询必须有问方和初始答方",
                    ));
                }
                if stage.duration_seconds.unwrap_or(0) == 0 {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "durationSeconds",
                        "质询必须设置共享时长",
                    ));
                }
                let opening_side = stage
                    .opening_side
                    .or_else(|| stage.questioner.map(|slot| slot.side()));
                let questioner_candidates = match opening_side {
                    Some(Side::Affirmative) => &stage.affirmative_candidates,
                    Some(Side::Negative) => &stage.negative_candidates,
                    None => &stage.fixed_speakers,
                };
                let respondent_candidates = match opening_side {
                    Some(Side::Affirmative) => &stage.negative_candidates,
                    Some(Side::Negative) => &stage.affirmative_candidates,
                    None => &stage.fixed_speakers,
                };
                if questioner_candidates.is_empty() || respondent_candidates.is_empty() {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "candidatePools",
                        "质询必须至少有一个可提问辩位和一个对方可回答辩位",
                    ));
                }
            }
            _ => {
                if stage.fixed_speakers.is_empty()
                    && stage.participation_mode == ParticipationMode::Single
                {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "fixedSpeakers",
                        "单人环节必须指定辩位",
                    ));
                }
                if matches!(stage.turn_mode, TurnMode::ContinuousSpeech)
                    && stage.duration_seconds.unwrap_or(0) == 0
                {
                    issues.push(RuleIssueV2::blocking(
                        stage,
                        "durationSeconds",
                        "连续陈述必须设置时长",
                    ));
                }
            }
        }
    }
    issues
}

pub fn compile_v2_to_v1(ruleset: &RulesetV2) -> Result<RulesetV1, Vec<RuleIssueV2>> {
    let issues = validate_ruleset_v2(ruleset);
    if issues
        .iter()
        .any(|issue| issue.severity == IssueSeverity::Blocking)
    {
        return Err(issues);
    }
    let stages = ruleset.stages.iter().map(compile_stage).collect::<Vec<_>>();
    Ok(RulesetV1 {
        schema_version: RULESET_SCHEMA_VERSION.to_owned(),
        ruleset_name: ruleset.ruleset_name.clone(),
        source_text: ruleset.source_text.clone(),
        affirmative_duties: ruleset.affirmative_duties.clone(),
        negative_duties: ruleset.negative_duties.clone(),
        stages,
    })
}

pub fn ruleset_v2_from_v1(ruleset: &RulesetV1) -> RulesetV2 {
    let mut stages = Vec::new();
    let mut index = 0;
    while index < ruleset.stages.len() {
        let stage = &ruleset.stages[index];
        if stage.kind == StageKind::FreeDebate {
            let mut affirmative_seconds = None;
            let mut negative_seconds = None;
            let mut consumed = 0;
            while let Some(candidate) = ruleset.stages.get(index + consumed) {
                if candidate.kind != StageKind::FreeDebate || consumed >= 2 {
                    break;
                }
                for (speaker_index, slot) in candidate.speakers.iter().enumerate() {
                    let seconds = candidate
                        .speaker_durations
                        .get(speaker_index)
                        .copied()
                        .unwrap_or(candidate.duration_seconds);
                    match slot.side() {
                        Side::Affirmative => affirmative_seconds = Some(seconds),
                        Side::Negative => negative_seconds = Some(seconds),
                    }
                }
                consumed += 1;
            }
            let missing_side = affirmative_seconds.is_none() || negative_seconds.is_none();
            stages.push(StageDefinitionV2 {
                id: stage.id.clone(),
                display_name: "自由辩".to_owned(),
                semantic_type: SemanticStageType::FreeDebate,
                participation_mode: ParticipationMode::WholeTeam,
                turn_mode: TurnMode::AlternatingTeams,
                clock_mode: ClockMode::TeamPool,
                interrupt_policy: InterruptPolicyV2::Forbidden,
                fixed_speakers: Vec::new(),
                affirmative_candidates: SlotId::ALL
                    .into_iter()
                    .filter(|slot| slot.side() == Side::Affirmative)
                    .collect(),
                negative_candidates: SlotId::ALL
                    .into_iter()
                    .filter(|slot| slot.side() == Side::Negative)
                    .collect(),
                questioner: None,
                respondent: None,
                opening_side: stage.speakers.first().map(|slot| slot.side()),
                duration_seconds: None,
                affirmative_seconds,
                negative_seconds,
                question_seconds: None,
                answer_seconds: None,
                turn_seconds: Some(30),
                follow_up_allowed: false,
                respondent_may_counter_question: false,
                max_rounds: None,
                ai_objective: "回应上一轮对方发言并推进本方未解决争点".to_owned(),
                scoring_focus: vec![
                    "反驳".to_owned(),
                    "临场应对".to_owned(),
                    "团队协作".to_owned(),
                ],
                source_evidence: vec![SourceEvidenceV2 {
                    quote: stage.name.clone(),
                    start: None,
                    end: None,
                }],
                confidence: if missing_side { 40 } else { 85 },
                unresolved_fields: if missing_side {
                    vec!["affirmativeSeconds/negativeSeconds".to_owned()]
                } else {
                    Vec::new()
                },
            });
            index += consumed.max(1);
            continue;
        }
        stages.push(stage_v2_from_v1(stage));
        index += 1;
    }
    let mut migrated = RulesetV2 {
        schema_version: RULESET_V2_SCHEMA_VERSION.to_owned(),
        plan_version: MATCH_PLAN_V3_VERSION.to_owned(),
        ruleset_name: ruleset.ruleset_name.clone(),
        source_text: ruleset.source_text.clone(),
        affirmative_duties: ruleset.affirmative_duties.clone(),
        negative_duties: ruleset.negative_duties.clone(),
        stages,
        clock_pools: default_clock_pools(),
        source_clauses: Vec::new(),
        coverage_entries: Vec::new(),
        migration_review_required: true,
    };
    if let Some(free) = migrated
        .stages
        .iter()
        .find(|stage| stage.semantic_type == SemanticStageType::FreeDebate)
    {
        for pool in &mut migrated.clock_pools {
            if pool.kind == ClockPoolKindV3::FreeDebate {
                pool.initial_seconds = match pool.side {
                    Side::Affirmative => free.affirmative_seconds.unwrap_or(240),
                    Side::Negative => free.negative_seconds.unwrap_or(240),
                };
            }
        }
    }
    populate_coverage(&mut migrated, false);
    migrated
}

fn stage_v2_from_v1(stage: &StageDefinitionV1) -> StageDefinitionV2 {
    let semantic_type = if stage.name.contains("小结") {
        SemanticStageType::CrossExamSummary
    } else if stage.name.contains("对辩") {
        SemanticStageType::PairedDebate
    } else if stage.name.contains("驳论") || stage.name.contains("反驳") {
        SemanticStageType::Rebuttal
    } else if stage.name.contains("申论") || stage.name.contains("展开") {
        SemanticStageType::CaseExtension
    } else {
        match stage.kind {
            StageKind::CrossExam => SemanticStageType::CrossExamination,
            StageKind::Summary => SemanticStageType::ClosingSummary,
            StageKind::Opening => SemanticStageType::OpeningStatement,
            StageKind::Custom => SemanticStageType::Custom,
            StageKind::FreeDebate => unreachable!("free debate is merged separately"),
        }
    };
    let is_cross = semantic_type == SemanticStageType::CrossExamination;
    let is_pair = semantic_type == SemanticStageType::PairedDebate;
    let questioner = stage.questioner.or_else(|| {
        if is_cross {
            stage.speakers.first().copied()
        } else {
            None
        }
    });
    let respondent = stage.respondent.or_else(|| {
        if is_cross {
            stage
                .eligible_respondents
                .first()
                .copied()
                .or_else(|| stage.speakers.get(1).copied())
        } else {
            None
        }
    });
    let mut affirmative_candidates = Vec::new();
    let mut negative_candidates = Vec::new();
    for slot in stage
        .eligible_questioners
        .iter()
        .chain(stage.eligible_respondents.iter())
    {
        let target = if slot.side() == Side::Affirmative {
            &mut affirmative_candidates
        } else {
            &mut negative_candidates
        };
        if !target.contains(slot) {
            target.push(*slot);
        }
    }
    for slot in [questioner, respondent].into_iter().flatten() {
        let target = if slot.side() == Side::Affirmative {
            &mut affirmative_candidates
        } else {
            &mut negative_candidates
        };
        if !target.contains(&slot) {
            target.push(slot);
        }
    }
    let alternates_sides = stage
        .eligible_questioners
        .iter()
        .any(|slot| slot.side() == Side::Affirmative)
        && stage
            .eligible_questioners
            .iter()
            .any(|slot| slot.side() == Side::Negative);
    let ambiguous_attack = stage.name.contains("攻辩") && stage.kind != StageKind::CrossExam;
    StageDefinitionV2 {
        id: stage.id.clone(),
        display_name: stage.name.clone(),
        semantic_type: if ambiguous_attack {
            SemanticStageType::Custom
        } else {
            semantic_type
        },
        participation_mode: if is_cross {
            ParticipationMode::CandidatePool
        } else if is_pair {
            ParticipationMode::FixedPair
        } else {
            ParticipationMode::Single
        },
        turn_mode: if is_cross && alternates_sides {
            TurnMode::AlternatingTeams
        } else if is_cross {
            TurnMode::OneWayQuestionAnswer
        } else if is_pair {
            TurnMode::AlternatingPair
        } else {
            TurnMode::ContinuousSpeech
        },
        clock_mode: if is_cross {
            ClockMode::QuestionerPool
        } else if is_pair {
            ClockMode::SeparateSides
        } else {
            ClockMode::Individual
        },
        interrupt_policy: if stage.interruptible {
            InterruptPolicyV2::QuestionerMayStopAnswer
        } else {
            InterruptPolicyV2::Forbidden
        },
        fixed_speakers: stage.speakers.clone(),
        affirmative_candidates,
        negative_candidates,
        questioner,
        respondent,
        opening_side: stage
            .initial_questioning_side
            .or_else(|| questioner.map(|slot| slot.side())),
        duration_seconds: Some(stage.duration_seconds),
        affirmative_seconds: if is_pair {
            stage
                .speakers
                .iter()
                .zip(stage.speaker_durations.iter())
                .find(|(slot, _)| slot.side() == Side::Affirmative)
                .map(|(_, seconds)| *seconds)
        } else {
            None
        },
        negative_seconds: if is_pair {
            stage
                .speakers
                .iter()
                .zip(stage.speaker_durations.iter())
                .find(|(slot, _)| slot.side() == Side::Negative)
                .map(|(_, seconds)| *seconds)
        } else {
            None
        },
        question_seconds: None,
        answer_seconds: None,
        turn_seconds: None,
        follow_up_allowed: stage.follow_up_allowed,
        respondent_may_counter_question: false,
        max_rounds: stage.max_cross_exam_rounds,
        ai_objective: match semantic_type {
            SemanticStageType::CrossExamination => "连续提问并检验对方论证".to_owned(),
            SemanticStageType::ClosingSummary => "总结全场争点并完成价值比较".to_owned(),
            SemanticStageType::Rebuttal => "回应对方论证并建立反驳链".to_owned(),
            _ => "完成本环节职责并推进本方论证".to_owned(),
        },
        scoring_focus: vec![
            "切题性".to_owned(),
            "论证质量".to_owned(),
            "逻辑".to_owned(),
        ],
        source_evidence: vec![SourceEvidenceV2 {
            quote: stage.name.clone(),
            start: None,
            end: None,
        }],
        confidence: if ambiguous_attack { 45 } else { 80 },
        unresolved_fields: if ambiguous_attack {
            vec!["turnMode".to_owned()]
        } else {
            Vec::new()
        },
    }
}

fn compile_stage(stage: &StageDefinitionV2) -> StageDefinitionV1 {
    let kind = match stage.semantic_type {
        SemanticStageType::CrossExamination => StageKind::CrossExam,
        SemanticStageType::FreeDebate => StageKind::FreeDebate,
        SemanticStageType::ClosingSummary => StageKind::Summary,
        SemanticStageType::Custom | SemanticStageType::PairedDebate => StageKind::Custom,
        _ => StageKind::Opening,
    };
    let fallback = stage
        .fixed_speakers
        .first()
        .copied()
        .or(stage.questioner)
        .or_else(|| stage.opening_side.map(first_slot))
        .unwrap_or(SlotId::Aff1);
    let speakers = if kind == StageKind::FreeDebate {
        // V2 runtime treats free debate as one dynamic session rather than one event per slot.
        vec![fallback]
    } else if stage.fixed_speakers.is_empty() {
        vec![fallback]
    } else {
        stage.fixed_speakers.clone()
    };
    let duration = if kind == StageKind::FreeDebate {
        stage
            .affirmative_seconds
            .or(stage.negative_seconds)
            .unwrap_or(1)
    } else {
        stage
            .duration_seconds
            .or(stage.affirmative_seconds)
            .or(stage.negative_seconds)
            .unwrap_or(1)
    };
    let speaker_durations = if stage.semantic_type == SemanticStageType::PairedDebate {
        speakers
            .iter()
            .map(|slot| match slot.side() {
                Side::Affirmative => stage.affirmative_seconds.unwrap_or(duration),
                Side::Negative => stage.negative_seconds.unwrap_or(duration),
            })
            .collect()
    } else {
        vec![duration; speakers.len()]
    };
    let mut cross_candidates = stage.affirmative_candidates.clone();
    cross_candidates.extend(stage.negative_candidates.iter().copied());
    let eligible_questioners = if kind == StageKind::CrossExam {
        if stage.turn_mode == TurnMode::AlternatingTeams {
            cross_candidates.clone()
        } else {
            let questioning_side = stage
                .opening_side
                .or_else(|| stage.questioner.map(|slot| slot.side()));
            cross_candidates
                .iter()
                .copied()
                .filter(|slot| Some(slot.side()) == questioning_side)
                .collect()
        }
    } else {
        Vec::new()
    };
    let eligible_respondents = if kind == StageKind::CrossExam {
        cross_candidates
    } else {
        Vec::new()
    };
    StageDefinitionV1 {
        id: stage.id.clone(),
        name: stage.display_name.clone(),
        kind,
        speaker_durations,
        speakers,
        clock_owner: match stage.clock_mode {
            ClockMode::Individual => ClockOwner::Individual,
            ClockMode::QuestionerPool => ClockOwner::Questioner,
            ClockMode::SeparateSides | ClockMode::TeamPool => ClockOwner::Team,
            ClockMode::SharedStage => ClockOwner::Shared,
        },
        duration_seconds: duration,
        early_end_allowed: true,
        follow_up_allowed: stage.follow_up_allowed,
        interruptible: stage.interrupt_policy == InterruptPolicyV2::QuestionerMayStopAnswer,
        interrupt_limit: if stage.interrupt_policy == InterruptPolicyV2::QuestionerMayStopAnswer {
            32
        } else {
            0
        },
        questioner: stage.questioner,
        respondent: stage.respondent,
        eligible_questioners,
        eligible_respondents,
        initial_questioning_side: if kind == StageKind::CrossExam {
            stage.opening_side
        } else {
            None
        },
        max_cross_exam_rounds: stage.max_rounds,
        weight: 1,
        repeat: 1,
    }
}

fn first_slot(side: Side) -> SlotId {
    match side {
        Side::Affirmative => SlotId::Aff1,
        Side::Negative => SlotId::Neg1,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compile_v2_to_v1, manual_ruleset_v2, ruleset_v2_from_v1, traditional_four_on_four_v2,
        validate_ruleset_v2, ClockMode, CoverageStatusV3, InterruptPolicyV2, IssueSeverity,
        SemanticStageType, MATCH_PLAN_V3_VERSION,
    };
    use crate::domain::ruleset::StageKind;

    #[test]
    fn traditional_four_on_four_v2_has_all_semantic_stages() {
        let ruleset = traditional_four_on_four_v2();
        assert_eq!(ruleset.stages.len(), 13);
        assert_eq!(ruleset.plan_version, MATCH_PLAN_V3_VERSION);
        assert_eq!(ruleset.clock_pools.len(), 4);
        assert_eq!(ruleset.clock_pools[0].initial_seconds, 1_080);
        assert_eq!(ruleset.clock_pools[2].initial_seconds, 240);
        assert_eq!(ruleset.source_clauses.len(), ruleset.coverage_entries.len());
        assert!(ruleset.coverage_entries.iter().all(|entry| matches!(
            entry.status,
            CoverageStatusV3::Mapped | CoverageStatusV3::Explanatory
        )));
        assert!(validate_ruleset_v2(&ruleset).is_empty());
        assert_eq!(ruleset.stages[0].display_name, "正方一辩陈词");
        assert_eq!(
            ruleset.stages[10].semantic_type,
            SemanticStageType::FreeDebate
        );
        assert_eq!(ruleset.stages[11].display_name, "反方四辩总结陈词");
        assert_eq!(ruleset.stages[12].display_name, "正方四辩总结陈词");
    }

    #[test]
    fn ambiguous_attack_defense_blocks_confirmation() {
        let mut ruleset = traditional_four_on_four_v2();
        ruleset.stages[2].semantic_type = SemanticStageType::Custom;
        ruleset.stages[2].unresolved_fields = vec!["turnMode".to_owned()];
        let issues = validate_ruleset_v2(&ruleset);
        assert!(issues.iter().any(|issue| {
            issue.severity == IssueSeverity::Blocking && issue.field == "turnMode"
        }));
        assert!(compile_v2_to_v1(&ruleset).is_err());
    }

    #[test]
    fn free_debate_requires_two_team_budgets_and_forbids_interrupt() {
        let mut ruleset = traditional_four_on_four_v2();
        let free = &mut ruleset.stages[10];
        assert_eq!(free.clock_mode, ClockMode::TeamPool);
        assert_eq!(free.affirmative_seconds, Some(240));
        assert_eq!(free.negative_seconds, Some(240));
        assert_eq!(free.turn_seconds, None);
        assert_eq!(free.interrupt_policy, InterruptPolicyV2::Forbidden);
        free.negative_seconds = None;
        free.interrupt_policy = InterruptPolicyV2::QuestionerMayStopAnswer;
        let issues = validate_ruleset_v2(&ruleset);
        assert!(issues.iter().any(|issue| issue.field == "clock"));
        assert!(issues.iter().any(|issue| issue.field == "interruptPolicy"));
    }

    #[test]
    fn v2_compiler_preserves_stage_order_and_keeps_free_debate_dynamic() {
        let compiled = compile_v2_to_v1(&traditional_four_on_four_v2()).expect("compile");
        assert_eq!(compiled.stages.len(), 13);
        assert_eq!(compiled.stages[0].name, "正方一辩陈词");
        assert_eq!(compiled.stages[1].eligible_questioners.len(), 4);
        assert_eq!(compiled.stages[1].eligible_respondents.len(), 8);
        assert_eq!(compiled.stages[10].kind, StageKind::FreeDebate);
        assert_eq!(compiled.stages[10].speakers.len(), 1);
        assert_eq!(compiled.stages[12].name, "正方四辩总结陈词");
    }

    #[test]
    fn v1_migration_merges_two_free_debate_team_limits() {
        let source = "正方一辩立论3分钟；自由辩正方4分钟；反方5分钟；反方四辩总结4分钟";
        let legacy = crate::domain::rule_times::ruleset_from_text(source).expect("legacy parse");
        let migrated = ruleset_v2_from_v1(&legacy);
        let free = migrated
            .stages
            .iter()
            .find(|stage| stage.semantic_type == SemanticStageType::FreeDebate)
            .expect("free debate");
        assert_eq!(free.affirmative_seconds, Some(240));
        assert_eq!(free.negative_seconds, Some(300));
        assert_eq!(free.turn_seconds, Some(30));
        let compiled = compile_v2_to_v1(&migrated).expect("compiled migration");
        assert_eq!(
            compiled
                .stages
                .iter()
                .filter(|stage| stage.kind == StageKind::FreeDebate)
                .count(),
            1
        );
    }

    #[test]
    fn failed_recognition_keeps_source_as_editable_blocking_draft() {
        let draft = manual_ruleset_v2("第一阶段陈词；第二阶段交锋");
        assert_eq!(draft.stages.len(), 2);
        assert_eq!(draft.stages[0].source_evidence[0].quote, "第一阶段陈词");
        assert!(validate_ruleset_v2(&draft)
            .iter()
            .any(|issue| issue.field == "semanticType"));
        assert!(compile_v2_to_v1(&draft).is_err());
    }
}
