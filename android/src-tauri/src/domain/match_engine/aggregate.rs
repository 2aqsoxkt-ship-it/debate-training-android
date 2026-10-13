use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::events::{
    Controller, CrossExamExchangeStatus, CrossExamExchangeV1, CrossExamSessionEndReason,
    CrossExamSessionPhase, CrossExamSessionV1, CrossExamTurnKind, DebaterAssignmentV1,
    FreeDebateSessionV1, MatchEventKind, MatchEventV1, TranscriptEntryV1,
    ASSIGNMENT_SCHEMA_VERSION, CROSS_EXAM_EXCHANGE_SCHEMA_VERSION,
    CROSS_EXAM_SESSION_SCHEMA_VERSION, EVENT_SCHEMA_VERSION, FREE_DEBATE_SESSION_SCHEMA_VERSION,
    TRANSCRIPT_SCHEMA_VERSION,
};
use crate::domain::ruleset::{
    expand_schedule, MatchMode, RulesetV1, ScheduleError, Side, SlotId, StageKind,
};
use crate::domain::ruleset_v2::{compile_v2_to_v1, ClockPoolKindV3, RulesetV2};
use crate::domain::text_units::{count_effective_units, split_effective};

pub const MAIN_TEAM_POOL_MS: u64 = 18 * 60 * 1_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchResourceMode {
    #[default]
    Time,
    Words,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordBudgetsV1 {
    pub affirmative_total_units: u32,
    pub negative_total_units: u32,
    pub affirmative_remaining_units: u32,
    pub negative_remaining_units: u32,
}

impl WordBudgetsV1 {
    fn new(speech_rate_per_minute: u32) -> Self {
        let total = speech_rate_per_minute.saturating_mul(18);
        Self {
            affirmative_total_units: total,
            negative_total_units: total,
            affirmative_remaining_units: total,
            negative_remaining_units: total,
        }
    }
}

const fn default_main_pool_ms() -> u64 {
    MAIN_TEAM_POOL_MS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchPhase {
    DraftSetup,
    RulesParsed,
    RulesConfirmed,
    TopicAnalyzed,
    ControlAssigned,
    Calibrated,
    LineupConfirmed,
    Ready,
    InProgress,
    MatchComplete,
    Scoring,
    ReportReady,
    RecoveryRequired,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandReject {
    #[error("rules are not confirmed")]
    RulesNotConfirmed,
    #[error("lineup is not confirmed")]
    LineupNotConfirmed,
    #[error("transition is not allowed")]
    InvalidTransition,
    #[error("free debate rejects every interrupt")]
    FreeDebateInterruptForbidden,
    #[error("this event cannot be interrupted")]
    InterruptNotAllowed,
    #[error("scoring mode cannot take over the other team")]
    CrossTeamTakeoverForbidden,
    #[error("scoring takeover needs a second confirmation")]
    TakeoverNeedsConfirmation,
    #[error("only the current schedule event can run")]
    WrongEvent,
    #[error("the eight debater slots must stay intact")]
    SlotInvariant,
    #[error("scoring mode requires calibration")]
    CalibrationRequired,
    #[error("the schedule cannot be executed")]
    ScheduleNotExecutable,
    #[error("the match is not ready")]
    MatchNotReady,
}

#[derive(Debug, Clone)]
pub enum MatchCommand {
    SubmitParsedRules {
        ruleset: Box<RulesetV1>,
    },
    ConfirmRules,
    RecordTopicAnalysis {
        summary: String,
    },
    AssignControl {
        user_side: Option<Side>,
        assignments: Vec<DebaterAssignmentV1>,
    },
    RecordCalibration,
    ConfirmLineup,
    MarkReady,
    Start,
    SetTopic {
        topic: String,
    },
    SetMaterials {
        materials: String,
    },
    SetSpeechRate {
        speech_rate_per_minute: u32,
    },
    SubmitCurrent {
        event_id: Uuid,
        text: String,
        input_ms: u64,
    },
    Interrupt,
    RequestTakeover {
        slot_index: usize,
        second_confirmed: bool,
    },
    NoteAiWait {
        waited_ms: u64,
    },
    CompleteMatch,
    BeginScoring,
    PublishReport,
    MarkRecoveryRequired,
    AbortMatch,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanScoringStateV3 {
    pub in_cap_units: u32,
    pub cap_units: u32,
    pub overflow_units: u32,
    pub reverse_units: u32,
    pub reverse_actions: u32,
    pub forward_units: u32,
    pub pause_ms: u64,
    pub input_ms: u64,
    pub baseline_milli_cpm: Option<u64>,
    pub calibration_samples_milli_cpm: Vec<u64>,
    pub low_confidence: bool,
    pub pasted_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationStateV1 {
    pub difficulty: crate::domain::visibility::Difficulty,
    pub strategies: Option<crate::domain::visibility::StrategyPacket>,
    pub role_packets: Option<Vec<crate::domain::debate_context::RolePacket>>,
}

#[derive(Debug, Clone)]
pub struct MatchAggregate {
    pub id: Uuid,
    pub mode: MatchMode,
    pub metering_mode: MatchResourceMode,
    pub word_budgets: Option<WordBudgetsV1>,
    pub phase: MatchPhase,
    pub topic: String,
    pub materials: String,
    pub speech_rate_per_minute: u32,
    pub ruleset: Option<RulesetV1>,
    pub semantic_ruleset: Option<RulesetV2>,
    pub rules_hash: Option<String>,
    pub lineup_hash: Option<String>,
    pub user_side: Option<Side>,
    pub assignments: [DebaterAssignmentV1; 8],
    pub current_control_overrides: Vec<crate::domain::control_policy::ControlOverride>,
    pub control_revision: u64,
    pub calibrated: bool,
    pub non_standard: bool,
    pub schedule: Vec<crate::domain::ruleset::ScheduledEvent>,
    pub cursor: usize,
    pub charged_ms: u64,
    pub ai_wait_ms: u64,
    pub affirmative_main_remaining_ms: u64,
    pub negative_main_remaining_ms: u64,
    pub events: Vec<MatchEventV1>,
    pub transcripts: Vec<TranscriptEntryV1>,
    pub cross_exam_exchanges: Vec<CrossExamExchangeV1>,
    pub cross_exam_sessions: Vec<CrossExamSessionV1>,
    pub free_debate_sessions: Vec<FreeDebateSessionV1>,
    pub scoring_state: Option<HumanScoringStateV3>,
    pub preparation: Option<PreparationStateV1>,
    pub public_response_links: Vec<crate::domain::issue_ledger::PublicResponseLink>,
    next_salt: u128,
    next_sequence: u32,
}

impl MatchAggregate {
    pub fn new(
        id: Uuid,
        mode: MatchMode,
        topic: impl Into<String>,
        materials: impl Into<String>,
        speech_rate_per_minute: u32,
    ) -> Self {
        Self {
            id,
            mode,
            metering_mode: MatchResourceMode::Words,
            word_budgets: Some(WordBudgetsV1::new(speech_rate_per_minute)),
            phase: MatchPhase::DraftSetup,
            topic: topic.into(),
            materials: materials.into(),
            speech_rate_per_minute,
            ruleset: None,
            semantic_ruleset: None,
            rules_hash: None,
            lineup_hash: None,
            user_side: None,
            assignments: default_assignments(),
            current_control_overrides: Vec::new(),
            control_revision: 0,
            calibrated: false,
            non_standard: false,
            schedule: Vec::new(),
            cursor: 0,
            charged_ms: 0,
            ai_wait_ms: 0,
            affirmative_main_remaining_ms: MAIN_TEAM_POOL_MS,
            negative_main_remaining_ms: MAIN_TEAM_POOL_MS,
            events: Vec::new(),
            transcripts: Vec::new(),
            cross_exam_exchanges: Vec::new(),
            cross_exam_sessions: Vec::new(),
            free_debate_sessions: Vec::new(),
            scoring_state: None,
            preparation: None,
            public_response_links: Vec::new(),
            next_salt: 1_000,
            next_sequence: 1,
        }
    }

    pub fn reseed_ids(&mut self, salt: u128) {
        self.next_salt = salt.max(1);
    }

    pub fn slots(&self) -> [SlotId; 8] {
        SlotId::ALL
    }

    pub fn main_clock_remaining(&self, side: Side) -> u64 {
        match side {
            Side::Affirmative => self.affirmative_main_remaining_ms,
            Side::Negative => self.negative_main_remaining_ms,
        }
    }

    pub fn remaining_main_units(&self, side: Side) -> u32 {
        if self.metering_mode == MatchResourceMode::Time {
            return self.units_for_millis(self.main_clock_remaining(side));
        }
        self.word_budgets.as_ref().map_or(0, |budgets| match side {
            Side::Affirmative => budgets.affirmative_remaining_units,
            Side::Negative => budgets.negative_remaining_units,
        })
    }

    fn units_for_seconds(&self, seconds: u32) -> u32 {
        (u64::from(seconds).saturating_mul(u64::from(self.speech_rate_per_minute)) / 60)
            .min(u64::from(u32::MAX)) as u32
    }

    fn units_for_millis(&self, millis: u64) -> u32 {
        (millis.saturating_mul(u64::from(self.speech_rate_per_minute)) / 60_000)
            .min(u64::from(u32::MAX)) as u32
    }

    fn charge_main_units(&mut self, side: Side, units: u32) {
        if let Some(budgets) = self.word_budgets.as_mut() {
            let remaining = match side {
                Side::Affirmative => &mut budgets.affirmative_remaining_units,
                Side::Negative => &mut budgets.negative_remaining_units,
            };
            *remaining = remaining.saturating_sub(units);
        }
    }

    pub fn current_remaining_units(&self) -> u32 {
        let Some(event) = self.schedule.get(self.cursor) else {
            return 0;
        };
        if event.kind == StageKind::FreeDebate {
            return self.current_free_debate_units().map_or(
                event.word_cap,
                |(affirmative, negative, side)| match side {
                    Side::Affirmative => affirmative,
                    Side::Negative => negative,
                },
            );
        }
        let side = if event.kind == StageKind::CrossExam {
            self.current_cross_exam_roles()
                .map(|roles| roles.0.side())
                .unwrap_or(event.speaker.side())
        } else {
            event.speaker.side()
        };
        let main = self.remaining_main_units(side);
        if self.metering_mode == MatchResourceMode::Words && event.kind == StageKind::CrossExam {
            if let Some(session) = self
                .cross_exam_sessions
                .iter()
                .find(|session| session.event_id == event.id)
            {
                return main.min(session.total_units.saturating_sub(session.used_units));
            }
        }
        main
    }

    pub fn current_word_cap(&self) -> u32 {
        let Some(event) = self.schedule.get(self.cursor) else {
            return 0;
        };
        if self.metering_mode == MatchResourceMode::Words {
            let remaining_units = self.current_remaining_units();
            if event.kind == StageKind::FreeDebate {
                remaining_units
            } else {
                event.word_cap.min(remaining_units)
            }
        } else {
            event.word_cap
        }
    }

    fn cross_exam_questioning_side(
        &self,
        stage: &crate::domain::ruleset::StageDefinitionV1,
        event_id: Uuid,
    ) -> Side {
        let side = self
            .cross_exam_sessions
            .iter()
            .find(|session| session.event_id == event_id)
            .map(|session| session.next_side)
            .or(stage.initial_questioning_side)
            .or_else(|| stage.questioner.map(|slot| slot.side()))
            .unwrap_or(Side::Affirmative);
        if self.metering_mode == MatchResourceMode::Words && self.remaining_main_units(side) == 0 {
            let other = opposite_side(side);
            if stage
                .eligible_questioners
                .iter()
                .any(|slot| slot.side() == other)
                && self.remaining_main_units(other) > 0
            {
                return other;
            }
        }
        side
    }

    pub fn skip_exhausted_main_events(&mut self) {
        if self.metering_mode != MatchResourceMode::Words || self.phase != MatchPhase::InProgress {
            return;
        }
        while let Some(event) = self.schedule.get(self.cursor).cloned() {
            if event.kind == StageKind::FreeDebate {
                break;
            }
            let side = if event.kind == StageKind::CrossExam {
                self.ruleset
                    .as_ref()
                    .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
                    .map(|stage| self.cross_exam_questioning_side(stage, event.id))
                    .unwrap_or(event.speaker.side())
            } else {
                event.speaker.side()
            };
            if self.remaining_main_units(side) > 0 {
                if let Some(session) = self
                    .cross_exam_sessions
                    .iter_mut()
                    .find(|session| session.event_id == event.id)
                {
                    session.next_side = side;
                }
                break;
            }
            if let Some(session) = self
                .cross_exam_sessions
                .iter_mut()
                .find(|session| session.event_id == event.id)
            {
                session.phase = CrossExamSessionPhase::Completed;
                session.active_exchange_id = None;
                session.end_reason = Some(CrossExamSessionEndReason::Timeout);
                session.revision = session.revision.saturating_add(1);
            }
            self.cursor = self.cursor.saturating_add(1);
            self.push_event(
                MatchEventKind::StateChanged,
                None,
                "stage skipped because team word budget is exhausted",
            );
        }
    }

    fn charge_main_clock(&mut self, side: Side, elapsed_ms: u64) {
        let remaining = match side {
            Side::Affirmative => &mut self.affirmative_main_remaining_ms,
            Side::Negative => &mut self.negative_main_remaining_ms,
        };
        *remaining = remaining.saturating_sub(elapsed_ms);
    }

    pub fn submit_semantic_rules(&mut self, ruleset: RulesetV2) -> Result<(), CommandReject> {
        let compiled =
            compile_v2_to_v1(&ruleset).map_err(|_| CommandReject::ScheduleNotExecutable)?;
        self.submit_rules(compiled)?;
        self.apply_v3_main_pools(&ruleset);
        self.semantic_ruleset = Some(ruleset);
        Ok(())
    }

    pub fn replace_semantic_draft(&mut self, ruleset: RulesetV2) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        self.apply_v3_main_pools(&ruleset);
        self.ruleset = compile_v2_to_v1(&ruleset).ok();
        self.semantic_ruleset = Some(ruleset);
        self.rules_hash = None;
        self.clear_preparation_outputs();
        self.schedule.clear();
        self.revoke_lineup();
        self.phase = MatchPhase::RulesParsed;
        self.push_event(MatchEventKind::StateChanged, None, "semantic rules edited");
        Ok(())
    }

    fn apply_v3_main_pools(&mut self, ruleset: &RulesetV2) {
        if self.metering_mode == MatchResourceMode::Words {
            self.word_budgets = Some(WordBudgetsV1::new(self.speech_rate_per_minute));
        }
        for pool in ruleset
            .clock_pools
            .iter()
            .filter(|pool| pool.kind == ClockPoolKindV3::Main)
        {
            let millis = u64::from(pool.initial_seconds).saturating_mul(1_000);
            let units = pool
                .unit_budget
                .unwrap_or_else(|| self.units_for_seconds(pool.initial_seconds));
            match pool.side {
                Side::Affirmative => {
                    self.affirmative_main_remaining_ms = millis;
                    if let Some(budgets) = self.word_budgets.as_mut() {
                        budgets.affirmative_total_units = units;
                        budgets.affirmative_remaining_units = units;
                    }
                }
                Side::Negative => {
                    self.negative_main_remaining_ms = millis;
                    if let Some(budgets) = self.word_budgets.as_mut() {
                        budgets.negative_total_units = units;
                        budgets.negative_remaining_units = units;
                    }
                }
            }
        }
    }

    pub fn handle(&mut self, command: MatchCommand) -> Result<(), CommandReject> {
        match command {
            MatchCommand::SubmitParsedRules { ruleset } => self.submit_rules(*ruleset),
            MatchCommand::ConfirmRules => self.confirm_rules(),
            MatchCommand::RecordTopicAnalysis { summary } => self.record_analysis(summary),
            MatchCommand::AssignControl {
                user_side,
                assignments,
            } => self.assign_control(user_side, assignments),
            MatchCommand::RecordCalibration => self.record_calibration(),
            MatchCommand::ConfirmLineup => self.confirm_lineup(),
            MatchCommand::MarkReady => self.mark_ready(),
            MatchCommand::Start => self.start(),
            MatchCommand::SetTopic { topic } => self.set_topic(topic),
            MatchCommand::SetMaterials { materials } => self.set_materials(materials),
            MatchCommand::SetSpeechRate {
                speech_rate_per_minute,
            } => self.set_speech_rate(speech_rate_per_minute),
            MatchCommand::SubmitCurrent {
                event_id,
                text,
                input_ms,
            } => self.submit_current(event_id, text, input_ms),
            MatchCommand::Interrupt => self.interrupt(),
            MatchCommand::RequestTakeover {
                slot_index,
                second_confirmed,
            } => self.request_takeover(slot_index, second_confirmed),
            MatchCommand::NoteAiWait { waited_ms } => self.note_ai_wait(waited_ms),
            MatchCommand::CompleteMatch => self.complete_match(),
            MatchCommand::BeginScoring => self.begin_scoring(),
            MatchCommand::PublishReport => self.publish_report(),
            MatchCommand::MarkRecoveryRequired => self.mark_recovery(),
            MatchCommand::AbortMatch => self.abort_match(),
        }
    }

    fn submit_rules(&mut self, ruleset: RulesetV1) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        if self.metering_mode == MatchResourceMode::Words {
            self.word_budgets = Some(WordBudgetsV1::new(self.speech_rate_per_minute));
        }
        self.ruleset = Some(ruleset);
        self.semantic_ruleset = None;
        self.rules_hash = None;
        self.clear_preparation_outputs();
        self.schedule.clear();
        self.revoke_lineup();
        self.phase = MatchPhase::RulesParsed;
        self.push_event(MatchEventKind::StateChanged, None, "rules parsed");
        Ok(())
    }

    fn confirm_rules(&mut self) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        let ruleset = self
            .ruleset
            .clone()
            .ok_or(CommandReject::InvalidTransition)?;
        let mut salt = self.next_salt;
        let schedule = expand_schedule(&ruleset, self.speech_rate_per_minute, || {
            salt += 1;
            Uuid::from_u128(salt)
        })
        .map_err(schedule_reject)?;
        self.next_salt = salt;
        self.schedule = schedule;
        self.cursor = 0;
        self.rules_hash = Some(match &self.semantic_ruleset {
            Some(semantic) => content_hash(semantic),
            None => content_hash(&ruleset),
        });
        self.clear_preparation_outputs();
        self.calibrated = false;
        self.revoke_lineup();
        self.phase = MatchPhase::RulesConfirmed;
        self.push_event(MatchEventKind::StateChanged, None, "rules confirmed");
        Ok(())
    }

    fn record_analysis(&mut self, summary: String) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::RulesConfirmed {
            return Err(CommandReject::RulesNotConfirmed);
        }
        self.phase = MatchPhase::TopicAnalyzed;
        self.push_event(MatchEventKind::StateChanged, None, &summary);
        Ok(())
    }

    fn assign_control(
        &mut self,
        user_side: Option<Side>,
        assignments: Vec<DebaterAssignmentV1>,
    ) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        if !matches!(
            self.phase,
            MatchPhase::TopicAnalyzed
                | MatchPhase::ControlAssigned
                | MatchPhase::Calibrated
                | MatchPhase::LineupConfirmed
                | MatchPhase::Ready
        ) {
            return Err(CommandReject::InvalidTransition);
        }
        let normalized = normalize_assignments(assignments)?;
        if self.mode == MatchMode::Scoring {
            let side = user_side.ok_or(CommandReject::SlotInvariant)?;
            let humans = normalized
                .iter()
                .filter(|assignment| assignment.controller == Controller::Human)
                .count();
            if !(1..=4).contains(&humans) {
                return Err(CommandReject::SlotInvariant);
            }
            if normalized.iter().any(|assignment| {
                assignment.controller == Controller::Human && assignment.slot.side() != side
            }) {
                return Err(CommandReject::CrossTeamTakeoverForbidden);
            }
        }
        self.user_side = user_side;
        self.assignments = normalized;
        self.revoke_lineup();
        if !matches!(self.phase, MatchPhase::Calibrated) {
            self.phase = MatchPhase::ControlAssigned;
        }
        self.push_event(MatchEventKind::StateChanged, None, "control assigned");
        Ok(())
    }

    fn record_calibration(&mut self) -> Result<(), CommandReject> {
        if self.mode != MatchMode::Scoring || self.phase != MatchPhase::ControlAssigned {
            return Err(CommandReject::InvalidTransition);
        }
        self.calibrated = true;
        self.phase = MatchPhase::Calibrated;
        self.push_event(MatchEventKind::StateChanged, None, "calibrated");
        Ok(())
    }

    fn confirm_lineup(&mut self) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        if self.rules_hash.is_none() {
            return Err(CommandReject::RulesNotConfirmed);
        }
        if self.topic.trim().is_empty() {
            return Err(CommandReject::InvalidTransition);
        }
        let control_ready = match self.mode {
            MatchMode::Training => self.phase == MatchPhase::ControlAssigned,
            MatchMode::Scoring => self.phase == MatchPhase::Calibrated,
        };
        if !control_ready {
            return Err(if self.mode == MatchMode::Scoring {
                CommandReject::CalibrationRequired
            } else {
                CommandReject::InvalidTransition
            });
        }
        let lineup_hash = {
            let seal = LineupSeal {
                rules_hash: self.rules_hash.as_deref().unwrap_or_default(),
                topic: &self.topic,
                materials: &self.materials,
                speech_rate_per_minute: self.speech_rate_per_minute,
                assignments: &self.assignments,
                event_ids: self.schedule.iter().map(|event| event.id).collect(),
            };
            content_hash(&seal)
        };
        self.lineup_hash = Some(lineup_hash);
        self.phase = MatchPhase::LineupConfirmed;
        self.push_event(MatchEventKind::StateChanged, None, "lineup confirmed");
        Ok(())
    }

    fn mark_ready(&mut self) -> Result<(), CommandReject> {
        if self.rules_hash.is_none() {
            return Err(CommandReject::RulesNotConfirmed);
        }
        if self.lineup_hash.is_none() {
            return Err(CommandReject::LineupNotConfirmed);
        }
        if self.phase != MatchPhase::LineupConfirmed {
            return Err(CommandReject::MatchNotReady);
        }
        self.phase = MatchPhase::Ready;
        self.push_event(MatchEventKind::StateChanged, None, "ready");
        Ok(())
    }

    fn start(&mut self) -> Result<(), CommandReject> {
        if self.rules_hash.is_none() {
            return Err(CommandReject::RulesNotConfirmed);
        }
        if self.lineup_hash.is_none() {
            return Err(CommandReject::LineupNotConfirmed);
        }
        if self.phase != MatchPhase::Ready {
            return Err(CommandReject::MatchNotReady);
        }
        self.phase = MatchPhase::InProgress;
        self.push_event(MatchEventKind::StateChanged, None, "started");
        self.skip_exhausted_main_events();
        Ok(())
    }

    fn set_topic(&mut self, topic: String) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        if self.topic == topic {
            return Ok(());
        }
        self.topic = topic;
        self.invalidate_preparation()?;
        self.push_event(MatchEventKind::StateChanged, None, "topic changed");
        Ok(())
    }

    fn set_materials(&mut self, materials: String) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        if self.materials == materials {
            return Ok(());
        }
        self.materials = materials;
        self.invalidate_preparation()?;
        self.push_event(MatchEventKind::StateChanged, None, "materials changed");
        Ok(())
    }

    fn set_speech_rate(&mut self, speech_rate_per_minute: u32) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        if speech_rate_per_minute == 0 {
            return Err(CommandReject::InvalidTransition);
        }
        self.speech_rate_per_minute = speech_rate_per_minute;
        if let Some(ruleset) = self.semantic_ruleset.clone() {
            self.apply_v3_main_pools(&ruleset);
        } else if self.metering_mode == MatchResourceMode::Words {
            self.word_budgets = Some(WordBudgetsV1::new(speech_rate_per_minute));
        }
        if let Some(ruleset) = self.ruleset.clone() {
            if self.rules_hash.is_some() {
                let mut salt = self.next_salt;
                self.schedule = expand_schedule(&ruleset, speech_rate_per_minute, || {
                    salt += 1;
                    Uuid::from_u128(salt)
                })
                .map_err(schedule_reject)?;
                self.next_salt = salt;
                self.cursor = 0;
            }
        }
        self.revoke_lineup();
        self.push_event(MatchEventKind::StateChanged, None, "speech rate changed");
        Ok(())
    }

    fn submit_current(
        &mut self,
        event_id: Uuid,
        text: String,
        input_ms: u64,
    ) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let current = self
            .schedule
            .get(self.cursor)
            .ok_or(CommandReject::InvalidTransition)?;
        if current.id != event_id {
            return Err(CommandReject::WrongEvent);
        }
        let speaker = current.speaker;
        self.append_current_transcript(event_id, speaker, text, input_ms, None, String::new())?;
        Ok(())
    }

    pub fn current_free_debate_side(&self) -> Option<Side> {
        let event = self.schedule.get(self.cursor)?;
        if event.kind != StageKind::FreeDebate {
            return None;
        }
        self.free_debate_sessions
            .iter()
            .find(|session| session.event_id == event.id)
            .map(|session| session.next_side)
            .or_else(|| {
                self.semantic_ruleset
                    .as_ref()?
                    .stages
                    .iter()
                    .find(|stage| stage.id == event.stage_id)?
                    .opening_side
            })
            .or(Some(Side::Affirmative))
    }

    pub fn current_free_debate_clock(&self) -> Option<(u64, u64, Side)> {
        let event = self.schedule.get(self.cursor)?;
        if event.kind != StageKind::FreeDebate {
            return None;
        }
        if let Some(session) = self
            .free_debate_sessions
            .iter()
            .find(|session| session.event_id == event.id)
        {
            return Some((
                session.affirmative_remaining_ms,
                session.negative_remaining_ms,
                session.next_side,
            ));
        }
        let stage = self
            .semantic_ruleset
            .as_ref()?
            .stages
            .iter()
            .find(|stage| stage.id == event.stage_id)?;
        Some((
            u64::from(stage.affirmative_seconds.unwrap_or(0)).saturating_mul(1_000),
            u64::from(stage.negative_seconds.unwrap_or(0)).saturating_mul(1_000),
            stage.opening_side.unwrap_or(Side::Affirmative),
        ))
    }

    fn free_debate_initial_units(
        &self,
        event: &crate::domain::ruleset::ScheduledEvent,
    ) -> Option<(u32, u32, Side)> {
        let ruleset = self.semantic_ruleset.as_ref()?;
        let stage = ruleset
            .stages
            .iter()
            .find(|stage| stage.id == event.stage_id)?;
        let pool_units = |side, seconds| {
            ruleset
                .clock_pools
                .iter()
                .find(|pool| pool.kind == ClockPoolKindV3::FreeDebate && pool.side == side)
                .map(|pool| {
                    pool.unit_budget
                        .unwrap_or_else(|| self.units_for_seconds(pool.initial_seconds))
                })
                .unwrap_or_else(|| self.units_for_seconds(seconds))
        };
        Some((
            pool_units(Side::Affirmative, stage.affirmative_seconds.unwrap_or(0)),
            pool_units(Side::Negative, stage.negative_seconds.unwrap_or(0)),
            stage.opening_side.unwrap_or(Side::Affirmative),
        ))
    }

    pub fn free_debate_remaining_units(&self, side: Side) -> u32 {
        if let Some((affirmative, negative, _)) = self.current_free_debate_units() {
            return match side {
                Side::Affirmative => affirmative,
                Side::Negative => negative,
            };
        }
        if let Some(session) = self.free_debate_sessions.last() {
            return match (self.metering_mode, side) {
                (MatchResourceMode::Words, Side::Affirmative) => {
                    session.affirmative_remaining_units
                }
                (MatchResourceMode::Words, Side::Negative) => session.negative_remaining_units,
                (MatchResourceMode::Time, Side::Affirmative) => {
                    self.units_for_millis(session.affirmative_remaining_ms)
                }
                (MatchResourceMode::Time, Side::Negative) => {
                    self.units_for_millis(session.negative_remaining_ms)
                }
            };
        }
        if let Some(ruleset) = self.semantic_ruleset.as_ref() {
            if let Some(pool) = ruleset
                .clock_pools
                .iter()
                .find(|pool| pool.kind == ClockPoolKindV3::FreeDebate && pool.side == side)
            {
                if self.metering_mode == MatchResourceMode::Words {
                    if let Some(units) = pool.unit_budget {
                        return units;
                    }
                }
                return self.units_for_seconds(pool.initial_seconds);
            }
            if let Some(seconds) = ruleset
                .stages
                .iter()
                .find(|stage| {
                    stage.semantic_type == crate::domain::ruleset_v2::SemanticStageType::FreeDebate
                })
                .and_then(|stage| match side {
                    Side::Affirmative => stage.affirmative_seconds,
                    Side::Negative => stage.negative_seconds,
                })
            {
                return self.units_for_seconds(seconds);
            }
        }
        self.units_for_seconds(4 * 60)
    }

    pub fn current_free_debate_units(&self) -> Option<(u32, u32, Side)> {
        let event = self.schedule.get(self.cursor)?;
        if event.kind != StageKind::FreeDebate {
            return None;
        }
        if self.metering_mode == MatchResourceMode::Time {
            return self
                .current_free_debate_clock()
                .map(|(affirmative, negative, side)| {
                    (
                        self.units_for_millis(affirmative),
                        self.units_for_millis(negative),
                        side,
                    )
                });
        }
        if let Some(session) = self
            .free_debate_sessions
            .iter()
            .find(|session| session.event_id == event.id)
        {
            return Some((
                session.affirmative_remaining_units,
                session.negative_remaining_units,
                session.next_side,
            ));
        }
        self.free_debate_initial_units(event)
    }

    fn ensure_free_debate_session(&mut self) -> Result<usize, CommandReject> {
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        if event.kind != StageKind::FreeDebate {
            return Err(CommandReject::InvalidTransition);
        }
        if let Some(index) = self
            .free_debate_sessions
            .iter()
            .position(|session| session.event_id == event.id)
        {
            return Ok(index);
        }
        let stage = self
            .semantic_ruleset
            .as_ref()
            .and_then(|ruleset| {
                ruleset
                    .stages
                    .iter()
                    .find(|stage| stage.id == event.stage_id)
            })
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let affirmative_total_ms =
            u64::from(stage.affirmative_seconds.unwrap_or(0)).saturating_mul(1_000);
        let negative_total_ms =
            u64::from(stage.negative_seconds.unwrap_or(0)).saturating_mul(1_000);
        if affirmative_total_ms == 0 || negative_total_ms == 0 {
            return Err(CommandReject::ScheduleNotExecutable);
        }
        let (affirmative_total_units, negative_total_units, opening_side) = self
            .free_debate_initial_units(&event)
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let next_side = if self.metering_mode == MatchResourceMode::Words {
            match opening_side {
                Side::Affirmative if affirmative_total_units == 0 && negative_total_units > 0 => {
                    Side::Negative
                }
                Side::Negative if negative_total_units == 0 && affirmative_total_units > 0 => {
                    Side::Affirmative
                }
                side => side,
            }
        } else {
            opening_side
        };
        self.free_debate_sessions.push(FreeDebateSessionV1 {
            schema_version: FREE_DEBATE_SESSION_SCHEMA_VERSION.to_owned(),
            event_id: event.id,
            affirmative_total_ms,
            negative_total_ms,
            affirmative_remaining_ms: affirmative_total_ms,
            negative_remaining_ms: negative_total_ms,
            affirmative_total_units,
            negative_total_units,
            affirmative_remaining_units: affirmative_total_units,
            negative_remaining_units: negative_total_units,
            next_side,
            round_number: 0,
            completed: false,
            revision: 1,
        });
        Ok(self.free_debate_sessions.len() - 1)
    }

    pub fn submit_free_debate_turn(
        &mut self,
        speaker: SlotId,
        text: String,
        elapsed_ms: u64,
    ) -> Result<Uuid, CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        if event.kind != StageKind::FreeDebate {
            return Err(CommandReject::InvalidTransition);
        }
        let session_index = self.ensure_free_debate_session()?;
        let expected_side = self.free_debate_sessions[session_index].next_side;
        if speaker.side() != expected_side {
            return Err(CommandReject::CrossTeamTakeoverForbidden);
        }
        let remaining_ms = match expected_side {
            Side::Affirmative => self.free_debate_sessions[session_index].affirmative_remaining_ms,
            Side::Negative => self.free_debate_sessions[session_index].negative_remaining_ms,
        };
        let remaining_units = match expected_side {
            Side::Affirmative => {
                self.free_debate_sessions[session_index].affirmative_remaining_units
            }
            Side::Negative => self.free_debate_sessions[session_index].negative_remaining_units,
        };
        if (self.metering_mode == MatchResourceMode::Time && remaining_ms == 0)
            || (self.metering_mode == MatchResourceMode::Words && remaining_units == 0)
        {
            return Err(CommandReject::InvalidTransition);
        }
        let time_cap =
            remaining_ms.saturating_mul(u64::from(self.speech_rate_per_minute.max(1))) / 60_000;
        let effective_cap = if self.metering_mode == MatchResourceMode::Words {
            remaining_units
        } else {
            event
                .word_cap
                .min(time_cap.max(1).min(u64::from(u32::MAX)) as u32)
        };
        let (effective_text, overflow_text) = split_effective(&text, effective_cap);
        let effective_text = effective_text.to_owned();
        let overflow_text = overflow_text.to_owned();
        let effective_units = count_effective_units(&effective_text);
        let response_target_text = self
            .transcripts
            .iter()
            .rev()
            .find(|entry| entry.event_id == event.id && entry.slot.side() != expected_side)
            .map(|entry| entry.effective_text.clone())
            .unwrap_or_default();
        let transcript_id = self.fresh_id();
        self.transcripts.push(TranscriptEntryV1 {
            schema_version: TRANSCRIPT_SCHEMA_VERSION.to_owned(),
            id: transcript_id,
            event_id: event.id,
            slot: speaker,
            controller: Some(self.effective_controller(speaker)),
            full_text: text,
            effective_units,
            word_cap_at_submission: (self.metering_mode == MatchResourceMode::Words)
                .then_some(effective_cap),
            effective_text,
            overflow_text,
            cross_exam_exchange_id: None,
            response_target_text,
            cross_exam_turn_kind: None,
            cross_exam_round_number: 0,
        });
        let charge = if self.metering_mode == MatchResourceMode::Words {
            elapsed_ms
        } else {
            elapsed_ms.max(1).min(remaining_ms)
        };
        self.charged_ms = self.charged_ms.saturating_add(charge);
        let session = &mut self.free_debate_sessions[session_index];
        match expected_side {
            Side::Affirmative => {
                if self.metering_mode == MatchResourceMode::Words {
                    session.affirmative_remaining_units = session
                        .affirmative_remaining_units
                        .saturating_sub(effective_units);
                } else {
                    session.affirmative_remaining_ms =
                        session.affirmative_remaining_ms.saturating_sub(charge);
                }
            }
            Side::Negative => {
                if self.metering_mode == MatchResourceMode::Words {
                    session.negative_remaining_units = session
                        .negative_remaining_units
                        .saturating_sub(effective_units);
                } else {
                    session.negative_remaining_ms =
                        session.negative_remaining_ms.saturating_sub(charge);
                }
            }
        }
        session.round_number = session.round_number.saturating_add(1);
        session.revision = session.revision.saturating_add(1);
        let other = match expected_side {
            Side::Affirmative => Side::Negative,
            Side::Negative => Side::Affirmative,
        };
        let other_remaining = match other {
            Side::Affirmative => {
                if self.metering_mode == MatchResourceMode::Words {
                    u64::from(session.affirmative_remaining_units)
                } else {
                    session.affirmative_remaining_ms
                }
            }
            Side::Negative => {
                if self.metering_mode == MatchResourceMode::Words {
                    u64::from(session.negative_remaining_units)
                } else {
                    session.negative_remaining_ms
                }
            }
        };
        let own_remaining = match expected_side {
            Side::Affirmative => {
                if self.metering_mode == MatchResourceMode::Words {
                    u64::from(session.affirmative_remaining_units)
                } else {
                    session.affirmative_remaining_ms
                }
            }
            Side::Negative => {
                if self.metering_mode == MatchResourceMode::Words {
                    u64::from(session.negative_remaining_units)
                } else {
                    session.negative_remaining_ms
                }
            }
        };
        if other_remaining > 0 {
            session.next_side = other;
        } else if own_remaining > 0 {
            session.next_side = expected_side;
        } else {
            session.completed = true;
            self.cursor = self.cursor.saturating_add(1);
        }
        self.push_event(
            MatchEventKind::SpeechSubmitted,
            Some(speaker),
            "free debate turn submitted",
        );
        self.skip_exhausted_main_events();
        Ok(transcript_id)
    }

    pub fn waive_current_free_debate_time(&mut self) -> Result<Side, CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        if event.kind != StageKind::FreeDebate {
            return Err(CommandReject::WrongEvent);
        }
        let session_index = self.ensure_free_debate_session()?;
        let side = self.free_debate_sessions[session_index].next_side;
        let session = &mut self.free_debate_sessions[session_index];
        match side {
            Side::Affirmative => {
                if self.metering_mode == MatchResourceMode::Words {
                    session.affirmative_remaining_units = 0;
                } else {
                    session.affirmative_remaining_ms = 0;
                }
            }
            Side::Negative => {
                if self.metering_mode == MatchResourceMode::Words {
                    session.negative_remaining_units = 0;
                } else {
                    session.negative_remaining_ms = 0;
                }
            }
        }
        let other = match side {
            Side::Affirmative => Side::Negative,
            Side::Negative => Side::Affirmative,
        };
        let other_remaining = match other {
            Side::Affirmative => {
                if self.metering_mode == MatchResourceMode::Words {
                    u64::from(session.affirmative_remaining_units)
                } else {
                    session.affirmative_remaining_ms
                }
            }
            Side::Negative => {
                if self.metering_mode == MatchResourceMode::Words {
                    u64::from(session.negative_remaining_units)
                } else {
                    session.negative_remaining_ms
                }
            }
        };
        if other_remaining > 0 {
            session.next_side = other;
        } else {
            session.completed = true;
            self.cursor = self.cursor.saturating_add(1);
        }
        session.revision = session.revision.saturating_add(1);
        self.push_event(
            MatchEventKind::StateChanged,
            None,
            "free debate resource waived",
        );
        self.skip_exhausted_main_events();
        Ok(side)
    }

    fn append_current_transcript(
        &mut self,
        event_id: Uuid,
        speaker: SlotId,
        text: String,
        input_ms: u64,
        cross_exam_exchange_id: Option<Uuid>,
        response_target_text: String,
    ) -> Result<Uuid, CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let current = self
            .schedule
            .get(self.cursor)
            .ok_or(CommandReject::InvalidTransition)?;
        if current.id != event_id {
            return Err(CommandReject::WrongEvent);
        }
        let stage_kind = current.kind;
        let cap = self.current_word_cap();
        let (effective_text, overflow_text) = split_effective(&text, cap);
        let effective_units = count_effective_units(effective_text);
        let effective_text = effective_text.to_owned();
        let overflow_text = overflow_text.to_owned();
        let transcript_id = self.fresh_id();
        self.transcripts.push(TranscriptEntryV1 {
            schema_version: TRANSCRIPT_SCHEMA_VERSION.to_owned(),
            id: transcript_id,
            event_id,
            slot: speaker,
            controller: Some(self.effective_controller(speaker)),
            full_text: text,
            effective_units,
            word_cap_at_submission: (self.metering_mode == MatchResourceMode::Words).then_some(cap),
            effective_text,
            overflow_text,
            cross_exam_exchange_id,
            response_target_text,
            cross_exam_turn_kind: None,
            cross_exam_round_number: 0,
        });
        self.charged_ms = self.charged_ms.saturating_add(input_ms);
        if stage_kind != StageKind::FreeDebate {
            if self.metering_mode == MatchResourceMode::Words {
                self.charge_main_units(speaker.side(), effective_units);
            } else {
                self.charge_main_clock(speaker.side(), input_ms);
            }
        }
        self.cursor += 1;
        self.push_event(
            MatchEventKind::SpeechSubmitted,
            Some(speaker),
            "speech submitted",
        );
        self.skip_exhausted_main_events();
        Ok(transcript_id)
    }

    pub fn current_cross_exam_roles(&self) -> Result<(SlotId, SlotId), CommandReject> {
        let event = self
            .schedule
            .get(self.cursor)
            .ok_or(CommandReject::InvalidTransition)?;
        if event.kind != StageKind::CrossExam {
            return Err(CommandReject::InvalidTransition);
        }
        let stage = self
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        if let Some(exchange) = self.cross_exam_exchanges.iter().find(|exchange| {
            exchange.event_id == event.id
                && matches!(
                    exchange.status,
                    CrossExamExchangeStatus::Selected | CrossExamExchangeStatus::QuestionSubmitted
                )
        }) {
            return Ok((exchange.questioner, exchange.respondent));
        }
        if stage.eligible_questioners.is_empty() || stage.eligible_respondents.is_empty() {
            return Ok((
                stage
                    .questioner
                    .ok_or(CommandReject::ScheduleNotExecutable)?,
                stage
                    .respondent
                    .ok_or(CommandReject::ScheduleNotExecutable)?,
            ));
        }
        let side = self.cross_exam_questioning_side(stage, event.id);
        let questioners: Vec<_> = stage
            .eligible_questioners
            .iter()
            .copied()
            .filter(|slot| slot.side() == side)
            .collect();
        let respondents: Vec<_> = stage
            .eligible_respondents
            .iter()
            .copied()
            .filter(|slot| slot.side() != side)
            .collect();
        let mut question_counts = std::collections::HashMap::new();
        let mut response_counts = std::collections::HashMap::new();
        for exchange in self
            .cross_exam_exchanges
            .iter()
            .filter(|exchange| exchange.event_id == event.id)
        {
            *question_counts.entry(exchange.questioner).or_insert(0_u32) += 1;
            *response_counts.entry(exchange.respondent).or_insert(0_u32) += 1;
        }
        let questioner = crate::domain::floor::choose_slot(&questioners, &question_counts)
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let respondent = self
            .transcripts
            .iter()
            .rev()
            .find(|entry| entry.slot.side() != side && respondents.contains(&entry.slot))
            .map(|entry| entry.slot)
            .or_else(|| crate::domain::floor::choose_slot(&respondents, &response_counts))
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        Ok((questioner, respondent))
    }

    pub fn override_cross_exam_questioner(&mut self, slot: SlotId) -> Result<(), CommandReject> {
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        let stage = self
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let side = self.cross_exam_questioning_side(stage, event.id);
        if slot.side() != side
            || (!stage.eligible_questioners.is_empty()
                && !stage.eligible_questioners.contains(&slot))
        {
            return Err(CommandReject::CrossTeamTakeoverForbidden);
        }
        let (_, respondent) = self.current_cross_exam_roles()?;
        self.ensure_cross_exam_exchange(event, slot, respondent, true)?;
        Ok(())
    }

    pub fn start_cross_exam_round(&mut self) -> Result<Uuid, CommandReject> {
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        let (questioner, respondent) = self.current_cross_exam_roles()?;
        self.ensure_cross_exam_exchange(event, questioner, respondent, false)
    }

    pub fn select_cross_exam_roles(
        &mut self,
        questioner: SlotId,
        respondent: SlotId,
        selection_summary: String,
    ) -> Result<Uuid, CommandReject> {
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        let stage = self
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .cloned()
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let side = self.cross_exam_questioning_side(&stage, event.id);
        let valid_questioner = if stage.eligible_questioners.is_empty() {
            stage.questioner == Some(questioner)
        } else {
            stage.eligible_questioners.contains(&questioner)
        };
        let valid_respondent = if stage.eligible_respondents.is_empty() {
            stage.respondent == Some(respondent)
        } else {
            stage.eligible_respondents.contains(&respondent)
        };
        if !valid_questioner
            || !valid_respondent
            || questioner.side() != side
            || respondent.side() == side
        {
            return Err(CommandReject::ScheduleNotExecutable);
        }
        let id = self.ensure_cross_exam_exchange(event, questioner, respondent, false)?;
        let questioner_controller = self.effective_controller(questioner);
        let respondent_controller = self.effective_controller(respondent);
        let exchange = self
            .cross_exam_exchanges
            .iter_mut()
            .find(|exchange| exchange.id == id)
            .ok_or(CommandReject::InvalidTransition)?;
        if exchange.status != CrossExamExchangeStatus::Selected {
            return Err(CommandReject::InvalidTransition);
        }
        exchange.questioner = questioner;
        exchange.respondent = respondent;
        exchange.questioner_controller = Some(questioner_controller);
        exchange.respondent_controller = Some(respondent_controller);
        exchange.selection_summary = selection_summary.chars().take(80).collect();
        Ok(id)
    }

    fn ensure_cross_exam_exchange(
        &mut self,
        event: crate::domain::ruleset::ScheduledEvent,
        questioner: SlotId,
        respondent: SlotId,
        human_override: bool,
    ) -> Result<Uuid, CommandReject> {
        if let Some(id) = self
            .cross_exam_exchanges
            .iter()
            .find(|exchange| {
                exchange.event_id == event.id
                    && exchange.status == CrossExamExchangeStatus::Selected
            })
            .map(|exchange| exchange.id)
        {
            if human_override {
                if let Some(exchange) = self
                    .cross_exam_exchanges
                    .iter_mut()
                    .find(|exchange| exchange.id == id)
                {
                    exchange.questioner = questioner;
                    exchange.questioner_controller = Some(Controller::Human);
                    exchange.selection_summary = "人工选择本方辩位插入提问".to_owned();
                }
            }
            return Ok(id);
        }
        let stage = self
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .cloned()
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let available_main_ms = self.main_clock_remaining(questioner.side());
        let available_main_units = if self.metering_mode == MatchResourceMode::Words {
            [Side::Affirmative, Side::Negative]
                .into_iter()
                .filter(|side| {
                    if stage.eligible_questioners.is_empty() {
                        *side == questioner.side()
                    } else {
                        stage
                            .eligible_questioners
                            .iter()
                            .any(|slot| slot.side() == *side)
                    }
                })
                .fold(0_u32, |total, side| {
                    total.saturating_add(self.remaining_main_units(side))
                })
        } else {
            0
        };
        let session_index = if let Some(index) = self
            .cross_exam_sessions
            .iter()
            .position(|session| session.event_id == event.id)
        {
            index
        } else {
            self.cross_exam_sessions.push(CrossExamSessionV1 {
                schema_version: CROSS_EXAM_SESSION_SCHEMA_VERSION.to_owned(),
                event_id: event.id,
                phase: CrossExamSessionPhase::Selecting,
                next_side: questioner.side(),
                total_ms: available_main_ms,
                used_ms: 0,
                total_units: available_main_units,
                used_units: 0,
                round_number: 1,
                active_exchange_id: None,
                exchange_ids: Vec::new(),
                end_reason: None,
                revision: 0,
            });
            self.cross_exam_sessions.len() - 1
        };
        let exchange_id = self.fresh_id();
        let previous_exchange_id = self.cross_exam_sessions[session_index]
            .exchange_ids
            .last()
            .copied();
        let round_number = self.cross_exam_sessions[session_index].round_number;
        self.cross_exam_exchanges.push(CrossExamExchangeV1 {
            schema_version: CROSS_EXAM_EXCHANGE_SCHEMA_VERSION.to_owned(),
            id: exchange_id,
            event_id: event.id,
            questioner,
            respondent,
            question_text: String::new(),
            answer_transcript_id: None,
            status: CrossExamExchangeStatus::Selected,
            displayed_answer_units: 0,
            interrupted_reason: String::new(),
            round_number,
            questioning_side: Some(questioner.side()),
            questioner_controller: Some(if human_override {
                Controller::Human
            } else {
                self.effective_controller(questioner)
            }),
            respondent_controller: Some(self.effective_controller(respondent)),
            previous_exchange_id,
            question_transcript_id: None,
            question_input_ms: 0,
            answer_input_ms: 0,
            selection_summary: if human_override {
                "人工选择本方辩位插入提问".to_owned()
            } else {
                format!(
                    "系统根据公开实录与出场均衡安排 {:?} 质询 {:?}",
                    questioner, respondent
                )
            },
        });
        let session = &mut self.cross_exam_sessions[session_index];
        session.next_side = questioner.side();
        session.active_exchange_id = Some(exchange_id);
        session.exchange_ids.push(exchange_id);
        session.phase = CrossExamSessionPhase::QuestionPending;
        session.revision = session.revision.saturating_add(1);
        Ok(exchange_id)
    }

    pub fn submit_cross_exam_question(
        &mut self,
        question: String,
        input_ms: u64,
    ) -> Result<Uuid, CommandReject> {
        if self.phase != MatchPhase::InProgress || question.trim().is_empty() {
            return Err(CommandReject::InvalidTransition);
        }
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        let (questioner, respondent) = self.current_cross_exam_roles()?;
        let id = self.ensure_cross_exam_exchange(event.clone(), questioner, respondent, false)?;
        let exchange_index = self
            .cross_exam_exchanges
            .iter()
            .position(|exchange| exchange.id == id)
            .ok_or(CommandReject::InvalidTransition)?;
        if self.cross_exam_exchanges[exchange_index].status != CrossExamExchangeStatus::Selected {
            return Err(CommandReject::InvalidTransition);
        }
        let round = self.cross_exam_exchanges[exchange_index].round_number;
        let target = self
            .transcripts
            .last()
            .map(|entry| entry.effective_text.clone())
            .unwrap_or_default();
        let transcript_id = self.append_cross_exam_transcript(
            event.id,
            questioner,
            question.trim().to_owned(),
            input_ms,
            id,
            target,
            CrossExamTurnKind::Question,
            round,
        )?;
        let effective_question = self
            .transcripts
            .last()
            .map(|entry| entry.effective_text.clone())
            .unwrap_or_default();
        let word_pool_exhausted = self.metering_mode == MatchResourceMode::Words
            && self.remaining_main_units(questioner.side()) == 0;
        let next_side_with_units = if word_pool_exhausted {
            self.ruleset
                .as_ref()
                .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
                .and_then(|stage| {
                    let other = opposite_side(questioner.side());
                    (stage
                        .eligible_questioners
                        .iter()
                        .any(|slot| slot.side() == other)
                        && self.remaining_main_units(other) > 0)
                        .then_some(other)
                })
        } else {
            None
        };
        let exchange = &mut self.cross_exam_exchanges[exchange_index];
        exchange.question_text = effective_question;
        exchange.question_transcript_id = Some(transcript_id);
        exchange.question_input_ms = input_ms;
        exchange.status = CrossExamExchangeStatus::QuestionSubmitted;
        if let Some(session) = self
            .cross_exam_sessions
            .iter_mut()
            .find(|session| session.event_id == event.id)
        {
            session.used_ms = if self.metering_mode == MatchResourceMode::Words {
                session.used_ms.saturating_add(input_ms)
            } else {
                session
                    .used_ms
                    .saturating_add(input_ms)
                    .min(session.total_ms)
            };
            session.phase = CrossExamSessionPhase::AnswerPending;
            session.revision = session.revision.saturating_add(1);
            let exhausted = if self.metering_mode == MatchResourceMode::Words {
                word_pool_exhausted || session.used_units >= session.total_units
            } else {
                session.used_ms >= session.total_ms
            };
            if exhausted {
                session.active_exchange_id = None;
                self.cross_exam_exchanges[exchange_index].status =
                    CrossExamExchangeStatus::TimedOut;
                self.cross_exam_exchanges[exchange_index].interrupted_reason =
                    if self.metering_mode == MatchResourceMode::Words {
                        "提问方共享字数额度已耗尽"
                    } else {
                        "质询共享时间已耗尽"
                    }
                    .to_owned();
                if let Some(next_side) = next_side_with_units {
                    session.next_side = next_side;
                    session.round_number = session.round_number.saturating_add(1);
                    session.phase = CrossExamSessionPhase::Selecting;
                } else {
                    session.phase = CrossExamSessionPhase::Completed;
                    session.end_reason = Some(CrossExamSessionEndReason::Timeout);
                    self.cursor = self.cursor.saturating_add(1);
                }
            }
        }
        self.skip_exhausted_main_events();
        Ok(id)
    }

    pub fn commit_cross_exam_answer(
        &mut self,
        text: String,
        input_ms: u64,
        interrupted_reason: Option<String>,
    ) -> Result<Uuid, CommandReject> {
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        let exchange_index = self
            .cross_exam_exchanges
            .iter()
            .position(|exchange| {
                exchange.event_id == event.id
                    && exchange.status == CrossExamExchangeStatus::QuestionSubmitted
            })
            .ok_or(CommandReject::InvalidTransition)?;
        let exchange_id = self.cross_exam_exchanges[exchange_index].id;
        let respondent = self.cross_exam_exchanges[exchange_index].respondent;
        let question = self.cross_exam_exchanges[exchange_index]
            .question_text
            .clone();
        let round = self.cross_exam_exchanges[exchange_index].round_number;
        let transcript_id = self.append_cross_exam_transcript(
            event.id,
            respondent,
            text,
            input_ms,
            exchange_id,
            question,
            CrossExamTurnKind::Answer,
            round,
        )?;
        let questioning_side = self.cross_exam_exchanges[exchange_index].questioner.side();
        let own_remaining_units = self.remaining_main_units(questioning_side);
        let other_remaining_units = self.remaining_main_units(opposite_side(questioning_side));
        let exchange = &mut self.cross_exam_exchanges[exchange_index];
        exchange.answer_transcript_id = Some(transcript_id);
        exchange.displayed_answer_units = self
            .transcripts
            .last()
            .map(|entry| entry.effective_units)
            .unwrap_or(0);
        exchange.answer_input_ms = input_ms;
        if let Some(reason) = interrupted_reason {
            exchange.status = CrossExamExchangeStatus::Interrupted;
            exchange.interrupted_reason = reason;
        } else {
            exchange.status = CrossExamExchangeStatus::Answered;
            exchange.interrupted_reason.clear();
        }
        let stage = self
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .cloned()
            .ok_or(CommandReject::ScheduleNotExecutable)?;
        let legacy_single = stage.eligible_questioners.is_empty();
        let alternates_sides = stage
            .eligible_questioners
            .iter()
            .any(|slot| slot.side() == Side::Affirmative)
            && stage
                .eligible_questioners
                .iter()
                .any(|slot| slot.side() == Side::Negative);
        let session = self
            .cross_exam_sessions
            .iter_mut()
            .find(|session| session.event_id == event.id)
            .ok_or(CommandReject::InvalidTransition)?;
        session.used_ms = if self.metering_mode == MatchResourceMode::Words {
            session.used_ms.saturating_add(input_ms)
        } else {
            session
                .used_ms
                .saturating_add(input_ms)
                .min(session.total_ms)
        };
        session.active_exchange_id = None;
        let round_limit = stage
            .max_cross_exam_rounds
            .is_some_and(|limit| session.round_number >= limit);
        let timed_out = if self.metering_mode == MatchResourceMode::Words {
            session.used_units >= session.total_units
                || (own_remaining_units == 0 && (!alternates_sides || other_remaining_units == 0))
        } else {
            session.used_ms >= session.total_ms
        };
        if timed_out || (self.metering_mode == MatchResourceMode::Words && own_remaining_units == 0)
        {
            exchange.status = CrossExamExchangeStatus::TimedOut;
            exchange.interrupted_reason = if self.metering_mode == MatchResourceMode::Words {
                "提问方共享字数额度已耗尽"
            } else {
                "质询共享时间已耗尽"
            }
            .to_owned();
        }
        if legacy_single || timed_out || round_limit {
            session.phase = CrossExamSessionPhase::Completed;
            session.end_reason = Some(if legacy_single {
                CrossExamSessionEndReason::LegacySingleRound
            } else if round_limit {
                CrossExamSessionEndReason::RoundLimit
            } else {
                CrossExamSessionEndReason::Timeout
            });
            self.cursor += 1;
        } else {
            if alternates_sides {
                session.next_side = if self.metering_mode == MatchResourceMode::Words
                    && other_remaining_units == 0
                {
                    questioning_side
                } else {
                    opposite_side(questioning_side)
                };
            }
            session.round_number = session.round_number.saturating_add(1);
            session.phase = CrossExamSessionPhase::Selecting;
        }
        session.revision = session.revision.saturating_add(1);
        self.skip_exhausted_main_events();
        Ok(transcript_id)
    }

    pub fn end_cross_exam_session(
        &mut self,
        reason: CrossExamSessionEndReason,
    ) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let event = self
            .schedule
            .get(self.cursor)
            .cloned()
            .ok_or(CommandReject::InvalidTransition)?;
        if event.kind != StageKind::CrossExam {
            return Err(CommandReject::WrongEvent);
        }
        let session = self
            .cross_exam_sessions
            .iter_mut()
            .find(|session| session.event_id == event.id)
            .ok_or(CommandReject::InvalidTransition)?;
        if session.phase == CrossExamSessionPhase::Completed {
            return Err(CommandReject::InvalidTransition);
        }
        if let Some(exchange_id) = session.active_exchange_id {
            if let Some(exchange) = self
                .cross_exam_exchanges
                .iter_mut()
                .find(|exchange| exchange.id == exchange_id)
            {
                if matches!(
                    exchange.status,
                    CrossExamExchangeStatus::Selected | CrossExamExchangeStatus::QuestionSubmitted
                ) {
                    exchange.status = if reason == CrossExamSessionEndReason::Timeout {
                        CrossExamExchangeStatus::TimedOut
                    } else {
                        CrossExamExchangeStatus::Interrupted
                    };
                    exchange.interrupted_reason = match reason {
                        CrossExamSessionEndReason::Timeout => {
                            if self.metering_mode == MatchResourceMode::Words {
                                "质询字数额度已耗尽"
                            } else {
                                "质询共享时间已耗尽"
                            }
                        }
                        CrossExamSessionEndReason::UserEnded => "用户结束质询",
                        CrossExamSessionEndReason::RoundLimit => "已达到赛制轮数上限",
                        CrossExamSessionEndReason::LegacySingleRound => "旧赛制单轮质询结束",
                    }
                    .to_owned();
                }
            }
        }
        session.phase = CrossExamSessionPhase::Completed;
        session.active_exchange_id = None;
        session.end_reason = Some(reason);
        if reason == CrossExamSessionEndReason::Timeout
            && self.metering_mode == MatchResourceMode::Time
        {
            session.used_ms = session.total_ms;
        }
        session.revision = session.revision.saturating_add(1);
        self.cursor = self.cursor.saturating_add(1);
        self.skip_exhausted_main_events();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_cross_exam_transcript(
        &mut self,
        event_id: Uuid,
        speaker: SlotId,
        text: String,
        input_ms: u64,
        exchange_id: Uuid,
        response_target_text: String,
        turn_kind: CrossExamTurnKind,
        round_number: u32,
    ) -> Result<Uuid, CommandReject> {
        let clock_side = self.current_cross_exam_roles()?.0.side();
        let event = self
            .schedule
            .get(self.cursor)
            .ok_or(CommandReject::InvalidTransition)?;
        if event.id != event_id || event.kind != StageKind::CrossExam {
            return Err(CommandReject::WrongEvent);
        }
        let cap = self.current_word_cap();
        let (effective, overflow) = split_effective(&text, cap);
        let effective_text = effective.to_owned();
        let overflow_text = overflow.to_owned();
        let effective_units = count_effective_units(&effective_text);
        let controller = self.effective_controller(speaker);
        let id = self.fresh_id();
        self.transcripts.push(TranscriptEntryV1 {
            schema_version: TRANSCRIPT_SCHEMA_VERSION.to_owned(),
            id,
            event_id,
            slot: speaker,
            controller: Some(controller),
            full_text: text,
            effective_text,
            effective_units,
            word_cap_at_submission: (self.metering_mode == MatchResourceMode::Words).then_some(cap),
            overflow_text,
            cross_exam_exchange_id: Some(exchange_id),
            response_target_text,
            cross_exam_turn_kind: Some(turn_kind),
            cross_exam_round_number: round_number,
        });
        self.charged_ms = self.charged_ms.saturating_add(input_ms);
        if self.metering_mode == MatchResourceMode::Words {
            self.charge_main_units(clock_side, effective_units);
            if let Some(session) = self
                .cross_exam_sessions
                .iter_mut()
                .find(|session| session.event_id == event_id)
            {
                session.used_units = session
                    .used_units
                    .saturating_add(effective_units)
                    .min(session.total_units);
            }
        } else {
            self.charge_main_clock(clock_side, input_ms);
        }
        Ok(id)
    }

    fn interrupt(&mut self) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let current = self
            .schedule
            .get(self.cursor)
            .ok_or(CommandReject::InvalidTransition)?;
        if current.kind == StageKind::FreeDebate {
            return Err(CommandReject::FreeDebateInterruptForbidden);
        }
        if !current.interruptible {
            return Err(CommandReject::InterruptNotAllowed);
        }
        Err(CommandReject::InvalidTransition)
    }

    fn request_takeover(
        &mut self,
        slot_index: usize,
        second_confirmed: bool,
    ) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        let slot = SlotId::from_index(slot_index).ok_or(CommandReject::SlotInvariant)?;
        if self.mode == MatchMode::Scoring {
            let user_side = self.user_side.ok_or(CommandReject::SlotInvariant)?;
            if slot.side() != user_side {
                return Err(CommandReject::CrossTeamTakeoverForbidden);
            }
            if !second_confirmed {
                return Err(CommandReject::TakeoverNeedsConfirmation);
            }
            self.non_standard = true;
        }
        self.assignments[slot.index()].controller = Controller::Human;
        // A selected exchange owns per-turn control overrides. A real takeover
        // must update that authority too, without reclassifying past turns.
        if let Some(event_id) = self.schedule.get(self.cursor).map(|event| event.id) {
            if let Some(exchange) = self.cross_exam_exchanges.iter_mut().find(|exchange| {
                exchange.event_id == event_id
                    && matches!(
                        exchange.status,
                        CrossExamExchangeStatus::Selected
                            | CrossExamExchangeStatus::QuestionSubmitted
                    )
            }) {
                if exchange.questioner == slot {
                    exchange.questioner_controller = Some(Controller::Human);
                }
                if exchange.respondent == slot {
                    exchange.respondent_controller = Some(Controller::Human);
                }
            }
        }
        self.push_event(MatchEventKind::Takeover, Some(slot), "takeover");
        Ok(())
    }

    fn note_ai_wait(&mut self, waited_ms: u64) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        self.ai_wait_ms = self.ai_wait_ms.saturating_add(waited_ms);
        self.push_event(MatchEventKind::AiWait, None, "ai wait");
        Ok(())
    }

    fn complete_match(&mut self) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::InProgress || self.cursor != self.schedule.len() {
            return Err(CommandReject::InvalidTransition);
        }
        self.phase = MatchPhase::MatchComplete;
        self.push_event(MatchEventKind::StateChanged, None, "match complete");
        Ok(())
    }

    fn begin_scoring(&mut self) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::MatchComplete {
            return Err(CommandReject::InvalidTransition);
        }
        self.phase = MatchPhase::Scoring;
        self.push_event(MatchEventKind::StateChanged, None, "scoring");
        Ok(())
    }

    fn publish_report(&mut self) -> Result<(), CommandReject> {
        if self.phase != MatchPhase::Scoring {
            return Err(CommandReject::InvalidTransition);
        }
        self.phase = MatchPhase::ReportReady;
        self.push_event(MatchEventKind::StateChanged, None, "report ready");
        Ok(())
    }

    fn mark_recovery(&mut self) -> Result<(), CommandReject> {
        if self.mode != MatchMode::Scoring || self.phase != MatchPhase::InProgress {
            return Err(CommandReject::InvalidTransition);
        }
        self.non_standard = true;
        self.phase = MatchPhase::RecoveryRequired;
        self.push_event(MatchEventKind::StateChanged, None, "recovery required");
        Ok(())
    }

    fn abort_match(&mut self) -> Result<(), CommandReject> {
        if !matches!(
            self.phase,
            MatchPhase::InProgress | MatchPhase::RecoveryRequired
        ) {
            return Err(CommandReject::InvalidTransition);
        }
        self.phase = MatchPhase::Aborted;
        self.push_event(MatchEventKind::StateChanged, None, "match aborted");
        Ok(())
    }

    fn reject_if_started(&self) -> Result<(), CommandReject> {
        if matches!(
            self.phase,
            MatchPhase::InProgress
                | MatchPhase::MatchComplete
                | MatchPhase::Scoring
                | MatchPhase::ReportReady
                | MatchPhase::RecoveryRequired
                | MatchPhase::Aborted
        ) {
            Err(CommandReject::InvalidTransition)
        } else {
            Ok(())
        }
    }

    fn revoke_lineup(&mut self) {
        self.lineup_hash = None;
        if matches!(self.phase, MatchPhase::LineupConfirmed | MatchPhase::Ready) {
            self.phase = if self.mode == MatchMode::Scoring && self.calibrated {
                MatchPhase::Calibrated
            } else {
                MatchPhase::ControlAssigned
            };
        }
    }

    fn clear_preparation_outputs(&mut self) {
        if let Some(preparation) = self.preparation.as_mut() {
            preparation.strategies = None;
            preparation.role_packets = None;
        }
    }

    pub fn invalidate_preparation(&mut self) -> Result<(), CommandReject> {
        self.reject_if_started()?;
        self.clear_preparation_outputs();
        self.lineup_hash = None;
        self.calibrated = false;
        self.phase = if self.rules_hash.is_some() {
            MatchPhase::RulesConfirmed
        } else if self.ruleset.is_some() || self.semantic_ruleset.is_some() {
            MatchPhase::RulesParsed
        } else {
            MatchPhase::DraftSetup
        };
        self.push_event(
            MatchEventKind::StateChanged,
            None,
            "preparation invalidated",
        );
        Ok(())
    }

    fn fresh_id(&mut self) -> Uuid {
        self.next_salt += 1;
        Uuid::from_u128(self.next_salt)
    }

    fn push_event(&mut self, kind: MatchEventKind, slot: Option<SlotId>, summary: &str) {
        let id = self.fresh_id();
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.events.push(MatchEventV1 {
            schema_version: EVENT_SCHEMA_VERSION.to_owned(),
            id,
            sequence,
            kind,
            slot,
            summary: summary.to_owned(),
        });
    }

    pub fn to_snapshot(&self) -> MatchSnapshotV1 {
        MatchSnapshotV1 {
            schema_version: MATCH_SNAPSHOT_SCHEMA_VERSION.to_owned(),
            id: self.id,
            mode: self.mode,
            metering_mode: self.metering_mode,
            word_budgets: self.word_budgets.clone(),
            phase: self.phase,
            topic: self.topic.clone(),
            materials: self.materials.clone(),
            speech_rate_per_minute: self.speech_rate_per_minute,
            ruleset: self.ruleset.clone(),
            semantic_ruleset: self.semantic_ruleset.clone(),
            rules_hash: self.rules_hash.clone(),
            lineup_hash: self.lineup_hash.clone(),
            user_side: self.user_side,
            assignments: self.assignments.clone(),
            current_control_overrides: self.active_overrides(),
            control_revision: self.control_revision,
            calibrated: self.calibrated,
            non_standard: self.non_standard,
            schedule: self.schedule.clone(),
            cursor: self.cursor,
            charged_ms: self.charged_ms,
            ai_wait_ms: self.ai_wait_ms,
            affirmative_main_remaining_ms: self.affirmative_main_remaining_ms,
            negative_main_remaining_ms: self.negative_main_remaining_ms,
            events: self.events.clone(),
            transcripts: self.transcripts.clone(),
            cross_exam_exchanges: self.cross_exam_exchanges.clone(),
            cross_exam_sessions: self.cross_exam_sessions.clone(),
            free_debate_sessions: self.free_debate_sessions.clone(),
            scoring_state: self.scoring_state.clone(),
            preparation: self.preparation.clone(),
            public_response_links: self.public_response_links.clone(),
            next_salt: self.next_salt.to_string(),
            next_sequence: self.next_sequence,
        }
    }

    pub fn from_snapshot(snapshot: MatchSnapshotV1) -> Result<Self, crate::error::AppError> {
        if snapshot.schema_version != MATCH_SNAPSHOT_SCHEMA_VERSION {
            return Err(crate::error::AppError::new(
                crate::error::ErrorCode::InvalidArgument,
                "match snapshot schema is not supported",
            ));
        }
        let next_salt = snapshot.next_salt.parse::<u128>().map_err(|_| {
            crate::error::AppError::new(
                crate::error::ErrorCode::InvalidArgument,
                "match snapshot salt is invalid",
            )
        })?;
        Ok(Self {
            id: snapshot.id,
            mode: snapshot.mode,
            metering_mode: snapshot.metering_mode,
            word_budgets: snapshot.word_budgets,
            phase: snapshot.phase,
            topic: snapshot.topic,
            materials: snapshot.materials,
            speech_rate_per_minute: snapshot.speech_rate_per_minute,
            ruleset: snapshot.ruleset,
            semantic_ruleset: snapshot.semantic_ruleset,
            rules_hash: snapshot.rules_hash,
            lineup_hash: snapshot.lineup_hash,
            user_side: snapshot.user_side,
            assignments: snapshot.assignments,
            current_control_overrides: snapshot.current_control_overrides,
            control_revision: snapshot.control_revision,
            calibrated: snapshot.calibrated,
            non_standard: snapshot.non_standard,
            schedule: snapshot.schedule,
            cursor: snapshot.cursor,
            charged_ms: snapshot.charged_ms,
            ai_wait_ms: snapshot.ai_wait_ms,
            affirmative_main_remaining_ms: snapshot.affirmative_main_remaining_ms,
            negative_main_remaining_ms: snapshot.negative_main_remaining_ms,
            events: snapshot.events,
            transcripts: snapshot.transcripts,
            cross_exam_exchanges: snapshot.cross_exam_exchanges,
            cross_exam_sessions: snapshot.cross_exam_sessions,
            free_debate_sessions: snapshot.free_debate_sessions,
            scoring_state: snapshot.scoring_state,
            preparation: snapshot.preparation,
            public_response_links: snapshot.public_response_links,
            next_salt,
            next_sequence: snapshot.next_sequence,
        })
    }
}

