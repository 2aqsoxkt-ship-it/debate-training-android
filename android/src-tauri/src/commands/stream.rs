use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tauri::{AppHandle, Emitter, State};

use crate::application::ai_turn::generate_turn_text_with_progress;
use crate::application::{ActiveStream, AppState};
use crate::domain::ruleset::SlotId;
use crate::infrastructure::ai::{StreamEvent, StreamIds, StreamSink};

struct TauriSink {
    app: AppHandle,
}

impl StreamSink for TauriSink {
    fn emit(&mut self, event: StreamEvent) {
        let _ = self.app.emit("ai-stream", event);
    }
}

/// Bind a new stream. Fails while a previous request (including a cancelled one
/// that has not finished exiting) still occupies the slot.
pub(crate) fn try_bind_stream(
    state: &AppState,
    request_id: &str,
) -> Result<Arc<AtomicBool>, String> {
    let mut slot = state.active_stream.lock().map_err(|_| "会话正忙")?;
    if slot.is_some() {
        return Err("上一次模型发言尚未结束，请稍候再试".to_owned());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *slot = Some(ActiveStream {
        cancel: Arc::clone(&cancel),
        request_id: request_id.to_owned(),
    });
    Ok(cancel)
}

/// Clear the active stream only when `request_id` still owns the slot.
pub(crate) fn clear_stream(state: &AppState, request_id: &str) {
    if let Ok(mut slot) = state.active_stream.lock() {
        if slot
            .as_ref()
            .is_some_and(|active| active.request_id == request_id)
        {
            *slot = None;
        }
    }
}

pub(crate) struct PreparedTurn {
    pub(crate) legal_team_slots: Vec<SlotId>,
    pub(crate) live: bool,
    pub(crate) config: crate::infrastructure::ai::ModelConfig,
    pub(crate) context: crate::infrastructure::ai::AiContext,
    pub(crate) ids: StreamIds,
    pub(crate) expected_slot: SlotId,
    pub(crate) word_cap: u32,
    pub(crate) revision: u64,
}

#[tauri::command]
pub async fn advance_ai(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::application::desk::UiSnapshot, String> {
    let prepared = {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        prepare_turn(&mut desk)?
    };
    execute_turn(app, state, prepared).await
}

// Keep preparation independently testable without a WebView or a live model.
pub(crate) fn prepare_turn(
    desk: &mut crate::application::desk::Desk,
) -> Result<PreparedTurn, String> {
    if desk.view().current_human {
        return Err("这一轮由本人提交".to_owned());
    }
    let view = desk.view();
    let match_id = desk
        .aggregate
        .as_ref()
        .map(|item| item.id.to_string())
        .unwrap_or_default();
    let event = desk
        .aggregate
        .as_ref()
        .and_then(|item| item.schedule.get(item.cursor).cloned())
        .ok_or_else(|| "当前没有可发言的环节".to_owned())?;
    if event.kind == crate::domain::ruleset::StageKind::CrossExam {
        return Err("质询必须使用提问—prepared 回答—展示流程".to_owned());
    }
    let ids = StreamIds::new(match_id, event.id.to_string());
    let packets = desk
        .role_packets_ref()
        .map(|items| items.to_vec())
        .or_else(|| {
            desk.aggregate
                .as_ref()
                .and_then(|state| state.ruleset.as_ref())
                .map(crate::domain::context_builder::default_packets)
        })
        .unwrap_or_default();
    let expected_slot = event.speaker;
    // Free debate has no per-turn hard limit, but an AI that targets the
    // whole four-minute team pool would exhaust it in one speech. Keep
    // the authoritative event cap unchanged and give generation a
    // short-turn target so several debaters can build a response chain.
    let available_cap = desk
        .aggregate
        .as_ref()
        .map(|aggregate| aggregate.current_word_cap())
        .unwrap_or(0);
    let word_cap = if event.kind == crate::domain::ruleset::StageKind::FreeDebate {
        available_cap.min(120)
    } else {
        available_cap
    };
    if word_cap == 0 {
        return Err("当前字数额度已耗尽".to_owned());
    }
    let is_ai_turn = !view.current_human;
    // Free debate: pick among same-side AI seats by appearance counts.
    let expected_slot = if event.kind == crate::domain::ruleset::StageKind::FreeDebate && is_ai_turn
    {
        let side = expected_slot.side();
        let candidates: Vec<_> = crate::domain::ruleset::SlotId::ALL
            .into_iter()
            .filter(|slot| {
                slot.side() == side
                    && desk.aggregate.as_ref().is_some_and(|aggregate| {
                        aggregate.effective_controller(*slot)
                            == crate::domain::events::Controller::Ai
                    })
            })
            .collect();
        let mut counts = std::collections::HashMap::new();
        if let Some(aggregate) = desk.aggregate.as_ref() {
            for entry in &aggregate.transcripts {
                *counts.entry(entry.slot).or_insert(0_u32) += 1;
            }
        }
        crate::application::free_debate_service::pick_free_debate_slot(&candidates, &counts)
            .unwrap_or(expected_slot)
    } else {
        expected_slot
    };
    if let Some(aggregate) = desk.aggregate.as_mut() {
        if let Some(current) = aggregate.schedule.get_mut(aggregate.cursor) {
            if current.kind == crate::domain::ruleset::StageKind::FreeDebate {
                current.speaker = expected_slot;
            }
        }
    }
    // Role packets, duties and appearance counts must all describe the
    // actual chosen seat, not the placeholder from the previous free turn.
    let turn_context = desk.aggregate.as_ref().and_then(|aggregate| {
        crate::domain::context_builder::ContextBuilder::build(
            aggregate,
            desk.strategies_ref(),
            &packets,
            desk.difficulty,
            view.remaining_ms,
            if expected_slot.side() == crate::domain::ruleset::Side::Affirmative {
                view.affirmative_ms
            } else {
                view.negative_ms
            },
        )
        .ok()
    });
    let mut own_strategy = turn_context
        .as_ref()
        .map(|item| {
            format!(
                "{}
role={}
duty={}
main={}
backup={}
floor={}",
                item.own_strategy,
                item.role_packet.identity,
                item.role_packet.duty,
                item.role_packet.main_argument,
                item.role_packet.backup_rebuttal,
                item.role_packet.defense_floor
            )
        })
        .unwrap_or_else(|| desk.difficulty.constraint().to_owned());
    own_strategy.push_str(&format!(
        "\nrequired_slot={:?}\nword_cap={word_cap}\ntarget_units_low={}\ntarget_units_high={}",
        expected_slot,
        ((word_cap as f32) * 0.85).floor() as u32,
        ((word_cap as f32) * 0.90).floor() as u32
    ));
    if let Some(aggregate) = desk.aggregate.as_ref() {
        if aggregate.metering_mode == crate::domain::match_engine::MatchResourceMode::Words {
            own_strategy.push_str(&format!(
                    "\nmetering_mode=words\nword_budget={}\nThe word_budget is the exact remaining allowance; time fields only estimate reading duration.",
                    aggregate.current_remaining_units()
                ));
        }
    }
    let legal_team_slots: Vec<_> =
        if event.kind == crate::domain::ruleset::StageKind::FreeDebate && desk.live {
            SlotId::ALL
                .into_iter()
                .filter(|slot| {
                    slot.side() == expected_slot.side()
                        && desk.aggregate.as_ref().is_some_and(|a| {
                            a.effective_controller(*slot) == crate::domain::events::Controller::Ai
                        })
                })
                .collect()
        } else {
            Vec::new()
        };
    if !legal_team_slots.is_empty() {
        let own_packets: Vec<_> = packets
            .iter()
            .filter(|packet| legal_team_slots.contains(&packet.slot))
            .collect();
        own_strategy.push_str(&format!(
            "\n[本方合法候选的辩位任务]\n{}",
            serde_json::to_string(&own_packets).map_err(|_| "角色任务编码失败")?
        ));
    }
    let context = crate::infrastructure::ai::AiContext {
        topic: view.topic,
        rules_text: turn_context
            .as_ref()
            .map(|item| item.rules_summary.clone())
            .unwrap_or_default(),
        materials: turn_context
            .as_ref()
            .map(|item| item.materials.clone())
            .unwrap_or_default(),
        own_strategy,
        public_transcript: turn_context
            .as_ref()
            .map(|item| {
                item.public_turns
                    .iter()
                    .map(|turn| format!("{}: {}", turn.id, turn.text))
                    .collect()
            })
            .unwrap_or_default(),
        ..crate::infrastructure::ai::AiContext::default()
    };
    Ok(PreparedTurn {
        legal_team_slots,
        live: desk.live,
        config: desk.model_config.clone(),
        context,
        ids,
        expected_slot,
        word_cap,
        revision: desk.turn_revision,
    })
}

async fn execute_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    prepared: PreparedTurn,
) -> Result<crate::application::desk::UiSnapshot, String> {
    let PreparedTurn {
        legal_team_slots,
        live,
        config,
        context,
        ids,
        expected_slot,
        word_cap,
        revision,
    } = prepared;
    let cancel = try_bind_stream(&state, &ids.request_id)?;
    if state.desk.lock().map_err(|_| "会话正忙")?.turn_revision != revision {
        clear_stream(&state, &ids.request_id);
        return Err("控制权已经变化，已丢弃过期请求".to_owned());
    }
    let mut sink = TauriSink { app: app.clone() };
    sink.emit(StreamEvent::Started { ids: ids.clone() });
    sink.emit(StreamEvent::Status {
        ids: ids.clone(),
        label: "正在思考".to_owned(),
    });

    let wait_started = Instant::now();
    let joined = tauri::async_runtime::spawn_blocking({
        let cancel = Arc::clone(&cancel);
        let ids = ids.clone();
        let app = app.clone();
        move || -> Result<crate::application::ai_turn::TeamTurn, String> {
            let delta = |delta: &str| {
                let _ = app.emit(
                    "ai-stream",
                    StreamEvent::TextDelta {
                        ids: ids.clone(),
                        text: delta.to_owned(),
                    },
                );
            };
            let phase = |label: &str| {
                let _ = app.emit(
                    "ai-stream",
                    StreamEvent::Status {
                        ids: ids.clone(),
                        label: label.to_owned(),
                    },
                );
            };
            if !legal_team_slots.is_empty() {
                return crate::application::ai_turn::generate_team_turn(
                    config,
                    &context,
                    &legal_team_slots,
                    word_cap,
                    &cancel,
                    delta,
                    phase,
                );
            }
            generate_turn_text_with_progress(
                live,
                config,
                &context,
                expected_slot,
                word_cap,
                &cancel,
                delta,
                phase,
            )
            .map(|text| crate::application::ai_turn::TeamTurn {
                slot: expected_slot,
                text,
                reply_to: Vec::new(),
            })
        }
    })
    .await;
    let waited_ms = wait_started.elapsed().as_millis() as u64;
    let result = match joined {
        Ok(result) => result,
        Err(_) => {
            sink.emit(StreamEvent::Failed {
                ids: ids.clone(),
                category: "cancelled".to_owned(),
                message: "模型发言被中断".to_owned(),
            });
            clear_stream(&state, &ids.request_id);
            return Err("模型发言被中断，可以重试或人工接管".to_owned());
        }
    };

    if cancel.load(Ordering::SeqCst) {
        sink.emit(StreamEvent::Cancelled { ids: ids.clone() });
        clear_stream(&state, &ids.request_id);
        return Err("模型发言已取消".to_owned());
    }

    match result {
        Ok(turn) => {
            let text = turn.text;
            sink.emit(StreamEvent::Status {
                ids: ids.clone(),
                label: "提交合法稿并保存现场".to_owned(),
            });
            // Retain cancellation ownership until the authoritative commit.
            // A generated candidate is not yet a completed public speech.
            let committed = crate::commands::mutate_submit_selected_ai(
                &state,
                text.clone(),
                waited_ms,
                &ids.match_id,
                &ids.event_id,
                expected_slot,
                revision,
                &cancel,
                turn.slot,
                turn.reply_to,
            );
            match &committed {
                Ok(_) => sink.emit(StreamEvent::Completed {
                    ids: ids.clone(),
                    text,
                }),
                Err(message) if message.contains("已取消") => {
                    sink.emit(StreamEvent::Cancelled { ids: ids.clone() });
                }
                Err(message) => sink.emit(StreamEvent::Failed {
                    ids: ids.clone(),
                    category: "commit_rejected".to_owned(),
                    message: message.clone(),
                }),
            }
            clear_stream(&state, &ids.request_id);
            committed
        }
        Err(message) if message == "cancelled" => {
            sink.emit(StreamEvent::Cancelled { ids: ids.clone() });
            clear_stream(&state, &ids.request_id);
            Err("模型发言已取消".to_owned())
        }
        Err(message) => {
            sink.emit(StreamEvent::Failed {
                ids: ids.clone(),
                category: "unavailable".to_owned(),
                message: message.clone(),
            });
            clear_stream(&state, &ids.request_id);
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn cancel_ai_stream(state: State<'_, AppState>) -> Result<(), String> {
    {
        let slot = state.active_stream.lock().map_err(|_| "会话正忙")?;
        if let Some(active) = slot.as_ref() {
            active.cancel.store(true, Ordering::SeqCst);
        }
    }
    // Do not label cancellation complete while transport still owns the slot.
    for _ in 0..250 {
        if state
            .active_stream
            .lock()
            .map_err(|_| "会话正忙")?
            .is_none()
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    Err("已请求取消，旧请求尚在退出，请稍后重试".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::application::ActiveStream;

    #[test]
    fn free_debate_context_uses_the_selected_debater_not_the_previous_seat() {
        use crate::application::desk::Desk;
        use crate::domain::ruleset::{MatchMode, SlotId, StageKind};
        use crate::domain::visibility::Difficulty;

        let mut desk = Desk::new();
        desk.open_template(
            MatchMode::Training,
            "人工智能应当受到限制".to_owned(),
            "闭卷材料".to_owned(),
            None,
            &[],
            Difficulty::Standard,
        )
        .unwrap();
        desk.confirm_rules().unwrap();
        desk.analyze().unwrap();
        desk.assign_and_ready(None, &[]).unwrap();
        desk.start().unwrap();
        // AFF_1 has already spoken; the balancing policy must now choose AFF_2.
        desk.submit("正方一辩的定义与判断标准".to_owned(), 500, 0, 0, 0, false)
            .unwrap();
        let aggregate = desk.aggregate.as_mut().unwrap();
        aggregate.cursor = aggregate
            .schedule
            .iter()
            .position(|event| event.kind == StageKind::FreeDebate)
            .unwrap();
        assert_eq!(aggregate.schedule[aggregate.cursor].speaker, SlotId::Aff1);
        let expected_packet = desk
            .role_packets_ref()
            .unwrap()
            .iter()
            .find(|packet| packet.slot == SlotId::Aff2)
            .unwrap()
            .clone();

        let prepared = super::prepare_turn(&mut desk).unwrap();
        assert_eq!(prepared.expected_slot, SlotId::Aff2);
        assert!(prepared.context.own_strategy.contains("required_slot=Aff2"));
        assert!(prepared.context.own_strategy.contains("role=正方二辩"));
        assert!(!prepared.context.own_strategy.contains("role=正方一辩"));
        let expected_duty = &desk
            .aggregate
            .as_ref()
            .unwrap()
            .ruleset
            .as_ref()
            .unwrap()
            .affirmative_duties[1];
        assert!(prepared
            .context
            .own_strategy
            .contains(&format!("当前辩位职责：{expected_duty}")));
        assert!(prepared
            .context
            .own_strategy
            .contains(&format!("main={}", expected_packet.main_argument)));
        assert!(prepared
            .context
            .own_strategy
            .contains(&format!("backup={}", expected_packet.backup_rebuttal)));
        assert_eq!(
            desk.aggregate.as_ref().unwrap().schedule[desk.aggregate.as_ref().unwrap().cursor]
                .speaker,
            SlotId::Aff2
        );
    }

    fn clear_if_owner(slot: &Mutex<Option<ActiveStream>>, request_id: &str) {
        let mut guard = slot.lock().expect("lock");
        if guard
            .as_ref()
            .is_some_and(|active| active.request_id == request_id)
        {
            *guard = None;
        }
    }

    fn try_bind(
        slot: &Mutex<Option<ActiveStream>>,
        request_id: &str,
    ) -> Result<Arc<AtomicBool>, String> {
        let mut guard = slot.lock().expect("lock");
        if guard.is_some() {
            return Err("上一次模型发言尚未结束，请稍候再试".to_owned());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *guard = Some(ActiveStream {
            cancel: Arc::clone(&cancel),
            request_id: request_id.to_owned(),
        });
        Ok(cancel)
    }

    #[test]
    fn late_clear_does_not_wipe_a_newer_request() {
        let slot = Mutex::new(None);
        let first = try_bind(&slot, "req-old").expect("bind old");
        first.store(true, Ordering::SeqCst);
        // Cancel signals without clearing; retry must wait.
        assert!(try_bind(&slot, "req-new").is_err());
        clear_if_owner(&slot, "req-old");
        let second = try_bind(&slot, "req-new").expect("bind new");
        // A late clear from the old request must not drop the new owner.
        clear_if_owner(&slot, "req-old");
        assert!(slot.lock().expect("lock").is_some());
        assert!(!second.load(Ordering::SeqCst));
        clear_if_owner(&slot, "req-new");
        assert!(slot.lock().expect("lock").is_none());
    }

    #[test]
    fn cancel_keeps_slot_until_owner_clears() {
        let slot = Mutex::new(None);
        let cancel = try_bind(&slot, "req-1").expect("bind");
        cancel.store(true, Ordering::SeqCst);
        assert!(try_bind(&slot, "req-2").is_err());
        clear_if_owner(&slot, "req-1");
        assert!(try_bind(&slot, "req-2").is_ok());
    }
}
