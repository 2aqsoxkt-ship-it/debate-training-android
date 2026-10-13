use tauri::State;
pub mod mobile;

use crate::application::cross_exam_service::{CrossExamSnapshot, HumanAnswerMetrics};
use crate::application::desk::{utc_stamp_now, Desk, MatchRow, ProfileRow, UiSnapshot};
use crate::application::AppState;
use crate::domain::calibration::chargeable_pause;
use crate::domain::ruleset::{MatchMode, Side};
use crate::domain::visibility::Difficulty;
use crate::infrastructure::ai::ModelConfig;

pub mod cross_exam_ai;
pub mod experience;
pub mod settings;
pub mod stream;

pub(crate) fn snapshot(state: &AppState) -> Result<UiSnapshot, String> {
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut view = desk.view();
    let settings = store.load_settings().map_err(|err| err.to_string())?;
    let secrets = crate::infrastructure::credentials::KeyringSecretStore::app();
    view.model_settings = crate::application::model_settings::ModelSettingsDto::from_parts(
        &settings,
        crate::application::model_settings::has_api_key(&secrets),
    );
    view.uses_live_model = desk.live
        && crate::application::model_settings::live_allowed(view.model_settings.connection_status);
    view.profiles = store
        .list_profiles()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|profile| ProfileRow {
            id: profile.id.to_string(),
            display_name: profile.display_name,
        })
        .collect();
    if let Some(profile_id) = desk.profile_id {
        view.recent = store
            .recent_matches(profile_id, 50)
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|item| MatchRow {
                id: item.id.to_string(),
                topic: item.topic,
                phase: item.phase,
            })
            .collect();
    }
    Ok(view)
}

pub(crate) fn persist_current_match(state: &AppState) -> Result<(), String> {
    // Reserve storage BEFORE reading the current desk. Capturing a snapshot
    // before this lock allows an older writer to overwrite a newer save.
    // All combined locks use store -> desk -> cross_exam, as snapshot does.
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    save_desk(&store, &desk)
}

