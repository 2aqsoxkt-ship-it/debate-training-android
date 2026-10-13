use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::ruleset::{sample_ruleset, MatchMode, RulesetV1, Side, SlotId};

pub const SETUP_SCHEMA_VERSION: &str = "match-setup.v1";
pub const ASSIGNMENT_SCHEMA_VERSION: &str = "debater-assignment.v1";
pub const EVENT_SCHEMA_VERSION: &str = "match-event.v1";
pub const TRANSCRIPT_SCHEMA_VERSION: &str = "transcript-entry.v1";
pub const CROSS_EXAM_EXCHANGE_SCHEMA_VERSION: &str = "cross-exam-exchange.v1";
pub const CROSS_EXAM_SESSION_SCHEMA_VERSION: &str = "cross-exam-session.v1";
pub const FREE_DEBATE_SESSION_SCHEMA_VERSION: &str = "free-debate-session.v1";
pub const METRIC_SCHEMA_VERSION: &str = "turn-metric-summary.v1";
pub const EVALUATION_SCHEMA_VERSION: &str = "stage-evaluation.v1";
pub const REPORT_SCHEMA_VERSION: &str = "match-report.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Controller {
    Human,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebaterAssignmentV1 {
    pub schema_version: String,
    pub slot: SlotId,
    pub controller: Controller,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSetupV1 {
    pub schema_version: String,
    pub mode: MatchMode,
    pub topic: String,
    pub materials: String,
    pub speech_rate_per_minute: u32,
    pub ruleset: RulesetV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchEventKind {
    StateChanged,
    SpeechSubmitted,
    AiWait,
    InterruptRejected,
    Takeover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchEventV1 {
    pub schema_version: String,
    pub id: Uuid,
    pub sequence: u32,
    pub kind: MatchEventKind,
    pub slot: Option<SlotId>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntryV1 {
    pub schema_version: String,
    pub id: Uuid,
    pub event_id: Uuid,
    pub slot: SlotId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<Controller>,
    pub full_text: String,
    pub effective_text: String,
    pub effective_units: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_cap_at_submission: Option<u32>,
    pub overflow_text: String,
    #[serde(default)]
    pub cross_exam_exchange_id: Option<Uuid>,
    #[serde(default)]
    pub response_target_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_exam_turn_kind: Option<CrossExamTurnKind>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub cross_exam_round_number: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossExamTurnKind {
    Question,
    Answer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossExamExchangeStatus {
    Selected,
    QuestionSubmitted,
    Answered,
    Interrupted,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossExamExchangeV1 {
    pub schema_version: String,
    pub id: Uuid,
    pub event_id: Uuid,
    pub questioner: SlotId,
    pub respondent: SlotId,
    pub question_text: String,
    pub answer_transcript_id: Option<Uuid>,
    pub status: CrossExamExchangeStatus,
    pub displayed_answer_units: u32,
    pub interrupted_reason: String,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub round_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questioning_side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questioner_controller: Option<Controller>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respondent_controller: Option<Controller>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_exchange_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_transcript_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub question_input_ms: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub answer_input_ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub selection_summary: String,
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossExamSessionPhase {
    Selecting,
    QuestionPending,
    AnswerPending,
    Revealing,
    BetweenExchanges,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossExamSessionEndReason {
    Timeout,
    UserEnded,
    RoundLimit,
    LegacySingleRound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossExamSessionV1 {
    pub schema_version: String,
    pub event_id: Uuid,
    pub phase: CrossExamSessionPhase,
    pub next_side: Side,
    pub total_ms: u64,
    pub used_ms: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub total_units: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub used_units: u32,
    pub round_number: u32,
    pub active_exchange_id: Option<Uuid>,
    pub exchange_ids: Vec<Uuid>,
    pub end_reason: Option<CrossExamSessionEndReason>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeDebateSessionV1 {
    pub schema_version: String,
    pub event_id: Uuid,
    pub affirmative_total_ms: u64,
    pub negative_total_ms: u64,
    pub affirmative_remaining_ms: u64,
    pub negative_remaining_ms: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub affirmative_total_units: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub negative_total_units: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub affirmative_remaining_units: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub negative_remaining_units: u32,
    pub next_side: Side,
    pub round_number: u32,
    pub completed: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnMetricSummaryV1 {
    pub schema_version: String,
    pub turn_id: Uuid,
    pub forward_typed_units: u32,
    pub effective_positive_units: u32,
    pub reverse_edited_units: u32,
    pub paste_units: u32,
    pub pause_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageEvaluationV1 {
    pub schema_version: String,
    pub stage_id: String,
    pub transcript_ids: Vec<Uuid>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReportV1 {
    pub schema_version: String,
    pub match_id: Uuid,
    pub formula_version: String,
    pub standard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractBundleV1 {
    pub ruleset: RulesetV1,
    pub match_setup: MatchSetupV1,
    pub assignment: DebaterAssignmentV1,
    pub match_event: MatchEventV1,
    pub transcript_entry: TranscriptEntryV1,
    pub cross_exam_exchange: CrossExamExchangeV1,
    pub turn_metric_summary: TurnMetricSummaryV1,
    pub stage_evaluation: StageEvaluationV1,
    pub match_report: MatchReportV1,
}

pub fn sample_contract_bundle() -> ContractBundleV1 {
    let ruleset = sample_ruleset();
    let match_id = Uuid::from_u128(1);
    let event_id = Uuid::from_u128(2);
    let transcript_id = Uuid::from_u128(3);
    ContractBundleV1 {
        match_setup: MatchSetupV1 {
            schema_version: SETUP_SCHEMA_VERSION.to_owned(),
            mode: MatchMode::Training,
            topic: "人工智能应被禁止".to_owned(),
            materials: "闭卷材料".to_owned(),
            speech_rate_per_minute: 240,
            ruleset: ruleset.clone(),
        },
        assignment: DebaterAssignmentV1 {
            schema_version: ASSIGNMENT_SCHEMA_VERSION.to_owned(),
            slot: SlotId::Aff1,
            controller: Controller::Human,
        },
        match_event: MatchEventV1 {
            schema_version: EVENT_SCHEMA_VERSION.to_owned(),
            id: event_id,
            sequence: 1,
            kind: MatchEventKind::SpeechSubmitted,
            slot: Some(SlotId::Aff1),
            summary: "提交陈词".to_owned(),
        },
        transcript_entry: TranscriptEntryV1 {
            schema_version: TRANSCRIPT_SCHEMA_VERSION.to_owned(),
            id: transcript_id,
            event_id,
            slot: SlotId::Aff1,
            controller: None,
            full_text: "你好，世界".to_owned(),
            effective_text: "你好".to_owned(),
            effective_units: 2,
            word_cap_at_submission: None,
            overflow_text: String::new(),
            cross_exam_exchange_id: None,
            response_target_text: String::new(),
            cross_exam_turn_kind: None,
            cross_exam_round_number: 0,
        },
        cross_exam_exchange: CrossExamExchangeV1 {
            schema_version: CROSS_EXAM_EXCHANGE_SCHEMA_VERSION.to_owned(),
            id: Uuid::from_u128(4),
            event_id,
            questioner: SlotId::Aff3,
            respondent: SlotId::Neg3,
            question_text: "请定义高风险用途".to_owned(),
            answer_transcript_id: None,
            status: CrossExamExchangeStatus::QuestionSubmitted,
            displayed_answer_units: 0,
            interrupted_reason: String::new(),
            round_number: 0,
            questioning_side: None,
            questioner_controller: None,
            respondent_controller: None,
            previous_exchange_id: None,
            question_transcript_id: None,
            question_input_ms: 0,
            answer_input_ms: 0,
            selection_summary: String::new(),
        },
        turn_metric_summary: TurnMetricSummaryV1 {
            schema_version: METRIC_SCHEMA_VERSION.to_owned(),
            turn_id: event_id,
            forward_typed_units: 2,
            effective_positive_units: 2,
            reverse_edited_units: 0,
            paste_units: 0,
            pause_ms: 0,
        },
        stage_evaluation: StageEvaluationV1 {
            schema_version: EVALUATION_SCHEMA_VERSION.to_owned(),
            stage_id: "opening".to_owned(),
            transcript_ids: vec![transcript_id],
            summary: "完成立论".to_owned(),
        },
        match_report: MatchReportV1 {
            schema_version: REPORT_SCHEMA_VERSION.to_owned(),
            match_id,
            formula_version: "score.v1".to_owned(),
            standard: true,
        },
        ruleset,
    }
}

#[cfg(test)]
mod tests {
    use super::sample_contract_bundle;

    const FIXTURE: &str = include_str!("../../../../tests/fixtures/contracts/bundle-v1.json");

    #[test]
    fn rust_contract_matches_shared_fixture() {
        let expected: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture json");
        let actual = serde_json::to_value(sample_contract_bundle()).expect("serialize");
        assert_eq!(actual, expected);
    }
}