pub const MATCH_SNAPSHOT_SCHEMA_VERSION: &str = "match-snapshot.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSnapshotV1 {
    pub schema_version: String,
    pub id: Uuid,
    pub mode: MatchMode,
    #[serde(default)]
    pub metering_mode: MatchResourceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_budgets: Option<WordBudgetsV1>,
    pub phase: MatchPhase,
    pub topic: String,
    pub materials: String,
    pub speech_rate_per_minute: u32,
    pub ruleset: Option<RulesetV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_ruleset: Option<RulesetV2>,
    pub rules_hash: Option<String>,
    pub lineup_hash: Option<String>,
    pub user_side: Option<Side>,
    pub assignments: [DebaterAssignmentV1; 8],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_control_overrides: Vec<crate::domain::control_policy::ControlOverride>,
    #[serde(
        default,
        skip_serializing_if = "crate::domain::control_policy::is_zero"
    )]
    pub control_revision: u64,
    pub calibrated: bool,
    pub non_standard: bool,
    pub schedule: Vec<crate::domain::ruleset::ScheduledEvent>,
    pub cursor: usize,
    pub charged_ms: u64,
    pub ai_wait_ms: u64,
    #[serde(default = "default_main_pool_ms")]
    pub affirmative_main_remaining_ms: u64,
    #[serde(default = "default_main_pool_ms")]
    pub negative_main_remaining_ms: u64,
    pub events: Vec<MatchEventV1>,
    pub transcripts: Vec<TranscriptEntryV1>,
    #[serde(default)]
    pub cross_exam_exchanges: Vec<CrossExamExchangeV1>,
    #[serde(default)]
    pub cross_exam_sessions: Vec<CrossExamSessionV1>,
    #[serde(default)]
    pub free_debate_sessions: Vec<FreeDebateSessionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoring_state: Option<HumanScoringStateV3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation: Option<PreparationStateV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_response_links: Vec<crate::domain::issue_ledger::PublicResponseLink>,
    pub next_salt: String,
    pub next_sequence: u32,
}

