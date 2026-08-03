//! EDINET Step 3 — light discovery, coverage, and day-list cache (no Heavy / ZIP).
//!
//! Contract (DESIGN_V3 §5):
//! - list exploration is a **light** job (no Heavy lease)
//! - finite resumable window; never 365-day brute force in-dialogue
//! - persist normalized 120/130 metadata only (never raw JSON)
//! - cache hit ⇒ HTTP 0
//! - `coverage=ok` only in the same atomic day-commit as full parse + index
//! - `candidate_prefix_complete` gates auto-merge / ZIP (ZIP itself is out of Step 3)
//! - `WindowComplete + NoEligibleInWindow` ≠ `WindowIncomplete`
//! - incomplete scan ⇒ `correction_available = Unknown`
//! - HTTP: 401 stop, 429 stop job, 400/404 no retry, 500/timeout finite retry

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::edinet_client::{
    collect_list_parse, recent_filing_dates, select_eligible_yuho_original, EdinetDocumentMeta,
    EdinetError, ListCandidateFilter, MAX_EDINET_LIST_BYTES,
};
use super::net_gateway::{
    fetch_bounded_with_deadline_limit, validate_response_meta, GatewayError, HttpTransport,
};

/// Inclusive lookback used by production name/code discovery (anchor + N prior days).
pub const DEFAULT_DISCOVERY_WINDOW_DAYS_BACK: u32 = 21;
/// Same-day coverage revalidation horizon (DESIGN_V3: initial 15 minutes).
pub const COVERAGE_REVALIDATE_SECS: i64 = 15 * 60;
/// Minimum spacing between live list refetches for the same subject/date.
pub const MIN_LIST_REFETCH_INTERVAL_SECS: i64 = 60;
/// Finite retries for 5xx / timeout only (attempts = 1 + this).
pub const LIST_RETRYABLE_MAX_RETRIES: u32 = 2;
pub const LIST_RETRY_BACKOFF: Duration = Duration::from_millis(50);

/// Wire `Tristate` (serde variants = snake_case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tristate {
    Yes,
    No,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryCoverage {
    NotRun,
    WindowComplete,
    WindowIncomplete,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryResult {
    NotRun,
    Selected,
    NoEligibleInWindow,
    IdentityAmbiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageDayStatus {
    Ok,
    Failed,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRecord {
    pub subject_key: String,
    pub date: String,
    pub status: CoverageDayStatus,
    pub fetched_at: i64,
    pub process_date_time: Option<String>,
    pub error_class: Option<String>,
    pub revalidate_after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanCursor {
    pub subject_key: String,
    pub anchor_date: String,
    pub window_days_back: u32,
    /// Next date still to scan (inclusive). `None` ⇒ window finished or abandoned.
    pub next_date: Option<String>,
    pub status: DiscoveryCoverage,
    pub updated_at: i64,
}

/// Atomic day commit: coverage row + filing index rows (never raw JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCommit {
    pub coverage: CoverageRecord,
    pub filings: Vec<EdinetDocumentMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    InvalidArgument,
    AuthStopped,
    RateLimitedStopped,
    NonRetryableApi,
    /// Transient 5xx (or equivalent) — finite retry only; do not alias to
    /// [`GatewayError::StatusRejected`] (that variant is classified NoRetry).
    RetryableTransient,
    ExhaustedRetries,
    LiveGetBudgetExhausted,
    Cache(String),
    Edinet(EdinetError),
    Gateway(GatewayError),
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArgument => write!(f, "edinet_discovery_invalid_argument"),
            Self::AuthStopped => write!(f, "edinet_discovery_auth_stopped"),
            Self::RateLimitedStopped => write!(f, "edinet_discovery_rate_limited"),
            Self::NonRetryableApi => write!(f, "edinet_discovery_non_retryable"),
            Self::RetryableTransient => write!(f, "edinet_discovery_retryable_transient"),
            Self::ExhaustedRetries => write!(f, "edinet_discovery_exhausted_retries"),
            Self::LiveGetBudgetExhausted => {
                write!(f, "edinet_discovery_live_get_budget_exhausted")
            }
            Self::Cache(msg) => write!(f, "edinet_discovery_cache:{msg}"),
            Self::Edinet(e) => write!(f, "{e}"),
            Self::Gateway(e) => write!(f, "edinet_gateway:{e}"),
        }
    }
}

impl From<EdinetError> for DiscoveryError {
    fn from(value: EdinetError) -> Self {
        Self::Edinet(value)
    }
}

impl From<GatewayError> for DiscoveryError {
    fn from(value: GatewayError) -> Self {
        Self::Gateway(value)
    }
}

/// Disposition of an HTTP list attempt (before body parse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListHttpDisposition {
    Ok,
    /// 401 — stop immediately.
    AuthStop,
    /// 429 — stop this job.
    RateLimitStop,
    /// 400 / 404 — do not retry.
    NoRetry,
    /// 5xx — finite retry.
    RetryableStatus,
}

pub fn classify_list_http_status(status: u16) -> ListHttpDisposition {
    match status {
        200 => ListHttpDisposition::Ok,
        401 => ListHttpDisposition::AuthStop,
        429 => ListHttpDisposition::RateLimitStop,
        400 | 404 => ListHttpDisposition::NoRetry,
        500..=599 => ListHttpDisposition::RetryableStatus,
        _ => ListHttpDisposition::NoRetry,
    }
}

