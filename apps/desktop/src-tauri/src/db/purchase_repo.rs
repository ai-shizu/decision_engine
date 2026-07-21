//! Vault `purchases` / `purchase_lines` repository (Phase 11).
//!
//! Cognitive snapshot columns (`r_at_decision`, `active_distortions_json`) are
//! baked at insert time — immutable fossils of decision-time mind state.

use rusqlite::{params, Connection};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct PurchaseRow {
    pub id: String,
    pub occurred_at: i64,
    pub merchant_norm: String,
    pub total_amount: i64,
    pub tax: i64,
    pub verified: i64,
    pub r_at_decision: f64,
    pub active_distortions_json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PurchaseLineRow {
    pub id: String,
    pub purchase_id: String,
    pub item_name: String,
    pub unit_price: i64,
    pub qty: i64,
    pub amount: i64,
}

/// Insert parent + lines. Caller must already have opened a writeable connection.
pub(crate) fn insert_purchase_with_lines(
    connection: &Connection,
    purchase: &PurchaseRow,
    lines: &[PurchaseLineRow],
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO purchases(\
                id, occurred_at, merchant_norm, total_amount, tax, verified, \
                r_at_decision, active_distortions_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                purchase.id,
                purchase.occurred_at,
                purchase.merchant_norm,
                purchase.total_amount,
                purchase.tax,
                purchase.verified,
                purchase.r_at_decision,
                purchase.active_distortions_json,
            ],
        )
        .map_err(map_storage_error)?;

    let mut statement = connection
        .prepare(
            "INSERT INTO purchase_lines(\
                id, purchase_id, item_name, unit_price, qty, amount\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(map_storage_error)?;
    for line in lines {
        statement
            .execute(params![
                line.id,
                line.purchase_id,
                line.item_name,
                line.unit_price,
                line.qty,
                line.amount,
            ])
            .map_err(map_storage_error)?;
    }
    Ok(())
}

const MONTH_VIEW_ROW_CAP: usize = 10_000;

/// List purchase snapshot rows in `[start_unix, end_unix)` for month aggregation.
pub(crate) fn list_purchases_in_range(
    connection: &Connection,
    start_unix: i64,
    end_unix: i64,
) -> Result<Vec<PurchaseRow>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT id, occurred_at, merchant_norm, total_amount, tax, verified, \
                    r_at_decision, active_distortions_json \
             FROM purchases \
             WHERE occurred_at >= ?1 AND occurred_at < ?2 \
             ORDER BY occurred_at ASC \
             LIMIT ?3",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map(
            params![start_unix, end_unix, MONTH_VIEW_ROW_CAP as i64],
            |row| {
                Ok(PurchaseRow {
                    id: row.get(0)?,
                    occurred_at: row.get(1)?,
                    merchant_norm: row.get(2)?,
                    total_amount: row.get(3)?,
                    tax: row.get(4)?,
                    verified: row.get(5)?,
                    r_at_decision: row.get(6)?,
                    active_distortions_json: row.get(7)?,
                })
            },
        )
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}


/// Newest-first purchase snapshots for cognition × spend analysis.
pub(crate) fn list_purchases_recent(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<PurchaseRow>, RepositoryError> {
    let lim = limit.clamp(1, 5_000) as i64;
    let mut statement = connection
        .prepare(
            "SELECT id, occurred_at, merchant_norm, total_amount, tax, verified, \
                    r_at_decision, active_distortions_json \
             FROM purchases \
             ORDER BY occurred_at DESC, id DESC \
             LIMIT ?1",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map(params![lim], |row| {
            Ok(PurchaseRow {
                id: row.get(0)?,
                occurred_at: row.get(1)?,
                merchant_norm: row.get(2)?,
                total_amount: row.get(3)?,
                tax: row.get(4)?,
                verified: row.get(5)?,
                r_at_decision: row.get(6)?,
                active_distortions_json: row.get(7)?,
            })
        })
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}
