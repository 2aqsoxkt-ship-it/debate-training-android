use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::application::ai_turn::generate_turn_text_with_progress;
use crate::application::cross_exam_service::{
    CrossExamGeneration, CrossExamSelectionRequest, CrossExamService, CrossExamSnapshot,
};
use crate::application::desk::Desk;
use crate::application::AppState;
use crate::domain::cross_exam::CrossExamPhase;
use crate::domain::events::{Controller, CrossExamExchangeStatus, CrossExamExchangeV1};
use crate::domain::match_engine::MatchPhase;
use crate::domain::ruleset::SlotId;
use crate::infrastructure::ai::schema::CrossExamSelectionV1;
use crate::infrastructure::ai::{DeepSeekTransport, Gateway, ThreadSleep, UreqHttp};
use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};

/// One owner and cancellation flag cover selection, generation and commit.
/// Cancellation keeps the slot occupied while the selected worker is exiting.
struct CrossExamRequest<'a> {
    state: &'a AppState,
    id: String,
    cancel: Arc<AtomicBool>,
}

impl<'a> CrossExamRequest<'a> {
    fn bind(state: &'a AppState) -> Result<Self, String> {
        static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
        let id = format!(
            "cross-exam-{}",
            NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst)
        );
        let cancel = super::stream::try_bind_stream(state, &id)?;
        Ok(Self { state, id, cancel })
    }
}

