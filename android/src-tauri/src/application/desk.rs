use std::sync::atomic::AtomicBool;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use uuid::Uuid;

use crate::domain::calibration::{assess, grade, median, passage_for, TEST_MS};
use crate::domain::control_policy::ControlScope;
use crate::domain::debate_context::RolePacket;
use crate::domain::events::{Controller, DebaterAssignmentV1};
use crate::domain::floor::{interrupt_at, DebateFloor, InterruptError};
use crate::domain::match_engine::{
    CommandReject, MatchAggregate, MatchCommand, MatchPhase, MatchResourceMode,
};
use crate::domain::report::{
    export_public, PublicReport, ReportDimension, ReportStageEvaluation, REPORT_FORMULA,
};
use crate::domain::review::{combine, CitedAdjustment, StageMark, DIMENSIONS};
use crate::domain::rules_check::{check_ruleset, RulesProblem};
use crate::domain::ruleset::{
    expand_schedule, four_on_four_template, ClockOwner, MatchMode, RulesetV1, Side, SlotId,
    StageKind,
};
use crate::domain::ruleset_v2::{
    compile_v2_to_v1, manual_ruleset_v2, ruleset_v2_from_v1, team_bank_ruleset_v3_from_source,
    traditional_four_on_four_v2, validate_ruleset_v2, InterruptPolicyV2, RuleIssueV2, RulesetV2,
};
use crate::domain::scoring::{score_program, ProgramInput};
use crate::domain::text_units::{count_effective_units, split_effective};
use crate::domain::timing::DEFAULT_SPEECH_RATE;
use crate::domain::visibility::{
    visible_role_packets, visible_strategies, Difficulty, StrategyPacket,
};
use crate::infrastructure::ai::{
    AiContext, DeepSeekTransport, Gateway, JudgementWinner, MockGateway, ModelConfig, ThreadSleep,
    UreqHttp, DEFAULT_MODEL_ID,
};
use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};
use crate::infrastructure::db::AppSettings;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRow {
    pub id: String,
    pub kind: String,
    pub speaker: String,
    pub duration_seconds: u32,
    pub word_cap: u32,
    pub interruptible: bool,
    pub controller: String,
    pub started: bool,
}

