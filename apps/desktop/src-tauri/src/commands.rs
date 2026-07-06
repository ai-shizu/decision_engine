use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::engine::EngineManager;

#[tauri::command]
pub async fn pkb_invoke(
    manager: State<'_, Arc<EngineManager>>,
    cmd: String,
    params: Option<Value>,
) -> Result<Value, String> {
    manager
        .invoke(&cmd, params.unwrap_or(Value::Object(Default::default())))
        .await
}

#[tauri::command]
pub fn pkb_engine_ready(manager: State<'_, Arc<EngineManager>>) -> bool {
    manager.is_ready()
}
