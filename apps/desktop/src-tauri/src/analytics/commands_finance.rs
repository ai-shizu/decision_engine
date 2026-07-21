//! Phase 11–13 — cognitive-snapshot purchases, month view, commitment fires.
//!
//! On insert we bake:
//! - `r_at_decision` from latest Digital Twin `R(t)`
//! - `active_distortions_json` from recent CBT `distortion_tags`
//! - `verified` from deterministic checksum `Σ amount + tax == total`
//!
//! Fail-closed: checksum mismatch → `verified = 0` (no RNG retry).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::analytics::digital_twin::TwinScenarioResult;
use crate::analytics::self_regulation::{
    commitment_row_from_proposal, evaluate_commitment_fires, proposals_from_report,
    to_commitment_view, CognitiveCommitmentsResult, CommitmentFire,
};
use crate::analytics::spend_cognition::{analyze_spend_cognition, ANALYSIS_ROW_CAP};
use crate::db::{PurchaseLineRow, PurchaseRow, VaultErrorCode, VaultHandle};

const MAX_LINES: usize = 128;
const MAX_TEXT: usize = 512;
const RECENT_DISTORTIONS: u32 = 32;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLineIn {
    pub item_name: String,
    pub unit_price: i64,
    pub qty: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptIn {
    pub merchant: String,
    pub occurred_at: String,
    pub tax: i64,
    pub total: i64,
    pub lines: Vec<ReceiptLineIn>,
}

impl ReceiptIn {
    fn normalize(&mut self) {
        self.merchant = self.merchant.trim().chars().take(MAX_TEXT).collect();
        if self.merchant.is_empty() {
            self.merchant = "unknown".into();
        }
        self.occurred_at = self.occurred_at.trim().to_string();
        if self.occurred_at.is_empty() {
            self.occurred_at = "unknown".into();
        }
        if self.tax < 0 {
            self.tax = 0;
        }
        if self.total < 0 {
            self.total = 0;
        }
        for line in &mut self.lines {
            line.item_name = line.item_name.trim().chars().take(MAX_TEXT).collect();
            if line.qty <= 0 {
                line.qty = 1;
            }
            if line.unit_price < 0 {
                line.unit_price = 0;
            }
            if line.amount < 0 {
                line.amount = 0;
            }
        }
    }

    /// Deterministic integer checksum — no floating point, no LLM retry.
    fn checksum_ok(&self) -> bool {
        let mut sum: i64 = 0;
        for line in &self.lines {
            match sum.checked_add(line.amount) {
                Some(v) => sum = v,
                None => return false,
            }
        }
        match sum.checked_add(self.tax) {
            Some(v) => v == self.total,
            None => false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordPurchaseParams {
    pub receipt: ReceiptIn,
    /// Optional override for receipt time (unix seconds). Else parse `occurred_at`.
    pub occurred_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecordPurchaseResult {
    pub id: String,
    pub verified: bool,
    pub line_count: usize,
    pub r_at_decision: f64,
    pub active_distortion_count: usize,
    /// Soft If-Then fires (never a hard block). Empty when no commitment matches.
    pub commitment_fires: Vec<CommitmentFire>,
}

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn normalize_merchant(s: &str) -> String {
    let t = s.trim().to_lowercase();
    if t.is_empty() || t == "unknown" {
        "unknown".into()
    } else {
        t.chars().take(MAX_TEXT).collect()
    }
}

fn parse_occurred_at(raw: &str, fallback: i64) -> i64 {
    let s = raw.trim();
    if s.is_empty() || s == "unknown" {
        return fallback;
    }
    if let Ok(v) = s.parse::<i64>() {
        return v;
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 8 {
        let y: i64 = digits[0..4].parse().unwrap_or(1970);
        let m: i64 = digits[4..6].parse().unwrap_or(1);
        let d: i64 = digits[6..8].parse().unwrap_or(1);
        return approx_days_since_epoch(y, m, d).saturating_mul(86_400);
    }
    fallback
}

fn approx_days_since_epoch(y: i64, m: i64, d: i64) -> i64 {
    // Civil date → days (Howard Hinnant algorithm, public domain).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn load_r_at_decision(vault: &VaultHandle) -> f64 {
    match vault.twin_run_latest_payload() {
        Ok(Some(payload)) => match serde_json::from_str::<TwinScenarioResult>(&payload) {
            Ok(twin) => {
                let r = twin.state.r_now;
                if r.is_finite() {
                    r.clamp(0.0, 1.0)
                } else {
                    0.5
                }
            }
            Err(_) => 0.5,
        },
        _ => 0.5,
    }
}

fn load_active_distortions_json(vault: &VaultHandle) -> (String, usize) {
    let rows = vault
        .distortion_tags_list(RECENT_DISTORTIONS)
        .unwrap_or_default();
    let recent: Vec<String> = rows
        .iter()
        .rev()
        .take(12)
        .map(|r| r.category.clone())
        .collect();
    let count = recent.len();
    let json = serde_json::to_string(&recent).unwrap_or_else(|_| "[]".into());
    (json, count)
}

/// Persist a receipt with cognitive snapshot columns (Twin R(t) + CBT tags).
#[tauri::command]
pub async fn record_purchase_with_snapshot(
    vault: State<'_, VaultHandle>,
    params: RecordPurchaseParams,
) -> Result<RecordPurchaseResult, String> {
    let mut receipt = params.receipt;
    receipt.normalize();
    if receipt.lines.len() > MAX_LINES {
        return Err("too many lines".into());
    }
    for line in &receipt.lines {
        if line.item_name.is_empty() {
            return Err("empty item_name".into());
        }
    }

    // Fail-closed checksum — never RNG-retry a hallucinated total.
    let verified = receipt.checksum_ok();

    let vault = vault.inner().clone();
    let occurred_override = params.occurred_at;
    tauri::async_runtime::spawn_blocking(move || {
        let created = now_unix();
        let occurred_at =
            occurred_override.unwrap_or_else(|| parse_occurred_at(&receipt.occurred_at, created));
        let r_at_decision = load_r_at_decision(&vault);
        let (active_distortions_json, active_distortion_count) =
            load_active_distortions_json(&vault);

        let id = format!("pur-{created}");
        let purchase = PurchaseRow {
            id: id.clone(),
            occurred_at,
            merchant_norm: normalize_merchant(&receipt.merchant),
            total_amount: receipt.total,
            tax: receipt.tax,
            verified: if verified { 1 } else { 0 },
            r_at_decision,
            active_distortions_json,
        };
        let mut lines = Vec::with_capacity(receipt.lines.len());
        for (i, line) in receipt.lines.iter().enumerate() {
            lines.push(PurchaseLineRow {
                id: format!("{id}-l{i}"),
                purchase_id: id.clone(),
                item_name: line.item_name.clone(),
                unit_price: line.unit_price,
                qty: line.qty,
                amount: line.amount,
            });
        }
        let line_count = lines.len();
        let commitments = vault.commitment_list_enabled().unwrap_or_default();
        let commitment_fires = evaluate_commitment_fires(
            &commitments,
            r_at_decision,
            &purchase.active_distortions_json,
            occurred_at,
        );
        vault
            .purchase_insert(purchase, lines)
            .map_err(map_vault)?;
        Ok(RecordPurchaseResult {
            id,
            verified,
            line_count,
            r_at_decision,
            active_distortion_count,
            commitment_fires,
        })
    })
    .await
    .map_err(|_| "record_purchase_with_snapshot join failed".to_string())?
}


#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CognitiveMonthViewParams {
    pub year: i32,
    pub month: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CognitiveDayView {
    pub date: String,
    pub r_value: Option<f64>,
    pub total_expense: i64,
    pub distortions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CognitiveMonthView {
    pub year: i32,
    pub month: u32,
    pub days: Vec<CognitiveDayView>,
}

/// JST (+09:00) civil midnight → unix seconds.
fn jst_midnight_unix(year: i32, month: u32, day: u32) -> i64 {
    let days = approx_days_since_epoch(year as i64, month as i64, day as i64);
    days.saturating_mul(86_400).saturating_sub(9 * 3_600)
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
        _ => 0,
    }
}

/// Half-open `[start, end)` unix range covering the JST calendar month.
fn jst_month_bounds(year: i32, month: u32) -> Result<(i64, i64), String> {
    if !(1..=12).contains(&month) {
        return Err("invalid month".into());
    }
    if year < 1970 || year > 2100 {
        return Err("invalid year".into());
    }
    let start = jst_midnight_unix(year, month, 1);
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let end = jst_midnight_unix(ny, nm, 1);
    Ok((start, end))
}

/// Howard Hinnant civil_from_days (public domain).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn unix_to_jst_date(unix: i64) -> String {
    let jst = unix.saturating_add(9 * 3_600);
    let days = jst.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

struct DayAcc {
    total_expense: i64,
    r_sum: f64,
    r_count: u32,
    distortions: BTreeSet<String>,
}

fn parse_distortion_categories(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<String>>(trimmed) {
        Ok(items) => items
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn aggregate_month_days(
    rows: &[crate::db::PurchaseRow],
) -> BTreeMap<String, DayAcc> {
    let mut map: BTreeMap<String, DayAcc> = BTreeMap::new();
    for row in rows {
        let date = unix_to_jst_date(row.occurred_at);
        let acc = map.entry(date).or_insert_with(|| DayAcc {
            total_expense: 0,
            r_sum: 0.0,
            r_count: 0,
            distortions: BTreeSet::new(),
        });
        acc.total_expense = acc.total_expense.saturating_add(row.total_amount);
        if row.r_at_decision.is_finite() {
            acc.r_sum += row.r_at_decision.clamp(0.0, 1.0);
            acc.r_count = acc.r_count.saturating_add(1);
        }
        for cat in parse_distortion_categories(&row.active_distortions_json) {
            acc.distortions.insert(cat);
        }
    }
    map
}

fn dense_month_days(
    year: i32,
    month: u32,
    sparse: BTreeMap<String, DayAcc>,
) -> Vec<CognitiveDayView> {
    let n = days_in_month(year, month);
    let mut days = Vec::with_capacity(n as usize);
    for day in 1..=n {
        let date = format!("{year:04}-{month:02}-{day:02}");
        match sparse.get(&date) {
            Some(acc) => {
                let r_value = if acc.r_count > 0 {
                    Some((acc.r_sum / f64::from(acc.r_count)).clamp(0.0, 1.0))
                } else {
                    None
                };
                days.push(CognitiveDayView {
                    date,
                    r_value,
                    total_expense: acc.total_expense,
                    distortions: acc.distortions.iter().cloned().collect(),
                });
            }
            None => days.push(CognitiveDayView {
                date,
                r_value: None,
                total_expense: 0,
                distortions: Vec::new(),
            }),
        }
    }
    days
}

/// One-month daily aggregates: Twin R(t), expenses, unique CBT distortions.
#[tauri::command]
pub async fn get_cognitive_month_view(
    vault: State<'_, VaultHandle>,
    params: CognitiveMonthViewParams,
) -> Result<CognitiveMonthView, String> {
    let year = params.year;
    let month = params.month;
    let (start_unix, end_unix) = jst_month_bounds(year, month)?;
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = vault
            .purchase_list_range(start_unix, end_unix)
            .map_err(map_vault)?;
        let sparse = aggregate_month_days(&rows);
        Ok(CognitiveMonthView {
            year,
            month,
            days: dense_month_days(year, month, sparse),
        })
    })
    .await
    .map_err(|_| "get_cognitive_month_view join failed".to_string())?
}


/// FDR-robust spend↔cognition links → If-Then commitment proposals (+ vault sync).
#[tauri::command]
pub async fn get_cognitive_commitments(
    vault: State<'_, VaultHandle>,
) -> Result<CognitiveCommitmentsResult, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = vault
            .purchase_list_recent(ANALYSIS_ROW_CAP as u32)
            .map_err(map_vault)?;
        let analysis = analyze_spend_cognition(&rows);
        let proposals = proposals_from_report(&analysis);
        let created = now_unix();
        for proposal in &proposals {
            let existing = vault
                .commitment_find_by_source(proposal.source_relation_id.clone())
                .unwrap_or(None);
            let row = commitment_row_from_proposal(proposal, created, existing);
            let _ = vault.commitment_upsert(row);
        }
        let commitments = vault
            .commitment_list(256)
            .unwrap_or_default()
            .iter()
            .map(to_commitment_view)
            .collect();
        Ok(CognitiveCommitmentsResult {
            analysis,
            proposals,
            commitments,
        })
    })
    .await
    .map_err(|_| "get_cognitive_commitments join failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_integer_gate() {
        let ok = ReceiptIn {
            merchant: "a".into(),
            occurred_at: "unknown".into(),
            tax: 10,
            total: 110,
            lines: vec![ReceiptLineIn {
                item_name: "x".into(),
                unit_price: 100,
                qty: 1,
                amount: 100,
            }],
        };
        assert!(ok.checksum_ok());
        let bad = ReceiptIn {
            total: 111,
            ..ok
        };
        assert!(!bad.checksum_ok());
    }

    #[test]
    fn jst_month_bounds_july_2026() {
        let (start, end) = jst_month_bounds(2026, 7).expect("bounds");
        // 2026-07-01 00:00 JST == 2026-06-30 15:00 UTC
        assert_eq!(start, jst_midnight_unix(2026, 7, 1));
        assert_eq!(end, jst_midnight_unix(2026, 8, 1));
        assert!(end > start);
        assert_eq!(unix_to_jst_date(start), "2026-07-01");
        assert_eq!(unix_to_jst_date(end - 1), "2026-07-31");
    }

    #[test]
    fn aggregate_unique_distortions_and_mean_r() {
        use crate::db::PurchaseRow;
        let rows = vec![
            PurchaseRow {
                id: "a".into(),
                occurred_at: jst_midnight_unix(2026, 7, 14) + 3600,
                merchant_norm: "x".into(),
                total_amount: 2000,
                tax: 0,
                verified: 1,
                r_at_decision: 0.2,
                active_distortions_json: r#"["labeling","all_or_nothing"]"#.into(),
            },
            PurchaseRow {
                id: "b".into(),
                occurred_at: jst_midnight_unix(2026, 7, 14) + 7200,
                merchant_norm: "y".into(),
                total_amount: 2200,
                tax: 0,
                verified: 1,
                r_at_decision: 0.4,
                active_distortions_json: r#"["labeling","should_statements"]"#.into(),
            },
        ];
        let sparse = aggregate_month_days(&rows);
        let day = sparse.get("2026-07-14").expect("day");
        assert_eq!(day.total_expense, 4200);
        assert_eq!(day.r_count, 2);
        assert!((day.r_sum / 2.0 - 0.3).abs() < 1e-9);
        let cats: Vec<_> = day.distortions.iter().cloned().collect();
        assert_eq!(
            cats,
            vec![
                "all_or_nothing".to_string(),
                "labeling".to_string(),
                "should_statements".to_string()
            ]
        );
        let dense = dense_month_days(2026, 7, sparse);
        assert_eq!(dense.len(), 31);
        let cell = dense.iter().find(|d| d.date == "2026-07-14").unwrap();
        assert_eq!(cell.total_expense, 4200);
        assert!((cell.r_value.unwrap() - 0.3).abs() < 1e-9);
        let empty = dense.iter().find(|d| d.date == "2026-07-15").unwrap();
        assert_eq!(empty.total_expense, 0);
        assert!(empty.r_value.is_none());
        assert!(empty.distortions.is_empty());
    }
}
