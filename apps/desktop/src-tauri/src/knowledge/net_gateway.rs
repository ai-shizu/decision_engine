//! E0b Egress Gateway (STEP 5). The ONLY file in this crate permitted to name
//! `reqwest::` (enforced by `tests/test_e0b_constitutional_guard.py` scoped guard).
//! Parent: docs/SPEC_E0B_STEP5_EGRESS_GATEWAY.md (feature-isolation update).
//!
//! Raw socket / low-level HTTP crate symbols remain forbidden in this file too
//! (§1.4) — all egress goes through `reqwest`, never raw sockets.
//!
//! # TLS feature isolation
//! The default build (`cargo build`/`cargo test`, no extra flags) links `reqwest`
//! with **zero TLS provider** (`features = ["stream"]` only in Cargo.toml), so no
//! C toolchain (aws-lc-sys / ring) is ever invoked by ordinary offline test runs.
//! Everything in this file that is testable offline (URL construction, deny-table
//! wiring, bounded-stream reading, JSON extraction, FSM/verify-gate integration)
//! is transport-agnostic via the [`HttpTransport`] / [`ResponseBody`] seam and
//! [`crate::knowledge::dns_guard::HostResolver`] seam — fakes are injected in
//! tests, never a real TLS client. The one piece of code that actually needs a
//! TLS provider (`ReqwestTransport`, a real `HttpTransport` impl backed by
//! `reqwest::Client`) is compiled ONLY under `#[cfg(feature = "egress-live")]`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::collections::BTreeSet;
use std::future::Future;
use std::net::IpAddr;
use std::time::Duration;

use crate::knowledge::dns_guard::{is_disallowed_ip, HostResolver};
use crate::knowledge::dual_run::{verify_and_gate, AttestedIntentPayload};
use crate::knowledge::fsm::{AbortReason, FsmError, ReadyToIntegrate, ResearchSlot, Txn};
use crate::knowledge::VerifyError;

pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_RESULTS_PER_QUERY: usize = 3;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_SNIPPET_BYTES: usize = 2048;

pub const WIKI_HOST: &str = "ja.wikipedia.org";
pub const WIKI_PATH: &str = "/w/api.php";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    UrlViolation,
    DnsDenied,
    WireViolation,
    StatusRejected,
    Malformed,
    Timeout,
    Cancelled,
    Fsm(FsmError),
    Verify(VerifyError),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UrlViolation => write!(f, "outbound URL violates fixed-template invariant"),
            Self::DnsDenied => write!(f, "resolved IP(s) denied by SSRF deny-table"),
            Self::WireViolation => write!(f, "response wire violates identity/size contract"),
            Self::StatusRejected => write!(f, "response status/content-type rejected"),
            Self::Malformed => write!(f, "response body malformed"),
            Self::Timeout => write!(f, "fetch deadline exceeded"),
            Self::Cancelled => write!(f, "fetch cancelled"),
            Self::Fsm(e) => write!(f, "fsm error: {e}"),
            Self::Verify(e) => write!(f, "verify error: {e}"),
        }
    }
}

impl std::error::Error for GatewayError {}

impl From<FsmError> for GatewayError {
    fn from(e: FsmError) -> Self {
        Self::Fsm(e)
    }
}

impl From<VerifyError> for GatewayError {
    fn from(e: VerifyError) -> Self {
        Self::Verify(e)
    }
}

// ---------------------------------------------------------------------------
// Transport / body seam (STEP 5.C) — no real network in the default test suite.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMeta {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_encoding: Option<String>,
}

/// Chunked response body seam. Production (`egress-live`) wraps
/// `reqwest::Response::bytes_stream()`; tests inject fakes.
pub trait ResponseBody: Send {
    fn next_chunk(&mut self) -> impl Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send;
}

/// HTTP transport seam (STEP 5.C/5.E). Production (`egress-live`) wraps
/// `reqwest::Client`; tests inject fakes with call-count instrumentation.
pub trait HttpTransport: Send + Sync {
    type Body: ResponseBody;
    fn get(&self, url: &str) -> impl Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send;
}

