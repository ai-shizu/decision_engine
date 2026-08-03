//! EDINET API v2 client foundation (M12 + lane Steps 1–4).
//!
//! **No HTTP client path construction here** — constitutional guard confines
//! the reqwest client to `net_gateway.rs`. This module owns fixed-template URL
//! construction, send-time URL validation, JSON list parsing, type-safe
//! document download classification (`EdinetDocumentKind`), and heuristic
//! section extraction from UTF-8 filing text. Live HTTP goes through
//! [`crate::knowledge::net_gateway`] + dual-factor egress
//! (`NetworkPolicy::Live` ∧ `egress-live`).
//!
//! Offline path: callers may inject [`CompanyFacts`] directly (no network).
//! Production ZIP stream-to-temp / extract is Step 5+ (not this module's Vec path).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::collections::HashSet;
use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::knowledge::net_gateway::{
    fetch_bounded_with_deadline, fetch_bounded_with_deadline_limit, safe_truncate,
    validate_response_meta, GatewayError, HttpTransport, ResponseMeta,
};
use crate::knowledge::render_guard::sanitize_external_text;

/// Fixed EDINET API host (no alternate hosts / open redirects).
pub const EDINET_HOST: &str = "api.edinet-fsa.go.jp";
pub const EDINET_LIST_PATH: &str = "/api/v2/documents.json";
pub const EDINET_DOC_PATH_PREFIX: &str = "/api/v2/documents/";

/// Soft caps for prompt-safe company fact fields.
pub const MAX_FACT_FIELD_BYTES: usize = 4_096;
/// Aggregate cap across the four field-capped members counted by
/// `sanitize_company_facts` (company_name + 3 narrative fields). Must stay
/// ≥ 4 × MAX_FACT_FIELD_BYTES: the pre-Step 9 value (12,000) was smaller than
/// 4,096 × 3 + name, so legitimately per-field-capped facts failed closed
/// (directive §Step 9 contradiction). Full raw text never enters
/// `CompanyFacts` — it lives in `EdinetEvidenceSection` (128 KiB / 256 KiB).
pub const MAX_COMPANY_FACTS_BYTES: usize = 4 * MAX_FACT_FIELD_BYTES;

/// EDINET documents.json body cap (Wikipedia keeps the shared 1MiB gateway cap).
pub const MAX_EDINET_LIST_BYTES: usize = 8 * 1024 * 1024;
/// Max retained 120/130 rows for one company while streaming `results`.
pub const MAX_EDINET_LIST_CANDIDATES: usize = 32;

const DOC_TYPE_YUHO: &str = "120";
const DOC_TYPE_YUHO_CORRECTION: &str = "130";
/// Serde custom-error token remapped to [`EdinetError::CandidateLimitExceeded`].
const CANDIDATE_LIMIT_SERDE_MSG: &str = "edinet_candidate_limit_exceeded";

/// One row from `documents.json` `results[]` (null-tolerant; API uses `docID` etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EdinetDocumentMeta {
    #[serde(default, rename = "docID")]
    pub doc_id: Option<String>,
    #[serde(default)]
    pub edinet_code: Option<String>,
    #[serde(default)]
    pub filer_name: Option<String>,
    #[serde(default)]
    pub ordinance_code: Option<String>,
    #[serde(default)]
    pub form_code: Option<String>,
    #[serde(default)]
    pub doc_type_code: Option<String>,
    #[serde(default)]
    pub period_start: Option<String>,
    #[serde(default)]
    pub period_end: Option<String>,
    #[serde(default)]
    pub submit_date_time: Option<String>,
    #[serde(default, rename = "parentDocID")]
    pub parent_doc_id: Option<String>,
    #[serde(default)]
    pub withdrawal_status: Option<String>,
    #[serde(default)]
    pub doc_info_edit_status: Option<String>,
    #[serde(default)]
    pub disclosure_status: Option<String>,
    #[serde(default)]
    pub xbrl_flag: Option<String>,
    #[serde(default)]
    pub csv_flag: Option<String>,
    #[serde(default)]
    pub legal_status: Option<String>,
    /// Optional display fields (not selection gates).
    #[serde(default)]
    pub doc_description: Option<String>,
    #[serde(default)]
    pub sec_code: Option<String>,
}

/// Strict 120 selection outcome (130 is diagnosis only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YuhoSelection {
    pub original: EdinetDocumentMeta,
    pub correction_available: bool,
}

/// Filter for streaming candidate retention (exact code / normalized filer name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListCandidateFilter {
    EdinetCode(String),
    FilerNameNormalized(String),
}

impl ListCandidateFilter {
    pub fn by_edinet_code(code: &str) -> Option<Self> {
        let t = code.trim();
        if t.is_empty() {
            None
        } else {
            Some(Self::EdinetCode(t.to_string()))
        }
    }

    pub fn by_filer_name(name: &str) -> Option<Self> {
        let key = normalize_filer_key(name);
        if key.len() < 2 {
            None
        } else {
            Some(Self::FilerNameNormalized(key))
        }
    }

    fn matches(&self, meta: &EdinetDocumentMeta) -> bool {
        match self {
            Self::EdinetCode(code) => meta
                .edinet_code
                .as_deref()
                .map(|c| c.trim().eq_ignore_ascii_case(code))
                .unwrap_or(false),
            Self::FilerNameNormalized(key) => meta
                .filer_name
                .as_deref()
                .map(|n| normalize_filer_key(n) == *key)
                .unwrap_or(false),
        }
    }
}

/// Normalized company facts for interview / ES prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompanyFacts {
    pub company_name: String,
    pub edinet_code: String,
    pub doc_id: String,
    pub business_summary: String,
    pub business_risks: String,
    pub performance_summary: String,
    /// Provenance note (e.g. "edinet_list" / "injected" / "edinet_text").
    pub source: String,
}

/// Internal result for a confirmed EDINET acquisition. This metadata must not
/// be inferred from the display-only `CompanyFacts.source` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdinetFieldAcquisition {
    pub business_summary: bool,
    pub business_risks: bool,
    pub performance_summary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdinetFactAcquisition {
    facts: CompanyFacts,
    selected_doc_id: String,
    submitted_at: String,
    edinet_code: String,
    fields: EdinetFieldAcquisition,
}

impl EdinetFactAcquisition {
    pub fn new(
        facts: CompanyFacts,
        selected_doc_id: String,
        submitted_at: String,
        edinet_code: String,
        fields: EdinetFieldAcquisition,
    ) -> Result<Self, EdinetError> {
        let doc_id = selected_doc_id.trim().to_string();
        let submitted = submitted_at.trim().to_string();
        let code = edinet_code.trim().to_string();
        if !is_doc_id(&doc_id)
            || !is_edinet_code(&code)
            || !is_submitted_at(&submitted)
            || facts.doc_id != doc_id
            || facts.edinet_code != code
        {
            return Err(EdinetError::Malformed);
        }
        Ok(Self {
            facts,
            selected_doc_id: doc_id,
            submitted_at: submitted,
            edinet_code: code,
            fields,
        })
    }
    pub fn facts(&self) -> &CompanyFacts {
        &self.facts
    }
    pub fn selected_doc_id(&self) -> &str {
        &self.selected_doc_id
    }
    pub fn submitted_at(&self) -> &str {
        &self.submitted_at
    }
    pub fn edinet_code(&self) -> &str {
        &self.edinet_code
    }
    pub fn fields(&self) -> &EdinetFieldAcquisition {
        &self.fields
    }
}

/// Document download `type` — callers must not pass raw `1`/`5` ints or strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdinetDocumentKind {
    /// API `type=1` — filing + XBRL ZIP.
    FilingAndXbrl,
    /// API `type=5` — XBRL CSV ZIP.
    XbrlCsv,
}

impl EdinetDocumentKind {
    /// Sole source of the query `type` value for document download URLs.
    pub const fn as_type_query(self) -> &'static str {
        match self {
            Self::FilingAndXbrl => "1",
            Self::XbrlCsv => "5",
        }
    }
}

/// How to interpret a document-download response body after meta classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentResponseClass {
    /// HTTP 200 + `application/octet-stream` — expect ZIP local-file magic next.
    ExpectedZip,
    /// `application/json` (any status) — EDINET API error envelope, not an archive.
    ApiErrorJson,
}

/// ZIP local file header magic (`PK\x03\x04`). Checked after content-type gate.
const ZIP_LOCAL_FILE_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdinetError {
    UrlViolation,
    InvalidArgument,
    Malformed,
    Gateway(GatewayError),
    ApiKeyMissing,
    CandidateLimitExceeded,
    NoEligibleFiling,
    AmbiguousSelection,
    InvalidContentType,
    /// EDINET API error envelope (`metadata.status` / `StatusCode`). Token only — no body.
    ApiResponse {
        status: String,
    },
    InvalidZip,
    /// Measured (or declared) archive bytes exceed the compressed-archive cap.
    TooLarge,
    /// Memory-pressure probe fired between chunks; download aborted.
    MemoryPressure,
    /// A `TempArchive` already exists — concurrent archive downloads are forbidden.
    TempArchiveBusy,
    /// Temp-file create/write/rewind failure (no path/key/body retained).
    TempFileIo,
    /// ZIP64, multi-disk, or other archive features outside the supported subset.
    UnsupportedArchive,
    /// Entry encoding is not the expected UTF-16LE TSV.
    UnsupportedEncoding,
    /// Bounded parse failed in a non-recoverable way (no body retained).
    Parse,
}

impl std::fmt::Display for EdinetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UrlViolation => write!(f, "edinet_url_violation"),
            Self::InvalidArgument => write!(f, "edinet_invalid_argument"),
            Self::Malformed => write!(f, "edinet_malformed"),
            Self::Gateway(e) => write!(f, "edinet_gateway:{e}"),
            Self::ApiKeyMissing => write!(f, "edinet_api_key_missing"),
            Self::CandidateLimitExceeded => write!(f, "edinet_candidate_limit_exceeded"),
            Self::NoEligibleFiling => write!(f, "edinet_no_eligible_filing"),
            Self::AmbiguousSelection => write!(f, "edinet_ambiguous_selection"),
            Self::InvalidContentType => write!(f, "edinet_invalid_content_type"),
            Self::ApiResponse { status } => write!(f, "edinet_api_response:{status}"),
            Self::InvalidZip => write!(f, "edinet_invalid_zip"),
            Self::TooLarge => write!(f, "edinet_too_large"),
            Self::MemoryPressure => write!(f, "edinet_memory_pressure"),
            Self::TempArchiveBusy => write!(f, "edinet_temp_archive_busy"),
            Self::TempFileIo => write!(f, "edinet_temp_file_io"),
            Self::UnsupportedArchive => write!(f, "edinet_unsupported_archive"),
            Self::UnsupportedEncoding => write!(f, "edinet_unsupported_encoding"),
            Self::Parse => write!(f, "edinet_parse"),
        }
    }
}