fn save_desk(store: &crate::infrastructure::db::SqliteStore, desk: &Desk) -> Result<(), String> {
    if let (Some(profile_id), Some(aggregate)) = (desk.profile_id, desk.aggregate.as_ref()) {
        let saved_at = utc_stamp_now();
        store
            .save_match(profile_id, aggregate, &saved_at)
            .map_err(|err| err.to_string())?;
        if let Some(report) = desk.report.as_ref() {
            store
                .save_report(aggregate.id, report, &saved_at)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn mutate<F>(state: &AppState, action: F) -> Result<UiSnapshot, String>
where
    F: FnOnce(&mut Desk) -> Result<(), String>,
{
    {
        // Keep the storage reservation through mutation and save. Never
        // publish a detached, potentially stale aggregate after unlocking.
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        let settings = store.load_settings().map_err(|err| err.to_string())?;
        let secrets = crate::infrastructure::credentials::KeyringSecretStore::app();
        let dto = crate::application::model_settings::ModelSettingsDto::from_parts(
            &settings,
            crate::application::model_settings::has_api_key(&secrets),
        );
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        desk.model_config = ModelConfig::from_settings(&settings);
        desk.live = crate::application::model_settings::live_allowed(dto.connection_status)
            && dto.has_api_key;
        if desk.live {
            desk.model_config.timeout_ms = 120_000;
        }
        action(&mut desk)?;
        save_desk(&store, &desk)?;
    }
    snapshot(state)
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn mutate_submit_ai(
    state: &AppState,
    text: String,
    waited_ms: u64,
    expected_match_id: &str,
    expected_event_id: &str,
    expected_slot: crate::domain::ruleset::SlotId,
    expected_revision: u64,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<UiSnapshot, String> {
    mutate_submit_selected_ai(
        state,
        text,
        waited_ms,
        expected_match_id,
        expected_event_id,
        expected_slot,
        expected_revision,
        cancel,
        expected_slot,
        Vec::new(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn mutate_submit_selected_ai(
    state: &AppState,
    text: String,
    waited_ms: u64,
    expected_match_id: &str,
    expected_event_id: &str,
    expected_slot: crate::domain::ruleset::SlotId,
    expected_revision: u64,
    cancel: &std::sync::atomic::AtomicBool,
    selected_slot: crate::domain::ruleset::SlotId,
    reply_to: Vec<String>,
) -> Result<UiSnapshot, String> {
    mutate(state, |desk| {
        // Check again at the authoritative commit boundary. Cancellation may
        // arrive while the generated text is waiting for store/desk locks.
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("模型发言已取消".to_owned());
        }
        let still_current = desk.aggregate.as_ref().is_some_and(|aggregate| {
            aggregate.id.to_string() == expected_match_id
                && aggregate
                    .schedule
                    .get(aggregate.cursor)
                    .is_some_and(|event| {
                        event.id.to_string() == expected_event_id
                            && event.speaker == expected_slot
                            && aggregate.effective_controller(expected_slot)
                                == crate::domain::events::Controller::Ai
                    })
        });
        if !still_current || desk.turn_revision != expected_revision {
            return Err("这条 AI 候选已因接管或赛程变化失效".to_owned());
        }
        let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
        if selected_slot != expected_slot
            && (aggregate.schedule[aggregate.cursor].kind
                != crate::domain::ruleset::StageKind::FreeDebate
                || selected_slot.side() != expected_slot.side()
                || aggregate.effective_controller(selected_slot)
                    != crate::domain::events::Controller::Ai)
        {
            return Err("团队派位已不合法，未提交".to_owned());
        }
        if reply_to.iter().any(|id| {
            !aggregate
                .transcripts
                .iter()
                .any(|t| t.id.to_string() == *id)
        }) {
            return Err("回应引用已失效，未提交".to_owned());
        }
        if crate::domain::text_units::count_effective_units(&text)
            > desk
                .aggregate
                .as_ref()
                .ok_or("还没有赛程")?
                .current_word_cap()
        {
            return Err("AI 候选超过当前字预算，未提交、未扣字，请重试或人工接管".to_owned());
        }
        if waited_ms > 0 {
            desk.note_wait(waited_ms)?;
        }
        let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
        aggregate.schedule[aggregate.cursor].speaker = selected_slot;
        let input_ms = desk.speech_display_ms(&text).max(1);
        desk.submit(text, input_ms, 0, 0, 0, false)?;
        if !reply_to.is_empty() {
            let aggregate = desk.aggregate.as_mut().ok_or("还没有赛程")?;
            if let Some(turn) = aggregate.transcripts.last() {
                aggregate.public_response_links.push(
                    crate::domain::issue_ledger::PublicResponseLink {
                        source_turn_id: turn.id.to_string(),
                        target_turn_ids: reply_to,
                    },
                );
            }
        }
        Ok(())
    })
}

#[tauri::command]
pub fn workspace_snapshot(state: State<AppState>) -> Result<UiSnapshot, String> {
    snapshot(&state)
}

#[tauri::command]
pub fn create_profile(state: State<AppState>, name: String) -> Result<UiSnapshot, String> {
    create_profile_in_state(&state, &name)?;
    snapshot(&state)
}

fn create_profile_in_state(state: &AppState, name: &str) -> Result<(), String> {
    // Profile creation and adoption share the store -> desk reservation. A
    // writer cannot save the previous match under the newly created profile.
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let id = desk.allocate_id();
    store
        .create_profile(id, name, &utc_stamp_now())
        .map_err(|err| err.to_string())?;
    desk.adopt_profile(id);
    desk.message = "档案已建立，已进入这份档案的独立赛前工作台。".to_owned();
    Ok(())
}

#[tauri::command]
pub fn select_profile(state: State<AppState>, id: String) -> Result<UiSnapshot, String> {
    let id = parse_profile_id(&id)?;
    {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        store
            .get_profile(id)
            .map_err(|_| "找不到这份档案".to_owned())?;
    }
    {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        desk.adopt_profile(id);
    }
    snapshot(&state)
}

#[tauri::command]
pub fn resume_match(state: State<AppState>, id: String) -> Result<UiSnapshot, String> {
    let id = uuid::Uuid::parse_str(&id).map_err(|_| "比赛编号无效".to_owned())?;
    resume_saved_match(&state, id)?;
    snapshot(&state)
}

fn resume_saved_match(state: &AppState, id: uuid::Uuid) -> Result<(), String> {
    {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        let aggregate = store.load_match(id).map_err(|err| err.to_string())?;
        let report = store.load_report(id).map_err(|err| err.to_string())?;
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        {
            // A same-match restore can retain every saved event/round ID.
            // Explicitly revoke the old request instead of trusting IDs alone.
            let active = state.active_stream.lock().map_err(|_| "会话正忙")?;
            if let Some(request) = active.as_ref() {
                request
                    .cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        restore_match_state(&mut desk, &mut cross, aggregate, report);
    }
    Ok(())
}

pub(crate) fn restore_match_state(
    desk: &mut Desk,
    cross: &mut crate::application::cross_exam_service::CrossExamService,
    aggregate: crate::domain::match_engine::MatchAggregate,
    report: Option<crate::domain::report::PublicReport>,
) {
    desk.restore_match(aggregate, report);
    cross.reset();
}

#[tauri::command]
pub fn abort_match(state: State<AppState>) -> Result<UiSnapshot, String> {
    {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let profile_id = desk.profile_id.ok_or("还没有选用档案")?;
        let mut aggregate = desk.aggregate.clone().ok_or("还没有比赛")?;
        aggregate
            .handle(crate::domain::match_engine::MatchCommand::AbortMatch)
            .map_err(|error| error.to_string())?;
        store
            .save_match(profile_id, &aggregate, &utc_stamp_now())
            .map_err(|error| error.to_string())?;
        desk.release_match(aggregate.id)?;
        state.cross_exam.lock().map_err(|_| "会话正忙")?.reset();
    }
    snapshot(&state)
}

#[tauri::command]
pub fn release_completed_match(state: State<AppState>) -> Result<UiSnapshot, String> {
    {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        desk.release_completed_match()?;
    }
    state.cross_exam.lock().map_err(|_| "会话正忙")?.reset();
    snapshot(&state)
}

#[tauri::command]
pub fn rename_profile(
    state: State<AppState>,
    id: String,
    name: String,
) -> Result<UiSnapshot, String> {
    let id = parse_profile_id(&id)?;
    {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        store
            .rename_profile(id, &name, &utc_stamp_now())
            .map_err(|err| {
                let text = err.to_string();
                if text.contains("profile name") {
                    "档案名称需要 1 到 80 个字".to_owned()
                } else if text.contains("not found") {
                    "找不到这份档案".to_owned()
                } else {
                    text
                }
            })?;
    }
    {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        desk.message = "档案已改名".to_owned();
    }
    snapshot(&state)
}

#[tauri::command]
pub fn delete_profile(
    state: State<AppState>,
    id: String,
    confirmed: bool,
) -> Result<UiSnapshot, String> {
    if !confirmed {
        return Err("删除档案会同时删除它下面的比赛，需要再确认一次".to_owned());
    }
    let id = parse_profile_id(&id)?;
    {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        store
            .delete_profile(id, true)
            .map_err(|_| "找不到这份档案".to_owned())?;
    }
    {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        desk.release_profile(id);
    }
    snapshot(&state)
}

fn parse_profile_id(id: &str) -> Result<uuid::Uuid, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "档案编号无效".to_owned())
}

#[tauri::command]
pub fn open_template(
    state: State<AppState>,
    mode: String,
    topic: String,
    materials: String,
    user_side: String,
    human_slots: Vec<usize>,
    difficulty: String,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| {
        if desk.profile_id.is_none() {
            return Err("先建立档案".to_owned());
        }
        desk.open_template(
            parse_mode(&mode)?,
            topic,
            materials,
            parse_side(&user_side),
            &human_slots,
            parse_difficulty(&difficulty)?,
        )
    })
}

#[tauri::command]
pub async fn recognize_rules(
    state: State<'_, AppState>,
    rules_text: String,
    mode: String,
    topic: String,
    materials: String,
    user_side: String,
    difficulty: String,
) -> Result<UiSnapshot, String> {
    let setup = crate::application::desk::RuleSetup {
        mode: parse_mode(&mode)?,
        topic,
        materials,
        user_side: parse_side(&user_side),
        difficulty: parse_difficulty(&difficulty)?,
    };
    let mut request = None;
    mutate(&state, |desk| {
        let boundary = desk.begin_preparation_request()?;
        request = Some((boundary, desk.live, desk.model_config.clone()));
        Ok(())
    })?;
    let (boundary, live, config) = request.ok_or("无法创建赛制识别请求")?;
    let source = rules_text.clone();
    let analysis_topic = setup.topic.clone();
    let analysis_materials = setup.materials.clone();
    let parsed = tauri::async_runtime::spawn_blocking(move || {
        crate::application::desk::parse_rules_with(
            live,
            config,
            &source,
            &analysis_topic,
            &analysis_materials,
        )
    })
    .await
    .map_err(|_| "赛制识别被中断".to_owned())?;
    match parsed {
        Ok((ruleset, confidence)) => mutate(&state, |desk| {
            desk.validate_preparation_request(&boundary)?;
            desk.apply_recognized_rules(ruleset, &rules_text, confidence, setup)
        }),
        Err(error) => mutate(&state, |desk| {
            desk.validate_preparation_request(&boundary)?;
            desk.open_manual_rules_draft(&rules_text, setup, error)
        }),
    }
}

#[tauri::command]
pub fn confirm_rules(state: State<AppState>) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.confirm_rules())
}

#[tauri::command]
pub fn edit_topic(state: State<AppState>, topic: String) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.edit_topic(topic))
}

#[tauri::command]
pub fn update_preparation(
    state: State<AppState>,
    mode: String,
    topic: String,
    materials: String,
    user_side: String,
    difficulty: String,
) -> Result<UiSnapshot, String> {
    let setup = crate::application::desk::RuleSetup {
        mode: parse_mode(&mode)?,
        topic,
        materials,
        user_side: parse_side(&user_side),
        difficulty: parse_difficulty(&difficulty)?,
    };
    mutate(&state, |desk| desk.update_preparation(setup))
}

#[tauri::command]
pub async fn analyze_topic(
    state: State<'_, AppState>,
    mode: String,
    topic: String,
    materials: String,
    user_side: String,
    difficulty: String,
) -> Result<UiSnapshot, String> {
    let setup = crate::application::desk::RuleSetup {
        mode: parse_mode(&mode)?,
        topic,
        materials,
        user_side: parse_side(&user_side),
        difficulty: parse_difficulty(&difficulty)?,
    };
    let mut request = None;
    mutate(&state, |desk| {
        if desk.profile_id.is_none() {
            return Err("先建立档案".to_owned());
        }
        desk.adopt_topic_for_analysis(setup)?;
        let (live, config, context, constraint) = desk.prepare_analysis()?;
        let ruleset = desk
            .aggregate
            .as_ref()
            .and_then(|item| item.ruleset.clone());
        let boundary = desk.begin_preparation_request()?;
        request = Some((boundary, live, config, context, constraint, ruleset));
        Ok(())
    })?;
    let (boundary, live, config, context, constraint, ruleset) =
        request.ok_or("无法创建拆题请求")?;
    let (summary, affirmative, negative, packets) =
        tauri::async_runtime::spawn_blocking(move || {
            crate::application::desk::run_topic_analysis(live, config, context, constraint, ruleset)
        })
        .await
        .map_err(|_| "拆题被中断".to_owned())??;
    mutate(&state, |desk| {
        desk.validate_preparation_request(&boundary)?;
        desk.store_analysis(summary, affirmative, negative, packets)
    })
}

#[tauri::command]
pub fn update_schedule(
    state: State<AppState>,
    index: usize,
    speaker: String,
    duration_seconds: u32,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| {
        desk.revise_schedule_row(index, &speaker, duration_seconds)
    })
}

#[tauri::command]
pub fn replace_semantic_rules(
    state: State<AppState>,
    ruleset: crate::domain::ruleset_v2::RulesetV2,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.replace_semantic_rules(ruleset))
}