impl Drop for CrossExamRequest<'_> {
    fn drop(&mut self) {
        super::stream::clear_stream(self.state, &self.id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CrossExamBoundary {
    match_id: Uuid,
    event_id: String,
    round_number: u32,
    phase: CrossExamPhase,
    exchange: Option<CrossExamExchangeV1>,
    session_revision: Option<u64>,
    turn_revision: u64,
    controllers: [Controller; 8],
    questioner: SlotId,
    respondent: SlotId,
    questioner_human: bool,
    respondent_human: bool,
}

impl CrossExamBoundary {
    fn capture(desk: &Desk, cross: &mut CrossExamService) -> Result<Self, String> {
        let snapshot = cross.snapshot(desk)?;
        let aggregate = desk.aggregate.as_ref().ok_or("还没有赛程")?;
        if aggregate.phase != MatchPhase::InProgress {
            return Err("比赛状态已经变化，已丢弃过期质询请求".to_owned());
        }
        let event = aggregate
            .schedule
            .get(aggregate.cursor)
            .ok_or("当前没有环节")?;
        if event.id.to_string() != snapshot.event_id {
            return Err("质询环节已经变化，已丢弃过期请求".to_owned());
        }
        Ok(Self {
            match_id: aggregate.id,
            event_id: snapshot.event_id,
            round_number: snapshot.round_number,
            phase: cross.state.phase,
            exchange: aggregate
                .cross_exam_exchanges
                .iter()
                .find(|exchange| {
                    exchange.event_id == event.id
                        && matches!(
                            exchange.status,
                            CrossExamExchangeStatus::Selected
                                | CrossExamExchangeStatus::QuestionSubmitted
                        )
                })
                .cloned(),
            session_revision: aggregate
                .cross_exam_sessions
                .iter()
                .find(|session| session.event_id == event.id)
                .map(|session| session.revision),
            controllers: std::array::from_fn(|index| {
                aggregate.effective_controller(SlotId::ALL[index])
            }),
            turn_revision: desk.turn_revision,
            questioner: snapshot.questioner,
            respondent: snapshot.respondent,
            questioner_human: snapshot.questioner_human,
            respondent_human: snapshot.respondent_human,
        })
    }

    fn require_ai(&self, answer: bool) -> Result<(), String> {
        let (human, phase) = if answer {
            (self.respondent_human, CrossExamPhase::QuestionSubmitted)
        } else {
            (self.questioner_human, CrossExamPhase::QuestionPending)
        };
        if self.phase != phase {
            return Err("质询阶段已经变化，已丢弃过期请求".to_owned());
        }
        // Effective exchange control allows this answer to be handed to AI
        // while the long-lived roster remains Human. Roster changes still
        // invalidate old requests through the full boundary comparison.
        if human {
            return Err(if answer {
                "当前回答者由真人控制"
            } else {
                "当前提问者由真人控制"
            }
            .to_owned());
        }
        Ok(())
    }

    fn validate(
        &self,
        desk: &Desk,
        cross: &mut CrossExamService,
        answer: bool,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let current = Self::capture(desk, cross)?;
        if self != &current {
            return Err("质询辩位、控制权或轮次已经变化，已丢弃过期请求".to_owned());
        }
        current.require_ai(answer)?;
        ensure_not_cancelled(cancel)
    }
}

fn ensure_not_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("模型发言已取消".to_owned())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn cross_exam_generate_question(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CrossExamSnapshot, String> {
    let request = CrossExamRequest::bind(&state)?;
    let ids = request_ids(&state, &request.id)?;
    let _ = app.emit(
        "ai-stream",
        crate::infrastructure::ai::StreamEvent::Started { ids: ids.clone() },
    );
    let _ = app.emit(
        "ai-stream",
        crate::infrastructure::ai::StreamEvent::Status {
            ids: ids.clone(),
            label: "检查合法质询派位".to_owned(),
        },
    );
    {
        let desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        CrossExamBoundary::capture(&desk, &mut cross)?.require_ai(false)?;
    }
    select_roles_if_needed(&state, &request.cancel).await?;
    let generated = generate(&state, false, &request.cancel, app.clone(), ids.clone()).await?;
    let snapshot = {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        commit_generated_question(&mut desk, &mut cross, generated, &request.cancel)?
    };
    super::persist_current_match(&state)?;
    let _ = app.emit(
        "ai-stream",
        crate::infrastructure::ai::StreamEvent::Completed {
            ids,
            text: String::new(),
        },
    );
    Ok(snapshot)
}

fn apply_pending_selection(
    desk: &mut Desk,
    cross: &mut CrossExamService,
    boundary: &CrossExamBoundary,
    selected: Option<CrossExamSelectionV1>,
    fallback: (SlotId, SlotId),
    cancel: &AtomicBool,
) -> Result<(), String> {
    boundary.validate(desk, cross, false, cancel)?;
    if let Some(selection) = selected {
        if cross
            .apply_role_selection(
                desk,
                selection.questioner,
                selection.respondent,
                selection.selection_summary,
            )
            .is_ok()
        {
            return Ok(());
        }
    }
    // Cancellation and stale authority are not model failures. Neither may
    // enter fallback or restart generation after the user took control.
    boundary.validate(desk, cross, false, cancel)?;
    cross.apply_role_selection(
        desk,
        fallback.0,
        fallback.1,
        "DeepSeek 派位不可用，已按公开实录与出场均衡安全回退".to_owned(),
    )
}

async fn select_roles_if_needed(state: &AppState, cancel: &Arc<AtomicBool>) -> Result<(), String> {
    ensure_not_cancelled(cancel)?;
    let (request, boundary) = {
        let desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        let boundary = CrossExamBoundary::capture(&desk, &mut cross)?;
        boundary.require_ai(false)?;
        if boundary.exchange.is_some() {
            return Ok(());
        }
        (cross.selection_request(&desk)?, boundary)
    };
    let CrossExamSelectionRequest {
        live,
        config,
        context,
        event_id,
        round_number,
        fallback_questioner,
        fallback_respondent,
    } = request;
    if event_id.to_string() != boundary.event_id || round_number != boundary.round_number {
        return Err("质询派位上下文已变化，已丢弃过期请求".to_owned());
    }
    let selected = if live {
        let key = KeyringSecretStore::app()
            .read_api_key()
            .map_err(|_| "还没有可用的 DeepSeek 密钥".to_owned())?;
        let worker_cancel = Arc::clone(cancel);
        let selected = tauri::async_runtime::spawn_blocking(move || {
            Gateway::new(
                DeepSeekTransport::new(UreqHttp, config.clone(), key),
                ThreadSleep,
                config,
            )
            .select_cross_exam(&context, &worker_cancel)
            .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "质询派位任务被中断".to_owned())?;
        ensure_not_cancelled(cancel)?;
        selected.ok()
    } else {
        None
    };
    {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        apply_pending_selection(
            &mut desk,
            &mut cross,
            &boundary,
            selected,
            (fallback_questioner, fallback_respondent),
            cancel,
        )?;
    }
    super::persist_current_match(state)?;
    ensure_not_cancelled(cancel)
}

#[tauri::command]
pub async fn cross_exam_generate_answer(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CrossExamSnapshot, String> {
    let request = CrossExamRequest::bind(&state)?;
    let ids = request_ids(&state, &request.id)?;
    let _ = app.emit(
        "ai-stream",
        crate::infrastructure::ai::StreamEvent::Started { ids: ids.clone() },
    );
    let generated = generate(&state, true, &request.cancel, app.clone(), ids.clone()).await?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    let next = prepare_generated_answer(&desk, &mut cross, generated, &request.cancel)?;
    let _ = app.emit(
        "ai-stream",
        crate::infrastructure::ai::StreamEvent::Completed {
            ids,
            text: String::new(),
        },
    );
    Ok(next)
}

fn request_ids(
    state: &AppState,
    request_id: &str,
) -> Result<crate::infrastructure::ai::StreamIds, String> {
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let view = desk.view();
    Ok(crate::infrastructure::ai::StreamIds {
        request_id: request_id.to_owned(),
        match_id: view.match_id,
        event_id: view
            .schedule
            .get(view.cursor)
            .ok_or("当前没有质询")?
            .id
            .clone(),
    })
}

struct GeneratedCrossExamText {
    boundary: CrossExamBoundary,
    text: String,
}

fn commit_generated_question(
    desk: &mut Desk,
    cross: &mut CrossExamService,
    generated: GeneratedCrossExamText,
    cancel: &AtomicBool,
) -> Result<CrossExamSnapshot, String> {
    generated.boundary.validate(desk, cross, false, cancel)?;
    require_current_budget(desk, &generated.text)?;
    let input_ms = desk.speech_display_ms(&generated.text).max(1);
    cross.submit_question(desk, generated.text, input_ms)?;
    cross.snapshot(desk)
}

fn prepare_generated_answer(
    desk: &Desk,
    cross: &mut CrossExamService,
    generated: GeneratedCrossExamText,
    cancel: &AtomicBool,
) -> Result<CrossExamSnapshot, String> {
    generated.boundary.validate(desk, cross, true, cancel)?;
    require_current_budget(desk, &generated.text)?;
    cross.prepare_answer_with_metrics(desk, generated.text, None)?;
    cross.snapshot(desk)
}

fn require_current_budget(desk: &Desk, text: &str) -> Result<(), String> {
    let cap = desk
        .aggregate
        .as_ref()
        .ok_or("还没有赛程")?
        .current_word_cap();
    if crate::domain::text_units::count_effective_units(text) > cap {
        return Err("AI 候选超过当前字预算，未公开、未扣字，请重试或人工接管".to_owned());
    }
    Ok(())
}

async fn generate(
    state: &AppState,
    answer: bool,
    cancel: &Arc<AtomicBool>,
    app: AppHandle,
    ids: crate::infrastructure::ai::StreamIds,
) -> Result<GeneratedCrossExamText, String> {
    ensure_not_cancelled(cancel)?;
    let (request, boundary) = {
        let desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        let boundary = CrossExamBoundary::capture(&desk, &mut cross)?;
        boundary.require_ai(answer)?;
        (cross.generation_request(&desk, answer)?, boundary)
    };
    if !request.live {
        return Err("请先在设置页验证 DeepSeek 连接".to_owned());
    }
    let CrossExamGeneration {
        live,
        config,
        context,
        expected_slot,
        word_cap,
        event_id,
        round_number,
    } = request;
    if event_id.to_string() != boundary.event_id
        || round_number != boundary.round_number
        || expected_slot
            != if answer {
                boundary.respondent
            } else {
                boundary.questioner
            }
    {
        return Err("质询生成上下文已变化，已丢弃过期请求".to_owned());
    }
    let worker_cancel = Arc::clone(cancel);
    let result = tauri::async_runtime::spawn_blocking(move || {
        generate_turn_text_with_progress(
            live,
            config,
            &context,
            expected_slot,
            word_cap,
            &worker_cancel,
            |_| {},
            |label| {
                let _ = app.emit(
                    "ai-stream",
                    crate::infrastructure::ai::StreamEvent::Status {
                        ids: ids.clone(),
                        label: label.to_owned(),
                    },
                );
            },
        )
    })
    .await;
    ensure_not_cancelled(cancel)?;
    let text = result
        .map_err(|_| "模型生成被中断".to_owned())?
        .map_err(|message| {
            if message == "cancelled" {
                "模型发言已取消".to_owned()
            } else {
                message
            }
        })?;
    Ok(GeneratedCrossExamText { boundary, text })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::domain::ruleset::{MatchMode, StageKind};
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::db::SqliteStore;

    fn ready_at_cross() -> (Desk, CrossExamService) {
        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "本地竞态回归".to_owned(),
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
        while desk.aggregate.as_ref().unwrap().schedule[desk.view().cursor].kind
            != StageKind::CrossExam
        {
            desk.submit("推进到质询".to_owned(), 100, 0, 0, 0, false)
                .unwrap();
        }
        (desk, CrossExamService::default())
    }

    fn select_fallback(desk: &mut Desk, cross: &mut CrossExamService) {
        let pending = cross.selection_request(desk).unwrap();
        cross
            .apply_role_selection(
                desk,
                pending.fallback_questioner,
                pending.fallback_respondent,
                "本地确定性派位".to_owned(),
            )
            .unwrap();
    }

    #[test]
    fn cross_answer_controller_roundtrip_cannot_revive_old_boundary() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        cross
            .submit_question(&mut desk, "问题".to_owned(), 500)
            .unwrap();
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        cross.set_respondent_controller(&mut desk, true).unwrap();
        cross.set_respondent_controller(&mut desk, false).unwrap();
        assert!(boundary
            .validate(&desk, &mut cross, true, &AtomicBool::new(false))
            .is_err());
        assert!(!cross.snapshot(&desk).unwrap().answer_prepared);
    }

    #[test]
    fn late_selection_after_human_insertion_neither_overrides_nor_falls_back() {
        let (mut desk, mut cross) = ready_at_cross();
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        let pending = cross.selection_request(&desk).unwrap();
        cross
            .insert_human_questioner(&mut desk, pending.fallback_questioner)
            .unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();

        assert!(apply_pending_selection(
            &mut desk,
            &mut cross,
            &boundary,
            Some(CrossExamSelectionV1 {
                schema_version: "cross-exam-selection.v1".to_owned(),
                questioner: pending.fallback_questioner,
                respondent: pending.fallback_respondent,
                selection_summary: "迟到 AI 派位".to_owned(),
            }),
            (pending.fallback_questioner, pending.fallback_respondent),
            &AtomicBool::new(false),
        )
        .is_err());
        assert!(cross.snapshot(&desk).unwrap().questioner_human);
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
    }

    #[test]
    fn late_selection_after_take_seat_invalidates_even_a_selected_ai_exchange() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        let claimed_slot = boundary.questioner;
        desk.claim_seat(claimed_slot.index(), false).unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();
        assert!(apply_pending_selection(
            &mut desk,
            &mut cross,
            &boundary,
            None,
            (boundary.questioner, boundary.respondent),
            &AtomicBool::new(false),
        )
        .is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
        assert_eq!(
            desk.aggregate.as_ref().unwrap().assignments[claimed_slot.index()].controller,
            Controller::Human
        );
    }

    #[test]
    fn cancelled_selection_cannot_enter_safe_fallback() {
        let (mut desk, mut cross) = ready_at_cross();
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();
        let error = apply_pending_selection(
            &mut desk,
            &mut cross,
            &boundary,
            None,
            (boundary.questioner, boundary.respondent),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(error.contains("已取消"));
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
    }

    #[test]
    fn cancelled_generated_question_does_not_submit_or_charge_words() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();
        assert!(commit_generated_question(
            &mut desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary,
                text: "取消后迟到的问题".to_owned()
            },
            &AtomicBool::new(true),
        )
        .is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
    }

    #[test]
    fn late_generated_question_after_human_insertion_does_not_submit_or_charge_words() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        cross
            .insert_human_questioner(&mut desk, boundary.questioner)
            .unwrap();
        let before = desk.aggregate.as_ref().unwrap().to_snapshot();
        assert!(commit_generated_question(
            &mut desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary,
                text: "人工接管后迟到的 AI 问题".to_owned()
            },
            &AtomicBool::new(false),
        )
        .is_err());
        assert!(cross.snapshot(&desk).unwrap().questioner_human);
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
    }

    #[test]
    fn repeated_question_candidate_cannot_commit_twice_in_the_same_round() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        commit_generated_question(
            &mut desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary: boundary.clone(),
                text: "第一次正式提问".to_owned(),
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let after = desk.aggregate.as_ref().unwrap().to_snapshot();
        assert!(commit_generated_question(
            &mut desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary,
                text: "第二份过期提问".to_owned()
            },
            &AtomicBool::new(false),
        )
        .is_err());
        assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), after);
    }

    #[test]
    fn answer_after_controller_switch_or_cancel_never_becomes_prepared() {
        for cancel_answer in [false, true] {
            let (mut desk, mut cross) = ready_at_cross();
            select_fallback(&mut desk, &mut cross);
            cross
                .submit_question(&mut desk, "问题".to_owned(), 100)
                .unwrap();
            let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
            if !cancel_answer {
                cross.set_respondent_controller(&mut desk, true).unwrap();
            }
            let before = desk.aggregate.as_ref().unwrap().to_snapshot();
            assert!(prepare_generated_answer(
                &desk,
                &mut cross,
                GeneratedCrossExamText {
                    boundary,
                    text: "迟到 AI 回答".to_owned()
                },
                &AtomicBool::new(cancel_answer),
            )
            .is_err());
            assert!(cross.state.prepared.is_none());
            assert_eq!(desk.aggregate.as_ref().unwrap().to_snapshot(), before);
        }
    }

    #[test]
    fn explicitly_handing_a_human_roster_answer_to_ai_is_legal() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        cross
            .submit_question(&mut desk, "问题".to_owned(), 100)
            .unwrap();
        let respondent = cross.snapshot(&desk).unwrap().respondent;
        desk.claim_seat(respondent.index(), false).unwrap();
        cross.set_respondent_controller(&mut desk, false).unwrap();
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        boundary
            .require_ai(true)
            .expect("本轮交给 AI 回答应覆盖长期真人席位");
        prepare_generated_answer(
            &desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary,
                text: "合法的本轮 AI 回答".to_owned(),
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let total = cross.snapshot(&desk).unwrap().total_units;
        cross.reveal(total, 1_000).unwrap();
        cross.complete_and_commit(&mut desk, None).unwrap();
        assert_eq!(
            desk.aggregate
                .as_ref()
                .unwrap()
                .transcripts
                .last()
                .unwrap()
                .controller,
            Some(Controller::Ai),
            "本轮明确交给 AI 的回答不能按长期真人席位计真人分"
        );
    }

    #[test]
    fn repeated_answer_candidate_cannot_replace_an_answer_already_revealing() {
        let (mut desk, mut cross) = ready_at_cross();
        select_fallback(&mut desk, &mut cross);
        cross
            .submit_question(&mut desk, "问题".to_owned(), 100)
            .unwrap();
        let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
        prepare_generated_answer(
            &desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary: boundary.clone(),
                text: "首次准备的回答".to_owned(),
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(prepare_generated_answer(
            &desk,
            &mut cross,
            GeneratedCrossExamText {
                boundary,
                text: "过期回答不能覆盖".to_owned()
            },
            &AtomicBool::new(false),
        )
        .is_err());
        assert_eq!(
            cross.state.prepared.as_ref().unwrap().full_text,
            "首次准备的回答"
        );
    }

    #[test]
    fn request_boundary_checks_match_event_round_and_exchange_identity() {
        for changed in ["match", "event", "round", "exchange", "phase"] {
            let (mut desk, mut cross) = ready_at_cross();
            select_fallback(&mut desk, &mut cross);
            let boundary = CrossExamBoundary::capture(&desk, &mut cross).unwrap();
            let aggregate = desk.aggregate.as_mut().unwrap();
            match changed {
                "match" => aggregate.id = Uuid::from_u128(90_001),
                "event" => aggregate.schedule[aggregate.cursor].id = Uuid::from_u128(90_002),
                "round" => aggregate.cross_exam_sessions[0].round_number += 1,
                "exchange" => aggregate.cross_exam_exchanges[0].id = Uuid::from_u128(90_003),
                "phase" => aggregate.phase = MatchPhase::RecoveryRequired,
                _ => unreachable!(),
            }
            assert!(
                boundary
                    .validate(&desk, &mut cross, false, &AtomicBool::new(false))
                    .is_err(),
                "changed {changed}"
            );
        }
    }

    #[test]
    fn full_request_owner_blocks_repeated_generation_until_cancelled_worker_exits() {
        let (desk, cross) = ready_at_cross();
        let state = AppState {
            store: Mutex::new(SqliteStore::open(":memory:").unwrap()),
            desk: Mutex::new(desk),
            cross_exam: Mutex::new(cross),
            active_stream: Mutex::new(None),
        };
        let owner = CrossExamRequest::bind(&state).unwrap();
        let old_id = owner.id.clone();
        // This is still the same lease during selection and text generation.
        assert!(CrossExamRequest::bind(&state).is_err());
        owner.cancel.store(true, Ordering::SeqCst);
        assert!(CrossExamRequest::bind(&state).is_err());
        drop(owner);
        let next = CrossExamRequest::bind(&state).unwrap();
        assert_ne!(next.id, old_id);
        super::super::stream::clear_stream(&state, &old_id);
        assert_eq!(
            state
                .active_stream
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .request_id,
            next.id
        );
        assert!(!next.cancel.load(Ordering::SeqCst));
        drop(next);
        assert!(state.active_stream.lock().unwrap().is_none());
    }
}