impl std::error::Error for EdinetError {}

impl From<GatewayError> for EdinetError {
    fn from(value: GatewayError) -> Self {
        Self::Gateway(value)
    }
}

fn is_ymd(date: &str) -> bool {
    let b = date.as_bytes();
    if b.len() != 10 {
        return false;
    }
    matches!(b.get(4), Some(b'-'))
        && matches!(b.get(7), Some(b'-'))
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                true
            } else {
                c.is_ascii_digit()
            }
        })
}

fn is_doc_id(doc_id: &str) -> bool {
    let t = doc_id.trim();
    if t.is_empty() || t.len() > 32 {
        return false;
    }
    t.bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn is_subscription_key(key: &str) -> bool {
    let t = key.trim();
    if t.is_empty() || t.len() > 128 {
        return false;
    }
    t.bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn percent_encode_query_param(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            other => {
                out.push('%');
                let hex_digits = "0123456789ABCDEF";
                let hi = (other >> 4) as usize;
                let lo = (other & 0x0f) as usize;
                if let (Some(h), Some(l)) = (hex_digits.get(hi..hi + 1), hex_digits.get(lo..lo + 1))
                {
                    out.push_str(h);
                    out.push_str(l);
                }
            }
        }
    }
    out
}

/// Build the ONLY permitted documents-list URL (Subscription-Key in query).
pub fn build_documents_list_url(date: &str, subscription_key: &str) -> Result<String, EdinetError> {
    if !is_ymd(date) || !is_subscription_key(subscription_key) {
        return Err(EdinetError::InvalidArgument);
    }
    Ok(format!(
        "https://{EDINET_HOST}{EDINET_LIST_PATH}?date={}&type=2&Subscription-Key={}",
        percent_encode_query_param(date),
        percent_encode_query_param(subscription_key.trim())
    ))
}

/// Build the ONLY permitted document-download URL.
/// `type` comes solely from [`EdinetDocumentKind`] — never from caller ints/strings.
pub fn build_document_download_url(
    doc_id: &str,
    kind: EdinetDocumentKind,
    subscription_key: &str,
) -> Result<String, EdinetError> {
    if !is_doc_id(doc_id) || !is_subscription_key(subscription_key) {
        return Err(EdinetError::InvalidArgument);
    }
    Ok(format!(
        "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}{}?type={}&Subscription-Key={}",
        percent_encode_query_param(doc_id.trim()),
        kind.as_type_query(),
        percent_encode_query_param(subscription_key.trim())
    ))
}

/// Send-time assertion for documents.json URLs.
pub fn validate_documents_list_url(url: &str, expected_date: &str) -> Result<(), EdinetError> {
    let prefix = format!("https://{EDINET_HOST}{EDINET_LIST_PATH}?");
    let rest = url.strip_prefix(&prefix).ok_or(EdinetError::UrlViolation)?;
    if rest.contains('#') || rest.contains('@') {
        return Err(EdinetError::UrlViolation);
    }
    let mut date_ok = false;
    let mut type_ok = false;
    let mut key_ok = false;
    for pair in rest.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().ok_or(EdinetError::UrlViolation)?;
        let v = it.next().ok_or(EdinetError::UrlViolation)?;
        match k {
            "date" => {
                if v != expected_date {
                    return Err(EdinetError::UrlViolation);
                }
                date_ok = true;
            }
            "type" => {
                if v != "2" {
                    return Err(EdinetError::UrlViolation);
                }
                type_ok = true;
            }
            "Subscription-Key" => {
                if v.is_empty() || v.len() > 128 {
                    return Err(EdinetError::UrlViolation);
                }
                key_ok = true;
            }
            _ => return Err(EdinetError::UrlViolation),
        }
    }
    if date_ok && type_ok && key_ok {
        Ok(())
    } else {
        Err(EdinetError::UrlViolation)
    }
}

/// Send-time assertion for `/documents/{docID}` URLs (host/path/query/type).
/// Proves the fixed template even if a future caller bypasses the builder:
/// exactly one `type`, exactly one `Subscription-Key`, and docID/key charset rules.
pub fn validate_document_download_url(
    url: &str,
    expected_doc_id: &str,
    expected_kind: EdinetDocumentKind,
) -> Result<(), EdinetError> {
    if !is_doc_id(expected_doc_id) {
        return Err(EdinetError::UrlViolation);
    }
    let prefix = format!("https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}");
    let rest = url.strip_prefix(&prefix).ok_or(EdinetError::UrlViolation)?;
    if rest.contains('#') || rest.contains('@') {
        return Err(EdinetError::UrlViolation);
    }
    let (id_part, query) = rest.split_once('?').ok_or(EdinetError::UrlViolation)?;
    if id_part != expected_doc_id || !is_doc_id(id_part) {
        return Err(EdinetError::UrlViolation);
    }
    let expected_type = expected_kind.as_type_query();
    let mut type_ok = false;
    let mut key_ok = false;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().ok_or(EdinetError::UrlViolation)?;
        let v = it.next().ok_or(EdinetError::UrlViolation)?;
        match k {
            "type" => {
                if type_ok || v != expected_type {
                    return Err(EdinetError::UrlViolation);
                }
                type_ok = true;
            }
            "Subscription-Key" => {
                if key_ok || !is_subscription_key(v) {
                    return Err(EdinetError::UrlViolation);
                }
                key_ok = true;
            }
            _ => return Err(EdinetError::UrlViolation),
        }
    }
    if type_ok && key_ok {
        Ok(())
    } else {
        Err(EdinetError::UrlViolation)
    }
}

fn primary_media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Classify document-download response meta. HTTP status alone is not success.
///
/// - `application/json` → API error envelope (even on HTTP 200)
/// - HTTP 200 + `application/octet-stream` → expected ZIP (magic checked later)
/// - HTML / other types → [`EdinetError::InvalidContentType`]
pub fn classify_document_response_meta(
    meta: &ResponseMeta,
) -> Result<DocumentResponseClass, EdinetError> {
    match meta.content_encoding.as_deref() {
        None => {}
        Some(enc) if enc.trim().eq_ignore_ascii_case("identity") => {}
        Some(_) => return Err(GatewayError::WireViolation.into()),
    }
    let primary = meta
        .content_type
        .as_deref()
        .map(primary_media_type)
        .unwrap_or_default();
    if primary == "application/json" {
        return Ok(DocumentResponseClass::ApiErrorJson);
    }
    if meta.status != 200 {
        return Err(GatewayError::StatusRejected.into());
    }
    if primary == "application/octet-stream" {
        return Ok(DocumentResponseClass::ExpectedZip);
    }
    Err(EdinetError::InvalidContentType)
}

/// After content-type gate: require ZIP local-file header magic.
pub fn verify_zip_local_file_magic(prefix: &[u8]) -> Result<(), EdinetError> {
    if prefix.len() < ZIP_LOCAL_FILE_MAGIC.len() {
        return Err(EdinetError::InvalidZip);
    }
    if prefix.get(..ZIP_LOCAL_FILE_MAGIC.len()) != Some(ZIP_LOCAL_FILE_MAGIC.as_slice()) {
        return Err(EdinetError::InvalidZip);
    }
    Ok(())
}

/// Sanitize API status tokens for errors/logs (never body text / keys).
fn sanitize_api_status_token(raw: &str) -> String {
    let mut out = String::new();
    for b in raw.trim().bytes() {
        if out.len() >= 32 {
            break;
        }
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
            out.push(b as char);
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ApiStatusField {
    Text(String),
    Num(u64),
}

impl ApiStatusField {
    fn into_token(self) -> String {
        match self {
            Self::Text(s) => sanitize_api_status_token(&s),
            Self::Num(n) => sanitize_api_status_token(&n.to_string()),
        }
    }
}

#[derive(Deserialize, Default)]
struct DocumentApiErrorMeta {
    #[serde(default)]
    status: Option<ApiStatusField>,
}

#[derive(Deserialize, Default)]
struct DocumentApiErrorRoot {
    #[serde(default)]
    metadata: DocumentApiErrorMeta,
    #[serde(default, rename = "StatusCode")]
    status_code: Option<ApiStatusField>,
}

/// Parse EDINET document API error status from JSON body.
/// Prefers `metadata.status` (string or number); falls back to top-level `StatusCode`.
pub fn parse_document_api_error_status(body: &[u8]) -> String {
    let parsed: DocumentApiErrorRoot = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return "unknown".to_string(),
    };
    if let Some(status) = parsed.metadata.status {
        return status.into_token();
    }
    if let Some(status) = parsed.status_code {
        return status.into_token();
    }
    "unknown".to_string()
}

fn document_api_error_from_body(body: &[u8]) -> EdinetError {
    EdinetError::ApiResponse {
        status: parse_document_api_error_status(body),
    }
}

fn opt_trim(value: &Option<String>) -> &str {
    value.as_deref().map(str::trim).unwrap_or("")
}

fn field_eq(value: &Option<String>, want: &str) -> bool {
    opt_trim(value) == want
}

fn is_yuho_or_correction_row(meta: &EdinetDocumentMeta) -> bool {
    matches!(
        opt_trim(&meta.doc_type_code),
        DOC_TYPE_YUHO | DOC_TYPE_YUHO_CORRECTION
    )
}

/// Eligible 有報原本 (120): not withdrawn, disclosed (API `disclosureStatus=="0"`),
/// legally viewable, XBRL present, submitDateTime present. See EDINET API v2.
pub fn is_eligible_yuho_original(meta: &EdinetDocumentMeta) -> bool {
    field_eq(&meta.doc_type_code, DOC_TYPE_YUHO)
        && field_eq(&meta.withdrawal_status, "0")
        // disclosureStatus: "0"=通常開示, "1"/"2"=不開示系 — "開示中" means not under 不開示.
        && field_eq(&meta.disclosure_status, "0")
        && (field_eq(&meta.legal_status, "1") || field_eq(&meta.legal_status, "2"))
        && field_eq(&meta.xbrl_flag, "1")
        && is_doc_id(opt_trim(&meta.doc_id))
        // Latestness is undefined without submitDateTime — never treat as eligible.
        && !opt_trim(&meta.submit_date_time).is_empty()
}

/// 130 may contribute `correction_available` only when status gates pass (not selected as original).
fn is_eligible_correction_notice(meta: &EdinetDocumentMeta) -> bool {
    field_eq(&meta.doc_type_code, DOC_TYPE_YUHO_CORRECTION)
        && field_eq(&meta.withdrawal_status, "0")
        && field_eq(&meta.disclosure_status, "0")
        && (field_eq(&meta.legal_status, "1") || field_eq(&meta.legal_status, "2"))
        && is_doc_id(opt_trim(&meta.doc_id))
        && !opt_trim(&meta.parent_doc_id).is_empty()
}

/// Stream-parse outcome: matching 120/130 rows + list `processDateTime` (no raw JSON kept).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListParseOutcome {
    pub candidates: Vec<EdinetDocumentMeta>,
    pub process_date_time: Option<String>,
}