/// Read a bounded body: stops (and drops the stream) the instant the running
/// total would exceed `MAX_RESPONSE_BYTES`, never appending the offending chunk.
pub async fn read_bounded_body<B: ResponseBody>(mut body: B) -> Result<Vec<u8>, GatewayError> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match body.next_chunk().await {
            None => break,
            Some(Err(e)) => return Err(e),
            Some(Ok(chunk)) => {
                if buf.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err(GatewayError::WireViolation);
                }
                buf.extend_from_slice(&chunk);
            }
        }
    }
    Ok(buf)
}

/// Validate status/content-type/content-encoding BEFORE any body byte is read.
pub fn validate_response_meta(meta: &ResponseMeta) -> Result<(), GatewayError> {
    if meta.status != 200 {
        return Err(GatewayError::StatusRejected);
    }
    let ct_ok = meta
        .content_type
        .as_deref()
        .map(|ct| ct.trim_start().to_ascii_lowercase().starts_with("application/json"))
        .unwrap_or(false);
    if !ct_ok {
        return Err(GatewayError::StatusRejected);
    }
    match meta.content_encoding.as_deref() {
        None => Ok(()),
        Some(enc) if enc.trim().eq_ignore_ascii_case("identity") => Ok(()),
        Some(_) => Err(GatewayError::WireViolation),
    }
}

/// Race body-read against cancel/deadline (§1.2/1.5). Whichever `select!` arm
/// is NOT chosen has its future dropped — dropping `read_bounded_body(body)`
/// drops `body` too, closing the (fake or real) connection immediately.
pub async fn fetch_bounded_with_deadline<B, C>(
    body: B,
    cancel: C,
    deadline: Duration,
) -> Result<Vec<u8>, GatewayError>
where
    B: ResponseBody,
    C: Future<Output = ()>,
{
    tokio::select! {
        _ = cancel => Err(GatewayError::Cancelled),
        _ = tokio::time::sleep(deadline) => Err(GatewayError::Timeout),
        result = read_bounded_body(body) => result,
    }
}

/// Safe byte-cap truncation on a UTF-8 char boundary. Never uses `&s[..n]`
/// slice-index syntax (forbidden by `clippy::string_slice`); uses `str::get`.
pub fn safe_truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// URL construction / send-time invariant (STEP 5.B)
// ---------------------------------------------------------------------------

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
                if let (Some(h), Some(l)) = (hex_digits.get(hi..hi + 1), hex_digits.get(lo..lo + 1)) {
                    out.push_str(h);
                    out.push_str(l);
                }
            }
        }
    }
    out
}

fn percent_decode(s: &str) -> Result<String, GatewayError> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = *bytes.get(i).ok_or(GatewayError::UrlViolation)?;
        if b == b'%' {
            let h1 = *bytes.get(i + 1).ok_or(GatewayError::UrlViolation)?;
            let h2 = *bytes.get(i + 2).ok_or(GatewayError::UrlViolation)?;
            let hi = (h1 as char).to_digit(16).ok_or(GatewayError::UrlViolation)?;
            let lo = (h2 as char).to_digit(16).ok_or(GatewayError::UrlViolation)?;
            let byte = u8::try_from(hi * 16 + lo).map_err(|_| GatewayError::UrlViolation)?;
            out.push(byte);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| GatewayError::UrlViolation)
}

/// Build the ONLY permitted outbound URL shape: fixed scheme/host/port/path,
/// with the search query as the sole percent-encoded variable.
pub fn build_request(query: &str) -> String {
    format!(
        "https://{WIKI_HOST}{WIKI_PATH}?action=query&list=search&format=json&srsearch={}",
        percent_encode_query_param(query)
    )
}

