//! Field-level EDINET provenance persistence with revision CAS.

use rusqlite::{params, Connection, Transaction};

use super::repository::{map_storage_error, RepositoryError};
use crate::knowledge::fact_merge::{
    cell_from_wire, cell_to_wire, merge_rekey_snapshots, FactCellWire, FactCells, FactOriginWire,
    FactStorageWire,
};

fn origin_text(v: FactOriginWire) -> &'static str {
    match v {
        FactOriginWire::Manual => "manual",
        FactOriginWire::Wikipedia => "wikipedia",
        FactOriginWire::Edinet => "edinet",
        FactOriginWire::UnknownProtected => "unknown_protected",
        FactOriginWire::Unknown => "unknown",
    }
}
pub(crate) fn cells(
    connection: &Connection,
    subject_key: &str,
) -> Result<Vec<FactCellWire>, RepositoryError> {
    let mut stmt = connection.prepare("SELECT field,value,origin,storage,doc_id,submitted_at,fetched_at,revision,schema_version FROM company_fact_cells WHERE subject_key=?1 ORDER BY field").map_err(map_storage_error)?;
    let rows = stmt
        .query_map(params![subject_key], |row| {
            let origin = match row.get::<_, String>(2)?.as_str() {
                "manual" => FactOriginWire::Manual,
                "wikipedia" => FactOriginWire::Wikipedia,
                "edinet" => FactOriginWire::Edinet,
                "unknown_protected" => FactOriginWire::UnknownProtected,
                "unknown" => FactOriginWire::Unknown,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let storage = match row.get::<_, String>(3)?.as_str() {
                "session" => FactStorageWire::Session,
                "vault" => FactStorageWire::Vault,
                "live" => FactStorageWire::Live,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(FactCellWire {
                field: row.get(0)?,
                value: row.get(1)?,
                origin,
                storage,
                doc_id: row.get(4)?,
                submitted_at: row.get(5)?,
                fetched_at: row.get(6)?,
                revision: row.get(7)?,
                schema_version: row.get(8)?,
            })
        })
        .map_err(map_storage_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(map_storage_error)
}

pub(crate) fn persist_cells(
    transaction: &Transaction<'_>,
    subject_key: &str,
    expected_revision: i64,
    cells: &[FactCellWire],
    updated_at: i64,
) -> Result<i64, RepositoryError> {
    if cells.is_empty() {
        return Err(RepositoryError::StorageFailed);
    }
    let current: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(revision),0) FROM company_fact_cells WHERE subject_key=?1",
            params![subject_key],
            |row| row.get(0),
        )
        .map_err(map_storage_error)?;
    if current != expected_revision {
        return Err(RepositoryError::Conflict);
    }
    let next = current + 1;
    transaction
        .execute(
            "DELETE FROM company_fact_cells WHERE subject_key=?1",
            params![subject_key],
        )
        .map_err(map_storage_error)?;
    for cell in cells {
        transaction
            .execute(
                "INSERT INTO company_fact_cells(subject_key,field,value,origin,storage,doc_id,submitted_at,fetched_at,revision,schema_version,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![subject_key,cell.field,cell.value,origin_text(cell.origin),"vault",cell.doc_id,cell.submitted_at,cell.fetched_at,next,cell.schema_version,updated_at],
            )
            .map_err(map_storage_error)?;
    }
    Ok(next)
}

pub(crate) fn rekey_cells(
    transaction: &Transaction<'_>,
    from: &str,
    to: &str,
    expected_revision: i64,
    incoming: &[FactCellWire],
    updated_at: i64,
) -> Result<i64, RepositoryError> {
    let current: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(revision),0) FROM company_fact_cells WHERE subject_key=?1",
            params![from],
            |row| row.get(0),
        )
        .map_err(map_storage_error)?;
    if current != expected_revision {
        return Err(RepositoryError::Conflict);
    }
    let conflicting_alias_count: i64 = if table_exists(transaction, "subject_alias_candidate")? {
        transaction
            .query_row(
                "SELECT COUNT(DISTINCT canonical_key) FROM subject_alias_candidate WHERE alias_key=?1 AND canonical_key<>?2",
                params![from, to],
                |row| row.get(0),
            )
            .map_err(map_storage_error)?
    } else {
        0
    };
    if conflicting_alias_count > 0 {
        return Err(RepositoryError::IdentityAmbiguous);
    }

    let source: FactCells = cells(transaction, from)?
        .into_iter()
        .map(|wire| (wire.field.clone(), cell_from_wire(&wire)))
        .collect();
    let target: FactCells = cells(transaction, to)?
        .into_iter()
        .map(|wire| (wire.field.clone(), cell_from_wire(&wire)))
        .collect();
    let incoming: FactCells = incoming
        .iter()
        .map(|wire| (wire.field.clone(), cell_from_wire(wire)))
        .collect();
    let merged = merge_rekey_snapshots(&merge_rekey_snapshots(&target, &source), &incoming);
    if merged.is_empty() {
        return Err(RepositoryError::StorageFailed);
    }
    let target_revision = cells(transaction, to)?
        .iter()
        .map(|cell| cell.revision)
        .max()
        .unwrap_or(0);
    let next = current.max(target_revision) + 1;

    migrate_metadata(transaction, from, to, updated_at)?;
    transaction
        .execute(
            "DELETE FROM company_fact_cells WHERE subject_key IN (?1,?2)",
            params![from, to],
        )
        .map_err(map_storage_error)?;
    for (field, cell) in merged {
        let wire = cell_to_wire(&field, &cell, next, 3);
        transaction.execute(
            "INSERT INTO company_fact_cells(subject_key,field,value,origin,storage,doc_id,submitted_at,fetched_at,revision,schema_version,updated_at) VALUES(?1,?2,?3,?4,'vault',?5,?6,?7,?8,?9,?10)",
            params![to,wire.field,wire.value,origin_text(wire.origin),wire.doc_id,wire.submitted_at,wire.fetched_at,next,wire.schema_version,updated_at],
        ).map_err(map_storage_error)?;
    }
    Ok(next)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, RepositoryError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            params![table],
            |row| row.get(0),
        )
        .map_err(map_storage_error)
}

