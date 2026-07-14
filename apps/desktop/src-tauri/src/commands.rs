use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::State;

use crate::engine::EngineManager;
use crate::ipc_contract::{
    CalendarAppleRequest, CalendarIcsRequest, ConsultRequest, ImportClassifyRequest,
    ImportDocumentRequest, ImportLineBatchRequest, ImportLineSingleRequest,
    NarrativeCompileRequest, ProbeAnswerRequest, ProbeDateRequest, RecordLoadRequest,
    RecordSaveRequest, ScopeRequest, SettingsSaveFixedRequest, TwinForecastRequest,
    ValidateRequest,
};

fn request_params<T: Serialize + ValidateRequest>(request: T) -> Result<Value, String> {
    request.validate()?;
    serde_json::to_value(request).map_err(|_| "invalid renderer request".to_string())
}

async fn invoke_request<T: Serialize + ValidateRequest>(
    manager: State<'_, Arc<EngineManager>>,
    command: &'static str,
    request: T,
    cid: Option<u64>,
) -> Result<Value, String> {
    let params = request_params(request)?;
    manager.invoke(command, params, cid).await
}

async fn invoke_request_with_fixed_field<T: Serialize + ValidateRequest>(
    manager: State<'_, Arc<EngineManager>>,
    command: &'static str,
    request: T,
    cid: Option<u64>,
    key: &'static str,
    value: &'static str,
) -> Result<Value, String> {
    let mut params = request_params(request)?;
    let object = params
        .as_object_mut()
        .ok_or_else(|| "invalid renderer request".to_string())?;
    object.insert(key.to_string(), Value::String(value.to_string()));
    manager.invoke(command, params, cid).await
}

async fn invoke_empty(
    manager: State<'_, Arc<EngineManager>>,
    command: &'static str,
) -> Result<Value, String> {
    manager
        .invoke(command, Value::Object(Map::new()), None)
        .await
}

#[tauri::command]
pub fn engine_ready(manager: State<'_, Arc<EngineManager>>) -> bool {
    manager.is_ready()
}

#[tauri::command]
pub async fn engine_health(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "health").await
}

#[tauri::command]
pub async fn record_load(
    manager: State<'_, Arc<EngineManager>>,
    request: RecordLoadRequest,
) -> Result<Value, String> {
    invoke_request(manager, "record.load", request, None).await
}

#[tauri::command]
pub async fn record_save(
    manager: State<'_, Arc<EngineManager>>,
    request: RecordSaveRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request(manager, "record.save", request, cid).await
}

#[tauri::command]
pub async fn calendar_event_dates(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "calendar.event_dates").await
}

#[tauri::command]
pub async fn import_stats(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "import.stats").await
}

#[tauri::command]
pub async fn es_view(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "es.view").await
}

#[tauri::command]
pub async fn consult(
    manager: State<'_, Arc<EngineManager>>,
    request: ConsultRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request(manager, "consult", request, cid).await
}

#[tauri::command]
pub async fn calendar_sync_ics(
    manager: State<'_, Arc<EngineManager>>,
    request: CalendarIcsRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request_with_fixed_field(manager, "calendar.sync", request, cid, "source", "ics").await
}

#[tauri::command]
pub async fn calendar_sync_apple(
    manager: State<'_, Arc<EngineManager>>,
    request: CalendarAppleRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request_with_fixed_field(manager, "calendar.sync", request, cid, "source", "apple").await
}

#[tauri::command]
pub async fn import_line_single(
    manager: State<'_, Arc<EngineManager>>,
    request: ImportLineSingleRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request(manager, "import.line", request, cid).await
}

#[tauri::command]
pub async fn import_line_batch(
    manager: State<'_, Arc<EngineManager>>,
    request: ImportLineBatchRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request(manager, "import.line", request, cid).await
}

#[tauri::command]
pub async fn import_classify(
    manager: State<'_, Arc<EngineManager>>,
    request: ImportClassifyRequest,
) -> Result<Value, String> {
    invoke_request(manager, "import.classify", request, None).await
}

#[tauri::command]
pub async fn import_document(
    manager: State<'_, Arc<EngineManager>>,
    request: ImportDocumentRequest,
    cid: Option<u64>,
) -> Result<Value, String> {
    invoke_request(manager, "import.document", request, cid).await
}

#[tauri::command]
pub async fn settings_get(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "settings.get").await
}

#[tauri::command]
pub async fn settings_save_fixed(
    manager: State<'_, Arc<EngineManager>>,
    request: SettingsSaveFixedRequest,
) -> Result<Value, String> {
    invoke_request(manager, "settings.save_fixed", request, None).await
}

#[tauri::command]
pub async fn settings_run_profiler(
    manager: State<'_, Arc<EngineManager>>,
) -> Result<Value, String> {
    invoke_empty(manager, "settings.run_profiler").await
}

#[tauri::command]
pub async fn oracle_payload(
    manager: State<'_, Arc<EngineManager>>,
    request: ScopeRequest,
) -> Result<Value, String> {
    invoke_request(manager, "oracle.payload", request, None).await
}

#[tauri::command]
pub async fn oracle_report(
    manager: State<'_, Arc<EngineManager>>,
    request: ScopeRequest,
) -> Result<Value, String> {
    invoke_request(manager, "oracle.report", request, None).await
}

#[tauri::command]
pub async fn twin_forecast(
    manager: State<'_, Arc<EngineManager>>,
    request: TwinForecastRequest,
) -> Result<Value, String> {
    invoke_request(manager, "twin.forecast", request, None).await
}

#[tauri::command]
pub async fn tensor_rebuild(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "tensor.rebuild").await
}

#[tauri::command]
pub async fn profile_source_code(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "profile.source_code").await
}

#[tauri::command]
pub async fn narrative_compile(
    manager: State<'_, Arc<EngineManager>>,
    request: NarrativeCompileRequest,
) -> Result<Value, String> {
    invoke_request(manager, "narrative.compile", request, None).await
}

#[tauri::command]
pub async fn knowledge_fetch_pending(
    manager: State<'_, Arc<EngineManager>>,
) -> Result<Value, String> {
    invoke_empty(manager, "knowledge.fetch_pending").await
}

#[tauri::command]
pub async fn probe_status(
    manager: State<'_, Arc<EngineManager>>,
    request: ProbeDateRequest,
) -> Result<Value, String> {
    invoke_request(manager, "probe.status", request, None).await
}

#[tauri::command]
pub async fn probe_next(
    manager: State<'_, Arc<EngineManager>>,
    request: ProbeDateRequest,
) -> Result<Value, String> {
    invoke_request(manager, "probe.next", request, None).await
}

#[tauri::command]
pub async fn probe_answer(
    manager: State<'_, Arc<EngineManager>>,
    request: ProbeAnswerRequest,
) -> Result<Value, String> {
    invoke_request(manager, "probe.answer", request, None).await
}

#[tauri::command]
pub async fn context_manifest_latest(
    manager: State<'_, Arc<EngineManager>>,
) -> Result<Value, String> {
    invoke_empty(manager, "context.manifest.latest").await
}
