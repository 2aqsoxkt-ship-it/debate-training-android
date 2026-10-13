mod application;
mod commands;
mod config;
pub mod domain;
mod error;
pub mod infrastructure;
mod platform;

use application::desk::Desk;
use application::AppState;
use infrastructure::logging;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    error::verify_error_code_contract().expect("error code contract");
    let mut sample = serde_json::json!({ "topic": "local" });
    logging::redact_json(&mut sample);
    if sample["topic"] != "local" {
        panic!("redaction changed public text");
    }
    tracing::info!(version = config::app_version(), "shell ready");

    tauri::Builder::default()
        .setup(|app| {
            let default_dir = app
                .path()
                .app_data_dir()
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            let dir = if std::env::var_os("DEBATE_ACCEPTANCE_LEDGER").is_some() {
                std::env::var_os("DEBATE_ACCEPTANCE_DATA_DIR")
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| std::io::Error::other("验收必须提供独立数据目录"))?
            } else {
                default_dir
            };
            std::fs::create_dir_all(&dir)?;
            let store = infrastructure::db::SqliteStore::open(dir.join("debate.sqlite"))?;
            let settings = store.load_settings().unwrap_or_default();
            let secrets = infrastructure::credentials::KeyringSecretStore::app();
            let dto = application::model_settings::ModelSettingsDto::from_parts(
                &settings,
                application::model_settings::has_api_key(&secrets),
            );
            let mut desk = Desk::new();
            desk.model_config = infrastructure::ai::ModelConfig::from_settings(&settings);
            desk.live =
                application::model_settings::live_allowed(dto.connection_status) && dto.has_api_key;
            if desk.live {
                desk.model_config.timeout_ms = 120_000;
            }
            app.manage(AppState {
                store: std::sync::Mutex::new(store),
                desk: std::sync::Mutex::new(desk),
                active_stream: std::sync::Mutex::new(None),
                cross_exam: std::sync::Mutex::new(
                    application::cross_exam_service::CrossExamService::default(),
                ),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::mobile::mobile_platform_snapshot,
            commands::mobile::mobile_native_self_test,
            commands::workspace_snapshot,
            commands::experience::load_draft,
            commands::experience::save_draft,
            commands::experience::create_practice,
            commands::experience::load_practice,
            commands::experience::save_practice,
            commands::experience::review_practice,
            commands::create_profile,
            commands::select_profile,
            commands::resume_match,
            commands::abort_match,
            commands::release_completed_match,
            commands::rename_profile,
            commands::delete_profile,
            commands::open_template,
            commands::recognize_rules,
            commands::confirm_rules,
            commands::edit_topic,
            commands::update_preparation,
            commands::analyze_topic,
            commands::update_schedule,
            commands::replace_semantic_rules,
            commands::ready_match,
            commands::submit_calibration,
            commands::start_match,
            commands::take_seat,
            commands::note_wait,
            commands::submit_turn,
            commands::set_event_controller,
            commands::cross_exam_submit_question,
            commands::cross_exam_snapshot,
            commands::cross_exam_insert_human_questioner,
            commands::cross_exam_set_respondent_controller,
            commands::set_current_controller,
            commands::cross_exam_ai::cross_exam_generate_question,
            commands::cross_exam_ai::cross_exam_generate_answer,
            commands::cross_exam_prepare_answer,
            commands::cross_exam_reveal,
            commands::cross_exam_visible_prefix,
            commands::cross_exam_interrupt,
            commands::cross_exam_complete,
            commands::cross_exam_timeout,
            commands::cross_exam_end_session,
            commands::waive_free_debate_time,
            commands::stream::advance_ai,
            commands::stream::cancel_ai_stream,
            commands::interrupt_turn,
            commands::finish_match,
            commands::export_report,
            commands::mark_recovery,
            commands::settings::get_model_settings,
            commands::settings::save_api_key,
            commands::settings::test_model_connection,
            commands::settings::clear_api_key,
            commands::settings::update_model_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