#[tauri::command]
pub fn ready_match(
    state: State<AppState>,
    user_side: String,
    human_slots: Vec<usize>,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| {
        desk.assign_and_ready(parse_side(&user_side), &human_slots)
    })
}

#[tauri::command]
pub fn submit_calibration(
    state: State<AppState>,
    typed: String,
    elapsed_ms: u64,
    pasted: bool,
) -> Result<UiSnapshot, String> {
    let history = {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        let desk = state.desk.lock().map_err(|_| "会话正忙")?;
        match desk.profile_id {
            Some(profile_id) => store
                .recent_calibration_cpm(profile_id)
                .map_err(|err| err.to_string())?,
            None => Vec::new(),
        }
    };
    let pending = {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        desk.accept_calibration(&typed, elapsed_ms, pasted, &history)?;
        let pending = desk.pending_cpm.take();
        let profile_id = desk.profile_id;
        let id = pending.map(|_| desk.allocate_id());
        (profile_id, id, pending)
    };
    if let (Some(profile_id), Some(id), Some(cpm)) = pending {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        store
            .insert_calibration(id, profile_id, cpm.round() as i64, &utc_stamp_now())
            .map_err(|err| err.to_string())?;
    }
    snapshot(&state)
}

#[tauri::command]
pub fn start_match(state: State<AppState>) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.start())
}

