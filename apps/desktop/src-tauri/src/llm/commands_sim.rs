//! M12 interview / ES simulation Tauri commands.
//!
//! Retrieves personal RAG context (vault KNN), merges EDINET company facts
//! (injected offline or dual-gated live list fetch), builds prompts via
//! `prompt_sim`, then streams through the existing M6 `LlmHandle::generate`
//! Channel pipeline (M7 cancel/purge unchanged).

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::db::{VaultErrorCode, VaultHandle};
#[cfg(feature = "egress-live")]
use crate::knowledge::edinet_client::subscription_key_from_env;
use crate::knowledge::edinet_client::{
    normalize_filer_key, render_company_facts_block, sanitize_company_facts, CompanyFacts,
    EdinetError, EdinetFactAcquisition,
};
use crate::knowledge::fact_merge::{
    apply_subject_transition, cell_from_wire, cell_to_wire, cells_from_edinet_acquisition,
    display_company_facts, merge_fact_cells, protected_cells_from_company_facts, FactCell,
    FactCellWire, FactCells, FactOrigin, FactOriginWire, FactStorage, FactStorageWire, SubjectKey,
    SubjectTransition,
};
use crate::knowledge::{refuse_if_egress_unavailable, refuse_if_policy_off, NetworkPolicyStore};
use crate::llm::commands_consult::ensure_model_loaded;
use crate::llm::context_budget::USER_TURN_MARKERS;
use crate::llm::params::GenerationParams;
use crate::llm::prompt_budget::fit_and_verify_prompt;
use crate::llm::prompt_sim::{
    build_company_analysis_prompt, build_es_review_prompt, build_interview_prompt,
    build_session_memory_prompt, ExperienceRef,
};
use crate::llm::service::TokenEvent;
use crate::llm::LlmHandle;
use crate::rag::commands_rag::{
    ingest_text_incremental_blocking, search_sync, validate_source_id, IngestKnowledgeResult,
};
use crate::rag::namespace::KnowledgeNamespace;

