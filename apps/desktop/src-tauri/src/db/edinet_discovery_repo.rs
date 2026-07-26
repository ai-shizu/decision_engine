//! Vault repository for EDINET light discovery metadata.
//!
//! Raw list JSON never crosses this boundary. A successful day commit replaces
//! that date's normalized 120/130 index and writes `coverage=ok` in one
//! transaction.

use rusqlite::{params, Connection, Transaction};

use crate::knowledge::edinet_client::EdinetDocumentMeta;
use crate::knowledge::edinet_discovery::{
    CoverageDayStatus, CoverageRecord, DayCommit, DiscoveryCoverage, ScanCursor,
};

use super::repository::{map_storage_error, RepositoryError};

fn coverage_status_text(status: CoverageDayStatus) -> &'static str {
    match status {
        CoverageDayStatus::Ok => "ok",
        CoverageDayStatus::Failed => "failed",
        CoverageDayStatus::Pending => "pending",
    }
}

fn parse_coverage_status(value: &str) -> Result<CoverageDayStatus, RepositoryError> {
    match value {
        "ok" => Ok(CoverageDayStatus::Ok),
        "failed" => Ok(CoverageDayStatus::Failed),
        "pending" => Ok(CoverageDayStatus::Pending),
        _ => Err(RepositoryError::StorageFailed),
    }
}

fn discovery_coverage_text(status: DiscoveryCoverage) -> &'static str {
    match status {
        DiscoveryCoverage::NotRun => "not_run",
        DiscoveryCoverage::WindowComplete => "window_complete",
        DiscoveryCoverage::WindowIncomplete => "window_incomplete",
        DiscoveryCoverage::Pinned => "pinned",
    }
}

fn parse_discovery_coverage(value: &str) -> Result<DiscoveryCoverage, RepositoryError> {
    match value {
        "not_run" => Ok(DiscoveryCoverage::NotRun),
        "window_complete" => Ok(DiscoveryCoverage::WindowComplete),
        "window_incomplete" => Ok(DiscoveryCoverage::WindowIncomplete),
        "pinned" => Ok(DiscoveryCoverage::Pinned),
        _ => Err(RepositoryError::StorageFailed),
    }
}

pub(crate) fn coverage(
    connection: &Connection,
    subject_key: &str,
    date: &str,
) -> Result<Option<CoverageRecord>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT status, fetched_at, process_date_time, error_class, revalidate_after \
             FROM edinet_list_coverage WHERE subject_key = ?1 AND date = ?2",
        )
        .map_err(map_storage_error)?;
    let mut rows = statement
        .query(params![subject_key, date])
        .map_err(map_storage_error)?;
    let Some(row) = rows.next().map_err(map_storage_error)? else {
        return Ok(None);
    };
    let status_text: String = row.get(0).map_err(map_storage_error)?;
    Ok(Some(CoverageRecord {
        subject_key: subject_key.to_string(),
        date: date.to_string(),
        status: parse_coverage_status(&status_text)?,
        fetched_at: row
            .get::<_, Option<i64>>(1)
            .map_err(map_storage_error)?
            .unwrap_or(0),
        process_date_time: row.get(2).map_err(map_storage_error)?,
        error_class: row.get(3).map_err(map_storage_error)?,
        revalidate_after: row.get(4).map_err(map_storage_error)?,
    }))
}

