//! Cross-exam application service wrapping CrossExamState + desk submit.

use crate::application::desk::Desk;
use crate::domain::cross_exam::{CrossExamPhase, CrossExamState};
use crate::domain::events::{Controller, CrossExamSessionEndReason};
use crate::domain::match_engine::MatchResourceMode;
use crate::domain::ruleset::{MatchMode, Side, SlotId, StageKind};
use crate::domain::text_units::{count_effective_units, split_effective};
use crate::domain::timing::DEFAULT_SPEECH_RATE;
use crate::infrastructure::ai::{AiContext, ModelConfig};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct CrossExamService {
    pub state: CrossExamState,
    event_id: Option<Uuid>,
    pending_human_metrics: Option<HumanAnswerMetrics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossExamSnapshot {
    pub event_id: String,
    pub phase: String,
    pub questioner: SlotId,
    pub respondent: SlotId,
    pub questioner_human: bool,
    pub respondent_human: bool,
    pub question: String,
    pub visible_prefix: String,
    pub interruptible: bool,
    pub answer_prepared: bool,
    pub revealed_units: u32,
    pub total_units: u32,
    pub round_number: u32,
    pub next_side: Side,
    pub total_ms: u64,
    pub used_ms: u64,
    pub remaining_ms: u64,
    pub metering_mode: String,
    pub remaining_units: u32,
    pub question_word_cap: u32,
    pub answer_word_cap: u32,
    pub selection_summary: String,
    pub roles_selected: bool,
    pub eligible_questioners: Vec<SlotId>,
    pub history: Vec<CrossExamHistoryRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossExamHistoryRow {
    pub round_number: u32,
    pub questioner: SlotId,
    pub respondent: SlotId,
    pub question: String,
    pub answer: String,
    pub status: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HumanAnswerMetrics {
    pub input_ms: u64,
    pub pause_ms: u64,
    pub reverse_units: u32,
    pub reverse_actions: u32,
    pub pasted: bool,
    pub input_counts: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub struct CrossExamGeneration {
    pub live: bool,
    pub config: ModelConfig,
    pub context: AiContext,
    pub expected_slot: SlotId,
    pub word_cap: u32,
    pub event_id: Uuid,
    pub round_number: u32,
}

#[derive(Debug, Clone)]
pub struct CrossExamSelectionRequest {
    pub live: bool,
    pub config: ModelConfig,
    pub context: AiContext,
    pub event_id: Uuid,
    pub round_number: u32,
    pub fallback_questioner: SlotId,
    pub fallback_respondent: SlotId,
}

impl CrossExamService {
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.state = CrossExamState::new();
        self.event_id = None;
        self.pending_human_metrics = None;
    }

    pub fn ensure_cross_exam(desk: &Desk) -> Result<(), String> {
        let event = desk
            .aggregate
            .as_ref()
            .and_then(|state| state.schedule.get(state.cursor))
            .ok_or("当前没有环节")?;
        if event.kind != StageKind::CrossExam {
            return Err("当前不是质询环节".to_owned());
        }
        Ok(())
    }

    fn sync_for_desk(&mut self, desk: &Desk) -> Result<(), String> {
        Self::ensure_cross_exam(desk)?;
        let event = desk
            .aggregate
            .as_ref()
            .and_then(|state| state.schedule.get(state.cursor))
            .ok_or("当前没有环节")?;
        if self.event_id != Some(event.id) {
            self.state = CrossExamState::new();
            self.event_id = Some(event.id);
            self.pending_human_metrics = None;
        }
        if self.state.phase == CrossExamPhase::QuestionPending {
            if let Some(exchange) = desk.aggregate.as_ref().and_then(|state| {
                state.cross_exam_exchanges.iter().find(|exchange| {
                    exchange.event_id == event.id
                        && exchange.status
                            == crate::domain::events::CrossExamExchangeStatus::QuestionSubmitted
                })
            }) {
                self.state.submit_question(exchange.question_text.clone())?;
            }
        }
        Ok(())
    }

    pub fn snapshot(&mut self, desk: &Desk) -> Result<CrossExamSnapshot, String> {
        let completed_event_id = desk.aggregate.as_ref().and_then(|aggregate| {
            self.event_id.filter(|event_id| {
                aggregate
                    .schedule
                    .get(aggregate.cursor)
                    .map(|event| event.id)
                    != Some(*event_id)
                    && aggregate.cross_exam_sessions.iter().any(|session| {
                        session.event_id == *event_id && session.end_reason.is_some()
                    })
            })
        });
        if completed_event_id.is_none() {
            self.sync_for_desk(desk)?;
        }
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        let event = if let Some(event_id) = completed_event_id {
            aggregate.schedule.iter().find(|event| event.id == event_id)
        } else {
            aggregate.schedule.get(aggregate.cursor)
        }
        .ok_or("当前没有环节")?;
        let (questioner, respondent) = if completed_event_id.is_some() {
            aggregate
                .cross_exam_exchanges
                .iter()
                .rev()
                .find(|exchange| exchange.event_id == event.id)
                .map(|exchange| (exchange.questioner, exchange.respondent))
                .ok_or("已结束质询缺少问答记录")?
        } else {
            aggregate
                .current_cross_exam_roles()
                .map_err(|err| err.to_string())?
        };
        let session = aggregate
            .cross_exam_sessions
            .iter()
            .find(|session| session.event_id == event.id);
        let active_exchange = aggregate.cross_exam_exchanges.iter().find(|exchange| {
            exchange.event_id == event.id
                && matches!(
                    exchange.status,
                    crate::domain::events::CrossExamExchangeStatus::Selected
                        | crate::domain::events::CrossExamExchangeStatus::QuestionSubmitted
                )
        });
        let next_side = session
            .map(|session| session.next_side)
            .unwrap_or(questioner.side());
        let total_ms = session
            .map(|session| session.total_ms)
            .unwrap_or_else(|| u64::from(event.duration_seconds).saturating_mul(1_000));
        let used_ms = session.map(|session| session.used_ms).unwrap_or(0);
        let remaining_units = if completed_event_id.is_some() {
            0
        } else {
            aggregate.current_remaining_units()
        };
        let word_cap = if completed_event_id.is_some() {
            0
        } else {
            aggregate.current_word_cap()
        };
        let history = aggregate
            .cross_exam_exchanges
            .iter()
            .filter(|exchange| exchange.event_id == event.id)
            .filter(|exchange| {
                !matches!(
                    exchange.status,
                    crate::domain::events::CrossExamExchangeStatus::Selected
                )
            })
            .map(|exchange| CrossExamHistoryRow {
                round_number: exchange.round_number,
                questioner: exchange.questioner,
                respondent: exchange.respondent,
                question: exchange.question_text.clone(),
                answer: exchange
                    .answer_transcript_id
                    .and_then(|id| aggregate.transcripts.iter().find(|entry| entry.id == id))
                    .map(|entry| entry.effective_text.clone())
                    .unwrap_or_default(),
                status: format!("{:?}", exchange.status).to_lowercase(),
            })
            .collect();
        let eligible_questioners = aggregate
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .map(|stage| {
                if stage.eligible_questioners.is_empty() {
                    stage.questioner.into_iter().collect()
                } else {
                    stage
                        .eligible_questioners
                        .iter()
                        .copied()
                        .filter(|slot| slot.side() == next_side)
                        .collect()
                }
            })
            .unwrap_or_default();
        let snapshot = CrossExamSnapshot {
            event_id: event.id.to_string(),
            phase: if completed_event_id.is_some() {
                "completed"
            } else {
                phase_name(self.state.phase)
            }
            .to_owned(),
            questioner,
            respondent,
            questioner_human: aggregate.effective_controller(questioner) == Controller::Human,
            respondent_human: aggregate.effective_controller(respondent) == Controller::Human,
            question: self.state.question.clone(),
            visible_prefix: self.state.visible_prefix(),
            interruptible: event.interruptible,
            answer_prepared: self.state.prepared.is_some(),
            revealed_units: self
                .state
                .prepared
                .as_ref()
                .map(|prepared| prepared.revealed_units)
                .unwrap_or(0),
            total_units: self
                .state
                .prepared
                .as_ref()
                .map(|prepared| count_effective_units(&prepared.full_text))
                .unwrap_or(0),
            round_number: session.map(|session| session.round_number).unwrap_or(1),
            next_side,
            total_ms,
            used_ms,
            remaining_ms: if completed_event_id.is_some() {
                0
            } else {
                total_ms.saturating_sub(used_ms)
            },
            metering_mode: resource_mode_name(aggregate.metering_mode).to_owned(),
            remaining_units,
            question_word_cap: word_cap,
            answer_word_cap: word_cap,
            selection_summary: active_exchange
                .map(|exchange| exchange.selection_summary.clone())
                .unwrap_or_else(|| "系统将依据公开实录和出场均衡派位".to_owned()),
            roles_selected: active_exchange.is_some(),
            eligible_questioners,
            history,
        };
        if completed_event_id.is_some() {
            self.reset();
        }
        Ok(snapshot)
    }

    pub fn selection_request(&mut self, desk: &Desk) -> Result<CrossExamSelectionRequest, String> {
        self.sync_for_desk(desk)?;
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .ok_or("当前没有环节")?;
        let stage = aggregate
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .ok_or("找不到质询赛制定义")?;
        let (fallback_questioner, fallback_respondent) = aggregate
            .current_cross_exam_roles()
            .map_err(|error| error.to_string())?;
        let side = aggregate
            .cross_exam_sessions
            .iter()
            .find(|session| session.event_id == event.id)
            .map(|session| session.next_side)
            .or(stage.initial_questioning_side)
            .unwrap_or(fallback_questioner.side());
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
        let counts = aggregate
            .cross_exam_exchanges
            .iter()
            .filter(|exchange| exchange.event_id == event.id)
            .map(|exchange| format!("{:?}->{:?}", exchange.questioner, exchange.respondent))
            .collect::<Vec<_>>()
            .join(",");
        let remaining_ms = self.snapshot(desk)?.remaining_ms;
        let remaining_units = aggregate.current_remaining_units();
        let resource_context = resource_context(
            aggregate.metering_mode,
            remaining_units,
            remaining_ms,
            aggregate.speech_rate_per_minute,
        );
        Ok(CrossExamSelectionRequest {
            live: desk.live,
            config: desk.model_config.clone(),
            context: AiContext {
                topic: aggregate.topic.clone(),
                rules_text: serde_json::to_string(stage).unwrap_or_default(),
                materials: aggregate.materials.clone(),
                own_strategy: format!(
                    "next_side={side:?}\neligible_questioners={questioners:?}\neligible_respondents={respondents:?}\ncompleted_pairings={counts}\n{resource_context}"
                ),
                opponent_strategy: String::new(),
                public_transcript: aggregate
                    .transcripts
                    .iter()
                    .map(|entry| format!("{:?}: {}", entry.slot, entry.effective_text))
                    .collect(),
            },
            event_id: event.id,
            round_number: aggregate
                .cross_exam_sessions
                .iter()
                .find(|session| session.event_id == event.id)
                .map(|session| session.round_number)
                .unwrap_or(1),
            fallback_questioner,
            fallback_respondent,
        })
    }

    pub fn apply_role_selection(
        &mut self,
        desk: &mut Desk,
        questioner: SlotId,
        respondent: SlotId,
        summary: String,
    ) -> Result<(), String> {
        self.sync_for_desk(desk)?;
        if self.state.phase != CrossExamPhase::QuestionPending {
            return Err("质询环节已经变化，已丢弃过期派位".to_owned());
        }
        let snapshot = self.snapshot(desk)?;
        if snapshot.roles_selected && snapshot.questioner_human {
            return Err("当前提问者由真人控制，已丢弃过期派位".to_owned());
        }
        desk.aggregate
            .as_mut()
            .ok_or("还没有赛程")?
            .select_cross_exam_roles(questioner, respondent, summary)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn generation_request(
        &mut self,
        desk: &Desk,
        answer: bool,
    ) -> Result<CrossExamGeneration, String> {
        self.sync_for_desk(desk)?;
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .ok_or("当前没有环节")?;
        let (questioner, respondent) = aggregate
            .current_cross_exam_roles()
            .map_err(|err| err.to_string())?;
        if answer && self.state.phase != CrossExamPhase::QuestionSubmitted {
            return Err("请先提交问题".to_owned());
        }
        if !answer && self.state.phase != CrossExamPhase::QuestionPending {
            return Err("问题已经提交".to_owned());
        }
        let expected_slot = if answer { respondent } else { questioner };
        let duty = aggregate
            .ruleset
            .as_ref()
            .map(|rules| {
                let duties = if expected_slot.side() == crate::domain::ruleset::Side::Affirmative {
                    &rules.affirmative_duties
                } else {
                    &rules.negative_duties
                };
                duties[expected_slot.index() % 4].clone()
            })
            .unwrap_or_default();
        let task = if answer {
            format!(
                "cross_exam_task=answer\nquestioner={questioner:?}\nrespondent={respondent:?}\nquestion={}\nRespond directly to the question as respondent. Do not restate hidden strategy.",
                self.state.question
            )
        } else {
            format!(
                "cross_exam_task=question\nquestioner={questioner:?}\nrespondent={respondent:?}\nAsk one concise, answerable cross-exam question that tests the opponent's public case."
            )
        };
        let multi_round = aggregate
            .ruleset
            .as_ref()
            .and_then(|rules| rules.stages.iter().find(|stage| stage.id == event.stage_id))
            .is_some_and(|stage| !stage.eligible_questioners.is_empty());
        let word_cap = if aggregate.metering_mode == MatchResourceMode::Words {
            aggregate
                .current_word_cap()
                .min(if answer { 60 } else { 40 })
        } else if answer {
            if multi_round {
                event.word_cap.min(60)
            } else {
                event.word_cap
            }
        } else if multi_round {
            event.word_cap.min(40)
        } else {
            event.word_cap.clamp(20, 80)
        };
        if word_cap == 0 {
            return Err("质询字数额度已耗尽".to_owned());
        }
        let resource_context = resource_context(
            aggregate.metering_mode,
            aggregate.current_remaining_units(),
            self.snapshot(desk)?.remaining_ms,
            aggregate.speech_rate_per_minute,
        );
        Ok(CrossExamGeneration {
            live: desk.live,
            config: desk.model_config.clone(),
            context: AiContext {
                topic: aggregate.topic.clone(),
                rules_text: aggregate
                    .ruleset
                    .as_ref()
                    .and_then(|rules| serde_json::to_string(rules).ok())
                    .unwrap_or_default(),
                materials: aggregate.materials.clone(),
                own_strategy: format!(
                    "{task}\nrole_duty={duty}\n{resource_context}\nword_cap={word_cap}\n{}",
                    crate::domain::issue_ledger::prompt(&aggregate.transcripts)
                ),
                opponent_strategy: String::new(),
                public_transcript: aggregate
                    .transcripts
                    .iter()
                    .map(|entry| {
                        format!("{}: {:?}: {}", entry.id, entry.slot, entry.effective_text)
                    })
                    .collect(),
            },
            expected_slot,
            word_cap,
            event_id: event.id,
            round_number: aggregate
                .cross_exam_sessions
                .iter()
                .find(|session| session.event_id == event.id)
                .map(|session| session.round_number)
                .unwrap_or(1),
        })
    }

    pub fn submit_question(
        &mut self,
        desk: &mut Desk,
        question: String,
        input_ms: u64,
    ) -> Result<Uuid, String> {
        self.sync_for_desk(desk)?;
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        let word_metered = aggregate.metering_mode == MatchResourceMode::Words;
        let question = if word_metered {
            if aggregate.current_word_cap() == 0 {
                return Err("质询字数额度已耗尽".to_owned());
            }
            split_effective(&question, aggregate.current_word_cap())
                .0
                .to_owned()
        } else {
            question
        };
        let before = self.state.clone();
        self.state.submit_question(question.clone())?;
        let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
        let exchange_id = match aggregate.submit_cross_exam_question(question, input_ms) {
            Ok(exchange_id) => exchange_id,
            Err(error) => {
                self.state = before;
                return Err(error.to_string());
            }
        };
        if !word_metered {
            desk.charge_cross_exam_question_clock(input_ms)?;
        }
        if desk.aggregate.as_ref().is_some_and(|aggregate| {
            aggregate
                .cross_exam_exchanges
                .iter()
                .find(|exchange| exchange.id == exchange_id)
                .is_some_and(|exchange| {
                    exchange.status
                        != crate::domain::events::CrossExamExchangeStatus::QuestionSubmitted
                })
        }) {
            self.reset_for_next_exchange(desk);
        }
        Ok(exchange_id)
    }

    pub fn insert_human_questioner(
        &mut self,
        desk: &mut Desk,
        slot: SlotId,
    ) -> Result<CrossExamSnapshot, String> {
        self.sync_for_desk(desk)?;
        if self.state.phase != CrossExamPhase::QuestionPending {
            return Err("只能在问题提交前插入人工提问者".to_owned());
        }
        let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
        if aggregate.mode == MatchMode::Scoring && aggregate.user_side != Some(slot.side()) {
            return Err("评分模式只能使用已选择队伍的辩位".to_owned());
        }
        aggregate
            .override_cross_exam_questioner(slot)
            .map_err(|error| error.to_string())?;
        self.snapshot(desk)
    }

    pub fn set_respondent_controller(
        &mut self,
        desk: &mut Desk,
        human: bool,
    ) -> Result<CrossExamSnapshot, String> {
        self.sync_for_desk(desk)?;
        if self.state.phase != CrossExamPhase::QuestionSubmitted {
            return Err("只能在回答开始前切换答方控制".to_owned());
        }
        let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .ok_or("当前没有质询环节")?;
        let exchange = aggregate
            .cross_exam_exchanges
            .iter_mut()
            .find(|exchange| {
                exchange.event_id == event.id
                    && exchange.status
                        == crate::domain::events::CrossExamExchangeStatus::QuestionSubmitted
            })
            .ok_or("当前没有等待回答的问题")?;
        if human
            && aggregate.mode == MatchMode::Scoring
            && aggregate.user_side != Some(exchange.respondent.side())
        {
            return Err("评分模式不能接管对方辩位".to_owned());
        }
        let controller = if human {
            Controller::Human
        } else {
            Controller::Ai
        };
        let previous = exchange
            .respondent_controller
            .unwrap_or(aggregate.assignments[exchange.respondent.index()].controller);
        exchange.respondent_controller = Some(controller);
        if previous != controller {
            desk.turn_revision = desk.turn_revision.wrapping_add(1);
            for session in aggregate
                .cross_exam_sessions
                .iter_mut()
                .filter(|session| session.event_id == event.id)
            {
                session.revision = session.revision.saturating_add(1);
            }
        }
        self.snapshot(desk)
    }

    pub fn end_session(&mut self, desk: &mut Desk) -> Result<(), String> {
        self.finish_session(desk, CrossExamSessionEndReason::UserEnded)
    }

    pub fn timeout_session(&mut self, desk: &mut Desk) -> Result<(), String> {
        self.sync_for_desk(desk)?;
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        if aggregate.metering_mode == MatchResourceMode::Words
            && aggregate.current_remaining_units() > 0
        {
            return Err("字数模式不按用时超时，请用完字数额度或主动结束质询".to_owned());
        }
        self.finish_session(desk, CrossExamSessionEndReason::Timeout)
    }

    fn finish_session(
        &mut self,
        desk: &mut Desk,
        reason: CrossExamSessionEndReason,
    ) -> Result<(), String> {
        self.sync_for_desk(desk)?;
        let word_metered = desk
            .aggregate
            .as_ref()
            .is_some_and(|aggregate| aggregate.metering_mode == MatchResourceMode::Words);
        let has_visible_answer = self.state.phase == CrossExamPhase::AnswerRevealing
            && !self.state.visible_prefix().is_empty();
        if has_visible_answer {
            let input_ms = if reason == CrossExamSessionEndReason::Timeout && !word_metered {
                self.snapshot(desk)?.remaining_ms.max(1)
            } else {
                0
            };
            self.interrupt_and_commit_with_input(
                desk,
                if reason == CrossExamSessionEndReason::Timeout {
                    if word_metered {
                        "质询字数额度已耗尽".to_owned()
                    } else {
                        "质询共享时间已耗尽".to_owned()
                    }
                } else {
                    "用户结束质询".to_owned()
                },
                Some(HumanAnswerMetrics {
                    input_ms,
                    ..HumanAnswerMetrics::default()
                }),
            )?;
        } else {
            self.state.clear_prepared_memory();
        }
        let still_cross_exam = desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| Some(event.id) == self.event_id);
        if still_cross_exam {
            let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
            let current_event_id = aggregate
                .schedule
                .get(aggregate.cursor)
                .map(|event| event.id)
                .ok_or("当前没有质询环节")?;
            if !aggregate
                .cross_exam_sessions
                .iter()
                .any(|session| session.event_id == current_event_id)
            {
                aggregate
                    .start_cross_exam_round()
                    .map_err(|error| error.to_string())?;
            }
            aggregate
                .end_cross_exam_session(reason)
                .map_err(|error| error.to_string())?;
        }
        self.reset();
        desk.message = if reason == CrossExamSessionEndReason::Timeout {
            if word_metered {
                "质询字数额度已用完，只保存已公开内容".to_owned()
            } else {
                "质询时间已用完，只保存已公开内容".to_owned()
            }
        } else {
            "质询已结束".to_owned()
        };
        Ok(())
    }

    pub fn prepare_answer_with_metrics(
        &mut self,
        desk: &Desk,
        full_text: String,
        metrics: Option<HumanAnswerMetrics>,
    ) -> Result<(), String> {
        self.sync_for_desk(desk)?;
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        let full_text = if aggregate.metering_mode == MatchResourceMode::Words {
            if aggregate.current_word_cap() == 0 {
                return Err("质询字数额度已耗尽".to_owned());
            }
            split_effective(&full_text, aggregate.current_word_cap())
                .0
                .to_owned()
        } else {
            full_text
        };
        let interruptible = desk
            .aggregate
            .as_ref()
            .and_then(|state| state.schedule.get(state.cursor))
            .map(|event| event.interruptible)
            .unwrap_or(false);
        if self.state.phase == CrossExamPhase::QuestionSubmitted {
            self.state.begin_prepare()?;
        }
        self.state.set_prepared(full_text, interruptible)?;
        self.state.start_reveal()?;
        self.pending_human_metrics = metrics;
        Ok(())
    }

    pub fn reveal(&mut self, units: u32, elapsed_ms: u64) -> Result<String, String> {
        self.state.reveal_units(units, elapsed_ms)
    }

    /// Prefix visible to the interrupting side — never includes the unrevealed tail.
    #[must_use]
    pub fn visible_for_interrupt_decision(&self) -> String {
        self.state.visible_prefix()
    }

    pub fn interrupt_and_commit_with_input(
        &mut self,
        desk: &mut Desk,
        reason: String,
        metrics: Option<HumanAnswerMetrics>,
    ) -> Result<(), String> {
        self.sync_for_desk(desk)?;
        let allowed = desk
            .aggregate
            .as_ref()
            .and_then(|state| state.schedule.get(state.cursor))
            .map(|event| event.interruptible)
            .unwrap_or(false);
        let (prefix, tail) = self.state.interrupt(reason.clone(), allowed)?;
        let displayed = count_effective_units(&prefix);
        // Human input time already charged when answer was prepared/submitted;
        // reveal animation must not double-charge. AI answers charge by revealed prefix.
        let rate = desk
            .aggregate
            .as_ref()
            .map(|state| state.speech_rate_per_minute)
            .unwrap_or(DEFAULT_SPEECH_RATE)
            .max(1);
        let simulated_ms = (displayed as u64).saturating_mul(60_000) / u64::from(rate);
        let metrics = self
            .pending_human_metrics
            .take()
            .or(metrics)
            .unwrap_or_default();
        let input_ms = if metrics.input_ms > 0 {
            metrics.input_ms
        } else {
            simulated_ms.max(1)
        };
        desk.commit_cross_exam_answer(
            prefix,
            input_ms,
            metrics.pause_ms,
            metrics.reverse_units,
            metrics.reverse_actions,
            metrics.pasted,
            Some(reason),
            tail,
        )?;
        if let Some((forward, positive)) = metrics.input_counts {
            desk.apply_last_validated_input_counts(forward, positive);
        }
        self.state.clear_prepared_memory();
        self.reset_for_next_exchange(desk);
        Ok(())
    }

    pub fn complete_and_commit(
        &mut self,
        desk: &mut Desk,
        metrics: Option<HumanAnswerMetrics>,
    ) -> Result<(), String> {
        self.sync_for_desk(desk)?;
        let text = self.state.complete_without_interrupt()?;
        let metrics = self
            .pending_human_metrics
            .take()
            .or(metrics)
            .unwrap_or_default();
        let input_ms = if metrics.input_ms > 0 {
            metrics.input_ms
        } else {
            desk.speech_display_ms(&text).max(1)
        };
        desk.commit_cross_exam_answer(
            text,
            input_ms,
            metrics.pause_ms,
            metrics.reverse_units,
            metrics.reverse_actions,
            metrics.pasted,
            None,
            String::new(),
        )?;
        if let Some((forward, positive)) = metrics.input_counts {
            desk.apply_last_validated_input_counts(forward, positive);
        }
        self.state.clear_prepared_memory();
        self.reset_for_next_exchange(desk);
        Ok(())
    }

    fn reset_for_next_exchange(&mut self, desk: &Desk) {
        let still_same_event = desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| Some(event.id) == self.event_id);
        if still_same_event {
            self.state = CrossExamState::new();
            self.pending_human_metrics = None;
        }
    }
}

fn phase_name(phase: CrossExamPhase) -> &'static str {
    match phase {
        CrossExamPhase::QuestionPending => "question",
        CrossExamPhase::QuestionSubmitted | CrossExamPhase::AnswerPreparing => "preparing",
        CrossExamPhase::AnswerPrepared | CrossExamPhase::AnswerRevealing => "revealing",
        CrossExamPhase::AnswerCompleted | CrossExamPhase::AnswerInterrupted => "done",
    }
}

fn resource_mode_name(mode: MatchResourceMode) -> &'static str {
    match mode {
        MatchResourceMode::Words => "words",
        MatchResourceMode::Time => "time",
    }
}