const MAX_TEXT_BYTES: usize = 64 * 1024;
const DEFAULT_CONTEXT_LIMIT: u32 = 5;
const MAX_CONTEXT_LIMIT: u32 = 20;
const DEFAULT_N_CTX: u32 = 2048;
const DEFAULT_MAX_TOKENS: u32 = 256;
/// Company-analysis dashboard only: 4 headed sections x 3-5 bullets does not fit
/// in a single-turn output budget. Kept local to that command so interview turns
/// (latency-sensitive, one question at a time) are unaffected.
const ANALYSIS_MAX_TOKENS: u32 = 768;
const ANALYSIS_N_CTX: u32 = 3072;
const EDINET_FETCH_DEADLINE: Duration = Duration::from_secs(15);
/// Per-archive parse wall budget (ZIP entry/XML/CSV). Download uses
/// `edinet_archive::DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE` (120s, Step 5).
#[cfg(feature = "egress-live")]
const EDINET_PARSE_DEADLINE: Duration = Duration::from_secs(20);
/// Whole ZIP job = 2 download + 2 parse + merge slack.
#[cfg(feature = "egress-live")]
const EDINET_ZIP_JOB_DEADLINE: Duration = Duration::from_secs(300);
/// How many calendar days before anchor to scan when resolving by filer name.
const EDINET_NAME_LOOKBACK_DAYS: u32 = 21;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimGenParams {
    pub n_ctx: Option<u32>,
    pub max_tokens: Option<u32>,
    pub temp: Option<f32>,
    pub top_k: Option<i32>,
    pub top_p: Option<f32>,
    pub seed: Option<u32>,
    pub context_limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInterviewParams {
    pub message: String,
    /// Offline / pre-fetched facts (preferred when egress is Off).
    pub company_facts: Option<CompanyFacts>,
    /// When set with Live∧egress-live, fetch documents.json for this code.
    pub edinet_code: Option<String>,
    /// Filing date YYYY-MM-DD for live list fetch.
    pub edinet_date: Option<String>,
    /// Optional UTF-8 yuho excerpt to merge risk/performance sections.
    pub filing_text: Option<String>,
    /// Optional ES body used as interview base (empty = zero-base).
    pub es_text: Option<String>,
    pub gen: Option<SimGenParams>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEsParams {
    pub es_draft: String,
    pub company_facts: Option<CompanyFacts>,
    pub edinet_code: Option<String>,
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
    /// Query used for RAG experience retrieval (defaults to es_draft prefix).
    pub experience_query: Option<String>,
    pub gen: Option<SimGenParams>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchEdinetFactsParams {
    /// Preferred: resolve EDINET code from filer name (UI never asks for code).
    pub company_name: Option<String>,
    /// Optional explicit code (legacy / internal). Prefer company_name.
    pub edinet_code: Option<String>,
    /// Filing date YYYY-MM-DD (anchor for list scan).
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdinetEnrichmentRequestV3 {
    pub schema_version: u16,
    pub subject_key: SubjectKey,
    pub subject_revision: i64,
    pub subject_transition: Option<SubjectTransition>,
    pub fact_cells: Option<Vec<FactCellWire>>,
    pub company_facts: Option<CompanyFacts>,
    pub edinet_code: Option<String>,
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdinetEnrichmentResponseV3 {
    pub schema_version: u16,
    pub facts: CompanyFacts,
    pub fact_cells: Vec<FactCellWire>,
    pub subject_key: SubjectKey,
    pub subject_revision: i64,
    pub field_provenance: Vec<FieldProvenance>,
    pub fetch: FetchStatus,
    pub extraction: ExtractionStatus,
    pub discovery_coverage: DiscoveryCoverage,
    pub discovery_result: DiscoveryResult,
    pub fact_persistence: FactPersistence,
    pub evidence_persistence: EvidencePersistence,
    pub served_from: ServedFrom,
    pub freshness: FilingFreshnessReport,
    pub warnings: Vec<EdinetWarningCode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldProvenance {
    pub field: String,
    pub origin: FactOriginWire,
    pub storage: FactStorageWire,
    pub doc_id: Option<String>,
    pub submitted_at: Option<String>,
    pub fetched_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilingFreshnessReport {
    pub selected_doc_id: Option<String>,
    pub submitted_at: Option<String>,
    pub correction_available: Tristate,
}

macro_rules! status_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[allow(dead_code)] // Stable V3 wire variants; not every status is emitted by this tranche.
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}
status_enum!(FetchStatus {
    NotAttempted,
    Succeeded,
    NetworkFailed,
    ApiError,
    Cancelled,
    MemoryPressure
});
status_enum!(ExtractionStatus {
    None,
    FinancialsOnly,
    NarrativesOnly,
    Both,
    ParseFailed,
    Cancelled
});
status_enum!(DiscoveryCoverage {
    NotRun,
    WindowComplete,
    WindowIncomplete,
    Pinned
});
status_enum!(DiscoveryResult {
    NotRun,
    Selected,
    NoEligibleInWindow,
    IdentityAmbiguous
});
status_enum!(FactPersistence {
    NotAttempted,
    Persisted,
    Conflict,
    Failed
});
status_enum!(EvidencePersistence {
    NotAttempted,
    SavedPendingEmbedding,
    Ready,
    Failed
});
status_enum!(ServedFrom {
    Live,
    Cache,
    Mixed,
    NotApplicable
});
status_enum!(Tristate { Yes, No, Unknown });
status_enum!(EdinetWarningCode {
    VaultReadFailed,
    VaultWriteFailed,
    SoftFallback
});

struct ResolvedEdinetAcquisition {
    acquisition: EdinetFactAcquisition,
    coverage: DiscoveryCoverage,
    result: DiscoveryResult,
    served_from: ServedFrom,
    correction_available: Tristate,
    fetch: FetchStatus,
    extraction: ExtractionStatus,
}

struct EdinetResolveFailure {
    message: String,
    fetch: FetchStatus,
    extraction: ExtractionStatus,
    coverage: DiscoveryCoverage,
    result: DiscoveryResult,
    served_from: ServedFrom,
    correction_available: Tristate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SimSessionResult {
    pub context_ids: Vec<String>,
    pub context_count: usize,
    pub company_name: String,
    pub facts_source: String,
}

fn map_vault_err(code: VaultErrorCode) -> String {
    format!("{code:?}").to_ascii_lowercase()
}

fn map_edinet_err(err: EdinetError) -> String {
    err.to_string()
}

fn has_company_knowledge(facts: &CompanyFacts) -> bool {
    !facts.business_summary.trim().is_empty()
        || !facts.business_risks.trim().is_empty()
        || !facts.performance_summary.trim().is_empty()
}

fn company_keys_match(left: &str, right: &str) -> bool {
    let left = normalize_filer_key(left);
    let right = normalize_filer_key(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right
        || (left.chars().count() >= 2
            && right.chars().count() >= 2
            && (left.contains(&right) || right.contains(&left)))
}

fn company_source_matches(source_id: &str, company_name: &str, edinet_code: &str) -> bool {
    let source = crate::db::knowledge_namespace::source_id_of(source_id);
    if let Some(code) = source.strip_prefix("edinet-") {
        return !edinet_code.trim().is_empty()
            && code.trim().eq_ignore_ascii_case(edinet_code.trim());
    }
    for prefix in ["company-", "wiki-"] {
        if let Some(alias) = source.strip_prefix(prefix) {
            return company_keys_match(alias, company_name);
        }
    }
    false
}

fn select_company_hits(
    hits: Vec<crate::db::KnowledgeSearchHit>,
    company_name: &str,
    edinet_code: &str,
) -> Vec<crate::db::KnowledgeSearchHit> {
    let mut matched_sources = HashSet::new();
    for hit in &hits {
        let source = crate::db::knowledge_namespace::source_id_of(&hit.id);
        if company_source_matches(source, company_name, edinet_code)
            || company_keys_match(&hit.text_content, company_name)
        {
            matched_sources.insert(source.to_string());
        }
    }
    hits.into_iter()
        .filter(|hit| {
            matched_sources.contains(crate::db::knowledge_namespace::source_id_of(&hit.id))
        })
        .collect()
}

fn resolve_gen(gen: Option<&SimGenParams>) -> (u32, GenerationParams) {
    let g = gen;
    let context_limit = g
        .and_then(|p| p.context_limit)
        .unwrap_or(DEFAULT_CONTEXT_LIMIT)
        .clamp(1, MAX_CONTEXT_LIMIT);
    let params = GenerationParams {
        prompt: String::new(),
        n_ctx: g.and_then(|p| p.n_ctx).unwrap_or(DEFAULT_N_CTX),
        max_tokens: g.and_then(|p| p.max_tokens).unwrap_or(DEFAULT_MAX_TOKENS),
        temp: g.and_then(|p| p.temp).unwrap_or(0.7),
        top_k: g.and_then(|p| p.top_k).unwrap_or(40),
        top_p: g.and_then(|p| p.top_p).unwrap_or(0.95),
        seed: g.and_then(|p| p.seed).unwrap_or(0),
    };
    (context_limit, params)
}

/// Multistage / oni interview: temperature is hallucination floor only; seed fixed (F-14).
/// FE `temp` cannot raise difficulty — intensity is tactic-driven when `oni_active`.
/// `turn_seed` comes from the frozen artifact's `per_turn_seeds` when present.
fn resolve_coliseum_gen(
    gen: Option<&SimGenParams>,
    turn_seed: Option<u32>,
) -> (u32, GenerationParams) {
    use crate::coliseum::{COLISEUM_GENERATION_SEED, COLISEUM_GENERATION_TEMP};
    let g = gen;
    let context_limit = g
        .and_then(|p| p.context_limit)
        .unwrap_or(DEFAULT_CONTEXT_LIMIT)
        .clamp(1, MAX_CONTEXT_LIMIT);
    let params = GenerationParams {
        prompt: String::new(),
        n_ctx: g.and_then(|p| p.n_ctx).unwrap_or(DEFAULT_N_CTX),
        max_tokens: g.and_then(|p| p.max_tokens).unwrap_or(DEFAULT_MAX_TOKENS),
        temp: COLISEUM_GENERATION_TEMP,
        top_k: g.and_then(|p| p.top_k).unwrap_or(40),
        top_p: g.and_then(|p| p.top_p).unwrap_or(0.95),
        seed: turn_seed.unwrap_or(COLISEUM_GENERATION_SEED),
    };
    (context_limit, params)
}

fn artifact_turn_seed(session: &InterviewSession) -> Option<u32> {
    session
        .session_artifact
        .as_ref()
        .map(|a| a.seed_for_turn(session.total_turns as usize))
}

/// Production entry: dual-factor gate + env key + (egress-live) Reqwest factory.
/// Tests use [`resolve_company_facts_with`] to inject key/factory and count calls.
async fn resolve_company_facts(
    store: &NetworkPolicyStore,
    _vault: &VaultHandle,
    injected: Option<CompanyFacts>,
    edinet_code: Option<String>,
    edinet_date: Option<String>,
    filing_text: Option<String>,
) -> Result<CompanyFacts, String> {
    #[cfg(feature = "egress-live")]
    {
        use crate::knowledge::net_gateway::ReqwestTransport;
        let mut discovery_cache =
            crate::knowledge::edinet_discovery::VaultDiscoveryCache::new(_vault);
        resolve_company_facts_with(
            store,
            Some(&mut discovery_cache),
            injected,
            edinet_code,
            edinet_date,
            filing_text,
            || subscription_key_from_env(),
            || ReqwestTransport::new().map_err(|_| "edinet_transport_unavailable".to_string()),
        )
        .await
    }
    #[cfg(not(feature = "egress-live"))]
    {
        // refuse_if_egress_unavailable always fails here — key/factory are never
        // invoked. Types exist only so the shared with() path typechecks.
        resolve_company_facts_with(
            store,
            None,
            injected,
            edinet_code,
            edinet_date,
            filing_text,
            || Err(EdinetError::ApiKeyMissing),
            || Err::<EgressDisabledTransport, String>("edinet_transport_unavailable".into()),
        )
        .await
    }
}

/// Injectable resolve path (Step 1 contract seam).
///
/// Call order after local early-returns:
/// 1. policy ∧ egress gate — on fail: soft-fallback, **no** key/factory/HTTP
/// 2. `key_provider` — on fail: soft-fallback, **no** factory/HTTP
/// 3. `transport_factory` (lazy) — on fail: soft-fallback, **no** HTTP get
/// 4. `HttpTransport::get` via EDINET list fetch — on fail: soft-fallback
async fn resolve_company_facts_with<KP, TF, T>(
    store: &NetworkPolicyStore,
    mut discovery_cache: Option<&mut dyn crate::knowledge::edinet_discovery::DiscoveryCache>,
    injected: Option<CompanyFacts>,
    edinet_code: Option<String>,
    edinet_date: Option<String>,
    filing_text: Option<String>,
    mut key_provider: KP,
    mut transport_factory: TF,
) -> Result<CompanyFacts, String>
where
    KP: FnMut() -> Result<String, EdinetError>,
    TF: FnMut() -> Result<T, String>,
    T: crate::knowledge::net_gateway::HttpTransport,
{
    use crate::knowledge::edinet_client::{
        fetch_company_facts_by_code, fetch_company_facts_by_name,
    };

    let injected_sanitized = match injected {
        Some(facts) => Some(sanitize_company_facts(&facts).map_err(map_edinet_err)?),
        None => None,
    };

    let mut code = edinet_code.unwrap_or_default();
    if code.trim().is_empty() {
        if let Some(base) = injected_sanitized.as_ref() {
            code = base.edinet_code.clone();
        }
    }
    let date = edinet_date.unwrap_or_default();
    let lookup_name = injected_sanitized
        .as_ref()
        .map(|f| f.company_name.trim().to_string())
        .unwrap_or_default();

    let can_fetch_by_code = !code.trim().is_empty() && !date.trim().is_empty();
    let can_fetch_by_name = !lookup_name.is_empty() && !date.trim().is_empty();

    if let Some(base) = injected_sanitized.as_ref() {
        let summary_ok = !base.business_summary.trim().is_empty();
        let code_ok = !base.edinet_code.trim().is_empty();
        if summary_ok && code_ok {
            return Ok(base.clone());
        }
        if summary_ok && !can_fetch_by_code && !can_fetch_by_name {
            return Ok(base.clone());
        }
        if !can_fetch_by_code && !can_fetch_by_name {
            if !base.company_name.trim().is_empty() {
                return Ok(base.clone());
            }
            return Err("company_facts_or_edinet_code_required".into());
        }
    } else if !can_fetch_by_code && !can_fetch_by_name {
        return Err("company_facts_or_edinet_code_required".into());
    }

    // Policy/egress off → soft-fail before key or transport (interview must not die).
    if refuse_if_policy_off(store.get()).is_err() || refuse_if_egress_unavailable().is_err() {
        return soft_fallback_named_base(injected_sanitized, "EGRESS_LIVE_NOT_READY");
    }
    let key = match key_provider() {
        Ok(k) => k,
        Err(err) => {
            return soft_fallback_named_base(injected_sanitized, &map_edinet_err(err));
        }
    };

    // Transport construction failure stays inside soft-fallback — never wipe base.
    let transport = match transport_factory() {
        Ok(t) => t,
        Err(msg) => {
            return soft_fallback_named_base(injected_sanitized, &msg);
        }
    };

    let fetched = if let Some(cache) = discovery_cache.as_deref_mut() {
        use crate::knowledge::edinet_client::{merge_company_facts, ListCandidateFilter};
        use crate::knowledge::edinet_discovery::{
            discover_eligible_in_window, may_transition_to_zip, DiscoverParams, DiscoveryResult,
            DEFAULT_DISCOVERY_WINDOW_DAYS_BACK,
        };
        let (subject_key, filter) = if can_fetch_by_code {
            match ListCandidateFilter::by_edinet_code(code.trim()) {
                Some(filter) => (format!("edinet:{}", code.trim()), filter),
                None => {
                    return soft_fallback_named_base(injected_sanitized, "edinet_invalid_argument");
                }
            }
        } else {
            match ListCandidateFilter::by_filer_name(&lookup_name) {
                Some(filter) => (
                    format!("name:{}", normalize_filer_key(&lookup_name)),
                    filter,
                ),
                None => {
                    return soft_fallback_named_base(injected_sanitized, "edinet_invalid_argument");
                }
            }
        };
        match discover_eligible_in_window(
            cache,
            &transport,
            DiscoverParams {
                subject_key: &subject_key,
                anchor_date: date.trim(),
                window_days_back: DEFAULT_DISCOVERY_WINDOW_DAYS_BACK,
                filter: &filter,
                subscription_key: &key,
                now_secs: now_unix(),
                max_live_gets: None,
                deadline: EDINET_FETCH_DEADLINE,
            },
        )
        .await
        {
            Ok(outcome) if may_transition_to_zip(&outcome) => match outcome.selected {
                Some(selected) => merge_company_facts(&selected, filing_text.as_deref()),
                None => Err(EdinetError::Malformed),
            },
            Ok(outcome) if outcome.result == DiscoveryResult::IdentityAmbiguous => {
                Err(EdinetError::AmbiguousSelection)
            }
            Ok(_) => Err(EdinetError::NoEligibleFiling),
            Err(crate::knowledge::edinet_discovery::DiscoveryError::Edinet(err)) => Err(err),
            Err(_) => Err(EdinetError::Malformed),
        }
    } else if can_fetch_by_code {
        fetch_company_facts_by_code(
            &transport,
            date.trim(),
            code.trim(),
            &key,
            filing_text.as_deref(),
            EDINET_FETCH_DEADLINE,
        )
        .await
    } else {
        fetch_company_facts_by_name(
            &transport,
            date.trim(),
            &lookup_name,
            &key,
            filing_text.as_deref(),
            EDINET_FETCH_DEADLINE,
            EDINET_NAME_LOOKBACK_DAYS,
        )
        .await
    };

    match fetched {
        Ok(fetched) => {
            if let Some(base) = injected_sanitized {
                Ok(merge_company_facts_prefer_filled(&base, &fetched))
            } else {
                Ok(fetched)
            }
        }
        Err(err) => soft_fallback_named_base(injected_sanitized, &map_edinet_err(err)),
    }
}

/// Type placeholder when `egress-live` is off (factory never succeeds / never used).
#[cfg(not(feature = "egress-live"))]
struct EgressDisabledTransport;

#[cfg(not(feature = "egress-live"))]
struct EgressDisabledBody;

#[cfg(not(feature = "egress-live"))]
impl crate::knowledge::net_gateway::ResponseBody for EgressDisabledBody {
    fn next_chunk(
        &mut self,
    ) -> impl std::future::Future<
        Output = Option<Result<Vec<u8>, crate::knowledge::net_gateway::GatewayError>>,
    > + Send {
        async { None }
    }
}

#[cfg(not(feature = "egress-live"))]
impl crate::knowledge::net_gateway::HttpTransport for EgressDisabledTransport {
    type Body = EgressDisabledBody;

    fn get(
        &self,
        _url: &str,
        _request_deadline: std::time::Duration,
    ) -> impl std::future::Future<
        Output = Result<
            (crate::knowledge::net_gateway::ResponseMeta, Self::Body),
            crate::knowledge::net_gateway::GatewayError,
        >,
    > + Send {
        async { Err(crate::knowledge::net_gateway::GatewayError::WireViolation) }
    }
}

/// Soft-fallback: keep sanitize済み base when it has a company name; else Err.
/// Never returns a fresh empty `CompanyFacts::default()`.
fn soft_fallback_named_base(
    base: Option<CompanyFacts>,
    err_without_base: &str,
) -> Result<CompanyFacts, String> {
    if let Some(base) = base {
        if !base.company_name.trim().is_empty() {
            return Ok(base);
        }
    }
    Err(err_without_base.to_string())
}

/// Fill empty fields on `base` from `incoming` (interview start merge).
fn merge_company_facts_prefer_filled(base: &CompanyFacts, incoming: &CompanyFacts) -> CompanyFacts {
    let pick = |cur: &str, next: &str| -> String {
        if !cur.trim().is_empty() {
            cur.to_string()
        } else if !next.trim().is_empty() {
            next.to_string()
        } else {
            cur.to_string()
        }
    };
    let source = if base.business_summary.trim().is_empty() && !incoming.source.trim().is_empty() {
        incoming.source.clone()
    } else {
        base.source.clone()
    };
    CompanyFacts {
        company_name: pick(&base.company_name, &incoming.company_name),
        edinet_code: pick(&base.edinet_code, &incoming.edinet_code),
        doc_id: pick(&base.doc_id, &incoming.doc_id),
        business_summary: pick(&base.business_summary, &incoming.business_summary),
        business_risks: pick(&base.business_risks, &incoming.business_risks),
        performance_summary: pick(&base.performance_summary, &incoming.performance_summary),
        source,
    }
}

/// Dual-gated EDINET list → [`CompanyFacts`] (UI prep / offline inject preferred).
#[tauri::command]
pub async fn fetch_edinet_company_facts(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    store: State<'_, NetworkPolicyStore>,
    llm: State<'_, LlmHandle>,
    params: FetchEdinetFactsParams,
) -> Result<CompanyFacts, String> {
    let company_name = params.company_name.clone().unwrap_or_default();
    let sanitized = sanitize_company_facts(&CompanyFacts {
        company_name,
        ..CompanyFacts::default()
    })
    .map_err(map_edinet_err)?;
    let subject_key = if let Some(code) = params.edinet_code.as_deref() {
        SubjectKey::try_from(format!("edinet:{}", code.trim()))
    } else {
        SubjectKey::try_from(format!(
            "name:{}",
            normalize_filer_key(&sanitized.company_name)
        ))
    }
    .map_err(|_| "invalid subject key".to_string())?;
    let temp_dir = resolve_edinet_temp_dir(&app);
    enrich_company_facts_from_edinet_core(
        vault.inner(),
        store.inner(),
        llm.inner(),
        temp_dir.as_deref(),
        EdinetEnrichmentRequestV3 {
            schema_version: 3,
            subject_key,
            subject_revision: 0,
            subject_transition: None,
            fact_cells: None,
            company_facts: Some(sanitized),
            edinet_code: params.edinet_code,
            edinet_date: params.edinet_date,
            filing_text: params.filing_text,
        },
    )
    .await
    .and_then(|outcome| match outcome.legacy_error {
        Some(msg) => Err(msg),
        None => Ok(outcome.response.facts),
    })
}

#[tauri::command]
pub async fn enrich_company_facts_from_edinet(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    store: State<'_, NetworkPolicyStore>,
    llm: State<'_, LlmHandle>,
    request: EdinetEnrichmentRequestV3,
) -> Result<EdinetEnrichmentResponseV3, String> {
    let temp_dir = resolve_edinet_temp_dir(&app);
    enrich_company_facts_from_edinet_core(
        vault.inner(),
        store.inner(),
        llm.inner(),
        temp_dir.as_deref(),
        request,
    )
    .await
    .map(|outcome| outcome.response)
}

fn resolve_edinet_temp_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let cache = app.path().app_cache_dir().ok()?;
    let dir = cache.join("edinet-tmp");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// fallback 層の唯一の戻り値。Err は入力棄却層専用。
struct EnrichOutcome {
    response: EdinetEnrichmentResponseV3,
    /// Some(...) は「base が空 かつ EDINET レーン失敗」の1ケースのみ（旧コマンド互換）。
    legacy_error: Option<String>,
}

fn merge_warning_codes(
    mut warnings: Vec<EdinetWarningCode>,
    extra: &[EdinetWarningCode],
) -> Vec<EdinetWarningCode> {
    for code in extra {
        if !warnings.contains(code) {
            warnings.push(*code);
        }
    }
    warnings
}

fn read_failed_warnings(read_failed: bool) -> Vec<EdinetWarningCode> {
    if read_failed {
        vec![EdinetWarningCode::VaultReadFailed]
    } else {
        Vec::new()
    }
}

/// V3 レーン唯一の outer fallback コンストラクタ（inner status は無加工転記）。
fn fallback_enrichment_response(
    base: &FactCells,
    revision: i64,
    subject: SubjectKey,
    failure: &EdinetResolveFailure,
    extra_warnings: &[EdinetWarningCode],
) -> EdinetEnrichmentResponseV3 {
    v3_response(
        wires_from_cells(base, revision),
        subject,
        revision,
        failure.fetch,
        failure.extraction,
        failure.coverage,
        failure.result,
        FactPersistence::NotAttempted,
        failure.served_from,
        None,
        failure.correction_available,
        merge_warning_codes(vec![EdinetWarningCode::SoftFallback], extra_warnings),
    )
}

fn identity_mismatch_response(
    base: &FactCells,
    revision: i64,
    subject: SubjectKey,
    resolved: &ResolvedEdinetAcquisition,
    extra_warnings: &[EdinetWarningCode],
) -> EdinetEnrichmentResponseV3 {
    v3_response(
        wires_from_cells(base, revision),
        subject,
        revision,
        resolved.fetch,
        resolved.extraction,
        resolved.coverage,
        DiscoveryResult::IdentityAmbiguous,
        FactPersistence::NotAttempted,
        resolved.served_from,
        None,
        resolved.correction_available,
        merge_warning_codes(vec![EdinetWarningCode::SoftFallback], extra_warnings),
    )
}

async fn enrich_company_facts_from_edinet_core(
    vault: &VaultHandle,
    store: &NetworkPolicyStore,
    llm: &LlmHandle,
    temp_dir: Option<&std::path::Path>,
    request: EdinetEnrichmentRequestV3,
) -> Result<EnrichOutcome, String> {
    if request.schema_version != 3 || request.subject_revision < 0 {
        return Err("invalid_v3_request".into());
    }
    if request.fact_cells.as_ref().is_some_and(|cells| {
        cells.is_empty()
            || cells.iter().any(|cell| {
                cell.schema_version != 3
                    || cell.revision != request.subject_revision
                    || !matches!(
                        cell.field.as_str(),
                        "company_name"
                            | "edinet_code"
                            | "doc_id"
                            | "business_summary"
                            | "business_risks"
                            | "performance_summary"
                    )
            })
    }) {
        return Err("invalid_v3_fact_cells".into());
    }
    if let Some(transition) = request.subject_transition.as_ref() {
        let to = match transition {
            SubjectTransition::Rekey { to, .. } | SubjectTransition::Switch { to, .. } => to,
        };
        if to != &request.subject_key {
            return Err("invalid_subject_transition".into());
        }
    }
    let sanitized_injected = request
        .company_facts
        .as_ref()
        .map(sanitize_company_facts)
        .transpose()
        .map_err(map_edinet_err)?;
    let params = FetchEdinetFactsParams {
        company_name: sanitized_injected
            .as_ref()
            .map(|facts| facts.company_name.clone()),
        edinet_code: request.edinet_code.clone(),
        edinet_date: request.edinet_date.clone(),
        filing_text: request.filing_text.clone(),
    };
    let subject = request.subject_key.clone();
    let subject_wire = String::from(subject.clone());
    let read_subject = request
        .subject_transition
        .as_ref()
        .map(|transition| match transition {
            SubjectTransition::Rekey { from, .. } | SubjectTransition::Switch { from, .. } => {
                from.clone()
            }
        })
        .unwrap_or_else(|| subject.clone());
    let read_subject_wire = String::from(read_subject.clone());
    let (vault_wire, read_failed) = match vault.fact_cells(read_subject_wire.clone()) {
        Ok(cells) => (cells, false),
        Err(_) => (Vec::new(), true),
    };
    let extras = read_failed_warnings(read_failed);
    let vault_revision = vault_wire
        .iter()
        .map(|cell| cell.revision)
        .max()
        .unwrap_or(0);
    if !read_failed && stale_subject_revision(request.subject_revision, vault_revision) {
        return Ok(EnrichOutcome {
            legacy_error: None,
            response: current_v3_response(
                vault,
                &read_subject_wire,
                read_subject,
                FactPersistence::Conflict,
                DiscoveryResult::NotRun,
                vault_wire,
                &extras,
            ),
        });
    }
    let revision = request.subject_revision;
    let input_wire = request.fact_cells.unwrap_or(vault_wire);
    let existing: FactCells = input_wire
        .iter()
        .map(|cell| (cell.field.clone(), cell_from_wire(cell)))
        .collect();
    let is_switch = matches!(
        request.subject_transition,
        Some(SubjectTransition::Switch { .. })
    );
    let transitioned = apply_subject_transition(&existing, request.subject_transition.as_ref());
    let base = if is_switch {
        switch_base_from_company_facts(sanitized_injected.as_ref())
    } else if transitioned.is_empty() {
        sanitized_injected
            .as_ref()
            .map(protected_cells_from_company_facts)
            .unwrap_or_default()
    } else {
        transitioned
    };
    let resolved = match resolve_typed_edinet_acquisition(
        vault,
        store,
        llm,
        sanitized_injected.as_ref(),
        params.edinet_code.as_deref(),
        params.edinet_date.as_deref(),
        params.filing_text.as_deref(),
        temp_dir,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(error) => {
            return Ok(EnrichOutcome {
                legacy_error: base.is_empty().then(|| error.message.clone()),
                response: fallback_enrichment_response(
                    &base, revision, subject, &error, &extras,
                ),
            });
        }
    };
    let acquisition = &resolved.acquisition;
    let incoming = cells_from_edinet_acquisition(acquisition);
    let merged = merge_fact_cells(&base, &incoming);
    let now = now_unix();
    let wires = wires_from_cells(&merged, revision + 1);
    let explicit_rekey_from =
        request
            .subject_transition
            .as_ref()
            .and_then(|transition| match transition {
                SubjectTransition::Rekey { from, to } if to == &subject => Some(from.clone()),
                _ => None,
            });
    if subject_wire.starts_with("name:") || explicit_rekey_from.is_some() {
        let target = match SubjectKey::try_from(format!("edinet:{}", acquisition.edinet_code())) {
            Ok(target) => target,
            Err(_) => {
                return Ok(EnrichOutcome {
                    legacy_error: None,
                    response: identity_mismatch_response(
                        &base, revision, subject, &resolved, &extras,
                    ),
                });
            }
        };
        if target != subject && explicit_rekey_from.is_some() {
            return Ok(EnrichOutcome {
                legacy_error: None,
                response: identity_mismatch_response(
                    &base, revision, subject, &resolved, &extras,
                ),
            });
        }
        let target_wire = String::from(target.clone());
        let from_wire = explicit_rekey_from
            .map(String::from)
            .unwrap_or_else(|| subject_wire.clone());
        let rekey_revision = if matches!(
            request.subject_transition,
            Some(SubjectTransition::Switch { .. })
        ) {
            0
        } else {
            revision
        };
        let persistence = match vault.fact_cells_rekey(
            from_wire,
            target_wire.clone(),
            rekey_revision,
            wires,
            now,
        ) {
            Ok(_) => FactPersistence::Persisted,
            Err(VaultErrorCode::Conflict) => FactPersistence::Conflict,
            Err(VaultErrorCode::IdentityAmbiguous) => {
                return Ok(EnrichOutcome {
                    legacy_error: None,
                    response: current_v3_response(
                        vault,
                        &target_wire,
                        target,
                        FactPersistence::Conflict,
                        DiscoveryResult::IdentityAmbiguous,
                        wires_from_cells(&merged, revision),
                        &extras,
                    ),
                });
            }
            Err(_) => FactPersistence::Failed,
        };
        return Ok(EnrichOutcome {
            legacy_error: None,
            response: persistence_response(
                vault,
                target,
                revision,
                persistence,
                &merged,
                &resolved,
                wires_from_cells(&merged, revision),
                &extras,
            ),
        });
    }
    let persist_revision = if matches!(
        request.subject_transition,
        Some(SubjectTransition::Switch { .. })
    ) {
        0
    } else {
        revision
    };
    let persistence = match vault.fact_cells_persist(subject_wire, persist_revision, wires, now) {
        Ok(_) => FactPersistence::Persisted,
        Err(VaultErrorCode::Conflict) => FactPersistence::Conflict,
        Err(_) => FactPersistence::Failed,
    };
    Ok(EnrichOutcome {
        legacy_error: None,
        response: persistence_response(
            vault,
            subject,
            revision,
            persistence,
            &merged,
            &resolved,
            wires_from_cells(&merged, revision),
            &extras,
        ),
    })
}

fn switch_base_from_company_facts(facts: Option<&CompanyFacts>) -> FactCells {
    let mut cells = FactCells::new();
    if let Some(company_name) = facts
        .map(|facts| facts.company_name.trim())
        .filter(|value| !value.is_empty())
    {
        cells.insert(
            "company_name".into(),
            FactCell {
                value: company_name.into(),
                origin: FactOrigin::Manual,
                storage: FactStorage::Session,
                doc_id: None,
                submitted_at: None,
                fetched_at: None,
            },
        );
    }
    cells
}

fn wires_from_cells(cells: &FactCells, revision: i64) -> Vec<FactCellWire> {
    cells
        .iter()
        .map(|(field, cell)| cell_to_wire(field, cell, revision, 3))
        .collect()
}

fn stale_subject_revision(expected: i64, current: i64) -> bool {
    expected != current
}

fn v3_response(
    fact_cells: Vec<FactCellWire>,
    subject_key: SubjectKey,
    revision: i64,
    fetch: FetchStatus,
    extraction: ExtractionStatus,
    discovery_coverage: DiscoveryCoverage,
    discovery_result: DiscoveryResult,
    fact_persistence: FactPersistence,
    served_from: ServedFrom,
    acquisition: Option<&EdinetFactAcquisition>,
    correction_available: Tristate,
    warnings: Vec<EdinetWarningCode>,
) -> EdinetEnrichmentResponseV3 {
    let cells: FactCells = fact_cells
        .iter()
        .map(|wire| (wire.field.clone(), cell_from_wire(wire)))
        .collect();
    let field_provenance = fact_cells
        .iter()
        .map(|cell| FieldProvenance {
            field: cell.field.clone(),
            origin: cell.origin,
            storage: cell.storage,
            doc_id: cell.doc_id.clone(),
            submitted_at: cell.submitted_at.clone(),
            fetched_at: cell.fetched_at,
        })
        .collect();
    EdinetEnrichmentResponseV3 {
        schema_version: 3,
        facts: display_company_facts(&cells, &CompanyFacts::default()),
        fact_cells,
        subject_key,
        subject_revision: revision,
        field_provenance,
        fetch,
        extraction,
        discovery_coverage,
        discovery_result,
        fact_persistence,
        evidence_persistence: EvidencePersistence::NotAttempted,
        served_from,
        freshness: FilingFreshnessReport {
            selected_doc_id: acquisition.map(|value| value.selected_doc_id().into()),
            submitted_at: acquisition.map(|value| value.submitted_at().into()),
            correction_available,
        },
        warnings,
    }
}

fn current_v3_response(
    vault: &VaultHandle,
    subject_wire: &str,
    subject_key: SubjectKey,
    persistence: FactPersistence,
    discovery_result: DiscoveryResult,
    known: Vec<FactCellWire>,
    extra_warnings: &[EdinetWarningCode],
) -> EdinetEnrichmentResponseV3 {
    let (fact_cells, revision, read_back_warnings) = match vault.fact_cells(subject_wire.to_string())
    {
        Ok(current) => {
            let revision = current.iter().map(|cell| cell.revision).max().unwrap_or(0);
            (current, revision, Vec::new())
        }
        Err(_) => {
            let revision = known.iter().map(|cell| cell.revision).max().unwrap_or(0);
            (
                known,
                revision,
                vec![EdinetWarningCode::VaultReadFailed],
            )
        }
    };
    v3_response(
        fact_cells,
        subject_key,
        revision,
        FetchStatus::NotAttempted,
        ExtractionStatus::None,
        DiscoveryCoverage::NotRun,
        discovery_result,
        persistence,
        ServedFrom::Cache,
        None,
        Tristate::Unknown,
        merge_warning_codes(read_back_warnings, extra_warnings),
    )
}

fn persistence_response(
    vault: &VaultHandle,
    subject_key: SubjectKey,
    previous_revision: i64,
    persistence: FactPersistence,
    merged: &FactCells,
    resolved: &ResolvedEdinetAcquisition,
    known_on_conflict: Vec<FactCellWire>,
    extra_warnings: &[EdinetWarningCode],
) -> EdinetEnrichmentResponseV3 {
    if matches!(persistence, FactPersistence::Conflict) {
        let subject_wire = String::from(subject_key.clone());
        return current_v3_response(
            vault,
            &subject_wire,
            subject_key,
            persistence,
            DiscoveryResult::Selected,
            known_on_conflict,
            extra_warnings,
        );
    }
    if matches!(persistence, FactPersistence::Persisted) {
        let subject_wire = String::from(subject_key.clone());
        return match vault.fact_cells(subject_wire) {
            Ok(current) => {
                let revision = current.iter().map(|cell| cell.revision).max().unwrap_or(0);
                v3_response(
                    current,
                    subject_key,
                    revision,
                    resolved.fetch,
                    resolved.extraction,
                    resolved.coverage,
                    resolved.result,
                    FactPersistence::Persisted,
                    resolved.served_from,
                    Some(&resolved.acquisition),
                    resolved.correction_available,
                    merge_warning_codes(Vec::new(), extra_warnings),
                )
            }
            Err(_) => v3_response(
                wires_from_cells(merged, previous_revision.saturating_add(1)),
                subject_key,
                previous_revision.saturating_add(1),
                resolved.fetch,
                resolved.extraction,
                resolved.coverage,
                resolved.result,
                FactPersistence::Persisted,
                resolved.served_from,
                Some(&resolved.acquisition),
                resolved.correction_available,
                merge_warning_codes(
                    vec![EdinetWarningCode::VaultReadFailed],
                    extra_warnings,
                ),
            ),
        };
    }
    v3_response(
        wires_from_cells(merged, previous_revision),
        subject_key,
        previous_revision,
        resolved.fetch,
        resolved.extraction,
        resolved.coverage,
        resolved.result,
        FactPersistence::Failed,
        resolved.served_from,
        Some(&resolved.acquisition),
        resolved.correction_available,
        merge_warning_codes(
            vec![EdinetWarningCode::VaultWriteFailed],
            extra_warnings,
        ),
    )
}

#[cfg_attr(not(feature = "egress-live"), allow(dead_code))]
fn extraction_from_acquisition(acquisition: &EdinetFactAcquisition) -> ExtractionStatus {
    let fields = acquisition.fields();
    let narratives = fields.business_summary || fields.business_risks;
    match (fields.performance_summary, narratives) {
        (true, true) => ExtractionStatus::Both,
        (true, false) => ExtractionStatus::FinancialsOnly,
        (false, true) => ExtractionStatus::NarrativesOnly,
        (false, false) => ExtractionStatus::None,
    }
}

#[cfg(feature = "egress-live")]
fn extraction_status_from_zip(
    acquisition: &EdinetFactAcquisition,
    interrupted: bool,
    extraction_failed: bool,
) -> ExtractionStatus {
    let base = extraction_from_acquisition(acquisition);
    let fields_all_false = matches!(base, ExtractionStatus::None);
    if interrupted && fields_all_false {
        ExtractionStatus::Cancelled
    } else if extraction_failed && fields_all_false {
        ExtractionStatus::ParseFailed
    } else {
        base
    }
}

#[cfg(feature = "egress-live")]
fn fetch_status_from_zip(
    interrupted: bool,
    guard: &crate::llm::service::EdinetJobGuard,
    any_download_ok: bool,
    only_network_failures: bool,
    only_api_failures: bool,
) -> FetchStatus {
    use crate::llm::service::EdinetCancelCause;
    if interrupted {
        return match guard.cancellation_cause() {
            EdinetCancelCause::MemoryPressure | EdinetCancelCause::ThermalPressure => {
                FetchStatus::MemoryPressure
            }
            EdinetCancelCause::Background
            | EdinetCancelCause::Dropped
            | EdinetCancelCause::None => FetchStatus::Cancelled,
        };
    }
    if any_download_ok {
        FetchStatus::Succeeded
    } else if only_network_failures {
        FetchStatus::NetworkFailed
    } else if only_api_failures {
        FetchStatus::ApiError
    } else {
        FetchStatus::Succeeded
    }
}

#[cfg(feature = "egress-live")]
fn is_interrupt_edinet_error(error: &EdinetError) -> bool {
    matches!(
        error,
        EdinetError::MemoryPressure
            | EdinetError::Gateway(crate::knowledge::net_gateway::GatewayError::Cancelled)
    )
}

#[cfg(feature = "egress-live")]
fn note_download_soft_fail(
    error: &EdinetError,
    only_network_failures: &mut bool,
    only_api_failures: &mut bool,
    extraction_failed: &mut bool,
) {
    match error {
        EdinetError::Gateway(crate::knowledge::net_gateway::GatewayError::Cancelled)
        | EdinetError::MemoryPressure => {}
        EdinetError::Gateway(_) | EdinetError::TempFileIo | EdinetError::TempArchiveBusy => {
            *only_api_failures = false;
        }
        EdinetError::ApiResponse { .. }
        | EdinetError::InvalidContentType
        | EdinetError::ApiKeyMissing => {
            *only_network_failures = false;
        }
        // STEP_12_DESIGN.md 決定表 #10-#12: 構造系リジェクト（サイズ上限・ZIP不正・
        // 非対応形式）はダウンロード段で検知されても extraction 系の失敗であり、
        // 両側成果ゼロなら fetch=succeeded + extraction=parse_failed に写像する。
        EdinetError::TooLarge
        | EdinetError::InvalidZip
        | EdinetError::UnsupportedArchive
        | EdinetError::UrlViolation
        | EdinetError::InvalidArgument => {
            *only_network_failures = false;
            *only_api_failures = false;
            *extraction_failed = true;
        }
        _ => {
            *only_network_failures = false;
            *only_api_failures = false;
            *extraction_failed = true;
        }
    }
}

/// パイプラインの soft 結果。Err を返さない（fallback は呼出側の既存機構）。
#[cfg(feature = "egress-live")]
struct EdinetZipOutcome {
    financials: Option<crate::knowledge::edinet_csv::PartialEdinetFacts>,
    narratives: Option<crate::knowledge::edinet_csv::PartialEdinetFacts>,
    fetch: FetchStatus,
    extraction_failed: bool,
    interrupted: bool,
    warnings: Vec<crate::knowledge::edinet_csv::EdinetWarning>,
}

/// spawn_blocking 境界。guard clone / token clone / TempArchive 所有権を
/// closure へ move。archive は closure 内で extract 後に必ず drop される。
#[cfg(feature = "egress-live")]
async fn extract_on_blocking<Fx>(
    guard: &crate::llm::service::EdinetJobGuard,
    archive: crate::knowledge::edinet_archive::TempArchive,
    extract: Fx,
) -> Result<crate::knowledge::edinet_csv::PartialEdinetFacts, EdinetError>
where
    Fx: FnOnce(
            &mut crate::knowledge::edinet_archive::TempArchive,
            &tokio_util::sync::CancellationToken,
        ) -> Result<crate::knowledge::edinet_csv::PartialEdinetFacts, EdinetError>
        + Send
        + 'static,
{
    let guard_clone = guard.clone();
    let token = guard.cancel_token();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let mut archive = archive;
        let result = extract(&mut archive, &token);
        drop(archive);
        drop(guard_clone);
        result
    })
    .await;
    match joined {
        Ok(result) => result,
        Err(_) => Err(EdinetError::Parse),
    }
}

/// 完全直列: [csvFlag=="1"] type=5 DL→extract→drop → type=1 DL→extract→drop。
#[cfg(feature = "egress-live")]
async fn run_edinet_zip_pipeline<T: crate::knowledge::net_gateway::HttpTransport>(
    transport: &T,
    selected: &crate::knowledge::edinet_client::YuhoSelection,
    subscription_key: &str,
    temp_dir: &std::path::Path,
    guard: &crate::llm::service::EdinetJobGuard,
) -> EdinetZipOutcome {
    use crate::knowledge::edinet_archive::{
        download_edinet_archive_to_temp, production_archive_gate, DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE,
        MAX_EDINET_ARCHIVE_BYTES,
    };
    use crate::knowledge::edinet_client::EdinetDocumentKind;
    use crate::knowledge::edinet_csv::{
        extract_financials_from_type5_archive, FinancialExtractMeta,
    };
    use crate::knowledge::edinet_xbrl::{
        extract_narratives_from_type1_archive, NarrativeExtractMeta,
    };
    use crate::llm::service::{EdinetCancelCause, EDINET_CANCEL_HEADROOM_BYTES};

    let mut financials = None;
    let mut narratives = None;
    let mut warnings = Vec::new();
    let mut extraction_failed = false;
    let mut interrupted = false;
    let mut any_download_ok = false;
    let mut only_network_failures = true;
    let mut only_api_failures = true;

    let doc_id = selected
        .original
        .doc_id
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let edinet_code = selected
        .original
        .edinet_code
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let submitted_at = selected
        .original
        .submit_date_time
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let csv_flag_one = selected
        .original
        .csv_flag
        .as_deref()
        .is_some_and(|v| v.trim() == "1");

    if csv_flag_one {
        guard.set_phase(crate::monitor::MemPhase::EdinetFetch);
        match download_edinet_archive_to_temp(
            transport,
            &doc_id,
            EdinetDocumentKind::XbrlCsv,
            subscription_key,
            temp_dir,
            &production_archive_gate(),
            &guard.cancel_token(),
            DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE,
            MAX_EDINET_ARCHIVE_BYTES,
            {
                let g = guard.clone();
                move || {
                    g.is_cancelled()
                        || crate::monitor::os_proc_available_memory_bytes()
                            .is_some_and(|v| v < EDINET_CANCEL_HEADROOM_BYTES)
                }
            },
        )
        .await
        {
            Ok(archive) => {
                any_download_ok = true;
                only_network_failures = false;
                only_api_failures = false;
                guard.set_phase(crate::monitor::MemPhase::EdinetExtract);
                let fin_meta = FinancialExtractMeta {
                    doc_id: doc_id.clone(),
                    edinet_code: edinet_code.clone(),
                    submitted_at: submitted_at.clone(),
                };
                match extract_on_blocking(guard, archive, move |archive, token| {
                    extract_financials_from_type5_archive(
                        archive,
                        &fin_meta,
                        EDINET_PARSE_DEADLINE,
                        token,
                    )
                })
                .await
                {
                    Ok(partial) => {
                        warnings.extend(partial.warnings.iter().cloned());
                        financials = Some(partial);
                    }
                    Err(error) if is_interrupt_edinet_error(&error) => {
                        if matches!(error, EdinetError::MemoryPressure) {
                            guard.cancel_with_cause(EdinetCancelCause::MemoryPressure);
                        }
                        interrupted = true;
                    }
                    Err(_) => {
                        extraction_failed = true;
                    }
                }
            }
            Err(error) if is_interrupt_edinet_error(&error) => {
                if matches!(error, EdinetError::MemoryPressure) {
                    guard.cancel_with_cause(EdinetCancelCause::MemoryPressure);
                }
                interrupted = true;
            }
            Err(error) => {
                note_download_soft_fail(
                    &error,
                    &mut only_network_failures,
                    &mut only_api_failures,
                    &mut extraction_failed,
                );
            }
        }
    }

    if !interrupted {
        guard.set_phase(crate::monitor::MemPhase::EdinetFetch);
        match download_edinet_archive_to_temp(
            transport,
            &doc_id,
            EdinetDocumentKind::FilingAndXbrl,
            subscription_key,
            temp_dir,
            &production_archive_gate(),
            &guard.cancel_token(),
            DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE,
            MAX_EDINET_ARCHIVE_BYTES,
            {
                let g = guard.clone();
                move || {
                    g.is_cancelled()
                        || crate::monitor::os_proc_available_memory_bytes()
                            .is_some_and(|v| v < EDINET_CANCEL_HEADROOM_BYTES)
                }
            },
        )
        .await
        {
            Ok(archive) => {
                any_download_ok = true;
                only_network_failures = false;
                only_api_failures = false;
                guard.set_phase(crate::monitor::MemPhase::EdinetExtract);
                let narr_meta = NarrativeExtractMeta {
                    doc_id: doc_id.clone(),
                    edinet_code: edinet_code.clone(),
                    submitted_at: submitted_at.clone(),
                    period_start: selected.original.period_start.clone(),
                    period_end: selected.original.period_end.clone(),
                };
                match extract_on_blocking(guard, archive, move |archive, token| {
                    extract_narratives_from_type1_archive(
                        archive,
                        &narr_meta,
                        EDINET_PARSE_DEADLINE,
                        token,
                    )
                })
                .await
                {
                    Ok(partial) => {
                        warnings.extend(partial.warnings.iter().cloned());
                        narratives = Some(partial);
                    }
                    Err(error) if is_interrupt_edinet_error(&error) => {
                        if matches!(error, EdinetError::MemoryPressure) {
                            guard.cancel_with_cause(EdinetCancelCause::MemoryPressure);
                        }
                        interrupted = true;
                    }
                    Err(_) => {
                        extraction_failed = true;
                    }
                }
            }
            Err(error) if is_interrupt_edinet_error(&error) => {
                if matches!(error, EdinetError::MemoryPressure) {
                    guard.cancel_with_cause(EdinetCancelCause::MemoryPressure);
                }
                interrupted = true;
            }
            Err(error) => {
                note_download_soft_fail(
                    &error,
                    &mut only_network_failures,
                    &mut only_api_failures,
                    &mut extraction_failed,
                );
            }
        }
    }

    let fetch = fetch_status_from_zip(
        interrupted,
        guard,
        any_download_ok,
        only_network_failures,
        only_api_failures,
    );
    EdinetZipOutcome {
        financials,
        narratives,
        fetch,
        extraction_failed,
        interrupted,
        warnings,
    }
}

async fn resolve_typed_edinet_acquisition(
    vault: &VaultHandle,
    store: &NetworkPolicyStore,
    llm: &LlmHandle,
    injected: Option<&CompanyFacts>,
    edinet_code: Option<&str>,
    edinet_date: Option<&str>,
    filing_text: Option<&str>,
    temp_dir: Option<&std::path::Path>,
) -> Result<ResolvedEdinetAcquisition, EdinetResolveFailure> {
    #[cfg(feature = "egress-live")]
    {
        if refuse_if_policy_off(store.get()).is_err() || refuse_if_egress_unavailable().is_err() {
            return Err(resolve_failure(
                "edinet_not_ready",
                FetchStatus::NotAttempted,
            ));
        }
        let key = subscription_key_from_env()
            .map_err(|error| resolve_failure(&map_edinet_err(error), FetchStatus::ApiError))?;
        let transport = crate::knowledge::net_gateway::ReqwestTransport::new().map_err(|_| {
            resolve_failure("edinet_transport_unavailable", FetchStatus::NetworkFailed)
        })?;
        let date = edinet_date
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| resolve_failure("edinet_date_required", FetchStatus::NotAttempted))?;
        let mut discovery_cache =
            crate::knowledge::edinet_discovery::VaultDiscoveryCache::new(vault);
        let (subject_key, filter) = if let Some(code) =
            edinet_code.filter(|value| !value.trim().is_empty())
        {
            (
                format!("edinet:{}", code.trim()),
                crate::knowledge::edinet_client::ListCandidateFilter::by_edinet_code(code.trim())
                    .ok_or_else(|| {
                    resolve_failure("edinet_invalid_argument", FetchStatus::NotAttempted)
                })?,
            )
        } else {
            let name = injected
                .map(|facts| facts.company_name.as_str())
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    resolve_failure("company_name_required", FetchStatus::NotAttempted)
                })?;
            (
                format!("name:{}", normalize_filer_key(name)),
                crate::knowledge::edinet_client::ListCandidateFilter::by_filer_name(name)
                    .ok_or_else(|| {
                        resolve_failure("edinet_invalid_argument", FetchStatus::NotAttempted)
                    })?,
            )
        };
        use crate::knowledge::edinet_discovery::{
            discover_eligible_in_window, may_transition_to_zip, DiscoverParams, DiscoveryResult,
            DEFAULT_DISCOVERY_WINDOW_DAYS_BACK,
        };
        let outcome = discover_eligible_in_window(
            &mut discovery_cache,
            &transport,
            DiscoverParams {
                subject_key: &subject_key,
                anchor_date: date.trim(),
                window_days_back: DEFAULT_DISCOVERY_WINDOW_DAYS_BACK,
                filter: &filter,
                subscription_key: &key,
                now_secs: now_unix(),
                max_live_gets: None,
                deadline: EDINET_FETCH_DEADLINE,
            },
        )
        .await
        .map_err(|error| {
            use crate::knowledge::edinet_discovery::DiscoveryError;
            let fetch = match error {
                DiscoveryError::Gateway(_)
                | DiscoveryError::RetryableTransient
                | DiscoveryError::ExhaustedRetries => FetchStatus::NetworkFailed,
                DiscoveryError::AuthStopped
                | DiscoveryError::RateLimitedStopped
                | DiscoveryError::NonRetryableApi
                | DiscoveryError::Edinet(_) => FetchStatus::ApiError,
                _ => FetchStatus::NotAttempted,
            };
            resolve_failure("edinet_discovery_failed", fetch)
        })?;
        let coverage = map_discovery_coverage(outcome.coverage);
        let result = map_discovery_result(outcome.result);
        let correction_available = map_tristate(outcome.correction_available);
        let served_from = served_from_for_discovery(outcome.cache_days, outcome.live_days);
        if outcome.result == DiscoveryResult::IdentityAmbiguous {
            return Err(EdinetResolveFailure {
                message: "edinet_identity_ambiguous".into(),
                fetch: FetchStatus::Succeeded,
                extraction: ExtractionStatus::None,
                coverage,
                result,
                served_from,
                correction_available,
            });
        }
        if !may_transition_to_zip(&outcome) {
            return Err(EdinetResolveFailure {
                message: "edinet_discovery_incomplete".into(),
                fetch: FetchStatus::Succeeded,
                extraction: ExtractionStatus::None,
                coverage,
                result,
                served_from,
                correction_available,
            });
        }
        let selected_meta = outcome.selected.ok_or_else(|| EdinetResolveFailure {
            message: "edinet_no_eligible_filing".into(),
            fetch: FetchStatus::Succeeded,
            extraction: ExtractionStatus::None,
            coverage,
            result,
            served_from,
            correction_available,
        })?;
        let guard = llm.acquire_edinet_job().map_err(|error| {
            let mut failure = resolve_failure(&error, FetchStatus::MemoryPressure);
            failure.coverage = coverage;
            failure.result = result;
            failure.served_from = served_from;
            failure.correction_available = correction_available;
            failure
        })?;
        let _cancel_on_drop = guard.cancel_token().drop_guard();
        let _watch = crate::llm::service::spawn_edinet_cancel_watch(&guard);

        if let Some(text) = filing_text {
            let acquisition =
                crate::knowledge::edinet_client::acquisition_from_document_meta_with_body(
                    &selected_meta,
                    Some(text),
                )
                .map_err(|error| {
                    let mut failure =
                        resolve_failure(&map_edinet_err(error), FetchStatus::Succeeded);
                    failure.extraction = ExtractionStatus::ParseFailed;
                    failure
                })?;
            if guard.is_cancelled() {
                return Err(resolve_failure(
                    "edinet_cancelled",
                    match guard.cancellation_cause() {
                        crate::llm::service::EdinetCancelCause::MemoryPressure
                        | crate::llm::service::EdinetCancelCause::ThermalPressure => {
                            FetchStatus::MemoryPressure
                        }
                        _ => FetchStatus::Cancelled,
                    },
                ));
            }
            let extraction = extraction_from_acquisition(&acquisition);
            return Ok(ResolvedEdinetAcquisition {
                acquisition,
                coverage,
                result,
                served_from,
                correction_available,
                fetch: FetchStatus::Succeeded,
                extraction,
            });
        }

        let Some(temp_dir) = temp_dir else {
            return Err(resolve_failure(
                "edinet_zip_unavailable",
                FetchStatus::NotAttempted,
            ));
        };

        let selected = crate::knowledge::edinet_client::YuhoSelection {
            original: selected_meta,
            correction_available: matches!(correction_available, Tristate::Yes),
        };

        let timed = tokio::time::timeout(
            EDINET_ZIP_JOB_DEADLINE,
            run_edinet_zip_pipeline(&transport, &selected, &key, temp_dir, &guard),
        )
        .await;
        let outcome = match timed {
            Ok(outcome) => outcome,
            Err(_) => {
                guard.cancel_with_cause(crate::llm::service::EdinetCancelCause::Dropped);
                EdinetZipOutcome {
                    financials: None,
                    narratives: None,
                    fetch: FetchStatus::Cancelled,
                    extraction_failed: false,
                    interrupted: true,
                    warnings: Vec::new(),
                }
            }
        };
        let _ = outcome.warnings;

        let (acquisition, _evidence, _warn) =
            crate::knowledge::edinet_client::acquisition_from_selected_with_partials(
                &selected,
                outcome.financials.as_ref(),
                outcome.narratives.as_ref(),
            )
            .map_err(|error| {
                let mut failure = resolve_failure(&map_edinet_err(error), FetchStatus::Succeeded);
                failure.extraction = ExtractionStatus::ParseFailed;
                failure.coverage = coverage;
                failure.result = result;
                failure.served_from = served_from;
                failure.correction_available = correction_available;
                failure
            })?;

        let extraction = extraction_status_from_zip(
            &acquisition,
            outcome.interrupted,
            outcome.extraction_failed,
        );
        if matches!(extraction, ExtractionStatus::Cancelled)
            && matches!(
                outcome.fetch,
                FetchStatus::Cancelled | FetchStatus::MemoryPressure
            )
        {
            let mut failure = resolve_failure("edinet_cancelled", outcome.fetch);
            failure.extraction = ExtractionStatus::Cancelled;
            failure.coverage = coverage;
            failure.result = result;
            failure.served_from = served_from;
            failure.correction_available = correction_available;
            return Err(failure);
        }
        if matches!(
            outcome.fetch,
            FetchStatus::NetworkFailed | FetchStatus::ApiError
        ) && matches!(extraction, ExtractionStatus::None | ExtractionStatus::ParseFailed)
        {
            let mut failure = resolve_failure(
                if matches!(outcome.fetch, FetchStatus::NetworkFailed) {
                    "edinet_network_failed"
                } else {
                    "edinet_api_error"
                },
                outcome.fetch,
            );
            failure.extraction = extraction;
            failure.coverage = coverage;
            failure.result = result;
            failure.served_from = served_from;
            failure.correction_available = correction_available;
            return Err(failure);
        }
        if matches!(extraction, ExtractionStatus::ParseFailed)
            && matches!(outcome.fetch, FetchStatus::Succeeded)
            && matches!(extraction_from_acquisition(&acquisition), ExtractionStatus::None)
        {
            let mut failure = resolve_failure("edinet_parse_failed", FetchStatus::Succeeded);
            failure.extraction = ExtractionStatus::ParseFailed;
            failure.coverage = coverage;
            failure.result = result;
            failure.served_from = served_from;
            failure.correction_available = correction_available;
            return Err(failure);
        }

        Ok(ResolvedEdinetAcquisition {
            acquisition,
            coverage,
            result,
            served_from,
            correction_available,
            fetch: outcome.fetch,
            extraction,
        })
    }
    #[cfg(not(feature = "egress-live"))]
    {
        let _ = (
            vault,
            store,
            llm,
            injected,
            edinet_code,
            edinet_date,
            filing_text,
            temp_dir,
        );
        Err(resolve_failure(
            "egress_unavailable",
            FetchStatus::NotAttempted,
        ))
    }
}

fn resolve_failure(message: &str, fetch: FetchStatus) -> EdinetResolveFailure {
    EdinetResolveFailure {
        message: message.into(),
        fetch,
        extraction: ExtractionStatus::None,
        coverage: DiscoveryCoverage::NotRun,
        result: DiscoveryResult::NotRun,
        served_from: ServedFrom::NotApplicable,
        correction_available: Tristate::Unknown,
    }
}

#[cfg_attr(not(feature = "egress-live"), allow(dead_code))]
fn served_from_for_discovery(cache_days: u32, live_days: u32) -> ServedFrom {
    match (cache_days, live_days) {
        (0, 0) => ServedFrom::NotApplicable,
        (_, 0) => ServedFrom::Cache,
        (0, _) => ServedFrom::Live,
        _ => ServedFrom::Mixed,
    }
}

#[cfg(feature = "egress-live")]
fn map_discovery_coverage(
    value: crate::knowledge::edinet_discovery::DiscoveryCoverage,
) -> DiscoveryCoverage {
    use crate::knowledge::edinet_discovery::DiscoveryCoverage as Source;
    match value {
        Source::NotRun => DiscoveryCoverage::NotRun,
        Source::WindowComplete => DiscoveryCoverage::WindowComplete,
        Source::WindowIncomplete => DiscoveryCoverage::WindowIncomplete,
        Source::Pinned => DiscoveryCoverage::Pinned,
    }
}

#[cfg(feature = "egress-live")]
fn map_discovery_result(
    value: crate::knowledge::edinet_discovery::DiscoveryResult,
) -> DiscoveryResult {
    use crate::knowledge::edinet_discovery::DiscoveryResult as Source;
    match value {
        Source::NotRun => DiscoveryResult::NotRun,
        Source::Selected => DiscoveryResult::Selected,
        Source::NoEligibleInWindow => DiscoveryResult::NoEligibleInWindow,
        Source::IdentityAmbiguous => DiscoveryResult::IdentityAmbiguous,
    }
}

#[cfg(feature = "egress-live")]
fn map_tristate(value: crate::knowledge::edinet_discovery::Tristate) -> Tristate {
    use crate::knowledge::edinet_discovery::Tristate as Source;
    match value {
        Source::Yes => Tristate::Yes,
        Source::No => Tristate::No,
        Source::Unknown => Tristate::Unknown,
    }
}

/// Interview turn: RAG experience + company facts → streaming interviewer.
#[tauri::command]
pub async fn start_interview_session(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    store: State<'_, NetworkPolicyStore>,
    params: StartInterviewParams,
    on_token: Channel<TokenEvent>,
) -> Result<SimSessionResult, String> {
    let message = params.message.trim().to_string();
    if message.is_empty() || message.len() > MAX_TEXT_BYTES {
        return Err("invalid message".into());
    }
    // Previously called generate() completely blind (no load check at all —
    // the same class of bug consult_with_oracle_context had before its own
    // fix). Reuses the shared guard rather than duplicating load logic.
    ensure_model_loaded(&app, &llm).await?;

    let facts = resolve_company_facts(
        store.inner(),
        vault.inner(),
        params.company_facts,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await?;

    let es_text = params
        .es_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let (context_limit, mut gen) = resolve_gen(params.gen.as_ref());
    let _ = (context_limit, vault.inner()); // no Vault auto-RAG in interview production (M20-J)
    let message_for_prompt = message.clone();
    let facts_for_prompt = facts.clone();
    let es_for_prompt = es_text;
    let llm_fit = llm.inner().clone();
    let governor = llm.inner().governor();
    let n_ctx = gen.n_ctx;
    let max_tokens = gen.max_tokens;

    let (prompt, context_ids, verified_tokens) = tauri::async_runtime::spawn_blocking(move || {
        // Foundation-style: company facts + optional ES base + utterance (no vault KNN).
        let prompt = build_interview_prompt(
            &message_for_prompt,
            &facts_for_prompt,
            &[],
            es_for_prompt.as_deref(),
        );
        let (prompt, verified_tokens) = fit_and_verify_prompt(
            &llm_fit,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens,
            USER_TURN_MARKERS,
            "interview(single)",
        );
        Ok::<_, String>((prompt, Vec::<String>::new(), verified_tokens))
    })
    .await
    .map_err(|e| {
        log::error!("interview(single): prompt-build task join failed: {e}");
        "interview retrieve task join failed".to_string()
    })?
    .map_err(|e| {
        log::error!("interview(single): prompt-build task returned error: {e}");
        e
    })?;

    gen.prompt = prompt;
    log::info!(
        "interview(single): calling llm.generate (prompt_len={}, prompt_tokens={:?}, n_ctx={}, max_tokens={})",
        gen.prompt.len(),
        verified_tokens,
        gen.n_ctx,
        gen.max_tokens
    );
    llm.generate(gen, None, on_token).await.map_err(|e| {
        log::error!("interview(single): llm.generate failed: {e}");
        e
    })?;
    log::info!("interview(single): llm.generate completed successfully");

    Ok(SimSessionResult {
        context_count: context_ids.len(),
        context_ids,
        company_name: facts.company_name,
        facts_source: facts.source,
    })
}

/// ES draft review: RAG experience + company facts → streaming reviewer.
#[tauri::command]
pub async fn review_es_draft(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    store: State<'_, NetworkPolicyStore>,
    params: ReviewEsParams,
    on_token: Channel<TokenEvent>,
) -> Result<SimSessionResult, String> {
    let draft = params.es_draft.trim().to_string();
    if draft.is_empty() || draft.len() > MAX_TEXT_BYTES {
        return Err("invalid es_draft".into());
    }
    ensure_model_loaded(&app, &llm).await?;

    let facts = resolve_company_facts(
        store.inner(),
        vault.inner(),
        params.company_facts,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await?;

    let (context_limit, mut gen) = resolve_gen(params.gen.as_ref());
    let query = params
        .experience_query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(draft.as_str())
        .to_string();

    let vault = vault.inner().clone();
    let llm_search = llm.inner().clone();
    let governor = llm.inner().governor();
    let draft_for_prompt = draft.clone();
    let facts_for_prompt = facts.clone();
    let n_ctx = gen.n_ctx;
    let max_tokens = gen.max_tokens;

    let (prompt, context_ids, verified_tokens) = tauri::async_runtime::spawn_blocking(move || {
        let hits = search_sync(
            &vault,
            &llm_search,
            &query,
            context_limit,
            KnowledgeNamespace::All,
        )
        .unwrap_or_default();
        let refs: Vec<ExperienceRef<'_>> = hits
            .iter()
            .map(|hit| ExperienceRef {
                id: hit.id.as_str(),
                text: hit.text_content.as_str(),
            })
            .collect();
        let prompt = build_es_review_prompt(&draft_for_prompt, &facts_for_prompt, &refs);
        let (prompt, verified_tokens) = fit_and_verify_prompt(
            &llm_search,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens,
            USER_TURN_MARKERS,
            "es_review",
        );
        let ids: Vec<String> = hits.into_iter().map(|hit| hit.id).collect();
        Ok::<_, String>((prompt, ids, verified_tokens))
    })
    .await
    .map_err(|e| {
        log::error!("es_review: prompt-build task join failed: {e}");
        "es review retrieve task join failed".to_string()
    })?
    .map_err(|e| {
        log::error!("es_review: prompt-build task returned error: {e}");
        e
    })?;

    gen.prompt = prompt;
    log::info!(
        "es_review: calling llm.generate (prompt_len={}, prompt_tokens={:?}, n_ctx={}, max_tokens={})",
        gen.prompt.len(),
        verified_tokens,
        gen.n_ctx,
        gen.max_tokens
    );
    llm.generate(gen, None, on_token).await.map_err(|e| {
        log::error!("es_review: llm.generate failed: {e}");
        e
    })?;
    log::info!("es_review: llm.generate completed successfully");

    Ok(SimSessionResult {
        context_count: context_ids.len(),
        context_ids,
        company_name: facts.company_name,
        facts_source: facts.source,
    })
}

// ─── M17 multi-stage interview machine ───────────────────────────────────────

use crate::coliseum::session_artifact::{
    amount_band, is_late_night_jst, purchase_text_summary, DistortionEvidenceSnap,
    EvidenceSnapshot, InterviewSessionArtifact, PurchaseEvidenceSnap,
};
use crate::coliseum::CognitiveFossilSnapshot;
use crate::db::InterviewSessionRow;
use crate::llm::consult_context::load_mentor_context;
use crate::llm::interview_machine::{
    apply_candidate_answer, artifact_evidence_prompt_block, build_stage_prompt_prefix,
    oni_pressure_prompt_from_artifact, oni_pressure_prompt_from_fossils,
    record_interviewer_utterance, AdvanceOutcome, InterviewSession, InterviewStage,
};
use crate::llm::model_path::MODEL_FILENAME;
use sha2::{Digest, Sha256};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn model_hash_label() -> String {
    hex::encode(Sha256::digest(MODEL_FILENAME.as_bytes()))
}

fn freeze_evidence_from_vault(vault: &VaultHandle, frozen_at_unix: i64) -> EvidenceSnapshot {
    let distortions = vault
        .distortion_tags_list(32)
        .unwrap_or_default()
        .into_iter()
        .map(|row| DistortionEvidenceSnap {
            source_id: row.id,
            category: row.category,
            text_summary: row.snippet,
            confidence_score: row.confidence_score,
        })
        .collect();
    let purchases = vault
        .purchase_list_recent(32)
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            let late = is_late_night_jst(row.occurred_at);
            let band = amount_band(row.total_amount).to_string();
            PurchaseEvidenceSnap {
                source_id: row.id,
                text_summary: purchase_text_summary(&row.merchant_norm, row.total_amount, late),
                amount_band: band,
                late_night: late,
            }
        })
        .collect();
    EvidenceSnapshot {
        distortions,
        purchases,
        frozen_at_unix,
    }
}

fn fossil_from_evidence(evidence: &EvidenceSnapshot, r_unit: f64) -> CognitiveFossilSnapshot {
    CognitiveFossilSnapshot {
        r_at_decision: Some(r_unit.clamp(0.0, 1.0)),
        distortion_categories: evidence
            .distortions
            .iter()
            .map(|d| d.category.clone())
            .collect(),
        purchase_amounts: evidence
            .purchases
            .iter()
            .filter_map(|p| match p.amount_band.as_str() {
                "micro" => Some(500),
                "low" => Some(2_000),
                "mid" => Some(10_000),
                "high" => Some(40_000),
                "extreme" => Some(200_000),
                _ => None,
            })
            .collect(),
        late_night_purchase_count: evidence.purchases.iter().filter(|p| p.late_night).count()
            as u32,
        impulse_purchase_count: 0,
        blacklist_seed_terms: Vec::new(),
    }
}

fn persist_session(vault: &VaultHandle, session: &InterviewSession) -> Result<(), String> {
    let payload_json = serde_json::to_string(session)
        .map_err(|_| "interview session serialize failed".to_string())?;
    let (artifact_json, artifact_fingerprint) = match &session.session_artifact {
        Some(art) => {
            let json = serde_json::to_string(art)
                .map_err(|_| "interview artifact serialize failed".to_string())?;
            (json, art.fingerprint.clone())
        }
        None => (String::new(), String::new()),
    };
    vault
        .interview_session_put(InterviewSessionRow {
            id: session.id.clone(),
            updated_at: now_unix(),
            stage: session.stage.as_str().into(),
            status: session.status.clone(),
            payload_json,
            artifact_json,
            artifact_fingerprint,
        })
        .map_err(map_vault_err)
}

fn load_session(vault: &VaultHandle, id: &str) -> Result<InterviewSession, String> {
    let row = vault
        .interview_session_get(id.to_string())
        .map_err(map_vault_err)?
        .ok_or_else(|| "interview_session_not_found".to_string())?;
    let mut session: InterviewSession = serde_json::from_str(&row.payload_json)
        .map_err(|_| "interview session parse failed".to_string())?;
    // Prefer dedicated columns when payload predates artifact embedding.
    if session.session_artifact.is_none() && !row.artifact_json.is_empty() {
        let mut art: InterviewSessionArtifact = serde_json::from_str(&row.artifact_json)
            .map_err(|_| "interview artifact parse failed".to_string())?;
        if !row.artifact_fingerprint.is_empty() && art.fingerprint != row.artifact_fingerprint {
            return Err("session_artifact_fingerprint_mismatch".into());
        }
        art.rehydrate_directives();
        // Fingerprint is over canonical body; rehydrate must be idempotent.
        if !art.verify_fingerprint() {
            return Err("session_artifact_fingerprint_mismatch".into());
        }
        session.attach_artifact(art);
    }
    Ok(session)
}

fn build_machine_prompt(
    vault: &VaultHandle,
    llm: &LlmHandle,
    session: &InterviewSession,
    facts: &CompanyFacts,
    user_message: &str,
    context_limit: u32,
) -> Result<(String, Vec<String>), String> {
    let is_debrief = session.stage == InterviewStage::Debrief;

    // M20-J scope: Vault auto-RAG + Gap/Tensor ONLY in Debrief.
    // Foundation / Pressure: company facts + session transcript only (no vault KNN).
    let hits = if is_debrief {
        search_sync(
            vault,
            llm,
            user_message,
            context_limit,
            KnowledgeNamespace::All,
        )
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    let refs: Vec<ExperienceRef<'_>> = hits
        .iter()
        .map(|hit| ExperienceRef {
            id: hit.id.as_str(),
            text: hit.text_content.as_str(),
        })
        .collect();

    let mut prompt = build_stage_prompt_prefix(session, is_debrief);
    prompt.push_str("\n## 企業ファクト（EDINET）\n");
    prompt.push_str(&render_company_facts_block(facts));

    if is_debrief {
        prompt.push_str(
            "\n## 参考情報（Vault 自動探索・講評専用）\n\
（この候補者の過去セッションから抽出された記憶と、日常の記録が混在している。\n\
静的なプロフィールと最新の記憶を区別せず、一体の人物像として解釈せよ。\n\
記録に無いことを補完するな。矛盾があれば、その矛盾自体を指摘せよ。）\n",
        );
        if refs.is_empty() {
            prompt.push_str("（該当する知識チャンクは見つかりませんでした）\n");
        } else {
            for (i, hit) in refs.iter().enumerate() {
                prompt.push_str(&format!(
                    "[{}] (id={})\n{}\n\n",
                    i + 1,
                    hit.id,
                    hit.text.trim()
                ));
            }
        }
        // I-22 / M20-J: Gap / Tensor / Oracle only in debrief.
        match load_mentor_context(vault) {
            Ok(mentor) => {
                prompt.push_str("\n## 主観×客観ギャップ（講評専用）\n");
                prompt.push_str(&mentor.gap_block);
                prompt.push_str("\n## Tensorプロファイル（講評専用）\n");
                prompt.push_str(&mentor.tensor_block);
                prompt.push_str("\n## Oracle予測（講評専用）\n");
                prompt.push_str(&mentor.oracle_block);
            }
            Err(_) => {
                prompt.push_str(
                    "\n（Gap/Tensor/Oracle 取得不可 — 講評は企業ファクトと会話ログのみ）\n",
                );
            }
        }
    } else {
        prompt.push_str(
            "\n## 参照範囲（面接本番）\n\
（Vault / Gap / Tensor の自動探索は無効。企業ファクトと本セッションの対話のみを根拠にせよ。）\n",
        );
        // Phase 14.4: Pressure uses frozen artifact directives (replayable; no live vault).
        if session.stage == InterviewStage::Pressure && session.oni_active {
            let sealed = if let Some(art) = session.session_artifact.as_ref() {
                oni_pressure_prompt_from_artifact(art, "")
            } else {
                let empty = CognitiveFossilSnapshot {
                    r_at_decision: None,
                    distortion_categories: Vec::new(),
                    purchase_amounts: Vec::new(),
                    late_night_purchase_count: 0,
                    impulse_purchase_count: 0,
                    blacklist_seed_terms: Vec::new(),
                };
                oni_pressure_prompt_from_fossils(empty, "")
            };
            match sealed {
                Ok(text) if !text.is_empty() => {
                    prompt.push_str("\n## 鬼モード抽象戦術（I-22 凍結ディレクティブ）\n");
                    prompt.push_str(&text);
                    prompt.push('\n');
                }
                _ => {}
            }
        }
    }

    if is_debrief {
        if let Some(art) = session.session_artifact.as_ref() {
            prompt.push('\n');
            prompt.push_str(&artifact_evidence_prompt_block(art));
        }
    }

    prompt.push_str("\n## 候補者の発話\n");
    prompt.push_str(user_message.trim());
    prompt.push('\n');

    let ids: Vec<String> = hits.into_iter().map(|h| h.id).collect();
    Ok((prompt, ids))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMultistageInterviewParams {
    pub opening_message: Option<String>,
    pub company_facts: Option<CompanyFacts>,
    pub edinet_code: Option<String>,
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
    pub gen: Option<SimGenParams>,
    /// Request 鬼モード (Pressure + abstract tactics). Subject to ZPD hard gate.
    pub oni_mode: Option<bool>,
    /// Quantized Twin R(t) on 0..=100. Required when oni_mode; else ignored.
    pub r_t: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MultistageInterviewResult {
    pub session_id: String,
    pub stage: String,
    pub status: String,
    pub turn_in_stage: u32,
    pub total_turns: u32,
    pub context_ids: Vec<String>,
    pub company_name: String,
    pub outcome: String,
    pub oni_active: bool,
}

/// Start Foundation stage; streams first interviewer question.
#[tauri::command]
pub async fn start_multistage_interview(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    store: State<'_, NetworkPolicyStore>,
    params: StartMultistageInterviewParams,
    on_token: Channel<TokenEvent>,
) -> Result<MultistageInterviewResult, String> {
    ensure_model_loaded(&app, &llm).await?;
    let facts = resolve_company_facts(
        store.inner(),
        vault.inner(),
        params.company_facts,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await?;
    let facts_json =
        serde_json::to_string(&facts).map_err(|_| "facts serialize failed".to_string())?;
    let session_id = format!("iv-{}", now_unix());
    let mut session =
        InterviewSession::new(session_id.clone(), facts.company_name.clone(), facts_json);
    let oni_requested = params.oni_mode.unwrap_or(false);
    let r_t = params
        .r_t
        .unwrap_or_else(|| crate::coliseum::mentor_zpd::r_t_from_unit_interval(1.0));
    session.oni_active = if !oni_requested {
        false
    } else if crate::coliseum::mentor_zpd::evaluate_oni_mode_eligibility(r_t).is_err() {
        // Structural hard-downgrade: depleted R never enters oni Pressure.
        false
    } else {
        crate::coliseum::resolve_oni_activation(true, r_t).unwrap_or(false)
    };

    // Phase 14.4: freeze start-of-session artifact (evidence bodies + directives).
    let frozen_at = now_unix();
    let evidence = freeze_evidence_from_vault(vault.inner(), frozen_at);
    let r_unit = f64::from(r_t) / 100.0;
    let fossil = fossil_from_evidence(&evidence, r_unit);
    let artifact = InterviewSessionArtifact::freeze(
        session_id.clone(),
        model_hash_label(),
        r_unit,
        fossil,
        evidence,
    );
    session.attach_artifact(artifact);

    let opening = params
        .opening_message
        .unwrap_or_else(|| "自己紹介と、志望動機を簡潔に述べてください。".into());

    let (context_limit, mut gen) =
        resolve_coliseum_gen(params.gen.as_ref(), artifact_turn_seed(&session));
    let vault_c = vault.inner().clone();
    let llm_s = llm.inner().clone();
    let governor = llm.inner().governor();
    let facts_c = facts.clone();
    let session_snapshot = session.clone();
    let opening_c = opening.clone();
    let n_ctx = gen.n_ctx;
    let max_tokens = gen.max_tokens;

    let (prompt, context_ids, verified_tokens) = tauri::async_runtime::spawn_blocking(move || {
        let (prompt, ids) = build_machine_prompt(
            &vault_c,
            &llm_s,
            &session_snapshot,
            &facts_c,
            &opening_c,
            context_limit,
        )?;
        let (prompt, verified_tokens) = fit_and_verify_prompt(
            &llm_s,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens,
            USER_TURN_MARKERS,
            "interview(multistage-start)",
        );
        Ok::<_, String>((prompt, ids, verified_tokens))
    })
    .await
    .map_err(|e| {
        log::error!("interview(multistage-start): prompt-build task join failed: {e}");
        "multistage start join failed".to_string()
    })?
    .map_err(|e| {
        log::error!("interview(multistage-start): prompt-build task returned error: {e}");
        e
    })?;

    record_interviewer_utterance(&mut session, &opening);
    persist_session(vault.inner(), &session)?;

    gen.prompt = prompt;
    log::info!(
        "interview(multistage-start): calling llm.generate (prompt_len={}, prompt_tokens={:?}, n_ctx={}, max_tokens={})",
        gen.prompt.len(),
        verified_tokens,
        gen.n_ctx,
        gen.max_tokens
    );
    llm.generate(gen, None, on_token).await.map_err(|e| {
        log::error!("interview(multistage-start): llm.generate failed: {e}");
        e
    })?;
    log::info!("interview(multistage-start): llm.generate completed successfully");

    Ok(MultistageInterviewResult {
        session_id,
        stage: session.stage.as_str().into(),
        status: session.status,
        turn_in_stage: session.turn_in_stage,
        total_turns: session.total_turns,
        context_ids,
        company_name: facts.company_name,
        outcome: "started".into(),
        oni_active: session.oni_active,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceInterviewParams {
    pub session_id: String,
    pub candidate_answer: String,
    pub gen: Option<SimGenParams>,
}

/// Apply candidate answer, advance FSM, stream next interviewer turn (or debrief).
#[tauri::command]
pub async fn advance_interview_stage(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    params: AdvanceInterviewParams,
    on_token: Channel<TokenEvent>,
) -> Result<MultistageInterviewResult, String> {
    let answer = params.candidate_answer.trim().to_string();
    if answer.is_empty() || answer.len() > MAX_TEXT_BYTES {
        return Err("invalid candidate_answer".into());
    }
    let session_id = params.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err("invalid session_id".into());
    }

    let vault_h = vault.inner().clone();
    let mut session = load_session(&vault_h, &session_id)?;
    let outcome = apply_candidate_answer(&mut session, &answer)?;
    let outcome_label = match &outcome {
        AdvanceOutcome::ContinueQuestion => "continue",
        AdvanceOutcome::EnteredStage(s) => match s {
            InterviewStage::Pressure => "entered_pressure",
            InterviewStage::Debrief => "entered_debrief",
            _ => "entered_stage",
        },
        AdvanceOutcome::CircuitBreakToDebrief => "circuit_break_debrief",
        AdvanceOutcome::Closed => "closed",
    };

    if matches!(outcome, AdvanceOutcome::Closed) {
        persist_session(&vault_h, &session)?;
        return Ok(MultistageInterviewResult {
            session_id,
            stage: session.stage.as_str().into(),
            status: session.status,
            turn_in_stage: session.turn_in_stage,
            total_turns: session.total_turns,
            context_ids: vec![],
            company_name: session.company_name,
            outcome: outcome_label.into(),
            oni_active: session.oni_active,
        });
    }

    // Only reached when the session actually continues (not Closed above),
    // i.e. only when generate() will really be called — placed after the
    // early-return branch so a Closed session never pays for a load check.
    ensure_model_loaded(&app, &llm).await?;

    let facts: CompanyFacts = serde_json::from_str(&session.facts_json)
        .map_err(|_| "stored facts parse failed".to_string())?;
    let (context_limit, mut gen) =
        resolve_coliseum_gen(params.gen.as_ref(), artifact_turn_seed(&session));
    let llm_s = llm.inner().clone();
    let governor = llm.inner().governor();
    let session_snap = session.clone();
    let answer_c = answer.clone();
    let facts_c = facts.clone();
    let vault_c = vault_h.clone();
    let n_ctx = gen.n_ctx;
    let max_tokens = gen.max_tokens;

    let (prompt, context_ids, verified_tokens) = tauri::async_runtime::spawn_blocking(move || {
        let (prompt, ids) = build_machine_prompt(
            &vault_c,
            &llm_s,
            &session_snap,
            &facts_c,
            &answer_c,
            context_limit,
        )?;
        let (prompt, verified_tokens) = fit_and_verify_prompt(
            &llm_s,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens,
            USER_TURN_MARKERS,
            "interview(multistage-advance)",
        );
        Ok::<_, String>((prompt, ids, verified_tokens))
    })
    .await
    .map_err(|e| {
        log::error!("interview(multistage-advance): prompt-build task join failed: {e}");
        "multistage advance join failed".to_string()
    })?
    .map_err(|e| {
        log::error!("interview(multistage-advance): prompt-build task returned error: {e}");
        e
    })?;

    persist_session(&vault_h, &session)?;
    gen.prompt = prompt;
    log::info!(
        "interview(multistage-advance): calling llm.generate (prompt_len={}, prompt_tokens={:?}, n_ctx={}, max_tokens={})",
        gen.prompt.len(),
        verified_tokens,
        gen.n_ctx,
        gen.max_tokens
    );
    llm.generate(gen, None, on_token).await.map_err(|e| {
        log::error!("interview(multistage-advance): llm.generate failed: {e}");
        e
    })?;
    log::info!("interview(multistage-advance): llm.generate completed successfully");

    Ok(MultistageInterviewResult {
        session_id,
        stage: session.stage.as_str().into(),
        status: session.status,
        turn_in_stage: session.turn_in_stage,
        total_turns: session.total_turns,
        context_ids,
        company_name: session.company_name,
        outcome: outcome_label.into(),
        oni_active: session.oni_active,
    })
}

#[tauri::command]
pub async fn get_interview_session(
    vault: State<'_, VaultHandle>,
    session_id: String,
) -> Result<InterviewSession, String> {
    let id = session_id.trim().to_string();
    if id.is_empty() {
        return Err("invalid session_id".into());
    }
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || load_session(&vault, &id))
        .await
        .map_err(|_| "get_interview_session join failed".to_string())?
}

/// Company-namespace dashboard analysis (delayed evaluation; never auto-runs).
#[tauri::command]
pub async fn analyze_company_knowledge(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    company_name: String,
    company_facts: Option<CompanyFacts>,
    on_token: Channel<TokenEvent>,
) -> Result<SimSessionResult, String> {
    let company_name = company_name.trim().to_string();
    if company_name.is_empty() || company_name.len() > MAX_TEXT_BYTES {
        return Err("invalid company_name".into());
    }
    ensure_model_loaded(&app, &llm).await?;

    let (_, mut gen) = resolve_gen(None);
    // The dashboard asks for 4 sections x 3-5 bullets. `DEFAULT_MAX_TOKENS`
    // (256) is sized for a single conversational turn and cut the last section
    // off mid-word on device ("- トヨタが20", 2026-07-25). Raise the output
    // budget and n_ctx together: `fit_and_verify_prompt` derives the *input*
    // budget as `n_ctx - max_tokens`, so lifting max_tokens alone would shrink
    // the room left for the company chunks being analysed.
    gen.max_tokens = ANALYSIS_MAX_TOKENS;
    gen.n_ctx = ANALYSIS_N_CTX;
    let vault_h = vault.inner().clone();
    let llm_fit = llm.inner().clone();
    let governor = llm.inner().governor();
    let n_ctx = gen.n_ctx;
    let max_tokens = gen.max_tokens;
    let name_for_search = company_name.clone();
    let current_facts = company_facts
        .and_then(|facts| sanitize_company_facts(&facts).ok())
        .filter(has_company_knowledge);

    let (prompt, context_ids, verified_tokens, facts_source) =
        tauri::async_runtime::spawn_blocking(move || {
            let edinet_code = current_facts
                .as_ref()
                .map(|facts| facts.edinet_code.as_str())
                .unwrap_or("");
            let hits = search_sync(
                &vault_h,
                &llm_fit,
                &name_for_search,
                8,
                KnowledgeNamespace::Company,
            )?;
            let hits = select_company_hits(hits, &name_for_search, edinet_code);

            // The editor preview and the dashboard used to read different data
            // sources: React held freshly fetched facts while the dashboard only
            // searched the asynchronously persisted Vault. Include the currently
            // displayed, sanitized facts as the primary context so an ingest race
            // or an absent EDINET code cannot produce a false "no data" state.
            let mut contexts: Vec<(String, String)> = Vec::new();
            if let Some(facts) = current_facts.as_ref() {
                contexts.push((
                    "current-company-facts".to_string(),
                    render_company_facts_block(facts),
                ));
            }
            contexts.extend(
                hits.iter()
                    .map(|hit| (hit.id.clone(), hit.text_content.clone())),
            );
            if contexts.is_empty() {
                return Ok((
                    String::new(),
                    Vec::<String>::new(),
                    None::<usize>,
                    String::new(),
                ));
            }
            let refs: Vec<ExperienceRef<'_>> = contexts
                .iter()
                .map(|(id, text)| ExperienceRef {
                    id: id.as_str(),
                    text: text.as_str(),
                })
                .collect();
            let prompt = build_company_analysis_prompt(&name_for_search, &refs);
            let (prompt, verified_tokens) = fit_and_verify_prompt(
                &llm_fit,
                governor.as_ref(),
                prompt,
                n_ctx,
                max_tokens,
                &["## 分析対象"],
                "company_analysis",
            );
            let ids: Vec<String> = contexts.into_iter().map(|(id, _)| id).collect();
            let facts_source = match (current_facts.is_some(), hits.is_empty()) {
                (true, false) => "current_state+company_vault",
                (true, true) => "current_state",
                (false, false) => "company_vault",
                (false, true) => "",
            }
            .to_string();
            Ok::<_, String>((prompt, ids, verified_tokens, facts_source))
        })
        .await
        .map_err(|e| {
            log::error!("company_analysis: prompt-build join failed: {e}");
            "company_analysis join failed".to_string()
        })?
        .map_err(|e| {
            log::error!("company_analysis: prompt-build error: {e}");
            e
        })?;

    if context_ids.is_empty() {
        return Ok(SimSessionResult {
            context_ids,
            context_count: 0,
            company_name,
            facts_source: String::new(),
        });
    }

    gen.prompt = prompt;
    log::info!(
        "company_analysis: llm.generate (prompt_len={}, prompt_tokens={:?})",
        gen.prompt.len(),
        verified_tokens
    );
    llm.generate(gen, None, on_token).await.map_err(|e| {
        log::error!("company_analysis: generate failed: {e}");
        e
    })?;

    Ok(SimSessionResult {
        context_count: context_ids.len(),
        context_ids,
        company_name,
        facts_source,
    })
}

/// Personal-namespace session memory (background; no FE stream).
#[tauri::command]
pub async fn ingest_session_memory(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    transcript: String,
    session_kind: String,
) -> Result<IngestKnowledgeResult, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use tauri::ipc::InvokeResponseBody;

    use crate::db::knowledge_namespace::{namespace_of, KnowledgeNamespace as Ns};

    let transcript = transcript.trim().to_string();
    if transcript.is_empty() || transcript.len() > MAX_TEXT_BYTES {
        return Err("invalid transcript".into());
    }
    let session_kind = session_kind.trim().to_string();
    if session_kind != "interview" && session_kind != "consult" {
        return Err("invalid session_kind".into());
    }
    ensure_model_loaded(&app, &llm).await?;

    let (_, mut gen) = resolve_gen(None);
    let llm_fit = llm.inner().clone();
    let governor = llm.inner().governor();
    let n_ctx = gen.n_ctx;
    let max_tokens = gen.max_tokens;
    let kind_for_prompt = session_kind.clone();
    let transcript_for_prompt = transcript.clone();

    let (prompt, verified_tokens) = tauri::async_runtime::spawn_blocking(move || {
        let prompt = build_session_memory_prompt(&kind_for_prompt, &transcript_for_prompt);
        let (prompt, verified_tokens) = fit_and_verify_prompt(
            &llm_fit,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens,
            &["## 対話ログ"],
            "session_memory",
        );
        Ok::<_, String>((prompt, verified_tokens))
    })
    .await
    .map_err(|e| {
        log::error!("session_memory: prompt-build join failed: {e}");
        "session_memory join failed".to_string()
    })?
    .map_err(|e| {
        log::error!("session_memory: prompt-build error: {e}");
        e
    })?;

    let acc = Arc::new(Mutex::new(String::new()));
    let failed = Arc::new(AtomicBool::new(false));
    let sink = Arc::clone(&acc);
    let fail_flag = Arc::clone(&failed);
    let collector = Channel::new(move |body| {
        if fail_flag.load(Ordering::SeqCst) {
            return Ok(());
        }
        match body {
            InvokeResponseBody::Json(json) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
                    return Ok(());
                };
                if value.get("error").map(|e| !e.is_null()).unwrap_or(false) {
                    fail_flag.store(true, Ordering::SeqCst);
                    if let Ok(mut g) = sink.lock() {
                        g.clear();
                    }
                    return Ok(());
                }
                if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        if let Ok(mut g) = sink.lock() {
                            g.push_str(text);
                        }
                    }
                }
            }
            InvokeResponseBody::Raw(_) => {}
        }
        Ok(())
    });

    gen.prompt = prompt;
    log::info!(
        "session_memory: llm.generate (prompt_len={}, prompt_tokens={:?})",
        gen.prompt.len(),
        verified_tokens
    );
    llm.generate(gen, None, collector).await.map_err(|e| {
        log::error!("session_memory: generate failed: {e}");
        e
    })?;

    if failed.load(Ordering::SeqCst) {
        return Ok(IngestKnowledgeResult {
            source_id: String::new(),
            chunk_count: 0,
            inserted: 0,
            truncated: false,
            part_count: 0,
        });
    }

    let memory_text = acc
        .lock()
        .map_err(|_| "session_memory accumulator lock poisoned".to_string())?
        .clone();
    if memory_text.trim().is_empty() {
        return Ok(IngestKnowledgeResult {
            source_id: String::new(),
            chunk_count: 0,
            inserted: 0,
            truncated: false,
            part_count: 0,
        });
    }

    let sid = memory_source_id(&session_kind, &transcript);
    validate_source_id(&sid)?;
    if namespace_of(&format!("{sid}::0000")) != Ns::Personal {
        return Err("memory source_id namespace check failed".into());
    }

    let vault_h = vault.inner().clone();
    let llm_h = llm.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ingest_text_incremental_blocking(&vault_h, &llm_h, &memory_text, &sid)
    })
    .await
    .map_err(|_| "session_memory ingest join failed".to_string())?
}

/// `memory-{kind}-{yyyy-mm-dd}-{sha256[:16]}` — must stay Personal (`memory-` prefix).
pub(crate) fn memory_source_id(session_kind: &str, transcript: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(transcript.as_bytes()));
    let short = digest.get(..16).unwrap_or("0000000000000000");
    let date = utc_ymd_today();
    format!("memory-{session_kind}-{date}-{short}")
}

