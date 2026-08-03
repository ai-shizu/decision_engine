//! Vault persistence for M16 Digital Twin / Oracle runs.

use rusqlite::{params, Connection};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct TwinRunRow {
    pub id: String,
    pub created_at: i64,
    pub schema_version: String,
    pub gate_passed: i64,
    /// Stored in the legacy SQL column named `bss`; value is observer coverage.
    pub coverage_score: f64,
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct OracleRunRow {
    pub id: String,
    pub created_at: i64,
    pub schema_version: String,
    pub gate_passed: i64,
    pub payload_json: String,
    pub provenance_json: String,
}

pub(crate) fn insert_twin_run(
    connection: &Connection,
    row: &TwinRunRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO twin_scenario_runs(\
                id, created_at, schema_version, gate_passed, bss, payload_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.id,
                row.created_at,
                row.schema_version,
                row.gate_passed,
                row.coverage_score,
                row.payload_json,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

/// Oldest-first payloads for RLS warmup (cap keeps Jetsam-friendly).
pub(crate) fn list_twin_run_payloads(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<String>, RepositoryError> {
    let limit = limit.clamp(1, 500);
    let mut stmt = connection
        .prepare(
            "SELECT payload_json FROM twin_scenario_runs \
             ORDER BY created_at ASC, id ASC LIMIT ?1",
        )
        .map_err(map_storage_error)?;
    let rows = stmt
        .query_map(params![limit], |row| row.get::<_, String>(0))
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}

pub(crate) fn latest_twin_run_payload(
    connection: &Connection,
) -> Result<Option<String>, RepositoryError> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT payload_json FROM twin_scenario_runs \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_storage_error)
}

pub(crate) fn insert_oracle_run(
    connection: &Connection,
    row: &OracleRunRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO oracle_payload_runs(\
                id, created_at, schema_version, gate_passed, payload_json, provenance_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.id,
                row.created_at,
                row.schema_version,
                row.gate_passed,
                row.payload_json,
                row.provenance_json,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn latest_oracle_run(
    connection: &Connection,
) -> Result<Option<OracleRunRow>, RepositoryError> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT id, created_at, schema_version, gate_passed, payload_json, provenance_json \
             FROM oracle_payload_runs ORDER BY created_at DESC, id DESC LIMIT 1",
            [],
            |row| {
                Ok(OracleRunRow {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    schema_version: row.get(2)?,
                    gate_passed: row.get(3)?,
                    payload_json: row.get(4)?,
                    provenance_json: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(map_storage_error)
}

#[derive(Debug, Clone)]
pub(crate) struct InterviewSessionRow {
    pub id: String,
    pub updated_at: i64,
    pub stage: String,
    pub status: String,
    pub payload_json: String,
    /// Phase 14.4 immutable artifact JSON (may be empty for pre-v10 rows).
    pub artifact_json: String,
    /// Phase 14.4 SHA-256 fingerprint (may be empty for pre-v10 rows).
    pub artifact_fingerprint: String,
}

pub(crate) fn put_interview_session(
    connection: &Connection,
    row: &InterviewSessionRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO interview_sessions(\
                id, updated_at, stage, status, payload_json, \
                artifact_json, artifact_fingerprint\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                updated_at=excluded.updated_at,
                stage=excluded.stage,
                status=excluded.status,
                payload_json=excluded.payload_json,
                artifact_json=excluded.artifact_json,
                artifact_fingerprint=excluded.artifact_fingerprint",
            params![
                row.id,
                row.updated_at,
                row.stage,
                row.status,
                row.payload_json,
                row.artifact_json,
                row.artifact_fingerprint,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn get_interview_session(
    connection: &Connection,
    id: &str,
) -> Result<Option<InterviewSessionRow>, RepositoryError> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT id, updated_at, stage, status, payload_json, \
                    artifact_json, artifact_fingerprint \
             FROM interview_sessions WHERE id = ?1",
            params![id],
            |row| {
                Ok(InterviewSessionRow {
                    id: row.get(0)?,
                    updated_at: row.get(1)?,
                    stage: row.get(2)?,
                    status: row.get(3)?,
                    payload_json: row.get(4)?,
                    artifact_json: row.get(5)?,
                    artifact_fingerprint: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(map_storage_error)
}
