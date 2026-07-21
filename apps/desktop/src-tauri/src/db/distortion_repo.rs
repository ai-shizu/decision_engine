//! Vault-side `distortion_tags` repository (Phase 8 / CBT fingerprint).

use rusqlite::{params, Connection};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct DistortionTagRow {
    pub id: String,
    pub created_at: i64,
    pub category: String,
    pub snippet: String,
    pub confidence_score: f64,
    pub source_kind: String,
    pub source_id: String,
    pub run_id: String,
}

pub(crate) fn insert_distortion_tags(
    connection: &Connection,
    rows: &[DistortionTagRow],
) -> Result<usize, RepositoryError> {
    let mut inserted = 0usize;
    let mut statement = connection
        .prepare(
            "INSERT INTO distortion_tags(\
                id, created_at, category, snippet, confidence_score, \
                source_kind, source_id, run_id\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .map_err(map_storage_error)?;
    for row in rows {
        statement
            .execute(params![
                row.id,
                row.created_at,
                row.category,
                row.snippet,
                row.confidence_score,
                row.source_kind,
                row.source_id,
                row.run_id,
            ])
            .map_err(map_storage_error)?;
        inserted += 1;
    }
    Ok(inserted)
}

pub(crate) fn list_distortion_tags(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<DistortionTagRow>, RepositoryError> {
    let lim = i64::from(limit.max(1));
    let mut statement = connection
        .prepare(
            "SELECT id, created_at, category, snippet, confidence_score, \
                    source_kind, source_id, run_id \
             FROM distortion_tags \
             ORDER BY created_at ASC, id ASC LIMIT ?1",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map(params![lim], |row| {
            Ok(DistortionTagRow {
                id: row.get(0)?,
                created_at: row.get(1)?,
                category: row.get(2)?,
                snippet: row.get(3)?,
                confidence_score: row.get(4)?,
                source_kind: row.get(5)?,
                source_id: row.get(6)?,
                run_id: row.get(7)?,
            })
        })
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}