/// UTC calendar date YYYY-MM-DD (no chrono dependency).
fn utc_ymd_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil date from Unix day count (Howard Hinnant).
    let z = (secs / 86_400).saturating_add(719_468);
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod memory_id_tests {
    use super::memory_source_id;
    use crate::db::knowledge_namespace::{namespace_of, KnowledgeNamespace};

    #[test]
    fn memory_source_id_is_personal_not_company() {
        let sid = memory_source_id("interview", "hello transcript");
        assert!(sid.starts_with("memory-interview-"));
        assert_eq!(
            namespace_of(&format!("{sid}::0000")),
            KnowledgeNamespace::Personal
        );
        // Trap: knowledge- is Company — never use for session memory.
        assert_eq!(
            namespace_of("knowledge-interview-2026-07-25-deadbeef::0000"),
            KnowledgeNamespace::Company
        );
    }
}

#[cfg(test)]
mod company_identity_tests {
    use super::{company_keys_match, company_source_matches};

    #[test]
    fn short_name_matches_normalized_official_name() {
        assert!(company_keys_match("トヨタ", "トヨタ自動車株式会社"));
        assert!(company_keys_match(" 株式会社 サンプル ", "サンプル"));
    }

    #[test]
    fn unrelated_company_names_do_not_match() {
        assert!(!company_keys_match("トヨタ", "本田技研工業株式会社"));
    }