#[tauri::command]
pub fn take_seat(
    state: State<AppState>,
    slot_index: usize,
    second_confirmed: bool,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| {
        claim_current_seat(&state, desk, slot_index, second_confirmed)
    })
}

fn claim_current_seat(
    state: &AppState,
    desk: &mut Desk,
    slot_index: usize,
    second_confirmed: bool,
) -> Result<(), String> {
    switch_current_control_scoped(
        state,
        desk,
        slot_index,
        true,
        second_confirmed,
        crate::domain::control_policy::ControlScope::Match,
    )
}

#[cfg(test)]
fn switch_current_control(
    state: &AppState,
    desk: &mut Desk,
    slot_index: usize,
    to_human: bool,
    second_confirmed: bool,
) -> Result<(), String> {
    switch_current_control_scoped(
        state,
        desk,
        slot_index,
        to_human,
        second_confirmed,
        crate::domain::control_policy::ControlScope::Turn,
    )
}

fn switch_current_control_scoped(
    state: &AppState,
    desk: &mut Desk,
    slot_index: usize,
    to_human: bool,
    second_confirmed: bool,
    scope: crate::domain::control_policy::ControlScope,
) -> Result<(), String> {
    if desk
        .aggregate
        .as_ref()
        .and_then(|aggregate| aggregate.schedule.get(aggregate.cursor))
        .is_some_and(|event| event.kind == crate::domain::ruleset::StageKind::CrossExam)
    {
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        let current = cross.snapshot(desk)?;
        // Prepared text has a known author. Never change its controller after
        // the reveal flow starts and count that AI text as a human answer.
        if current.phase == "revealing" && current.respondent.index() == slot_index {
            return Err("当前回答已进入展示阶段，请在下一轮接管回答辩位".to_owned());
        }
        if matches!(current.phase.as_str(), "done" | "completed") {
            return Err("本轮已经提交，请在下一轮切换控制权".to_owned());
        }
    }
    let previous_revision = desk.turn_revision;
    if scope == crate::domain::control_policy::ControlScope::Match && to_human {
        desk.claim_seat(slot_index, second_confirmed)?;
    } else {
        desk.switch_current_controller_scoped(slot_index, to_human, second_confirmed, scope)?;
    }
    if previous_revision != desk.turn_revision {
        cancel_active_generation(state)?;
    }
    Ok(())
}

