use crate::application::{desk::utc_stamp_now, AppState};
use crate::infrastructure::db::sqlite::experience::{
    DraftKey, DraftPayload, PracticeRecord, SavedDraft,
};
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub fn load_draft(key: DraftKey, state: State<'_, AppState>) -> Result<Option<SavedDraft>, String> {
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let profile = desk.profile_id.ok_or("请先选择档案")?;
    store.load_draft(profile, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_draft(
    key: DraftKey,
    revision: u64,
    payload: DraftPayload,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let profile = desk.profile_id.ok_or("请先选择档案")?;
    if desk
        .aggregate
        .as_ref()
        .is_none_or(|a| a.id != key.match_id || a.control_window_id() != key.window_id)
    {
        return Ok(false);
    }
    store
        .save_draft(profile, &key, revision, &payload, &utc_stamp_now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_practice(
    match_id: Uuid,
    source_turn_id: Uuid,
    state: State<'_, AppState>,
) -> Result<PracticeRecord, String> {
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let mut desk = state.desk.lock().map_err(|_| "会话正忙")?;
    let profile = desk.profile_id.ok_or("请先选择档案")?;
    let id = desk.allocate_id();
    store
        .create_practice(profile, match_id, source_turn_id, id, &utc_stamp_now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_practice(id: Uuid, state: State<'_, AppState>) -> Result<PracticeRecord, String> {
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    store
        .load_practice(desk.profile_id.ok_or("请先选择档案")?, id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_practice(
    id: Uuid,
    text: String,
    revision: u64,
    state: State<'_, AppState>,
) -> Result<PracticeRecord, String> {
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    store
        .save_practice(
            desk.profile_id.ok_or("请先选择档案")?,
            id,
            &text,
            revision,
            None,
            &utc_stamp_now(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn review_practice(
    id: Uuid,
    revision: u64,
    state: State<'_, AppState>,
) -> Result<PracticeRecord, String> {
    let (profile, record, config) = {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        let desk = state.desk.lock().map_err(|_| "会话正忙")?;
        let profile = desk.profile_id.ok_or("请先选择档案")?;
        let settings = store.load_settings().map_err(|e| e.to_string())?;
        if !crate::application::model_settings::ConnectionStatus::parse(&settings.connection_status)
            .is_some_and(crate::application::model_settings::live_allowed)
        {
            return Err("请先验证 DeepSeek 连接".to_owned());
        }
        let record = store
            .load_practice(profile, id)
            .map_err(|e| e.to_string())?;
        if record.revision != revision || record.rewritten_text.trim().is_empty() {
            return Err("先保存当前重练稿，再请求反馈".to_owned());
        }
        (
            profile,
            record,
            crate::infrastructure::ai::ModelConfig::from_settings(&settings),
        )
    };
    let context = crate::application::practice_service::feedback_context(&record);
    let cancel = super::stream::try_bind_stream(&state, &format!("practice-{id}"))?;
    let worker_cancel = std::sync::Arc::clone(&cancel);
    let feedback = tauri::async_runtime::spawn_blocking(move || {
        use crate::infrastructure::credentials::SecretStore;
        let key = crate::infrastructure::credentials::KeyringSecretStore::app()
            .read_api_key()
            .map_err(|e| e.to_string())?;
        let transport = crate::infrastructure::ai::DeepSeekTransport::new(
            crate::infrastructure::ai::UreqHttp,
            config.clone(),
            key,
        );
        let gateway = crate::infrastructure::ai::Gateway::new(
            transport,
            crate::infrastructure::ai::ThreadSleep,
            config,
        );
        let feedback = gateway
            .score_stage(&context, &worker_cancel)
            .map_err(|e| e.to_string())?;
        if !feedback.transcript_ids.contains(&id) {
            return Err("练习反馈没有引用当前练习".to_owned());
        }
        serde_json::to_string(&feedback).map_err(|e| e.to_string())
    })
    .await;
    super::stream::clear_stream(&state, &format!("practice-{id}"));
    let feedback = feedback.map_err(|_| "练习反馈线程退出")??;
    let store = state.store.lock().map_err(|_| "数据库正忙")?;
    let desk = state.desk.lock().map_err(|_| "会话正忙")?;
    if desk.profile_id != Some(profile) || cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("练习反馈已取消或档案已切换".to_owned());
    }
    store
        .save_practice(
            profile,
            id,
            &record.rewritten_text,
            revision,
            Some(&feedback),
            &utc_stamp_now(),
        )
        .map_err(|e| e.to_string())
}