    #[test]
    fn source_matches_alias_or_stable_edinet_code() {
        assert!(company_source_matches(
            "wiki-トヨタ自動車::0002",
            "トヨタ",
            ""
        ));
        assert!(company_source_matches(
            "edinet-E02144::0001",
            "トヨタ",
            "e02144"
        ));
        assert!(!company_source_matches(
            "edinet-E00001::0001",
            "トヨタ",
            "E02144"
        ));
    }
}

#[cfg(test)]
mod edinet_v3_wire_tests {
    use super::*;

    #[test]
    fn request_subject_and_fact_cells_round_trip() {
        let request = EdinetEnrichmentRequestV3 {
            schema_version: 3,
            subject_key: SubjectKey::Edinet("E02144".into()),
            subject_revision: 7,
            subject_transition: Some(SubjectTransition::Rekey {
                from: SubjectKey::Name("トヨタ自動車".into()),
                to: SubjectKey::Edinet("E02144".into()),
            }),
            fact_cells: Some(vec![FactCellWire {
                field: "business_summary".into(),
                value: "value".into(),
                origin: FactOriginWire::Edinet,
                storage: FactStorageWire::Vault,
                doc_id: Some("S100TEST".into()),
                submitted_at: Some("2026-07-25 15:00".into()),
                fetched_at: None,
                revision: 7,
                schema_version: 3,
            }]),
            company_facts: None,
            edinet_code: Some("E02144".into()),
            edinet_date: Some("2026-07-26".into()),
            filing_text: None,
        };
        let value = serde_json::to_value(&request).expect("serialize request");
        let decoded: EdinetEnrichmentRequestV3 =
            serde_json::from_value(value.clone()).expect("deserialize request");
        assert_eq!(decoded.schema_version, 3);
        assert_eq!(decoded.subject_revision, 7);
        assert_eq!(decoded.fact_cells, request.fact_cells);
        assert_eq!(value["subjectKey"], "edinet:E02144");
        assert_eq!(value["subjectTransition"]["kind"], "rekey");
    }