fn opposite_side(side: Side) -> Side {
    match side {
        Side::Affirmative => Side::Negative,
        Side::Negative => Side::Affirmative,
    }
}

fn default_assignments() -> [DebaterAssignmentV1; 8] {
    SlotId::ALL.map(|slot| DebaterAssignmentV1 {
        schema_version: ASSIGNMENT_SCHEMA_VERSION.to_owned(),
        slot,
        controller: Controller::Ai,
    })
}

fn normalize_assignments(
    assignments: Vec<DebaterAssignmentV1>,
) -> Result<[DebaterAssignmentV1; 8], CommandReject> {
    if assignments.len() != 8 {
        return Err(CommandReject::SlotInvariant);
    }
    let mut placed: [Option<DebaterAssignmentV1>; 8] = Default::default();
    for assignment in assignments {
        let index = assignment.slot.index();
        if placed[index].is_some() || assignment.slot != SlotId::ALL[index] {
            return Err(CommandReject::SlotInvariant);
        }
        placed[index] = Some(DebaterAssignmentV1 {
            schema_version: ASSIGNMENT_SCHEMA_VERSION.to_owned(),
            slot: assignment.slot,
            controller: assignment.controller,
        });
    }
    let mut normalized = default_assignments();
    for (index, assignment) in placed.into_iter().enumerate() {
        normalized[index] = assignment.ok_or(CommandReject::SlotInvariant)?;
    }
    Ok(normalized)
}