struct LiveSemanticResult {
    marks: Vec<StageMark>,
    adjustments: Vec<CitedAdjustment>,
    team: String,
    stage_evaluations: Vec<ReportStageEvaluation>,
    review_summary: String,
    team_summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRow {
    pub controller: Option<Controller>,
    pub id: String,
    pub slot: String,
    pub effective_text: String,
    pub overflow_text: String,
    pub interrupted_tail: String,
    pub responds_to: String,
    pub response_target_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatRow {
    pub default_controller: String,
    pub index: usize,
    pub name: String,
    pub duty: String,
    pub side: String,
    pub controller: String,
    pub speaking: bool,
    pub claimable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRow {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRow {
    pub id: String,
    pub topic: String,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePacketRow {
    pub slot: String,
    pub identity: String,
    pub duty: String,
    pub main_argument: String,
    pub backup_rebuttal: String,
    pub cross_exam_target: String,
    pub defense_floor: String,
    pub summary_callback: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSnapshot {
    pub public_issues: Vec<crate::domain::issue_ledger::PublicIssue>,
    pub match_id: String,
    pub profiles: Vec<ProfileRow>,
    pub active_profile_id: String,
    pub recent: Vec<MatchRow>,
    pub phase: String,
    pub topic: String,
    pub mode: String,
    pub user_side: String,
    pub difficulty: String,
    pub preparation_ready: bool,
    pub rules_error: String,
    pub rules_confirmed: bool,
    pub lineup_confirmed: bool,
    pub schedule: Vec<ScheduleRow>,
    pub rule_draft: Option<RulesetV2>,
    pub rule_issues: Vec<RuleIssueV2>,
    pub rules_can_confirm: bool,
    pub cursor: usize,
    pub remaining_ms: u64,
    pub metering_mode: String,
    pub speech_rate_per_minute: u32,
    pub remaining_units: u32,
    pub current_word_cap: u32,
    pub affirmative_main_units: u32,
    pub negative_main_units: u32,
    pub affirmative_free_units: u32,
    pub negative_free_units: u32,
    pub charged_ms: u64,
    pub ai_wait_ms: u64,
    pub transcripts: Vec<TranscriptRow>,
    pub own_strategy: Option<String>,
    pub opponent_strategy: Option<String>,
    pub role_packets: Vec<RolePacketRow>,
    pub message: String,
    pub report: Option<PublicReport>,
    pub standard: bool,
    pub current_speaker: String,
    pub current_human: bool,
    pub current_window_id: String,
    pub control_revision: u64,
    pub current_interruptible: bool,
    pub uses_live_model: bool,
    pub affirmative_ms: u64,
    pub negative_ms: u64,
    pub affirmative_main_ms: u64,
    pub negative_main_ms: u64,
    pub floor_holder: String,
    pub calibration_passage: String,
    pub baseline_cpm: f64,
    pub calibration_rounds_completed: usize,
    pub low_confidence: bool,
    pub materials: String,
    pub awaiting_seat: bool,
    pub seats: Vec<SeatRow>,
    pub model_settings: crate::application::model_settings::ModelSettingsDto,
}

pub struct Desk {
    pub profile_id: Option<Uuid>,
    pub aggregate: Option<MatchAggregate>,
    pub difficulty: Difficulty,
    strategies: Option<StrategyPacket>,
    role_packets: Option<Vec<RolePacket>>,
    pub report: Option<PublicReport>,
    pub message: String,
    pub rules_error: String,
    pub floor: DebateFloor,
    event_started: Option<Instant>,
    in_cap_units: u32,
    cap_units: u32,
    overflow_units: u32,
    reverse_units: u32,
    reverse_actions: u32,
    forward_units: u32,
    pause_ms: u64,
    input_ms: u64,
    salt: u128,
    pub live: bool,
    pub model_config: ModelConfig,
    interrupted_tails: Vec<(Uuid, String)>,
    interrupted_events: Vec<Uuid>,
    pasted_ids: Vec<Uuid>,
    baseline_cpm: Option<f64>,
    calibration_samples: Vec<f64>,
    low_confidence: bool,
    calibration_attempt: u32,
    pub pending_cpm: Option<f64>,
    preparation_revision: u64,
    pub(crate) turn_revision: u64,
}

#[derive(Clone)]
pub(crate) struct PreparationBoundary {
    revision: u64,
    profile_id: Option<Uuid>,
    snapshot: Option<crate::domain::match_engine::MatchSnapshotV1>,
    difficulty: Difficulty,
    live: bool,
    model_config: ModelConfig,
}

impl Desk {
    pub fn new() -> Self {
        Self {
            profile_id: None,
            aggregate: None,
            difficulty: Difficulty::Standard,
            strategies: None,
            role_packets: None,
            report: None,
            message: String::new(),
            rules_error: String::new(),
            floor: DebateFloor::new(),
            event_started: None,
            in_cap_units: 0,
            cap_units: 0,
            overflow_units: 0,
            reverse_units: 0,
            reverse_actions: 0,
            forward_units: 0,
            pause_ms: 0,
            input_ms: 0,
            salt: fresh_salt(),
            live: false,
            model_config: ModelConfig::from_settings(&AppSettings::default()),
            interrupted_tails: Vec::new(),
            interrupted_events: Vec::new(),
            pasted_ids: Vec::new(),
            baseline_cpm: None,
            calibration_samples: Vec::new(),
            low_confidence: false,
            calibration_attempt: 0,
            pending_cpm: None,
            preparation_revision: 0,
            turn_revision: 0,
        }
    }

    pub fn view(&self) -> UiSnapshot {
        let aggregate = self.aggregate.as_ref();
        let mode = aggregate
            .map(|state| state.mode)
            .unwrap_or(MatchMode::Training);
        let user_side = aggregate
            .and_then(|state| state.user_side)
            .unwrap_or(Side::Affirmative);
        let visible = self
            .strategies
            .as_ref()
            .map(|packet| visible_strategies(mode, user_side, packet));
        let current = aggregate.and_then(|state| state.schedule.get(state.cursor));
        let current_cross = aggregate.and_then(current_cross_presentation);
        let current_speaker = current_cross
            .map(|cross| cross.speaker)
            .or_else(|| current.map(|event| event.speaker));
        let free_clock = aggregate.and_then(MatchAggregate::current_free_debate_clock);
        let current_human = current_speaker
            .and_then(|slot| {
                aggregate.map(|state| {
                    current_cross
                        .and_then(|cross| cross.controller_for(slot))
                        .unwrap_or(state.effective_controller(slot))
                        == Controller::Human
                })
            })
            .unwrap_or(false);
        let rule_draft = aggregate.and_then(|state| state.semantic_ruleset.clone());
        let rule_issues = rule_draft
            .as_ref()
            .map(validate_ruleset_v2)
            .unwrap_or_default();
        let rules_can_confirm = aggregate.and_then(|state| state.ruleset.as_ref()).is_some()
            && !rule_issues
                .iter()
                .any(|issue| issue.severity == crate::domain::ruleset_v2::IssueSeverity::Blocking);
        UiSnapshot {
            public_issues: self
                .aggregate
                .as_ref()
                .map(|a| {
                    crate::domain::issue_ledger::with_links(
                        &a.transcripts,
                        &a.public_response_links,
                    )
                })
                .unwrap_or_default(),
            match_id: aggregate
                .map(|state| state.id.to_string())
                .unwrap_or_default(),
            profiles: Vec::new(),
            active_profile_id: self.profile_id.map(|id| id.to_string()).unwrap_or_default(),
            recent: Vec::new(),
            phase: aggregate
                .map(|state| enum_label(&state.phase))
                .unwrap_or_else(|| "draft_setup".to_owned()),
            topic: aggregate
                .map(|state| state.topic.clone())
                .unwrap_or_default(),
            mode: aggregate
                .map(|state| enum_label(&state.mode))
                .unwrap_or_else(|| "training".to_owned()),
            user_side: aggregate
                .and_then(|state| state.user_side)
                .map(|side| enum_label(&side))
                .unwrap_or_default(),
            difficulty: enum_label(&self.difficulty),
            preparation_ready: self.preparation_ready(),
            rules_error: self.rules_error.clone(),
            rules_confirmed: aggregate
                .and_then(|state| state.rules_hash.as_ref())
                .is_some(),
            lineup_confirmed: aggregate
                .and_then(|state| state.lineup_hash.as_ref())
                .is_some(),
            schedule: aggregate
                .map(|state| {
                    preview_schedule(state)
                        .iter()
                        .enumerate()
                        .map(|(index, event)| ScheduleRow {
                            id: event.id.to_string(),
                            kind: enum_label(&event.kind),
                            speaker: enum_label(&if index == state.cursor {
                                current_cross
                                    .map(|cross| cross.speaker)
                                    .unwrap_or(event.speaker)
                            } else {
                                event.speaker
                            }),
                            duration_seconds: event.duration_seconds,
                            word_cap: event.word_cap,
                            interruptible: event.interruptible,
                            controller: enum_label(&if index == state.cursor {
                                current_cross
                                    .and_then(|cross| cross.controller_for(cross.speaker))
                                    .unwrap_or(state.effective_controller(event.speaker))
                            } else {
                                state.effective_controller(event.speaker)
                            }),
                            started: index < state.cursor
                                || (index == state.cursor
                                    && matches!(state.phase, MatchPhase::InProgress)),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            rule_draft,
            rule_issues,
            rules_can_confirm,
            cursor: aggregate.map(|state| state.cursor).unwrap_or(0),
            remaining_ms: self.remaining_ms(),
            metering_mode: aggregate
                .map(|state| enum_label(&state.metering_mode))
                .unwrap_or_else(|| "words".to_owned()),
            speech_rate_per_minute: aggregate
                .map(|state| state.speech_rate_per_minute)
                .unwrap_or(DEFAULT_SPEECH_RATE),
            remaining_units: aggregate
                .map(MatchAggregate::current_remaining_units)
                .unwrap_or(0),
            current_word_cap: aggregate.map(MatchAggregate::current_word_cap).unwrap_or(0),
            affirmative_main_units: aggregate
                .map(|state| state.remaining_main_units(Side::Affirmative))
                .unwrap_or(4_320),
            negative_main_units: aggregate
                .map(|state| state.remaining_main_units(Side::Negative))
                .unwrap_or(4_320),
            affirmative_free_units: aggregate
                .map(|state| state.free_debate_remaining_units(Side::Affirmative))
                .unwrap_or(960),
            negative_free_units: aggregate
                .map(|state| state.free_debate_remaining_units(Side::Negative))
                .unwrap_or(960),
            charged_ms: aggregate.map(|state| state.charged_ms).unwrap_or(0),
            ai_wait_ms: aggregate.map(|state| state.ai_wait_ms).unwrap_or(0),
            transcripts: {
                let tails = &self.interrupted_tails;
                aggregate
                    .map(|state| {
                        state
                            .transcripts
                            .iter()
                            .enumerate()
                            .map(|(index, entry)| TranscriptRow {
                                controller: entry.controller,
                                id: entry.id.to_string(),
                                slot: enum_label(&entry.slot),
                                effective_text: entry.effective_text.clone(),
                                overflow_text: entry.overflow_text.clone(),
                                interrupted_tail: tails
                                    .iter()
                                    .find(|(id, _)| *id == entry.id)
                                    .map(|(_, tail)| tail.clone())
                                    .unwrap_or_default(),
                                responds_to: index
                                    .checked_sub(1)
                                    .and_then(|previous| state.transcripts.get(previous))
                                    .map(|previous| previous.id.to_string())
                                    .unwrap_or_default(),
                                response_target_text: entry.response_target_text.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            },
            own_strategy: visible.as_ref().and_then(|item| item.own.clone()),
            opponent_strategy: visible.as_ref().and_then(|item| item.opponent.clone()),
            role_packets: self
                .role_packets
                .as_ref()
                .map(|packets| {
                    visible_role_packets(mode, user_side, packets)
                        .into_iter()
                        .map(|packet| RolePacketRow {
                            slot: enum_label(&packet.slot),
                            identity: packet.identity,
                            duty: packet.duty,
                            main_argument: packet.main_argument,
                            backup_rebuttal: packet.backup_rebuttal,
                            cross_exam_target: packet.cross_exam_target,
                            defense_floor: packet.defense_floor,
                            summary_callback: packet.summary_callback,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            message: self.message.clone(),
            report: self.report.clone(),
            standard: aggregate.map(|state| !state.non_standard).unwrap_or(true),
            current_speaker: current_speaker
                .map(|slot| enum_label(&slot))
                .unwrap_or_default(),
            current_human,
            current_window_id: aggregate
                .map(MatchAggregate::control_window_id)
                .unwrap_or_default(),
            control_revision: aggregate.map_or(0, |state| state.control_revision),
            current_interruptible: current
                .is_some_and(|event| event.interruptible && event.kind == StageKind::CrossExam),
            uses_live_model: self.live,
            affirmative_ms: free_clock
                .map(|clock| clock.0)
                .unwrap_or(self.floor.affirmative_ms),
            negative_ms: free_clock
                .map(|clock| clock.1)
                .unwrap_or(self.floor.negative_ms),
            affirmative_main_ms: aggregate
                .map(|state| state.affirmative_main_remaining_ms)
                .unwrap_or(crate::domain::match_engine::MAIN_TEAM_POOL_MS),
            negative_main_ms: aggregate
                .map(|state| state.negative_main_remaining_ms)
                .unwrap_or(crate::domain::match_engine::MAIN_TEAM_POOL_MS),
            floor_holder: free_clock
                .map(|clock| enum_label(&clock.2))
                .or_else(|| self.floor.holder.map(|side| enum_label(&side)))
                .unwrap_or_default(),
            calibration_passage: self.passage(),
            baseline_cpm: self.baseline_cpm.unwrap_or(0.0),
            calibration_rounds_completed: self.calibration_samples.len(),
            low_confidence: self.low_confidence,
            materials: aggregate
                .map(|state| state.materials.clone())
                .unwrap_or_default(),
            awaiting_seat: aggregate.is_some_and(|state| {
                state.phase == MatchPhase::InProgress
                    && state.schedule.get(state.cursor).is_some_and(|event| {
                        if event.kind == StageKind::FreeDebate {
                            self.event_started.is_none()
                        } else {
                            !current_human
                        }
                    })
            }),
            seats: aggregate.map(seat_rows).unwrap_or_default(),
            model_settings: crate::application::model_settings::ModelSettingsDto::from_parts(
                &AppSettings::default(),
                false,
            ),
        }
    }

    pub fn strategies_ref(&self) -> Option<&StrategyPacket> {
        self.strategies.as_ref()
    }

    pub fn role_packets_ref(&self) -> Option<&[RolePacket]> {
        self.role_packets.as_deref()
    }

    pub fn adopt_profile(&mut self, id: Uuid) {
        if self.profile_id == Some(id) {
            self.message = "这份档案已是当前档案".to_owned();
            return;
        }
        self.profile_id = Some(id);
        self.clear_match();
        self.message = "已选用这份档案。未保存的当前赛程已放下。".to_owned();
    }

    pub fn release_profile(&mut self, id: Uuid) {
        if self.profile_id == Some(id) {
            self.profile_id = None;
            self.clear_match();
        }
        self.message = "档案已删除".to_owned();
    }

    pub fn restore_match(&mut self, mut aggregate: MatchAggregate, report: Option<PublicReport>) {
        let scoring_state = aggregate.scoring_state.clone();
        let preparation = aggregate.preparation.clone();
        self.clear_match();
        if let Some(saved) = preparation {
            self.difficulty = saved.difficulty;
            self.strategies = saved.strategies;
            self.role_packets = saved.role_packets;
        }
        let missing_preparation = !self.preparation_ready()
            && matches!(
                aggregate.phase,
                MatchPhase::TopicAnalyzed
                    | MatchPhase::ControlAssigned
                    | MatchPhase::Calibrated
                    | MatchPhase::LineupConfirmed
                    | MatchPhase::Ready
            );
        if missing_preparation {
            aggregate
                .invalidate_preparation()
                .expect("preparation phase checked");
            self.strategies = None;
            self.role_packets = None;
        }
        if let Some(saved) = scoring_state {
            self.in_cap_units = saved.in_cap_units;
            self.cap_units = saved.cap_units;
            self.overflow_units = saved.overflow_units;
            self.reverse_units = saved.reverse_units;
            self.reverse_actions = saved.reverse_actions;
            self.forward_units = saved.forward_units;
            self.pause_ms = saved.pause_ms;
            self.input_ms = saved.input_ms;
            self.baseline_cpm = saved.baseline_milli_cpm.map(|value| value as f64 / 1_000.0);
            self.calibration_samples = saved
                .calibration_samples_milli_cpm
                .iter()
                .map(|value| *value as f64 / 1_000.0)
                .collect();
            self.low_confidence = saved.low_confidence;
            self.pasted_ids = saved.pasted_ids;
        } else if aggregate.mode == MatchMode::Scoring {
            // Legacy saves have no input facts; never silently claim high confidence.
            self.low_confidence = true;
        }
        if aggregate.mode == MatchMode::Scoring
            && matches!(
                aggregate.phase,
                MatchPhase::InProgress | MatchPhase::RecoveryRequired
            )
        {
            aggregate.non_standard = true;
            if aggregate.phase == MatchPhase::RecoveryRequired {
                aggregate.phase = MatchPhase::InProgress;
            }
        }
        self.aggregate = Some(aggregate);
        self.report = report;
        self.event_started = None;
        self.message = "已恢复到最近一次正式保存的回合；未提交输入不会恢复".to_owned();
        if missing_preparation {
            self.message = "这份赛前存档缺少完整拆题策略和辩位任务，请重新拆题并确认阵容；原赛制和人工辩位已保留。".to_owned();
        }
        self.persist_preparation();
    }

    fn persist_scoring_state(&mut self) {
        if let Some(aggregate) = self.aggregate.as_mut() {
            aggregate.scoring_state = Some(crate::domain::match_engine::HumanScoringStateV3 {
                in_cap_units: self.in_cap_units,
                cap_units: self.cap_units,
                overflow_units: self.overflow_units,
                reverse_units: self.reverse_units,
                reverse_actions: self.reverse_actions,
                forward_units: self.forward_units,
                pause_ms: self.pause_ms,
                input_ms: self.input_ms,
                baseline_milli_cpm: self
                    .baseline_cpm
                    .map(|value| (value * 1_000.0).round() as u64),
                calibration_samples_milli_cpm: self
                    .calibration_samples
                    .iter()
                    .map(|value| (value * 1_000.0).round() as u64)
                    .collect(),
                low_confidence: self.low_confidence,
                pasted_ids: self.pasted_ids.clone(),
            });
        }
    }

    fn clear_match(&mut self) {
        self.preparation_revision = self.preparation_revision.wrapping_add(1);
        self.turn_revision = self.turn_revision.wrapping_add(1);
        self.aggregate = None;
        self.difficulty = Difficulty::Standard;
        self.strategies = None;
        self.role_packets = None;
        self.report = None;
        self.rules_error.clear();
        self.floor = DebateFloor::new();
        self.event_started = None;
        self.in_cap_units = 0;
        self.cap_units = 0;
        self.overflow_units = 0;
        self.reverse_units = 0;
        self.reverse_actions = 0;
        self.forward_units = 0;
        self.pause_ms = 0;
        self.input_ms = 0;
        self.interrupted_tails.clear();
        self.interrupted_events.clear();
        self.pasted_ids.clear();
        self.baseline_cpm = None;
        self.calibration_samples.clear();
        self.low_confidence = false;
        self.calibration_attempt = 0;
        self.pending_cpm = None;
    }

    pub fn release_match(&mut self, match_id: Uuid) -> Result<(), String> {
        if self.aggregate.as_ref().map(|state| state.id) != Some(match_id) {
            return Err("当前比赛已经变化".to_owned());
        }
        self.clear_match();
        self.message = "本场已中止并保存".to_owned();
        Ok(())
    }

    pub fn release_completed_match(&mut self) -> Result<(), String> {
        let phase = self.aggregate.as_ref().map(|state| state.phase);
        if !matches!(phase, Some(MatchPhase::ReportReady | MatchPhase::Aborted)) {
            return Err("只有已出报告或已中止的比赛可以放下".to_owned());
        }
        self.clear_match();
        self.message = "已返回赛前工作台".to_owned();
        Ok(())
    }

    pub fn open_template(
        &mut self,
        mode: MatchMode,
        topic: String,
        materials: String,
        user_side: Option<Side>,
        human_slots: &[usize],
        difficulty: Difficulty,
    ) -> Result<(), String> {
        self.clear_match();
        self.difficulty = difficulty;
        self.report = None;
        self.strategies = None;
        self.role_packets = None;
        self.rules_error.clear();
        let mut semantic_ruleset = traditional_four_on_four_v2();
        let mut ruleset =
            compile_v2_to_v1(&semantic_ruleset).map_err(|_| "默认赛制无法编译".to_owned())?;
        if materials.contains("自由辩可打断") {
            if let Some(stage) = ruleset
                .stages
                .iter_mut()
                .find(|stage| stage.kind == StageKind::FreeDebate)
            {
                stage.interruptible = true;
            }
            if let Some(stage) = semantic_ruleset.stages.iter_mut().find(|stage| {
                stage.semantic_type == crate::domain::ruleset_v2::SemanticStageType::FreeDebate
            }) {
                stage.interrupt_policy = InterruptPolicyV2::QuestionerMayStopAnswer;
            }
        }
        if let Err(problem) = check_ruleset(&ruleset) {
            self.rules_error = rules_problem_text(problem);
        }
        let mut aggregate =
            MatchAggregate::new(self.next_id(), mode, topic, materials, DEFAULT_SPEECH_RATE);
        aggregate.reseed_ids(self.next_id().as_u128());
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(ruleset),
            })
            .map_err(reject)?;
        aggregate.semantic_ruleset = Some(semantic_ruleset);
        aggregate.user_side = user_side;
        let _ = human_slots;
        self.aggregate = Some(aggregate);
        self.persist_preparation();
        self.message = "规则已解析，确认后才能继续".to_owned();
        Ok(())
    }

    pub fn apply_recognized_rules(
        &mut self,
        mut ruleset: RulesetV1,
        source: &str,
        confidence: u32,
        setup: RuleSetup,
    ) -> Result<(), String> {
        if confidence != 0 {
            ruleset.source_text = source.trim().to_owned();
        }
        normalize_stage_times(&mut ruleset);
        if let Err(problem) = check_ruleset(&ruleset) {
            self.rules_error = rules_problem_text(problem);
            return Err(self.rules_error.clone());
        }
        self.clear_match();
        self.difficulty = setup.difficulty;
        self.report = None;
        self.strategies = None;
        self.role_packets = None;
        self.rules_error.clear();
        let mut aggregate = MatchAggregate::new(
            self.next_id(),
            setup.mode,
            setup.topic,
            setup.materials,
            DEFAULT_SPEECH_RATE,
        );
        aggregate.reseed_ids(self.next_id().as_u128());
        let semantic_ruleset = if source.contains("双方各18分钟")
            && source.contains("自由辩")
            && source.contains("各4分钟")
        {
            team_bank_ruleset_v3_from_source(source)
        } else {
            ruleset_v2_from_v1(&ruleset)
        };
        aggregate
            .replace_semantic_draft(semantic_ruleset)
            .map_err(reject)?;
        aggregate.user_side = setup.user_side;
        self.aggregate = Some(aggregate);
        self.persist_preparation();
        self.message = if confidence == 100 {
            "已按原文读出每人的秒数。请核对赛程后再确认规则。".to_owned()
        } else if confidence == 80 {
            "模型未解析成功，已用本地读出的秒数作回退。请核对赛程后再确认规则。".to_owned()
        } else {
            format!("赛制已识别，置信度 {confidence}。请核对下面的赛程，再确认规则。")
        };
        Ok(())
    }

    pub fn open_manual_rules_draft(
        &mut self,
        source: &str,
        setup: RuleSetup,
        recognition_error: String,
    ) -> Result<(), String> {
        self.clear_match();
        self.difficulty = setup.difficulty;
        self.report = None;
        self.strategies = None;
        self.role_packets = None;
        let mut aggregate = MatchAggregate::new(
            self.next_id(),
            setup.mode,
            setup.topic,
            setup.materials,
            DEFAULT_SPEECH_RATE,
        );
        aggregate.reseed_ids(self.next_id().as_u128());
        aggregate
            .replace_semantic_draft(manual_ruleset_v2(source))
            .map_err(reject)?;
        aggregate.user_side = setup.user_side;
        self.aggregate = Some(aggregate);
        self.persist_preparation();
        self.rules_error = format!("自动识别未完成：{recognition_error}");
        self.message =
            "已保留原文并生成可编辑补救草稿。逐项选择环节职责、辩位和时间后即可确认。".to_owned();
        Ok(())
    }

    pub fn confirm_rules(&mut self) -> Result<(), String> {
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        if let Some(ruleset) = &aggregate.semantic_ruleset {
            if let Some(issue) = validate_ruleset_v2(ruleset)
                .into_iter()
                .find(|issue| issue.severity == crate::domain::ruleset_v2::IssueSeverity::Blocking)
            {
                self.rules_error = issue.message;
                return Err(self.rules_error.clone());
            }
        }
        if let Some(ruleset) = aggregate.ruleset.clone() {
            if let Err(problem) = check_ruleset(&ruleset) {
                self.rules_error = rules_problem_text(problem);
                return Err(self.rules_error.clone());
            }
        }
        aggregate
            .handle(MatchCommand::ConfirmRules)
            .map_err(reject)?;
        self.strategies = None;
        self.role_packets = None;
        self.persist_preparation();
        self.rules_error.clear();
        self.message = "规则已确认".to_owned();
        Ok(())
    }

    pub fn edit_topic(&mut self, topic: String) -> Result<(), String> {
        let aggregate = self.aggregate.as_ref().ok_or("还没有赛程")?;
        self.update_preparation(RuleSetup {
            mode: aggregate.mode,
            topic,
            materials: aggregate.materials.clone(),
            user_side: aggregate.user_side,
            difficulty: self.difficulty,
        })
    }

    #[cfg(test)]
    pub fn analyze(&mut self) -> Result<(), String> {
        let topic = self
            .aggregate
            .as_ref()
            .map(|state| state.topic.clone())
            .unwrap_or_default();
        let rules_text = self
            .aggregate
            .as_ref()
            .and_then(|state| state.ruleset.as_ref())
            .map(|ruleset| ruleset.source_text.clone())
            .unwrap_or_default();
        let context = AiContext {
            topic,
            rules_text,
            materials: self
                .aggregate
                .as_ref()
                .map(|state| state.materials.clone())
                .unwrap_or_default(),
            own_strategy: self.difficulty.constraint().to_owned(),
            ..AiContext::default()
        };
        let (summary, affirmative, negative, packets) = if self.live {
            let gateway = self.live_gateway()?;
            self.strategies_from(&gateway, context)?
        } else {
            let gateway = MockGateway::mock(self.model_config.clone());
            self.strategies_from(&gateway, context)?
        };
        self.store_analysis(summary, affirmative, negative, packets)
    }

    pub fn prepare_analysis(&self) -> Result<(bool, ModelConfig, AiContext, String), String> {
        let phase = self.aggregate.as_ref().map(|state| state.phase);
        if phase != Some(MatchPhase::RulesConfirmed) {
            return Err("先确认规则，再拆题".to_owned());
        }
        if self
            .aggregate
            .as_ref()
            .map(|state| state.topic.trim().is_empty())
            .unwrap_or(true)
        {
            return Err("先填写辩题".to_owned());
        }
        let mut config = self.model_config.clone();
        config.timeout_ms = 120_000;
        let topic = self
            .aggregate
            .as_ref()
            .map(|state| state.topic.clone())
            .unwrap_or_default();
        let rules_text = self
            .aggregate
            .as_ref()
            .and_then(|state| state.ruleset.as_ref())
            .map(|ruleset| ruleset.source_text.clone())
            .unwrap_or_default();
        let materials = self
            .aggregate
            .as_ref()
            .map(|state| state.materials.clone())
            .unwrap_or_default();
        Ok((
            self.live,
            config,
            AiContext {
                topic,
                rules_text,
                materials,
                ..AiContext::default()
            },
            self.difficulty.constraint().to_owned(),
        ))
    }

    pub fn store_analysis(
        &mut self,
        summary: String,
        affirmative: String,
        negative: String,
        packets: Vec<RolePacket>,
    ) -> Result<(), String> {
        if self.aggregate.as_ref().map(|state| state.phase) != Some(MatchPhase::RulesConfirmed) {
            return Err("拆题结果已因赛前状态变化失效，请重新拆题".to_owned());
        }
        if affirmative.trim().is_empty()
            || negative.trim().is_empty()
            || !complete_role_packets(&packets)
        {
            return Err("拆题未生成双方完整策略和八位辩手任务，请重试".to_owned());
        }
        self.aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .handle(MatchCommand::RecordTopicAnalysis { summary })
            .map_err(reject)?;
        self.strategies = Some(StrategyPacket {
            affirmative,
            negative,
        });
        self.role_packets = Some(packets);
        self.persist_preparation();
        self.message = "拆题完成，双方策略与辩位任务已分配".to_owned();
        Ok(())
    }

    pub fn revise_schedule_row(
        &mut self,
        index: usize,
        speaker: &str,
        duration_seconds: u32,
    ) -> Result<(), String> {
        if !(1..=3_600).contains(&duration_seconds) {
            return Err("时长要在 1 到 3600 秒之间".to_owned());
        }
        let slot = slot_from_label(speaker)?;
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        if matches!(
            aggregate.phase,
            MatchPhase::InProgress
                | MatchPhase::MatchComplete
                | MatchPhase::Scoring
                | MatchPhase::ReportReady
                | MatchPhase::RecoveryRequired
                | MatchPhase::Aborted
        ) {
            return Err("开赛后不能改赛程".to_owned());
        }
        let mut ruleset = aggregate.ruleset.clone().ok_or("还没有赛制")?;
        let shared = edit_schedule_row(&mut ruleset, index, slot, duration_seconds)?;
        if let Err(problem) = check_ruleset(&ruleset) {
            return Err(rules_problem_text(problem));
        }
        aggregate
            .handle(MatchCommand::SubmitParsedRules {
                ruleset: Box::new(ruleset),
            })
            .map_err(reject)?;
        self.strategies = None;
        self.role_packets = None;
        self.report = None;
        self.rules_error.clear();
        self.persist_preparation();
        self.message = if shared {
            format!("赛程已修改。这一环节的各轮时长都是 {duration_seconds} 秒。请再确认规则。")
        } else {
            "赛程已修改，请再确认规则。".to_owned()
        };
        Ok(())
    }

    pub fn replace_semantic_rules(&mut self, ruleset: RulesetV2) -> Result<(), String> {
        if ruleset.schema_version != crate::domain::ruleset_v2::RULESET_V2_SCHEMA_VERSION {
            return Err("赛制版本无效".to_owned());
        }
        self.aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .replace_semantic_draft(ruleset)
            .map_err(reject)?;
        self.strategies = None;
        self.role_packets = None;
        self.report = None;
        self.rules_error.clear();
        self.persist_preparation();
        self.message = "赛程草稿已更新，请处理红色问题后确认规则。".to_owned();
        Ok(())
    }

    pub fn passage(&self) -> String {
        passage_for((self.salt as usize).wrapping_add(self.calibration_attempt as usize))
    }

    pub fn accept_calibration(
        &mut self,
        typed: &str,
        elapsed_ms: u64,
        pasted: bool,
        _history: &[f64],
    ) -> Result<(), String> {
        if pasted {
            return Err("打字测试不能粘贴".to_owned());
        }
        if elapsed_ms < TEST_MS {
            return Err("打字测试需要满 60 秒".to_owned());
        }
        let (correct, wrong) = grade(&self.passage(), typed);
        self.calibration_attempt = self.calibration_attempt.saturating_add(1);
        let outcome = assess(correct, wrong, &[], self.calibration_attempt);
        if outcome.blocked {
            return Err("有效速度为 0，需要重新测试".to_owned());
        }
        if outcome.must_retest {
            self.baseline_cpm = None;
            self.pending_cpm = None;
            self.message = "准确率低于 90%，请再测一次".to_owned();
            self.persist_scoring_state();
            return Ok(());
        }
        self.calibration_samples.push(outcome.current_cpm);
        self.low_confidence |= outcome.low_confidence;
        if self.calibration_samples.len() < 2 {
            self.baseline_cpm = None;
            self.pending_cpm = None;
            self.message = if self.low_confidence {
                "第 1/2 轮测速已记录（低置信），请完成第二轮".to_owned()
            } else {
                "第 1/2 轮测速已记录，请完成第二轮".to_owned()
            };
            self.persist_scoring_state();
            return Ok(());
        }
        let baseline = median(&self.calibration_samples);
        self.baseline_cpm = Some(baseline);
        self.pending_cpm = Some(baseline);
        self.message = if self.low_confidence {
            "两轮打字测速已完成，本场中位数基线置信度低".to_owned()
        } else {
            "两轮打字测速已完成，已采用有效速度中位数".to_owned()
        };
        self.persist_scoring_state();
        Ok(())
    }

    pub fn assign_and_ready(
        &mut self,
        user_side: Option<Side>,
        human_slots: &[usize],
    ) -> Result<(), String> {
        if !self.preparation_ready() {
            return Err("请先完成拆题，再确认阵容".to_owned());
        }
        let mode = self.aggregate.as_ref().ok_or("还没有赛程")?.mode;
        let assignments = build_assignments(mode, user_side, human_slots)?;
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        aggregate
            .handle(MatchCommand::AssignControl {
                user_side,
                assignments,
            })
            .map_err(reject)?;
        if mode == MatchMode::Scoring {
            if self.baseline_cpm.is_none() {
                return Err("评分赛要先完成 60 秒打字测试".to_owned());
            }
            aggregate
                .handle(MatchCommand::RecordCalibration)
                .map_err(reject)?;
        }
        aggregate
            .handle(MatchCommand::ConfirmLineup)
            .map_err(reject)?;
        aggregate.handle(MatchCommand::MarkReady).map_err(reject)?;
        self.message = "可以开赛".to_owned();
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .handle(MatchCommand::Start)
            .map_err(reject)?;
        if self.view().current_human {
            self.event_started = Some(Instant::now());
            self.message = "比赛进行中".to_owned();
        } else {
            self.event_started = None;
            self.message = "点选本环节辩位后开始计时".to_owned();
        }
        Ok(())
    }

    pub fn claim_seat(&mut self, slot_index: usize, second_confirmed: bool) -> Result<(), String> {
        self.switch_current_controller_scoped(
            slot_index,
            true,
            second_confirmed,
            ControlScope::Match,
        )
    }

    #[cfg(test)]
    pub fn switch_current_controller(
        &mut self,
        slot_index: usize,
        to_human: bool,
        second_confirmed: bool,
    ) -> Result<(), String> {
        self.switch_current_controller_scoped(
            slot_index,
            to_human,
            second_confirmed,
            ControlScope::Turn,
        )
    }

    pub fn switch_current_controller_scoped(
        &mut self,
        slot_index: usize,
        to_human: bool,
        second_confirmed: bool,
        scope: ControlScope,
    ) -> Result<(), String> {
        // Validate on a candidate: a rejected scoring confirmation must not
        // change the selected free-debate seat or any exchange override.
        let mut next = self.aggregate.as_ref().ok_or("还没有赛程")?.clone();
        if next.phase != MatchPhase::InProgress {
            return Err("只能在正式发言窗口切换控制权".to_owned());
        }
        let event = next
            .schedule
            .get(next.cursor)
            .cloned()
            .ok_or("当前没有环节")?;
        let slot = SlotId::from_index(slot_index).ok_or("辩位无效")?;
        if next.mode == MatchMode::Scoring && next.user_side != Some(slot.side()) {
            return Err("计分模式不能点对方辩位".to_owned());
        }
        if event.kind == StageKind::FreeDebate {
            let expected_side = self
                .aggregate
                .as_ref()
                .and_then(MatchAggregate::current_free_debate_side)
                .unwrap_or(event.speaker.side());
            if slot.side() != expected_side {
                return Err("自由辩只能点当前持权方的辩位".to_owned());
            }
            next.schedule[next.cursor].speaker = slot;
        } else if event.kind == StageKind::CrossExam {
            let (questioner, respondent) = self
                .aggregate
                .as_ref()
                .ok_or("还没有赛程")?
                .current_cross_exam_roles()
                .map_err(reject)?;
            if slot != questioner && slot != respondent {
                return Err("质询只能接管当前实际提问者或回答者".to_owned());
            }
        } else if slot != event.speaker {
            return Err("这一环节只能点指定辩位".to_owned());
        }
        let was_human = self
            .view()
            .seats
            .iter()
            .find(|seat| seat.index == slot_index)
            .is_some_and(|seat| seat.controller == "human");
        let desired = if to_human {
            Controller::Human
        } else {
            Controller::Ai
        };
        let scope_changes = scope == ControlScope::Match
            && (next.assignments[slot_index].controller != desired
                || next.active_overrides().iter().any(|item| item.slot == slot));
        let no_change = !scope_changes
            && was_human == to_human
            && (event.kind == StageKind::CrossExam || event.speaker == slot);
        if no_change && (!to_human || !self.view().awaiting_seat) {
            return Ok(());
        }
        let roster = next.assignments.clone();
        if to_human && !was_human {
            next.handle(MatchCommand::RequestTakeover {
                slot_index,
                second_confirmed,
            })
            .map_err(claim_reject)?;
        }
        let controller = if to_human {
            crate::domain::events::Controller::Human
        } else {
            crate::domain::events::Controller::Ai
        };
        if scope == ControlScope::Turn {
            next.assignments = roster;
            next.set_turn_override(slot, controller);
        } else {
            next.assignments[slot_index].controller = controller;
            next.current_control_overrides
                .retain(|item| item.slot != slot);
        }
        if next.mode == MatchMode::Scoring && !no_change {
            next.non_standard = true;
        }
        for exchange in next.cross_exam_exchanges.iter_mut().filter(|exchange| {
            scope == ControlScope::Match
                && exchange.event_id == event.id
                && matches!(
                    exchange.status,
                    crate::domain::events::CrossExamExchangeStatus::Selected
                        | crate::domain::events::CrossExamExchangeStatus::QuestionSubmitted
                )
        }) {
            if exchange.questioner == slot {
                exchange.questioner_controller = Some(controller);
            }
            if exchange.respondent == slot {
                exchange.respondent_controller = Some(controller);
            }
        }
        for session in next
            .cross_exam_sessions
            .iter_mut()
            .filter(|session| session.event_id == event.id)
        {
            session.revision = session.revision.saturating_add(1);
        }
        for session in next
            .free_debate_sessions
            .iter_mut()
            .filter(|session| session.event_id == event.id)
        {
            session.revision = session.revision.saturating_add(1);
        }
        self.aggregate = Some(next);
        if let Some(aggregate) = self.aggregate.as_mut() {
            aggregate.control_revision = aggregate.control_revision.wrapping_add(1);
        }
        self.turn_revision = self.turn_revision.wrapping_add(1);
        self.event_started = if to_human { Some(Instant::now()) } else { None };
        self.message = if to_human {
            "已由人工控制，可输入发言"
        } else {
            "已交给 AI，可点击生成本轮"
        }
        .to_owned();
        Ok(())
    }

    pub fn waive_free_debate_time(&mut self) -> Result<(), String> {
        let side = self
            .aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .waive_current_free_debate_time()
            .map_err(reject)?;
        self.event_started = None;
        self.message = match side {
            Side::Affirmative => "正方已放弃自由辩剩余时间".to_owned(),
            Side::Negative => "反方已放弃自由辩剩余时间".to_owned(),
        };
        Ok(())
    }

    pub fn set_pending_controller(
        &mut self,
        index: usize,
        to_human: bool,
        second_confirmed: bool,
    ) -> Result<(), String> {
        let aggregate = self.aggregate.as_ref().ok_or("还没有赛程")?;
        let event = aggregate.schedule.get(index).ok_or("环节编号无效")?.clone();
        let (controller, mark_non_standard) = crate::domain::event_control::assign_controller(
            aggregate.mode,
            aggregate.user_side,
            event.speaker,
            to_human,
            index,
            aggregate.cursor,
            aggregate.phase,
            second_confirmed,
        )
        .map_err(|err| err.message().to_owned())?;
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        aggregate.assignments[event.speaker.index()].controller = controller;
        if mark_non_standard {
            aggregate.non_standard = true;
        }
        // Changing control after lineup confirmation revokes lineup hash.
        if aggregate.lineup_hash.is_some() {
            aggregate.lineup_hash = None;
        }
        self.message = if to_human {
            "已改为本人发言（仅未开始环节）".to_owned()
        } else {
            "已改为 AI 发言（仅未开始环节）".to_owned()
        };
        Ok(())
    }

    pub fn note_wait(&mut self, waited_ms: u64) -> Result<(), String> {
        let before = self
            .aggregate
            .as_ref()
            .map(|state| state.charged_ms)
            .unwrap_or(0);
        let remaining_before = self.remaining_ms();
        self.aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .handle(MatchCommand::NoteAiWait { waited_ms })
            .map_err(reject)?;
        let after = self
            .aggregate
            .as_ref()
            .map(|state| state.charged_ms)
            .unwrap_or(0);
        if before != after {
            return Err("等待模型不应消耗比赛时间".to_owned());
        }
        if let Some(started) = self.event_started.as_mut() {
            *started += std::time::Duration::from_millis(waited_ms);
        }
        let remaining_after = self.remaining_ms();
        if remaining_after + 50 < remaining_before {
            return Err("等待模型后本轮剩余时间被错误扣减".to_owned());
        }
        Ok(())
    }

    pub fn speech_display_ms(&self, text: &str) -> u64 {
        let units = count_effective_units(text) as u64;
        let rate = self
            .aggregate
            .as_ref()
            .map(|state| state.speech_rate_per_minute)
            .unwrap_or(crate::domain::timing::DEFAULT_SPEECH_RATE)
            .max(1) as u64;
        units.saturating_mul(60_000) / rate
    }

    pub fn submit(
        &mut self,
        text: String,
        input_ms: u64,
        pause_ms: u64,
        reverse_units: u32,
        reverse_actions: u32,
        pasted: bool,
    ) -> Result<(), String> {
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .cloned()
            .ok_or("当前没有可提交的环节")?;
        let event_id = event.id;
        let words = aggregate.metering_mode == MatchResourceMode::Words;
        let word_cap = aggregate.current_word_cap();
        if words && word_cap == 0 {
            return Err("本方字数额度已用尽".to_owned());
        }
        let human = aggregate.effective_controller(event.speaker) == Controller::Human;
        let semantic_free =
            event.kind == StageKind::FreeDebate && aggregate.semantic_ruleset.is_some();
        if semantic_free {
            let remaining_before = aggregate
                .current_free_debate_clock()
                .map(|(affirmative, negative, side)| match side {
                    Side::Affirmative => affirmative,
                    Side::Negative => negative,
                })
                .unwrap_or(u64::from(event.duration_seconds).saturating_mul(1_000));
            let effective_cap = if words {
                word_cap
            } else {
                event.word_cap.min(
                    (remaining_before
                        .saturating_mul(u64::from(aggregate.speech_rate_per_minute.max(1)))
                        / 60_000)
                        .max(1) as u32,
                )
            };
            let before = aggregate.transcripts.len();
            aggregate
                .submit_free_debate_turn(event.speaker, text, input_ms)
                .map_err(reject)?;
            if aggregate.transcripts.len() != before + 1 {
                return Err("自由辩发言未被保存".to_owned());
            }
            let entry = aggregate.transcripts.last().ok_or("自由辩发言未被保存")?;
            if human {
                self.cap_units = self.cap_units.saturating_add(effective_cap);
                if !pasted {
                    self.in_cap_units = self.in_cap_units.saturating_add(entry.effective_units);
                    self.forward_units = self.forward_units.saturating_add(entry.effective_units);
                }
                self.overflow_units = self
                    .overflow_units
                    .saturating_add(count_effective_units(&entry.overflow_text));
                self.reverse_units = self.reverse_units.saturating_add(reverse_units);
                self.reverse_actions = self.reverse_actions.saturating_add(reverse_actions);
                self.pause_ms = self.pause_ms.saturating_add(pause_ms);
                self.input_ms = self.input_ms.saturating_add(input_ms.max(1));
                if pasted {
                    self.pasted_ids.push(entry.id);
                }
            }
            if let Some(next_side) = aggregate.current_free_debate_side() {
                if let Some(current) = aggregate.schedule.get_mut(aggregate.cursor) {
                    current.speaker = match next_side {
                        Side::Affirmative => SlotId::Aff1,
                        Side::Negative => SlotId::Neg1,
                    };
                }
            }
            self.event_started = None;
            self.message = if aggregate
                .schedule
                .get(aggregate.cursor)
                .is_some_and(|next| next.kind == StageKind::FreeDebate)
            {
                "本轮已保存，发言权已交给另一方，请点选辩位或让 AI 发言。".to_owned()
            } else {
                "自由辩结束，已进入下一环节。".to_owned()
            };
            self.persist_scoring_state();
            return Ok(());
        }
        if event.kind == StageKind::FreeDebate && self.floor.holder == Some(event.speaker.side()) {
            return Err("同一方不能连续取得发言权".to_owned());
        }
        let before = aggregate.transcripts.len();
        aggregate
            .handle(MatchCommand::SubmitCurrent {
                event_id,
                text: text.clone(),
                input_ms,
            })
            .map_err(reject)?;
        if aggregate.transcripts.len() != before + 1 {
            return Err("重复提交没有被拒绝".to_owned());
        }
        let (prefix, tail) = split_effective(&text, word_cap);
        let prefix_units = count_effective_units(prefix);
        let tail_units = count_effective_units(tail);
        if human {
            self.cap_units = self.cap_units.saturating_add(word_cap);
            if !pasted {
                self.in_cap_units = self.in_cap_units.saturating_add(prefix_units);
                self.forward_units = self.forward_units.saturating_add(prefix_units);
            }
            self.overflow_units = self.overflow_units.saturating_add(tail_units);
            self.reverse_units = self.reverse_units.saturating_add(reverse_units);
            self.reverse_actions = self.reverse_actions.saturating_add(reverse_actions);
            self.pause_ms = self.pause_ms.saturating_add(pause_ms);
            self.input_ms = self.input_ms.saturating_add(input_ms.max(1));
        }
        if event.kind == StageKind::FreeDebate {
            let side = event.speaker.side();
            self.floor
                .take_turn(side, true)
                .map_err(|_| "同一方不能连续取得发言权".to_owned())?;
            if !words {
                self.floor.charge(side, input_ms.max(1));
            }
        }
        if pasted && human {
            if let Some(entry) = aggregate.transcripts.last() {
                self.pasted_ids.push(entry.id);
            }
        }
        self.event_started = if aggregate
            .schedule
            .get(aggregate.cursor)
            .is_some_and(|next| next.kind == StageKind::FreeDebate)
        {
            None
        } else {
            Some(Instant::now())
        };
        self.message = "本轮已保存".to_owned();
        self.persist_scoring_state();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_cross_exam_answer(
        &mut self,
        text: String,
        input_ms: u64,
        pause_ms: u64,
        reverse_units: u32,
        reverse_actions: u32,
        pasted: bool,
        interrupted_reason: Option<String>,
        _hidden_tail: String,
    ) -> Result<(), String> {
        let clock_side = self.cross_exam_clock_side(true)?;
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .cloned()
            .ok_or("当前没有可提交的质询")?;
        let words = aggregate.metering_mode == MatchResourceMode::Words;
        let word_cap = aggregate.current_word_cap();
        if event.kind != StageKind::CrossExam {
            return Err("当前不是质询环节".to_owned());
        }
        let (prefix, overflow) = split_effective(&text, word_cap);
        let prefix_units = count_effective_units(prefix);
        let overflow_units = count_effective_units(overflow);
        let transcript_id = aggregate
            .commit_cross_exam_answer(text, input_ms.max(1), interrupted_reason)
            .map_err(reject)?;
        let human = aggregate
            .transcripts
            .last()
            .is_some_and(|entry| entry.controller == Some(Controller::Human));
        if human {
            self.cap_units = self.cap_units.saturating_add(word_cap);
            if !pasted {
                self.in_cap_units = self.in_cap_units.saturating_add(prefix_units);
                self.forward_units = self.forward_units.saturating_add(prefix_units);
            }
            self.overflow_units = self.overflow_units.saturating_add(overflow_units);
            self.reverse_units = self.reverse_units.saturating_add(reverse_units);
            self.reverse_actions = self.reverse_actions.saturating_add(reverse_actions);
            self.pause_ms = self.pause_ms.saturating_add(pause_ms);
            self.input_ms = self.input_ms.saturating_add(input_ms.max(1));
            if pasted {
                self.pasted_ids.push(transcript_id);
            }
        }
        if let Some(side) = clock_side.filter(|_| !words) {
            self.floor.charge(side, input_ms.max(1));
        }
        self.event_started = Some(Instant::now());
        self.message = "质询回答已保存".to_owned();
        self.persist_scoring_state();
        Ok(())
    }

    pub fn charge_cross_exam_question_clock(&mut self, input_ms: u64) -> Result<(), String> {
        if self
            .aggregate
            .as_ref()
            .is_some_and(|state| state.metering_mode == MatchResourceMode::Words)
        {
            return Ok(());
        }
        if let Some(side) = self.cross_exam_clock_side(false)? {
            self.floor.charge(side, input_ms);
        }
        Ok(())
    }

    /// Replace the legacy net-text estimate with replay-validated input facts.
    pub fn note_unverified_input(&mut self) {
        self.low_confidence = true;
        if let Some(aggregate) = self.aggregate.as_mut() {
            aggregate.non_standard = true;
        }
        self.message =
            "文本已保存；部分输入来源无法确认，本场输入控制评分标记为低置信、非标准。".to_owned();
        self.persist_scoring_state();
    }

    pub fn apply_last_validated_input_counts(&mut self, forward: u32, positive: u32) {
        let Some(entry) = self
            .aggregate
            .as_ref()
            .and_then(|state| state.transcripts.last())
        else {
            return;
        };
        if entry.controller != Some(Controller::Human) {
            return;
        }
        let estimate = if self.pasted_ids.contains(&entry.id) {
            0
        } else {
            entry.effective_units
        };
        self.forward_units = self
            .forward_units
            .saturating_sub(estimate)
            .saturating_add(forward);
        self.in_cap_units = self
            .in_cap_units
            .saturating_sub(estimate)
            .saturating_add(positive.min(entry.effective_units));
        self.persist_scoring_state();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_cross_exam_question_metrics(
        &mut self,
        exchange_id: Uuid,
        input_ms: u64,
        pause_ms: u64,
        reverse_units: u32,
        reverse_actions: u32,
        pasted: bool,
    ) -> Result<(), String> {
        let aggregate = self.aggregate.as_ref().ok_or("还没有赛程")?;
        let transcript = aggregate
            .transcripts
            .iter()
            .rev()
            .find(|entry| {
                entry.cross_exam_exchange_id == Some(exchange_id)
                    && entry.cross_exam_turn_kind
                        == Some(crate::domain::events::CrossExamTurnKind::Question)
            })
            .ok_or("找不到质询问题记录")?;
        let event = aggregate
            .schedule
            .iter()
            .find(|event| event.id == transcript.event_id)
            .ok_or("找不到质询环节")?;
        let word_cap = transcript.word_cap_at_submission.unwrap_or_else(|| {
            if aggregate.metering_mode == MatchResourceMode::Words {
                event.word_cap.min(
                    aggregate
                        .remaining_main_units(transcript.slot.side())
                        .saturating_add(transcript.effective_units),
                )
            } else {
                event.word_cap
            }
        });
        if transcript.controller != Some(Controller::Human) {
            return Ok(());
        }
        self.cap_units = self.cap_units.saturating_add(word_cap);
        if !pasted {
            self.in_cap_units = self.in_cap_units.saturating_add(transcript.effective_units);
            self.forward_units = self
                .forward_units
                .saturating_add(transcript.effective_units);
        }
        self.reverse_units = self.reverse_units.saturating_add(reverse_units);
        self.reverse_actions = self.reverse_actions.saturating_add(reverse_actions);
        self.pause_ms = self.pause_ms.saturating_add(pause_ms);
        self.input_ms = self.input_ms.saturating_add(input_ms.max(1));
        if pasted {
            self.pasted_ids.push(transcript.id);
        }
        self.persist_scoring_state();
        Ok(())
    }

    fn cross_exam_clock_side(&self, answer: bool) -> Result<Option<Side>, String> {
        let aggregate = self.aggregate.as_ref().ok_or("还没有赛程")?;
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .ok_or("当前没有质询环节")?;
        let stage = aggregate
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .ok_or("找不到质询赛制定义")?;
        let (questioner, respondent) = aggregate
            .current_cross_exam_roles()
            .map_err(|_| "质询缺少合法问答辩位")?;
        Ok(match stage.clock_owner {
            ClockOwner::Questioner => Some(questioner.side()),
            ClockOwner::Respondent => Some(respondent.side()),
            ClockOwner::Shared => Some(if answer {
                respondent.side()
            } else {
                questioner.side()
            }),
            ClockOwner::Individual | ClockOwner::Team => None,
        })
    }

    #[cfg(test)]
    pub fn advance_ai(&mut self) -> Result<(), String> {
        if self.view().current_human {
            return Err("这一轮由本人提交".to_owned());
        }
        let context = AiContext {
            topic: self.view().topic,
            own_strategy: self.difficulty.constraint().to_owned(),
            ..AiContext::default()
        };
        let cancel = AtomicBool::new(false);
        let turn = if self.live {
            let gateway = self.live_gateway()?;
            gateway
                .generate_turn(&context, &cancel)
                .map_err(|err| err.to_string())?
        } else {
            let gateway = MockGateway::mock(self.model_config.clone());
            gateway
                .generate_turn(&context, &cancel)
                .map_err(|err| err.to_string())?
        };
        let input_ms = self.speech_display_ms(&turn.text).max(1);
        self.submit(turn.text, input_ms, 0, 0, 0, false)
    }

    pub fn interrupt_shown(&mut self, full: String, displayed_units: u32) -> Result<(), String> {
        self.interrupt_shown_with_input(full, displayed_units, 1)
    }

    pub fn interrupt_shown_with_input(
        &mut self,
        full: String,
        displayed_units: u32,
        input_ms: u64,
    ) -> Result<(), String> {
        let event = self
            .aggregate
            .as_ref()
            .and_then(|state| state.schedule.get(state.cursor).cloned())
            .ok_or("当前没有可打断的环节")?;
        let allowed = event.kind == StageKind::CrossExam && event.interruptible;
        let closed =
            self.aggregate.as_ref().map(|state| state.phase) != Some(MatchPhase::InProgress);
        let already = self.interrupted_events.contains(&event.id);
        if !allowed {
            let transcripts = self
                .aggregate
                .as_ref()
                .map(|state| state.transcripts.len())
                .unwrap_or(0);
            let clocks = self.floor.clone();
            let error = self
                .aggregate
                .as_mut()
                .ok_or("还没有赛程")?
                .handle(MatchCommand::Interrupt);
            if self.aggregate.as_ref().map(|state| state.transcripts.len()) != Some(transcripts)
                || self.floor != clocks
            {
                return Err("非法打断改变了比赛".to_owned());
            }
            return Err(error.map_or_else(|reject| reject.to_string(), |_| "打断被拒绝".to_owned()));
        }
        let (prefix, tail) = interrupt_at(&full, displayed_units, true, already, closed)
            .map_err(interrupt_message)?;
        self.submit(prefix, input_ms.max(1), 0, 0, 0, false)?;
        let id = self
            .aggregate
            .as_ref()
            .and_then(|state| state.transcripts.last().map(|entry| entry.id))
            .ok_or("打断没有写入记录")?;
        self.interrupted_tails.push((id, tail));
        self.interrupted_events.push(event.id);
        self.message = "已按有效前缀打断".to_owned();
        Ok(())
    }

    pub fn finish(&mut self, semantic_ok: bool) -> Result<(), String> {
        let marks = self.human_marks();
        let (live_review, live_error) = if semantic_ok && self.live {
            match self.live_semantic(&marks) {
                Ok(review) => (Some(review), None),
                Err(error) => (None, Some(error)),
            }
        } else {
            (None, None)
        };
        let semantic_ok = if self.live {
            live_review.is_some()
        } else {
            semantic_ok
        };
        let aggregate = self.aggregate.as_mut().ok_or("还没有赛程")?;
        if aggregate.cursor != aggregate.schedule.len() {
            return Err("赛程还没有完成".to_owned());
        }
        aggregate
            .handle(MatchCommand::CompleteMatch)
            .map_err(reject)?;
        aggregate
            .handle(MatchCommand::BeginScoring)
            .map_err(reject)?;
        let program = score_program(ProgramInput {
            in_cap_units: self.in_cap_units,
            cap_units: self.cap_units.max(1),
            overflow_units: self.overflow_units,
            reverse_units: self.reverse_units,
            reverse_actions: self.reverse_actions,
            forward_typed_units: self.forward_units,
            pause_ms: self.pause_ms,
            input_ms: self.input_ms.max(1),
            baseline_cpm: self.baseline_cpm.unwrap_or(180.0),
            started_on_time: true,
            stayed_legal: !aggregate.non_standard,
            role_complete: true,
            reviewable: !aggregate.transcripts.is_empty(),
        });
        let (review_marks, adjustments, team, stage_evaluations, review_summary, team_summary) =
            if let Some(review) = live_review {
                (
                    review.marks,
                    review.adjustments,
                    review.team,
                    review.stage_evaluations,
                    review.review_summary,
                    review.team_summary,
                )
            } else if semantic_ok {
                (
                    marks.clone(),
                    cited_adjustment(&marks),
                    "tie".to_owned(),
                    Vec::new(),
                    String::new(),
                    String::new(),
                )
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    "tie".to_owned(),
                    Vec::new(),
                    String::new(),
                    String::new(),
                )
            };
        let valid_ids: Vec<String> = review_marks
            .iter()
            .map(|mark| mark.transcript_id.clone())
            .collect();
        let scored = combine(
            &review_marks,
            &adjustments,
            &valid_ids,
            program.total,
            &team,
            semantic_ok,
        )
        .map_err(|_| "复核被拒绝".to_owned())?;
        let standard = !aggregate.non_standard;
        self.report = Some(PublicReport {
            formula_version: REPORT_FORMULA.to_owned(),
            model_id: DEFAULT_MODEL_ID.to_owned(),
            difficulty: difficulty_label(self.difficulty).to_owned(),
            speech_rate: aggregate.speech_rate_per_minute,
            personal: scored.personal,
            program: scored.program,
            semantic: scored.semantic,
            team_winner: scored.team_winner,
            standard,
            non_standard_reason: if standard {
                String::new()
            } else {
                "非标准流程".to_owned()
            },
            length: program.length,
            input_control: program.input_control,
            execution: program.execution,
            trace: format!(
                "length={} control={} execution={} factor={}",
                program.length, program.input_control, program.execution, program.speed_factor
            ),
            scoring_status: scored.status.clone(),
            low_confidence: self.low_confidence,
            dimensions: scored
                .dimensions
                .iter()
                .map(|(name, raw, points, transcript_id)| ReportDimension {
                    name: name.clone(),
                    raw: *raw,
                    points: *points,
                    transcript_id: transcript_id.clone(),
                })
                .collect(),
            stage_evaluations,
            review_summary,
            team_summary,
        });
        aggregate
            .handle(MatchCommand::PublishReport)
            .map_err(reject)?;
        self.message = if semantic_ok {
            "报告已生成".to_owned()
        } else if let Some(error) = live_error {
            format!("SCORING_PENDING: {error}")
        } else {
            "SCORING_PENDING".to_owned()
        };
        Ok(())
    }

    pub fn export(&self) -> String {
        self.report
            .as_ref()
            .map(export_public)
            .unwrap_or_else(|| "{}".to_owned())
    }

    pub fn mark_recovery(&mut self) -> Result<(), String> {
        self.aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .handle(MatchCommand::MarkRecoveryRequired)
            .map_err(reject)?;
        self.message = "需要恢复，本场记为非标准".to_owned();
        Ok(())
    }

    fn live_gateway(&self) -> Result<Gateway<DeepSeekTransport<UreqHttp>, ThreadSleep>, String> {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .map_err(|_| "还没有可用的 DeepSeek 密钥".to_owned())?;
        Ok(Gateway::new(
            DeepSeekTransport::new(UreqHttp, self.model_config.clone(), key),
            ThreadSleep,
            self.model_config.clone(),
        ))
    }

    #[cfg(test)]
    fn strategies_from<T, S>(
        &self,
        gateway: &Gateway<T, S>,
        context: AiContext,
    ) -> Result<(String, String, String, Vec<RolePacket>), String>
    where
        T: crate::infrastructure::ai::ModelTransport,
        S: crate::infrastructure::ai::Wait,
    {
        let (summary, affirmative, negative, packets) = run_topic_pipeline(
            gateway,
            context,
            self.difficulty.constraint(),
            self.aggregate
                .as_ref()
                .and_then(|state| state.ruleset.as_ref()),
        )?;
        Ok((summary, affirmative, negative, packets))
    }

    fn live_semantic(&self, marks: &[StageMark]) -> Result<LiveSemanticResult, String> {
        let gateway = self.live_gateway()?;
        let cancel = AtomicBool::new(false);
        let state = self.aggregate.as_ref().ok_or("还没有赛程")?;
        let dimension_names = DIMENSIONS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join("、");
        let mut updated = marks.to_vec();
        let mut stage_results = Vec::new();
        let mut stage_evaluations = Vec::new();
        for mark in marks {
            let transcript_id =
                Uuid::parse_str(&mark.transcript_id).map_err(|_| "评分记录编号无效".to_owned())?;
            let excluded = if self.pasted_ids.contains(&transcript_id) {
                "粘贴正文不可评分；只返回零分并说明原因"
            } else {
                "排除 overflow_text 与 interrupted_tail，仅评价 effective_text"
            };
            let context = crate::application::scoring_service::stage_context(
                state,
                transcript_id,
                &dimension_names,
                excluded,
            )?;
            let score = gateway
                .score_stage(&context, &cancel)
                .map_err(|err| err.to_string())?;
            if !score.transcript_ids.contains(&transcript_id) {
                return Err("逐环节评分没有引用当前真人发言".to_owned());
            }
            let target = updated
                .iter_mut()
                .find(|item| item.transcript_id == mark.transcript_id)
                .ok_or("逐环节评分无法绑定真人发言")?;
            target.raw = validated_stage_dimensions(&score.dimensions)?;
            stage_results.push(serde_json::to_string(&score).map_err(|_| "逐环节评分无法序列化")?);
            stage_evaluations.push(ReportStageEvaluation {
                transcript_id: transcript_id.to_string(),
                strength: score.strength,
                improvement: score.improvement,
                material_support: score.material_support,
                confidence: score.confidence,
            });
        }
        let context = crate::application::scoring_service::match_review_context(
            state,
            &stage_results,
            &dimension_names,
            &marks
                .iter()
                .map(|mark| mark.transcript_id.clone())
                .collect::<Vec<_>>(),
        );
        let review = gateway
            .review_match(&context, &cancel)
            .map_err(|err| err.to_string())?;
        let judgement = gateway
            .judge_match(&context, &cancel)
            .map_err(|err| err.to_string())?;
        let mut adjustments = Vec::new();
        for adjustment in &review.adjustments {
            let Some(index) = dimension_index(&adjustment.dimension) else {
                continue;
            };
            let Ok(transcript_id) = bind_transcript(&adjustment.transcript_id, marks) else {
                continue;
            };
            adjustments.push(CitedAdjustment {
                dimension: index,
                delta: adjustment.delta,
                transcript_id: Some(transcript_id),
            });
        }
        let winner = match judgement.winner {
            JudgementWinner::Affirmative => "affirmative",
            JudgementWinner::Negative => "negative",
            JudgementWinner::Tie => "tie",
        };
        Ok(LiveSemanticResult {
            marks: updated,
            adjustments,
            team: winner.to_owned(),
            stage_evaluations,
            review_summary: review.summary,
            team_summary: judgement.summary,
        })
    }

    fn human_marks(&self) -> Vec<StageMark> {
        let Some(state) = self.aggregate.as_ref() else {
            return Vec::new();
        };
        state
            .transcripts
            .iter()
            .filter_map(|entry| {
                let human = entry
                    .controller
                    .or_else(|| {
                        entry
                            .cross_exam_exchange_id
                            .and_then(|exchange_id| {
                                state
                                    .cross_exam_exchanges
                                    .iter()
                                    .find(|exchange| exchange.id == exchange_id)
                            })
                            .and_then(|exchange| match entry.cross_exam_turn_kind {
                                Some(crate::domain::events::CrossExamTurnKind::Question) => {
                                    exchange.questioner_controller
                                }
                                Some(crate::domain::events::CrossExamTurnKind::Answer) => {
                                    exchange.respondent_controller
                                }
                                None => None,
                            })
                    })
                    .unwrap_or(state.assignments[entry.slot.index()].controller)
                    == Controller::Human;
                if !human {
                    return None;
                }
                Some(StageMark {
                    transcript_id: entry.id.to_string(),
                    raw: [0.0; 7],
                })
            })
            .collect()
    }

    fn remaining_ms(&self) -> u64 {
        let Some(aggregate) = self.aggregate.as_ref() else {
            return 0;
        };
        let Some(event) = aggregate.schedule.get(aggregate.cursor) else {
            return 0;
        };
        if aggregate.metering_mode == MatchResourceMode::Words {
            return 0;
        }
        let budget = aggregate
            .current_free_debate_clock()
            .map(|(affirmative, negative, side)| match side {
                Side::Affirmative => affirmative,
                Side::Negative => negative,
            })
            .unwrap_or_else(|| {
                if aggregate.semantic_ruleset.is_some() {
                    aggregate.main_clock_remaining(event.speaker.side())
                } else {
                    u64::from(event.duration_seconds).saturating_mul(1_000)
                }
            });
        let elapsed = self
            .event_started
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);
        budget.saturating_sub(elapsed)
    }

    fn next_id(&mut self) -> Uuid {
        self.salt += 1;
        Uuid::from_u128(self.salt)
    }

    pub(crate) fn allocate_id(&mut self) -> Uuid {
        self.next_id()
    }

    fn preparation_ready(&self) -> bool {
        self.strategies.as_ref().is_some_and(|strategies| {
            !strategies.affirmative.trim().is_empty() && !strategies.negative.trim().is_empty()
        }) && self
            .role_packets
            .as_deref()
            .is_some_and(complete_role_packets)
    }

    fn persist_preparation(&mut self) {
        if let Some(aggregate) = self.aggregate.as_mut() {
            aggregate.preparation = Some(crate::domain::match_engine::PreparationStateV1 {
                difficulty: self.difficulty,
                strategies: self.strategies.clone(),
                role_packets: self.role_packets.clone(),
            });
        }
    }

    pub fn update_preparation(&mut self, setup: RuleSetup) -> Result<(), String> {
        let Some(aggregate) = self.aggregate.as_ref() else {
            return Ok(());
        };
        if !is_preparation_phase(aggregate.phase) {
            return Err("开赛后不能修改赛前设置".to_owned());
        }
        let topic = setup.topic.trim().to_owned();
        let changed = aggregate.mode != setup.mode
            || aggregate.topic != topic
            || aggregate.materials != setup.materials
            || aggregate.user_side != setup.user_side
            || self.difficulty != setup.difficulty;
        if !changed {
            return Ok(());
        }
        let mode_changed = aggregate.mode != setup.mode;
        let aggregate = self.aggregate.as_mut().expect("aggregate checked");
        aggregate.invalidate_preparation().map_err(reject)?;
        aggregate.mode = setup.mode;
        aggregate.topic = topic;
        aggregate.materials = setup.materials;
        aggregate.user_side = setup.user_side;
        self.difficulty = setup.difficulty;
        self.strategies = None;
        self.role_packets = None;
        self.report = None;
        self.preparation_revision = self.preparation_revision.wrapping_add(1);
        if mode_changed {
            self.baseline_cpm = None;
            self.calibration_samples.clear();
            self.low_confidence = false;
            self.calibration_attempt = 0;
            self.pending_cpm = None;
            self.persist_scoring_state();
        }
        self.persist_preparation();
        self.message =
            "赛前设置已保存，旧拆题与阵容确认已撤销；原赛制已保留，请重新拆题。".to_owned();
        Ok(())
    }

    pub(crate) fn begin_preparation_request(&mut self) -> Result<PreparationBoundary, String> {
        if self.profile_id.is_none() {
            return Err("先建立档案".to_owned());
        }
        if self
            .aggregate
            .as_ref()
            .is_some_and(|state| !is_preparation_phase(state.phase))
        {
            return Err("开赛后不能重新识别赛制或拆题".to_owned());
        }
        self.preparation_revision = self.preparation_revision.wrapping_add(1);
        Ok(PreparationBoundary {
            revision: self.preparation_revision,
            profile_id: self.profile_id,
            snapshot: self.aggregate.as_ref().map(MatchAggregate::to_snapshot),
            difficulty: self.difficulty,
            live: self.live,
            model_config: self.model_config.clone(),
        })
    }

    pub(crate) fn validate_preparation_request(
        &self,
        boundary: &PreparationBoundary,
    ) -> Result<(), String> {
        let current = self.aggregate.as_ref().map(MatchAggregate::to_snapshot);
        if self.preparation_revision != boundary.revision
            || self.profile_id != boundary.profile_id
            || current != boundary.snapshot
            || self.difficulty != boundary.difficulty
            || self.live != boundary.live
            || self.model_config != boundary.model_config
        {
            return Err("该结果已因档案、赛制或赛前设置变化失效，请重试当前操作".to_owned());
        }
        Ok(())
    }

    pub fn adopt_topic_for_analysis(&mut self, setup: RuleSetup) -> Result<(), String> {
        let topic = setup.topic.trim().to_owned();
        if topic.is_empty() {
            return Err("先填写辩题".to_owned());
        }
        match self.aggregate.as_ref().map(|state| state.phase) {
            None => {
                self.open_template(
                    setup.mode,
                    topic,
                    setup.materials,
                    setup.user_side,
                    &[],
                    setup.difficulty,
                )?;
                self.confirm_rules()
            }
            Some(MatchPhase::RulesParsed) => {
                let default_rules = self
                    .aggregate
                    .as_ref()
                    .and_then(|state| state.ruleset.as_ref())
                    .is_some_and(|ruleset| ruleset.ruleset_name == "四对四通用");
                if !default_rules {
                    return Err("先确认规则，再拆题".to_owned());
                }
                self.update_preparation(setup)?;
                self.confirm_rules()
            }
            Some(phase) if is_preparation_phase(phase) => {
                if self
                    .aggregate
                    .as_ref()
                    .and_then(|state| state.rules_hash.as_ref())
                    .is_none()
                {
                    return Err("先确认规则，再拆题".to_owned());
                }
                self.update_preparation(setup)?;
                if self.aggregate.as_ref().map(|state| state.phase)
                    != Some(MatchPhase::RulesConfirmed)
                {
                    self.aggregate
                        .as_mut()
                        .expect("aggregate checked")
                        .invalidate_preparation()
                        .map_err(reject)?;
                    self.strategies = None;
                    self.role_packets = None;
                    self.persist_preparation();
                }
                Ok(())
            }
            _ => Err("先确认规则，再拆题".to_owned()),
        }
    }
}

#[derive(Clone)]
pub struct RuleSetup {
    pub mode: MatchMode,
    pub topic: String,
    pub materials: String,
    pub user_side: Option<Side>,
    pub difficulty: Difficulty,
}

fn is_preparation_phase(phase: MatchPhase) -> bool {
    matches!(
        phase,
        MatchPhase::DraftSetup
            | MatchPhase::RulesParsed
            | MatchPhase::RulesConfirmed
            | MatchPhase::TopicAnalyzed
            | MatchPhase::ControlAssigned
            | MatchPhase::Calibrated
            | MatchPhase::LineupConfirmed
            | MatchPhase::Ready
    )
}

fn complete_role_packets(packets: &[RolePacket]) -> bool {
    packets.len() == SlotId::ALL.len()
        && SlotId::ALL
            .iter()
            .all(|slot| packets.iter().filter(|packet| packet.slot == *slot).count() == 1)
}

pub fn parse_rules_with(
    live: bool,
    mut config: ModelConfig,
    source: &str,
    topic: &str,
    materials: &str,
) -> Result<(RulesetV1, u32), String> {
    let source = source.trim();
    if source.chars().count() > 30_000 {
        return Err("赛制说明不能超过 30000 字".to_owned());
    }
    let local = if source.is_empty() {
        None
    } else {
        crate::domain::rule_times::ruleset_from_text(source)
    };
    if local.as_ref().is_some_and(|ruleset| {
        ruleset.stages.len() == 13
            && source.contains("双方各18分钟")
            && source.contains("自由辩")
            && source.contains("各4分钟")
    }) {
        return Ok((local.expect("checked local rules"), 100));
    }
    if live {
        config.timeout_ms = 120_000;
        let key = KeyringSecretStore::app()
            .read_api_key()
            .map_err(|_| "还没有可用的 DeepSeek 密钥。请先到设置页保存并测试连接。".to_owned())?;
        let mut context = AiContext {
            topic: topic.to_owned(),
            rules_text: source.to_owned(),
            materials: materials.to_owned(),
            ..AiContext::default()
        };
        if let Some(candidate) = &local {
            let summary = candidate
                .stages
                .iter()
                .map(|stage| {
                    format!(
                        "{}:{:?}:{:?}",
                        stage.name, stage.speakers, stage.speaker_durations
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let block = format!("local_candidate_stages={summary}");
            context.materials = if context.materials.is_empty() {
                block
            } else {
                format!("{block}\n{}", context.materials)
            };
        }
        let cancel = AtomicBool::new(false);
        match Gateway::new(
            DeepSeekTransport::new(UreqHttp, config.clone(), key),
            ThreadSleep,
            config,
        )
        .parse_rules(&context, &cancel)
        {
            Ok(parsed) => {
                let mut ruleset = parsed.ruleset;
                if let Some(ref local_rules) = local {
                    overlay_local_seconds(&mut ruleset, local_rules);
                }
                if crate::domain::rules_check::check_ruleset(&ruleset).is_err() {
                    if let Some(local_rules) = local {
                        return Ok((local_rules, parsed.confidence.clamp(1, 70)));
                    }
                    return Err(
                        "模型返回的赛制无法执行。请修改赛制说明，或主动点「载入四对四模板」。"
                            .to_owned(),
                    );
                }
                return Ok((ruleset, parsed.confidence.max(1)));
            }
            Err(err) => {
                if let Some(ruleset) = local {
                    return Ok((ruleset, 80));
                }
                return Err(format!(
                    "模型未能解析赛制：{err}。请修改赛制说明，或主动点「载入四对四模板」。"
                ));
            }
        }
    }
    if let Some(ruleset) = local {
        return Ok((ruleset, 100));
    }
    if source.is_empty() {
        return Err("赛制说明为空。请粘贴赛制，或主动点「载入四对四模板」。".to_owned());
    }
    Err("原文里没有读出每人的发言秒数。请改写赛制说明，或主动点「载入四对四模板」。".to_owned())
}

fn overlay_local_seconds(target: &mut RulesetV1, local: &RulesetV1) {
    for local_stage in &local.stages {
        if let Some(stage) = target
            .stages
            .iter_mut()
            .find(|stage| stage.speakers == local_stage.speakers && stage.kind == local_stage.kind)
        {
            if !local_stage.speaker_durations.is_empty() {
                stage.speaker_durations = local_stage.speaker_durations.clone();
                stage.duration_seconds = local_stage.duration_seconds;
            }
        }
    }
}

fn preview_schedule(state: &MatchAggregate) -> Vec<crate::domain::ruleset::ScheduledEvent> {
    if !state.schedule.is_empty() {
        return state.schedule.clone();
    }
    let Some(ruleset) = &state.ruleset else {
        return Vec::new();
    };
    let mut salt = 1_u128;
    expand_schedule(ruleset, state.speech_rate_per_minute, || {
        salt += 1;
        Uuid::from_u128(salt)
    })
    .unwrap_or_default()
}

pub fn run_topic_analysis(
    live: bool,
    config: ModelConfig,
    context: AiContext,
    constraint: String,
    ruleset: Option<RulesetV1>,
) -> Result<(String, String, String, Vec<RolePacket>), String> {
    if live {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .map_err(|_| "还没有可用的 DeepSeek 密钥".to_owned())?;
        let gateway = Gateway::new(
            DeepSeekTransport::new(UreqHttp, config.clone(), key),
            ThreadSleep,
            config,
        );
        run_topic_pipeline(&gateway, context, &constraint, ruleset.as_ref())
    } else {
        let gateway = MockGateway::mock(config);
        run_topic_pipeline(&gateway, context, &constraint, ruleset.as_ref())
    }
}

fn run_topic_pipeline<T, S>(
    gateway: &Gateway<T, S>,
    mut context: AiContext,
    constraint: &str,
    ruleset: Option<&RulesetV1>,
) -> Result<(String, String, String, Vec<RolePacket>), String>
where
    T: crate::infrastructure::ai::ModelTransport,
    S: crate::infrastructure::ai::Wait,
{
    let cancel = AtomicBool::new(false);
    let analysis = gateway
        .analyze_topic(&context, &cancel)
        .map_err(|err| format!("拆题失败：{err}"))?;
    context.own_strategy = format!(
        "side=affirmative constraint={constraint}
neutral_summary={}",
        analysis.summary
    );
    let affirmative = gateway
        .build_strategy(&context, &cancel)
        .map_err(|err| format!("正方策略失败：{err}"))?;
    context.own_strategy = format!(
        "side=negative constraint={constraint}
neutral_summary={}",
        analysis.summary
    );
    context.opponent_strategy.clear();
    let negative = gateway
        .build_strategy(&context, &cancel)
        .map_err(|err| format!("反方策略失败：{err}"))?;
    let summary = format_topic_summary(&analysis);
    let affirmative_text = format_strategy("正方", &affirmative);
    let negative_text = format_strategy("反方", &negative);
    let packets = crate::domain::role_assign::assign_role_packets(
        ruleset.unwrap_or(&four_on_four_template()),
        &crate::domain::role_assign::TopicBrief {
            summary: analysis.summary.clone(),
            affirmative_burden: analysis.affirmative_burden.clone(),
            negative_burden: analysis.negative_burden.clone(),
            contentions: analysis.contentions.clone(),
        },
        &crate::domain::role_assign::SideBrief {
            thesis: affirmative.thesis.clone(),
            arguments: affirmative.arguments.clone(),
            risks: affirmative.risks.clone(),
        },
        &crate::domain::role_assign::SideBrief {
            thesis: negative.thesis.clone(),
            arguments: negative.arguments.clone(),
            risks: negative.risks.clone(),
        },
    );
    Ok((summary, affirmative_text, negative_text, packets))
}

fn format_topic_summary(analysis: &crate::infrastructure::ai::schema::TopicAnalysisV1) -> String {
    format!(
        "题型={}；概念={}；正方负担={}；反方负担={}；争点={}；结论={}",
        analysis.topic_type,
        analysis.concepts.join("、"),
        analysis.affirmative_burden,
        analysis.negative_burden,
        analysis.contentions.join("、"),
        analysis.summary
    )
}

fn format_strategy(
    side: &str,
    strategy: &crate::infrastructure::ai::schema::TeamStrategyV1,
) -> String {
    format!(
        "{side}主张：{}；论证：{}；风险：{}",
        strategy.thesis,
        strategy.arguments.join("、"),
        strategy.risks.join("、")
    )
}

fn normalize_stage_times(ruleset: &mut RulesetV1) {
    let minute_scale = ruleset.source_text.contains('分')
        && ruleset.stages.iter().all(|stage| {
            stage.duration_seconds > 0
                && stage.duration_seconds <= 20
                && stage
                    .speaker_durations
                    .iter()
                    .all(|seconds| *seconds > 0 && *seconds <= 20)
        });
    if !minute_scale {
        return;
    }
    for stage in &mut ruleset.stages {
        stage.duration_seconds = stage.duration_seconds.saturating_mul(60);
        for seconds in &mut stage.speaker_durations {
            *seconds = seconds.saturating_mul(60);
        }
    }
}

fn edit_schedule_row(
    ruleset: &mut RulesetV1,
    index: usize,
    slot: SlotId,
    duration_seconds: u32,
) -> Result<bool, String> {
    let mut cursor = 0_usize;
    for stage in &mut ruleset.stages {
        let repeat = stage.repeat.max(1);
        for _ in 0..repeat {
            for offset in 0..stage.speakers.len() {
                if cursor == index {
                    stage.speakers[offset] = slot;
                    if stage.kind == StageKind::CrossExam && offset == 0 {
                        stage.questioner = Some(slot);
                    }
                    if stage.speaker_durations.len() != stage.speakers.len() {
                        stage.speaker_durations =
                            vec![stage.duration_seconds; stage.speakers.len()];
                    }
                    stage.speaker_durations[offset] = duration_seconds;
                    stage.duration_seconds = stage.speaker_durations[0];
                    return Ok(false);
                }
                cursor += 1;
            }
        }
    }
    Err("没有这一行赛程".to_owned())
}

fn slot_from_label(label: &str) -> Result<SlotId, String> {
    match label {
        "AFF_1" => Ok(SlotId::Aff1),
        "AFF_2" => Ok(SlotId::Aff2),
        "AFF_3" => Ok(SlotId::Aff3),
        "AFF_4" => Ok(SlotId::Aff4),
        "NEG_1" => Ok(SlotId::Neg1),
        "NEG_2" => Ok(SlotId::Neg2),
        "NEG_3" => Ok(SlotId::Neg3),
        "NEG_4" => Ok(SlotId::Neg4),
        _ => Err("辩位无法对应".to_owned()),
    }
}

fn rules_problem_text(problem: RulesProblem) -> String {
    match problem {
        RulesProblem::NegativeDuration | RulesProblem::NonPositiveDuration => {
            "环节时长必须大于 0".to_owned()
        }
        RulesProblem::FreeDebateInterruptible => "自由辩不能打断".to_owned(),
        RulesProblem::InterruptOutsideCrossExam => "只有质询可以打断".to_owned(),
        RulesProblem::CrossExamMissingRoles => "质询必须写明提问方、回答方和计时归属".to_owned(),
        RulesProblem::RepeatNotFinite => "环节重复次数必须在 1 到 32 之间".to_owned(),
        RulesProblem::EmptySchedule => "赛制没有可执行的环节".to_owned(),
        RulesProblem::UnknownSlot => "赛制里有无法对应的辩位".to_owned(),
    }
}

fn fresh_salt() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = u128::from(SEQUENCE.fetch_add(1, Ordering::Relaxed));
    nanos.saturating_add(sequence << 96) | 0x1_0000
}

impl Default for Desk {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_assignments(
    mode: MatchMode,
    user_side: Option<Side>,
    human_slots: &[usize],
) -> Result<Vec<DebaterAssignmentV1>, String> {
    if mode == MatchMode::Scoring {
        let side = user_side.ok_or("计分模式需要选择阵营")?;
        if human_slots.is_empty() || human_slots.len() > 4 {
            return Err("计分模式由本人控制 1 到 4 个本方辩位".to_owned());
        }
        for index in human_slots {
            let slot = SlotId::from_index(*index).ok_or("辩位无效")?;
            if slot.side() != side {
                return Err("不能接管对方辩位".to_owned());
            }
        }
    }
    let mut rows = crate::domain::match_engine::training_assignments();
    for row in &mut rows {
        row.controller = Controller::Ai;
    }
    for index in human_slots {
        let slot = SlotId::from_index(*index).ok_or("辩位无效")?;
        rows[slot.index()].controller = Controller::Human;
    }
    Ok(rows)
}

fn bind_transcript(id: &Uuid, marks: &[StageMark]) -> Result<String, ()> {
    let text = id.to_string();
    if marks.iter().any(|mark| mark.transcript_id == text) {
        Ok(text)
    } else {
        Err(())
    }
}

fn dimension_index(name: &str) -> Option<usize> {
    DIMENSIONS.iter().position(|(label, _)| *label == name)
}

fn validated_stage_dimensions(
    dimensions: &[crate::infrastructure::ai::schema::ScoreDimensionV1],
) -> Result<[f64; 7], String> {
    let mut scores = [0.0; 7];
    let mut seen = [false; 7];
    for dimension in dimensions {
        let Some(index) = dimension_index(&dimension.name) else {
            return Err(format!("逐环节评分包含未知维度：{}", dimension.name));
        };
        if seen[index] {
            return Err(format!("逐环节评分重复维度：{}", dimension.name));
        }
        seen[index] = true;
        scores[index] = f64::from(dimension.score);
    }
    if seen.iter().any(|value| !value) {
        return Err("逐环节评分没有返回完整七维".to_owned());
    }
    Ok(scores)
}

fn cited_adjustment(marks: &[StageMark]) -> Vec<CitedAdjustment> {
    marks
        .first()
        .map(|mark| CitedAdjustment {
            dimension: 0,
            delta: 0,
            transcript_id: Some(mark.transcript_id.clone()),
        })
        .into_iter()
        .collect()
}

fn interrupt_message(error: InterruptError) -> String {
    match error {
        InterruptError::TooEarly => "还没有可打断的内容",
        InterruptError::Duplicate => "重复打断",
        InterruptError::Late => "这一轮已经结束",
        InterruptError::NotAllowed => "当前环节不能打断",
    }
    .to_owned()
}

fn reject(error: CommandReject) -> String {
    error.to_string()
}

fn claim_reject(error: CommandReject) -> String {
    match error {
        CommandReject::TakeoverNeedsConfirmation => "计分模式接管本方辩位需要再确认一次".to_owned(),
        CommandReject::CrossTeamTakeoverForbidden => "计分模式不能点对方辩位".to_owned(),
        other => other.to_string(),
    }
}

#[derive(Clone, Copy)]
struct CurrentCrossPresentation {
    questioner: SlotId,
    respondent: SlotId,
    speaker: SlotId,
    questioner_controller: Controller,
    respondent_controller: Controller,
}

impl CurrentCrossPresentation {
    fn controller_for(self, slot: SlotId) -> Option<Controller> {
        if slot == self.questioner {
            Some(self.questioner_controller)
        } else if slot == self.respondent {
            Some(self.respondent_controller)
        } else {
            None
        }
    }
}

/// Present current question/answer authority, not the schedule's team placeholder.
/// Only an in-progress cross-exam is affected; the confirmed lineup stays intact.
fn current_cross_presentation(state: &MatchAggregate) -> Option<CurrentCrossPresentation> {
    use crate::domain::events::CrossExamExchangeStatus;

    if state.phase != MatchPhase::InProgress {
        return None;
    }
    let event = state.schedule.get(state.cursor)?;
    if event.kind != StageKind::CrossExam {
        return None;
    }
    let (questioner, respondent) = state.current_cross_exam_roles().ok()?;
    let exchange = state.cross_exam_exchanges.iter().find(|exchange| {
        exchange.event_id == event.id
            && matches!(
                exchange.status,
                CrossExamExchangeStatus::Selected | CrossExamExchangeStatus::QuestionSubmitted
            )
    });
    Some(CurrentCrossPresentation {
        questioner,
        respondent,
        speaker: if exchange
            .is_some_and(|exchange| exchange.status == CrossExamExchangeStatus::QuestionSubmitted)
        {
            respondent
        } else {
            questioner
        },
        questioner_controller: state.effective_controller(questioner),
        respondent_controller: state.effective_controller(respondent),
    })
}

fn seat_rows(state: &MatchAggregate) -> Vec<SeatRow> {
    const NAMES: [&str; 8] = [
        "正方一辩",
        "正方二辩",
        "正方三辩",
        "正方四辩",
        "反方一辩",
        "反方二辩",
        "反方三辩",
        "反方四辩",
    ];
    let current = state.schedule.get(state.cursor);
    let current_cross = current_cross_presentation(state);
    let speaking = current_cross
        .map(|cross| cross.speaker)
        .or_else(|| current.map(|event| event.speaker));
    let in_play = state.phase == MatchPhase::InProgress && current.is_some();
    NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let slot = SlotId::ALL[index];
            let duty = state
                .ruleset
                .as_ref()
                .map(|ruleset| {
                    if index < 4 {
                        ruleset.affirmative_duties[index].clone()
                    } else {
                        ruleset.negative_duties[index - 4].clone()
                    }
                })
                .unwrap_or_default();
            let claimable = in_play
                && current.is_some_and(|event| {
                    if event.kind == StageKind::FreeDebate {
                        slot.side()
                            == state
                                .current_free_debate_side()
                                .unwrap_or(event.speaker.side())
                    } else if event.kind == StageKind::CrossExam {
                        current_cross.is_some_and(|cross| {
                            slot == cross.questioner || slot == cross.respondent
                        })
                    } else {
                        slot == event.speaker
                    }
                })
                && (state.mode != MatchMode::Scoring || state.user_side == Some(slot.side()));
            SeatRow {
                default_controller: enum_label(&state.assignments[slot.index()].controller),
                index,
                name: (*name).to_owned(),
                duty,
                side: enum_label(&slot.side()),
                controller: enum_label(
                    &current_cross
                        .and_then(|cross| cross.controller_for(slot))
                        .unwrap_or(state.effective_controller(slot)),
                ),
                speaking: Some(slot) == speaking,
                claimable,
            }
        })
        .collect()
}

fn difficulty_label(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Beginner => "入门",
        Difficulty::Standard => "标准",
        Difficulty::Contest => "竞赛级",
    }
}

fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|item| item.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub fn utc_stamp_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    utc_stamp(seconds)
}

pub fn utc_stamp(unix_seconds: u64) -> String {
    let days = unix_seconds / 86_400;
    let tod = unix_seconds % 86_400;
    let hour = tod / 3_600;
    let minute = (tod % 3_600) / 60;
    let second = tod % 60;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::{count_effective_units, utc_stamp, validated_stage_dimensions, Desk};
    use crate::domain::match_engine::{MatchCommand, MatchPhase};
    use crate::domain::report::{trend, PublicReport, REPORT_FORMULA};
    use crate::domain::ruleset::{MatchMode, Side, StageKind};
    use crate::domain::text_units::split_effective;
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::ai::ModelConfig;
    use crate::infrastructure::db::AppSettings;
    use uuid::Uuid;

    #[test]
    fn reselecting_current_human_does_not_restart_the_input_clock() {
        let mut desk = training_ready();
        desk.start().unwrap();
        let index = desk.aggregate.as_ref().unwrap().schedule[desk.view().cursor]
            .speaker
            .index();
        desk.switch_current_controller(index, true, false).unwrap();
        let started = desk.event_started;
        let revision = desk.turn_revision;
        desk.switch_current_controller(index, true, false).unwrap();
        assert_eq!(desk.event_started, started);
        assert_eq!(desk.turn_revision, revision);
    }

    #[test]
    fn default_current_control_is_only_this_window_and_preserves_the_roster() {
        let mut desk = training_ready();
        desk.start().unwrap();
        let slot = desk.aggregate.as_ref().unwrap().schedule[desk.view().cursor].speaker;
        desk.aggregate.as_mut().unwrap().assignments[slot.index()].controller =
            crate::domain::events::Controller::Ai;
        let before = desk.aggregate.as_ref().unwrap().assignments.clone();
        desk.switch_current_controller(slot.index(), true, false)
            .unwrap();
        assert!(desk.view().current_human);
        assert_eq!(desk.aggregate.as_ref().unwrap().assignments, before);
        desk.submit("本轮由人工代打".to_owned(), 1000, 0, 0, 0, false)
            .unwrap();
        assert_eq!(
            desk.aggregate
                .as_ref()
                .unwrap()
                .transcripts
                .last()
                .unwrap()
                .controller,
            Some(crate::domain::events::Controller::Human)
        );
        assert_eq!(desk.aggregate.as_ref().unwrap().assignments, before);
    }

    #[test]
    fn free_debate_control_rejection_does_not_partially_select_a_seat() {
        let mut desk = training_ready();
        desk.start().unwrap();
        let aggregate = desk.aggregate.as_mut().unwrap();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == StageKind::FreeDebate)
            .unwrap();
        aggregate.mode = MatchMode::Scoring;
        aggregate.user_side = Some(Side::Affirmative);
        let before = aggregate.to_snapshot();
        assert!(desk.switch_current_controller(1, true, false).is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
        assert!(desk.switch_current_controller(5, true, true).is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
        desk.switch_current_controller(1, true, true).unwrap();
        assert!(desk.view().current_human);
        desk.switch_current_controller(1, false, true).unwrap();
        assert!(!desk.view().current_human);
        let after = desk.aggregate.as_ref().unwrap();
        assert!(after.non_standard);
        assert_eq!(after.transcripts, before.transcripts);
        assert_eq!(after.word_budgets, before.word_budgets);
    }

    fn preparation_desk() -> Desk {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "原辩题".to_owned(),
            "原材料".to_owned(),
            Some(Side::Affirmative),
            &[],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk
    }

    #[test]
    fn preparation_analysis_applies_all_inputs_and_preserves_custom_rules() {
        let mut desk = preparation_desk();
        let mut custom = desk
            .aggregate
            .as_ref()
            .unwrap()
            .semantic_ruleset
            .clone()
            .unwrap();
        custom.ruleset_name = "用户自定义赛程".to_owned();
        desk.replace_semantic_rules(custom.clone()).unwrap();
        desk.confirm_rules().unwrap();
        desk.adopt_topic_for_analysis(super::RuleSetup {
            mode: MatchMode::Scoring,
            topic: "新辩题".to_owned(),
            materials: "新材料含关键证据".to_owned(),
            user_side: Some(Side::Negative),
            difficulty: Difficulty::Contest,
        })
        .unwrap();
        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.materials, "新材料含关键证据");
        assert_eq!(aggregate.mode, MatchMode::Scoring);
        assert_eq!(aggregate.user_side, Some(Side::Negative));
        assert_eq!(aggregate.semantic_ruleset.as_ref(), Some(&custom));
        let (_, _, context, constraint) = desk.prepare_analysis().unwrap();
        assert_eq!(context.materials, "新材料含关键证据");
        assert_eq!(constraint, Difficulty::Contest.constraint());
    }

    #[test]
    fn preparation_rejected_analysis_cannot_overwrite_valid_strategy() {
        let mut desk = preparation_desk();
        desk.analyze().unwrap();
        let before_strategy = desk.strategies_ref().cloned();
        let before_packets = desk.role_packets_ref().map(<[_]>::to_vec);
        let result = desk.store_analysis(
            "晚到的拆题".to_owned(),
            "不应该覆盖的正方策略".to_owned(),
            "不应该覆盖的反方策略".to_owned(),
            Vec::new(),
        );
        assert!(result.is_err());
        assert_eq!(desk.strategies_ref(), before_strategy.as_ref());
        assert_eq!(desk.role_packets_ref(), before_packets.as_deref());
    }

    #[test]
    fn preparation_outputs_and_difficulty_survive_snapshot_restore() {
        let mut desk = preparation_desk();
        desk.difficulty = Difficulty::Contest;
        desk.analyze().unwrap();
        let snapshot = desk.aggregate.as_ref().unwrap().to_snapshot();
        let aggregate =
            crate::domain::match_engine::MatchAggregate::from_snapshot(snapshot).unwrap();
        let before_strategy = desk.strategies_ref().cloned();
        let before_packets = desk.role_packets_ref().map(<[_]>::to_vec);
        let mut restored = Desk::new();
        restored.restore_match(aggregate, None);
        assert_eq!(restored.strategies_ref(), before_strategy.as_ref());
        assert_eq!(restored.role_packets_ref(), before_packets.as_deref());
        assert_eq!(restored.difficulty, Difficulty::Contest);
        assert_eq!(
            restored.aggregate.as_ref().unwrap().phase,
            MatchPhase::TopicAnalyzed
        );
    }

    #[test]
    fn preparation_topic_edit_invalidates_analysis_not_only_lineup() {
        let mut desk = preparation_desk();
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        desk.edit_topic("新的辩题".to_owned()).unwrap();
        assert!(desk.strategies_ref().is_none());
        assert!(desk.role_packets_ref().is_none());
        assert_eq!(
            desk.aggregate.as_ref().unwrap().phase,
            MatchPhase::RulesConfirmed
        );
        assert!(desk.aggregate.as_ref().unwrap().lineup_hash.is_none());
    }

    #[test]
    fn preparation_reconfirm_rules_requires_fresh_analysis_not_old_outputs() {
        let mut desk = preparation_desk();
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        desk.confirm_rules().unwrap();
        assert_eq!(
            desk.aggregate.as_ref().unwrap().phase,
            MatchPhase::RulesConfirmed
        );
        assert!(!desk.view().preparation_ready);
        assert!(desk.strategies_ref().is_none());
    }

    fn current_setup(desk: &Desk) -> super::RuleSetup {
        let aggregate = desk.aggregate.as_ref().unwrap();
        super::RuleSetup {
            mode: aggregate.mode,
            topic: aggregate.topic.clone(),
            materials: aggregate.materials.clone(),
            user_side: aggregate.user_side,
            difficulty: desk.difficulty,
        }
    }

    #[test]
    fn preparation_unchanged_save_preserves_ready_lineup_and_active_request() {
        let mut desk = preparation_desk();
        desk.profile_id = Some(Uuid::from_u128(9101));
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        let boundary = desk.begin_preparation_request().unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();
        desk.update_preparation(current_setup(&desk)).unwrap();
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
        assert!(desk.validate_preparation_request(&boundary).is_ok());
        assert!(desk.view().preparation_ready);
    }

    #[test]
    fn preparation_reanalysis_from_ready_preserves_confirmed_custom_schedule() {
        let mut desk = preparation_desk();
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        let rules = desk.aggregate.as_ref().unwrap().semantic_ruleset.clone();
        let schedule = desk.aggregate.as_ref().unwrap().schedule.clone();
        desk.adopt_topic_for_analysis(current_setup(&desk)).unwrap();
        assert_eq!(
            desk.aggregate.as_ref().unwrap().phase,
            MatchPhase::RulesConfirmed
        );
        assert_eq!(desk.aggregate.as_ref().unwrap().semantic_ruleset, rules);
        assert_eq!(desk.aggregate.as_ref().unwrap().schedule, schedule);
        assert!(!desk.view().preparation_ready);
        desk.analyze().unwrap();
        assert!(desk.view().preparation_ready);
    }

    #[test]
    fn preparation_change_rejected_after_start_and_no_match_is_not_created() {
        let mut desk = preparation_desk();
        let setup = current_setup(&desk);
        let mut empty = Desk::new();
        empty.update_preparation(setup).unwrap();
        assert!(empty.aggregate.is_none());
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        desk.start().unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();
        let mut changed = current_setup(&desk);
        changed.materials = "不可修改".to_owned();
        assert!(desk.update_preparation(changed).is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
    }

    #[test]
    fn preparation_topic_correction_preserves_speed_but_mode_change_clears_it() {
        let mut desk = preparation_desk();
        let mut scoring = current_setup(&desk);
        scoring.mode = MatchMode::Scoring;
        desk.update_preparation(scoring).unwrap();
        desk.baseline_cpm = Some(120.0);
        desk.calibration_samples = vec![115.0, 125.0];
        let mut correction = current_setup(&desk);
        correction.topic = "评分辩题修正".to_owned();
        desk.update_preparation(correction).unwrap();
        assert_eq!(desk.baseline_cpm, Some(120.0));
        let mut training = current_setup(&desk);
        training.mode = MatchMode::Training;
        desk.update_preparation(training).unwrap();
        assert!(desk.baseline_cpm.is_none());
        assert!(desk.calibration_samples.is_empty());
    }

    #[test]
    fn preparation_request_rejects_same_snapshot_restore_and_superseding_request() {
        let mut desk = preparation_desk();
        desk.profile_id = Some(Uuid::from_u128(9102));
        let first = desk.begin_preparation_request().unwrap();
        let second = desk.begin_preparation_request().unwrap();
        assert!(desk.validate_preparation_request(&first).is_err());
        assert!(desk.validate_preparation_request(&second).is_ok());
        let saved = desk.aggregate.as_ref().unwrap().clone();
        desk.restore_match(saved, None);
        assert!(desk.validate_preparation_request(&second).is_err());
    }

    #[test]
    fn preparation_request_rejects_changed_profile_rules_and_model() {
        let mut desk = preparation_desk();
        desk.profile_id = Some(Uuid::from_u128(9103));
        let rule_request = desk.begin_preparation_request().unwrap();
        let rules = desk
            .aggregate
            .as_ref()
            .unwrap()
            .semantic_ruleset
            .clone()
            .unwrap();
        desk.replace_semantic_rules(rules).unwrap();
        assert!(desk.validate_preparation_request(&rule_request).is_err());
        let model_request = desk.begin_preparation_request().unwrap();
        desk.model_config.model_id = "new-model".to_owned();
        assert!(desk.validate_preparation_request(&model_request).is_err());
        let profile_request = desk.begin_preparation_request().unwrap();
        desk.adopt_profile(Uuid::from_u128(9104));
        assert!(desk.validate_preparation_request(&profile_request).is_err());
    }

    #[test]
    fn preparation_legacy_restore_and_profile_switch_do_not_inherit_previous_difficulty() {
        let mut desk = preparation_desk();
        desk.difficulty = Difficulty::Contest;
        let mut old = desk.aggregate.as_ref().unwrap().clone();
        old.preparation = None;
        desk.restore_match(old, None);
        assert_eq!(desk.difficulty, Difficulty::Standard);
        desk.difficulty = Difficulty::Contest;
        desk.adopt_profile(Uuid::from_u128(9106));
        assert_eq!(desk.difficulty, Difficulty::Standard);
        assert!(!desk.view().preparation_ready);
    }

    #[test]
    fn preparation_legacy_ready_requires_reanalysis_but_started_match_keeps_phase_and_budgets() {
        let mut desk = preparation_desk();
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        let mut value =
            serde_json::to_value(desk.aggregate.as_ref().unwrap().to_snapshot()).unwrap();
        value.as_object_mut().unwrap().remove("preparation");
        let old: crate::domain::match_engine::MatchSnapshotV1 =
            serde_json::from_value(value).unwrap();
        let aggregate = crate::domain::match_engine::MatchAggregate::from_snapshot(old).unwrap();
        let assignments = aggregate.assignments.clone();
        let semantic = aggregate.semantic_ruleset.clone();
        let mut restored = Desk::new();
        restored.restore_match(aggregate.clone(), None);
        assert_eq!(
            restored.aggregate.as_ref().unwrap().phase,
            MatchPhase::RulesConfirmed
        );
        assert_eq!(
            restored.aggregate.as_ref().unwrap().assignments,
            assignments
        );
        assert_eq!(
            restored.aggregate.as_ref().unwrap().semantic_ruleset,
            semantic
        );
        assert!(restored.message.contains("重新拆题"));
        assert!(!restored.view().preparation_ready);
        let mut started = aggregate;
        started.handle(MatchCommand::Start).unwrap();
        let budget = started.word_budgets.clone();
        restored.restore_match(started, None);
        assert_eq!(
            restored.aggregate.as_ref().unwrap().phase,
            MatchPhase::InProgress
        );
        assert_eq!(restored.aggregate.as_ref().unwrap().word_budgets, budget);
    }

    #[test]
    fn preparation_sqlite_roundtrip_restores_outputs_difficulty_and_scoring_visibility() {
        let store = crate::infrastructure::db::SqliteStore::open(":memory:").unwrap();
        let profile = Uuid::from_u128(9105);
        store
            .create_profile(profile, "赛前恢复测试", "2026-10-07T00:00:00Z")
            .unwrap();
        let mut desk = preparation_desk();
        desk.profile_id = Some(profile);
        let mut setup = current_setup(&desk);
        setup.mode = MatchMode::Scoring;
        setup.user_side = Some(Side::Negative);
        setup.difficulty = Difficulty::Contest;
        desk.update_preparation(setup).unwrap();
        desk.analyze().unwrap();
        let original = desk.aggregate.as_ref().unwrap();
        store
            .save_match(profile, original, "2026-10-07T00:00:01Z")
            .unwrap();
        let loaded = store.load_match(original.id).unwrap();
        assert_eq!(loaded.word_budgets, original.word_budgets);
        let mut restored = Desk::new();
        restored.restore_match(loaded, None);
        assert_eq!(restored.difficulty, Difficulty::Contest);
        assert_eq!(restored.strategies_ref(), desk.strategies_ref());
        assert_eq!(restored.role_packets_ref(), desk.role_packets_ref());
        assert_eq!(restored.role_packets_ref().unwrap().len(), 8);
        let view = restored.view();
        assert_eq!(view.user_side, "negative");
        assert_eq!(view.difficulty, "contest");
        assert!(view.preparation_ready);
        assert!(view.opponent_strategy.is_none());
        assert_eq!(view.role_packets.len(), 4);
        assert!(view
            .role_packets
            .iter()
            .all(|packet| packet.slot.starts_with("NEG_")));
    }

    #[test]
    fn switching_profile_puts_down_the_open_match() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        let first = Uuid::from_u128(80);
        desk.adopt_profile(first);
        assert_eq!(desk.profile_id, Some(first));
        assert!(desk.aggregate.is_none());
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        desk.adopt_profile(first);
        assert!(desk.aggregate.is_some());
        desk.release_profile(first);
        assert!(desk.profile_id.is_none());
        assert!(desk.aggregate.is_none());
    }

    #[test]
    fn snapshot_exposes_match_identity_and_only_releases_terminal_matches() {
        let mut desk = Desk::new();
        assert!(desk.view().match_id.is_empty());
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            None,
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        let match_id = desk.aggregate.as_ref().unwrap().id.to_string();
        assert_eq!(desk.view().match_id, match_id);
        assert!(desk.release_completed_match().is_err());
        desk.aggregate.as_mut().unwrap().phase = MatchPhase::ReportReady;
        desk.release_completed_match().unwrap();
        assert!(desk.view().match_id.is_empty());
        assert!(desk.aggregate.is_none());
    }

    #[test]
    fn minute_limits_become_seconds_for_each_speaker() {
        let mut desk = Desk::new();
        let mut ruleset = crate::domain::ruleset::four_on_four_template();
        for stage in &mut ruleset.stages {
            stage.speaker_durations = vec![3; stage.speakers.len()];
            stage.duration_seconds = 3;
        }
        ruleset.stages[2].speaker_durations = vec![4, 5];
        desk.apply_recognized_rules(
            ruleset,
            "正方三分钟，自由辩正方四分钟反方五分钟",
            80,
            super::RuleSetup {
                mode: MatchMode::Training,
                topic: "辩题".to_owned(),
                materials: String::new(),
                user_side: Some(Side::Affirmative),
                difficulty: Difficulty::Standard,
            },
        )
        .unwrap();
        let view = desk.view();
        assert_eq!(view.schedule[0].duration_seconds, 180);
        let free = view
            .rule_draft
            .as_ref()
            .unwrap()
            .stages
            .iter()
            .find(|stage| {
                stage.semantic_type == crate::domain::ruleset_v2::SemanticStageType::FreeDebate
            })
            .unwrap();
        assert_eq!(free.affirmative_seconds, Some(240));
        assert_eq!(free.negative_seconds, Some(300));
        assert_eq!(
            view.schedule
                .iter()
                .filter(|event| event.kind == "free_debate")
                .count(),
            1
        );
    }

    #[test]
    fn a_confirmed_schedule_can_be_revised_before_the_match() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        assert!(desk.view().rules_confirmed);
        desk.revise_schedule_row(0, "AFF_2", 120).unwrap();
        let view = desk.view();
        assert!(!view.rules_confirmed);
        assert_eq!(view.schedule[0].speaker, "AFF_2");
        assert_eq!(view.schedule[0].duration_seconds, 120);
        assert_eq!(view.phase, "rules_parsed");
        assert!(desk.revise_schedule_row(0, "AFF_2", 0).is_err());
    }

    #[test]
    fn vague_rules_do_not_silently_become_the_default_template() {
        let err = super::parse_rules_with(
            false,
            ModelConfig::from_settings(&AppSettings::default()),
            "一辩陈词，自由辩交替，四辩总结。",
            "辩题",
            "",
        )
        .unwrap_err();
        assert!(err.contains("载入四对四模板"));
        assert!(!err.contains("已使用默认"));
    }

    #[test]
    fn pasted_times_are_read_without_the_model() {
        let mut desk = Desk::new();
        let source = "正方一辩立论3分钟，反方一辩立论3分钟，自由辩论正方4分钟，反方5分钟，反方四辩总结4分钟。";
        let (ruleset, confidence) = super::parse_rules_with(
            false,
            ModelConfig::from_settings(&AppSettings::default()),
            source,
            "辩题",
            "",
        )
        .unwrap();
        assert_eq!(confidence, 100);
        desk.apply_recognized_rules(
            ruleset,
            source,
            confidence,
            super::RuleSetup {
                mode: MatchMode::Training,
                topic: "辩题".to_owned(),
                materials: String::new(),
                user_side: Some(Side::Affirmative),
                difficulty: Difficulty::Standard,
            },
        )
        .unwrap();
        let view = desk.view();
        let rows: Vec<_> = view
            .schedule
            .iter()
            .map(|row| (row.speaker.as_str(), row.duration_seconds))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("AFF_1", 180),
                ("NEG_1", 180),
                ("AFF_1", 240),
                ("NEG_4", 240),
            ]
        );
        let free = view
            .rule_draft
            .as_ref()
            .unwrap()
            .stages
            .iter()
            .find(|stage| {
                stage.semantic_type == crate::domain::ruleset_v2::SemanticStageType::FreeDebate
            })
            .unwrap();
        assert_eq!(free.affirmative_seconds, Some(240));
        assert_eq!(free.negative_seconds, Some(300));
        assert!(view.message.contains("已按原文读出"));
        desk.confirm_rules().unwrap();
        assert!(desk.view().rules_confirmed);
    }

    #[test]
    fn empty_rules_require_an_explicit_template_choice() {
        let err = super::parse_rules_with(
            false,
            ModelConfig::from_settings(&AppSettings::default()),
            "",
            "人工智能应当受到限制",
            "",
        )
        .unwrap_err();
        assert!(err.contains("载入四对四模板"));
        let mut desk = Desk::new();
        desk.adopt_topic_for_analysis(super::RuleSetup {
            mode: MatchMode::Training,
            topic: "人工智能应当受到限制".to_owned(),
            materials: String::new(),
            user_side: Some(Side::Affirmative),
            difficulty: Difficulty::Standard,
        })
        .unwrap();
        let (_, _, context, _) = desk.prepare_analysis().unwrap();
        assert_eq!(context.topic, "人工智能应当受到限制");
        assert!(context.rules_text.contains("一辩"));
        assert!(!context.rules_text.contains("人工智能"));
    }

    #[test]
    fn restarted_sessions_do_not_reuse_record_ids() {
        let mut first = Desk::new();
        let mut second = Desk::new();
        assert_ne!(first.allocate_id(), second.allocate_id());
        first
            .open_template(
                MatchMode::Training,
                "辩题".to_owned(),
                String::new(),
                Some(Side::Affirmative),
                &[0],
                Difficulty::Standard,
            )
            .unwrap();
        second
            .open_template(
                MatchMode::Training,
                "辩题".to_owned(),
                String::new(),
                Some(Side::Affirmative),
                &[0],
                Difficulty::Standard,
            )
            .unwrap();
        first.confirm_rules().unwrap();
        second.confirm_rules().unwrap();
        let left = &first.view().schedule[0].id;
        let right = &second.view().schedule[0].id;
        assert_ne!(left, right);
    }

    #[test]
    fn offline_sessions_do_not_call_the_network() {
        let desk = Desk::new();
        assert!(!desk.live);
    }

    #[test]
    fn stage_scoring_requires_each_named_dimension_exactly_once() {
        let dimensions = crate::domain::review::DIMENSIONS
            .iter()
            .map(
                |(name, _)| crate::infrastructure::ai::schema::ScoreDimensionV1 {
                    name: (*name).to_owned(),
                    score: 70,
                },
            )
            .collect::<Vec<_>>();
        assert_eq!(validated_stage_dimensions(&dimensions).unwrap(), [70.0; 7]);
        assert!(validated_stage_dimensions(&dimensions[..6]).is_err());
        let mut duplicate = dimensions;
        duplicate[6].name = duplicate[0].name.clone();
        assert!(validated_stage_dimensions(&duplicate).is_err());
    }

    #[test]
    fn stamp_is_twenty_character_utc() {
        let stamp = utc_stamp(1_790_131_200);
        assert_eq!(stamp.len(), 20);
        assert!(stamp.ends_with('Z'));
    }

    #[test]
    fn template_reaches_ready_and_hides_scoring_strategy() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Scoring,
            "人工智能应被限制".to_owned(),
            String::new(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Contest,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        assert!(desk.view().schedule.len() >= 4);
        desk.analyze().unwrap();
        let visible = desk.view();
        assert!(visible
            .own_strategy
            .as_deref()
            .unwrap()
            .contains("正方主张"));
        assert!(visible.opponent_strategy.is_none());
        assert_eq!(visible.role_packets.len(), 4);
        assert!(visible
            .role_packets
            .iter()
            .all(|packet| packet.slot.starts_with("AFF")));
        complete_calibration(&mut desk);
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        assert_eq!(desk.aggregate.as_ref().unwrap().phase, MatchPhase::Ready);
        desk.edit_topic("修改后的辩题".to_owned()).unwrap();
        assert!(desk.aggregate.as_ref().unwrap().lineup_hash.is_none());
    }

    #[test]
    fn free_debate_interrupt_text_cannot_confirm() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            "自由辩可打断".to_owned(),
            None,
            &[],
            Difficulty::Standard,
        )
        .unwrap();
        assert!(desk.confirm_rules().is_err());
    }

    #[test]
    fn submit_once_wait_does_not_charge_and_failure_keeps_program() {
        let mut desk = training_ready();
        desk.start().unwrap();
        let charged = desk.aggregate.as_ref().unwrap().charged_ms;
        let remaining_before = desk.view().remaining_ms;
        thread::sleep(std::time::Duration::from_millis(120));
        desk.note_wait(1_500).unwrap();
        assert_eq!(desk.aggregate.as_ref().unwrap().charged_ms, charged);
        assert!(desk.aggregate.as_ref().unwrap().ai_wait_ms >= 1_500);
        assert!(desk.view().remaining_ms + 50 >= remaining_before);
        desk.submit("开场".to_owned(), 1_000, 0, 0, 0, false)
            .unwrap();
        let event_id = desk.aggregate.as_ref().unwrap().transcripts[0].event_id;
        let count = desk.aggregate.as_ref().unwrap().transcripts.len();
        assert!(desk
            .aggregate
            .as_mut()
            .unwrap()
            .handle(MatchCommand::SubmitCurrent {
                event_id,
                text: "开场".to_owned(),
                input_ms: 1_000,
            })
            .is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().transcripts.len(), count);
        while desk.aggregate.as_ref().unwrap().cursor
            < desk.aggregate.as_ref().unwrap().schedule.len()
        {
            if desk.view().current_human {
                let input_ms = desk
                    .aggregate
                    .as_ref()
                    .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
                    .filter(|event| event.kind == StageKind::FreeDebate)
                    .map(|event| u64::from(event.duration_seconds).saturating_mul(1_000))
                    .unwrap_or(500);
                desk.submit("继续".to_owned(), input_ms, 0, 0, 0, false)
                    .unwrap();
            } else {
                desk.advance_ai().unwrap();
            }
        }
        desk.finish(false).unwrap();
        let report = desk.report.clone().unwrap();
        assert_eq!(report.semantic, 0.0);
        assert!(report.program > 0.0);
        assert_eq!(
            desk.aggregate.as_ref().unwrap().phase,
            MatchPhase::ReportReady
        );
        let personal = report.personal;
        desk.report.as_mut().unwrap().team_winner = "affirmative".to_owned();
        assert_eq!(desk.report.as_ref().unwrap().personal, personal);
        assert_eq!(desk.report.as_ref().unwrap().team_winner, "affirmative");
        assert!(!desk.export().contains("keystroke"));
        assert_eq!(
            trend(std::slice::from_ref(desk.report.as_ref().unwrap())).len(),
            1
        );
    }

    #[test]
    fn scoring_takeover_of_the_other_side_is_rejected() {
        let error = super::build_assignments(MatchMode::Scoring, Some(Side::Affirmative), &[4]);
        assert!(error.is_err());
    }

    #[test]
    fn recovery_marks_the_scoring_match_non_standard() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Scoring,
            "辩题".to_owned(),
            String::new(),
            Some(Side::Negative),
            &[4],
            Difficulty::Beginner,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        complete_calibration(&mut desk);
        desk.assign_and_ready(Some(Side::Negative), &[4]).unwrap();
        desk.start().unwrap();
        desk.mark_recovery().unwrap();
        assert_eq!(
            desk.aggregate.as_ref().unwrap().phase,
            MatchPhase::RecoveryRequired
        );
        assert!(desk.aggregate.as_ref().unwrap().non_standard);
        assert!(!desk.view().standard);
    }

    #[test]
    fn phase9_cross_exam_keeps_the_tail_and_free_debate_does_not_change() {
        let mut desk = training_ready();
        desk.start().unwrap();
        let clocks = desk.floor.clone();
        let transcripts = desk.aggregate.as_ref().unwrap().transcripts.len();
        assert!(desk.interrupt_shown("你好世界".to_owned(), 2).is_err());
        assert_eq!(desk.floor, clocks);
        assert_eq!(
            desk.aggregate.as_ref().unwrap().transcripts.len(),
            transcripts
        );
        while desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| event.kind != StageKind::CrossExam)
        {
            desk.submit("推进到质询".to_owned(), 1_000, 0, 0, 0, false)
                .unwrap();
        }
        let input_before_interrupt = desk.input_ms;
        let charged_before_interrupt = desk.aggregate.as_ref().unwrap().charged_ms;
        desk.interrupt_shown_with_input("你好世界".to_owned(), 2, 3_456)
            .unwrap();
        assert_eq!(desk.input_ms, input_before_interrupt); // AI reply is not human input.
        assert_eq!(
            desk.aggregate.as_ref().unwrap().charged_ms - charged_before_interrupt,
            3_456
        );
        let row = desk
            .view()
            .transcripts
            .into_iter()
            .find(|row| row.interrupted_tail == "世界")
            .expect("tail");
        assert!(!row.effective_text.contains("世界"));
        assert!(desk.interrupt_shown("你好世界".to_owned(), 0).is_err());
    }

    #[test]
    fn restoring_a_scoring_match_marks_it_non_standard_and_restores_its_report() {
        let mut source = Desk::new();
        source
            .open_template(
                MatchMode::Scoring,
                "辩题".to_owned(),
                String::new(),
                Some(Side::Affirmative),
                &[0],
                Difficulty::Standard,
            )
            .unwrap();
        source.confirm_rules().unwrap();
        source.analyze().unwrap();
        complete_calibration(&mut source);
        source
            .assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        source.start().unwrap();

        let aggregate = source.aggregate.take().unwrap();
        let report = PublicReport {
            formula_version: REPORT_FORMULA.to_owned(),
            model_id: "deepseek-flash".to_owned(),
            difficulty: "标准".to_owned(),
            speech_rate: 240,
            personal: 80.0,
            program: 20.0,
            semantic: 60.0,
            team_winner: "tie".to_owned(),
            standard: true,
            non_standard_reason: String::new(),
            length: 8.0,
            input_control: 8.0,
            execution: 4.0,
            trace: "test".to_owned(),
            scoring_status: "READY".to_owned(),
            low_confidence: false,
            dimensions: Vec::new(),
            stage_evaluations: Vec::new(),
            review_summary: String::new(),
            team_summary: String::new(),
        };
        let mut restored = Desk::new();
        restored.restore_match(aggregate, Some(report.clone()));

        let aggregate = restored.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.phase, MatchPhase::InProgress);
        assert!(aggregate.non_standard);
        assert_eq!(restored.report, Some(report));
        assert!(restored.message.contains("未提交输入不会恢复"));
    }

    #[test]
    fn free_debate_uses_team_pools_allows_all_four_slots_and_alternates_sides() {
        let mut desk = training_ready();
        desk.start().unwrap();
        while desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| event.kind != StageKind::FreeDebate)
        {
            desk.submit("推进到自由辩".to_owned(), 1_000, 0, 0, 0, false)
                .unwrap();
        }
        assert!(desk.view().awaiting_seat);
        assert!(desk.claim_seat(4, false).is_err());
        desk.claim_seat(2, false).unwrap();
        assert!(desk.view().current_human);
        assert_eq!(desk.view().current_speaker, "AFF_3");
        assert!(desk
            .view()
            .seats
            .iter()
            .any(|seat| seat.speaking && seat.index == 2));
        let cursor = desk.view().cursor;
        let affirmative_before = desk.view().affirmative_free_units;
        let legacy_clock_before = desk.view().affirmative_ms;
        desk.submit("正方自由辩发言".to_owned(), 1_000, 0, 0, 0, false)
            .unwrap();
        assert_eq!(desk.view().cursor, cursor);
        assert_eq!(
            desk.view().affirmative_free_units,
            affirmative_before - count_effective_units("正方自由辩发言")
        );
        assert_eq!(desk.view().affirmative_ms, legacy_clock_before);
        assert_eq!(desk.view().floor_holder, "negative");
        assert!(desk.claim_seat(1, false).is_err());
        desk.claim_seat(5, false).unwrap();
        desk.submit("反方回应正方".to_owned(), 1_000, 0, 0, 0, false)
            .unwrap();
        assert_eq!(desk.view().cursor, cursor);
        assert_eq!(desk.view().floor_holder, "affirmative");
        assert_eq!(
            desk.aggregate
                .as_ref()
                .unwrap()
                .transcripts
                .last()
                .unwrap()
                .response_target_text,
            "正方自由辩发言"
        );
    }

    #[test]
    fn dynamic_cross_snapshot_tracks_actual_roles_effective_control_and_speaking_phase() {
        use crate::application::cross_exam_service::CrossExamService;
        use crate::domain::ruleset::SlotId;

        let mut desk = training_ready();
        desk.start().unwrap();
        while desk.aggregate.as_ref().unwrap().schedule[desk.view().cursor].kind
            != StageKind::CrossExam
        {
            desk.submit("推进质询".to_owned(), 100, 0, 0, 0, false)
                .unwrap();
        }
        let original_placeholder =
            desk.aggregate.as_ref().unwrap().schedule[desk.view().cursor].speaker;
        let mut cross = CrossExamService::default();
        cross
            .apply_role_selection(
                &mut desk,
                SlotId::Neg2,
                SlotId::Aff3,
                "派出反二质询正三".to_owned(),
            )
            .unwrap();
        let view = desk.view();
        assert_eq!(view.current_speaker, "NEG_2");
        assert!(!view.current_human);
        assert_eq!(
            view.seats
                .iter()
                .filter(|seat| seat.speaking)
                .map(|seat| seat.index)
                .collect::<Vec<_>>(),
            vec![5]
        );
        assert!(view.seats[5].claimable && view.seats[2].claimable);
        assert!(view
            .seats
            .iter()
            .all(|seat| [2, 5].contains(&seat.index) || !seat.claimable));
        assert_eq!(view.schedule[view.cursor].speaker, "NEG_2");

        cross
            .insert_human_questioner(&mut desk, SlotId::Neg2)
            .unwrap();
        let view = desk.view();
        assert!(
            view.current_human,
            "人工单轮插入不需要改变长期席位才能显示真人"
        );
        assert_eq!(view.seats[5].controller, "human");
        assert_eq!(view.schedule[view.cursor].controller, "human");
        cross
            .submit_question(&mut desk, "请回答当前问题".to_owned(), 100)
            .unwrap();
        let view = desk.view();
        assert_eq!(view.current_speaker, "AFF_3");
        assert!(!view.current_human);
        assert_eq!(
            view.seats
                .iter()
                .filter(|seat| seat.speaking)
                .map(|seat| seat.index)
                .collect::<Vec<_>>(),
            vec![2]
        );

        desk.claim_seat(2, false).unwrap();
        assert!(desk.view().current_human);
        cross.set_respondent_controller(&mut desk, false).unwrap();
        let view = desk.view();
        assert!(
            !view.current_human,
            "长期真人席位交还本轮 AI 后显示 AI 控制"
        );
        assert_eq!(view.seats[2].controller, "ai");
        assert_eq!(view.schedule[view.cursor].controller, "ai");
        assert_eq!(
            desk.aggregate.as_ref().unwrap().schedule[view.cursor].speaker,
            original_placeholder,
            "展示实际问答辩位不得改写已确认的原始赛程占位"
        );

        let aggregate = desk.aggregate.as_mut().unwrap();
        aggregate.mode = MatchMode::Scoring;
        aggregate.user_side = Some(Side::Affirmative);
        let view = desk.view();
        assert!(!view.seats[5].claimable);
        assert!(view.seats[2].claimable);
        assert!(desk.claim_seat(5, true).is_err(), "评分模式仍禁止接管对手");
    }

    #[test]
    fn one_ai_free_debate_turn_does_not_consume_the_whole_team_pool() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            None,
            &[],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        desk.assign_and_ready(None, &[]).unwrap();
        desk.start().unwrap();
        while desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| event.kind != StageKind::FreeDebate)
        {
            desk.advance_ai().unwrap();
        }
        let before = desk.view().affirmative_free_units;
        desk.advance_ai().unwrap();
        let after = desk.view().affirmative_free_units;
        assert!(after < before);
        assert!(after > 0);
        assert_eq!(desk.view().floor_holder, "negative");
    }

    #[test]
    fn each_side_can_waive_its_free_debate_quota_without_blocking_the_match() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            None,
            &[],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        desk.assign_and_ready(None, &[]).unwrap();
        desk.start().unwrap();
        while desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| event.kind != StageKind::FreeDebate)
        {
            desk.advance_ai().unwrap();
        }
        let free_cursor = desk.view().cursor;
        desk.waive_free_debate_time().unwrap();
        assert_eq!(desk.view().affirmative_free_units, 0);
        assert_eq!(desk.view().floor_holder, "negative");
        assert_eq!(desk.view().cursor, free_cursor);
        desk.waive_free_debate_time().unwrap();
        assert!(desk.view().cursor > free_cursor);
    }

    #[test]
    fn scoring_needs_two_full_minutes_and_uses_the_two_round_median() {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Scoring,
            "辩题".to_owned(),
            String::new(),
            Some(Side::Affirmative),
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        assert!(desk
            .assign_and_ready(Some(Side::Affirmative), &[0])
            .is_err());
        assert!(desk.accept_calibration("你好", 1_000, false, &[]).is_err());
        assert!(desk.accept_calibration("粘贴", 60_000, true, &[]).is_err());
        let first_passage = desk.passage();
        let first_input = split_effective(&first_passage, 80).0.to_owned();
        desk.accept_calibration(&first_input, 60_000, false, &[80.0, 120.0])
            .unwrap();
        assert!(desk.baseline_cpm.is_none());
        assert!(desk
            .assign_and_ready(Some(Side::Affirmative), &[0])
            .is_err());

        let second_passage = desk.passage();
        assert_ne!(first_passage, second_passage);
        let second_input = split_effective(&second_passage, 120).0.to_owned();
        desk.accept_calibration(&second_input, 60_000, false, &[80.0, 120.0])
            .unwrap();
        assert_eq!(desk.baseline_cpm, Some(100.0));
        desk.assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        assert_eq!(desk.aggregate.as_ref().unwrap().phase, MatchPhase::Ready);
    }

    #[test]
    fn training_can_reassign_unstarted_controller_and_scoring_blocks_opponent() {
        let mut desk = training_ready();
        desk.start().unwrap();
        // Current event cannot flip.
        assert!(desk.set_pending_controller(0, false, false).is_err());
        // Later event can flip in training.
        desk.set_pending_controller(2, true, false).unwrap();
        assert_eq!(desk.view().schedule[2].controller, "human");

        let mut scoring = Desk::new();
        scoring
            .open_template(
                MatchMode::Scoring,
                "辩题".to_owned(),
                String::new(),
                Some(Side::Affirmative),
                &[0],
                Difficulty::Standard,
            )
            .unwrap();
        scoring.confirm_rules().unwrap();
        scoring.analyze().unwrap();
        complete_calibration(&mut scoring);
        scoring
            .assign_and_ready(Some(Side::Affirmative), &[0])
            .unwrap();
        scoring.start().unwrap();
        let opp = scoring
            .view()
            .schedule
            .iter()
            .enumerate()
            .find(|(index, row)| *index > 0 && row.speaker.starts_with("NEG"))
            .map(|(index, _)| index)
            .expect("need opponent seat in schedule");
        assert!(scoring.set_pending_controller(opp, true, true).is_err());
    }

    fn training_ready() -> Desk {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            None,
            &[0],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        desk.assign_and_ready(None, &[0]).unwrap();
        desk
    }

    #[test]
    fn ai_turn_does_not_change_human_program_metrics() {
        let mut desk = training_ready();
        desk.start().unwrap();
        desk.submit("人工陈词".to_owned(), 5_000, 700, 2, 1, false)
            .unwrap();
        let expected = (
            desk.in_cap_units,
            desk.cap_units,
            desk.input_ms,
            desk.reverse_units,
        );
        // Only AFF_1 is human; this next stage is AI-controlled.
        desk.submit(
            "AI陈词不应进入个人程序分".to_owned(),
            10_000,
            3_000,
            7,
            4,
            false,
        )
        .unwrap();
        assert_eq!(
            (
                desk.in_cap_units,
                desk.cap_units,
                desk.input_ms,
                desk.reverse_units
            ),
            expected
        );
    }

    #[test]
    fn restoring_into_new_desk_preserves_human_metrics_and_baseline() {
        let mut desk = training_ready();
        desk.baseline_cpm = Some(95.5);
        desk.start().unwrap();
        desk.submit("人工陈词".to_owned(), 5_000, 700, 2, 1, false)
            .unwrap();
        let aggregate = crate::domain::match_engine::MatchAggregate::from_snapshot(
            desk.aggregate.as_ref().unwrap().to_snapshot(),
        )
        .unwrap();
        let mut restored = Desk::new();
        restored.restore_match(aggregate, None);
        assert_eq!(restored.in_cap_units, 4);
        assert_eq!(restored.input_ms, 5_000);
        assert_eq!(restored.reverse_units, 2);
        assert_eq!(restored.baseline_cpm, Some(95.5));
    }

    #[test]
    fn human_forward_input_uses_replay_counts_not_net_text() {
        let mut desk = training_ready();
        desk.start().unwrap();
        desk.submit("人工陈词".to_owned(), 5_000, 700, 2, 1, false)
            .unwrap();
        desk.apply_last_validated_input_counts(6, 4);
        assert_eq!(desk.forward_units, 6);
        assert_eq!(desk.in_cap_units, 4);
        assert_eq!(
            desk.aggregate
                .as_ref()
                .unwrap()
                .scoring_state
                .as_ref()
                .unwrap()
                .forward_units,
            6
        );
    }

    #[test]
    fn later_takeover_does_not_reclassify_previous_ai_transcript() {
        let mut desk = training_ready();
        desk.aggregate.as_mut().unwrap().assignments[0].controller =
            crate::domain::events::Controller::Ai;
        desk.start().unwrap();
        desk.submit("模型陈词".to_owned(), 5_000, 0, 0, 0, false)
            .unwrap();
        desk.aggregate.as_mut().unwrap().assignments[0].controller =
            crate::domain::events::Controller::Human;
        assert!(desk.human_marks().is_empty());
    }

    #[test]
    fn word_match_separates_input_time_from_shared_budget() {
        let mut desk = training_ready();
        desk.start().unwrap();
        assert_eq!(desk.view().metering_mode, "words");
        assert_eq!(desk.view().affirmative_main_units, 4_320);
        desk.submit("人工陈词".to_owned(), 600_000, 45_000, 2, 1, false)
            .unwrap();
        assert_eq!(desk.view().affirmative_main_units, 4_316);
        assert_eq!(desk.input_ms, 600_000);
        assert_eq!(desk.pause_ms, 45_000);
        assert_eq!(desk.view().remaining_ms, 0);
        assert_eq!(desk.view().negative_main_units, 4_320);
    }

    #[test]
    fn word_match_effective_cap_limits_program_length_and_preserves_overflow() {
        let mut desk = training_ready();
        desk.aggregate
            .as_mut()
            .unwrap()
            .word_budgets
            .as_mut()
            .unwrap()
            .affirmative_remaining_units = 3;
        desk.start().unwrap();
        assert_eq!(desk.view().current_word_cap, 3);
        desk.submit("人工陈词".to_owned(), 600_000, 0, 0, 0, false)
            .unwrap();
        let row = desk.aggregate.as_ref().unwrap().transcripts.last().unwrap();
        assert_eq!(row.effective_units, 3);
        assert_eq!(row.overflow_text, "词");
        assert_eq!(desk.cap_units, 3);
        assert_eq!(desk.in_cap_units, 3);
        assert_eq!(desk.overflow_units, 1);
    }

    fn complete_calibration(desk: &mut Desk) {
        for _ in 0..2 {
            let passage = desk.passage();
            desk.accept_calibration(&passage, 60_000, false, &[])
                .unwrap();
        }
    }
}