/// Send-time assertion (§1.3): re-derive scheme/host/port/path/query-keys from
/// the constructed URL string and reject any deviation. Runs on every request,
/// even ones this module itself built, so a future refactor cannot silently
/// widen the template.
pub fn validate_outbound_url(url: &str, expected_query: &str) -> Result<(), GatewayError> {
    let prefix = format!("https://{WIKI_HOST}{WIKI_PATH}?");
    let rest = url.strip_prefix(&prefix).ok_or(GatewayError::UrlViolation)?;
    if rest.contains('#') || rest.contains('@') {
        return Err(GatewayError::UrlViolation);
    }
    let expected_keys: BTreeSet<&str> = ["action", "list", "format", "srsearch"].into_iter().collect();
    let mut got_keys: BTreeSet<&str> = BTreeSet::new();
    let mut srsearch_val: Option<&str> = None;
    for pair in rest.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().ok_or(GatewayError::UrlViolation)?;
        let v = it.next().ok_or(GatewayError::UrlViolation)?;
        if !got_keys.insert(k) {
            return Err(GatewayError::UrlViolation); // duplicate key
        }
        if k == "srsearch" {
            srsearch_val = Some(v);
        }
    }
    if got_keys != expected_keys {
        return Err(GatewayError::UrlViolation);
    }
    let decoded = percent_decode(srsearch_val.ok_or(GatewayError::UrlViolation)?)?;
    if decoded != expected_query {
        return Err(GatewayError::UrlViolation);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DNS resolve -> deny -> pin (STEP 5.B)
// ---------------------------------------------------------------------------

/// Resolve `host`, then reject (fail-closed) if ANY resolved IP is non-global.
/// Never "use the ones that passed" — a single denied IP aborts the whole set.
pub fn resolve_and_pin<R: HostResolver>(resolver: &R, host: &str) -> Result<Vec<IpAddr>, GatewayError> {
    let ips = resolver.resolve(host).map_err(|_| GatewayError::DnsDenied)?;
    if ips.is_empty() {
        return Err(GatewayError::DnsDenied);
    }
    for ip in &ips {
        if is_disallowed_ip(*ip) {
            return Err(GatewayError::DnsDenied);
        }
    }
    Ok(ips)
}

// ---------------------------------------------------------------------------
// Bounded JSON extraction (STEP 5.D)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub snippet: String,
}

