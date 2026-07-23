use std::io::{self, Write};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::State;

use crate::engine::{EngineManager, IPC_MAX_REQUEST_LINE_BYTES};
use crate::ipc_contract::{
    CalendarAppleRequest, CalendarIcsRequest, ConsultRequest, EsViewRequest,
    ImportClassifyRequest, ImportDocumentRequest, ImportLineBatchRequest,
    ImportLineSingleRequest, KnowledgePolicySetRequest, KnowledgeResearchRequest,
    LlmWarmRequest, NarrativeCompileRequest, ProbeAnswerRequest, ProbeDateRequest,
    RecordLoadRequest, RecordSaveRequest, ScopeRequest, SettingsSaveFixedRequest,
    TwinForecastRequest, ValidateRequest, IPC_REQUEST_ENVELOPE_HEADROOM_BYTES,
    MAX_REQUEST_PARAMS_JSON_BYTES, MAX_TEXT_BYTES, REQUEST_PARAMS_JSON_HEADROOM_BYTES,
};
use crate::knowledge::{
    refuse_if_egress_unavailable, refuse_if_policy_off, NetworkPolicyStore,
};

// iOS/mobile on-device fallback for settings_run_profiler / tensor_rebuild
// (M20 データ連携 — bug 3 root fix, moved from a frontend try/catch into
// backend routing per user architecture ruling 2026-07-23). Only compiled
// where the Vault + Rust analytics stack exist.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::analytics::commands::{calculate_gap_analysis, ensure_authoritative_tensor_profile};
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::analytics::commands_finance::unix_to_jst_date;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::analytics::input::{AnalyticsDailyDay, CalculateGapRequest, TransactionIn};
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::db::VaultHandle;

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
            return Err(io::Error::other(
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
pub async fn es_view(
    manager: State<'_, Arc<EngineManager>>,
    request: Option<EsViewRequest>,
) -> Result<Value, String> {
    let req = request.unwrap_or(EsViewRequest { id: None });
    let id = req
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match id {
        Some(_) => invoke_request(manager, "es.view", req, None).await,
        None => invoke_empty(manager, "es.view").await,
    }
}

#[tauri::command]
pub async fn es_list(manager: State<'_, Arc<EngineManager>>) -> Result<Value, String> {
    invoke_empty(manager, "es.list").await
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
pub async fn llm_warm(
    manager: State<'_, Arc<EngineManager>>,
    request: LlmWarmRequest,
) -> Result<Value, String> {
    invoke_request(manager, "llm.warm", request, None).await
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

/// Last-30-day on-device evidence window for the gap/tensor rebuild fallback
/// when the Python sidecar is absent (iOS — `EngineManager::start()` is
/// `#[cfg(not(mobile))]`, so it never boots there).
///
/// Sourced from the Vault's purchase ledger — the one on-device, Rust-owned
/// RECORD-equivalent store that exists today (`record_purchase_with_snapshot`).
/// `diary_text` / `consultations` / `line_self_text` / `calendar_events` are
/// left at their honest empty defaults: there is no on-device Rust store for
/// diary or CONSULT-session text, and while `calendar::event_kit` could
/// supply `calendar_events`, reading it here would silently trigger the OS
/// Calendar permission prompt as a side effect of pressing "re-analyze
/// profile" — wiring that in belongs with a dedicated calendar-connect UI
/// flow, not this fallback. `today` (JST) is always included even with zero
/// purchases so the day list is never empty (mirrors Python's `rows = len
/// (daily)` semantics in `facade.py::tensor_rebuild`).
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
fn on_device_gap_days(vault: &VaultHandle) -> Result<Vec<AnalyticsDailyDay>, String> {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    const LOOKBACK_SECS: i64 = 30 * 24 * 3600;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let rows = vault
        .purchase_list_range(now - LOOKBACK_SECS, now)
        .map_err(|e| format!("{e:?}").to_ascii_lowercase())?;

    let mut by_date: BTreeMap<String, Vec<TransactionIn>> = BTreeMap::new();
    for row in rows {
        by_date
            .entry(unix_to_jst_date(row.occurred_at))
            .or_default()
            .push(TransactionIn {
                tx_type: "expense".to_string(),
                category: row.merchant_norm,
                amount: row.total_amount,
            });
    }
    by_date.entry(unix_to_jst_date(now)).or_default();

    Ok(by_date
        .into_iter()
        .map(|(date, transactions)| AnalyticsDailyDay {
            date,
            diary_text: String::new(),
            consultations: Vec::new(),
            transactions,
            calendar_events: Vec::new(),
            line_self_text: String::new(),
        })
        .collect())
}

/// iOS/mobile fallback shared by `settings_run_profiler` / `tensor_rebuild`:
/// rebuild the authoritative gap + tensor snapshot on-device from
/// [`on_device_gap_days`], reusing the exact same analytics commands the
/// desktop Python path is a proxy for (`calculate_gap_analysis` +
/// `ensure_authoritative_tensor_profile`) rather than re-implementing them.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
async fn on_device_profile_rebuild(vault: State<'_, VaultHandle>) -> Result<(bool, usize), String> {
    let days = on_device_gap_days(&vault)?;
    let day_count = days.len();
    calculate_gap_analysis(vault.clone(), CalculateGapRequest { days }).await?;
    ensure_authoritative_tensor_profile(vault).await?;
    Ok((true, day_count))
}

// Two mutually-exclusive definitions (same command name) rather than a
// `#[cfg]`-gated parameter — matches the established `EngineManager::start`
// / lib.rs mobile-vs-desktop split idiom already used in this codebase.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
#[tauri::command]
pub async fn settings_run_profiler(
    manager: State<'_, Arc<EngineManager>>,
    vault: State<'_, VaultHandle>,
) -> Result<Value, String> {
    if manager.is_ready() {
        return invoke_empty(manager, "settings.run_profiler").await;
    }
    let (rebuilt, day_count) = on_device_profile_rebuild(vault).await?;
    Ok(serde_json::json!({
        "ok": rebuilt,
        "message": format!("オンデバイスで再解析しました（{day_count}日分の記録）"),
    }))
}

#[cfg(not(all(feature = "secure-vault", target_vendor = "apple")))]
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

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
#[tauri::command]
pub async fn tensor_rebuild(
    manager: State<'_, Arc<EngineManager>>,
    vault: State<'_, VaultHandle>,
) -> Result<Value, String> {
    if manager.is_ready() {
        return invoke_empty(manager, "tensor.rebuild").await;
    }
    let (rebuilt, day_count) = on_device_profile_rebuild(vault).await?;
    Ok(serde_json::json!({ "rebuilt": rebuilt, "rows": day_count }))
}

#[cfg(not(all(feature = "secure-vault", target_vendor = "apple")))]
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

/// STEP 8: read persisted user consent for external research.
#[tauri::command]
pub async fn knowledge_policy_get(
    store: State<'_, NetworkPolicyStore>,
) -> Result<Value, String> {
    Ok(serde_json::json!({
        "schema": "knowledge_policy.v1",
        "enabled": store.enabled(),
    }))
}

/// STEP 8: persist user consent (default Off).
#[tauri::command]
pub async fn knowledge_policy_set(
    store: State<'_, NetworkPolicyStore>,
    request: KnowledgePolicySetRequest,
) -> Result<Value, String> {
    request.validate()?;
    store.set_enabled(request.enabled)?;
    knowledge_policy_get(store).await
}

/// STEP 6/8: explicit external research. Requires consent (Live) AND egress-live build.
#[tauri::command]
pub async fn knowledge_research(
    store: State<'_, NetworkPolicyStore>,
    request: KnowledgeResearchRequest,
) -> Result<Value, String> {
    request.validate()?;
    refuse_if_policy_off(store.get()).map_err(|e| e.to_string())?;
    refuse_if_egress_unavailable().map_err(|e| e.to_string())?;
    // Full live pipeline (intent → fetch → integrate) ships with egress-live ACK;
    // until wired end-to-end, fail closed after both gates pass.
    Err("EGRESS_LIVE_NOT_READY".to_string())
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
    // iOS/mobile has no Python sidecar (the engine never boots — see the
    // `#[cfg(mobile)]` setup branch in lib.rs), so proxying would return
    // "engine unavailable" and surface a scary "取得または検証に失敗" error.
    //
    // Investigated whether an on-device Rust equivalent exists (per the
    // architecture ruling that env-routing must live in Rust, not be
    // fabricated FE-side): `RetrievalManifestV1`'s candidate/lane/budget
    // bookkeeping is produced by Python's context-assembly pipeline as a
    // side effect of a `consult` call. The on-device CONSULT path
    // (`llm::commands_consult::consult_with_oracle_context`) builds its
    // prompt by direct string concatenation (`build_consult_with_oracle_prompt`)
    // — it never runs candidate selection or lane/budget accounting, so there
    // is no on-device data of this shape to read, only to fabricate. Per the
    // no-fabrication rule (never invent a manifest that wasn't actually
    // computed), return the schema's valid "no manifest yet" shape so the UI
    // renders the graceful "マニフェスト未生成" empty state instead of an
    // error. Porting the candidate/lane pipeline to Rust would make this
    // real; that is a real feature, not a fallback.
    if !manager.is_ready() {
        return Ok(serde_json::json!({ "manifest": null, "reason": "NO_MANIFEST" }));
    }
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
