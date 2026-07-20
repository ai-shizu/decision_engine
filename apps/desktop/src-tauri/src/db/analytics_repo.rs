//! Vault persistence for gap_analysis runs + tensor profiles (M14).

use rusqlite::{params, Connection};

use super::repository::{map_storage_error, RepositoryError};

#[derive(Debug, Clone)]
pub(crate) struct GapAnalysisRow {
    pub id: String,
    pub created_at: i64,
    pub schema_version: String,
    pub data_sufficiency: f64,
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TensorProfileRow {
    pub id: String,
    pub created_at: i64,
    pub schema_version: String,
    pub model_hash: String,
    pub payload_json: String,
}

pub(crate) fn insert_gap_analysis(
    connection: &Connection,
    row: &GapAnalysisRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO gap_analysis_runs(\
                id, created_at, schema_version, data_sufficiency, payload_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                row.id,
                row.created_at,
                row.schema_version,
                row.data_sufficiency,
                row.payload_json,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn latest_gap_analysis(
    connection: &Connection,
) -> Result<Option<GapAnalysisRow>, RepositoryError> {
    let mut stmt = connection
        .prepare(
            "SELECT id, created_at, schema_version, data_sufficiency, payload_json \
             FROM gap_analysis_runs ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .map_err(map_storage_error)?;
    let mut rows = stmt.query([]).map_err(map_storage_error)?;
    match rows.next().map_err(map_storage_error)? {
        Some(row) => Ok(Some(GapAnalysisRow {
            id: row.get(0).map_err(map_storage_error)?,
            created_at: row.get(1).map_err(map_storage_error)?,
            schema_version: row.get(2).map_err(map_storage_error)?,
            data_sufficiency: row.get(3).map_err(map_storage_error)?,
            payload_json: row.get(4).map_err(map_storage_error)?,
        })),
        None => Ok(None),
    }
}

pub(crate) fn insert_tensor_profile(
    connection: &Connection,
    row: &TensorProfileRow,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO tensor_profiles(\
                id, created_at, schema_version, model_hash, payload_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                row.id,
                row.created_at,
                row.schema_version,
                row.model_hash,
                row.payload_json,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn latest_tensor_profile(
    connection: &Connection,
) -> Result<Option<TensorProfileRow>, RepositoryError> {
    let mut stmt = connection
        .prepare(
            "SELECT id, created_at, schema_version, model_hash, payload_json \
             FROM tensor_profiles ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .map_err(map_storage_error)?;
    let mut rows = stmt.query([]).map_err(map_storage_error)?;
    match rows.next().map_err(map_storage_error)? {
        Some(row) => Ok(Some(TensorProfileRow {
            id: row.get(0).map_err(map_storage_error)?,
            created_at: row.get(1).map_err(map_storage_error)?,
            schema_version: row.get(2).map_err(map_storage_error)?,
            model_hash: row.get(3).map_err(map_storage_error)?,
            payload_json: row.get(4).map_err(map_storage_error)?,
        })),
        None => Ok(None),
    }
}
