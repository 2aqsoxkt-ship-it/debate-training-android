use serde_json::Value;

#[tauri::command]
pub async fn mobile_platform_snapshot() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let json = crate::platform::call("capabilities", "")?;
        serde_json::from_str(&json).map_err(|_| "安卓系统状态解析失败".to_owned())
    })
    .await
    .map_err(|_| "安卓状态读取被中断".to_owned())?
}

#[tauri::command]
pub async fn mobile_native_self_test() -> Result<Value, String> {
    if !cfg!(debug_assertions) {
        return Err("安全存储自检仅在调试验收包启用".to_owned());
    }
    tauri::async_runtime::spawn_blocking(|| {
        let native = crate::platform::call("selfTest", "")?;
        let store = crate::infrastructure::db::SqliteStore::open(":memory:")
            .map_err(|_| "SQLite自检失败")?;
        let rules = crate::domain::ruleset_v2::traditional_four_on_four_v2();
        Ok(serde_json::json!({
            "keystore": native,
            "sqliteVersion": store.schema_version().map_err(|_| "SQLite版本读取失败")?,
            "seats": crate::domain::ruleset::SlotId::ALL.len(),
            "stages": rules.stages.len(),
            "effectiveUnits": crate::domain::text_units::count_effective_units("安卓中文测试"),
        }))
    })
    .await
    .map_err(|_| "安卓原生自检被中断".to_owned())?
}