fn migrate_metadata(
    transaction: &Transaction<'_>,
    from: &str,
    to: &str,
    updated_at: i64,
) -> Result<(), RepositoryError> {
    if table_exists(transaction, "company_doc_pointers")? {
        transaction.execute(
            "INSERT INTO company_doc_pointers(subject_key,latest_selected_doc,latest_selected_rev,rag_ready_doc,rag_ready_rev,updated_at)
             SELECT ?2,latest_selected_doc,latest_selected_rev,rag_ready_doc,rag_ready_rev,updated_at FROM company_doc_pointers WHERE subject_key=?1
             ON CONFLICT(subject_key) DO UPDATE SET
               latest_selected_doc=CASE WHEN excluded.latest_selected_rev>latest_selected_rev THEN excluded.latest_selected_doc ELSE latest_selected_doc END,
               latest_selected_rev=MAX(latest_selected_rev,excluded.latest_selected_rev),
               rag_ready_doc=CASE WHEN excluded.rag_ready_rev>rag_ready_rev THEN excluded.rag_ready_doc ELSE rag_ready_doc END,
               rag_ready_rev=MAX(rag_ready_rev,excluded.rag_ready_rev),
               updated_at=MAX(updated_at,excluded.updated_at)",
            params![from, to],
        ).map_err(map_storage_error)?;
        transaction
            .execute(
                "DELETE FROM company_doc_pointers WHERE subject_key=?1",
                params![from],
            )
            .map_err(map_storage_error)?;
    }
    if table_exists(transaction, "company_filing_index")? {
        transaction
            .execute(
                "DELETE FROM company_filing_index AS target
             WHERE target.subject_key=?2 AND EXISTS(
               SELECT 1 FROM company_filing_index AS source
               WHERE source.subject_key=?1 AND source.doc_id=target.doc_id
                 AND source.indexed_at>target.indexed_at)",
                params![from, to],
            )
            .map_err(map_storage_error)?;
        transaction
            .execute(
                "DELETE FROM company_filing_index AS source
             WHERE source.subject_key=?1 AND EXISTS(
               SELECT 1 FROM company_filing_index AS target
               WHERE target.subject_key=?2 AND target.doc_id=source.doc_id)",
                params![from, to],
            )
            .map_err(map_storage_error)?;
        transaction
            .execute(
                "UPDATE company_filing_index SET subject_key=?2 WHERE subject_key=?1",
                params![from, to],
            )
            .map_err(map_storage_error)?;
    }
    if table_exists(transaction, "edinet_list_coverage")? {
        transaction
            .execute(
                "DELETE FROM edinet_list_coverage AS target
             WHERE target.subject_key=?2 AND EXISTS(
               SELECT 1 FROM edinet_list_coverage AS source
               WHERE source.subject_key=?1 AND source.date=target.date
                 AND (
                   (source.status='ok' AND target.status<>'ok')
                   OR (CASE WHEN source.status='ok' THEN 1 ELSE 0 END =
                       CASE WHEN target.status='ok' THEN 1 ELSE 0 END
                       AND COALESCE(source.fetched_at,-1)>COALESCE(target.fetched_at,-1))
                 ))",
                params![from, to],
            )
            .map_err(map_storage_error)?;
        transaction
            .execute(
                "DELETE FROM edinet_list_coverage AS source
             WHERE source.subject_key=?1 AND EXISTS(
               SELECT 1 FROM edinet_list_coverage AS target
               WHERE target.subject_key=?2 AND target.date=source.date)",
                params![from, to],
            )
            .map_err(map_storage_error)?;
        transaction
            .execute(
                "UPDATE edinet_list_coverage SET subject_key=?2 WHERE subject_key=?1",
                params![from, to],
            )
            .map_err(map_storage_error)?;
    }
    if table_exists(transaction, "edinet_scan_cursor")? {
        transaction
            .execute(
            "DELETE FROM edinet_scan_cursor WHERE subject_key=?2 AND EXISTS(
               SELECT 1 FROM edinet_scan_cursor WHERE subject_key=?1
                 AND (
                   (status='window_complete' AND
                    (SELECT status FROM edinet_scan_cursor WHERE subject_key=?2)<>'window_complete')
                   OR (CASE WHEN status='window_complete' THEN 1 ELSE 0 END =
                       CASE WHEN (SELECT status FROM edinet_scan_cursor WHERE subject_key=?2)='window_complete' THEN 1 ELSE 0 END
                       AND updated_at>(SELECT updated_at FROM edinet_scan_cursor WHERE subject_key=?2))
                 ))",
                params![from, to],
            )
            .map_err(map_storage_error)?;
        transaction
            .execute(
                "DELETE FROM edinet_scan_cursor WHERE subject_key=?1 AND EXISTS(
               SELECT 1 FROM edinet_scan_cursor WHERE subject_key=?2)",
                params![from, to],
            )
            .map_err(map_storage_error)?;
        transaction
            .execute(
                "UPDATE edinet_scan_cursor SET subject_key=?2 WHERE subject_key=?1",
                params![from, to],
            )
            .map_err(map_storage_error)?;
    }
    if table_exists(transaction, "subject_alias_candidate")? {
        transaction.execute("INSERT OR IGNORE INTO subject_alias_candidate(alias_key,canonical_key,created_at) SELECT alias_key,?2,created_at FROM subject_alias_candidate WHERE canonical_key=?1", params![from,to]).map_err(map_storage_error)?;
        transaction
            .execute(
                "DELETE FROM subject_alias_candidate WHERE canonical_key=?1",
                params![from],
            )
            .map_err(map_storage_error)?;
        transaction.execute(
            "INSERT OR IGNORE INTO subject_alias_candidate(alias_key,canonical_key,created_at) VALUES(?1,?2,?3)",
            params![from, to, updated_at],
        ).map_err(map_storage_error)?;
        transaction
            .execute(
                "DELETE FROM subject_alias_candidate WHERE alias_key=?1 AND canonical_key<>?2",
                params![from, to],
            )
            .map_err(map_storage_error)?;
    }
    for table in ["knowledge_chunk_meta", "edinet_evidence_pending"] {
        if table_exists(transaction, table)? {
            let sql = format!("UPDATE {table} SET subject_key=?2 WHERE subject_key=?1");
            transaction
                .execute(&sql, params![from, to])
                .map_err(map_storage_error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn schema(connection: &Connection) {
        connection.execute_batch("CREATE TABLE company_fact_cells(subject_key TEXT NOT NULL,field TEXT NOT NULL,value TEXT NOT NULL,origin TEXT NOT NULL,storage TEXT NOT NULL,doc_id TEXT,submitted_at TEXT,fetched_at INTEGER,revision INTEGER NOT NULL,schema_version INTEGER NOT NULL,updated_at INTEGER NOT NULL,PRIMARY KEY(subject_key,field));").unwrap();
    }
    fn wire(field: &str, value: &str) -> FactCellWire {
        FactCellWire {
            field: field.into(),
            value: value.into(),
            origin: FactOriginWire::Edinet,
            storage: FactStorageWire::Live,
            doc_id: None,
            submitted_at: Some("2024-01-01".into()),
            fetched_at: None,
            revision: 0,
            schema_version: 3,
        }
    }

    #[test]
    fn cas_snapshot_replace_and_vault_normalize() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        let tx = connection.transaction().unwrap();
        assert_eq!(
            persist_cells(
                &tx,
                "name:a",
                0,
                &[wire("business_summary", "x"), wire("business_risks", "y")],
                1
            )
            .unwrap(),
            1
        );
        tx.commit().unwrap();
        let tx = connection.transaction().unwrap();
        assert_eq!(
            persist_cells(&tx, "name:a", 1, &[wire("business_summary", "z")], 2).unwrap(),
            2
        );
        tx.commit().unwrap();
        let rows = cells(&connection, "name:a").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].revision, 2);
        assert_eq!(rows[0].storage, FactStorageWire::Vault);
    }

    #[test]
    fn stale_revision_does_not_change_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        let tx = connection.transaction().unwrap();
        persist_cells(&tx, "name:a", 0, &[wire("business_summary", "x")], 1).unwrap();
        tx.commit().unwrap();
        let tx = connection.transaction().unwrap();
        assert_eq!(
            persist_cells(&tx, "name:a", 0, &[wire("business_summary", "bad")], 2),
            Err(RepositoryError::Conflict)
        );
        tx.rollback().unwrap();
        assert_eq!(cells(&connection, "name:a").unwrap()[0].value, "x");
    }

    #[test]
    fn empty_snapshot_is_rejected_without_revision() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        let tx = connection.transaction().unwrap();
        assert!(persist_cells(&tx, "name:a", 0, &[], 1).is_err());
        tx.rollback().unwrap();
        assert!(cells(&connection, "name:a").unwrap().is_empty());
    }

    #[test]
    fn insert_failure_rolls_back_delete_and_prior_insert() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        connection.execute_batch("CREATE TRIGGER fail_snapshot_insert BEFORE INSERT ON company_fact_cells WHEN NEW.value='FAIL' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        let tx = connection.transaction().unwrap();
        persist_cells(
            &tx,
            "name:a",
            0,
            &[
                wire("business_summary", "old"),
                wire("business_risks", "old-risk"),
            ],
            1,
        )
        .unwrap();
        tx.commit().unwrap();
        let tx = connection.transaction().unwrap();
        let result = persist_cells(
            &tx,
            "name:a",
            1,
            &[
                wire("business_summary", "new"),
                wire("business_risks", "FAIL"),
            ],
            2,
        );
        assert!(result.is_err());
        tx.rollback().unwrap();
        let rows = cells(&connection, "name:a").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.value == "old"));
        assert!(rows.iter().any(|row| row.value == "old-risk"));
    }

    #[test]
    fn rekey_moves_snapshot_and_advances_revision() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        let tx = connection.transaction().unwrap();
        persist_cells(&tx, "name:a", 0, &[wire("business_summary", "x")], 1).unwrap();
        tx.commit().unwrap();
        let tx = connection.transaction().unwrap();
        assert_eq!(
            rekey_cells(
                &tx,
                "name:a",
                "edinet:E12345",
                1,
                &[wire("business_summary", "x")],
                2,
            )
            .unwrap(),
            2
        );
        tx.commit().unwrap();
        assert!(cells(&connection, "name:a").unwrap().is_empty());
        let rows = cells(&connection, "edinet:E12345").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].revision, 2);
        assert_eq!(rows[0].storage, FactStorageWire::Vault);
    }

    #[test]
    fn initial_zero_row_rekey_persists_target_snapshot_atomically() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        let tx = connection.transaction().unwrap();
        assert_eq!(
            rekey_cells(
                &tx,
                "name:a",
                "edinet:E12345",
                0,
                &[wire("business_summary", "first")],
                1,
            )
            .unwrap(),
            1
        );
        tx.commit().unwrap();
        let rows = cells(&connection, "edinet:E12345").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].revision, 1);
        assert_eq!(rows[0].value, "first");
    }

    #[test]
    fn rekey_merges_existing_target_snapshot() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        let tx = connection.transaction().unwrap();
        persist_cells(
            &tx,
            "edinet:E12345",
            0,
            &[wire("business_summary", "target")],
            1,
        )
        .unwrap();
        persist_cells(&tx, "name:a", 0, &[wire("business_risks", "source")], 1).unwrap();
        tx.commit().unwrap();
        let tx = connection.transaction().unwrap();
        assert_eq!(
            rekey_cells(
                &tx,
                "name:a",
                "edinet:E12345",
                1,
                &[wire("performance_summary", "incoming")],
                2,
            )
            .unwrap(),
            2
        );
        tx.commit().unwrap();
        let rows = cells(&connection, "edinet:E12345").unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.revision == 2));
    }

    #[test]
    fn rekey_moves_metadata_and_pending_evidence_in_same_transaction() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        connection.execute_batch(
            "CREATE TABLE subject_alias_candidate(alias_key TEXT NOT NULL,canonical_key TEXT NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(alias_key,canonical_key));
             CREATE TABLE company_doc_pointers(subject_key TEXT PRIMARY KEY,latest_selected_doc TEXT,latest_selected_rev INTEGER NOT NULL,rag_ready_doc TEXT,rag_ready_rev INTEGER NOT NULL,updated_at INTEGER NOT NULL);
             CREATE TABLE company_filing_index(subject_key TEXT NOT NULL,doc_id TEXT NOT NULL,indexed_at INTEGER NOT NULL,PRIMARY KEY(subject_key,doc_id));
             CREATE TABLE edinet_list_coverage(subject_key TEXT NOT NULL,date TEXT NOT NULL,status TEXT NOT NULL,fetched_at INTEGER,PRIMARY KEY(subject_key,date));
             CREATE TABLE edinet_scan_cursor(subject_key TEXT PRIMARY KEY,status TEXT NOT NULL,updated_at INTEGER NOT NULL);
             CREATE TABLE knowledge_chunk_meta(chunk_id TEXT PRIMARY KEY,subject_key TEXT);
             CREATE TABLE edinet_evidence_pending(id TEXT PRIMARY KEY,subject_key TEXT NOT NULL);",
        ).unwrap();
        connection
            .execute_batch(
                "INSERT INTO company_doc_pointers VALUES('name:a','S1',1,NULL,0,1);
             INSERT INTO company_filing_index VALUES('name:a','S1',1);
             INSERT INTO company_filing_index VALUES('edinet:E12345','S1',2);
             INSERT INTO edinet_list_coverage VALUES('name:a','2026-07-26','ok',1);
             INSERT INTO edinet_list_coverage VALUES('edinet:E12345','2026-07-26','ok',2);
             INSERT INTO edinet_scan_cursor VALUES('name:a','window_complete',1);
             INSERT INTO edinet_scan_cursor VALUES('edinet:E12345','window_complete',2);
             INSERT INTO knowledge_chunk_meta VALUES('chunk','name:a');
             INSERT INTO edinet_evidence_pending VALUES('pending','name:a');",
            )
            .unwrap();
        let tx = connection.transaction().unwrap();
        rekey_cells(
            &tx,
            "name:a",
            "edinet:E12345",
            0,
            &[wire("business_summary", "first")],
            10,
        )
        .unwrap();
        tx.commit().unwrap();
        for table in [
            "company_doc_pointers",
            "company_filing_index",
            "edinet_list_coverage",
            "edinet_scan_cursor",
            "knowledge_chunk_meta",
            "edinet_evidence_pending",
        ] {
            let sql = format!("SELECT COUNT(*) FROM {table} WHERE subject_key='edinet:E12345'");
            assert_eq!(
                connection
                    .query_row(&sql, [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT canonical_key FROM subject_alias_candidate WHERE alias_key='name:a'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "edinet:E12345"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT indexed_at FROM company_filing_index WHERE subject_key='edinet:E12345' AND doc_id='S1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT fetched_at FROM edinet_list_coverage WHERE subject_key='edinet:E12345'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT updated_at FROM edinet_scan_cursor WHERE subject_key='edinet:E12345'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
    }

    #[test]
    fn rekey_rejects_single_alias_pointing_to_another_canonical() {
        let mut connection = Connection::open_in_memory().unwrap();
        schema(&connection);
        connection.execute_batch(
            "CREATE TABLE subject_alias_candidate(alias_key TEXT NOT NULL,canonical_key TEXT NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(alias_key,canonical_key));
             INSERT INTO subject_alias_candidate VALUES('name:a','edinet:E99999',1);",
        ).unwrap();
        let tx = connection.transaction().unwrap();
        assert_eq!(
            rekey_cells(
                &tx,
                "name:a",
                "edinet:E12345",
                0,
                &[wire("business_summary", "first")],
                10,
            ),
            Err(RepositoryError::IdentityAmbiguous)
        );
    }
}
