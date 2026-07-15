use std::io::{self, Write};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::State;

use crate::engine::{EngineManager, IPC_MAX_REQUEST_LINE_BYTES};
use crate::ipc_contract::{
    CalendarAppleRequest, CalendarIcsRequest, ConsultRequest, ImportClassifyRequest,
    ImportDocumentRequest, ImportLineBatchRequest, ImportLineSingleRequest,
    NarrativeCompileRequest, ProbeAnswerRequest, ProbeDateRequest, RecordLoadRequest,
    RecordSaveRequest, ScopeRequest, SettingsSaveFixedRequest, TwinForecastRequest,
    ValidateRequest, IPC_REQUEST_ENVELOPE_HEADROOM_BYTES, MAX_REQUEST_PARAMS_JSON_BYTES,
    MAX_TEXT_BYTES, REQUEST_PARAMS_JSON_HEADROOM_BYTES,
};

const _: () =
    assert!(MAX_TEXT_BYTES + REQUEST_PARAMS_JSON_HEADROOM_BYTES == MAX_REQUEST_PARAMS_JSON_BYTES);
const _: () = assert!(
    MAX_REQUEST_PARAMS_JSON_BYTES + IPC_REQUEST_ENVELOPE_HEADROOM_BYTES
        == IPC_MAX_REQUEST_LINE_BYTES
);

struct CappedJsonSink {
    written: usize,
    exceeded: bool,
}

impl CappedJsonSink {
    fn new() -> Self {
        Self {
            written: 0,
            exceeded: false,
        }
    }
}

impl Write for CappedJsonSink {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.len() > MAX_REQUEST_PARAMS_JSON_BYTES.saturating_sub(self.written) {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "renderer request size limit exceeded",
            ));
        }
        self.written += buffer.len();
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn require_request_params_size<T: Serialize + ?Sized>(params: &T) -> Result<(), String> {
    let mut sink = CappedJsonSink::new();
    match serde_json::to_writer(&mut sink, params) {
        Ok(()) => Ok(()),
        Err(_) if sink.exceeded => Err("renderer request exceeds the allowed size".to_string()),
        Err(_) => Err("invalid renderer request".to_string()),
    }
}

fn request_params<T: Serialize + ValidateRequest>(request: T) -> Result<Value, String> {
    request.validate()?;
    require_request_params_size(&request)?;
    let params =
        serde_json::to_value(request).map_err(|_| "invalid renderer request".to_string())?;
    Ok(params)
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
    require_request_params_size(&params)?;
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

#[cfg(test)]
mod request_size_tests {
    use super::*;

    #[test]
    fn exact_field_limit_plain_text_fits_below_the_aggregate_cap() {
        let query = "a".repeat(MAX_TEXT_BYTES);
        let request: ConsultRequest =
            serde_json::from_value(serde_json::json!({ "query": query })).unwrap();

        assert!(request.validate().is_ok());
        let params = request_params(request).expect("near-limit params must fit");
        let mut sink = CappedJsonSink::new();
        serde_json::to_writer(&mut sink, &params).unwrap();

        assert!(sink.written <= MAX_REQUEST_PARAMS_JSON_BYTES);
    }

    #[test]
    fn aggregate_cap_rejects_individually_valid_batch_items() {
        let content = "a".repeat(4 * 1024 * 1024);
        let request: CalendarIcsRequest = serde_json::from_value(serde_json::json!({
            "mode": "append",
            "ics_files": [
                { "content": content.clone(), "filename": "a.ics" },
                { "content": content, "filename": "b.ics" }
            ]
        }))
        .unwrap();

        assert!(
            request.validate().is_ok(),
            "each item is below its field cap"
        );
        assert_eq!(
            request_params(request).unwrap_err(),
            "renderer request exceeds the allowed size"
        );
    }

    #[test]
    fn aggregate_cap_rejects_json_escape_expansion_below_the_field_cap() {
        let query = "\u{0001}".repeat(2 * 1024 * 1024);
        let request: ConsultRequest =
            serde_json::from_value(serde_json::json!({ "query": query })).unwrap();

        assert!(
            request.validate().is_ok(),
            "raw text is below its field cap"
        );
        assert_eq!(
            request_params(request).unwrap_err(),
            "renderer request exceeds the allowed size"
        );
    }
}