    #[test]
    fn initial_full_ui_facts_are_promoted_without_loss() {
        let facts = CompanyFacts {
            company_name: "  株式会社テスト  ".into(),
            business_summary: "事業概要".into(),
            business_risks: "事業リスク".into(),
            performance_summary: "業績".into(),
            source: "wikipedia".into(),
            ..CompanyFacts::default()
        };
        let sanitized = sanitize_company_facts(&facts).expect("sanitize");
        let cells = protected_cells_from_company_facts(&sanitized);
        let displayed = display_company_facts(&cells, &CompanyFacts::default());
        assert_eq!(displayed.business_summary, "事業概要");
        assert_eq!(displayed.business_risks, "事業リスク");
        assert_eq!(displayed.performance_summary, "業績");
    }

    #[test]
    fn discovery_served_from_distinguishes_cache_mixed_and_live() {
        assert_eq!(served_from_for_discovery(4, 0), ServedFrom::Cache);
        assert_eq!(served_from_for_discovery(4, 3), ServedFrom::Mixed);
        assert_eq!(served_from_for_discovery(0, 3), ServedFrom::Live);
    }

    #[test]
    fn stale_request_revision_is_detected_before_acquisition() {
        assert!(stale_subject_revision(3, 4));
        assert!(!stale_subject_revision(4, 4));
    }