fn cancel_active_generation(state: &AppState) -> Result<(), String> {
    if let Some(active) = state.active_stream.lock().map_err(|_| "会话正忙")?.as_ref() {
        active
            .cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_current_controller(
    state: State<'_, AppState>,
    slot_index: usize,
    to_human: bool,
    second_confirmed: bool,
    scope: Option<crate::domain::control_policy::ControlScope>,
) -> Result<UiSnapshot, String> {
    let next = mutate(&state, |desk| {
        switch_current_control_scoped(
            &state,
            desk,
            slot_index,
            to_human,
            second_confirmed,
            scope.unwrap_or_default(),
        )
    })?;
    // A human can type immediately. When handing back to AI, let the cancelled
    // worker release its slot before offering another generation. Never clear
    // another owner's slot or hold desk/store locks during this wait.
    if !to_human {
        for _ in 0..50 {
            if state
                .active_stream
                .lock()
                .map_err(|_| "会话正忙")?
                .is_none()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    Ok(next)
}

#[tauri::command]
pub fn note_wait(state: State<AppState>, waited_ms: u64) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.note_wait(waited_ms))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn submit_turn(
    state: State<AppState>,
    text: String,
    input_ms: u64,
    pause_intervals: Vec<u64>,
    reverse_units: u32,
    reverse_actions: u32,
    pasted: bool,
    events: Option<Vec<crate::domain::telemetry::InputEvent>>,
    claimed: Option<crate::domain::telemetry::ClaimedMetrics>,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| {
        if let (Some(events), Some(claimed)) = (events, claimed) {
            crate::application::human_turn::submit_validated_human_turn(
                desk, text, &events, &claimed,
            )
            .map(|_| ())
        } else {
            // Legacy path for older callers; prefer events+claimed.
            desk.submit(
                text,
                input_ms,
                chargeable_pause(&pause_intervals),
                reverse_units,
                reverse_actions,
                pasted,
            )
        }
    })
}

#[tauri::command]
pub fn set_event_controller(
    state: State<AppState>,
    index: usize,
    to_human: bool,
    second_confirmed: bool,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| {
        desk.set_pending_controller(index, to_human, second_confirmed)
    })
}

#[tauri::command]
pub fn cross_exam_submit_question(
    state: State<AppState>,
    question: String,
    input_ms: Option<u64>,
    events: Option<Vec<crate::domain::telemetry::InputEvent>>,
    claimed: Option<crate::domain::telemetry::ClaimedMetrics>,
) -> Result<UiSnapshot, String> {
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    if !cross.snapshot(&desk)?.questioner_human {
        return Err("当前提问者由 AI 控制，请使用 AI 生成问题".to_owned());
    }
    let validated = match (events, claimed) {
        (Some(events), Some(claimed)) => Some(
            crate::application::human_turn::validate_human_turn_for_desk(
                &desk, &question, &events, &claimed,
            )?,
        ),
        (None, None) => None,
        _ => return Err("输入事件和摘要必须同时提交".to_owned()),
    };
    if validated.is_none() {
        return Err("真人质询问题必须包含可回放输入事件".to_owned());
    }
    let metrics = validated.unwrap();
    let effective_input_ms = metrics.input_elapsed_ms.max(input_ms.unwrap_or(0));
    let exchange_id = cross.submit_question(&mut desk, question, effective_input_ms)?;
    desk.record_cross_exam_question_metrics(
        exchange_id,
        effective_input_ms,
        metrics.pause_ms,
        metrics.reverse_edited_units,
        metrics.reverse_edit_actions,
        metrics.paste_units > 0,
    )?;
    desk.apply_last_validated_input_counts(
        metrics.forward_typed_units,
        metrics.effective_positive_units,
    );
    if metrics.unverified_input {
        desk.note_unverified_input();
    }
    drop(cross);
    drop(desk);
    persist_current_match(&state)?;
    snapshot(&state)
}

#[tauri::command]
pub fn cross_exam_snapshot(state: State<AppState>) -> Result<CrossExamSnapshot, String> {
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    cross.snapshot(&desk)
}

#[tauri::command]
pub fn cross_exam_insert_human_questioner(
    state: State<AppState>,
    slot_index: usize,
) -> Result<CrossExamSnapshot, String> {
    let slot = crate::domain::ruleset::SlotId::from_index(slot_index).ok_or("辩位无效")?;
    let snapshot = {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        cross.insert_human_questioner(&mut desk, slot)?
    };
    persist_current_match(&state)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn cross_exam_set_respondent_controller(
    state: State<AppState>,
    human: bool,
) -> Result<CrossExamSnapshot, String> {
    let snapshot = {
        let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
        cross.set_respondent_controller(&mut desk, human)?
    };
    persist_current_match(&state)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn cross_exam_prepare_answer(
    state: State<AppState>,
    full_text: String,
    events: Option<Vec<crate::domain::telemetry::InputEvent>>,
    claimed: Option<crate::domain::telemetry::ClaimedMetrics>,
) -> Result<CrossExamSnapshot, String> {
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    if !cross.snapshot(&desk)?.respondent_human {
        return Err("当前回答者由 AI 控制，请使用 AI 准备回答".to_owned());
    }
    let unverified_input;
    let metrics = match (events, claimed) {
        (Some(events), Some(claimed)) => {
            let validated = crate::application::human_turn::validate_human_turn_for_desk(
                &desk, &full_text, &events, &claimed,
            )?;
            unverified_input = validated.unverified_input;
            Some(HumanAnswerMetrics {
                input_ms: validated.input_elapsed_ms,
                pause_ms: validated.pause_ms,
                reverse_units: validated.reverse_edited_units,
                reverse_actions: validated.reverse_edit_actions,
                pasted: validated.paste_units > 0,
                input_counts: Some((
                    validated.forward_typed_units,
                    validated.effective_positive_units,
                )),
            })
        }
        (None, None) => return Err("真人质询回答必须包含可回放输入事件".to_owned()),
        _ => return Err("输入事件和摘要必须同时提交".to_owned()),
    };
    cross.prepare_answer_with_metrics(&desk, full_text, metrics)?;
    if unverified_input {
        desk.note_unverified_input();
    }
    cross.snapshot(&desk)
}

#[tauri::command]
pub fn cross_exam_reveal(
    state: State<AppState>,
    units: u32,
    elapsed_ms: u64,
    event_id: Option<String>,
    round_number: Option<u32>,
) -> Result<CrossExamSnapshot, String> {
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    require_cross_scope(&cross.snapshot(&desk)?, event_id.as_deref(), round_number)?;
    cross.reveal(units, elapsed_ms)?;
    cross.snapshot(&desk)
}

#[tauri::command]
pub fn cross_exam_visible_prefix(state: State<AppState>) -> Result<String, String> {
    let cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    Ok(cross.visible_for_interrupt_decision())
}

#[tauri::command]
pub fn cross_exam_interrupt(
    state: State<AppState>,
    reason: String,
    human_input_ms: Option<u64>,
    displayed_units: Option<u32>,
    event_id: Option<String>,
    round_number: Option<u32>,
) -> Result<UiSnapshot, String> {
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    let current = cross.snapshot(&desk)?;
    require_cross_scope(&current, event_id.as_deref(), round_number)?;
    limit_interrupt_prefix(&mut cross, &current, displayed_units)?;
    cross.interrupt_and_commit_with_input(
        &mut desk,
        reason,
        Some(HumanAnswerMetrics {
            input_ms: human_input_ms.unwrap_or(0),
            ..HumanAnswerMetrics::default()
        }),
    )?;
    drop(cross);
    drop(desk);
    persist_current_match(&state)?;
    snapshot(&state)
}

fn require_cross_scope(
    current: &CrossExamSnapshot,
    event_id: Option<&str>,
    round: Option<u32>,
) -> Result<(), String> {
    match (event_id, round) {
        (None, None) => Ok(()), // Legacy callers; new UI always supplies both.
        (Some(event), Some(round))
            if event == current.event_id && round == current.round_number =>
        {
            Ok(())
        }
        _ => Err("质询轮次已经变化，已丢弃过期展示或打断请求".to_owned()),
    }
}

fn limit_interrupt_prefix(
    cross: &mut crate::application::cross_exam_service::CrossExamService,
    current: &CrossExamSnapshot,
    displayed: Option<u32>,
) -> Result<(), String> {
    if let Some(units) = displayed {
        if !current.interruptible
            || current.phase != "revealing"
            || units == 0
            || units > current.revealed_units
        {
            return Err("只能打断本轮已确认展示的前缀".to_owned());
        }
        // IPC may already have advanced the backend beyond the last prefix
        // actually painted by the client. Never charge that unseen suffix.
        cross.reveal(units, cross.state.reveal_elapsed_ms)?;
    }
    Ok(())
}

#[tauri::command]
pub fn cross_exam_complete(
    state: State<AppState>,
    human_input_ms: u64,
) -> Result<UiSnapshot, String> {
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    cross.complete_and_commit(
        &mut desk,
        Some(HumanAnswerMetrics {
            input_ms: human_input_ms,
            ..HumanAnswerMetrics::default()
        }),
    )?;
    drop(cross);
    drop(desk);
    persist_current_match(&state)?;
    snapshot(&state)
}

#[tauri::command]
pub fn cross_exam_timeout(state: State<AppState>) -> Result<UiSnapshot, String> {
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    cross.timeout_session(&mut desk)?;
    drop(cross);
    drop(desk);
    persist_current_match(&state)?;
    snapshot(&state)
}

#[tauri::command]
pub fn cross_exam_end_session(state: State<AppState>) -> Result<UiSnapshot, String> {
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let mut cross = state.cross_exam.lock().map_err(|_| "会话正忙")?;
    cross.end_session(&mut desk)?;
    drop(cross);
    drop(desk);
    persist_current_match(&state)?;
    snapshot(&state)
}

#[tauri::command]
pub fn waive_free_debate_time(state: State<AppState>) -> Result<UiSnapshot, String> {
    mutate(&state, Desk::waive_free_debate_time)
}

#[tauri::command]
pub fn interrupt_turn(
    state: State<AppState>,
    text: String,
    displayed_units: u32,
) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.interrupt_shown(text, displayed_units))
}

#[tauri::command]
pub fn finish_match(state: State<AppState>) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.finish(true))
}