/// Extract title+snippet from a Wikipedia search-API JSON body, bounded to
/// `MAX_RESULTS_PER_QUERY` results and byte-capped fields. Never panics on
/// adversarial JSON (deep nesting / type confusion / huge arrays) — every
/// field access goes through `.get()`/`.and_then()`, never indexing.
pub fn extract_results(body: &[u8]) -> Result<Vec<SearchResult>, GatewayError> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| GatewayError::Malformed)?;
    let arr = value
        .get("query")
        .and_then(|q| q.get("search"))
        .and_then(|s| s.as_array())
        .ok_or(GatewayError::Malformed)?;
    let mut out = Vec::with_capacity(MAX_RESULTS_PER_QUERY);
    for item in arr.iter().take(MAX_RESULTS_PER_QUERY) {
        let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let snippet = item.get("snippet").and_then(|t| t.as_str()).unwrap_or("");
        out.push(SearchResult {
            title: safe_truncate(title, MAX_TITLE_BYTES),
            snippet: safe_truncate(snippet, MAX_SNIPPET_BYTES),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// STEP 5.E: end-to-end research_fetch (FSM ∧ verify-gate ∧ bounded gateway)
// ---------------------------------------------------------------------------

/// Fetch one query end-to-end: build URL -> send-time assert -> resolve/deny/pin
/// -> transport GET -> validate meta -> bounded read w/ cancel+deadline -> extract.
pub async fn fetch_one<T, R, C>(
    transport: &T,
    resolver: &R,
    query: &str,
    cancel: C,
    deadline: Duration,
) -> Result<Vec<SearchResult>, GatewayError>
where
    T: HttpTransport,
    R: HostResolver,
    C: Future<Output = ()>,
{
    let url = build_request(query);
    validate_outbound_url(&url, query)?;
    let _pinned = resolve_and_pin(resolver, WIKI_HOST)?;
    let (meta, body) = transport.get(&url).await?;
    validate_response_meta(&meta)?;
    let raw = fetch_bounded_with_deadline(body, cancel, deadline).await?;
    extract_results(&raw)
}

/// Bundled STEP3 verify inputs (kept out of `research_fetch`'s own parameter
/// list to stay under clippy's arg-count limit).
pub struct VerifyInputs<'a> {
    pub dict_terms: &'a [String],
    pub k_spawn: &'a str,
}

/// Full E0b egress pipeline (§1.1 blueprint flow): `begin` (FSM) ->
/// `verify_and_gate` (STEP3 AND-gate) -> `transition_to_fetching` -> bounded
/// fetch per query -> `transition_to_ready`. ANY verification/FSM failure
/// aborts BEFORE the transport is ever touched (egress == 0 on failure paths).
///
/// `cancel_factory`/`deadline` apply per-query (a fresh cancel future is
/// requested per query since futures are not `Clone`).
pub async fn research_fetch<T, R, CF, C>(
    payload: AttestedIntentPayload,
    verify: VerifyInputs<'_>,
    slot: &std::sync::Arc<ResearchSlot>,
    transport: &T,
    resolver: &R,
    mut cancel_factory: CF,
    deadline: Duration,
) -> Result<(Txn<ReadyToIntegrate>, Vec<Vec<SearchResult>>), GatewayError>
where
    T: HttpTransport,
    R: HostResolver,
    CF: FnMut() -> C,
    C: Future<Output = ()>,
{
    let txn = slot.begin(payload)?;
    verify_and_gate(txn.payload(), verify.dict_terms, verify.k_spawn)?;
    let mut fetching = txn.transition_to_fetching();

    let queries = fetching.payload().queries.clone();
    let mut all_results = Vec::with_capacity(queries.len());
    for q in &queries {
        match fetch_one(transport, resolver, q, cancel_factory(), deadline).await {
            Ok(r) => all_results.push(r),
            Err(e) => {
                let _ = fetching.abort(AbortReason::Explicit);
                return Err(e);
            }
        }
    }
    let _ = &mut fetching; // results already collected above
    let ready = fetching.transition_to_ready();
    Ok((ready, all_results))
}

// ---------------------------------------------------------------------------
// egress-live: the ONLY code path in this crate that requires a TLS provider.
// ---------------------------------------------------------------------------
#[cfg(feature = "egress-live")]
mod live {
    //! Real `reqwest`-backed [`super::HttpTransport`]. Compiled only when the
    //! `egress-live` Cargo feature is enabled — the default build/test suite
    //! never touches this module, so it never requires aws-lc-rs/ring/clang-cl.
    use super::{GatewayError, HttpTransport, ResponseBody, ResponseMeta};

    pub struct ReqwestTransport {
        client: reqwest::Client,
    }

    impl ReqwestTransport {
        pub fn new() -> Result<Self, GatewayError> {
            let client = reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|_| GatewayError::WireViolation)?;
            Ok(Self { client })
        }
    }

    pub struct ReqwestBody {
        stream: reqwest::Response,
    }

    impl ResponseBody for ReqwestBody {
        async fn next_chunk(&mut self) -> Option<Result<Vec<u8>, GatewayError>> {
            match self.stream.chunk().await {
                Ok(Some(bytes)) => Some(Ok(bytes.to_vec())),
                Ok(None) => None,
                Err(_) => Some(Err(GatewayError::WireViolation)),
            }
        }
    }

    impl HttpTransport for ReqwestTransport {
        type Body = ReqwestBody;

        async fn get(&self, url: &str) -> Result<(ResponseMeta, Self::Body), GatewayError> {
            let resp = self
                .client
                .get(url)
                .header("Accept-Encoding", "identity")
                .send()
                .await
                .map_err(|_| GatewayError::WireViolation)?;
            let status = resp.status().as_u16();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let content_encoding = resp
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let meta = ResponseMeta {
                status,
                content_type,
                content_encoding,
            };
            Ok((meta, ReqwestBody { stream: resp }))
        }
    }
}

#[cfg(feature = "egress-live")]
pub use live::ReqwestTransport;