    #[test]
    fn switch_base_keeps_only_new_company_name_as_manual() {
        let facts = CompanyFacts {
            company_name: "新会社".into(),
            business_summary: "旧会社の概要".into(),
            business_risks: "旧会社のリスク".into(),
            performance_summary: "旧会社の業績".into(),
            ..CompanyFacts::default()
        };
        let cells = switch_base_from_company_facts(Some(&facts));
        assert_eq!(cells.len(), 1);
        let company = cells.get("company_name").expect("company name");
        assert_eq!(company.value, "新会社");
        assert_eq!(company.origin, FactOrigin::Manual);
    }

    #[test]
    fn rust_normalizer_matches_shared_frontend_golden() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../src/lib/filerNameNormalizationGolden.json"
        ))
        .expect("golden json");
        for vector in vectors.as_array().expect("golden array") {
            let input = vector["input"].as_str().expect("input");
            let expected = vector["normalized"].as_str().expect("normalized");
            let sanitized = sanitize_company_facts(&CompanyFacts {
                company_name: input.into(),
                ..CompanyFacts::default()
            })
            .expect("sanitize");
            assert_eq!(normalize_filer_key(&sanitized.company_name), expected);
        }
    }
}

/// Step 1 contract: soft-fallback / gate / name-only base (no live network).
/// The instrumented path uses [`resolve_company_facts_with`] with injected key
/// provider + lazy transport factory + fake `HttpTransport`, so no test ever
/// touches the real env key or Reqwest. Every scenario asserts the exact
/// call-count contract and `assert_eq!(out, before)`.
#[cfg(test)]
mod edinet_step1_fallback_contract_tests {
    use super::{resolve_company_facts_with, soft_fallback_named_base, CompanyFacts};
    #[cfg(all(feature = "egress-live", target_vendor = "apple"))]
    use crate::knowledge::edinet_client::EdinetDocumentMeta;
    use crate::knowledge::edinet_client::{sanitize_company_facts, EdinetError};
    #[cfg(all(feature = "egress-live", target_vendor = "apple"))]
    use crate::knowledge::edinet_discovery::{
        CoverageDayStatus, CoverageRecord, DayCommit, VaultDiscoveryCache,
    };
    use crate::knowledge::net_gateway::{GatewayError, HttpTransport, ResponseBody, ResponseMeta};
    use crate::knowledge::NetworkPolicyStore;
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Holds the `TempDir` for the store's whole lifetime. OS-guaranteed unique
    /// path (no process-local counter) so parallel cargo-test processes across
    /// feature configs never collide on the same policy-store directory.
    struct StoreFixture {
        _dir: TempDir,
        store: NetworkPolicyStore,
    }

    fn isolated_store() -> StoreFixture {
        let dir = tempfile::Builder::new()
            .prefix("pkb-edinet-step1-")
            .tempdir()
            .expect("unique temp dir");
        let store = NetworkPolicyStore::from_root(dir.path().to_path_buf());
        StoreFixture { _dir: dir, store }
    }

    fn wiki_base_raw() -> CompanyFacts {
        CompanyFacts {
            company_name: "トヨタ自動車".into(),
            edinet_code: String::new(),
            doc_id: String::new(),
            business_summary: "自動車の製造・販売".into(),
            business_risks: String::new(),
            performance_summary: String::new(),
            source: "wikipedia".into(),
        }
    }

    /// Sanitized base — soft-fallback returns this shape (`assert_eq!(out, before)`).
    fn wiki_before() -> CompanyFacts {
        sanitize_company_facts(&wiki_base_raw()).expect("wiki sanitize")
    }

    struct CountingFailTransport {
        gets: Arc<AtomicUsize>,
    }

    struct EmptyBody;

    impl ResponseBody for EmptyBody {
        fn next_chunk(
            &mut self,
        ) -> impl Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send {
            async { None }
        }
    }

    impl HttpTransport for CountingFailTransport {
        type Body = EmptyBody;

        fn get(
            &self,
            _url: &str,
            _request_deadline: std::time::Duration,
        ) -> impl Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send {
            self.gets.fetch_add(1, Ordering::SeqCst);
            async { Err(GatewayError::StatusRejected) }
        }
    }

    struct CallCounters {
        key: Arc<AtomicUsize>,
        factory: Arc<AtomicUsize>,
        gets: Arc<AtomicUsize>,
    }

