//! Vault-side knowledge_chunks repository (M10).
//!
//! All SQL runs on the vault worker thread against the unlocked SQLCipher
//! connection with sqlite-vec already activated.

use rusqlite::{params, Connection, Transaction};

use super::knowledge_namespace::{self, KnowledgeNamespace};
use super::migrations::KNOWLEDGE_EMBEDDING_DIMS;
use super::repository::{map_storage_error, RepositoryError};

/// Over-fetch multiplier when filtering by namespace (vec0 cannot LIKE on id in KNN).
const NAMESPACE_OVERFETCH_FACTOR: u32 = 8;
/// Cap on KNN `k` — matches `worker::MAX_LIST_LIMIT`.
const MAX_KNN_K: u32 = 200;

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
///
/// vec0 KNN cannot constrain TEXT metadata with LIKE/GLOB, so non-`All`
/// namespaces over-fetch then filter in Rust (id is exact-match metadata only).
pub(crate) fn search_chunks(
    connection: &Connection,
    query_embedding: &[f32],
    limit: u32,
    namespace: KnowledgeNamespace,
) -> Result<Vec<KnowledgeSearchHit>, RepositoryError> {
    let blob = embedding_blob(query_embedding)?;
    let limit = limit.max(1);
    let k = if namespace == KnowledgeNamespace::All {
        i64::from(limit)
    } else {
        i64::from(limit.saturating_mul(NAMESPACE_OVERFETCH_FACTOR).min(MAX_KNN_K))
    };
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
        let hit = row.map_err(map_storage_error)?;
        if knowledge_namespace::matches(namespace, &hit.id) {
            hits.push(hit);
            if hits.len() >= limit as usize {
                break;
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use rusqlite::Connection;
    use std::error::Error;

    fn emb(seed: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; KNOWLEDGE_EMBEDDING_DIMS];
        v[0] = seed;
        v[1] = 1.0 - seed.abs().min(1.0);
        // L2-ish non-zero so vec0 distance is defined.
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for x in &mut v {
            *x /= norm;
        }
        v
    }

    fn insert_chunk(
        conn: &Connection,
        id: &str,
        text: &str,
        seed: f32,
    ) -> Result<(), Box<dyn Error>> {
        let blob = embedding_blob(&emb(seed))?;
        conn.execute(
            "INSERT INTO knowledge_chunks(embedding, id, created_at, text_content) \
             VALUES (?1, ?2, ?3, ?4)",
            params![blob, id, 1_700_000_000_i64, text],
        )?;
        Ok(())
    }

    #[test]
    fn search_chunks_namespace_separation() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;

        // Near-identical embeddings so KNN returns both; filter decides.
        insert_chunk(&connection, "line-talk::0001", "LINE personal", 0.91)?;
        insert_chunk(&connection, "edinet-E1::0001", "EDINET company", 0.90)?;
        insert_chunk(&connection, "daily-2026-07-20::0000", "daily personal", 0.89)?;
        insert_chunk(&connection, "company-acme::0000", "company lane", 0.88)?;

        let q = emb(0.91);
        let company = search_chunks(&connection, &q, 10, KnowledgeNamespace::Company)?;
        assert!(
            company.iter().all(|h| knowledge_namespace::matches(
                KnowledgeNamespace::Company,
                &h.id
            )),
            "Company lane must not include personal ids: {:?}",
            company.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
        assert!(
            company.iter().any(|h| h.id.starts_with("edinet-") || h.id.starts_with("company-")),
            "expected at least one company hit"
        );

        let personal = search_chunks(&connection, &q, 10, KnowledgeNamespace::Personal)?;
        assert!(
            personal.iter().all(|h| knowledge_namespace::matches(
                KnowledgeNamespace::Personal,
                &h.id
            )),
            "Personal lane must not include company ids: {:?}",
            personal.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
        assert!(
            !personal.iter().any(|h| h.id.starts_with("edinet-") || h.id.starts_with("company-")),
            "personal must exclude company"
        );

        let all = search_chunks(&connection, &q, 10, KnowledgeNamespace::All)?;
        assert!(all.len() >= 2, "All should return mixed hits, got {}", all.len());
        Ok(())
    }
}