pub(crate) fn filings_for_date(
    connection: &Connection,
    subject_key: &str,
    date: &str,
) -> Result<Vec<EdinetDocumentMeta>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT doc_id, edinet_code, filer_name, ordinance_code, form_code, \
             doc_type_code, period_start, period_end, submit_date_time, parent_doc_id, \
             withdrawal_status, doc_info_edit_status, disclosure_status, xbrl_flag, \
             csv_flag, legal_status, doc_description, sec_code \
             FROM company_filing_index \
             WHERE subject_key = ?1 AND list_date = ?2 ORDER BY doc_id",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map(params![subject_key, date], |row| {
            Ok(EdinetDocumentMeta {
                doc_id: Some(row.get(0)?),
                edinet_code: row.get(1)?,
                filer_name: row.get(2)?,
                ordinance_code: row.get(3)?,
                form_code: row.get(4)?,
                doc_type_code: row.get(5)?,
                period_start: row.get(6)?,
                period_end: row.get(7)?,
                submit_date_time: row.get(8)?,
                parent_doc_id: row.get(9)?,
                withdrawal_status: row.get(10)?,
                doc_info_edit_status: row.get(11)?,
                disclosure_status: row.get(12)?,
                xbrl_flag: row.get(13)?,
                csv_flag: row.get(14)?,
                legal_status: row.get(15)?,
                doc_description: row.get(16)?,
                sec_code: row.get(17)?,
            })
        })
        .map_err(map_storage_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(map_storage_error)?);
    }
    Ok(out)
}

pub(crate) fn commit_day(
    transaction: &Transaction<'_>,
    day: &DayCommit,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "DELETE FROM company_filing_index WHERE subject_key = ?1 AND list_date = ?2",
            params![day.coverage.subject_key, day.coverage.date],
        )
        .map_err(map_storage_error)?;

    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO company_filing_index( \
                 subject_key, doc_id, list_date, edinet_code, filer_name, ordinance_code, \
                 form_code, doc_type_code, period_start, period_end, submit_date_time, \
                 parent_doc_id, withdrawal_status, doc_info_edit_status, disclosure_status, \
                 xbrl_flag, csv_flag, legal_status, doc_description, sec_code, \
                 process_date_time, indexed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                 ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            )
            .map_err(map_storage_error)?;
        for filing in &day.filings {
            let doc_id = filing
                .doc_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or(RepositoryError::StorageFailed)?;
            statement
                .execute(params![
                    day.coverage.subject_key,
                    doc_id,
                    day.coverage.date,
                    filing.edinet_code,
                    filing.filer_name,
                    filing.ordinance_code,
                    filing.form_code,
                    filing.doc_type_code,
                    filing.period_start,
                    filing.period_end,
                    filing.submit_date_time,
                    filing.parent_doc_id,
                    filing.withdrawal_status,
                    filing.doc_info_edit_status,
                    filing.disclosure_status,
                    filing.xbrl_flag,
                    filing.csv_flag,
                    filing.legal_status,
                    filing.doc_description,
                    filing.sec_code,
                    day.coverage.process_date_time,
                    day.coverage.fetched_at,
                ])
                .map_err(map_storage_error)?;
        }
    }

    transaction
        .execute(
            "INSERT INTO edinet_list_coverage( \
             subject_key, date, status, fetched_at, process_date_time, error_class, \
             revalidate_after) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(subject_key, date) DO UPDATE SET \
             status=excluded.status, fetched_at=excluded.fetched_at, \
             process_date_time=excluded.process_date_time, \
             error_class=excluded.error_class, revalidate_after=excluded.revalidate_after",
            params![
                day.coverage.subject_key,
                day.coverage.date,
                coverage_status_text(day.coverage.status),
                day.coverage.fetched_at,
                day.coverage.process_date_time,
                day.coverage.error_class,
                day.coverage.revalidate_after,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

pub(crate) fn cursor(
    connection: &Connection,
    subject_key: &str,
) -> Result<Option<ScanCursor>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT anchor_date, window_days_back, next_date, status, updated_at \
             FROM edinet_scan_cursor WHERE subject_key = ?1",
        )
        .map_err(map_storage_error)?;
    let mut rows = statement
        .query(params![subject_key])
        .map_err(map_storage_error)?;
    let Some(row) = rows.next().map_err(map_storage_error)? else {
        return Ok(None);
    };
    let window_days: i64 = row.get(1).map_err(map_storage_error)?;
    let window_days_back =
        u32::try_from(window_days).map_err(|_| RepositoryError::StorageFailed)?;
    let status_text: String = row.get(3).map_err(map_storage_error)?;
    Ok(Some(ScanCursor {
        subject_key: subject_key.to_string(),
        anchor_date: row.get(0).map_err(map_storage_error)?,
        window_days_back,
        next_date: row.get(2).map_err(map_storage_error)?,
        status: parse_discovery_coverage(&status_text)?,
        updated_at: row.get(4).map_err(map_storage_error)?,
    }))
}