    impl CallCounters {
        fn new() -> Self {
            Self {
                key: Arc::new(AtomicUsize::new(0)),
                factory: Arc::new(AtomicUsize::new(0)),
                gets: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn snapshot(&self) -> (usize, usize, usize) {
            (
                self.key.load(Ordering::SeqCst),
                self.factory.load(Ordering::SeqCst),
                self.gets.load(Ordering::SeqCst),
            )
        }
    }

    /// Counting key provider that yields a fixed fake key.
    fn ok_key(counts: &CallCounters) -> impl FnMut() -> Result<String, EdinetError> {
        let key_c = Arc::clone(&counts.key);
        move || {
            key_c.fetch_add(1, Ordering::SeqCst);
            Ok("test-subscription-key".into())
        }
    }

    /// Counting transport factory that yields a fake counting transport.
    fn ok_factory(counts: &CallCounters) -> impl FnMut() -> Result<CountingFailTransport, String> {
        let factory_c = Arc::clone(&counts.factory);
        let gets_c = Arc::clone(&counts.gets);
        move || {
            factory_c.fetch_add(1, Ordering::SeqCst);
            Ok(CountingFailTransport {
                gets: Arc::clone(&gets_c),
            })
        }
    }

    #[test]
    fn soft_fallback_keeps_named_base_never_empty_default() {
        let kept = soft_fallback_named_base(Some(wiki_before()), "boom").expect("base");
        assert_eq!(kept, wiki_before());
        assert!(soft_fallback_named_base(None, "boom").is_err());
        assert!(soft_fallback_named_base(
            Some(CompanyFacts {
                company_name: "   ".into(),
                ..CompanyFacts::default()
            }),
            "boom"
        )
        .is_err());
    }

    #[test]
    fn name_only_default_is_valid_sanitize_and_fallback_input() {
        let raw = CompanyFacts {
            company_name: "トヨタ自動車".into(),
            ..CompanyFacts::default()
        };
        let sanitized = sanitize_company_facts(&raw).expect("name-only sanitize");
        assert_eq!(sanitized.company_name, "トヨタ自動車");
        assert!(sanitized.business_summary.is_empty());
        let kept = soft_fallback_named_base(Some(sanitized.clone()), "x").expect("fallback");
        assert_eq!(kept, sanitized);
    }

    #[tokio::test]
    async fn policy_off_zero_key_factory_and_http() {
        let fx = isolated_store();
        assert!(!fx.store.enabled());
        let before = wiki_before();
        let counts = CallCounters::new();

        let out = resolve_company_facts_with(
            &fx.store,
            None,
            Some(wiki_base_raw()),
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            ok_factory(&counts),
        )
        .await
        .expect("soft fallback");

        assert_eq!(out, before);
        assert_eq!(
            counts.snapshot(),
            (0, 0, 0),
            "policy off: key/factory/get must be 0"
        );
    }

    /// egress-live OFF: even with consent ON + key present + factory available,
    /// the second egress factor closes the gate → key/factory/get all 0.
    #[cfg(not(feature = "egress-live"))]
    #[tokio::test]
    async fn egress_disabled_consent_on_still_zero_key_factory_and_http() {
        let fx = isolated_store();
        fx.store.set_enabled(true).expect("enable");
        let before = wiki_before();
        let counts = CallCounters::new();

        let out = resolve_company_facts_with(
            &fx.store,
            None,
            Some(wiki_base_raw()),
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            ok_factory(&counts),
        )
        .await
        .expect("soft fallback");

        assert_eq!(out, before);
        assert_eq!(
            counts.snapshot(),
            (0, 0, 0),
            "egress-live off: consent alone must not reach key/factory/get"
        );
    }

    /// egress-live ON: consent ON + key missing → key consulted, factory/get 0.
    #[cfg(all(feature = "egress-live", target_vendor = "apple"))]
    #[tokio::test]
    async fn api_key_missing_zero_factory_and_http() {
        let fx = isolated_store();
        fx.store.set_enabled(true).expect("enable");
        let before = wiki_before();
        let counts = CallCounters::new();
        let key_c = Arc::clone(&counts.key);

        let out = resolve_company_facts_with(
            &fx.store,
            None,
            Some(wiki_base_raw()),
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            move || {
                key_c.fetch_add(1, Ordering::SeqCst);
                Err(EdinetError::ApiKeyMissing)
            },
            ok_factory(&counts),
        )
        .await
        .expect("soft fallback");

        assert_eq!(out, before);
        assert_eq!(
            counts.key.load(Ordering::SeqCst),
            1,
            "key provider must be consulted"
        );
        assert_eq!(
            counts.factory.load(Ordering::SeqCst),
            0,
            "no factory when key missing"
        );
        assert_eq!(
            counts.gets.load(Ordering::SeqCst),
            0,
            "no HTTP when key missing"
        );
    }

    #[cfg(feature = "egress-live")]
    #[tokio::test]
    async fn transport_new_failure_preserves_wikipedia_exact() {
        let fx = isolated_store();
        fx.store.set_enabled(true).expect("enable");
        let before = wiki_before();
        let counts = CallCounters::new();
        let factory_c = Arc::clone(&counts.factory);

        let out = resolve_company_facts_with(
            &fx.store,
            None,
            Some(wiki_base_raw()),
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            move || {
                factory_c.fetch_add(1, Ordering::SeqCst);
                Err::<CountingFailTransport, _>("edinet_transport_unavailable".into())
            },
        )
        .await
        .expect("soft fallback");

        assert_eq!(out, before);
        assert_eq!(counts.key.load(Ordering::SeqCst), 1);
        assert_eq!(counts.factory.load(Ordering::SeqCst), 1);
        assert_eq!(
            counts.gets.load(Ordering::SeqCst),
            0,
            "failed factory must not HTTP"
        );
    }

    #[cfg(feature = "egress-live")]
    #[tokio::test]
    async fn http_fetch_failure_preserves_wikipedia_exact_one_get() {
        let fx = isolated_store();
        fx.store.set_enabled(true).expect("enable");
        let before = wiki_before();
        let counts = CallCounters::new();

        // Explicit edinet_code → single list GET (not name lookback loop).
        let out = resolve_company_facts_with(
            &fx.store,
            None,
            Some(wiki_base_raw()),
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            ok_factory(&counts),
        )
        .await
        .expect("soft fallback");

        assert_eq!(out, before);
        assert_eq!(counts.key.load(Ordering::SeqCst), 1);
        assert_eq!(counts.factory.load(Ordering::SeqCst), 1);
        assert_eq!(
            counts.gets.load(Ordering::SeqCst),
            1,
            "by-code fetch: exactly one GET"
        );
    }

    #[cfg(feature = "egress-live")]
    #[tokio::test]
    async fn vault_discovery_prefix_gap_preserves_base_without_auto_merge() {
        let fx = isolated_store();
        fx.store.set_enabled(true).expect("enable");
        let before = wiki_before();
        let counts = CallCounters::new();
        let vault = crate::db::VaultHandle::spawn_test_unlocked().expect("test vault");
        vault
            .edinet_commit_day(DayCommit {
                coverage: CoverageRecord {
                    subject_key: "edinet:E02144".into(),
                    date: "2026-07-25".into(),
                    status: CoverageDayStatus::Ok,
                    fetched_at: 1,
                    process_date_time: None,
                    error_class: None,
                    revalidate_after: None,
                },
                filings: vec![EdinetDocumentMeta {
                    doc_id: Some("S100GAP".into()),
                    edinet_code: Some("E02144".into()),
                    filer_name: Some("トヨタ自動車".into()),
                    doc_type_code: Some("120".into()),
                    submit_date_time: Some("2026-07-25 15:00".into()),
                    withdrawal_status: Some("0".into()),
                    disclosure_status: Some("0".into()),
                    xbrl_flag: Some("1".into()),
                    legal_status: Some("1".into()),
                    ..EdinetDocumentMeta::default()
                }],
            })
            .expect("seed vault filing");
        let mut cache = VaultDiscoveryCache::new(&vault);

        let out = resolve_company_facts_with(
            &fx.store,
            Some(&mut cache),
            Some(wiki_base_raw()),
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            ok_factory(&counts),
        )
        .await
        .expect("gap must soft-fallback");

        assert_eq!(out, before);
        assert_eq!(counts.snapshot(), (1, 1, 1));
    }

    #[tokio::test]
    async fn without_base_policy_off_is_egress_not_ready() {
        let fx = isolated_store();
        let counts = CallCounters::new();

        let err = resolve_company_facts_with(
            &fx.store,
            None,
            None,
            Some("E02144".into()),
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            ok_factory(&counts),
        )
        .await
        .expect_err("no base");
        assert_eq!(err, "EGRESS_LIVE_NOT_READY");
        assert_eq!(counts.snapshot(), (0, 0, 0));
    }

    #[tokio::test]
    async fn invalid_short_filer_name_keeps_injected_base() {
        let fx = isolated_store();
        fx.store.set_enabled(true).expect("enable");
        let raw = CompanyFacts {
            company_name: "あ".into(),
            business_summary: "必ず残る要約".into(),
            source: "wikipedia".into(),
            ..CompanyFacts::default()
        };
        let before = sanitize_company_facts(&raw).expect("sanitize");
        let counts = CallCounters::new();
        let out = resolve_company_facts_with(
            &fx.store,
            None,
            Some(raw),
            None,
            Some("2026-07-26".into()),
            None,
            ok_key(&counts),
            ok_factory(&counts),
        )
        .await
        .expect("soft fallback on short filer key");
        assert_eq!(out, before);
        assert_eq!(out.business_summary, "必ず残る要約");
    }
}

/// Step 12 — outer fallback exit unification (no live network).
#[cfg(test)]
mod edinet_step12_outer_fallback_tests {
    use super::{
        enrich_company_facts_from_edinet_core, fallback_enrichment_response,
        identity_mismatch_response, persistence_response, resolve_failure, wires_from_cells,
        DiscoveryCoverage, DiscoveryResult, EdinetEnrichmentRequestV3, EdinetWarningCode,
        ExtractionStatus, FactPersistence, FetchStatus, ResolvedEdinetAcquisition, ServedFrom,
        Tristate,
    };
    use crate::db::VaultHandle;
    use crate::knowledge::edinet_client::{
        sanitize_company_facts, CompanyFacts, EdinetFactAcquisition, EdinetFieldAcquisition,
    };
    use crate::knowledge::fact_merge::{
        cells_from_edinet_acquisition, merge_fact_cells, protected_cells_from_company_facts,
        FactCells, SubjectKey,
    };
    use crate::knowledge::NetworkPolicyStore;
    use crate::llm::LlmHandle;
    use tempfile::TempDir;

    fn named_base_cells() -> (CompanyFacts, FactCells) {
        let facts = sanitize_company_facts(&CompanyFacts {
            company_name: "トヨタ自動車".into(),
            business_summary: "wikiの事業概要".into(),
            business_risks: "wikiのリスク".into(),
            source: "wikipedia".into(),
            ..CompanyFacts::default()
        })
        .expect("sanitize");
        let cells = protected_cells_from_company_facts(&facts);
        (facts, cells)
    }

    #[test]
    fn fallback_response_transcribes_inner_status_and_keeps_base() {
        let (facts, base) = named_base_cells();
        let failure = resolve_failure("edinet_not_ready", FetchStatus::NotAttempted);
        let subject = SubjectKey::try_from("name:トヨタ自動車".to_string()).expect("key");
        let response = fallback_enrichment_response(&base, 3, subject, &failure, &[]);
        assert_eq!(response.fetch, FetchStatus::NotAttempted);
        assert_eq!(response.extraction, ExtractionStatus::None);
        assert_eq!(response.fact_persistence, FactPersistence::NotAttempted);
        assert!(response.warnings.contains(&EdinetWarningCode::SoftFallback));
        assert_eq!(response.facts.business_summary, facts.business_summary);
        assert_eq!(response.facts.business_risks, facts.business_risks);
        assert!(response.freshness.selected_doc_id.is_none());
    }

    #[test]
    fn identity_mismatch_uses_base_not_merged_and_clears_freshness() {
        let (facts, base) = named_base_cells();
        let acquisition = EdinetFactAcquisition::new(
            CompanyFacts {
                company_name: "他社".into(),
                edinet_code: "E02144".into(),
                doc_id: "S100TEST1".into(),
                business_summary: "EDINET側".into(),
                business_risks: String::new(),
                performance_summary: "業績".into(),
                source: "edinet_zip".into(),
            },
            "S100TEST1".into(),
            "2024-06-25 15:00".into(),
            "E02144".into(),
            EdinetFieldAcquisition {
                business_summary: true,
                business_risks: false,
                performance_summary: true,
            },
        )
        .expect("acq");
        let resolved = ResolvedEdinetAcquisition {
            acquisition,
            coverage: DiscoveryCoverage::WindowComplete,
            result: DiscoveryResult::Selected,
            served_from: ServedFrom::Live,
            correction_available: Tristate::No,
            fetch: FetchStatus::Succeeded,
            extraction: ExtractionStatus::Both,
        };
        let subject = SubjectKey::try_from("edinet:E99999".to_string()).expect("key");
        let response = identity_mismatch_response(&base, 1, subject, &resolved, &[]);
        assert_eq!(response.discovery_result, DiscoveryResult::IdentityAmbiguous);
        assert_eq!(response.fact_persistence, FactPersistence::NotAttempted);
        assert_eq!(response.facts.business_summary, facts.business_summary);
        assert_ne!(response.facts.business_summary, "EDINET側");
        assert!(response.freshness.selected_doc_id.is_none());
        assert!(response.warnings.contains(&EdinetWarningCode::SoftFallback));
    }

    #[tokio::test]
    async fn input_rejection_schema_version_still_errs() {
        let vault = VaultHandle::unavailable();
        let dir = TempDir::new().expect("temp");
        let store = NetworkPolicyStore::from_root(dir.path().to_path_buf());
        let llm = LlmHandle::unavailable("test".into());
        let err = match enrich_company_facts_from_edinet_core(
            &vault,
            &store,
            &llm,
            None,
            EdinetEnrichmentRequestV3 {
                schema_version: 2,
                subject_key: SubjectKey::try_from("name:x".to_string()).expect("key"),
                subject_revision: 0,
                subject_transition: None,
                fact_cells: None,
                company_facts: None,
                edinet_code: None,
                edinet_date: None,
                filing_text: None,
            },
        )
        .await
        {
            Ok(_) => panic!("input rejection must Err"),
            Err(message) => message,
        };
        assert_eq!(err, "invalid_v3_request");
    }

    #[tokio::test]
    async fn v3_keeps_base_when_edinet_not_ready() {
        let vault = VaultHandle::unavailable();
        let dir = TempDir::new().expect("temp");
        let store = NetworkPolicyStore::from_root(dir.path().to_path_buf());
        let llm = LlmHandle::unavailable("test".into());
        let (before, _) = named_base_cells();
        let subject = SubjectKey::try_from(format!(
            "name:{}",
            crate::knowledge::edinet_client::normalize_filer_key(&before.company_name)
        ))
        .expect("key");
        let outcome = enrich_company_facts_from_edinet_core(
            &vault,
            &store,
            &llm,
            None,
            EdinetEnrichmentRequestV3 {
                schema_version: 3,
                subject_key: subject,
                subject_revision: 0,
                subject_transition: None,
                fact_cells: None,
                company_facts: Some(before.clone()),
                edinet_code: Some("E02144".into()),
                edinet_date: Some("2026-07-26".into()),
                filing_text: None,
            },
        )
        .await
        .expect("fallback ok");
        assert!(outcome.legacy_error.is_none());
        assert_eq!(outcome.response.facts.business_summary, before.business_summary);
        assert_eq!(outcome.response.facts.business_risks, before.business_risks);
        assert_eq!(outcome.response.fetch, FetchStatus::NotAttempted);
        assert_eq!(outcome.response.extraction, ExtractionStatus::None);
        assert_eq!(
            outcome.response.fact_persistence,
            FactPersistence::NotAttempted
        );
        assert!(outcome
            .response
            .warnings
            .contains(&EdinetWarningCode::SoftFallback));
        assert!(outcome
            .response
            .warnings
            .contains(&EdinetWarningCode::VaultReadFailed));
    }

    #[tokio::test]
    async fn empty_base_sets_legacy_error_for_old_command() {
        let vault = VaultHandle::unavailable();
        let dir = TempDir::new().expect("temp");
        let store = NetworkPolicyStore::from_root(dir.path().to_path_buf());
        let llm = LlmHandle::unavailable("test".into());
        let subject = SubjectKey::try_from("name:emptybase".to_string()).expect("key");
        let outcome = enrich_company_facts_from_edinet_core(
            &vault,
            &store,
            &llm,
            None,
            EdinetEnrichmentRequestV3 {
                schema_version: 3,
                subject_key: subject,
                subject_revision: 0,
                subject_transition: None,
                fact_cells: None,
                company_facts: None,
                edinet_code: Some("E02144".into()),
                edinet_date: Some("2026-07-26".into()),
                filing_text: None,
            },
        )
        .await
        .expect("v3 ok with empty facts");
        assert!(
            matches!(
                outcome.legacy_error.as_deref(),
                Some("edinet_not_ready") | Some("egress_unavailable")
            ),
            "legacy_error={:?}",
            outcome.legacy_error
        );
        assert_eq!(outcome.response.fetch, FetchStatus::NotAttempted);
        assert!(outcome.response.facts.company_name.is_empty());
        assert!(outcome
            .response
            .warnings
            .contains(&EdinetWarningCode::SoftFallback));
    }

    #[cfg(not(feature = "egress-live"))]
    #[tokio::test]
    async fn egress_off_v3_ok_not_attempted_keeps_base() {
        let vault = VaultHandle::unavailable();
        let dir = TempDir::new().expect("temp");
        let store = NetworkPolicyStore::from_root(dir.path().to_path_buf());
        store.set_enabled(true).expect("enable");
        let llm = LlmHandle::unavailable("test".into());
        let (before, _) = named_base_cells();
        let subject = SubjectKey::try_from(format!(
            "name:{}",
            crate::knowledge::edinet_client::normalize_filer_key(&before.company_name)
        ))
        .expect("key");
        let outcome = enrich_company_facts_from_edinet_core(
            &vault,
            &store,
            &llm,
            None,
            EdinetEnrichmentRequestV3 {
                schema_version: 3,
                subject_key: subject,
                subject_revision: 0,
                subject_transition: None,
                fact_cells: None,
                company_facts: Some(before.clone()),
                edinet_code: Some("E02144".into()),
                edinet_date: Some("2026-07-26".into()),
                filing_text: None,
            },
        )
        .await
        .expect("ok");
        assert!(outcome.legacy_error.is_none());
        assert_eq!(outcome.response.facts.business_summary, before.business_summary);
        assert_eq!(outcome.response.fetch, FetchStatus::NotAttempted);
        assert!(outcome
            .response
            .warnings
            .contains(&EdinetWarningCode::SoftFallback));
    }

    fn resolved_with_full_acquisition() -> ResolvedEdinetAcquisition {
        let acquisition = EdinetFactAcquisition::new(
            CompanyFacts {
                company_name: "トヨタ自動車".into(),
                edinet_code: "E02144".into(),
                doc_id: "S100TEST1".into(),
                business_summary: "EDINET側事業".into(),
                business_risks: "EDINET側リスク".into(),
                performance_summary: "EDINET側業績".into(),
                source: "edinet_zip".into(),
            },
            "S100TEST1".into(),
            "2024-06-25 15:00".into(),
            "E02144".into(),
            EdinetFieldAcquisition {
                business_summary: true,
                business_risks: true,
                performance_summary: true,
            },
        )
        .unwrap_or_else(|e| panic!("acq: {e:?}"));
        ResolvedEdinetAcquisition {
            acquisition,
            coverage: DiscoveryCoverage::WindowComplete,
            result: DiscoveryResult::Selected,
            served_from: ServedFrom::Live,
            correction_available: Tristate::No,
            fetch: FetchStatus::Succeeded,
            extraction: ExtractionStatus::Both,
        }
    }

    /// STEP_12_DESIGN.md 決定表 #10-#16: outer は inner の (fetch, extraction) を
    /// 無加工転記し、facts は常に base、persistence は not_attempted + SoftFallback。
    #[test]
    fn decision_table_rows_transcribe_exact_tuples() {
        let rows: [(&str, FetchStatus, ExtractionStatus); 4] = [
            // #10-#13 両側成果ゼロの構造系/parse系
            (
                "edinet_parse_failed",
                FetchStatus::Succeeded,
                ExtractionStatus::ParseFailed,
            ),
            // #14 memory pressure
            (
                "edinet_cancelled",
                FetchStatus::MemoryPressure,
                ExtractionStatus::Cancelled,
            ),
            // #15 acquire 失敗（purge 未完了 / headroom）
            (
                "edinet insufficient headroom",
                FetchStatus::MemoryPressure,
                ExtractionStatus::None,
            ),
            // #16 cancel / background / job deadline
            (
                "edinet_cancelled",
                FetchStatus::Cancelled,
                ExtractionStatus::Cancelled,
            ),
        ];
        let (facts, base) = named_base_cells();
        for (message, fetch, extraction) in rows {
            let mut failure = resolve_failure(message, fetch);
            failure.extraction = extraction;
            let subject = SubjectKey::try_from("edinet:E02144".to_string())
                .unwrap_or_else(|e| panic!("key: {e:?}"));
            let response = fallback_enrichment_response(&base, 7, subject, &failure, &[]);
            assert_eq!(response.fetch, fetch, "row {message}");
            assert_eq!(response.extraction, extraction, "row {message}");
            assert_eq!(
                response.fact_persistence,
                FactPersistence::NotAttempted,
                "row {message}"
            );
            assert!(response.warnings.contains(&EdinetWarningCode::SoftFallback));
            assert_eq!(response.facts.business_summary, facts.business_summary);
            assert_eq!(response.facts.business_risks, facts.business_risks);
            assert_eq!(response.subject_revision, 7);
        }
    }

    /// 決定表 #17: Vault 書込失敗 → failed + VaultWriteFailed + merged（旧 revision）。
    /// 「保存済みと表示しない」= revision 据え置き + failed が実装形。
    #[test]
    fn vault_write_failure_returns_merged_with_old_revision() {
        let (facts, base) = named_base_cells();
        let resolved = resolved_with_full_acquisition();
        let merged = merge_fact_cells(&base, &cells_from_edinet_acquisition(&resolved.acquisition));
        let vault = VaultHandle::unavailable();
        let subject = SubjectKey::try_from("edinet:E02144".to_string())
            .unwrap_or_else(|e| panic!("key: {e:?}"));
        let response = persistence_response(
            &vault,
            subject,
            4,
            FactPersistence::Failed,
            &merged,
            &resolved,
            wires_from_cells(&merged, 4),
            &[],
        );
        assert_eq!(response.fact_persistence, FactPersistence::Failed);
        assert!(response.warnings.contains(&EdinetWarningCode::VaultWriteFailed));
        assert_eq!(response.subject_revision, 4);
        // merged: EDINET 由来の新規フィールドは画面用に返る
        assert_eq!(response.facts.performance_summary, "EDINET側業績");
        // base の保護フィールド（wiki 由来の非空）は EDINET に上書きされない
        assert_eq!(response.facts.business_summary, facts.business_summary);
        assert_eq!(response.fetch, FetchStatus::Succeeded);
        assert_eq!(response.extraction, ExtractionStatus::Both);
    }