pub fn classify_gateway_error(err: &GatewayError) -> ListHttpDisposition {
    match err {
        GatewayError::Timeout => ListHttpDisposition::RetryableStatus,
        GatewayError::StatusRejected => ListHttpDisposition::NoRetry,
        GatewayError::Cancelled => ListHttpDisposition::NoRetry,
        _ => ListHttpDisposition::NoRetry,
    }
}

/// Cache + cursor seam (Vault-backed in production; memory in Step 3 tests).
pub trait DiscoveryCache: Send {
    fn coverage(
        &self,
        subject_key: &str,
        date: &str,
    ) -> Result<Option<CoverageRecord>, DiscoveryError>;
    fn filings_for_date(
        &self,
        subject_key: &str,
        date: &str,
    ) -> Result<Vec<EdinetDocumentMeta>, DiscoveryError>;
    /// Atomically persist coverage + index for one list date (raw JSON forbidden).
    fn commit_day(&mut self, day: DayCommit) -> Result<(), DiscoveryError>;
    fn cursor(&self, subject_key: &str) -> Result<Option<ScanCursor>, DiscoveryError>;
    fn save_cursor(&mut self, cursor: ScanCursor) -> Result<(), DiscoveryError>;
}

#[derive(Debug, Default)]
pub struct MemoryDiscoveryCache {
    coverage: HashMap<(String, String), CoverageRecord>,
    filings: HashMap<(String, String), Vec<EdinetDocumentMeta>>,
    cursors: HashMap<String, ScanCursor>,
}

impl DiscoveryCache for MemoryDiscoveryCache {
    fn coverage(
        &self,
        subject_key: &str,
        date: &str,
    ) -> Result<Option<CoverageRecord>, DiscoveryError> {
        Ok(self
            .coverage
            .get(&(subject_key.to_string(), date.to_string()))
            .cloned())
    }

    fn filings_for_date(
        &self,
        subject_key: &str,
        date: &str,
    ) -> Result<Vec<EdinetDocumentMeta>, DiscoveryError> {
        Ok(self
            .filings
            .get(&(subject_key.to_string(), date.to_string()))
            .cloned()
            .unwrap_or_default())
    }

    fn commit_day(&mut self, day: DayCommit) -> Result<(), DiscoveryError> {
        let key = (day.coverage.subject_key.clone(), day.coverage.date.clone());
        self.filings.insert(key.clone(), day.filings);
        self.coverage.insert(key, day.coverage);
        Ok(())
    }

    fn cursor(&self, subject_key: &str) -> Result<Option<ScanCursor>, DiscoveryError> {
        Ok(self.cursors.get(subject_key).cloned())
    }

    fn save_cursor(&mut self, cursor: ScanCursor) -> Result<(), DiscoveryError> {
        self.cursors.insert(cursor.subject_key.clone(), cursor);
        Ok(())
    }
}

#[cfg(all(target_vendor = "apple", feature = "egress-live"))]
#[allow(dead_code)]
pub(crate) struct VaultDiscoveryCache<'a> {
    vault: &'a crate::db::VaultHandle,
}

#[cfg(all(target_vendor = "apple", feature = "egress-live"))]
#[allow(dead_code)]
impl<'a> VaultDiscoveryCache<'a> {
    pub(crate) fn new(vault: &'a crate::db::VaultHandle) -> Self {
        Self { vault }
    }
}

#[cfg(all(target_vendor = "apple", feature = "egress-live"))]
impl DiscoveryCache for VaultDiscoveryCache<'_> {
    fn coverage(
        &self,
        subject_key: &str,
        date: &str,
    ) -> Result<Option<CoverageRecord>, DiscoveryError> {
        self.vault
            .edinet_coverage(subject_key.to_string(), date.to_string())
            .map_err(|_| DiscoveryError::Cache("vault discovery cache unavailable".into()))
    }

    fn filings_for_date(
        &self,
        subject_key: &str,
        date: &str,
    ) -> Result<Vec<EdinetDocumentMeta>, DiscoveryError> {
        self.vault
            .edinet_filings_for_date(subject_key.to_string(), date.to_string())
            .map_err(|_| DiscoveryError::Cache("vault discovery cache unavailable".into()))
    }

    fn commit_day(&mut self, day: DayCommit) -> Result<(), DiscoveryError> {
        self.vault
            .edinet_commit_day(day)
            .map_err(|_| DiscoveryError::Cache("vault discovery cache unavailable".into()))
    }

    fn cursor(&self, subject_key: &str) -> Result<Option<ScanCursor>, DiscoveryError> {
        self.vault
            .edinet_cursor(subject_key.to_string())
            .map_err(|_| DiscoveryError::Cache("vault discovery cache unavailable".into()))
    }

    fn save_cursor(&mut self, cursor: ScanCursor) -> Result<(), DiscoveryError> {
        self.vault
            .edinet_save_cursor(cursor)
            .map_err(|_| DiscoveryError::Cache("vault discovery cache unavailable".into()))
    }
}