pub(crate) fn save_cursor(
    transaction: &Transaction<'_>,
    cursor: &ScanCursor,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "INSERT INTO edinet_scan_cursor( \
             subject_key, anchor_date, window_days_back, next_date, status, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(subject_key) DO UPDATE SET \
             anchor_date=excluded.anchor_date, window_days_back=excluded.window_days_back, \
             next_date=excluded.next_date, status=excluded.status, updated_at=excluded.updated_at",
            params![
                cursor.subject_key,
                cursor.anchor_date,
                i64::from(cursor.window_days_back),
                cursor.next_date,
                discovery_coverage_text(cursor.status),
                cursor.updated_at,
            ],
        )
        .map_err(map_storage_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use crate::knowledge::edinet_client::ListCandidateFilter;
    use crate::knowledge::edinet_discovery::{
        discover_eligible_in_window, DiscoverParams, DiscoveryCache, DiscoveryError,
        DiscoveryResult,
    };
    use crate::knowledge::net_gateway::{GatewayError, HttpTransport, ResponseBody, ResponseMeta};
    use std::error::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct RepoCache {
        connection: Connection,
    }

    impl DiscoveryCache for RepoCache {
        fn coverage(
            &self,
            subject_key: &str,
            date: &str,
        ) -> Result<Option<CoverageRecord>, DiscoveryError> {
            super::coverage(&self.connection, subject_key, date)
                .map_err(|_| DiscoveryError::Cache("test repository read failed".into()))
        }

        fn filings_for_date(
            &self,
            subject_key: &str,
            date: &str,
        ) -> Result<Vec<EdinetDocumentMeta>, DiscoveryError> {
            super::filings_for_date(&self.connection, subject_key, date)
                .map_err(|_| DiscoveryError::Cache("test repository read failed".into()))
        }

        fn commit_day(&mut self, day: DayCommit) -> Result<(), DiscoveryError> {
            let transaction = self
                .connection
                .transaction()
                .map_err(|_| DiscoveryError::Cache("test transaction failed".into()))?;
            super::commit_day(&transaction, &day)
                .map_err(|_| DiscoveryError::Cache("test repository write failed".into()))?;
            transaction
                .commit()
                .map_err(|_| DiscoveryError::Cache("test commit failed".into()))
        }

        fn cursor(&self, subject_key: &str) -> Result<Option<ScanCursor>, DiscoveryError> {
            super::cursor(&self.connection, subject_key)
                .map_err(|_| DiscoveryError::Cache("test repository read failed".into()))
        }

        fn save_cursor(&mut self, cursor: ScanCursor) -> Result<(), DiscoveryError> {
            let transaction = self
                .connection
                .transaction()
                .map_err(|_| DiscoveryError::Cache("test transaction failed".into()))?;
            super::save_cursor(&transaction, &cursor)
                .map_err(|_| DiscoveryError::Cache("test repository write failed".into()))?;
            transaction
                .commit()
                .map_err(|_| DiscoveryError::Cache("test commit failed".into()))
        }
    }

    struct NeverBody;

    impl ResponseBody for NeverBody {
        fn next_chunk(
            &mut self,
        ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
        {
            async { None }
        }
    }

    struct CountingNeverTransport {
        gets: AtomicUsize,
    }

    impl HttpTransport for CountingNeverTransport {
        type Body = NeverBody;

        fn get(
            &self,
            _url: &str,
            _request_deadline: std::time::Duration,
        ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
        {
            self.gets.fetch_add(1, Ordering::SeqCst);
            async { Err(GatewayError::WireViolation) }
        }
    }

    fn sample_day() -> DayCommit {
        DayCommit {
            coverage: CoverageRecord {
                subject_key: "edinet:E02144".into(),
                date: "2024-06-25".into(),
                status: CoverageDayStatus::Ok,
                fetched_at: 100,
                process_date_time: Some("2024-06-25 18:00".into()),
                error_class: None,
                revalidate_after: Some(1_000),
            },
            filings: vec![EdinetDocumentMeta {
                doc_id: Some("S100VAULT".into()),
                edinet_code: Some("E02144".into()),
                filer_name: Some("テスト株式会社".into()),
                doc_type_code: Some("120".into()),
                submit_date_time: Some("2024-06-25 15:00".into()),
                withdrawal_status: Some("0".into()),
                disclosure_status: Some("0".into()),
                legal_status: Some("1".into()),
                xbrl_flag: Some("1".into()),
                ..EdinetDocumentMeta::default()
            }],
        }
    }

    #[test]
    fn day_cache_survives_database_reopen() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::TempDir::new()?;
        let path = temp.path().join("vault-cache.sqlite3");
        {
            let mut connection = Connection::open(&path)?;
            run_migrations(&mut connection)?;
            let transaction = connection.transaction()?;
            commit_day(&transaction, &sample_day())?;
            transaction.commit()?;
        }
        let connection = Connection::open(&path)?;
        let stored =
            coverage(&connection, "edinet:E02144", "2024-06-25")?.ok_or("coverage missing")?;
        assert_eq!(stored.status, CoverageDayStatus::Ok);
        let filings = filings_for_date(&connection, "edinet:E02144", "2024-06-25")?;
        assert_eq!(filings.len(), 1);
        assert_eq!(filings[0].doc_id.as_deref(), Some("S100VAULT"));
        Ok(())
    }

    #[tokio::test]
    async fn reopened_vault_cache_hit_performs_zero_http() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::TempDir::new()?;
        let path = temp.path().join("vault-cache-http-zero.sqlite3");
        {
            let mut connection = Connection::open(&path)?;
            run_migrations(&mut connection)?;
            let transaction = connection.transaction()?;
            commit_day(&transaction, &sample_day())?;
            transaction.commit()?;
        }

        let connection = Connection::open(&path)?;
        let mut cache = RepoCache { connection };
        let transport = CountingNeverTransport {
            gets: AtomicUsize::new(0),
        };
        let filter = ListCandidateFilter::by_edinet_code("E02144").ok_or("filter")?;
        let outcome = discover_eligible_in_window(
            &mut cache,
            &transport,
            DiscoverParams {
                subject_key: "edinet:E02144",
                anchor_date: "2024-06-25",
                window_days_back: 0,
                filter: &filter,
                subscription_key: "test-key",
                now_secs: 110,
                max_live_gets: None,
                deadline: Duration::from_secs(1),
            },
        )
        .await
        .expect("reopened cache discovery");
        assert_eq!(transport.gets.load(Ordering::SeqCst), 0);
        assert_eq!(outcome.http_gets, 0);
        assert_eq!(outcome.result, DiscoveryResult::Selected);
        Ok(())
    }

    #[test]
    fn index_and_coverage_roll_back_together() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        connection.execute_batch(
            "CREATE TRIGGER fail_coverage BEFORE INSERT ON edinet_list_coverage \
             BEGIN SELECT RAISE(ABORT, 'forced rollback'); END;",
        )?;
        {
            let transaction = connection.transaction()?;
            assert!(commit_day(&transaction, &sample_day()).is_err());
        }
        assert!(coverage(&connection, "edinet:E02144", "2024-06-25")?.is_none());
        assert!(filings_for_date(&connection, "edinet:E02144", "2024-06-25")?.is_empty());
        Ok(())
    }
}