#[cfg(test)]
mod snapshot_compatibility_tests {
    use super::{MatchAggregate, MatchCommand, MatchResourceMode, MatchSnapshotV1};
    use crate::domain::events::{
        CrossExamExchangeStatus, CrossExamSessionEndReason, CrossExamSessionPhase,
        CrossExamTurnKind,
    };
    use crate::domain::match_engine::training_assignments;
    use crate::domain::ruleset::{four_on_four_template, MatchMode, Side, SlotId};
    use uuid::Uuid;

    #[test]
    fn old_snapshot_without_cross_exam_exchanges_still_loads() {
        let aggregate = MatchAggregate::new(
            Uuid::from_u128(77),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        let mut json = serde_json::to_value(aggregate.to_snapshot()).unwrap();
        json.as_object_mut().unwrap().remove("meteringMode");
        json.as_object_mut().unwrap().remove("wordBudgets");
        json.as_object_mut().unwrap().remove("crossExamExchanges");
        json.as_object_mut().unwrap().remove("crossExamSessions");
        json.as_object_mut()
            .unwrap()
            .remove("affirmativeMainRemainingMs");
        json.as_object_mut()
            .unwrap()
            .remove("negativeMainRemainingMs");
        let snapshot: MatchSnapshotV1 = serde_json::from_value(json).unwrap();
        let restored = MatchAggregate::from_snapshot(snapshot).unwrap();
        assert_eq!(restored.metering_mode, MatchResourceMode::Time);
        assert!(restored.word_budgets.is_none());
        assert!(restored.cross_exam_exchanges.is_empty());
        assert!(restored.cross_exam_sessions.is_empty());
        assert_eq!(
            restored.affirmative_main_remaining_ms,
            super::MAIN_TEAM_POOL_MS
        );
        assert_eq!(
            restored.negative_main_remaining_ms,
            super::MAIN_TEAM_POOL_MS
        );
    }

    #[test]
    fn cross_exam_question_and_answer_persist_the_real_participants_and_target() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(78),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".to_owned(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        let opening = aggregate.schedule[0].id;
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "开场".to_owned(),
                input_ms: 1_000,
            })
            .unwrap();

        let exchange_id = aggregate
            .submit_cross_exam_question("请定义高风险用途".to_owned(), 800)
            .unwrap();
        let transcript_id = aggregate
            .commit_cross_exam_answer("高风险用途是影响生命安全的应用".to_owned(), 2_000, None)
            .unwrap();
        let transcript = aggregate.transcripts.last().unwrap();
        assert_eq!(transcript.id, transcript_id);
        assert_eq!(transcript.slot, SlotId::Neg1);
        assert_eq!(transcript.cross_exam_exchange_id, Some(exchange_id));
        assert_eq!(transcript.response_target_text, "请定义高风险用途");
        let exchange = aggregate.cross_exam_exchanges.last().unwrap();
        assert_eq!(exchange.questioner, SlotId::Aff1);
        assert_eq!(exchange.respondent, SlotId::Neg1);
        assert_eq!(exchange.status, CrossExamExchangeStatus::Answered);
        assert_eq!(exchange.answer_transcript_id, Some(transcript_id));
    }

    #[test]
    fn v3_main_team_clocks_persist_across_non_contiguous_stages() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(7_803),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        aggregate.metering_mode = MatchResourceMode::Time;
        aggregate.word_budgets = None;
        aggregate
            .submit_semantic_rules(crate::domain::ruleset_v2::traditional_four_on_four_v2())
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".to_owned(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();

        let opening = aggregate.schedule[0].id;
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "正方开篇".to_owned(),
                input_ms: 5_000,
            })
            .unwrap();
        assert_eq!(aggregate.affirmative_main_remaining_ms, 1_075_000);
        assert_eq!(aggregate.negative_main_remaining_ms, 1_080_000);

        aggregate
            .submit_cross_exam_question("请说明边界".to_owned(), 1_000)
            .unwrap();
        aggregate
            .commit_cross_exam_answer("边界依据风险分级".to_owned(), 2_000, None)
            .unwrap();
        assert_eq!(aggregate.negative_main_remaining_ms, 1_077_000);
        assert_eq!(aggregate.affirmative_main_remaining_ms, 1_075_000);
    }

    #[test]
    fn multi_role_cross_exam_alternates_three_rounds_and_advances_only_on_session_end() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(79),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".to_owned(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        let opening = aggregate.schedule[0].id;
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "开场".to_owned(),
                input_ms: 1_000,
            })
            .unwrap();
        let cross_cursor = aggregate.cursor;

        for round in 1..=3 {
            aggregate
                .submit_cross_exam_question(format!("第{round}轮问题"), 100)
                .unwrap();
            aggregate
                .commit_cross_exam_answer(format!("第{round}轮回答"), 200, None)
                .unwrap();
            assert_eq!(aggregate.cursor, cross_cursor);
        }

        let sides: Vec<_> = aggregate
            .cross_exam_exchanges
            .iter()
            .map(|exchange| exchange.questioning_side.unwrap())
            .collect();
        assert_eq!(
            sides,
            vec![Side::Affirmative, Side::Negative, Side::Affirmative]
        );
        assert_eq!(aggregate.cross_exam_exchanges[0].questioner, SlotId::Aff1);
        assert_eq!(aggregate.cross_exam_exchanges[1].questioner, SlotId::Neg1);
        assert_eq!(aggregate.cross_exam_exchanges[2].questioner, SlotId::Aff2);
        assert_eq!(aggregate.cross_exam_sessions[0].used_ms, 900);
        assert_eq!(aggregate.cross_exam_sessions[0].round_number, 4);
        assert_eq!(aggregate.transcripts.len(), 7);
        assert_eq!(
            aggregate.transcripts[1].cross_exam_turn_kind,
            Some(CrossExamTurnKind::Question)
        );
        assert_eq!(
            aggregate.transcripts[2].cross_exam_turn_kind,
            Some(CrossExamTurnKind::Answer)
        );

        aggregate
            .end_cross_exam_session(CrossExamSessionEndReason::UserEnded)
            .unwrap();
        assert_eq!(aggregate.cursor, cross_cursor + 1);
        assert_eq!(
            aggregate.cross_exam_sessions[0].phase,
            CrossExamSessionPhase::Completed
        );
        assert_eq!(
            aggregate.cross_exam_sessions[0].end_reason,
            Some(CrossExamSessionEndReason::UserEnded)
        );
    }

    #[test]
    fn human_questioner_override_cannot_choose_the_other_side_or_the_respondent() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(80),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".into(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        let opening = aggregate.schedule[0].id;
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "开场".into(),
                input_ms: 10,
            })
            .unwrap();

        assert!(aggregate
            .override_cross_exam_questioner(SlotId::Neg2)
            .is_err());
        aggregate
            .override_cross_exam_questioner(SlotId::Aff4)
            .unwrap();
        let selected = aggregate.cross_exam_exchanges.last().unwrap();
        assert_eq!(selected.questioner, SlotId::Aff4);
        assert_eq!(selected.respondent.side(), Side::Negative);
    }

    #[test]
    fn only_an_active_match_can_be_aborted() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(81),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        assert!(aggregate.handle(MatchCommand::AbortMatch).is_err());
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(four_on_four_template()),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".into(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        aggregate.handle(MatchCommand::AbortMatch).unwrap();
        assert_eq!(aggregate.phase, super::MatchPhase::Aborted);
        assert!(aggregate.handle(MatchCommand::AbortMatch).is_err());
    }

    fn ready_semantic_match() -> MatchAggregate {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(8_000),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        aggregate
            .submit_semantic_rules(crate::domain::ruleset_v2::traditional_four_on_four_v2())
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".to_owned(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        aggregate
    }

    #[test]
    fn words_charge_only_the_effective_prefix_and_preserve_elapsed_statistics() {
        let mut aggregate = ready_semantic_match();
        assert_eq!(aggregate.metering_mode, MatchResourceMode::Words);
        assert_eq!(aggregate.remaining_main_units(Side::Affirmative), 4_320);
        aggregate
            .word_budgets
            .as_mut()
            .unwrap()
            .affirmative_remaining_units = 3;
        let opening = aggregate.schedule[aggregate.cursor].id;
        aggregate
            .handle(MatchCommand::NoteAiWait { waited_ms: 900_000 })
            .unwrap();
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "甲， 乙丙丁戊".to_owned(),
                input_ms: 10_000_000,
            })
            .unwrap();
        let entry = aggregate.transcripts.last().unwrap();
        assert_eq!(entry.effective_text, "甲， 乙丙");
        assert_eq!(entry.overflow_text, "丁戊");
        assert_eq!(entry.effective_units, 3);
        assert_eq!(aggregate.remaining_main_units(Side::Affirmative), 0);
        assert_eq!(aggregate.remaining_main_units(Side::Negative), 4_320);
        assert_eq!(
            aggregate.affirmative_main_remaining_ms,
            super::MAIN_TEAM_POOL_MS
        );
        assert_eq!(aggregate.charged_ms, 10_000_000);
        assert_eq!(aggregate.ai_wait_ms, 900_000);
        assert!(aggregate.current_word_cap() > 0);
        let restored = MatchAggregate::from_snapshot(aggregate.to_snapshot()).unwrap();
        assert_eq!(restored.word_budgets, aggregate.word_budgets);
        assert_eq!(restored.metering_mode, MatchResourceMode::Words);
    }

    #[test]
    fn question_and_answer_share_only_the_questioning_sides_word_pool() {
        let mut aggregate = ready_semantic_match();
        let opening = aggregate.schedule[0].id;
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id: opening,
                text: "开场".to_owned(),
                input_ms: 1,
            })
            .unwrap();
        let (questioner, respondent) = aggregate.current_cross_exam_roles().unwrap();
        let pool = aggregate.word_budgets.as_mut().unwrap();
        match questioner.side() {
            Side::Affirmative => pool.affirmative_remaining_units = 7,
            Side::Negative => pool.negative_remaining_units = 7,
        }
        let other_before = aggregate.remaining_main_units(respondent.side());
        aggregate
            .submit_cross_exam_question("甲乙丙".to_owned(), 1_000_000)
            .unwrap();
        assert_eq!(aggregate.remaining_main_units(questioner.side()), 4);
        assert_eq!(aggregate.current_word_cap(), 4);
        aggregate
            .commit_cross_exam_answer(
                "丁戊己庚辛壬".to_owned(),
                2_000_000,
                Some("提问方打断".to_owned()),
            )
            .unwrap();
        let answer = aggregate.transcripts.last().unwrap();
        assert_eq!(answer.effective_text, "丁戊己庚");
        assert_eq!(answer.overflow_text, "辛壬");
        assert_eq!(aggregate.remaining_main_units(questioner.side()), 0);
        assert_eq!(
            aggregate.remaining_main_units(respondent.side()),
            other_before
        );
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 7);
        assert_eq!(aggregate.cross_exam_sessions[0].used_ms, 3_000_000);
    }

    #[test]
    fn exact_word_overrides_survive_speech_rate_edits() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(8_001),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        let mut rules = crate::domain::ruleset_v2::traditional_four_on_four_v2();
        rules
            .clock_pools
            .iter_mut()
            .find(|pool| {
                pool.kind == crate::domain::ruleset_v2::ClockPoolKindV3::Main
                    && pool.side == Side::Affirmative
            })
            .unwrap()
            .unit_budget = Some(123);
        aggregate.submit_semantic_rules(rules).unwrap();
        assert_eq!(aggregate.remaining_main_units(Side::Affirmative), 123);
        aggregate
            .handle(MatchCommand::SetSpeechRate {
                speech_rate_per_minute: 300,
            })
            .unwrap();
        assert_eq!(aggregate.remaining_main_units(Side::Affirmative), 123);
        assert_eq!(aggregate.remaining_main_units(Side::Negative), 5_400);
    }

    #[test]
    fn free_debate_words_are_shared_by_teammates_and_last_side_can_continue() {
        let mut aggregate = ready_semantic_match();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        assert_eq!(
            aggregate.current_free_debate_units(),
            Some((960, 960, Side::Affirmative))
        );
        for pool in &mut aggregate.semantic_ruleset.as_mut().unwrap().clock_pools {
            if pool.kind == crate::domain::ruleset_v2::ClockPoolKindV3::FreeDebate {
                pool.unit_budget = Some(if pool.side == Side::Affirmative { 4 } else { 6 });
            }
        }
        let main_before = aggregate.word_budgets.clone();
        aggregate
            .submit_free_debate_turn(SlotId::Aff1, "甲乙".to_owned(), 9_000_000)
            .unwrap();
        aggregate
            .submit_free_debate_turn(SlotId::Neg1, "甲乙".to_owned(), 8_000_000)
            .unwrap();
        aggregate
            .submit_free_debate_turn(SlotId::Aff2, "丙丁戊己".to_owned(), 7_000_000)
            .unwrap();
        assert_eq!(aggregate.transcripts.last().unwrap().effective_text, "丙丁");
        assert_eq!(
            aggregate.current_free_debate_units(),
            Some((0, 4, Side::Negative))
        );
        aggregate
            .submit_free_debate_turn(SlotId::Neg2, "丙丁".to_owned(), 6_000_000)
            .unwrap();
        assert_eq!(
            aggregate.current_free_debate_units(),
            Some((0, 2, Side::Negative))
        );
        aggregate
            .submit_free_debate_turn(SlotId::Neg4, "戊己庚".to_owned(), 5_000_000)
            .unwrap();
        assert!(aggregate.free_debate_sessions[0].completed);
        assert_eq!(
            aggregate.free_debate_sessions[0].negative_remaining_units,
            0
        );
        assert_eq!(aggregate.word_budgets, main_before);
        assert_eq!(
            aggregate.free_debate_sessions[0].affirmative_remaining_ms,
            240_000
        );
        assert_eq!(
            aggregate.free_debate_sessions[0].negative_remaining_ms,
            240_000
        );
    }

    #[test]
    fn free_debate_exact_word_allowance_can_exceed_the_source_event_cap() {
        let mut aggregate = ready_semantic_match();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        assert_eq!(aggregate.schedule[aggregate.cursor].word_cap, 960);
        aggregate
            .semantic_ruleset
            .as_mut()
            .unwrap()
            .clock_pools
            .iter_mut()
            .find(|pool| {
                pool.kind == crate::domain::ruleset_v2::ClockPoolKindV3::FreeDebate
                    && pool.side == Side::Affirmative
            })
            .unwrap()
            .unit_budget = Some(1_003);
        assert_eq!(aggregate.current_word_cap(), 1_003);
        assert_eq!(aggregate.current_remaining_units(), 1_003);
        let main_before = aggregate.word_budgets.clone();
        aggregate
            .submit_free_debate_turn(SlotId::Aff1, "字".repeat(1_004), 9_000_000)
            .unwrap();
        let transcript = aggregate.transcripts.last().unwrap();
        assert_eq!(transcript.effective_units, 1_003);
        assert_eq!(transcript.effective_text, "字".repeat(1_003));
        assert_eq!(transcript.overflow_text, "字");
        assert_eq!(
            aggregate.current_free_debate_units(),
            Some((0, 960, Side::Negative))
        );
        assert_eq!(aggregate.word_budgets, main_before);
    }

    #[test]
    fn free_debate_source_pool_conversion_matches_the_displayed_allowance() {
        let mut aggregate = ready_semantic_match();
        aggregate
            .semantic_ruleset
            .as_mut()
            .unwrap()
            .clock_pools
            .iter_mut()
            .find(|pool| {
                pool.kind == crate::domain::ruleset_v2::ClockPoolKindV3::FreeDebate
                    && pool.side == Side::Affirmative
            })
            .unwrap()
            .initial_seconds = 300;
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            1200
        );
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        assert_eq!(aggregate.current_word_cap(), 1200);
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            1200
        );
        aggregate
            .submit_free_debate_turn(SlotId::Aff1, "甲乙丙".to_owned(), 9_000_000)
            .unwrap();
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            1197
        );
    }

    #[test]
    fn another_free_debate_event_displays_its_new_pool_not_the_previous_zero_pool() {
        let mut aggregate = ready_semantic_match();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        let mut next_event = aggregate.schedule[aggregate.cursor].clone();
        next_event.id = Uuid::from_u128(90_010);
        aggregate.schedule.push(next_event);
        aggregate.waive_current_free_debate_time().unwrap();
        aggregate.waive_current_free_debate_time().unwrap();
        assert_eq!(aggregate.free_debate_remaining_units(Side::Affirmative), 0);
        aggregate.cursor = aggregate.schedule.len() - 1;
        assert_eq!(aggregate.current_remaining_units(), 960);
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            960
        );
        assert_eq!(aggregate.free_debate_remaining_units(Side::Negative), 960);
    }

    #[test]
    fn free_debate_allowance_is_available_before_the_stage_and_retained_after_it_ends() {
        let mut aggregate = MatchAggregate::new(
            Uuid::from_u128(8_002),
            MatchMode::Training,
            "辩题",
            "材料",
            240,
        );
        let mut rules = crate::domain::ruleset_v2::traditional_four_on_four_v2();
        rules
            .clock_pools
            .iter_mut()
            .find(|pool| {
                pool.kind == crate::domain::ruleset_v2::ClockPoolKindV3::FreeDebate
                    && pool.side == Side::Affirmative
            })
            .unwrap()
            .unit_budget = Some(1_003);
        aggregate.replace_semantic_draft(rules).unwrap();
        assert!(aggregate.current_free_debate_units().is_none());
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            1_003
        );
        assert_eq!(aggregate.free_debate_remaining_units(Side::Negative), 960);
        aggregate.handle(MatchCommand::ConfirmRules).unwrap();
        aggregate
            .handle(MatchCommand::RecordTopicAnalysis {
                summary: "拆题".to_owned(),
            })
            .unwrap();
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side: None,
                assignments: training_assignments(),
            })
            .unwrap();
        aggregate.handle(MatchCommand::ConfirmLineup).unwrap();
        aggregate.handle(MatchCommand::MarkReady).unwrap();
        aggregate.handle(MatchCommand::Start).unwrap();
        assert!(aggregate.current_free_debate_units().is_none());
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            1_003
        );
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        aggregate
            .submit_free_debate_turn(SlotId::Aff1, "甲乙丙".to_owned(), 9_000_000)
            .unwrap();
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            1_000
        );
        assert_eq!(aggregate.free_debate_remaining_units(Side::Negative), 960);
        aggregate.waive_current_free_debate_time().unwrap();
        aggregate.waive_current_free_debate_time().unwrap();
        assert!(aggregate.current_free_debate_units().is_none());
        assert_eq!(aggregate.free_debate_remaining_units(Side::Affirmative), 0);
        assert_eq!(aggregate.free_debate_remaining_units(Side::Negative), 0);
        let restored = MatchAggregate::from_snapshot(aggregate.to_snapshot()).unwrap();
        assert_eq!(restored.free_debate_remaining_units(Side::Affirmative), 0);
        assert_eq!(restored.free_debate_remaining_units(Side::Negative), 0);
    }

    #[test]
    fn free_debate_allowance_accessor_converts_legacy_remaining_time() {
        let mut aggregate = ready_semantic_match();
        aggregate.metering_mode = MatchResourceMode::Time;
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            960
        );
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        aggregate
            .submit_free_debate_turn(SlotId::Aff1, "甲乙".to_owned(), 1_000)
            .unwrap();
        assert_eq!(
            aggregate.free_debate_remaining_units(Side::Affirmative),
            956
        );
        assert_eq!(aggregate.free_debate_remaining_units(Side::Negative), 960);
    }

    #[test]
    fn alternating_cross_exam_can_continue_when_one_team_runs_out_of_words() {
        let mut aggregate = ready_semantic_match();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::CrossExam)
            .unwrap();
        let event = aggregate.schedule[aggregate.cursor].clone();
        let stage = aggregate
            .ruleset
            .as_mut()
            .unwrap()
            .stages
            .iter_mut()
            .find(|stage| stage.id == event.stage_id)
            .unwrap();
        stage.eligible_questioners = SlotId::ALL.to_vec();
        stage.eligible_respondents = SlotId::ALL.to_vec();
        stage.initial_questioning_side = Some(Side::Affirmative);
        stage.max_cross_exam_rounds = None;
        let budgets = aggregate.word_budgets.as_mut().unwrap();
        budgets.affirmative_remaining_units = 4;
        budgets.negative_remaining_units = 6;
        aggregate
            .submit_cross_exam_question("甲乙丙丁戊".to_owned(), 9_000_000)
            .unwrap();
        assert_eq!(
            aggregate.cursor,
            aggregate
                .schedule
                .iter()
                .position(|candidate| candidate.id == event.id)
                .unwrap()
        );
        assert_eq!(aggregate.cross_exam_sessions[0].total_units, 10);
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 4);
        assert_eq!(
            aggregate.cross_exam_sessions[0].phase,
            CrossExamSessionPhase::Selecting
        );
        assert_eq!(
            aggregate.current_cross_exam_roles().unwrap().0.side(),
            Side::Negative
        );
        assert_eq!(aggregate.current_word_cap(), 6);
        assert_eq!(aggregate.cross_exam_exchanges[0].question_text, "甲乙丙丁");

        aggregate
            .submit_cross_exam_question("己庚".to_owned(), 8_000_000)
            .unwrap();
        aggregate
            .commit_cross_exam_answer("辛壬癸子丑".to_owned(), 7_000_000, None)
            .unwrap();
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 10);
        assert_eq!(
            aggregate.cross_exam_sessions[0].phase,
            CrossExamSessionPhase::Completed
        );
        assert_eq!(aggregate.remaining_main_units(Side::Affirmative), 0);
        assert_eq!(aggregate.remaining_main_units(Side::Negative), 0);
        assert_eq!(
            aggregate.schedule[aggregate.cursor].kind,
            crate::domain::ruleset::StageKind::FreeDebate
        );
    }

    #[test]
    fn free_debate_word_waiver_preserves_the_other_teams_allowance() {
        let mut aggregate = ready_semantic_match();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == crate::domain::ruleset::StageKind::FreeDebate)
            .unwrap();
        let cursor = aggregate.cursor;
        assert_eq!(
            aggregate.waive_current_free_debate_time().unwrap(),
            Side::Affirmative
        );
        assert_eq!(
            aggregate.current_free_debate_units(),
            Some((0, 960, Side::Negative))
        );
        aggregate
            .submit_free_debate_turn(SlotId::Neg3, "甲乙丙".to_owned(), 9_000_000)
            .unwrap();
        assert_eq!(
            aggregate.current_free_debate_units(),
            Some((0, 957, Side::Negative))
        );
        assert_eq!(
            aggregate.waive_current_free_debate_time().unwrap(),
            Side::Negative
        );
        assert_eq!(aggregate.cursor, cursor + 1);
        assert!(aggregate.free_debate_sessions[0].completed);
        let restored = MatchAggregate::from_snapshot(aggregate.to_snapshot()).unwrap();
        assert_eq!(
            restored.free_debate_sessions,
            aggregate.free_debate_sessions
        );
    }
}

fn schedule_reject(_error: ScheduleError) -> CommandReject {
    CommandReject::ScheduleNotExecutable
}

fn content_hash<T: serde::Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("contract json");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        encoded.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    encoded
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LineupSeal<'a> {
    rules_hash: &'a str,
    topic: &'a str,
    materials: &'a str,
    speech_rate_per_minute: u32,
    assignments: &'a [DebaterAssignmentV1; 8],
    event_ids: Vec<Uuid>,
}

pub fn training_assignments() -> Vec<DebaterAssignmentV1> {
    default_assignments().to_vec()
}

pub fn scoring_assignments(side: Side) -> Vec<DebaterAssignmentV1> {
    let mut assignments = default_assignments();
    let slot = match side {
        Side::Affirmative => SlotId::Aff1,
        Side::Negative => SlotId::Neg1,
    };
    assignments[slot.index()].controller = Controller::Human;
    assignments.to_vec()
}
