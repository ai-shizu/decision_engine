//! Vault `commitments` repository (Phase 13 — If-Then self-regulation).

use rusqlite::{params, Connection, OptionalExtension};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct CommitmentRow {
    pub id: String,
    pub created_at: i64,
    pub condition_json: String,
    pub action_type: String,
    pub custom_prompt: String,
    pub delay_seconds: i64,
    pub source_relation_id: String,
    pub enabled: i64,
    pub origin: String,
}

pub(crate) fn upsert_commitment(
    connection: &Connection,
    row: &CommitmentRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO commitments(\
                id, created_at, condition_json, action_type, custom_prompt, \
                delay_seconds, source_relation_id, enabled, origin\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(source_relation_id) DO UPDATE SET
                condition_json = excluded.condition_json,
                action_type = excluded.action_type,
                custom_prompt = excluded.custom_prompt,
                delay_seconds = excluded.delay_seconds",
            params![
                row.id,
                row.created_at,
                row.condition_json,
                row.action_type,
                row.custom_prompt,
                row.delay_seconds,
                row.source_relation_id,
                row.enabled,
                row.origin,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn list_commitments(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<CommitmentRow>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT id, created_at, condition_json, action_type, custom_prompt, \
                    delay_seconds, source_relation_id, enabled, origin \
             FROM commitments \
             ORDER BY enabled DESC, created_at DESC \
             LIMIT ?1",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map(params![limit as i64], |row| {
            Ok(CommitmentRow {
                id: row.get(0)?,
                created_at: row.get(1)?,
                condition_json: row.get(2)?,
                action_type: row.get(3)?,
                custom_prompt: row.get(4)?,
                delay_seconds: row.get(5)?,
                source_relation_id: row.get(6)?,
                enabled: row.get(7)?,
                origin: row.get(8)?,
            })
        })
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}

pub(crate) fn list_enabled_commitments(
    connection: &Connection,
) -> Result<Vec<CommitmentRow>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT id, created_at, condition_json, action_type, custom_prompt, \
                    delay_seconds, source_relation_id, enabled, origin \
             FROM commitments \
             WHERE enabled = 1 \
             ORDER BY created_at DESC \
             LIMIT 256",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(CommitmentRow {
                id: row.get(0)?,
                created_at: row.get(1)?,
                condition_json: row.get(2)?,
                action_type: row.get(3)?,
                custom_prompt: row.get(4)?,
                delay_seconds: row.get(5)?,
                source_relation_id: row.get(6)?,
                enabled: row.get(7)?,
                origin: row.get(8)?,
            })
        })
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}

/// Resolve existing id for a source_relation_id (idempotent proposal upsert).
pub(crate) fn find_id_by_source_relation(
    connection: &Connection,
    source_relation_id: &str,
) -> Result<Option<String>, RepositoryError> {
    connection
        .query_row(
            "SELECT id FROM commitments WHERE source_relation_id = ?1 LIMIT 1",
            params![source_relation_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_storage_error)
}