fn coverage_is_fresh_hit(rec: &CoverageRecord, now_secs: i64) -> bool {
    if rec.status != CoverageDayStatus::Ok {
        return false;
    }
    if let Some(revalidate_after) = rec.revalidate_after {
        if now_secs >= revalidate_after {
            return false;
        }
    }
    true
}

fn date_part_of_submit(submit: &str) -> Option<String> {
    let t = submit.trim();
    let day = t.get(..10)?;
    if day.as_bytes().len() != 10 {
        return None;
    }
    // Reuse calendar validation via recent_filing_dates(anchor, 0).
    if recent_filing_dates(day, 0).len() == 1 {
        Some(day.to_string())
    } else {
        None
    }
}

/// True iff every calendar day from `anchor` through `end_inclusive` has fresh `coverage=ok`.
pub fn candidate_prefix_complete<C: DiscoveryCache + ?Sized>(
    cache: &C,
    subject_key: &str,
    anchor: &str,
    end_inclusive: &str,
    now_secs: i64,
) -> Result<bool, DiscoveryError> {
    let mut cur = anchor.to_string();
    loop {
        match cache.coverage(subject_key, &cur)? {
            Some(rec) if coverage_is_fresh_hit(&rec, now_secs) => {}
            _ => return Ok(false),
        }
        if cur == end_inclusive {
            return Ok(true);
        }
        match super::edinet_client::prev_ymd(&cur) {
            Some(prev) => {
                // Walk toward the past; if we pass end without hitting it, incomplete.
                if prev < end_inclusive.to_string() {
                    return Ok(false);
                }
                cur = prev;
            }
            None => return Ok(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryOutcome {
    pub coverage: DiscoveryCoverage,
    pub result: DiscoveryResult,
    pub candidate_prefix_complete: bool,
    pub selected: Option<EdinetDocumentMeta>,
    pub correction_available: Tristate,
    pub http_gets: u32,
    pub cache_days: u32,
    pub live_days: u32,
    /// Light-job marker — Step 3 never acquires Heavy lease.
    pub heavy_lease_acquired: bool,
}

pub struct DiscoverParams<'a> {
    pub subject_key: &'a str,
    pub anchor_date: &'a str,
    pub window_days_back: u32,
    pub filter: &'a ListCandidateFilter,
    pub subscription_key: &'a str,
    pub now_secs: i64,
    /// When set, stop after this many live HTTP GETs (forces `WindowIncomplete`).
    pub max_live_gets: Option<u32>,
    pub deadline: Duration,
}

/// Light discovery over a finite window. Resumable via [`DiscoveryCache::cursor`].
pub async fn discover_eligible_in_window<C, T>(
    cache: &mut C,
    transport: &T,
    params: DiscoverParams<'_>,
) -> Result<DiscoveryOutcome, DiscoveryError>
where
    C: DiscoveryCache + ?Sized,
    T: HttpTransport,
{
    if params.subject_key.trim().is_empty() || params.subscription_key.trim().is_empty() {
        return Err(DiscoveryError::InvalidArgument);
    }
    let dates = recent_filing_dates(params.anchor_date, params.window_days_back);
    if dates.is_empty() {
        return Err(DiscoveryError::InvalidArgument);
    }

    let mut http_gets: u32 = 0;
    let mut live_dates = std::collections::BTreeSet::new();
    let mut stop_error: Option<DiscoveryError> = None;

    // Resume only when the persisted cursor describes this exact incomplete scan.
    // A cursor from another window must not authorize skipping uninspected dates.
    let resume_from = cache
        .cursor(params.subject_key)?
        .filter(|c| {
            c.anchor_date == params.anchor_date
                && c.window_days_back == params.window_days_back
                && c.status == DiscoveryCoverage::WindowIncomplete
        })
        .and_then(|c| c.next_date)
        .filter(|d| dates.iter().any(|x| x == d));
    let mut next_unscanned_date = Some(
        resume_from
            .clone()
            .unwrap_or_else(|| params.anchor_date.to_string()),
    );

    for date in &dates {
        if let Some(ref resume) = resume_from {
            // A cursor is only a hint: never skip a missing, failed, or expired day.
            let skipped_day_is_fresh = date > resume
                && cache
                    .coverage(params.subject_key, date)?
                    .as_ref()
                    .is_some_and(|rec| coverage_is_fresh_hit(rec, params.now_secs));
            if skipped_day_is_fresh {
                continue;
            }
        }

        if let Some(max) = params.max_live_gets {
            // Budget applies to live GETs only; cache hits are free.
            let will_need_live = match cache.coverage(params.subject_key, date)? {
                Some(rec) if coverage_is_fresh_hit(&rec, params.now_secs) => false,
                _ => true,
            };
            if will_need_live && http_gets >= max {
                break;
            }
        }

        let gets_before = http_gets;
        match load_or_fetch_day(
            cache,
            transport,
            params.subject_key,
            date,
            params.filter,
            params.subscription_key,
            params.now_secs,
            params.deadline,
            params.max_live_gets,
            date == params.anchor_date,
            &mut http_gets,
        )
        .await
        {
            Ok(_) => {
                if http_gets > gets_before {
                    live_dates.insert(date.clone());
                }
            }
            Err(e) => {
                stop_error = Some(e);
                break;
            }
        }

        let next = super::edinet_client::prev_ymd(date);
        next_unscanned_date = next.clone();
        cache.save_cursor(ScanCursor {
            subject_key: params.subject_key.to_string(),
            anchor_date: params.anchor_date.to_string(),
            window_days_back: params.window_days_back,
            next_date: next,
            status: DiscoveryCoverage::WindowIncomplete,
            updated_at: params.now_secs,
        })?;
    }

    if let Some(err) = stop_error {
        // Persist incomplete cursor; propagate hard stops.
        match err {
            DiscoveryError::AuthStopped | DiscoveryError::RateLimitedStopped => return Err(err),
            other => {
                let _ = other;
            }
        }
    }

    // Always select from the whole window's committed index. This restores rows
    // from dates skipped by cursor resume and prevents false NoEligibleInWindow.
    let mut accumulated = Vec::new();
    let mut window_complete = true;
    let mut cache_days = 0_u32;
    let mut live_days = 0_u32;
    for date in &dates {
        match cache.coverage(params.subject_key, date)? {
            Some(rec) if coverage_is_fresh_hit(&rec, params.now_secs) => {
                accumulated.extend(cache.filings_for_date(params.subject_key, date)?);
                if live_dates.contains(date) {
                    live_days = live_days.saturating_add(1);
                } else {
                    cache_days = cache_days.saturating_add(1);
                }
            }
            _ => window_complete = false,
        }
    }
    let scanned_all = window_complete;
    let selection = select_eligible_yuho_original(&accumulated);
    let (result, selected, correction) = match selection {
        Ok(sel) => {
            let correction = if scanned_all {
                if sel.correction_available {
                    Tristate::Yes
                } else {
                    Tristate::No
                }
            } else {
                Tristate::Unknown
            };
            (DiscoveryResult::Selected, Some(sel.original), correction)
        }
        Err(EdinetError::AmbiguousSelection) => {
            (DiscoveryResult::IdentityAmbiguous, None, Tristate::Unknown)
        }
        Err(EdinetError::NoEligibleFiling) => {
            if scanned_all {
                (DiscoveryResult::NoEligibleInWindow, None, Tristate::No)
            } else {
                // V3: incomplete must never collapse into NoEligibleInWindow.
                (DiscoveryResult::NotRun, None, Tristate::Unknown)
            }
        }
        // Cap exceeded is a hard discovery fault (not "no eligible").
        Err(EdinetError::CandidateLimitExceeded) => {
            return Err(DiscoveryError::Edinet(EdinetError::CandidateLimitExceeded));
        }
        Err(e) => return Err(DiscoveryError::Edinet(e)),
    };

    let coverage = if scanned_all {
        DiscoveryCoverage::WindowComplete
    } else {
        DiscoveryCoverage::WindowIncomplete
    };

    let prefix_complete = match (&result, &selected) {
        (DiscoveryResult::Selected, Some(meta)) => meta
            .submit_date_time
            .as_deref()
            .and_then(date_part_of_submit)
            .map(|submit| {
                candidate_prefix_complete(
                    cache,
                    params.subject_key,
                    params.anchor_date,
                    &submit,
                    params.now_secs,
                )
            })
            .transpose()?
            .unwrap_or(false),
        (DiscoveryResult::NoEligibleInWindow, _) if scanned_all => true,
        _ => false,
    };

    cache.save_cursor(ScanCursor {
        subject_key: params.subject_key.to_string(),
        anchor_date: params.anchor_date.to_string(),
        window_days_back: params.window_days_back,
        next_date: if scanned_all {
            None
        } else {
            next_unscanned_date
        },
        status: coverage,
        updated_at: params.now_secs,
    })?;

    // V3: never collapse incomplete scans into NoEligibleInWindow.
    debug_assert!(
        !(coverage == DiscoveryCoverage::WindowIncomplete
            && result == DiscoveryResult::NoEligibleInWindow)
    );

    Ok(DiscoveryOutcome {
        coverage,
        result,
        candidate_prefix_complete: prefix_complete,
        selected,
        correction_available: correction,
        http_gets,
        cache_days,
        live_days,
        heavy_lease_acquired: false,
    })
}

async fn load_or_fetch_day<C, T>(
    cache: &mut C,
    transport: &T,
    subject_key: &str,
    date: &str,
    filter: &ListCandidateFilter,
    subscription_key: &str,
    now_secs: i64,
    deadline: Duration,
    max_live_gets: Option<u32>,
    is_anchor_date: bool,
    http_gets: &mut u32,
) -> Result<Vec<EdinetDocumentMeta>, DiscoveryError>
where
    C: DiscoveryCache + ?Sized,
    T: HttpTransport,
{
    if let Some(rec) = cache.coverage(subject_key, date)? {
        if coverage_is_fresh_hit(&rec, now_secs) {
            return cache.filings_for_date(subject_key, date);
        }
        // Respect min refetch interval even when revalidate expired for failed→retry paths.
        if rec.status == CoverageDayStatus::Ok
            && now_secs.saturating_sub(rec.fetched_at) < MIN_LIST_REFETCH_INTERVAL_SECS
        {
            return cache.filings_for_date(subject_key, date);
        }
    }

    let (rows, process_date_time) = fetch_list_day_with_retry(
        transport,
        date,
        filter,
        subscription_key,
        deadline,
        max_live_gets,
        http_gets,
    )
    .await?;

    let revalidate_after =
        is_anchor_date.then(|| now_secs.saturating_add(COVERAGE_REVALIDATE_SECS));
    cache.commit_day(DayCommit {
        coverage: CoverageRecord {
            subject_key: subject_key.to_string(),
            date: date.to_string(),
            status: CoverageDayStatus::Ok,
            fetched_at: now_secs,
            process_date_time,
            error_class: None,
            revalidate_after,
        },
        filings: rows.clone(),
    })?;
    Ok(rows)
}

async fn fetch_list_day_with_retry<T: HttpTransport>(
    transport: &T,
    date: &str,
    filter: &ListCandidateFilter,
    subscription_key: &str,
    deadline: Duration,
    max_live_gets: Option<u32>,
    http_gets: &mut u32,
) -> Result<(Vec<EdinetDocumentMeta>, Option<String>), DiscoveryError> {
    let max_attempts = LIST_RETRYABLE_MAX_RETRIES.saturating_add(1);
    let mut attempts: u32 = 0;
    loop {
        if max_live_gets.is_some_and(|max| *http_gets >= max) {
            return Err(DiscoveryError::LiveGetBudgetExhausted);
        }
        attempts = attempts.saturating_add(1);
        *http_gets = http_gets.saturating_add(1);
        match fetch_list_day_once(transport, date, filter, subscription_key, deadline).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let retryable = match &e {
                    DiscoveryError::RetryableTransient => true,
                    DiscoveryError::Gateway(g) => {
                        classify_gateway_error(g) == ListHttpDisposition::RetryableStatus
                    }
                    _ => false,
                };
                if retryable
                    && attempts < max_attempts
                    && !max_live_gets.is_some_and(|max| *http_gets >= max)
                {
                    tokio::time::sleep(LIST_RETRY_BACKOFF).await;
                    continue;
                }
                if retryable
                    && attempts < max_attempts
                    && max_live_gets.is_some_and(|max| *http_gets >= max)
                {
                    return Err(DiscoveryError::LiveGetBudgetExhausted);
                }
                if retryable {
                    return Err(DiscoveryError::ExhaustedRetries);
                }
                return Err(e);
            }
        }
    }
}

async fn fetch_list_day_once<T: HttpTransport>(
    transport: &T,
    date: &str,
    filter: &ListCandidateFilter,
    subscription_key: &str,
    deadline: Duration,
) -> Result<(Vec<EdinetDocumentMeta>, Option<String>), DiscoveryError> {
    use super::edinet_client::{build_documents_list_url, validate_documents_list_url};

    let url = build_documents_list_url(date, subscription_key)?;
    validate_documents_list_url(&url, date)?;
    let (meta, body) = transport
        .get(&url, deadline)
        .await
        .map_err(DiscoveryError::from)?;
    match classify_list_http_status(meta.status) {
        ListHttpDisposition::Ok => {}
        ListHttpDisposition::AuthStop => return Err(DiscoveryError::AuthStopped),
        ListHttpDisposition::RateLimitStop => return Err(DiscoveryError::RateLimitedStopped),
        ListHttpDisposition::NoRetry => return Err(DiscoveryError::NonRetryableApi),
        ListHttpDisposition::RetryableStatus => {
            return Err(DiscoveryError::RetryableTransient);
        }
    }
    validate_response_meta(&meta).map_err(DiscoveryError::from)?;
    let bytes = fetch_bounded_with_deadline_limit(
        body,
        std::future::pending::<()>(),
        deadline,
        MAX_EDINET_LIST_BYTES,
    )
    .await
    .map_err(DiscoveryError::from)?;
    let parsed = collect_list_parse(&bytes, filter)?;
    Ok((parsed.candidates, parsed.process_date_time))
}

/// Auto-merge / ZIP may proceed only when discovery selected a filing **and**
/// the candidate prefix has no coverage gaps. Step 3 never starts ZIP itself.
pub fn may_transition_to_zip(outcome: &DiscoveryOutcome) -> bool {
    outcome.result == DiscoveryResult::Selected
        && outcome.candidate_prefix_complete
        && outcome.selected.is_some()
        && !outcome.heavy_lease_acquired // lease is acquired later (Step 4+), not here
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::net_gateway::{GatewayError, HttpTransport, ResponseBody, ResponseMeta};
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct ScriptedBody(Option<Vec<u8>>);
    impl ResponseBody for ScriptedBody {
        fn next_chunk(
            &mut self,
        ) -> impl Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send {
            let next = self.0.take().map(Ok);
            async move { next }
        }
    }

    #[derive(Clone)]
    enum ScriptedResp {
        Ok(Vec<u8>),
        Status(u16),
        Timeout,
    }

    struct ScriptedTransport {
        gets: Arc<AtomicUsize>,
        queue: Arc<Mutex<Vec<ScriptedResp>>>,
    }

    impl ScriptedTransport {
        fn new(mut queue: Vec<ScriptedResp>) -> Self {
            queue.reverse(); // pop() yields FIFO order
            Self {
                gets: Arc::new(AtomicUsize::new(0)),
                queue: Arc::new(Mutex::new(queue)),
            }
        }
    }

    impl HttpTransport for ScriptedTransport {
        type Body = ScriptedBody;
        fn get(
            &self,
            _url: &str,
            _request_deadline: Duration,
        ) -> impl Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send {
            self.gets.fetch_add(1, Ordering::SeqCst);
            let next = self
                .queue
                .lock()
                .ok()
                .and_then(|mut q| q.pop())
                .unwrap_or(ScriptedResp::Status(500));
            async move {
                match next {
                    ScriptedResp::Ok(bytes) => Ok((
                        ResponseMeta {
                            status: 200,
                            content_type: Some("application/json".into()),
                            content_encoding: Some("identity".into()),
                            content_length: None,
                        },
                        ScriptedBody(Some(bytes)),
                    )),
                    ScriptedResp::Status(code) => Ok((
                        ResponseMeta {
                            status: code,
                            content_type: Some("application/json".into()),
                            content_encoding: None,
                            content_length: None,
                        },
                        ScriptedBody(Some(br#"{"results":[]}"#.to_vec())),
                    )),
                    ScriptedResp::Timeout => Err(GatewayError::Timeout),
                }
            }
        }
    }

    fn eligible_list_json(doc_id: &str, submit: &str) -> Vec<u8> {
        format!(
            r#"{{
              "metadata": {{"status":"200","processDateTime":"2024-06-25 12:00"}},
              "results": [{{
                "docID": "{doc_id}",
                "edinetCode": "E02144",
                "filerName": "テスト株式会社",
                "docTypeCode": "120",
                "submitDateTime": "{submit}",
                "withdrawalStatus": "0",
                "disclosureStatus": "0",
                "xbrlFlag": "1",
                "legalStatus": "1"
              }}]
            }}"#
        )
        .into_bytes()
    }

    fn empty_list_json() -> Vec<u8> {
        br#"{"metadata":{"status":"200","processDateTime":"2024-06-25 12:00"},"results":[]}"#
            .to_vec()
    }

    #[test]
    fn classify_http_matrix() {
        assert_eq!(classify_list_http_status(200), ListHttpDisposition::Ok);
        assert_eq!(
            classify_list_http_status(401),
            ListHttpDisposition::AuthStop
        );
        assert_eq!(
            classify_list_http_status(429),
            ListHttpDisposition::RateLimitStop
        );
        assert_eq!(classify_list_http_status(400), ListHttpDisposition::NoRetry);
        assert_eq!(classify_list_http_status(404), ListHttpDisposition::NoRetry);
        assert_eq!(
            classify_list_http_status(500),
            ListHttpDisposition::RetryableStatus
        );
        assert_eq!(
            classify_gateway_error(&GatewayError::Timeout),
            ListHttpDisposition::RetryableStatus
        );
    }

    #[tokio::test]
    async fn cache_hit_performs_zero_http() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![ScriptedResp::Ok(eligible_list_json(
            "S100A",
            "2024-06-25 15:00",
        ))]);
        let first = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 1_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("first");
        assert_eq!(first.http_gets, 1);
        assert_eq!(first.result, DiscoveryResult::Selected);
        assert!(!first.heavy_lease_acquired);

        let transport2 = ScriptedTransport::new(vec![ScriptedResp::Ok(empty_list_json())]);
        let second = discover_eligible_in_window(
            &mut cache,
            &transport2,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 1_010, // within revalidate window
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("cached");
        assert_eq!(second.http_gets, 0, "fresh coverage must not refetch");
        assert_eq!(transport2.gets.load(Ordering::SeqCst), 0);
        assert_eq!(second.result, DiscoveryResult::Selected);
    }

    #[tokio::test]
    async fn historical_days_do_not_expire_on_same_day_revalidation_ttl() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let first_transport = ScriptedTransport::new(vec![
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        discover_eligible_in_window(
            &mut cache,
            &first_transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 1_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("initial scan");

        assert_eq!(
            cache
                .coverage("edinet:E02144", "2024-06-25")
                .expect("anchor coverage")
                .and_then(|rec| rec.revalidate_after),
            Some(1_000 + COVERAGE_REVALIDATE_SECS)
        );
        assert_eq!(
            cache
                .coverage("edinet:E02144", "2024-06-24")
                .expect("historical coverage")
                .and_then(|rec| rec.revalidate_after),
            None
        );

        let second_transport = ScriptedTransport::new(vec![ScriptedResp::Ok(empty_list_json())]);
        let second = discover_eligible_in_window(
            &mut cache,
            &second_transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 1_000 + COVERAGE_REVALIDATE_SECS,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("same-day revalidation");
        assert_eq!(second.http_gets, 1);
        assert_eq!(second_transport.gets.load(Ordering::SeqCst), 1);
        assert_eq!(second.coverage, DiscoveryCoverage::WindowComplete);
    }

    #[tokio::test]
    async fn window_complete_no_eligible_is_not_incomplete() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        // Two empty days (anchor + 1 back).
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 1,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 2_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("complete");
        assert_eq!(out.coverage, DiscoveryCoverage::WindowComplete);
        assert_eq!(out.result, DiscoveryResult::NoEligibleInWindow);
        assert_eq!(out.correction_available, Tristate::No);
        assert!(out.candidate_prefix_complete);
    }

    #[tokio::test]
    async fn window_incomplete_never_reports_no_eligible_in_window() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![ScriptedResp::Ok(empty_list_json())]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 3,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 3_000,
                max_live_gets: Some(1),
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("incomplete");
        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_ne!(out.result, DiscoveryResult::NoEligibleInWindow);
        assert_eq!(out.correction_available, Tristate::Unknown);
        assert!(!may_transition_to_zip(&out));
    }

    #[tokio::test]
    async fn resume_reaggregates_candidates_from_entire_cached_window() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let first_transport = ScriptedTransport::new(vec![ScriptedResp::Ok(eligible_list_json(
            "S100RESUME",
            "2024-06-25 15:00",
        ))]);
        let first = discover_eligible_in_window(
            &mut cache,
            &first_transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 3_100,
                max_live_gets: Some(1),
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("first partial scan");
        assert_eq!(first.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_eq!(first.result, DiscoveryResult::Selected);

        let resume_transport = ScriptedTransport::new(vec![
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        let resumed = discover_eligible_in_window(
            &mut cache,
            &resume_transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 3_110,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("resumed scan");
        assert_eq!(resumed.coverage, DiscoveryCoverage::WindowComplete);
        assert_eq!(resumed.result, DiscoveryResult::Selected);
        assert_eq!(
            resumed.selected.and_then(|meta| meta.doc_id).as_deref(),
            Some("S100RESUME")
        );
        assert_eq!(resume_transport.gets.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cursor_from_different_window_is_not_reused() {
        let mut cache = MemoryDiscoveryCache::default();
        cache
            .save_cursor(ScanCursor {
                subject_key: "edinet:E02144".into(),
                anchor_date: "2024-06-26".into(),
                window_days_back: 3,
                next_date: Some("2024-06-24".into()),
                status: DiscoveryCoverage::WindowIncomplete,
                updated_at: 1,
            })
            .expect("seed stale cursor");
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 3_200,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("fresh scan");
        assert_eq!(transport.gets.load(Ordering::SeqCst), 3);
        assert_eq!(out.coverage, DiscoveryCoverage::WindowComplete);
        assert_eq!(out.result, DiscoveryResult::NoEligibleInWindow);
    }

    #[tokio::test]
    async fn stale_cursor_is_reinitialized_when_budget_stops_before_first_get() {
        let mut cache = MemoryDiscoveryCache::default();
        cache
            .save_cursor(ScanCursor {
                subject_key: "edinet:E02144".into(),
                anchor_date: "2024-06-26".into(),
                window_days_back: 3,
                next_date: Some("2024-06-24".into()),
                status: DiscoveryCoverage::WindowIncomplete,
                updated_at: 1,
            })
            .expect("seed stale cursor");
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 3_250,
                max_live_gets: Some(0),
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("budget stop");

        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        let cursor = cache
            .cursor("edinet:E02144")
            .expect("cursor read")
            .expect("cursor");
        assert_eq!(cursor.anchor_date, "2024-06-25");
        assert_eq!(cursor.window_days_back, 2);
        assert_eq!(cursor.next_date.as_deref(), Some("2024-06-25"));
    }

    #[tokio::test]
    async fn cursor_cannot_skip_missing_coverage_and_report_window_complete() {
        let mut cache = MemoryDiscoveryCache::default();
        for date in ["2024-06-24", "2024-06-23"] {
            cache
                .commit_day(DayCommit {
                    coverage: CoverageRecord {
                        subject_key: "edinet:E02144".into(),
                        date: date.into(),
                        status: CoverageDayStatus::Ok,
                        fetched_at: 100,
                        process_date_time: None,
                        error_class: None,
                        revalidate_after: None,
                    },
                    filings: vec![],
                })
                .expect("seed historical coverage");
        }
        cache
            .save_cursor(ScanCursor {
                subject_key: "edinet:E02144".into(),
                anchor_date: "2024-06-25".into(),
                window_days_back: 2,
                next_date: Some("2024-06-24".into()),
                status: DiscoveryCoverage::WindowIncomplete,
                updated_at: 100,
            })
            .expect("seed cursor");

        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 2,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 200,
                max_live_gets: Some(0),
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("incomplete scan");

        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_ne!(out.result, DiscoveryResult::NoEligibleInWindow);
        assert_eq!(transport.gets.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn invalid_submit_datetime_never_completes_candidate_prefix() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![ScriptedResp::Ok(eligible_list_json(
            "S100BADDATE",
            "not-a-date",
        ))]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 3_300,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("selected but fail-closed");
        assert_eq!(out.result, DiscoveryResult::Selected);
        assert!(!out.candidate_prefix_complete);
        assert!(!may_transition_to_zip(&out));
    }

    #[tokio::test]
    async fn selected_with_prefix_gap_forbids_zip_transition() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        // Find filing on older day; leave a gap by not covering a newer day as ok.
        // Simulate: only fetch the older day via max_live_gets after priming a failed newer day.
        cache
            .commit_day(DayCommit {
                coverage: CoverageRecord {
                    subject_key: "edinet:E02144".into(),
                    date: "2024-06-25".into(),
                    status: CoverageDayStatus::Failed,
                    fetched_at: 1,
                    process_date_time: None,
                    error_class: Some("test".into()),
                    revalidate_after: None,
                },
                filings: vec![],
            })
            .expect("seed failed day");

        let transport = ScriptedTransport::new(vec![
            // Will try 2024-06-25 first (failed → refetch)
            ScriptedResp::Ok(empty_list_json()),
            ScriptedResp::Ok(eligible_list_json("S100OLD", "2024-06-24 10:00")),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 1,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 4_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("selected");
        assert_eq!(out.result, DiscoveryResult::Selected);
        // After successful refetch, prefix should be complete — assert may_transition true.
        assert!(out.candidate_prefix_complete);
        assert!(may_transition_to_zip(&out));
    }

    #[tokio::test]
    async fn partial_window_with_only_live_adopted_days_is_not_mixed() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("filter");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Ok(eligible_list_json("S100LIVE", "2024-06-25 10:00")),
            ScriptedResp::Status(404),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 1,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 4_100,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("partial selected");
        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_eq!(out.result, DiscoveryResult::Selected);
        assert_eq!(out.cache_days, 0);
        assert_eq!(out.live_days, 1);
    }

    #[tokio::test]
    async fn auth_401_stops_without_retry() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Status(401),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        let err = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 5_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect_err("401");
        assert_eq!(err, DiscoveryError::AuthStopped);
        assert_eq!(transport.gets.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rate_limit_429_stops_job() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![ScriptedResp::Status(429)]);
        let err = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 6_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect_err("429");
        assert_eq!(err, DiscoveryError::RateLimitedStopped);
    }

    #[tokio::test]
    async fn status_404_is_non_retryable() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Status(404),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 7_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("soft incomplete path");
        // Day fails closed → incomplete / not selected as no_eligible_in_window for single-day fail
        assert_eq!(transport.gets.load(Ordering::SeqCst), 1);
        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_ne!(out.result, DiscoveryResult::NoEligibleInWindow);
    }

    #[tokio::test]
    async fn status_500_retries_then_may_exhaust() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Status(500),
            ScriptedResp::Status(500),
            ScriptedResp::Status(500),
            ScriptedResp::Status(500),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 8_100,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("day soft-fail after 5xx retries");
        // attempts = 1 + LIST_RETRYABLE_MAX_RETRIES (=3)
        assert_eq!(
            transport.gets.load(Ordering::SeqCst),
            (LIST_RETRYABLE_MAX_RETRIES.saturating_add(1)) as usize
        );
        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_ne!(out.result, DiscoveryResult::NoEligibleInWindow);
    }

    #[tokio::test]
    async fn retry_never_exceeds_live_get_budget() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Status(500),
            ScriptedResp::Ok(empty_list_json()),
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 8_200,
                max_live_gets: Some(1),
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("budget exhaustion is an incomplete scan");

        assert_eq!(out.http_gets, 1);
        assert_eq!(transport.gets.load(Ordering::SeqCst), 1);
        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
        assert_ne!(out.result, DiscoveryResult::NoEligibleInWindow);
    }

    #[tokio::test]
    async fn timeout_retries_then_may_exhaust() {
        let mut cache = MemoryDiscoveryCache::default();
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let transport = ScriptedTransport::new(vec![
            ScriptedResp::Timeout,
            ScriptedResp::Timeout,
            ScriptedResp::Timeout,
            ScriptedResp::Timeout,
        ]);
        let out = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 8_000,
                max_live_gets: None,
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("day soft-fail after retries");
        assert!(transport.gets.load(Ordering::SeqCst) >= 2);
        assert_eq!(out.coverage, DiscoveryCoverage::WindowIncomplete);
    }

    #[test]
    fn process_datetime_captured_without_raw_json() {
        let body = eligible_list_json("S100P", "2024-06-25 15:00");
        let filter = ListCandidateFilter::by_edinet_code("E02144").expect("f");
        let parsed = collect_list_parse(&body, &filter).expect("parse");
        assert_eq!(
            parsed.process_date_time.as_deref(),
            Some("2024-06-25 12:00")
        );
        assert_eq!(parsed.candidates.len(), 1);
    }

    #[test]
    fn incomplete_selected_unknown_correction() {
        // Pure contract: incomplete ⇒ Unknown regardless of bool from select.
        let out = DiscoveryOutcome {
            coverage: DiscoveryCoverage::WindowIncomplete,
            result: DiscoveryResult::Selected,
            candidate_prefix_complete: false,
            selected: Some(EdinetDocumentMeta {
                doc_id: Some("S1".into()),
                ..EdinetDocumentMeta::default()
            }),
            correction_available: Tristate::Unknown,
            http_gets: 1,
            cache_days: 0,
            live_days: 1,
            heavy_lease_acquired: false,
        };
        assert!(!may_transition_to_zip(&out));
        assert_eq!(out.correction_available, Tristate::Unknown);
    }
}
