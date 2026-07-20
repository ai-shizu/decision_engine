//! Vault persistence for M16 Digital Twin / Oracle runs.

use rusqlite::{params, Connection};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct TwinRunRow {
    pub id: String,
    pub created_at: i64,
    pub schema_version: String,
    pub gate_passed: i64,
    pub bss: f64,
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
                row.bss,
                row.payload_json,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
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