fn resource_context(mode: MatchResourceMode, units: u32, remaining_ms: u64, rate: u32) -> String {
    match mode {
        MatchResourceMode::Words => {
            let estimated_seconds = u64::from(units).saturating_mul(60) / u64::from(rate.max(1));
            format!(
                "metering_mode=words\nword_budget={units}\nremaining_units={units}\nremaining_time={estimated_seconds}\nestimated_seconds={estimated_seconds}\nThe word_budget is the exact shared question-and-answer allowance. remaining_time is only an estimated reading duration; input or reveal time never consumes the allowance."
            )
        }
        MatchResourceMode::Time => format!(
            "metering_mode=time\nremaining_ms={remaining_ms}\nremaining_time={}",
            remaining_ms / 1_000
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{CrossExamService, HumanAnswerMetrics};
    use crate::application::desk::Desk;
    use crate::domain::events::{Controller, CrossExamSessionEndReason};
    use crate::domain::match_engine::{MatchResourceMode, WordBudgetsV1};
    use crate::domain::ruleset::{MatchMode, Side};
    use crate::domain::text_units::count_effective_units;
    use crate::domain::visibility::Difficulty;

    fn ready_at_cross() -> Desk {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "辩题".to_owned(),
            String::new(),
            Some(Side::Negative),
            &[6],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        desk.assign_and_ready(Some(Side::Negative), &[6]).unwrap();
        desk.start().unwrap();
        while desk
            .aggregate
            .as_ref()
            .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
            .is_some_and(|event| event.kind != crate::domain::ruleset::StageKind::CrossExam)
        {
            desk.submit("推进到质询".to_owned(), 500, 0, 0, 0, false)
                .unwrap();
        }
        desk
    }

    fn ready_with_word_budget(units: u32) -> Desk {
        let mut desk = ready_at_cross();
        let aggregate = desk.aggregate.as_mut().unwrap();
        aggregate.metering_mode = MatchResourceMode::Words;
        aggregate.word_budgets = Some(WordBudgetsV1 {
            affirmative_total_units: units,
            negative_total_units: units,
            affirmative_remaining_units: units,
            negative_remaining_units: units,
        });
        desk
    }

    #[test]
    fn snapshot_reports_all_four_controller_combinations() {
        for (questioner_human, respondent_human) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let mut desk = ready_at_cross();
            let (questioner, respondent) = desk
                .aggregate
                .as_ref()
                .unwrap()
                .current_cross_exam_roles()
                .unwrap();
            let aggregate = desk.aggregate.as_mut().unwrap();
            aggregate.assignments[questioner.index()].controller = if questioner_human {
                Controller::Human
            } else {
                Controller::Ai
            };
            aggregate.assignments[respondent.index()].controller = if respondent_human {
                Controller::Human
            } else {
                Controller::Ai
            };
            let mut service = CrossExamService::default();
            let snapshot = service.snapshot(&desk).unwrap();
            assert_eq!(snapshot.questioner, questioner);
            assert_eq!(snapshot.respondent, respondent);
            assert_eq!(snapshot.questioner_human, questioner_human);
            assert_eq!(snapshot.respondent_human, respondent_human);
        }
    }

    #[test]
    fn human_answer_metrics_are_charged_once_at_commit() {
        let mut desk = ready_at_cross();
        desk.aggregate.as_mut().unwrap().metering_mode = MatchResourceMode::Time;
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "请定义边界".to_owned(), 100)
            .unwrap();
        let before_prepare = desk.aggregate.as_ref().unwrap().charged_ms;
        let answer = "边界是对生命安全有重大影响的用途".to_owned();
        service
            .prepare_answer_with_metrics(
                &desk,
                answer.clone(),
                Some(HumanAnswerMetrics {
                    input_ms: 2_000,
                    pause_ms: 300,
                    reverse_units: 2,
                    reverse_actions: 1,
                    pasted: false,
                    input_counts: None,
                }),
            )
            .unwrap();
        assert_eq!(desk.aggregate.as_ref().unwrap().charged_ms, before_prepare);
        service
            .reveal(count_effective_units(&answer), 2_000)
            .unwrap();
        service.complete_and_commit(&mut desk, None).unwrap();
        assert_eq!(
            desk.aggregate.as_ref().unwrap().charged_ms,
            before_prepare + 2_000
        );
        // The new default starts with a negative-side team cross-examination.
        assert_eq!(desk.floor.affirmative_ms, 0);
        assert_eq!(desk.floor.negative_ms, 2_100);
    }

    #[test]
    fn late_ai_role_selection_cannot_replace_an_inserted_human_questioner() {
        let mut desk = ready_at_cross();
        let mut service = CrossExamService::default();
        let pending = service.selection_request(&desk).unwrap();
        service
            .insert_human_questioner(&mut desk, pending.fallback_questioner)
            .unwrap();
        let before = desk
            .aggregate
            .as_ref()
            .unwrap()
            .cross_exam_exchanges
            .clone();

        let late = service.apply_role_selection(
            &mut desk,
            pending.fallback_questioner,
            pending.fallback_respondent,
            "迟到的 AI 派位".to_owned(),
        );

        assert!(late.is_err(), "迟到派位不得覆盖人工插入");
        assert!(service.snapshot(&desk).unwrap().questioner_human);
        assert_eq!(
            desk.aggregate.as_ref().unwrap().cross_exam_exchanges,
            before
        );
        assert!(desk
            .aggregate
            .as_ref()
            .unwrap()
            .transcripts
            .iter()
            .all(|entry| {
                entry.cross_exam_turn_kind
                    != Some(crate::domain::events::CrossExamTurnKind::Question)
            }));
    }

    #[test]
    fn selected_roles_takeover_updates_the_effective_questioner_and_respondent() {
        for answer in [false, true] {
            let mut desk = ready_at_cross();
            let mut service = CrossExamService::default();
            let pending = service.selection_request(&desk).unwrap();
            // Both selected roles start as AI even though other roster seats
            // may be human in this training fixture.
            desk.aggregate.as_mut().unwrap().assignments[pending.fallback_questioner.index()]
                .controller = Controller::Ai;
            desk.aggregate.as_mut().unwrap().assignments[pending.fallback_respondent.index()]
                .controller = Controller::Ai;
            service
                .apply_role_selection(
                    &mut desk,
                    pending.fallback_questioner,
                    pending.fallback_respondent,
                    "系统选定当前问答辩位".to_owned(),
                )
                .unwrap();
            if answer {
                service
                    .submit_question(&mut desk, "请回答问题".to_owned(), 100)
                    .unwrap();
            }
            let slot = if answer {
                pending.fallback_respondent
            } else {
                pending.fallback_questioner
            };
            desk.claim_seat(slot.index(), false).unwrap();
            let snapshot = service.snapshot(&desk).unwrap();
            assert!(if answer {
                snapshot.respondent_human
            } else {
                snapshot.questioner_human
            });
            assert_eq!(
                if answer {
                    snapshot.respondent
                } else {
                    snapshot.questioner
                },
                slot
            );
        }
    }

    #[test]
    fn ending_during_reveal_saves_only_visible_prefix_and_does_not_create_empty_round() {
        let mut desk = ready_at_cross();
        let cursor_before = desk.aggregate.as_ref().unwrap().cursor;
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "请给出边界".to_owned(), 100)
            .unwrap();
        service
            .prepare_answer_with_metrics(&desk, "只公开这一段后面仍是隐藏内容".to_owned(), None)
            .unwrap();
        let visible = service.reveal(6, 500).unwrap();
        service.end_session(&mut desk).unwrap();

        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.cross_exam_exchanges.len(), 1);
        assert_eq!(
            aggregate.transcripts.last().unwrap().effective_text,
            visible
        );
        assert_eq!(aggregate.cursor, cursor_before + 1);
        assert_eq!(
            aggregate.cross_exam_sessions[0].end_reason,
            Some(CrossExamSessionEndReason::UserEnded)
        );
    }

    #[test]
    fn one_way_cross_exam_keeps_the_ruleset_questioning_side_between_rounds() {
        let mut desk = ready_at_cross();
        let initial_side = desk
            .aggregate
            .as_ref()
            .unwrap()
            .current_cross_exam_roles()
            .unwrap()
            .0
            .side();
        let cursor = desk.aggregate.as_ref().unwrap().cursor;
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "请说明标准".to_owned(), 100)
            .unwrap();
        service
            .prepare_answer_with_metrics(&desk, "标准是公共风险".to_owned(), None)
            .unwrap();
        service.reveal(8, 500).unwrap();
        service.complete_and_commit(&mut desk, None).unwrap();

        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.cursor, cursor);
        assert_eq!(
            aggregate.current_cross_exam_roles().unwrap().0.side(),
            initial_side
        );
        assert_eq!(aggregate.cross_exam_sessions[0].next_side, initial_side);
    }

    #[test]
    fn words_long_input_and_reveal_delays_do_not_exhaust_cross_exam() {
        let mut desk = ready_with_word_budget(100);
        let cursor = desk.aggregate.as_ref().unwrap().cursor;
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "请定义边界".to_owned(), 3_600_000)
            .unwrap();
        service
            .prepare_answer_with_metrics(
                &desk,
                "边界依据风险".to_owned(),
                Some(HumanAnswerMetrics {
                    input_ms: 7_200_000,
                    pause_ms: 7_199_000,
                    ..HumanAnswerMetrics::default()
                }),
            )
            .unwrap();
        service.reveal(6, 10_800_000).unwrap();
        service.complete_and_commit(&mut desk, None).unwrap();

        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.cursor, cursor);
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 11);
        assert_eq!(service.snapshot(&desk).unwrap().remaining_units, 89);
        assert!(service.timeout_session(&mut desk).is_err());
    }

    #[test]
    fn words_interrupt_only_charges_the_public_prefix() {
        let mut desk = ready_with_word_budget(100);
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "问".to_owned(), 3_600_000)
            .unwrap();
        service
            .prepare_answer_with_metrics(
                &desk,
                "公开内容后面隐藏回答".to_owned(),
                Some(HumanAnswerMetrics {
                    input_ms: 7_200_000,
                    ..HumanAnswerMetrics::default()
                }),
            )
            .unwrap();
        let prefix = service.reveal(4, 10_800_000).unwrap();
        service
            .interrupt_and_commit_with_input(&mut desk, "追问".to_owned(), None)
            .unwrap();

        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 5);
        assert_eq!(aggregate.transcripts.last().unwrap().effective_text, prefix);
        assert!(!aggregate
            .transcripts
            .last()
            .unwrap()
            .effective_text
            .contains("隐藏"));
        assert_eq!(service.snapshot(&desk).unwrap().remaining_units, 95);
        assert!(service.state.prepared.is_none());
    }

    #[test]
    fn words_question_and_answer_share_the_questioning_team_allowance() {
        let mut desk = ready_with_word_budget(10);
        let cursor = desk.aggregate.as_ref().unwrap().cursor;
        let questioner = desk
            .aggregate
            .as_ref()
            .unwrap()
            .current_cross_exam_roles()
            .unwrap()
            .0;
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "问题内容".to_owned(), 3_600_000)
            .unwrap();
        let snapshot = service.snapshot(&desk).unwrap();
        assert_eq!(snapshot.metering_mode, "words");
        assert_eq!(snapshot.remaining_units, 6);
        assert_eq!(snapshot.answer_word_cap, 6);
        service
            .prepare_answer_with_metrics(&desk, "回答内容超过剩余额度".to_owned(), None)
            .unwrap();
        let prepared = service.snapshot(&desk).unwrap();
        assert_eq!(prepared.total_units, 6);
        service.reveal(99, 10_800_000).unwrap();
        service.complete_and_commit(&mut desk, None).unwrap();

        let aggregate = desk.aggregate.as_ref().unwrap();
        assert!(aggregate.cursor > cursor);
        assert_eq!(aggregate.remaining_main_units(questioner.side()), 0);
        let other_side = match questioner.side() {
            Side::Affirmative => Side::Negative,
            Side::Negative => Side::Affirmative,
        };
        assert_eq!(aggregate.remaining_main_units(other_side), 10);
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 10);
        assert_eq!(aggregate.transcripts.last().unwrap().effective_units, 6);
        let terminal = service.snapshot(&desk).unwrap();
        assert_eq!(terminal.phase, "completed");
        assert_eq!(terminal.remaining_units, 0);
    }

    #[test]
    fn words_ai_generation_caps_follow_the_remaining_allowance() {
        let mut desk = ready_with_word_budget(100);
        let mut service = CrossExamService::default();
        let question = service.generation_request(&desk, false).unwrap();
        assert_eq!(question.word_cap, 40);
        assert!(question.context.own_strategy.contains("word_budget=100"));
        service
            .submit_question(&mut desk, "问".to_owned(), 9_000_000)
            .unwrap();
        let answer = service.generation_request(&desk, true).unwrap();
        assert_eq!(answer.word_cap, 60);
        assert!(answer.context.own_strategy.contains("word_budget=99"));

        let mut small = ready_with_word_budget(7);
        let mut small_service = CrossExamService::default();
        small_service
            .submit_question(&mut small, "问".to_owned(), 9_000_000)
            .unwrap();
        assert_eq!(
            small_service
                .generation_request(&small, true)
                .unwrap()
                .word_cap,
            6
        );
        assert_eq!(small_service.snapshot(&small).unwrap().question_word_cap, 6);
    }

    #[test]
    fn words_question_exhaustion_returns_a_terminal_snapshot() {
        let mut desk = ready_with_word_budget(4);
        let cursor = desk.aggregate.as_ref().unwrap().cursor;
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "问题内容多余不公开".to_owned(), 9_000_000)
            .unwrap();
        assert!(desk.aggregate.as_ref().unwrap().cursor > cursor);
        let terminal = service.snapshot(&desk).unwrap();
        assert_eq!(terminal.phase, "completed");
        assert_eq!(terminal.remaining_units, 0);
        assert_eq!(terminal.history[0].question, "问题内容");
    }

    #[test]
    fn restoring_saved_question_discards_uncommitted_answer_and_metrics() {
        let mut desk = ready_with_word_budget(100);
        let mut service = CrossExamService::default();
        service
            .submit_question(&mut desk, "请定义边界".to_owned(), 2_000)
            .unwrap();
        let question_budget = service.snapshot(&desk).unwrap().remaining_units;
        let saved_transcripts = desk.aggregate.as_ref().unwrap().transcripts.len();
        let store = crate::infrastructure::db::SqliteStore::open(":memory:").unwrap();
        let profile_id = uuid::Uuid::from_u128(88);
        let saved_at = "2026-10-04T00:00:00Z";
        store
            .create_profile(profile_id, "恢复回归", saved_at)
            .unwrap();
        store
            .save_match(profile_id, desk.aggregate.as_ref().unwrap(), saved_at)
            .unwrap();
        let match_id = desk.aggregate.as_ref().unwrap().id;
        let old_answer = "未提交回答及隐藏尾部";
        service
            .prepare_answer_with_metrics(
                &desk,
                old_answer.to_owned(),
                Some(HumanAnswerMetrics {
                    input_ms: 99_999,
                    pause_ms: 55_555,
                    input_counts: Some((99, 99)),
                    ..HumanAnswerMetrics::default()
                }),
            )
            .unwrap();
        service
            .reveal(count_effective_units(old_answer), 88_888)
            .unwrap();
        assert!(service.state.prepared.is_some());
        assert!(service.pending_human_metrics.is_some());

        crate::commands::restore_match_state(
            &mut desk,
            &mut service,
            store.load_match(match_id).unwrap(),
            None,
        );
        let restored = service.snapshot(&desk).unwrap();
        assert_eq!(restored.phase, "preparing");
        assert_eq!(restored.question, "请定义边界");
        assert_eq!(restored.remaining_units, question_budget);
        assert!(!restored.answer_prepared);
        assert!(restored.visible_prefix.is_empty());
        assert!(service.pending_human_metrics.is_none());
        assert!(service.complete_and_commit(&mut desk, None).is_err());
        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(aggregate.transcripts.len(), saved_transcripts);
        assert_eq!(aggregate.cross_exam_sessions[0].used_units, 5);
        assert!(!serde_json::to_string(&aggregate.to_snapshot())
            .unwrap()
            .contains(old_answer));
    }
}