/// Stream-parse `documents.json`: visit every `results[]` row (no 500 truncate,
/// no whole-tree `serde_json::Value`), keep only matching 120/130 candidates.
pub fn collect_list_candidates(
    body: &[u8],
    filter: &ListCandidateFilter,
) -> Result<Vec<EdinetDocumentMeta>, EdinetError> {
    Ok(collect_list_parse(body, filter)?.candidates)
}

/// Like [`collect_list_candidates`], also returning root `metadata.processDateTime`.
pub fn collect_list_parse(
    body: &[u8],
    filter: &ListCandidateFilter,
) -> Result<ListParseOutcome, EdinetError> {
    if body.len() > MAX_EDINET_LIST_BYTES {
        return Err(EdinetError::Malformed);
    }
    let mut de = serde_json::Deserializer::from_slice(body);
    let outcome = match de.deserialize_any(ListRootVisitor { filter }) {
        Ok(rows) => rows,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains(CANDIDATE_LIMIT_SERDE_MSG) {
                return Err(EdinetError::CandidateLimitExceeded);
            }
            return Err(EdinetError::Malformed);
        }
    };
    // Reject trailing tokens after the root value (e.g. `{"results":[]}null`).
    de.end().map_err(|_| EdinetError::Malformed)?;
    Ok(outcome)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EdinetListMetadataWire {
    #[serde(default)]
    process_date_time: Option<String>,
}

struct ListRootVisitor<'a> {
    filter: &'a ListCandidateFilter,
}

impl<'de, 'a> Visitor<'de> for ListRootVisitor<'a> {
    type Value = ListParseOutcome;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EDINET documents.json object with results array")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut found: Option<Vec<EdinetDocumentMeta>> = None;
        let mut process_date_time: Option<String> = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == "results" {
                if found.is_some() {
                    return Err(de::Error::custom("duplicate results"));
                }
                found = Some(map.next_value_seed(ResultsSeed {
                    filter: self.filter,
                })?);
            } else if key == "metadata" {
                let meta: EdinetListMetadataWire = map.next_value()?;
                let pdt = meta
                    .process_date_time
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                process_date_time = pdt;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        let candidates = found.ok_or_else(|| de::Error::custom("missing results"))?;
        Ok(ListParseOutcome {
            candidates,
            process_date_time,
        })
    }
}

struct ResultsSeed<'a> {
    filter: &'a ListCandidateFilter,
}

impl<'de, 'a> DeserializeSeed<'de> for ResultsSeed<'a> {
    type Value = Vec<EdinetDocumentMeta>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(ResultsVisitor {
            filter: self.filter,
        })
    }
}

struct ResultsVisitor<'a> {
    filter: &'a ListCandidateFilter,
}

impl<'de, 'a> Visitor<'de> for ResultsVisitor<'a> {
    type Value = Vec<EdinetDocumentMeta>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "results array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out: Vec<EdinetDocumentMeta> = Vec::new();
        let mut seen_doc_ids: HashSet<String> = HashSet::new();
        while let Some(row) = seq.next_element::<EdinetDocumentMeta>()? {
            if !is_yuho_or_correction_row(&row) {
                continue;
            }
            if !self.filter.matches(&row) {
                continue;
            }
            let id = opt_trim(&row.doc_id);
            if !is_doc_id(id) {
                continue;
            }
            // Dedupe before the candidate cap so duplicate docIDs cannot burn the budget.
            if !seen_doc_ids.insert(id.to_string()) {
                continue;
            }
            if out.len() >= MAX_EDINET_LIST_CANDIDATES {
                return Err(de::Error::custom(CANDIDATE_LIMIT_SERDE_MSG));
            }
            out.push(row);
        }
        Ok(out)
    }
}

/// Strict original selection: eligible 120 only, submitDateTime desc, soft-fail on ties.
/// Distinct non-empty `edinetCode` values → [`EdinetError::AmbiguousSelection`] (no silent pick).
/// 130 never wins; `correction_available` only for same-code eligible 130 → chosen docID.
pub fn select_eligible_yuho_original(
    candidates: &[EdinetDocumentMeta],
) -> Result<YuhoSelection, EdinetError> {
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut originals: Vec<&EdinetDocumentMeta> = Vec::new();
    let mut corrections: Vec<&EdinetDocumentMeta> = Vec::new();

    for row in candidates {
        let id = opt_trim(&row.doc_id);
        if !is_doc_id(id) {
            continue;
        }
        if !seen_ids.insert(id.to_string()) {
            continue;
        }
        if field_eq(&row.doc_type_code, DOC_TYPE_YUHO) {
            if is_eligible_yuho_original(row) {
                originals.push(row);
            }
        } else if is_eligible_correction_notice(row) {
            corrections.push(row);
        }
    }

    if originals.is_empty() {
        return Err(EdinetError::NoEligibleFiling);
    }

    // Same normalized filer name can map to multiple issuers — never pick by datetime alone.
    let mut distinct_codes: HashSet<String> = HashSet::new();
    for row in &originals {
        let code = opt_trim(&row.edinet_code);
        if !code.is_empty() {
            distinct_codes.insert(code.to_ascii_uppercase());
        }
    }
    if distinct_codes.len() > 1 {
        return Err(EdinetError::AmbiguousSelection);
    }

    originals.sort_by(|a, b| {
        let sa = opt_trim(&a.submit_date_time);
        let sb = opt_trim(&b.submit_date_time);
        sb.cmp(sa)
    });

    let first = originals.first().ok_or(EdinetError::NoEligibleFiling)?;
    if let Some(second) = originals.get(1) {
        if opt_trim(&first.submit_date_time) == opt_trim(&second.submit_date_time) {
            return Err(EdinetError::AmbiguousSelection);
        }
    }

    let chosen = (*first).clone();
    let chosen_id = opt_trim(&chosen.doc_id);
    let chosen_code = opt_trim(&chosen.edinet_code);
    let correction_available = corrections.iter().any(|c| {
        opt_trim(&c.parent_doc_id) == chosen_id
            && !chosen_code.is_empty()
            && opt_trim(&c.edinet_code).eq_ignore_ascii_case(chosen_code)
    });

    Ok(YuhoSelection {
        original: chosen,
        correction_available,
    })
}

/// Prefer 有価証券報告書原本 matching `edinet_code` (strict gates; no 130 / no arbitrary fallback).
pub fn select_yuho_document<'a>(
    docs: &'a [EdinetDocumentMeta],
    edinet_code: &str,
) -> Option<&'a EdinetDocumentMeta> {
    let code = edinet_code.trim();
    if code.is_empty() {
        return None;
    }
    let matched: Vec<EdinetDocumentMeta> = docs
        .iter()
        .filter(|d| {
            d.edinet_code
                .as_deref()
                .map(|c| c.trim().eq_ignore_ascii_case(code))
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    select_eligible_yuho_original(&matched)
        .ok()
        .and_then(|sel| {
            let id = opt_trim(&sel.original.doc_id).to_string();
            docs.iter().find(|d| opt_trim(&d.doc_id) == id)
        })
}

/// Strip corporate suffixes / whitespace for filer-name matching (no industry if-elif).
pub fn normalize_filer_key(name: &str) -> String {
    let mut s = name.trim().to_string();
    for suffix in [
        "株式会社",
        "有限会社",
        "合同会社",
        "合名会社",
        "合資会社",
        "(株)",
        "（株）",
        "(有)",
        "（有）",
        "㈱",
        "㈲",
        "Inc.",
        "Inc",
        "Corp.",
        "Corp",
        "Ltd.",
        "Ltd",
        "LLC",
        "Co., Ltd.",
        "Co.,Ltd.",
        "Co. Ltd.",
    ] {
        s = s.replace(suffix, "");
    }
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(
            ch,
            ' ' | '\u{3000}' | '・' | '･' | '.' | '/' | '／' | '-' | 'ー' | '—' | '_'
        ) {
            continue;
        }
        for c in ch.to_lowercase() {
            out.push(c);
        }
    }
    out
}

