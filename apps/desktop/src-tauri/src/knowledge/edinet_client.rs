//! EDINET API v2 client foundation (M12).
//!
//! **No `reqwest` here** — constitutional guard confines `reqwest::` to
//! `net_gateway.rs`. This module owns fixed-template URL construction, send-time
//! URL validation, JSON list parsing, and heuristic section extraction from
//! UTF-8 filing text. Live HTTP goes through [`crate::knowledge::net_gateway`]
//! + dual-factor egress (`NetworkPolicy::Live` ∧ `egress-live`).
//!
//! Offline path: callers may inject [`CompanyFacts`] directly (no network).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use serde::{Deserialize, Serialize};

use crate::knowledge::net_gateway::{
    fetch_bounded_with_deadline, safe_truncate, validate_response_meta, GatewayError, HttpTransport,
    ResponseMeta, MAX_RESPONSE_BYTES,
};
use crate::knowledge::render_guard::sanitize_external_text;

/// Fixed EDINET API host (no alternate hosts / open redirects).
pub const EDINET_HOST: &str = "api.edinet-fsa.go.jp";
pub const EDINET_LIST_PATH: &str = "/api/v2/documents.json";
pub const EDINET_DOC_PATH_PREFIX: &str = "/api/v2/documents/";

/// Soft caps for prompt-safe company fact fields.
pub const MAX_FACT_FIELD_BYTES: usize = 4_096;
pub const MAX_COMPANY_FACTS_BYTES: usize = 12_000;

const DOC_TYPE_YUHO: &str = "120";
const DOC_TYPE_YUHO_CORRECTION: &str = "130";

/// One row from `documents.json` `results[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdinetDocumentMeta {
    pub doc_id: String,
    pub edinet_code: String,
    pub filer_name: String,
    pub doc_type_code: String,
    pub doc_description: String,
    pub period_start: String,
    pub period_end: String,
    pub submit_date_time: String,
    pub sec_code: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdinetError {
    UrlViolation,
    InvalidArgument,
    Malformed,
    Gateway(GatewayError),
    ApiKeyMissing,
}

impl std::fmt::Display for EdinetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UrlViolation => write!(f, "edinet_url_violation"),
            Self::InvalidArgument => write!(f, "edinet_invalid_argument"),
            Self::Malformed => write!(f, "edinet_malformed"),
            Self::Gateway(e) => write!(f, "edinet_gateway:{e}"),
            Self::ApiKeyMissing => write!(f, "edinet_api_key_missing"),
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
pub fn build_documents_list_url(
    date: &str,
    subscription_key: &str,
) -> Result<String, EdinetError> {
    if !is_ymd(date) || !is_subscription_key(subscription_key) {
        return Err(EdinetError::InvalidArgument);
    }
    Ok(format!(
        "https://{EDINET_HOST}{EDINET_LIST_PATH}?date={}&type=2&Subscription-Key={}",
        percent_encode_query_param(date),
        percent_encode_query_param(subscription_key.trim())
    ))
}

