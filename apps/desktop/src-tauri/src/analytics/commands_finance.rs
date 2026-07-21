//! Phase 11 — cognitive-snapshot purchase insert (hierarchical 家計簿).
//!
//! On insert we bake:
//! - `r_at_decision` from latest Digital Twin `R(t)`
//! - `active_distortions_json` from recent CBT `distortion_tags`
//! - `verified` from deterministic checksum `Σ amount + tax == total`
//!
//! Fail-closed: checksum mismatch → `verified = 0` (no RNG retry).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::analytics::digital_twin::TwinScenarioResult;
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
        vault
            .purchase_insert(purchase, lines)
            .map_err(map_vault)?;
        Ok(RecordPurchaseResult {
            id,
            verified,
            line_count,
            r_at_decision,
            active_distortion_count,
        })
    })
    .await
    .map_err(|_| "record_purchase_with_snapshot join failed".to_string())?
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
}
