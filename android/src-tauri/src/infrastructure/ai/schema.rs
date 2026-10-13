use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::ruleset::{sample_ruleset, RulesetV1, Side, SlotId};
use crate::error::{AppError, ErrorCode};

use super::types::{Purpose, DEFAULT_MODEL_ID, PROMPT_BUNDLE_VERSION};

pub const PARSE_SCHEMA: &str = "parse-rules.v1";
pub const TOPIC_SCHEMA: &str = "topic-analysis.v1";
pub const STRATEGY_SCHEMA: &str = "team-strategy.v1";
pub const TURN_SCHEMA: &str = "generated-turn.v1";
pub const CROSS_EXAM_SELECTION_SCHEMA: &str = "cross-exam-selection.v1";
pub const INTERRUPT_SCHEMA: &str = "interruption-decision.v1";
pub const SCORE_SCHEMA: &str = "stage-score.v1";
pub const REVIEW_SCHEMA: &str = "match-review.v1";
pub const JUDGE_SCHEMA: &str = "match-judgement.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseRulesOutputV1 {
    pub schema_version: String,
    pub ruleset: RulesetV1,
    #[serde(default, deserialize_with = "null_default_u32")]
    pub confidence: u32,
    #[serde(default, deserialize_with = "null_default_note_vec")]
    pub field_notes: Vec<String>,
}

fn null_default_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u32>::deserialize(deserializer)?.unwrap_or(0))
}

fn null_default_string_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

fn null_default_note_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<serde_json::Value>>::deserialize(deserializer)?
        .unwrap_or_default()
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(text) => text,
            other => other.to_string(),
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicAnalysisV1 {
    #[serde(default = "default_topic_schema")]
    pub schema_version: String,
    pub topic_type: String,
    #[serde(default, deserialize_with = "null_default_string_vec")]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub affirmative_burden: String,
    #[serde(default)]
    pub negative_burden: String,
    #[serde(default, deserialize_with = "null_default_string_vec")]
    pub contentions: Vec<String>,
    pub summary: String,
}