/// Match EDINET list rows by filer name (normalized exact key only for selection).
pub fn select_yuho_by_filer_name<'a>(
    docs: &'a [EdinetDocumentMeta],
    company_name: &str,
) -> Option<&'a EdinetDocumentMeta> {
    let q = normalize_filer_key(company_name);
    if q.len() < 2 {
        return None;
    }
    let matched: Vec<EdinetDocumentMeta> = docs
        .iter()
        .filter(|d| {
            d.filer_name
                .as_deref()
                .map(|n| normalize_filer_key(n) == q)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    select_eligible_yuho_original(&matched)
        .ok()
        .and_then(|sel| {
            let id = opt_trim(&sel.original.doc_id).to_string();
            docs.iter().find(|d| opt_trim(&d.doc_id) == id)
        })
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            if leap {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Previous calendar day for `YYYY-MM-DD` (no chrono dependency).
pub fn prev_ymd(date: &str) -> Option<String> {
    if !is_ymd(date) {
        return None;
    }
    let y: i32 = date.get(0..4)?.parse().ok()?;
    let m: u32 = date.get(5..7)?.parse().ok()?;
    let d: u32 = date.get(8..10)?.parse().ok()?;
    if d > 1 {
        return Some(format!("{y:04}-{m:02}-{:02}", d - 1));
    }
    let (py, pm) = if m > 1 { (y, m - 1) } else { (y - 1, 12) };
    let pd = days_in_month(py, pm);
    Some(format!("{py:04}-{pm:02}-{pd:02}"))
}

/// Anchor date plus up to `days_back` previous calendar days (inclusive of anchor).
pub fn recent_filing_dates(anchor: &str, days_back: u32) -> Vec<String> {
    let mut out = Vec::new();
    if !is_ymd(anchor) {
        return out;
    }
    out.push(anchor.to_string());
    let mut cur = anchor.to_string();
    for _ in 0..days_back {
        match prev_ymd(&cur) {
            Some(prev) => {
                out.push(prev.clone());
                cur = prev;
            }
            None => break,
        }
    }
    out
}

/// Build sparse facts from list metadata (no body text yet).
pub fn facts_from_document_meta(meta: &EdinetDocumentMeta) -> CompanyFacts {
    let summary = format!(
        "提出書類: {} ({})\n期間: {} 〜 {}\n提出日時: {}\n証券コード: {}",
        opt_trim(&meta.doc_description),
        opt_trim(&meta.doc_type_code),
        opt_trim(&meta.period_start),
        opt_trim(&meta.period_end),
        opt_trim(&meta.submit_date_time),
        opt_trim(&meta.sec_code)
    );
    CompanyFacts {
        company_name: safe_truncate(opt_trim(&meta.filer_name), MAX_FACT_FIELD_BYTES),
        edinet_code: safe_truncate(opt_trim(&meta.edinet_code), 64),
        doc_id: safe_truncate(opt_trim(&meta.doc_id), 64),
        business_summary: safe_truncate(&summary, MAX_FACT_FIELD_BYTES),
        business_risks: String::new(),
        performance_summary: String::new(),
        source: "edinet_list".into(),
    }
}

fn find_section<'a>(text: &'a str, markers: &[&str], stop: &[&str]) -> String {
    let lower_hay: String = text.to_string();
    let mut start = None;
    for m in markers {
        if let Some(idx) = lower_hay.find(m) {
            start = Some(idx.saturating_add(m.len()));
            break;
        }
    }
    let Some(s) = start else {
        return String::new();
    };
    let rest = lower_hay.get(s..).unwrap_or("");
    let mut end = rest.len();
    for st in stop {
        if let Some(idx) = rest.find(st) {
            end = end.min(idx);
        }
    }
    let slice = rest.get(..end).unwrap_or("").trim();
    safe_truncate(slice, MAX_FACT_FIELD_BYTES)
}

/// Heuristic extraction of 事業の内容 / 事業等のリスク / 経営成績 from UTF-8 text
/// (HTML extract, pasted yuho text, or future XBRL plaintext). Never panics.
pub fn extract_sections_from_text(text: &str) -> CompanyFacts {
    let business = find_section(
        text,
        &["事業の内容", "【事業の内容】", "事業内容"],
        &[
            "事業等のリスク",
            "経営者による",
            "財政状態",
            "【事業等のリスク】",
        ],
    );
    let risks = find_section(
        text,
        &["事業等のリスク", "【事業等のリスク】", "リスク情報"],
        &["経営者による", "財政状態", "重要な会計", "研究開発活動"],
    );
    let perf = find_section(
        text,
        &[
            "経営成績",
            "財政状態、経営成績",
            "【経営成績】",
            "業績の概要",
        ],
        &[
            "キャッシュ・フロー",
            "生産、受注",
            "研究開発",
            "事業等のリスク",
        ],
    );
    CompanyFacts {
        business_summary: business,
        business_risks: risks,
        performance_summary: perf,
        source: "edinet_text".into(),
        ..CompanyFacts::default()
    }
}

/// Merge list metadata with optional UTF-8 body sections; sanitize for prompts.
pub fn merge_company_facts(
    meta: &EdinetDocumentMeta,
    body_text: Option<&str>,
) -> Result<CompanyFacts, EdinetError> {
    let mut facts = facts_from_document_meta(meta);
    if let Some(text) = body_text {
        let sections = extract_sections_from_text(text);
        if !sections.business_summary.is_empty() {
            facts.business_summary = sections.business_summary;
        }
        facts.business_risks = sections.business_risks;
        facts.performance_summary = sections.performance_summary;
        facts.source = "edinet_list+text".into();
    }
    sanitize_company_facts(&facts)
}

/// Sanitize one optional fact field. An **empty** field means "not fetched yet",
/// which is a legitimate state — not malformed external input. `sanitize_external_text`
/// rejects empty/whitespace-only strings (`render_guard.rs`: an empty result from a
/// non-empty external payload means everything was stripped, i.e. hostile), so passing
/// a natively-empty field into it turned every partially-filled `CompanyFacts` into
/// `edinet_malformed`. That is exactly what `fetch_edinet_company_facts` builds
/// (company_name only, all other fields `""`), so the EDINET lane failed before any
/// network or file access ever happened (2026-07-25 device E2E). Empty in ⇒ empty out;
/// non-empty input keeps the full fail-closed sanitize.
fn sanitize_optional_field(value: &str, max_bytes: usize) -> Result<String, EdinetError> {
    if value.trim().is_empty() {
        return Ok(String::new());
    }
    sanitize_external_text(value, max_bytes).map_err(|_| EdinetError::Malformed)
}

/// Fail-closed sanitize of all fact fields (external evidence discipline).
pub fn sanitize_company_facts(facts: &CompanyFacts) -> Result<CompanyFacts, EdinetError> {
    let company_name = sanitize_optional_field(&facts.company_name, MAX_FACT_FIELD_BYTES)?;
    let edinet_code = sanitize_optional_field(&facts.edinet_code, 64)?;
    let doc_id = sanitize_optional_field(&facts.doc_id, 64)?;
    let business_summary = sanitize_optional_field(&facts.business_summary, MAX_FACT_FIELD_BYTES)?;
    let business_risks = sanitize_optional_field(&facts.business_risks, MAX_FACT_FIELD_BYTES)?;
    let performance_summary =
        sanitize_optional_field(&facts.performance_summary, MAX_FACT_FIELD_BYTES)?;
    let source = sanitize_optional_field(&facts.source, 64)?;

    let total = company_name
        .len()
        .saturating_add(business_summary.len())
        .saturating_add(business_risks.len())
        .saturating_add(performance_summary.len());
    if total > MAX_COMPANY_FACTS_BYTES {
        return Err(EdinetError::Malformed);
    }

    Ok(CompanyFacts {
        company_name,
        edinet_code,
        doc_id,
        business_summary,
        business_risks,
        performance_summary,
        source,
    })
}

/// Render facts as a prompt block (no secret material).
pub fn render_company_facts_block(facts: &CompanyFacts) -> String {
    format!(
        "企業名: {}\nEDINETコード: {}\n書類ID: {}\n出典: {}\n\n### 事業概要\n{}\n\n### 事業等のリスク\n{}\n\n### 業績サマリー\n{}\n",
        facts.company_name,
        facts.edinet_code,
        facts.doc_id,
        facts.source,
        if facts.business_summary.is_empty() {
            "（未取得）"
        } else {
            facts.business_summary.as_str()
        },
        if facts.business_risks.is_empty() {
            "（未取得）"
        } else {
            facts.business_risks.as_str()
        },
        if facts.performance_summary.is_empty() {
            "（未取得）"
        } else {
            facts.performance_summary.as_str()
        },
    )
}

/// Read API key from env (never logged). Live fetch only.
pub fn subscription_key_from_env() -> Result<String, EdinetError> {
    match std::env::var("PKB_EDINET_API_KEY") {
        Ok(k) if is_subscription_key(&k) => Ok(k.trim().to_string()),
        _ => Err(EdinetError::ApiKeyMissing),
    }
}

fn validate_edinet_json_meta(meta: &ResponseMeta) -> Result<(), GatewayError> {
    validate_response_meta(meta)
}

/// Async list fetch: stream-parse with company filter (8MiB cap; no full Value tree).
pub async fn fetch_documents_list_candidates<T: HttpTransport>(
    transport: &T,
    date: &str,
    subscription_key: &str,
    filter: &ListCandidateFilter,
    deadline: std::time::Duration,
) -> Result<Vec<EdinetDocumentMeta>, EdinetError> {
    let url = build_documents_list_url(date, subscription_key)?;
    validate_documents_list_url(&url, date)?;
    let (meta, body) = transport.get(&url, deadline).await?;
    validate_edinet_json_meta(&meta)?;
    let bytes = fetch_bounded_with_deadline_limit(
        body,
        std::future::pending::<()>(),
        deadline,
        MAX_EDINET_LIST_BYTES,
    )
    .await?;
    collect_list_candidates(&bytes, filter)
}

/// Fetch list and resolve yuho facts for `edinet_code` (optional body text merge).
fn acquisition_from_selected(
    selected: &YuhoSelection,
    facts: CompanyFacts,
    fields: EdinetFieldAcquisition,
) -> Result<EdinetFactAcquisition, EdinetError> {
    let doc_id = opt_trim(&selected.original.doc_id).to_string();
    let submitted_at = opt_trim(&selected.original.submit_date_time).to_string();
    let edinet_code = opt_trim(&selected.original.edinet_code).to_string();
    if !is_doc_id(&doc_id) || !is_edinet_code(&edinet_code) || !is_submitted_at(&submitted_at) {
        return Err(EdinetError::Malformed);
    }
    if facts.doc_id != doc_id || facts.edinet_code != edinet_code {
        return Err(EdinetError::Malformed);
    }
    EdinetFactAcquisition::new(facts, doc_id, submitted_at, edinet_code, fields)
}

fn fields_from_body(body_text: Option<&str>) -> EdinetFieldAcquisition {
    let extracted = body_text.map(extract_sections_from_text);
    EdinetFieldAcquisition {
        business_summary: extracted
            .as_ref()
            .is_some_and(|facts| !facts.business_summary.is_empty()),
        business_risks: extracted
            .as_ref()
            .is_some_and(|facts| !facts.business_risks.is_empty()),
        performance_summary: extracted
            .as_ref()
            .is_some_and(|facts| !facts.performance_summary.is_empty()),
    }
}

pub fn acquisition_from_selected_with_body(
    selected: &YuhoSelection,
    body_text: Option<&str>,
) -> Result<EdinetFactAcquisition, EdinetError> {
    let facts = merge_company_facts(&selected.original, body_text)?;
    acquisition_from_selected(selected, facts, fields_from_body(body_text))
}

/// PartialEdinetFacts（財務/ナラティブ）から Step 10 マージ入力を組み立てる。
///
/// facts のフィールド上限・sanitize・doc_id/edinet_code 整合は既存
/// `merge_company_facts` / `EdinetFactAcquisition::new` の規律を再利用する。
/// evidence は入力の連結をそのまま返す（再 sanitize しない）。
pub fn acquisition_from_selected_with_partials(
    selected: &YuhoSelection,
    financials: Option<&crate::knowledge::edinet_csv::PartialEdinetFacts>,
    narratives: Option<&crate::knowledge::edinet_csv::PartialEdinetFacts>,
) -> Result<
    (
        EdinetFactAcquisition,
        Vec<crate::knowledge::edinet_csv::EdinetEvidenceSection>,
        Vec<crate::knowledge::edinet_csv::EdinetWarning>,
    ),
    EdinetError,
> {
    if !is_eligible_yuho_original(&selected.original) {
        return Err(EdinetError::Malformed);
    }
    let mut facts = facts_from_document_meta(&selected.original);
    let mut fields = EdinetFieldAcquisition {
        business_summary: false,
        business_risks: false,
        performance_summary: false,
    };
    let mut evidence = Vec::new();
    let mut warnings = Vec::new();

    if let Some(partial) = financials {
        let rendered = render_performance_summary_from_financials(&partial.financials);
        if !rendered.is_empty() {
            facts.performance_summary = rendered;
            fields.performance_summary = true;
        }
        evidence.extend(partial.evidence.iter().cloned());
        warnings.extend(partial.warnings.iter().cloned());
    }
    if let Some(partial) = narratives {
        for narrative in &partial.narratives {
            match narrative.concept {
                crate::knowledge::edinet_csv::NarrativeConcept::DescriptionOfBusiness => {
                    if !narrative.text.is_empty() {
                        facts.business_summary =
                            safe_truncate(&narrative.text, MAX_FACT_FIELD_BYTES);
                        fields.business_summary = true;
                    }
                }
                crate::knowledge::edinet_csv::NarrativeConcept::BusinessRisks => {
                    if !narrative.text.is_empty() {
                        facts.business_risks =
                            safe_truncate(&narrative.text, MAX_FACT_FIELD_BYTES);
                        fields.business_risks = true;
                    }
                }
                crate::knowledge::edinet_csv::NarrativeConcept::ManagementAnalysis => {
                    // Display CompanyFacts has no dedicated field; evidence only.
                }
            }
        }
        evidence.extend(partial.evidence.iter().cloned());
        warnings.extend(partial.warnings.iter().cloned());
    }

    if fields.business_summary
        || fields.business_risks
        || fields.performance_summary
    {
        facts.source = "edinet_zip".into();
    }
    let facts = sanitize_company_facts(&facts)?;
    let acquisition = acquisition_from_selected(selected, facts, fields)?;
    Ok((acquisition, evidence, warnings))
}

fn render_performance_summary_from_financials(
    financials: &[crate::knowledge::edinet_csv::ExtractedFinancialFact],
) -> String {
    if financials.is_empty() {
        return String::new();
    }
    let mut lines = Vec::with_capacity(financials.len());
    for fact in financials {
        let label = if fact.label.trim().is_empty() {
            fact.element_id.as_str()
        } else {
            fact.label.as_str()
        };
        let unit = if fact.unit_label.trim().is_empty() {
            fact.unit_id.as_str()
        } else {
            fact.unit_label.as_str()
        };
        let year = match fact.relative_year {
            crate::knowledge::edinet_csv::RelativeYear::Current => "current",
            crate::knowledge::edinet_csv::RelativeYear::Prior => "prior",
            crate::knowledge::edinet_csv::RelativeYear::Other => "other",
        };
        let consol = match fact.consolidation {
            crate::knowledge::edinet_csv::Consolidation::Consolidated => "consol",
            crate::knowledge::edinet_csv::Consolidation::NonConsolidated => "nonconsol",
            crate::knowledge::edinet_csv::Consolidation::Unknown => "unknown",
        };
        lines.push(format!(
            "{label}\t{year}\t{consol}\t{unit}\t{}",
            fact.value_text
        ));
    }
    let joined = lines.join("\n");
    safe_truncate(&joined, MAX_FACT_FIELD_BYTES)
}

pub fn acquisition_from_document_meta_with_body(
    original: &EdinetDocumentMeta,
    body_text: Option<&str>,
) -> Result<EdinetFactAcquisition, EdinetError> {
    if !is_eligible_yuho_original(original) {
        return Err(EdinetError::Malformed);
    }
    let selected = YuhoSelection {
        original: original.clone(),
        correction_available: false,
    };
    acquisition_from_selected_with_body(&selected, body_text)
}

fn is_edinet_code(value: &str) -> bool {
    value.len() == 6
        && value.starts_with('E')
        && value
            .get(1..)
            .is_some_and(|rest| rest.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_submitted_at(value: &str) -> bool {
    let bytes = value.as_bytes();
    let has_seconds = bytes.len() == 19;
    if !(bytes.len() == 16 || has_seconds)
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b' ')
        || bytes.get(13) != Some(&b':')
        || (has_seconds && bytes.get(16) != Some(&b':'))
    {
        return false;
    }
    let digits = |start: usize, end: usize| {
        bytes
            .get(start..end)
            .is_some_and(|range| range.iter().all(u8::is_ascii_digit))
    };
    if !digits(0, 4)
        || !digits(5, 7)
        || !digits(8, 10)
        || !digits(11, 13)
        || (has_seconds && !digits(14, 16))
        || (has_seconds && !digits(17, 19))
    {
        return false;
    }
    let year = value.get(0..4).and_then(|s| s.parse::<i32>().ok());
    let month = value.get(5..7).and_then(|s| s.parse::<u32>().ok());
    let day = value.get(8..10).and_then(|s| s.parse::<u32>().ok());
    let hour = value.get(11..13).and_then(|s| s.parse::<u32>().ok());
    let minute = value.get(14..16).and_then(|s| s.parse::<u32>().ok());
    let second = if has_seconds {
        value.get(17..19).and_then(|s| s.parse::<u32>().ok())
    } else {
        Some(0)
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) =
        (year, month, day, hour, minute, second)
    else {
        return false;
    };
    let max_day = days_in_month(year, month);
    (1..=12).contains(&month)
        && day >= 1
        && day <= max_day
        && hour <= 23
        && minute <= 59
        && second <= 59
}

pub async fn fetch_company_facts_acquisition_by_code<T: HttpTransport>(
    transport: &T,
    date: &str,
    edinet_code: &str,
    subscription_key: &str,
    body_text: Option<&str>,
    deadline: std::time::Duration,
) -> Result<EdinetFactAcquisition, EdinetError> {
    let filter =
        ListCandidateFilter::by_edinet_code(edinet_code).ok_or(EdinetError::InvalidArgument)?;
    let docs =
        fetch_documents_list_candidates(transport, date, subscription_key, &filter, deadline)
            .await?;
    let selected = select_eligible_yuho_original(&docs)?;
    let facts = merge_company_facts(&selected.original, body_text)?;
    acquisition_from_selected(&selected, facts, fields_from_body(body_text))
}

pub async fn fetch_company_facts_by_code<T: HttpTransport>(
    transport: &T,
    date: &str,
    edinet_code: &str,
    subscription_key: &str,
    body_text: Option<&str>,
    deadline: std::time::Duration,
) -> Result<CompanyFacts, EdinetError> {
    Ok(fetch_company_facts_acquisition_by_code(
        transport,
        date,
        edinet_code,
        subscription_key,
        body_text,
        deadline,
    )
    .await?
    .facts)
}

/// Resolve company facts by filer name: scan recent filing dates until a match.
pub async fn fetch_company_facts_acquisition_by_name<T: HttpTransport>(
    transport: &T,
    anchor_date: &str,
    company_name: &str,
    subscription_key: &str,
    body_text: Option<&str>,
    deadline: std::time::Duration,
    days_back: u32,
) -> Result<EdinetFactAcquisition, EdinetError> {
    let filter =
        ListCandidateFilter::by_filer_name(company_name).ok_or(EdinetError::InvalidArgument)?;
    let mut last_err = EdinetError::NoEligibleFiling;
    for date in recent_filing_dates(anchor_date, days_back) {
        let docs = match fetch_documents_list_candidates(
            transport,
            &date,
            subscription_key,
            &filter,
            deadline,
        )
        .await
        {
            Ok(docs) => docs,
            Err(EdinetError::CandidateLimitExceeded) => {
                return Err(EdinetError::CandidateLimitExceeded)
            }
            Err(error) => {
                last_err = error;
                continue;
            }
        };
        match select_eligible_yuho_original(&docs) {
            Ok(selected) => {
                let facts = merge_company_facts(&selected.original, body_text)?;
                return acquisition_from_selected(&selected, facts, fields_from_body(body_text));
            }
            Err(EdinetError::AmbiguousSelection) => return Err(EdinetError::AmbiguousSelection),
            Err(error) => last_err = error,
        }
    }
    Err(last_err)
}

pub async fn fetch_company_facts_by_name<T: HttpTransport>(
    transport: &T,
    anchor_date: &str,
    company_name: &str,
    subscription_key: &str,
    body_text: Option<&str>,
    deadline: std::time::Duration,
    days_back: u32,
) -> Result<CompanyFacts, EdinetError> {
    Ok(fetch_company_facts_acquisition_by_name(
        transport,
        anchor_date,
        company_name,
        subscription_key,
        body_text,
        deadline,
        days_back,
    )
    .await?
    .facts)
}

/// Type-safe document download with response classification (not HTTP status alone).
///
/// **Not the production ZIP path** — buffers into `Vec<u8>` under the shared
/// Wikipedia `MAX_RESPONSE_BYTES` (1 MiB). Step 5 stream-to-temp supersedes this
/// for real archives. Retained for contract tests and provisional fixtures.
pub async fn fetch_document_bytes<T: HttpTransport>(
    transport: &T,
    doc_id: &str,
    kind: EdinetDocumentKind,
    subscription_key: &str,
    cancel: impl std::future::Future<Output = ()>,
    deadline: std::time::Duration,
) -> Result<Vec<u8>, EdinetError> {
    let url = build_document_download_url(doc_id, kind, subscription_key)?;
    validate_document_download_url(&url, doc_id.trim(), kind)?;
    let (meta, body) = transport.get(&url, deadline).await?;
    match classify_document_response_meta(&meta)? {
        DocumentResponseClass::ExpectedZip => {
            let bytes = fetch_bounded_with_deadline(body, cancel, deadline)
                .await
                .map_err(EdinetError::from)?;
            verify_zip_local_file_magic(&bytes)?;
            Ok(bytes)
        }
        DocumentResponseClass::ApiErrorJson => {
            let bytes = fetch_bounded_with_deadline(body, cancel, deadline)
                .await
                .map_err(EdinetError::from)?;
            Err(document_api_error_from_body(&bytes))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::net_gateway::ResponseBody;

    fn eligible_120(doc_id: &str, submit: &str) -> String {
        eligible_120_for("E02144", doc_id, submit)
    }

    fn eligible_120_for(edinet_code: &str, doc_id: &str, submit: &str) -> String {
        format!(
            r#"{{
              "docID": "{doc_id}",
              "edinetCode": "{edinet_code}",
              "filerName": "テスト株式会社",
              "ordinanceCode": "010",
              "formCode": "030000",
              "docTypeCode": "120",
              "docDescription": "有価証券報告書",
              "periodStart": "2023-04-01",
              "periodEnd": "2024-03-31",
              "submitDateTime": "{submit}",
              "parentDocID": null,
              "withdrawalStatus": "0",
              "docInfoEditStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "csvFlag": "1",
              "legalStatus": "1",
              "secCode": "72030"
            }}"#
        )
    }

    #[test]
    fn typed_acquisition_normalizes_and_validates_metadata() {
        let selection = YuhoSelection {
            original: EdinetDocumentMeta {
                doc_id: Some(" DOC-1 ".into()),
                edinet_code: Some(" E02144 ".into()),
                submit_date_time: Some(" 2024-02-29 23:59:00 ".into()),
                ..EdinetDocumentMeta::default()
            },
            correction_available: false,
        };
        let facts = CompanyFacts {
            edinet_code: "E02144".into(),
            doc_id: "DOC-1".into(),
            business_summary: "summary".into(),
            ..CompanyFacts::default()
        };
        let result = acquisition_from_selected(&selection, facts, fields_from_body(None)).unwrap();
        assert_eq!(result.selected_doc_id(), "DOC-1");
        assert_eq!(result.submitted_at(), "2024-02-29 23:59:00");
        assert_eq!(result.edinet_code(), "E02144");
        assert!(!result.fields().business_summary);
        assert!(!result.fields().business_risks);
    }

    #[test]
    fn typed_acquisition_rejects_missing_or_invalid_identity() {
        let selection = YuhoSelection {
            original: EdinetDocumentMeta {
                doc_id: Some("DOC-1".into()),
                edinet_code: Some("".into()),
                submit_date_time: Some("2024-02-30 10:00".into()),
                ..EdinetDocumentMeta::default()
            },
            correction_available: false,
        };
        let facts = CompanyFacts {
            edinet_code: String::new(),
            doc_id: "DOC-1".into(),
            ..CompanyFacts::default()
        };
        assert_eq!(
            acquisition_from_selected(&selection, facts, fields_from_body(None)),
            Err(EdinetError::Malformed)
        );
    }

    #[test]
    fn legacy_facts_are_exactly_the_typed_facts() {
        let selection = YuhoSelection {
            original: EdinetDocumentMeta {
                doc_id: Some("DOC-1".into()),
                edinet_code: Some("E02144".into()),
                submit_date_time: Some("2024-01-01 00:00".into()),
                ..EdinetDocumentMeta::default()
            },
            correction_available: false,
        };
        let facts = CompanyFacts {
            edinet_code: "E02144".into(),
            doc_id: "DOC-1".into(),
            business_summary: "summary".into(),
            ..CompanyFacts::default()
        };
        let typed = acquisition_from_selected(
            &selection,
            facts.clone(),
            fields_from_body(Some("事業の内容\n本文")),
        )
        .unwrap();
        assert_eq!(typed.facts(), &facts);
    }

    fn wrap_results(rows: &[String]) -> String {
        format!(
            r#"{{"metadata":{{"status":"200"}},"results":[{}]}}"#,
            rows.join(",")
        )
    }

    fn gated_120(
        doc_id: &str,
        withdrawal: &str,
        disclosure: &str,
        xbrl: &str,
        legal: Option<&str>,
        submit: Option<&str>,
    ) -> String {
        let legal_json = match legal {
            None => "null".to_string(),
            Some(v) => format!("\"{v}\""),
        };
        let submit_json = match submit {
            None => "null".to_string(),
            Some(v) => format!("\"{v}\""),
        };
        format!(
            r#"{{
              "docID": "{doc_id}",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "120",
              "submitDateTime": {submit_json},
              "withdrawalStatus": "{withdrawal}",
              "disclosureStatus": "{disclosure}",
              "xbrlFlag": "{xbrl}",
              "legalStatus": {legal_json}
            }}"#
        )
    }

    #[test]
    fn list_url_round_trip_validate() {
        let url = build_documents_list_url("2024-06-25", "test-key-abc").expect("url");
        assert!(url.contains(EDINET_HOST));
        validate_documents_list_url(&url, "2024-06-25").expect("validate");
    }

    #[test]
    fn document_download_url_type1_and_type5_round_trip() {
        for kind in [
            EdinetDocumentKind::FilingAndXbrl,
            EdinetDocumentKind::XbrlCsv,
        ] {
            let url = build_document_download_url("S100ABCD", kind, "test-key-abc").expect("url");
            assert!(url.contains(EDINET_HOST));
            assert!(url.contains(&format!("type={}", kind.as_type_query())));
            assert!(!url.contains("type=2"));
            validate_document_download_url(&url, "S100ABCD", kind).expect("validate");
        }
    }

    #[test]
    fn document_download_url_rejects_wrong_kind_at_validate() {
        let url = build_document_download_url(
            "S100ABCD",
            EdinetDocumentKind::FilingAndXbrl,
            "test-key-abc",
        )
        .expect("url");
        assert_eq!(
            validate_document_download_url(&url, "S100ABCD", EdinetDocumentKind::XbrlCsv),
            Err(EdinetError::UrlViolation)
        );
    }

    #[test]
    fn document_download_url_rejects_raw_type_forgery_and_fragments() {
        let forged = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}S100ABCD?type=9&Subscription-Key=k"
        );
        assert_eq!(
            validate_document_download_url(&forged, "S100ABCD", EdinetDocumentKind::FilingAndXbrl),
            Err(EdinetError::UrlViolation)
        );
        let with_hash = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}S100ABCD?type=1&Subscription-Key=k#x"
        );
        assert_eq!(
            validate_document_download_url(
                &with_hash,
                "S100ABCD",
                EdinetDocumentKind::FilingAndXbrl
            ),
            Err(EdinetError::UrlViolation)
        );
        assert!(build_document_download_url("", EdinetDocumentKind::FilingAndXbrl, "k").is_err());
    }

    #[test]
    fn document_download_url_send_time_rejects_duplicate_keys_and_charset() {
        let dup_type = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}S100ABCD?type=1&type=1&Subscription-Key=k"
        );
        assert_eq!(
            validate_document_download_url(
                &dup_type,
                "S100ABCD",
                EdinetDocumentKind::FilingAndXbrl
            ),
            Err(EdinetError::UrlViolation)
        );
        let dup_key = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}S100ABCD?type=1&Subscription-Key=k&Subscription-Key=k"
        );
        assert_eq!(
            validate_document_download_url(&dup_key, "S100ABCD", EdinetDocumentKind::FilingAndXbrl),
            Err(EdinetError::UrlViolation)
        );
        let bad_key_chars = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}S100ABCD?type=1&Subscription-Key=bad%20key"
        );
        assert_eq!(
            validate_document_download_url(
                &bad_key_chars,
                "S100ABCD",
                EdinetDocumentKind::FilingAndXbrl
            ),
            Err(EdinetError::UrlViolation)
        );
        let bad_key_symbol = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}S100ABCD?type=1&Subscription-Key=k*v"
        );
        assert_eq!(
            validate_document_download_url(
                &bad_key_symbol,
                "S100ABCD",
                EdinetDocumentKind::FilingAndXbrl
            ),
            Err(EdinetError::UrlViolation)
        );
        // Invalid docID charset: path matches expected, but is_doc_id must still fail closed.
        let weird_id = "S100/../X";
        let weird_url = format!(
            "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}{weird_id}?type=1&Subscription-Key=k"
        );
        assert_eq!(
            validate_document_download_url(&weird_url, weird_id, EdinetDocumentKind::FilingAndXbrl),
            Err(EdinetError::UrlViolation)
        );
    }

    #[test]
    fn classify_document_meta_octet_stream_ok_json_is_api_error_html_rejected() {
        let zip_ok = ResponseMeta {
            status: 200,
            content_type: Some("application/octet-stream; charset=binary".into()),
            content_encoding: Some("identity".into()),
            content_length: None,
        };
        assert_eq!(
            classify_document_response_meta(&zip_ok),
            Ok(DocumentResponseClass::ExpectedZip)
        );

        let json_200 = ResponseMeta {
            status: 200,
            content_type: Some("application/json; charset=utf-8".into()),
            content_encoding: None,
            content_length: None,
        };
        assert_eq!(
            classify_document_response_meta(&json_200),
            Ok(DocumentResponseClass::ApiErrorJson)
        );

        let html = ResponseMeta {
            status: 200,
            content_type: Some("text/html".into()),
            content_encoding: None,
            content_length: None,
        };
        assert_eq!(
            classify_document_response_meta(&html),
            Err(EdinetError::InvalidContentType)
        );

        let app_zip = ResponseMeta {
            status: 200,
            content_type: Some("application/zip".into()),
            content_encoding: None,
            content_length: None,
        };
        assert_eq!(
            classify_document_response_meta(&app_zip),
            Err(EdinetError::InvalidContentType)
        );
    }

    #[test]
    fn zip_magic_and_api_error_status_parsers() {
        verify_zip_local_file_magic(b"PK\x03\x04rest").expect("magic");
        assert_eq!(
            verify_zip_local_file_magic(b"PK\x05\x06"),
            Err(EdinetError::InvalidZip)
        );
        assert_eq!(
            verify_zip_local_file_magic(b"PK"),
            Err(EdinetError::InvalidZip)
        );

        assert_eq!(
            parse_document_api_error_status(br#"{"metadata":{"status":"404"},"results":null}"#),
            "404"
        );
        assert_eq!(
            parse_document_api_error_status(br#"{"StatusCode":503,"results":null}"#),
            "503"
        );
        assert_eq!(
            parse_document_api_error_status(br#"{"metadata":{"status":404}}"#),
            "404"
        );
        assert_eq!(parse_document_api_error_status(b"not-json"), "unknown");
    }

    #[tokio::test]
    async fn fetch_document_bytes_accepts_octet_stream_zip_magic() {
        struct DocBody(Option<Vec<u8>>);
        impl ResponseBody for DocBody {
            fn next_chunk(
                &mut self,
            ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
            {
                let next = self.0.take().map(Ok);
                async move { next }
            }
        }
        struct DocTransport {
            meta: ResponseMeta,
            body: Vec<u8>,
            last_url: std::sync::Mutex<Option<String>>,
        }
        impl HttpTransport for DocTransport {
            type Body = DocBody;
            fn get(
                &self,
                url: &str,
                _request_deadline: std::time::Duration,
            ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
            {
                if let Ok(mut g) = self.last_url.lock() {
                    *g = Some(url.to_string());
                }
                let meta = self.meta.clone();
                let body = self.body.clone();
                async move { Ok((meta, DocBody(Some(body)))) }
            }
        }

        let transport = DocTransport {
            meta: ResponseMeta {
                status: 200,
                content_type: Some("application/octet-stream".into()),
                content_encoding: Some("identity".into()),
                content_length: None,
            },
            body: b"PK\x03\x04fixture".to_vec(),
            last_url: std::sync::Mutex::new(None),
        };
        let bytes = fetch_document_bytes(
            &transport,
            "S100ZIP1",
            EdinetDocumentKind::FilingAndXbrl,
            "test-key",
            std::future::pending::<()>(),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("zip bytes");
        assert!(bytes.starts_with(b"PK\x03\x04"));
        let url = transport
            .last_url
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .expect("url recorded");
        assert!(url.contains("type=1"));
        // Never assert/log full URL with key in production; test only checks type query.
        assert!(url.contains("Subscription-Key="));
    }

    #[tokio::test]
    async fn fetch_document_bytes_rejects_json_even_on_http_200() {
        struct DocBody(Option<Vec<u8>>);
        impl ResponseBody for DocBody {
            fn next_chunk(
                &mut self,
            ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
            {
                let next = self.0.take().map(Ok);
                async move { next }
            }
        }
        struct DocTransport {
            meta: ResponseMeta,
            body: Vec<u8>,
        }
        impl HttpTransport for DocTransport {
            type Body = DocBody;
            fn get(
                &self,
                _url: &str,
                _request_deadline: std::time::Duration,
            ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
            {
                let meta = self.meta.clone();
                let body = self.body.clone();
                async move { Ok((meta, DocBody(Some(body)))) }
            }
        }

        let transport = DocTransport {
            meta: ResponseMeta {
                status: 200,
                content_type: Some("application/json".into()),
                content_encoding: None,
                content_length: None,
            },
            body: br#"{"metadata":{"status":"404"},"results":null}"#.to_vec(),
        };
        let err = fetch_document_bytes(
            &transport,
            "S100JSON",
            EdinetDocumentKind::XbrlCsv,
            "test-key",
            std::future::pending::<()>(),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect_err("json is api error");
        assert_eq!(
            err,
            EdinetError::ApiResponse {
                status: "404".into()
            }
        );
    }

    #[tokio::test]
    async fn fetch_document_bytes_rejects_html_and_bad_zip_magic() {
        struct DocBody(Option<Vec<u8>>);
        impl ResponseBody for DocBody {
            fn next_chunk(
                &mut self,
            ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
            {
                let next = self.0.take().map(Ok);
                async move { next }
            }
        }
        struct DocTransport {
            meta: ResponseMeta,
            body: Vec<u8>,
        }
        impl HttpTransport for DocTransport {
            type Body = DocBody;
            fn get(
                &self,
                _url: &str,
                _request_deadline: std::time::Duration,
            ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
            {
                let meta = self.meta.clone();
                let body = self.body.clone();
                async move { Ok((meta, DocBody(Some(body)))) }
            }
        }

        let html = DocTransport {
            meta: ResponseMeta {
                status: 200,
                content_type: Some("text/html".into()),
                content_encoding: None,
                content_length: None,
            },
            body: b"<html>Sorry</html>".to_vec(),
        };
        assert_eq!(
            fetch_document_bytes(
                &html,
                "S100HTML",
                EdinetDocumentKind::FilingAndXbrl,
                "test-key",
                std::future::pending::<()>(),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect_err("html"),
            EdinetError::InvalidContentType
        );

        let bad_magic = DocTransport {
            meta: ResponseMeta {
                status: 200,
                content_type: Some("application/octet-stream".into()),
                content_encoding: None,
                content_length: None,
            },
            body: b"%PDF-1.4 not a zip".to_vec(),
        };
        assert_eq!(
            fetch_document_bytes(
                &bad_magic,
                "S100PDF",
                EdinetDocumentKind::FilingAndXbrl,
                "test-key",
                std::future::pending::<()>(),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect_err("bad magic"),
            EdinetError::InvalidZip
        );
    }

    #[test]
    fn rejects_bad_date() {
        assert!(build_documents_list_url("20240625", "key").is_err());
    }

    #[test]
    fn parse_list_and_select_strict_120() {
        let body = wrap_results(&[eligible_120("S100TEST1", "2024-06-25 15:00")]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        let sel = select_eligible_yuho_original(&docs).expect("select");
        assert_eq!(opt_trim(&sel.original.doc_id), "S100TEST1");
        assert!(!sel.correction_available);
        let facts = facts_from_document_meta(&sel.original);
        assert!(facts.company_name.contains("テスト"));
        let by_name = select_yuho_by_filer_name(&docs, "テスト").expect("name");
        assert_eq!(opt_trim(&by_name.edinet_code), "E02144");
        assert_eq!(normalize_filer_key("テスト株式会社"), "テスト");
        assert_eq!(prev_ymd("2024-06-01").as_deref(), Some("2024-05-31"));
        assert_eq!(recent_filing_dates("2024-06-03", 2).len(), 3);
    }

    #[test]
    fn null_fields_are_accepted() {
        let body = r#"{
          "results": [{
            "docID": "S100NULL1",
            "edinetCode": "E02144",
            "filerName": "テスト株式会社",
            "ordinanceCode": null,
            "formCode": null,
            "docTypeCode": "120",
            "periodStart": null,
            "periodEnd": null,
            "submitDateTime": "2024-06-25 15:00",
            "parentDocID": null,
            "withdrawalStatus": "0",
            "docInfoEditStatus": null,
            "disclosureStatus": "0",
            "xbrlFlag": "1",
            "csvFlag": null,
            "legalStatus": "1",
            "docDescription": null,
            "secCode": null
          }]
        }"#;
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("null ok");
        assert_eq!(docs.len(), 1);
        let row = docs.first().expect("one");
        assert!(row.parent_doc_id.is_none());
        assert!(row.ordinance_code.is_none());
        let sel = select_eligible_yuho_original(&docs).expect("eligible");
        assert_eq!(opt_trim(&sel.original.doc_id), "S100NULL1");
    }

    #[test]
    fn candidates_beyond_index_500_are_still_found() {
        let mut rows = Vec::with_capacity(501);
        for i in 0..500 {
            rows.push(format!(
                r#"{{"docID":"S100PAD{i:04}","edinetCode":"E00099","filerName":"他社","docTypeCode":"120","submitDateTime":"2024-01-01 09:00","withdrawalStatus":"0","disclosureStatus":"0","xbrlFlag":"1","legalStatus":"1"}}"#
            ));
        }
        rows.push(eligible_120("S100LATE", "2024-06-25 15:00"));
        let body = wrap_results(&rows);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse past 500");
        assert_eq!(docs.len(), 1);
        assert_eq!(opt_trim(&docs.first().expect("one").doc_id), "S100LATE");
        select_eligible_yuho_original(&docs).expect("select late row");
    }

    #[test]
    fn candidate_limit_exceeded_soft_fails() {
        let mut rows = Vec::with_capacity(MAX_EDINET_LIST_CANDIDATES + 1);
        for i in 0..=MAX_EDINET_LIST_CANDIDATES {
            rows.push(format!(
                r#"{{"docID":"S100C{i:03}","edinetCode":"E02144","filerName":"テスト株式会社","docTypeCode":"120","submitDateTime":"2024-06-{:02} 10:00","withdrawalStatus":"0","disclosureStatus":"0","xbrlFlag":"1","legalStatus":"1"}}"#,
                (i % 28) + 1
            ));
        }
        let body = wrap_results(&rows);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let err = collect_list_candidates(body.as_bytes(), &filter).expect_err("limit");
        assert_eq!(err, EdinetError::CandidateLimitExceeded);
    }

    #[test]
    fn no_120_is_no_eligible_filing() {
        let body = wrap_results(&[format!(
            r#"{{
              "docID": "S100ONLY130",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-01 10:00",
              "parentDocID": "S100MISSING",
              "withdrawalStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
        )]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            select_eligible_yuho_original(&docs).expect_err("no 120"),
            EdinetError::NoEligibleFiling
        );
    }

    #[test]
    fn correction_130_alone_never_selected_as_original() {
        // Same as no_120 — explicit contract name for 130-only soft-fail.
        let body = wrap_results(&[format!(
            r#"{{
              "docID": "S100CORR",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-01 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
        )]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert!(select_yuho_document(&docs, "E02144").is_none());
        assert_eq!(
            select_eligible_yuho_original(&docs).unwrap_err(),
            EdinetError::NoEligibleFiling
        );
    }

    #[test]
    fn correction_available_when_130_points_at_selected_120() {
        let body = wrap_results(&[
            eligible_120("S100ORIG", "2024-06-25 15:00"),
            format!(
                r#"{{
              "docID": "S100CORR",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-01 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
            ),
        ]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        let sel = select_eligible_yuho_original(&docs).expect("select");
        assert_eq!(opt_trim(&sel.original.doc_id), "S100ORIG");
        assert_eq!(opt_trim(&sel.original.doc_type_code), "120");
        assert!(sel.correction_available);
    }

    #[test]
    fn same_submit_datetime_tie_is_ambiguous() {
        let body = wrap_results(&[
            eligible_120("S100A", "2024-06-25 15:00"),
            eligible_120("S100B", "2024-06-25 15:00"),
        ]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            select_eligible_yuho_original(&docs).expect_err("tie"),
            EdinetError::AmbiguousSelection
        );
    }

    #[test]
    fn doc_id_duplicates_are_deduped_before_candidate_cap() {
        let body = wrap_results(&[
            eligible_120("S100SAME", "2024-06-25 15:00"),
            eligible_120("S100SAME", "2024-06-25 15:00"),
        ]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            docs.len(),
            1,
            "collector must dedupe docID before retaining"
        );
        let sel = select_eligible_yuho_original(&docs).expect("dedupe");
        assert_eq!(opt_trim(&sel.original.doc_id), "S100SAME");
    }

    #[test]
    fn duplicate_doc_ids_do_not_consume_candidate_budget() {
        let mut rows = Vec::with_capacity(MAX_EDINET_LIST_CANDIDATES * 3);
        for i in 0..MAX_EDINET_LIST_CANDIDATES {
            let id = format!("S100U{i:03}");
            // Three identical copies per unique id — must count as one toward the cap.
            rows.push(eligible_120(&id, "2024-06-25 15:00"));
            rows.push(eligible_120(&id, "2024-06-25 15:00"));
            rows.push(eligible_120(&id, "2024-06-25 15:00"));
        }
        let body = wrap_results(&rows);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("within cap");
        assert_eq!(docs.len(), MAX_EDINET_LIST_CANDIDATES);
    }

    #[test]
    fn same_filer_name_distinct_edinet_codes_are_ambiguous() {
        let body = wrap_results(&[
            eligible_120_for("E02144", "S100A", "2024-06-25 15:00"),
            eligible_120_for("E09999", "S100B", "2024-06-26 15:00"),
        ]);
        let filter = ListCandidateFilter::by_filer_name("テスト").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(docs.len(), 2);
        assert_eq!(
            select_eligible_yuho_original(&docs).expect_err("multi-code"),
            EdinetError::AmbiguousSelection
        );
    }

    #[test]
    fn trailing_json_after_root_is_malformed() {
        let mut body = wrap_results(&[eligible_120("S100TRAIL", "2024-06-25 15:00")]);
        body.push_str("null");
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        assert_eq!(
            collect_list_candidates(body.as_bytes(), &filter).expect_err("trailing"),
            EdinetError::Malformed
        );
    }

    #[test]
    fn empty_submit_datetime_is_not_eligible() {
        let body = wrap_results(&[gated_120("S100NOSUB", "0", "0", "1", Some("1"), None)]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            select_eligible_yuho_original(&docs).unwrap_err(),
            EdinetError::NoEligibleFiling
        );
    }

    #[test]
    fn correction_ignores_other_edinet_code_and_bad_status_130() {
        let body = wrap_results(&[
            eligible_120("S100ORIG", "2024-06-25 15:00"),
            format!(
                r#"{{
              "docID": "S100OTHER",
              "edinetCode": "E09999",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-01 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
            ),
            format!(
                r#"{{
              "docID": "S100WD",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-02 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "2",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
            ),
            format!(
                r#"{{
              "docID": "S100ND",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-03 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "0",
              "disclosureStatus": "2",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
            ),
            format!(
                r#"{{
              "docID": "S100BADL",
              "edinetCode": "E02144",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-04 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "0"
            }}"#
            ),
        ]);
        // Code filter keeps E02144 120 + ineligible same-code 130s; other-code 130 is dropped.
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        let sel = select_eligible_yuho_original(&docs).expect("select");
        assert!(
            !sel.correction_available,
            "other-code / withdrawn / non-disclosed / bad-legal 130 must not count"
        );
    }

    #[test]
    fn correction_other_code_ignored_under_name_filter() {
        let body = wrap_results(&[
            eligible_120("S100ORIG", "2024-06-25 15:00"),
            format!(
                r#"{{
              "docID": "S100OTHER",
              "edinetCode": "E09999",
              "filerName": "テスト株式会社",
              "docTypeCode": "130",
              "submitDateTime": "2024-07-01 10:00",
              "parentDocID": "S100ORIG",
              "withdrawalStatus": "0",
              "disclosureStatus": "0",
              "xbrlFlag": "1",
              "legalStatus": "1"
            }}"#
            ),
        ]);
        let filter = ListCandidateFilter::by_filer_name("テスト").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        let sel = select_eligible_yuho_original(&docs).expect("one original code");
        assert!(!sel.correction_available);
    }

    #[test]
    fn withdrawal_status_gate_rejects_120() {
        let body = wrap_results(&[gated_120(
            "S100WD",
            "2",
            "0",
            "1",
            Some("1"),
            Some("2024-06-25 15:00"),
        )]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            select_eligible_yuho_original(&docs).unwrap_err(),
            EdinetError::NoEligibleFiling
        );
    }

    #[test]
    fn disclosure_status_gate_rejects_non_disclosed_120() {
        let body = wrap_results(&[gated_120(
            "S100ND",
            "0",
            "2",
            "1",
            Some("1"),
            Some("2024-06-25 15:00"),
        )]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            select_eligible_yuho_original(&docs).unwrap_err(),
            EdinetError::NoEligibleFiling
        );
    }

    #[test]
    fn xbrl_flag_gate_rejects_120_without_xbrl() {
        let body = wrap_results(&[gated_120(
            "S100NX",
            "0",
            "0",
            "0",
            Some("1"),
            Some("2024-06-25 15:00"),
        )]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        assert_eq!(
            select_eligible_yuho_original(&docs).unwrap_err(),
            EdinetError::NoEligibleFiling
        );
    }

    #[test]
    fn legal_status_one_and_two_are_eligible() {
        for legal in ["1", "2"] {
            let body = wrap_results(&[gated_120(
                "S100LG",
                "0",
                "0",
                "1",
                Some(legal),
                Some("2024-06-25 15:00"),
            )]);
            let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
            let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
            select_eligible_yuho_original(&docs).unwrap_or_else(|_| panic!("legal={legal}"));
        }
    }

    #[test]
    fn legal_status_invalid_or_null_rejects_120() {
        for legal in [Some("0"), Some("9"), None] {
            let body = wrap_results(&[gated_120(
                "S100BADL",
                "0",
                "0",
                "1",
                legal,
                Some("2024-06-25 15:00"),
            )]);
            let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
            let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
            assert_eq!(
                select_eligible_yuho_original(&docs).unwrap_err(),
                EdinetError::NoEligibleFiling
            );
        }
    }

    #[test]
    fn list_body_between_1mib_and_8mib_is_accepted() {
        // Pad inside JSON so Deserializer::end() still succeeds; >1MiB and ≤8MiB.
        let pad_len = 1024 * 1024 + 64;
        let row = eligible_120("S100PAD1M", "2024-06-25 15:00");
        let body = format!(r#"{{"pad":"{}","results":[{}]}}"#, "x".repeat(pad_len), row);
        assert!(body.len() > 1024 * 1024);
        assert!(body.len() <= MAX_EDINET_LIST_BYTES);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("8MiB path");
        assert_eq!(docs.len(), 1);
    }

    #[test]
    fn list_body_over_8mib_is_rejected() {
        let body = vec![b'x'; MAX_EDINET_LIST_BYTES.saturating_add(1)];
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        assert_eq!(
            collect_list_candidates(&body, &filter).expect_err("over 8MiB"),
            EdinetError::Malformed
        );
    }

    #[test]
    fn extract_risk_section() {
        let text = "前文\n【事業の内容】\nクラウド事業を展開。\n【事業等のリスク】\n為替変動リスクがある。\n【経営成績】\n売上高は増加した。\n末尾";
        let sections = extract_sections_from_text(text);
        assert!(sections.business_summary.contains("クラウド"));
        assert!(sections.business_risks.contains("為替"));
        assert!(sections.performance_summary.contains("売上高"));
    }

    #[test]
    fn name_only_company_facts_default_is_valid_sanitize_input() {
        // Step 1 contract: fetch_edinet_company_facts builds this shape as fallback base.
        let facts = CompanyFacts {
            company_name: "トヨタ自動車".into(),
            ..CompanyFacts::default()
        };
        let out = sanitize_company_facts(&facts).expect("sanitize");
        assert_eq!(out.company_name, "トヨタ自動車");
        assert!(out.edinet_code.is_empty());
        assert!(out.business_summary.is_empty());
        assert!(out.business_risks.is_empty());
        assert!(out.performance_summary.is_empty());
    }

    #[test]
    fn per_field_capped_facts_pass_total_cap() {
        // Step 9 contradiction fix: 3 narrative fields + name at the per-field
        // cap must not trip the aggregate cap (12,000 < 4,096 × 3 + name did).
        let full = "a".repeat(MAX_FACT_FIELD_BYTES);
        let facts = CompanyFacts {
            company_name: full.clone(),
            business_summary: full.clone(),
            business_risks: full.clone(),
            performance_summary: full,
            ..CompanyFacts::default()
        };
        let out = sanitize_company_facts(&facts).expect("capped fields fit total");
        assert_eq!(out.business_summary.len(), MAX_FACT_FIELD_BYTES);
        assert_eq!(out.performance_summary.len(), MAX_FACT_FIELD_BYTES);
    }

    #[test]
    fn acquisition_from_selected_with_partials_maps_fields_and_evidence() {
        use crate::knowledge::edinet_csv::{
            Consolidation, EdinetEvidenceSection, ExtractedFinancialFact, ExtractedNarrative,
            FinancialConcept, NarrativeConcept, PartialEdinetFacts, PeriodKind, RelativeYear,
        };

        let body = wrap_results(&[eligible_120("S100PART", "2024-06-25 15:00")]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        let selected = select_eligible_yuho_original(&docs).expect("select");

        let financials = PartialEdinetFacts {
            financials: vec![ExtractedFinancialFact {
                concept: FinancialConcept::Revenue,
                element_id: "jppfs_cor_NetSales".into(),
                label: "売上高".into(),
                context_id: "CurrentYearDuration".into(),
                relative_year: RelativeYear::Current,
                consolidation: Consolidation::Consolidated,
                period_kind: PeriodKind::Duration,
                unit_id: "JPY".into(),
                unit_label: "円".into(),
                value_text: "100".into(),
            }],
            narratives: Vec::new(),
            evidence: Vec::new(),
            warnings: Vec::new(),
        };
        let narratives = PartialEdinetFacts {
            financials: Vec::new(),
            narratives: vec![
                ExtractedNarrative {
                    concept: NarrativeConcept::DescriptionOfBusiness,
                    local_name: "DescriptionOfBusinessTextBlock".into(),
                    text: "事業の内容本文".into(),
                    truncated: false,
                },
                ExtractedNarrative {
                    concept: NarrativeConcept::BusinessRisks,
                    local_name: "BusinessRisksTextBlock".into(),
                    text: "リスク本文".into(),
                    truncated: false,
                },
            ],
            evidence: vec![EdinetEvidenceSection {
                doc_id: "S100PART".into(),
                edinet_code: "E02144".into(),
                submitted_at: "2024-06-25 15:00".into(),
                period_start: Some("2023-04-01".into()),
                period_end: Some("2024-03-31".into()),
                concept: NarrativeConcept::BusinessRisks,
                local_name: "BusinessRisksTextBlock".into(),
                unit: None,
                text: "リスク本文".into(),
                truncated: false,
            }],
            warnings: Vec::new(),
        };

        let (acq, evidence, _warnings) = acquisition_from_selected_with_partials(
            &selected,
            Some(&financials),
            Some(&narratives),
        )
        .expect("partials");
        assert!(acq.fields().performance_summary);
        assert!(acq.fields().business_summary);
        assert!(acq.fields().business_risks);
        assert!(acq.facts().performance_summary.contains("100"));
        assert_eq!(acq.facts().business_summary, "事業の内容本文");
        assert_eq!(acq.facts().business_risks, "リスク本文");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].text, "リスク本文");
    }

    #[test]
    fn acquisition_from_selected_with_partials_empty_keeps_fields_false() {
        let body = wrap_results(&[eligible_120("S100EMPTY", "2024-06-25 15:00")]);
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let docs = collect_list_candidates(body.as_bytes(), &filter).expect("parse");
        let selected = select_eligible_yuho_original(&docs).expect("select");
        let (acq, evidence, warnings) =
            acquisition_from_selected_with_partials(&selected, None, None).expect("empty");
        assert!(!acq.fields().performance_summary);
        assert!(!acq.fields().business_summary);
        assert!(!acq.fields().business_risks);
        assert!(evidence.is_empty());
        assert!(warnings.is_empty());
        assert!(!acq.facts().company_name.is_empty());
    }
}
