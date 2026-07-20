//! Vault persistence for M15 psychometrics (pulse / rasch / probe store).

use rusqlite::{params, Connection, OptionalExtension};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct PulseRunRow {
    pub id: String,
    pub created_at: i64,
    pub affinity_score: Option<i64>,
    pub interaction_tendency: String,
    pub next_best_action: String,
    pub metrics_json: String,
    pub input_hash: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RaschRunRow {
    pub id: String,
    pub created_at: i64,
    pub artifact_sha256: String,
    pub posterior_json: String,
    pub excluded_json: String,
    pub last_selection_json: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProbeStoreRow {
    pub id: String,
    pub updated_at: i64,
    pub payload_json: String,
}

pub(crate) fn insert_pulse_run(
    connection: &Connection,
    row: &PulseRunRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO interaction_pulse_runs(\
                id, created_at, affinity_score, interaction_tendency, \
                next_best_action, metrics_json, input_hash\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.id,
                row.created_at,
                row.affinity_score,
                row.interaction_tendency,
                row.next_best_action,
                row.metrics_json,
                row.input_hash,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn upsert_rasch_run(
    connection: &Connection,
    row: &RaschRunRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO rasch_filter_runs(\
                id, created_at, artifact_sha256, posterior_json, \
                excluded_json, last_selection_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                created_at=excluded.created_at,
                artifact_sha256=excluded.artifact_sha256,
                posterior_json=excluded.posterior_json,
                excluded_json=excluded.excluded_json,
                last_selection_json=excluded.last_selection_json",
            params![
                row.id,
                row.created_at,
                row.artifact_sha256,
                row.posterior_json,
                row.excluded_json,
                row.last_selection_json,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn latest_rasch_run(
    connection: &Connection,
) -> Result<Option<RaschRunRow>, RepositoryError> {
    let mut stmt = connection
        .prepare(
            "SELECT id, created_at, artifact_sha256, posterior_json, \
                    excluded_json, last_selection_json \
             FROM rasch_filter_runs ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .map_err(map_storage_error)?;
    let mut rows = stmt.query([]).map_err(map_storage_error)?;
    match rows.next().map_err(map_storage_error)? {
        Some(row) => Ok(Some(RaschRunRow {
            id: row.get(0).map_err(map_storage_error)?,
            created_at: row.get(1).map_err(map_storage_error)?,
            artifact_sha256: row.get(2).map_err(map_storage_error)?,
            posterior_json: row.get(3).map_err(map_storage_error)?,
            excluded_json: row.get(4).map_err(map_storage_error)?,
            last_selection_json: row.get(5).map_err(map_storage_error)?,
        })),
        None => Ok(None),
    }
}

pub(crate) fn get_probe_store(
    connection: &Connection,
) -> Result<Option<ProbeStoreRow>, RepositoryError> {
    connection
        .query_row(
            "SELECT id, updated_at, payload_json FROM probe_store WHERE id = 'default'",
            [],
            |row| {
                Ok(ProbeStoreRow {
                    id: row.get(0)?,
                    updated_at: row.get(1)?,
                    payload_json: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(map_storage_error)
}

pub(crate) fn put_probe_store(
    connection: &Connection,
    row: &ProbeStoreRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO probe_store(id, updated_at, payload_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                updated_at=excluded.updated_at,
                payload_json=excluded.payload_json",
            params![row.id, row.updated_at, row.payload_json],
        )
        .map_err(map_storage_error)?;
    Ok(())
}