fn default_topic_schema() -> String {
    TOPIC_SCHEMA.to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamStrategyV1 {
    #[serde(default = "default_strategy_schema")]
    pub schema_version: String,
    pub side: Side,
    pub thesis: String,
    #[serde(default, deserialize_with = "null_default_string_vec")]
    pub arguments: Vec<String>,
    #[serde(default, deserialize_with = "null_default_string_vec")]
    pub risks: Vec<String>,
}

fn default_strategy_schema() -> String {
    STRATEGY_SCHEMA.to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedTurnV1 {
    #[serde(default = "default_turn_schema")]
    pub schema_version: String,
    pub slot: SlotId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossExamSelectionV1 {
    #[serde(default = "default_cross_exam_selection_schema")]
    pub schema_version: String,
    pub questioner: SlotId,
    pub respondent: SlotId,
    pub selection_summary: String,
}

fn default_cross_exam_selection_schema() -> String {
    CROSS_EXAM_SELECTION_SCHEMA.to_owned()
}

fn default_turn_schema() -> String {
    TURN_SCHEMA.to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptionDecisionV1 {
    pub schema_version: String,
    pub interrupt: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDimensionV1 {
    pub name: String,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageScoreV1 {
    pub schema_version: String,
    pub transcript_ids: Vec<Uuid>,
    pub dimensions: Vec<ScoreDimensionV1>,
    pub strength: String,
    pub improvement: String,
    #[serde(default)]
    pub material_support: String,
    pub confidence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewAdjustmentV1 {
    pub dimension: String,
    pub delta: i32,
    pub transcript_id: Uuid,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReviewV1 {
    pub schema_version: String,
    pub adjustments: Vec<ReviewAdjustmentV1>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgementWinner {
    Affirmative,
    Negative,
    Tie,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchJudgementV1 {
    pub schema_version: String,
    pub winner: JudgementWinner,
    pub margin: u32,
    pub confidence: u32,
    pub summary: String,
    pub model_id: String,
    pub prompt_bundle_version: String,
}

pub fn fixture_json(purpose: Purpose) -> String {
    match purpose {
        Purpose::ParseRules => to_json(&parse_fixture()),
        Purpose::AnalyzeTopic => to_json(&topic_fixture()),
        Purpose::BuildStrategy => to_json(&strategy_fixture()),
        Purpose::SelectCrossExam => to_json(&cross_exam_selection_fixture()),
        Purpose::GenerateTurn => to_json(&turn_fixture()),
        Purpose::DecideInterruption => to_json(&interrupt_fixture()),
        Purpose::ScoreStage => to_json(&score_fixture()),
        Purpose::ReviewMatch => to_json(&review_fixture()),
        Purpose::JudgeMatch => to_json(&judge_fixture()),
    }
}

pub fn ensure_parse(value: &ParseRulesOutputV1) -> Result<(), AppError> {
    version(&value.schema_version, PARSE_SCHEMA)?;
    percent(value.confidence)?;
    if value.ruleset.stages.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_topic(value: &TopicAnalysisV1) -> Result<(), AppError> {
    version(&value.schema_version, TOPIC_SCHEMA)?;
    if !matches!(
        value.topic_type.as_str(),
        "fact"
            | "value"
            | "policy"
            | "mixed"
            | "事实"
            | "事实辩题"
            | "价值"
            | "价值辩题"
            | "政策"
            | "政策辩题"
            | "混合"
            | "混合辩题"
    ) || value.summary.is_empty()
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_strategy(value: &TeamStrategyV1) -> Result<(), AppError> {
    version(&value.schema_version, STRATEGY_SCHEMA)?;
    if value.arguments.is_empty() || value.thesis.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_turn(value: &GeneratedTurnV1) -> Result<(), AppError> {
    version(&value.schema_version, TURN_SCHEMA)?;
    if value.text.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_cross_exam_selection(value: &CrossExamSelectionV1) -> Result<(), AppError> {
    version(&value.schema_version, CROSS_EXAM_SELECTION_SCHEMA)?;
    if value.selection_summary.trim().is_empty()
        || value.selection_summary.chars().count() > 80
        || value.questioner.side() == value.respondent.side()
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_interrupt(value: &InterruptionDecisionV1) -> Result<(), AppError> {
    version(&value.schema_version, INTERRUPT_SCHEMA)?;
    if value.reason.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_score(value: &StageScoreV1) -> Result<(), AppError> {
    version(&value.schema_version, SCORE_SCHEMA)?;
    percent(value.confidence)?;
    if value.transcript_ids.is_empty()
        || value.dimensions.is_empty()
        || value.strength.is_empty()
        || value.improvement.is_empty()
        || value
            .dimensions
            .iter()
            .any(|dimension| dimension.name.is_empty() || dimension.score > 100)
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_review(value: &MatchReviewV1) -> Result<(), AppError> {
    version(&value.schema_version, REVIEW_SCHEMA)?;
    if value.summary.is_empty()
        || value
            .adjustments
            .iter()
            .any(|adjustment| !(-8..=8).contains(&adjustment.delta) || adjustment.reason.is_empty())
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn ensure_judge(value: &MatchJudgementV1) -> Result<(), AppError> {
    version(&value.schema_version, JUDGE_SCHEMA)?;
    percent(value.margin)?;
    percent(value.confidence)?;
    if value.summary.is_empty() || value.prompt_bundle_version != PROMPT_BUNDLE_VERSION {
        return Err(invalid());
    }
    Ok(())
}

fn parse_fixture() -> ParseRulesOutputV1 {
    ParseRulesOutputV1 {
        schema_version: PARSE_SCHEMA.to_owned(),
        ruleset: sample_ruleset(),
        confidence: 90,
        field_notes: vec!["duration from the sample rules".to_owned()],
    }
}

fn topic_fixture() -> TopicAnalysisV1 {
    TopicAnalysisV1 {
        schema_version: TOPIC_SCHEMA.to_owned(),
        topic_type: "policy".to_owned(),
        concepts: vec!["人工智能".to_owned()],
        affirmative_burden: "证明应当限制".to_owned(),
        negative_burden: "证明不应禁止".to_owned(),
        contentions: vec!["安全".to_owned(), "创新".to_owned()],
        summary: "政策辩题".to_owned(),
    }
}

fn strategy_fixture() -> TeamStrategyV1 {
    TeamStrategyV1 {
        schema_version: STRATEGY_SCHEMA.to_owned(),
        side: Side::Affirmative,
        thesis: "应当限制高风险用途".to_owned(),
        arguments: vec!["风险".to_owned(), "责任".to_owned()],
        risks: vec!["定义过宽".to_owned()],
    }
}

fn turn_fixture() -> GeneratedTurnV1 {
    GeneratedTurnV1 {
        schema_version: TURN_SCHEMA.to_owned(),
        slot: SlotId::Aff1,
        text: "我方认为应当谨慎限制。".to_owned(),
    }
}

fn cross_exam_selection_fixture() -> CrossExamSelectionV1 {
    CrossExamSelectionV1 {
        schema_version: CROSS_EXAM_SELECTION_SCHEMA.to_owned(),
        questioner: SlotId::Aff1,
        respondent: SlotId::Neg1,
        selection_summary: "正一负责定义，反一提出了待澄清主张".to_owned(),
    }
}

fn interrupt_fixture() -> InterruptionDecisionV1 {
    InterruptionDecisionV1 {
        schema_version: INTERRUPT_SCHEMA.to_owned(),
        interrupt: false,
        reason: "当前环节不允许打断".to_owned(),
    }
}

fn score_fixture() -> StageScoreV1 {
    StageScoreV1 {
        schema_version: SCORE_SCHEMA.to_owned(),
        transcript_ids: vec![Uuid::from_u128(3)],
        dimensions: vec![ScoreDimensionV1 {
            name: "argument".to_owned(),
            score: 70,
        }],
        strength: "结构清楚".to_owned(),
        improvement: "补上材料引用".to_owned(),
        material_support: "无材料支持".to_owned(),
        confidence: 60,
    }
}

fn review_fixture() -> MatchReviewV1 {
    MatchReviewV1 {
        schema_version: REVIEW_SCHEMA.to_owned(),
        adjustments: vec![ReviewAdjustmentV1 {
            dimension: "argument".to_owned(),
            delta: 2,
            transcript_id: Uuid::from_u128(3),
            reason: "回收了立论".to_owned(),
        }],
        summary: "小幅上调".to_owned(),
    }
}

fn judge_fixture() -> MatchJudgementV1 {
    MatchJudgementV1 {
        schema_version: JUDGE_SCHEMA.to_owned(),
        winner: JudgementWinner::Tie,
        margin: 10,
        confidence: 50,
        summary: "难分胜负".to_owned(),
        model_id: DEFAULT_MODEL_ID.to_owned(),
        prompt_bundle_version: PROMPT_BUNDLE_VERSION.to_owned(),
    }
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("fixture json")
}

fn version(actual: &str, expected: &str) -> Result<(), AppError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn percent(value: u32) -> Result<(), AppError> {
    if value <= 100 {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn invalid() -> AppError {
    AppError::new(ErrorCode::InvalidArgument, "model output is invalid")
}

#[cfg(test)]
mod null_tolerance_tests {
    use super::{
        ensure_strategy, ensure_topic, ParseRulesOutputV1, TeamStrategyV1, TopicAnalysisV1,
        PARSE_SCHEMA, STRATEGY_SCHEMA, TOPIC_SCHEMA,
    };
    use crate::domain::ruleset::Side;

    #[test]
    fn unknown_numeric_rule_fields_become_zero_for_later_blocking_validation() {
        let output = ParseRulesOutputV1 {
            schema_version: PARSE_SCHEMA.to_owned(),
            ruleset: crate::domain::ruleset::sample_ruleset(),
            confidence: 90,
            field_notes: Vec::new(),
        };
        let mut value = serde_json::to_value(output).unwrap();
        value["confidence"] = serde_json::Value::Null;
        value["ruleset"]["stages"][0]["durationSeconds"] = serde_json::Value::Null;
        value["ruleset"]["stages"][0]["speakerDurations"] = serde_json::json!([null]);
        value["ruleset"]["stages"][0]["interruptible"] = serde_json::Value::Null;
        value["ruleset"]["stages"][0]["eligibleQuestioners"] = serde_json::Value::Null;
        value["fieldNotes"] = serde_json::json!([
            "普通备注",
            {"field":"stage-1.duration","note":"原文未给时长"}
        ]);
        let parsed: ParseRulesOutputV1 = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.confidence, 0);
        assert_eq!(parsed.ruleset.stages[0].duration_seconds, 0);
        assert_eq!(parsed.ruleset.stages[0].speaker_durations, vec![0]);
        assert!(!parsed.ruleset.stages[0].interruptible);
        assert!(parsed.ruleset.stages[0].eligible_questioners.is_empty());
        assert_eq!(parsed.field_notes[0], "普通备注");
        assert!(parsed.field_notes[1].contains("stage-1.duration"));
    }

    #[test]
    fn chinese_topic_labels_and_longer_nonempty_strategy_lists_remain_usable() {
        let topic = TopicAnalysisV1 {
            schema_version: TOPIC_SCHEMA.to_owned(),
            topic_type: "政策辩题".to_owned(),
            concepts: Vec::new(),
            affirmative_burden: "证明应限制".to_owned(),
            negative_burden: "证明不应限制".to_owned(),
            contentions: Vec::new(),
            summary: "围绕风险治理展开".to_owned(),
        };
        assert!(ensure_topic(&topic).is_ok());
        let strategy = TeamStrategyV1 {
            schema_version: STRATEGY_SCHEMA.to_owned(),
            side: Side::Affirmative,
            thesis: "限制高风险用途".to_owned(),
            arguments: (0..9).map(|index| format!("论证{index}")).collect(),
            risks: Vec::new(),
        };
        assert!(ensure_strategy(&strategy).is_ok());
    }
}
