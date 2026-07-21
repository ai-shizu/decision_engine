//! Vault-side knowledge_chunks repository (M10).
//!
//! All SQL runs on the vault worker thread against the unlocked SQLCipher
//! connection with sqlite-vec already activated.

use rusqlite::{params, Connection, Transaction};

use super::migrations::KNOWLEDGE_EMBEDDING_DIMS;
use super::repository::{map_storage_error, RepositoryError};

/// One row ready to insert into `knowledge_chunks`.
#[derive(Debug, Clone)]
pub(crate) struct KnowledgeChunkRow {
    pub id: String,
    pub text_content: String,
    pub embedding: Vec<f32>,
    pub created_at: i64,
}

/// One KNN hit returned to IPC / hybrid recall.
#[derive(Debug, Clone)]
pub(crate) struct KnowledgeSearchHit {
    pub id: String,
    pub text_content: String,
    pub distance: f64,
    /// Unix UTC seconds when the chunk was ingested.
    pub created_at: i64,
    /// Associative recall score after RRF × Ebbinghaus (0 until fused).
    pub recall_score: f64,
}

fn embedding_blob(embedding: &[f32]) -> Result<Vec<u8>, RepositoryError> {
    if embedding.len() != KNOWLEDGE_EMBEDDING_DIMS {
        return Err(RepositoryError::StorageFailed);
    }
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

/// Replace every chunk whose id is prefixed by `source_id::`, then insert `rows`
/// inside the caller's transaction.
pub(crate) fn replace_source_chunks(
    transaction: &Transaction<'_>,
    source_id: &str,
    rows: &[KnowledgeChunkRow],
) -> Result<usize, RepositoryError> {
    let pattern = format!("{source_id}::%");
    transaction
        .execute(
            "DELETE FROM knowledge_chunks WHERE id LIKE ?1",
            params![pattern],
        )
        .map_err(map_storage_error)?;

    let mut inserted = 0usize;
    let mut statement = transaction
        .prepare(
            "INSERT INTO knowledge_chunks(embedding, id, created_at, text_content) \
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(map_storage_error)?;

    for row in rows {
        let blob = embedding_blob(&row.embedding)?;
        statement
            .execute(params![blob, row.id, row.created_at, row.text_content])
            .map_err(map_storage_error)?;
        inserted += 1;
    }
    Ok(inserted)
}

/// KNN search via sqlite-vec `MATCH` + `k = ?`.
pub(crate) fn search_chunks(
    connection: &Connection,
    query_embedding: &[f32],
    limit: u32,
) -> Result<Vec<KnowledgeSearchHit>, RepositoryError> {
    let blob = embedding_blob(query_embedding)?;
    let k = i64::from(limit.max(1));
    let mut statement = connection
        .prepare(
            "SELECT id, text_content, distance, created_at \
             FROM knowledge_chunks \
             WHERE embedding MATCH ?1 AND k = ?2",
        )
        .map_err(map_storage_error)?;

    let rows = statement
        .query_map(params![blob, k], |row| {
            Ok(KnowledgeSearchHit {
                id: row.get(0)?,
                text_content: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                distance: row.get(2)?,
                created_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                recall_score: 0.0,
            })
        })
        .map_err(map_storage_error)?;

    let mut hits = Vec::new();
    for row in rows {
        hits.push(row.map_err(map_storage_error)?);
    }
    Ok(hits)
}