    /// 決定表 #17b: 保存成功後の read-back 失敗 → persisted のまま +
    /// revision+1 の wires（= CAS で書いた内容そのもの）+ VaultReadFailed。
    #[test]
    fn persisted_readback_failure_keeps_persisted_with_bumped_revision() {
        let (_, base) = named_base_cells();
        let resolved = resolved_with_full_acquisition();
        let merged = merge_fact_cells(&base, &cells_from_edinet_acquisition(&resolved.acquisition));
        let vault = VaultHandle::unavailable();
        let subject = SubjectKey::try_from("edinet:E02144".to_string())
            .unwrap_or_else(|e| panic!("key: {e:?}"));
        let response = persistence_response(
            &vault,
            subject,
            4,
            FactPersistence::Persisted,
            &merged,
            &resolved,
            wires_from_cells(&merged, 4),
            &[],
        );
        assert_eq!(response.fact_persistence, FactPersistence::Persisted);
        assert!(response.warnings.contains(&EdinetWarningCode::VaultReadFailed));
        assert!(!response.warnings.contains(&EdinetWarningCode::VaultWriteFailed));
        assert_eq!(response.subject_revision, 5);
        assert!(response
            .fact_cells
            .iter()
            .all(|cell| cell.revision == 5));
        assert_eq!(response.facts.performance_summary, "EDINET側業績");
    }

    /// 決定表 #17c: Conflict 応答時の read-back 失敗 → conflict のまま +
    /// 手元の既知 wires + VaultReadFailed（Conflict は FE 再試行制御に必要）。
    #[test]
    fn conflict_readback_failure_returns_known_wires() {
        let (facts, base) = named_base_cells();
        let resolved = resolved_with_full_acquisition();
        let merged = merge_fact_cells(&base, &cells_from_edinet_acquisition(&resolved.acquisition));
        let vault = VaultHandle::unavailable();
        let subject = SubjectKey::try_from("edinet:E02144".to_string())
            .unwrap_or_else(|e| panic!("key: {e:?}"));
        let response = persistence_response(
            &vault,
            subject,
            4,
            FactPersistence::Conflict,
            &merged,
            &resolved,
            wires_from_cells(&merged, 4),
            &[],
        );
        assert_eq!(response.fact_persistence, FactPersistence::Conflict);
        assert!(response.warnings.contains(&EdinetWarningCode::VaultReadFailed));
        assert_eq!(response.subject_revision, 4);
        // Conflict + read 不能では EDINET 側の値も base も知り得る限りで返す
        assert_eq!(response.facts.business_summary, facts.business_summary);
        assert_eq!(response.fetch, FetchStatus::NotAttempted);
        assert_eq!(response.extraction, ExtractionStatus::None);
    }
}

/// Step 11 — ZIP pipeline orchestration (fake transport; no live network).
#[cfg(all(test, feature = "egress-live"))]
mod edinet_step11_zip_pipeline_tests {
    use super::{extraction_status_from_zip, run_edinet_zip_pipeline, ExtractionStatus, FetchStatus};
    use crate::knowledge::edinet_archive::MAX_EDINET_ARCHIVE_BYTES;
    use crate::knowledge::edinet_client::{
        acquisition_from_selected_with_partials, EdinetDocumentMeta, YuhoSelection,
    };
    use crate::knowledge::net_gateway::{GatewayError, HttpTransport, ResponseBody, ResponseMeta};
    use crate::llm::service::{
        edinet_coordinator_test_handle, EdinetCancelCause, EDINET_MIN_START_HEADROOM_BYTES,
    };
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    /// `production_archive_gate()` is process-global; serialize Step 11 pipeline tests.
    fn archive_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct ChunkBody {
        data: Option<Vec<u8>>,
    }

    impl ResponseBody for ChunkBody {
        fn next_chunk(
            &mut self,
        ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
        {
            let next = self.data.take().map(Ok);
            async move { next }
        }
    }

    struct RecordingTransport {
        payloads: Mutex<Vec<Vec<u8>>>,
        calls: Mutex<Vec<String>>,
        fail_mode: Mutex<Option<&'static str>>,
    }

    impl RecordingTransport {
        fn new(payloads: Vec<Vec<u8>>) -> Self {
            Self {
                payloads: Mutex::new(payloads),
                calls: Mutex::new(Vec::new()),
                fail_mode: Mutex::new(None),
            }
        }

        fn with_fail(payloads: Vec<Vec<u8>>, mode: &'static str) -> Self {
            let t = Self::new(payloads);
            if let Ok(mut m) = t.fail_mode.lock() {
                *m = Some(mode);
            }
            t
        }

        fn call_types(&self) -> Vec<String> {
            self.calls.lock().map(|c| c.clone()).unwrap_or_default()
        }
    }

    impl HttpTransport for RecordingTransport {
        type Body = ChunkBody;
        fn get(
            &self,
            url: &str,
            _deadline: Duration,
        ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
        {
            let kind = if url.contains("type=5") {
                "5"
            } else if url.contains("type=1") {
                "1"
            } else {
                "?"
            };
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(kind.into());
            }
            let fail = self.fail_mode.lock().ok().and_then(|m| *m);
            let payload = self.payloads.lock().ok().and_then(|mut p| {
                if p.is_empty() {
                    None
                } else {
                    Some(p.remove(0))
                }
            });
            async move {
                if fail == Some("network") && kind == "1" {
                    return Err(GatewayError::DnsLookupFailed);
                }
                if fail == Some("too_large") && kind == "5" {
                    return Ok((
                        ResponseMeta {
                            status: 200,
                            content_type: Some("application/octet-stream".into()),
                            content_encoding: Some("identity".into()),
                            content_length: Some(MAX_EDINET_ARCHIVE_BYTES + 1),
                        },
                        ChunkBody {
                            data: Some(vec![0x50, 0x4b, 0x03, 0x04]),
                        },
                    ));
                }
                if fail == Some("too_large_type1") && kind == "1" {
                    return Ok((
                        ResponseMeta {
                            status: 200,
                            content_type: Some("application/octet-stream".into()),
                            content_encoding: Some("identity".into()),
                            content_length: Some(MAX_EDINET_ARCHIVE_BYTES + 1),
                        },
                        ChunkBody {
                            data: Some(vec![0x50, 0x4b, 0x03, 0x04]),
                        },
                    ));
                }
                let body = payload.unwrap_or_else(|| vec![0x50, 0x4b, 0x03, 0x04]);
                Ok((
                    ResponseMeta {
                        status: 200,
                        content_type: Some("application/octet-stream".into()),
                        content_encoding: Some("identity".into()),
                        content_length: Some(body.len() as u64),
                    },
                    ChunkBody { data: Some(body) },
                ))
            }
        }
    }

    fn minimal_zip_bytes() -> Vec<u8> {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("empty.zip");
        {
            let file = std::fs::File::create(&path).expect("create");
            let zw = zip::ZipWriter::new(file);
            zw.finish().expect("finish");
        }
        std::fs::read(&path).expect("read")
    }

    fn zip_with_stored_entry(name: &str, bytes: &[u8]) -> Vec<u8> {
        use std::io::Write as _;
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("one.zip");
        {
            let file = std::fs::File::create(&path).expect("create");
            let mut zw = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zw.start_file(name, opts).expect("start");
            zw.write_all(bytes).expect("write");
            zw.finish().expect("finish");
        }
        std::fs::read(&path).expect("read")
    }

    /// type=1 fixture: 有効な PublicDoc Inline XBRL（BusinessRisksTextBlock 1件）。
    fn narrative_zip_bytes() -> Vec<u8> {
        const IX: &str = "http://www.xbrl.org/2013/inlineXBRL";
        const JPCRP_COR: &str =
            "http://disclosure.edinet-fsa.go.jp/taxonomy/jpcrp/2023-12-01/jpcrp_cor";
        let xml = format!(
            r#"<?xml version="1.0"?>
<html xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">ZIPリスク本文</ix:nonNumeric>
</html>"#
        );
        zip_with_stored_entry("XBRL/PublicDoc/0001_ixbrl.htm", xml.as_bytes())
    }

    /// 決定表 #11 fixture: ZIP magic は通るが EOCD が無い不正アーカイブ。
    fn corrupt_zip_bytes() -> Vec<u8> {
        let mut bytes = vec![0x50, 0x4b, 0x03, 0x04];
        bytes.extend(std::iter::repeat(0xA5).take(96));
        bytes
    }

    /// 決定表 #12 fixture: UTF-16LE BOM を持たない（UTF-8 の）type=5 CSV entry。
    fn utf8_csv_zip_bytes() -> Vec<u8> {
        zip_with_stored_entry(
            "XBRL_TO_CSV/sample.csv",
            "要素ID\t項目名\r\n".as_bytes(),
        )
    }

    /// 決定表 #13 fixture: encoding は正しいがヘッダー検証に失敗する TSV。
    fn bad_header_utf16_csv_zip_bytes() -> Vec<u8> {
        let tsv = "列A\t列B\r\n\"x\"\t\"y\"\r\n";
        let mut payload = vec![0xFF, 0xFE];
        for unit in tsv.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        zip_with_stored_entry("XBRL_TO_CSV/sample.csv", &payload)
    }

    fn selection(csv_flag: &str) -> YuhoSelection {
        YuhoSelection {
            original: EdinetDocumentMeta {
                doc_id: Some("S100ZIP1".into()),
                edinet_code: Some("E02144".into()),
                filer_name: Some("テスト株式会社".into()),
                doc_type_code: Some("120".into()),
                doc_description: Some("有価証券報告書".into()),
                submit_date_time: Some("2024-06-25 15:00".into()),
                withdrawal_status: Some("0".into()),
                disclosure_status: Some("0".into()),
                legal_status: Some("1".into()),
                xbrl_flag: Some("1".into()),
                csv_flag: Some(csv_flag.into()),
                period_start: Some("2023-04-01".into()),
                period_end: Some("2024-03-31".into()),
                ..EdinetDocumentMeta::default()
            },
            correction_available: false,
        }
    }

    #[tokio::test]
    async fn csv_flag_one_downloads_type5_then_type1_serially() {
        let _lock = archive_test_lock();
        let zip = minimal_zip_bytes();
        let transport = RecordingTransport::new(vec![zip.clone(), zip]);
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        let selected = selection("1");
        let outcome = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert_eq!(transport.call_types(), vec!["5".to_string(), "1".to_string()]);
        assert!(matches!(
            outcome.fetch,
            FetchStatus::Succeeded | FetchStatus::NetworkFailed | FetchStatus::ApiError
        ));
        let leftover: Vec<_> = std::fs::read_dir(temp.path())
            .expect("read temp")
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftover.is_empty(), "temp archive must be dropped");
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    #[tokio::test]
    async fn csv_flag_not_one_skips_type5() {
        let _lock = archive_test_lock();
        let zip = minimal_zip_bytes();
        let transport = RecordingTransport::new(vec![zip]);
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        let selected = selection("0");
        let _ = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert_eq!(transport.call_types(), vec!["1".to_string()]);
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    #[tokio::test]
    async fn type5_too_large_continues_to_type1() {
        let _lock = archive_test_lock();
        let zip = minimal_zip_bytes();
        let transport = RecordingTransport::with_fail(vec![zip], "too_large");
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        let selected = selection("1");
        let _ = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert_eq!(transport.call_types(), vec!["5".to_string(), "1".to_string()]);
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    #[tokio::test]
    async fn type1_network_fail_after_type5_attempt_keeps_serial_order() {
        let _lock = archive_test_lock();
        let zip = minimal_zip_bytes();
        let transport = RecordingTransport::with_fail(vec![zip], "network");
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        let selected = selection("1");
        let outcome = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert_eq!(transport.call_types(), vec!["5".to_string(), "1".to_string()]);
        let (acq, _, _) = acquisition_from_selected_with_partials(
            &selected,
            outcome.financials.as_ref(),
            outcome.narratives.as_ref(),
        )
        .expect("acq");
        let extraction =
            extraction_status_from_zip(&acq, outcome.interrupted, outcome.extraction_failed);
        assert!(matches!(
            extraction,
            ExtractionStatus::None
                | ExtractionStatus::ParseFailed
                | ExtractionStatus::FinancialsOnly
        ));
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    #[tokio::test]
    async fn pre_cancelled_token_maps_to_cancelled_extraction() {
        let _lock = archive_test_lock();
        let zip = minimal_zip_bytes();
        let transport = RecordingTransport::new(vec![zip.clone(), zip]);
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        guard.cancel_with_cause(EdinetCancelCause::Background);
        let selected = selection("1");
        let outcome = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert!(outcome.interrupted);
        assert_eq!(outcome.fetch, FetchStatus::Cancelled);
        let (acq, _, _) = acquisition_from_selected_with_partials(
            &selected,
            outcome.financials.as_ref(),
            outcome.narratives.as_ref(),
        )
        .expect("acq");
        let extraction = extraction_status_from_zip(&acq, true, outcome.extraction_failed);
        assert_eq!(extraction, ExtractionStatus::Cancelled);
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    #[tokio::test]
    async fn memory_cause_maps_to_memory_pressure_fetch() {
        let _lock = archive_test_lock();
        let zip = minimal_zip_bytes();
        let transport = RecordingTransport::new(vec![zip]);
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        guard.cancel_with_cause(EdinetCancelCause::MemoryPressure);
        let selected = selection("0");
        let outcome = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert!(outcome.interrupted);
        assert_eq!(outcome.fetch, FetchStatus::MemoryPressure);
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    /// STEP_12_DESIGN.md 決定表 #10-#13 共通の assert:
    /// 両側成果ゼロ + 構造/parse 失敗 → fetch=succeeded, extraction=parse_failed。
    async fn assert_row_succeeded_parse_failed(
        transport: RecordingTransport,
        csv_flag: &str,
        row: &str,
    ) {
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        let selected = selection(csv_flag);
        let outcome = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert!(!outcome.interrupted, "row {row}");
        assert!(outcome.extraction_failed, "row {row}");
        assert_eq!(outcome.fetch, FetchStatus::Succeeded, "row {row}");
        let (acq, _, _) = acquisition_from_selected_with_partials(
            &selected,
            outcome.financials.as_ref(),
            outcome.narratives.as_ref(),
        )
        .expect("acq");
        let extraction =
            extraction_status_from_zip(&acq, outcome.interrupted, outcome.extraction_failed);
        assert_eq!(extraction, ExtractionStatus::ParseFailed, "row {row}");
        let leftover: Vec<_> = std::fs::read_dir(temp.path())
            .expect("read temp")
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftover.is_empty(), "row {row}: temp must be clean");
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }

    /// 決定表 #10: DL レベルの上限超過（Content-Length > cap、両側成果ゼロ）。
    #[tokio::test]
    async fn row10_download_too_large_yields_succeeded_parse_failed() {
        let _lock = archive_test_lock();
        let transport = RecordingTransport::with_fail(Vec::new(), "too_large_type1");
        assert_row_succeeded_parse_failed(transport, "0", "#10").await;
    }

    /// 決定表 #11: magic は通るが EOCD の無い不正 ZIP（extract 段で InvalidZip）。
    #[tokio::test]
    async fn row11_invalid_zip_yields_succeeded_parse_failed() {
        let _lock = archive_test_lock();
        let transport = RecordingTransport::new(vec![corrupt_zip_bytes()]);
        assert_row_succeeded_parse_failed(transport, "0", "#11").await;
    }

    /// 決定表 #12: type=5 が UTF-16LE でない（UnsupportedEncoding）。
    #[tokio::test]
    async fn row12_unsupported_encoding_yields_succeeded_parse_failed() {
        let _lock = archive_test_lock();
        let transport =
            RecordingTransport::new(vec![utf8_csv_zip_bytes(), minimal_zip_bytes()]);
        assert_row_succeeded_parse_failed(transport, "1", "#12").await;
    }

    /// 決定表 #13: TSV ヘッダー検証失敗（hard Parse）。
    #[tokio::test]
    async fn row13_bad_tsv_header_yields_succeeded_parse_failed() {
        let _lock = archive_test_lock();
        let transport =
            RecordingTransport::new(vec![bad_header_utf16_csv_zip_bytes(), minimal_zip_bytes()]);
        assert_row_succeeded_parse_failed(transport, "1", "#13").await;
    }

    /// 決定表 #13b: 片側（type=5）構造失敗でも type=1 の成果は捨てない →
    /// narratives_only + succeeded。「片方失敗で全部捨て」の禁止を固定する。
    #[tokio::test]
    async fn row13b_partial_keeps_narratives_when_type5_rejected() {
        let _lock = archive_test_lock();
        // payload[0] は type=5 呼出で消費される（too_large モードにより破棄）。
        // narrative fixture は2要素目として type=1 に届ける。
        let transport = RecordingTransport::with_fail(
            vec![Vec::new(), narrative_zip_bytes()],
            "too_large",
        );
        let temp = tempfile::tempdir().expect("temp");
        let (handle, worker) = edinet_coordinator_test_handle();
        let guard = handle
            .acquire_edinet_job_with_headroom(|| Some(EDINET_MIN_START_HEADROOM_BYTES))
            .expect("acquire");
        let selected = selection("1");
        let outcome = run_edinet_zip_pipeline(
            &transport,
            &selected,
            "test-key",
            temp.path(),
            &guard,
        )
        .await;
        assert_eq!(transport.call_types(), vec!["5".to_string(), "1".to_string()]);
        assert!(!outcome.interrupted);
        assert_eq!(outcome.fetch, FetchStatus::Succeeded);
        let narrative_count = outcome
            .narratives
            .as_ref()
            .map(|partial| partial.narratives.len())
            .unwrap_or(0);
        assert_eq!(narrative_count, 1, "type=1 narrative must survive");
        let (acq, _, _) = acquisition_from_selected_with_partials(
            &selected,
            outcome.financials.as_ref(),
            outcome.narratives.as_ref(),
        )
        .expect("acq");
        let extraction =
            extraction_status_from_zip(&acq, outcome.interrupted, outcome.extraction_failed);
        assert_eq!(extraction, ExtractionStatus::NarrativesOnly);
        drop(guard);
        drop(handle);
        worker.join().expect("worker");
    }
}
