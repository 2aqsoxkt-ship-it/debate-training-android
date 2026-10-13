use tauri::{AppHandle, Manager};

use crate::application::desk::utc_stamp_now;
use crate::application::model_settings::{
    apply_model_fields, clear_api_key_only, has_api_key, live_allowed, mark_not_configured,
    mark_probe_result, mark_untested, save_api_key_only, ConnectionProbe, DeepSeekConnectionProbe,
    ModelSettingsDto,
};
use crate::application::AppState;
use crate::infrastructure::ai::ModelConfig;
use crate::infrastructure::credentials::{KeyringSecretStore, SecretStore};

async fn in_background(
    app: AppHandle,
    operation: impl FnOnce(&AppState) -> Result<ModelSettingsDto, String> + Send + 'static,
) -> Result<ModelSettingsDto, String> {
    tauri::async_runtime::spawn_blocking(move || operation(&app.state::<AppState>()))
        .await
        .map_err(|_| "模型设置操作被中断".to_owned())?
}

fn revoke_active_request(state: &AppState) {
    if let Ok(active) = state.active_stream.lock() {
        if let Some(active) = active.as_ref() {
            active
                .cancel
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

fn load_settings_dto(state: &AppState) -> Result<ModelSettingsDto, String> {
    let settings = {
        let store = state.store.lock().map_err(|_| "数据库正忙")?;
        store.load_settings().map_err(|err| err.to_string())?
    };
    let secrets = KeyringSecretStore::app();
    Ok(ModelSettingsDto::from_parts(
        &settings,
        has_api_key(&secrets),
    ))
}

fn sync_desk_live(state: &AppState, settings: &crate::infrastructure::db::AppSettings) {
    if let Ok(mut desk) = state.desk.lock() {
        desk.model_config = ModelConfig::from_settings(settings);
        let status = crate::application::model_settings::ConnectionStatus::parse(
            &settings.connection_status,
        )
        .unwrap_or(crate::application::model_settings::ConnectionStatus::NotConfigured);
        desk.live = live_allowed(status) && has_api_key(&KeyringSecretStore::app());
        if desk.live {
            desk.model_config.timeout_ms = 120_000;
        }
    }
}

#[tauri::command]
pub async fn get_model_settings(app: AppHandle) -> Result<ModelSettingsDto, String> {
    in_background(app, load_settings_dto).await
}

#[tauri::command]
pub async fn save_api_key(app: AppHandle, api_key: String) -> Result<ModelSettingsDto, String> {
    in_background(app, move |state| {
        revoke_active_request(state);
        let secrets = KeyringSecretStore::app();
        save_api_key_only(&secrets, &api_key).map_err(|err| err.to_string())?;
        let mut settings = {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store.load_settings().map_err(|err| err.to_string())?
        };
        mark_untested(&mut settings);
        {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store
                .save_settings(&settings, &utc_stamp_now())
                .map_err(|err| err.to_string())?;
        }
        sync_desk_live(state, &settings);
        load_settings_dto(state)
    })
    .await
}

#[tauri::command]
pub async fn clear_api_key(app: AppHandle) -> Result<ModelSettingsDto, String> {
    in_background(app, |state| {
        revoke_active_request(state);
        let secrets = KeyringSecretStore::app();
        clear_api_key_only(&secrets).map_err(|err| err.to_string())?;
        let mut settings = {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store.load_settings().map_err(|err| err.to_string())?
        };
        mark_not_configured(&mut settings);
        {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store
                .save_settings(&settings, &utc_stamp_now())
                .map_err(|err| err.to_string())?;
        }
        sync_desk_live(state, &settings);
        load_settings_dto(state)
    })
    .await
}

#[tauri::command]
pub async fn update_model_settings(
    app: AppHandle,
    model_id: Option<String>,
    base_url: Option<String>,
    reasoning_level: Option<String>,
) -> Result<ModelSettingsDto, String> {
    in_background(app, move |state| {
        revoke_active_request(state);
        let mut settings = {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store.load_settings().map_err(|err| err.to_string())?
        };
        apply_model_fields(&mut settings, model_id, base_url, reasoning_level)
            .map_err(|err| err.to_string())?;
        if has_api_key(&KeyringSecretStore::app())
            && settings.connection_status
                == crate::application::model_settings::ConnectionStatus::Ready.as_str()
        {
            mark_untested(&mut settings);
        }
        {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store
                .save_settings(&settings, &utc_stamp_now())
                .map_err(|err| err.to_string())?;
        }
        sync_desk_live(state, &settings);
        load_settings_dto(state)
    })
    .await
}

#[tauri::command]
pub async fn test_model_connection(app: AppHandle) -> Result<ModelSettingsDto, String> {
    in_background(app, |state| {
        let secrets = KeyringSecretStore::app();
        let api_key = secrets
            .read_api_key()
            .map_err(|_| "还没有可用的 DeepSeek 密钥".to_owned())?;
        let mut settings = {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store.load_settings().map_err(|err| err.to_string())?
        };
        settings.connection_status = crate::application::model_settings::ConnectionStatus::Testing
            .as_str()
            .to_owned();
        {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store
                .save_settings(&settings, &utc_stamp_now())
                .map_err(|err| err.to_string())?;
        }
        let config = ModelConfig::from_settings(&settings);
        let probe = DeepSeekConnectionProbe;
        let result = probe.probe(&config, &api_key);
        mark_probe_result(&mut settings, result, &utc_stamp_now());
        {
            let store = state.store.lock().map_err(|_| "数据库正忙")?;
            store
                .save_settings(&settings, &utc_stamp_now())
                .map_err(|err| err.to_string())?;
        }
        sync_desk_live(state, &settings);
        load_settings_dto(state)
    })
    .await
}