/// Build the ONLY permitted document-download URL (`type=1` = XBRL ZIP meta path).
/// Body parsing of ZIP is out of scope for M12 foundation; URL is validated for
/// future extractors. Prefer UTF-8 text injection + [`extract_sections_from_text`].
pub fn build_document_download_url(
    doc_id: &str,
    subscription_key: &str,
) -> Result<String, EdinetError> {
    if !is_doc_id(doc_id) || !is_subscription_key(subscription_key) {
        return Err(EdinetError::InvalidArgument);
    }
    Ok(format!(
        "https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}{}?type=1&Subscription-Key={}",
        percent_encode_query_param(doc_id.trim()),
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

/// Send-time assertion for `/documents/{docID}` URLs.
pub fn validate_document_download_url(url: &str, expected_doc_id: &str) -> Result<(), EdinetError> {
    let prefix = format!("https://{EDINET_HOST}{EDINET_DOC_PATH_PREFIX}");
    let rest = url.strip_prefix(&prefix).ok_or(EdinetError::UrlViolation)?;
    let (id_part, query) = rest.split_once('?').ok_or(EdinetError::UrlViolation)?;
    if id_part != expected_doc_id {
        return Err(EdinetError::UrlViolation);
    }
    let mut type_ok = false;
    let mut key_ok = false;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().ok_or(EdinetError::UrlViolation)?;
        let v = it.next().ok_or(EdinetError::UrlViolation)?;
        match k {
            "type" => {
                if v != "1" {
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
    if type_ok && key_ok {
        Ok(())
    } else {
        Err(EdinetError::UrlViolation)
    }
}

fn json_str_field(obj: &serde_json::Value, key: &str) -> String {
    obj.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Parse `documents.json` body into metadata rows (yuho-focused filter optional).
pub fn parse_documents_list(body: &[u8]) -> Result<Vec<EdinetDocumentMeta>, EdinetError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(EdinetError::Malformed);
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| EdinetError::Malformed)?;
    let results = value
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or(EdinetError::Malformed)?;

    let mut out = Vec::new();
    for item in results.iter().take(500) {
        let doc_id = json_str_field(item, "docID");
        if !is_doc_id(&doc_id) {
            continue;
        }
        let mut doc_type = json_str_field(item, "docTypeCode");
        doc_type = doc_type.replace(['\'', '"'], "");
        out.push(EdinetDocumentMeta {
            doc_id,
            edinet_code: json_str_field(item, "edinetCode"),
            filer_name: json_str_field(item, "filerName"),
            doc_type_code: doc_type,
            doc_description: json_str_field(item, "docDescription"),
            period_start: json_str_field(item, "periodStart"),
            period_end: json_str_field(item, "periodEnd"),
            submit_date_time: json_str_field(item, "submitDateTime"),
            sec_code: json_str_field(item, "secCode"),
        });
    }
    Ok(out)
}

/// Prefer 有価証券報告書 (120/130) matching `edinet_code` or filer name substring.
pub fn select_yuho_document<'a>(
    docs: &'a [EdinetDocumentMeta],
    edinet_code: &str,
) -> Option<&'a EdinetDocumentMeta> {
    let code = edinet_code.trim();
    if code.is_empty() {
        return None;
    }
    docs.iter().find(|d| {
        d.edinet_code.eq_ignore_ascii_case(code)
            && (d.doc_type_code == DOC_TYPE_YUHO
                || d.doc_type_code == DOC_TYPE_YUHO_CORRECTION)
    })
    .or_else(|| {
        docs.iter()
            .find(|d| d.edinet_code.eq_ignore_ascii_case(code))
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
        if matches!(ch, ' ' | '\u{3000}' | '・' | '･' | '.' | '/' | '／' | '-' | 'ー' | '—' | '_')
        {
            continue;
        }
        for c in ch.to_lowercase() {
            out.push(c);
        }
    }
    out
}

fn is_yuho_type(doc_type: &str) -> bool {
    doc_type == DOC_TYPE_YUHO || doc_type == DOC_TYPE_YUHO_CORRECTION
}

/// Match EDINET list rows by filer name (exact normalized key, then containment).
pub fn select_yuho_by_filer_name<'a>(
    docs: &'a [EdinetDocumentMeta],
    company_name: &str,
) -> Option<&'a EdinetDocumentMeta> {
    let q = normalize_filer_key(company_name);
    if q.len() < 2 {
        return None;
    }

    let mut best: Option<(i32, &'a EdinetDocumentMeta)> = None;
    for d in docs {
        let f = normalize_filer_key(&d.filer_name);
        if f.len() < 2 {
            continue;
        }
        let score = if f == q {
            100
        } else if f.contains(&q) || q.contains(&f) {
            let shorter = f.len().min(q.len());
            let longer = f.len().max(q.len()).max(1);
            70 + ((shorter * 20) / longer) as i32
        } else {
            continue;
        };
        let score = if is_yuho_type(&d.doc_type_code) {
            score + 10
        } else {
            score
        };
        match best {
            Some((prev, _)) if prev >= score => {}
            _ => best = Some((score, d)),
        }
    }
    best.map(|(_, d)| d)
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
    let (py, pm) = if m > 1 {
        (y, m - 1)
    } else {
        (y - 1, 12)
    };
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
        meta.doc_description,
        meta.doc_type_code,
        meta.period_start,
        meta.period_end,
        meta.submit_date_time,
        meta.sec_code
    );
    CompanyFacts {
        company_name: safe_truncate(&meta.filer_name, MAX_FACT_FIELD_BYTES),
        edinet_code: safe_truncate(&meta.edinet_code, 32),
        doc_id: safe_truncate(&meta.doc_id, 32),
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
        &["事業等のリスク", "経営者による", "財政状態", "【事業等のリスク】"],
    );
    let risks = find_section(
        text,
        &["事業等のリスク", "【事業等のリスク】", "リスク情報"],
        &["経営者による", "財政状態", "重要な会計", "研究開発活動"],
    );
    let perf = find_section(
        text,
        &["経営成績", "財政状態、経営成績", "【経営成績】", "業績の概要"],
        &["キャッシュ・フロー", "生産、受注", "研究開発", "事業等のリスク"],
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

/// Fail-closed sanitize of all fact fields (external evidence discipline).
pub fn sanitize_company_facts(facts: &CompanyFacts) -> Result<CompanyFacts, EdinetError> {
    let company_name = sanitize_external_text(&facts.company_name, MAX_FACT_FIELD_BYTES)
        .map_err(|_| EdinetError::Malformed)?;
    let edinet_code = sanitize_external_text(&facts.edinet_code, 64)
        .map_err(|_| EdinetError::Malformed)?;
    let doc_id = sanitize_external_text(&facts.doc_id, 64).map_err(|_| EdinetError::Malformed)?;
    let business_summary =
        sanitize_external_text(&facts.business_summary, MAX_FACT_FIELD_BYTES)
            .map_err(|_| EdinetError::Malformed)?;
    let business_risks = sanitize_external_text(&facts.business_risks, MAX_FACT_FIELD_BYTES)
        .map_err(|_| EdinetError::Malformed)?;
    let performance_summary =
        sanitize_external_text(&facts.performance_summary, MAX_FACT_FIELD_BYTES)
            .map_err(|_| EdinetError::Malformed)?;
    let source =
        sanitize_external_text(&facts.source, 64).map_err(|_| EdinetError::Malformed)?;

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

/// Async list fetch via injected [`HttpTransport`] (tests: fake; live: ReqwestTransport).
pub async fn fetch_documents_list<T: HttpTransport>(
    transport: &T,
    date: &str,
    subscription_key: &str,
    deadline: std::time::Duration,
) -> Result<Vec<EdinetDocumentMeta>, EdinetError> {
    let url = build_documents_list_url(date, subscription_key)?;
    validate_documents_list_url(&url, date)?;
    let (meta, body) = transport.get(&url).await?;
    validate_edinet_json_meta(&meta)?;
    let bytes = fetch_bounded_with_deadline(body, std::future::pending::<()>(), deadline).await?;
    parse_documents_list(&bytes)
}

/// Fetch list and resolve yuho facts for `edinet_code` (optional body text merge).
pub async fn fetch_company_facts_by_code<T: HttpTransport>(
    transport: &T,
    date: &str,
    edinet_code: &str,
    subscription_key: &str,
    body_text: Option<&str>,
    deadline: std::time::Duration,
) -> Result<CompanyFacts, EdinetError> {
    let docs = fetch_documents_list(transport, date, subscription_key, deadline).await?;
    let meta = select_yuho_document(&docs, edinet_code).ok_or(EdinetError::Malformed)?;
    merge_company_facts(meta, body_text)
}

/// Resolve company facts by filer name: scan recent filing dates until a match.
pub async fn fetch_company_facts_by_name<T: HttpTransport>(
    transport: &T,
    anchor_date: &str,
    company_name: &str,
    subscription_key: &str,
    body_text: Option<&str>,
    deadline: std::time::Duration,
    days_back: u32,
) -> Result<CompanyFacts, EdinetError> {
    let name = company_name.trim();
    if name.is_empty() || normalize_filer_key(name).len() < 2 {
        return Err(EdinetError::InvalidArgument);
    }
    for date in recent_filing_dates(anchor_date, days_back) {
        let docs = match fetch_documents_list(transport, &date, subscription_key, deadline).await {
            Ok(d) => d,
            Err(_) => continue,
        };
        if let Some(meta) = select_yuho_by_filer_name(&docs, name) {
            return merge_company_facts(meta, body_text);
        }
    }
    Err(EdinetError::Malformed)
}

/// Drain a response body with cancel/deadline (re-export helper for document bytes).
pub async fn fetch_document_bytes<T: HttpTransport>(
    transport: &T,
    doc_id: &str,
    subscription_key: &str,
    cancel: impl std::future::Future<Output = ()>,
    deadline: std::time::Duration,
) -> Result<Vec<u8>, EdinetError> {
    let url = build_document_download_url(doc_id, subscription_key)?;
    validate_document_download_url(&url, doc_id.trim())?;
    let (meta, body) = transport.get(&url).await?;
    // Document ZIP is not application/json — allow octet-stream / zip / json.
    if meta.status != 200 {
        return Err(GatewayError::StatusRejected.into());
    }
    match meta.content_encoding.as_deref() {
        None | Some("identity") => {}
        Some(_) => return Err(GatewayError::WireViolation.into()),
    }
    let ct = meta
        .content_type
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ct_ok = ct.contains("json")
        || ct.contains("zip")
        || ct.contains("octet-stream")
        || ct.contains("pdf")
        || ct.is_empty();
    if !ct_ok {
        return Err(GatewayError::StatusRejected.into());
    }
    fetch_bounded_with_deadline(body, cancel, deadline)
        .await
        .map_err(EdinetError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_url_round_trip_validate() {
        let url = build_documents_list_url("2024-06-25", "test-key-abc").expect("url");
        assert!(url.contains(EDINET_HOST));
        validate_documents_list_url(&url, "2024-06-25").expect("validate");
    }

    #[test]
    fn rejects_bad_date() {
        assert!(build_documents_list_url("20240625", "key").is_err());
    }

    #[test]
    fn parse_list_and_select() {
        let body = format!(
            r#"{{
          "results": [
            {{
              "docID": "S100TEST1",
              "edinetCode": "E02144",
              "filerName": "{}",
              "docTypeCode": "120",
              "docDescription": "{}",
              "periodStart": "2023-04-01",
              "periodEnd": "2024-03-31",
              "submitDateTime": "2024-06-25 15:00",
              "secCode": "72030"
            }}
          ]
        }}"#,
            "テスト株式会社",
            "有価証券報告書"
        );
        let docs = parse_documents_list(body.as_bytes()).expect("parse");
        let meta = select_yuho_document(&docs, "E02144").expect("select");
        assert_eq!(meta.doc_id, "S100TEST1");
        let facts = facts_from_document_meta(meta);
        assert!(facts.company_name.contains("テスト"));
        let by_name = select_yuho_by_filer_name(&docs, "テスト").expect("name");
        assert_eq!(by_name.edinet_code, "E02144");
        assert_eq!(normalize_filer_key("テスト株式会社"), "テスト");
        assert_eq!(prev_ymd("2024-06-01").as_deref(), Some("2024-05-31"));
        assert_eq!(recent_filing_dates("2024-06-03", 2).len(), 3);
    }

    #[test]
    fn extract_risk_section() {
        let text = "前文\n【事業の内容】\nクラウド事業を展開。\n【事業等のリスク】\n為替変動リスクがある。\n【経営成績】\n売上高は増加した。\n末尾";
        let sections = extract_sections_from_text(text);
        assert!(sections.business_summary.contains("クラウド"));
        assert!(sections.business_risks.contains("為替"));
        assert!(sections.performance_summary.contains("売上高"));
    }
}