#[tauri::command]
pub fn export_report(state: State<AppState>) -> Result<String, String> {
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    Ok(desk.export())
}

#[tauri::command]
pub fn mark_recovery(state: State<AppState>) -> Result<UiSnapshot, String> {
    mutate(&state, |desk| desk.mark_recovery())
}

fn parse_mode(value: &str) -> Result<MatchMode, String> {
    match value {
        "scoring" => Ok(MatchMode::Scoring),
        "training" => Ok(MatchMode::Training),
        _ => Err("模式无效".to_owned()),
    }
}

fn parse_side(value: &str) -> Option<Side> {
    match value {
        "affirmative" => Some(Side::Affirmative),
        "negative" => Some(Side::Negative),
        _ => None,
    }
}

fn parse_difficulty(value: &str) -> Result<Difficulty, String> {
    match value {
        "beginner" => Ok(Difficulty::Beginner),
        "contest" => Ok(Difficulty::Contest),
        "standard" => Ok(Difficulty::Standard),
        _ => Err("难度无效".to_owned()),
    }
}

#[cfg(test)]
mod persistence_tests {
    use std::sync::Mutex;

    use crate::application::cross_exam_service::CrossExamService;
    use crate::application::desk::Desk;
    use crate::application::AppState;
    use crate::domain::ruleset::{MatchMode, Side};
    use crate::domain::visibility::Difficulty;
    use crate::infrastructure::db::SqliteStore;
    use uuid::Uuid;

    fn state_with_opening() -> AppState {
        let store = SqliteStore::open(":memory:").unwrap();
        let profile_id = Uuid::from_u128(7401);
        store
            .create_profile(profile_id, "存档并发测试", "2026-10-07T00:00:00Z")
            .unwrap();
        let mut desk = Desk::new();
        desk.profile_id = Some(profile_id);
        desk.open_template(
            MatchMode::Training,
            "人工智能应当受到限制".to_owned(),
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
        AppState {
            store: Mutex::new(store),
            desk: Mutex::new(desk),
            active_stream: Mutex::new(None),
            cross_exam: Mutex::new(CrossExamService::default()),
        }
    }

    #[test]
    fn preparation_new_profile_never_adopts_or_reassigns_the_previous_match() {
        let state = state_with_opening();
        let (previous_profile, previous_match, boundary) = {
            let mut desk = state.desk.lock().unwrap();
            desk.aggregate.as_mut().unwrap().phase = crate::domain::match_engine::MatchPhase::Ready;
            let boundary = desk.begin_preparation_request().unwrap();
            (
                desk.profile_id.unwrap(),
                desk.aggregate.as_ref().unwrap().id,
                boundary,
            )
        };
        super::persist_current_match(&state).unwrap();
        super::create_profile_in_state(&state, "新档案不能挪走旧比赛").unwrap();
        let created_profile = {
            let desk = state.desk.lock().unwrap();
            assert!(desk.aggregate.is_none());
            assert!(desk.validate_preparation_request(&boundary).is_err());
            assert!(desk.strategies_ref().is_none());
            desk.profile_id.unwrap()
        };
        assert_ne!(created_profile, previous_profile);
        super::persist_current_match(&state).unwrap();
        let store = state.store.lock().unwrap();
        let previous_history = store.recent_matches(previous_profile, 50).unwrap();
        assert_eq!(previous_history.len(), 1);
        assert_eq!(previous_history[0].id, previous_match);
        assert!(store
            .recent_matches(created_profile, 50)
            .unwrap()
            .is_empty());
        assert_eq!(
            store.load_match(previous_match).unwrap().phase,
            crate::domain::match_engine::MatchPhase::Ready
        );
    }

    #[test]
    fn mutation_reserves_storage_until_the_changed_match_is_saved() {
        let state = state_with_opening();
        // A second writer must not be able to publish a newer snapshot between
        // this action and its save. Probe the real command's reservation here.
        let result = super::mutate(&state, |desk| {
            assert!(
                state.store.try_lock().is_err(),
                "storage was released before changing the match; an older snapshot can save last"
            );
            desk.submit("存档不得丢失的正文".to_owned(), 500, 0, 0, 0, false)
        });
        result.unwrap();
        let desk = state.desk.lock().unwrap();
        let expected = desk.aggregate.as_ref().unwrap();
        let restored = state.store.lock().unwrap().load_match(expected.id).unwrap();
        assert_eq!(restored.transcripts, expected.transcripts);
        assert_eq!(
            restored.remaining_main_units(Side::Affirmative),
            expected.remaining_main_units(Side::Affirmative)
        );
    }

    #[test]
    fn delayed_persist_after_takeover_saves_the_latest_transcript_and_word_balance() {
        use crate::domain::events::Controller;
        use std::sync::{mpsc, Arc};

        let state = Arc::new(state_with_opening());
        {
            let mut desk = state.desk.lock().unwrap();
            desk.submit("先完成的正方陈词".to_owned(), 500, 0, 0, 0, false)
                .unwrap();
        }
        let slot_index = {
            let desk = state.desk.lock().unwrap();
            let aggregate = desk.aggregate.as_ref().unwrap();
            aggregate.current_cross_exam_roles().unwrap().0.index()
        };
        let (start_send, start_receive) = mpsc::channel();
        let (entered_send, entered_receive) = mpsc::channel();
        let pending_state = Arc::clone(&state);
        let pending_save = std::thread::spawn(move || {
            start_receive.recv().unwrap();
            entered_send.send(()).unwrap();
            super::persist_current_match(&pending_state)
        });
        // Force the older caller to request persistence while the newer action
        // owns both locks. It must read the desk only after this action saves.
        super::mutate(&state, |desk| {
            desk.claim_seat(slot_index, false)?;
            start_send.send(()).unwrap();
            entered_receive.recv().unwrap();
            assert!(state.store.try_lock().is_err());
            Ok(())
        })
        .unwrap();
        pending_save.join().unwrap().unwrap();
        let desk = state.desk.lock().unwrap();
        let expected = desk.aggregate.as_ref().unwrap();
        let restored = state.store.lock().unwrap().load_match(expected.id).unwrap();
        assert_eq!(
            restored.assignments[slot_index].controller,
            Controller::Human
        );
        assert_eq!(restored.transcripts, expected.transcripts);
        assert_eq!(restored.word_budgets, expected.word_budgets);
        assert_eq!(restored.cursor, expected.cursor);
    }

    #[test]
    fn respondent_takeover_during_prepared_reveal_cannot_relabel_ai_text_as_human() {
        use crate::domain::events::Controller;
        let state = state_with_opening();
        let respondent = {
            let mut desk = state.desk.lock().unwrap();
            desk.submit("正方陈词".to_owned(), 500, 0, 0, 0, false)
                .unwrap();
            let mut cross = state.cross_exam.lock().unwrap();
            cross
                .submit_question(&mut desk, "问题".to_owned(), 500)
                .unwrap();
            cross
                .prepare_answer_with_metrics(&desk, "由AI准备的回答正文".to_owned(), None)
                .unwrap();
            cross.reveal(1, 500).unwrap();
            cross.snapshot(&desk).unwrap().respondent
        };
        let result = super::mutate(&state, |desk| {
            super::claim_current_seat(&state, desk, respondent.index(), false)
        });
        assert!(
            result.is_err(),
            "prepared AI text must never be relabelled as a human submission"
        );
        let desk = state.desk.lock().unwrap();
        let mut cross = state.cross_exam.lock().unwrap();
        let snapshot = cross.snapshot(&desk).unwrap();
        assert!(!snapshot.respondent_human);
        assert_eq!(
            desk.aggregate.as_ref().unwrap().assignments[respondent.index()].controller,
            Controller::Ai
        );
        assert_eq!(snapshot.revealed_units, 1);
    }

    #[test]
    fn cancelled_ordinary_candidate_cannot_charge_words_when_its_turn_still_matches() {
        use std::sync::atomic::AtomicBool;
        let state = state_with_opening();
        let before = state.desk.lock().unwrap().aggregate.clone().unwrap();
        let revision = state.desk.lock().unwrap().turn_revision;
        let event = &before.schedule[before.cursor];
        let result = super::mutate_submit_ai(
            &state,
            "已经取消的候选正文".to_owned(),
            7000,
            &before.id.to_string(),
            &event.id.to_string(),
            event.speaker,
            revision,
            &AtomicBool::new(true),
        );
        assert!(
            result.is_err(),
            "a cancelled candidate must not commit merely because its seat still matches"
        );
        let desk = state.desk.lock().unwrap();
        let after = desk.aggregate.as_ref().unwrap();
        assert_eq!(after.transcripts, before.transcripts);
        assert_eq!(after.word_budgets, before.word_budgets);
        assert_eq!(after.ai_wait_ms, before.ai_wait_ms);
    }

    #[test]
    fn ai_human_ai_roundtrip_revokes_the_original_candidate_without_charging() {
        use std::sync::atomic::AtomicBool;
        let state = state_with_opening();
        let before = state.desk.lock().unwrap().aggregate.clone().unwrap();
        let revision = state.desk.lock().unwrap().turn_revision;
        let event = &before.schedule[before.cursor];
        {
            let mut desk = state.desk.lock().unwrap();
            desk.switch_current_controller(event.speaker.index(), true, false)
                .unwrap();
            desk.switch_current_controller(event.speaker.index(), false, false)
                .unwrap();
        }
        let result = super::mutate_submit_ai(
            &state,
            "过期但辩位再次是AI的候选".to_owned(),
            7000,
            &before.id.to_string(),
            &event.id.to_string(),
            event.speaker,
            revision,
            &AtomicBool::new(false),
        );
        assert!(
            result.is_err(),
            "AI→人工→AI must not revive the old request"
        );
        let desk = state.desk.lock().unwrap();
        let after = desk.aggregate.as_ref().unwrap();
        assert_eq!(after.transcripts, before.transcripts);
        assert_eq!(after.word_budgets, before.word_budgets);
        assert_eq!(after.ai_wait_ms, before.ai_wait_ms);
    }

    #[test]
    fn accepted_control_switch_cancels_owner_and_noop_preserves_revision() {
        use crate::application::ActiveStream;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let state = state_with_opening();
        let cancel = Arc::new(AtomicBool::new(false));
        *state.active_stream.lock().unwrap() = Some(ActiveStream {
            cancel: Arc::clone(&cancel),
            request_id: "silent-request".to_owned(),
        });
        let mut desk = state.desk.lock().unwrap();
        let slot = desk.aggregate.as_ref().unwrap().schedule[desk.view().cursor]
            .speaker
            .index();
        super::switch_current_control(&state, &mut desk, slot, true, false).unwrap();
        assert!(cancel.load(Ordering::SeqCst));
        let revision = desk.turn_revision;
        super::switch_current_control(&state, &mut desk, slot, true, false).unwrap();
        assert_eq!(desk.turn_revision, revision);
    }

    #[test]
    fn interrupt_while_ipc_pending_commits_only_client_acknowledged_prefix() {
        let state = state_with_opening();
        let mut desk = state.desk.lock().unwrap();
        desk.submit("开篇".to_owned(), 500, 0, 0, 0, false).unwrap();
        let mut cross = state.cross_exam.lock().unwrap();
        cross
            .submit_question(&mut desk, "问题".to_owned(), 500)
            .unwrap();
        cross
            .prepare_answer_with_metrics(&desk, "已经看到尚未看到".to_owned(), None)
            .unwrap();
        cross.reveal(8, 1000).unwrap();
        let current = cross.snapshot(&desk).unwrap();
        let before = desk.aggregate.as_ref().unwrap().current_remaining_units();
        assert!(super::require_cross_scope(
            &current,
            Some("another-event"),
            Some(current.round_number)
        )
        .is_err());
        assert!(super::limit_interrupt_prefix(&mut cross, &current, Some(9)).is_err());
        assert_eq!(cross.snapshot(&desk).unwrap().revealed_units, 8);
        super::limit_interrupt_prefix(&mut cross, &current, Some(4)).unwrap();
        cross
            .interrupt_and_commit_with_input(&mut desk, "人工打断".to_owned(), None)
            .unwrap();
        let aggregate = desk.aggregate.as_ref().unwrap();
        assert_eq!(
            aggregate.transcripts.last().unwrap().effective_text,
            "已经看到"
        );
        assert_eq!(aggregate.current_remaining_units(), before - 4);
    }

    #[test]
    fn over_budget_ai_candidate_is_rejected_without_any_public_fragment() {
        use std::sync::atomic::AtomicBool;
        let state = state_with_opening();
        let before = state.desk.lock().unwrap().aggregate.clone().unwrap();
        let revision = state.desk.lock().unwrap().turn_revision;
        let event = &before.schedule[before.cursor];
        let text = "字".repeat(before.current_word_cap() as usize + 1);
        let result = super::mutate_submit_ai(
            &state,
            text,
            7000,
            &before.id.to_string(),
            &event.id.to_string(),
            event.speaker,
            revision,
            &AtomicBool::new(false),
        );
        assert!(
            result.is_err(),
            "AI must never commit a mechanically clipped speech"
        );
        let desk = state.desk.lock().unwrap();
        let after = desk.aggregate.as_ref().unwrap();
        assert_eq!(after.transcripts, before.transcripts);
        assert_eq!(after.word_budgets, before.word_budgets);
        assert_eq!(after.ai_wait_ms, before.ai_wait_ms);
    }

    #[test]
    fn restoring_the_same_match_cancels_its_still_running_generation() {
        use crate::application::ActiveStream;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let state = state_with_opening();
        super::persist_current_match(&state).unwrap();
        let match_id = state.desk.lock().unwrap().aggregate.as_ref().unwrap().id;
        let cancel = Arc::new(AtomicBool::new(false));
        *state.active_stream.lock().unwrap() = Some(ActiveStream {
            cancel: Arc::clone(&cancel),
            request_id: "pending-before-resume".to_owned(),
        });
        super::resume_saved_match(&state, match_id).unwrap();
        assert!(
            cancel.load(Ordering::SeqCst),
            "restoring the same event must revoke pending candidates even if all saved IDs match"
        );
        assert!(
            state.active_stream.lock().unwrap().is_some(),
            "the old worker remains owner until it exits"
        );
        assert!(state
            .desk
            .lock()
            .unwrap()
            .aggregate
            .as_ref()
            .unwrap()
            .transcripts
            .is_empty());
    }
}
