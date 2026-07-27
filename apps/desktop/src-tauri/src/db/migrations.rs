//! Immutable, sequential SQLCipher schema migrations.
//!
//! Every migration runs in its own IMMEDIATE transaction. The schema is
//! verified before `user_version` is advanced, so an incompatible pre-existing
//! table cannot be silently accepted by `CREATE TABLE IF NOT EXISTS`.

use std::{error::Error, fmt};

use rusqlite::{Connection, TransactionBehavior};

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 12;

/// Canonical embedding width for `knowledge_chunks.embedding` (M9 foundation).
/// Matches the historical PKBVEC01 384-d space; a future 768-d migration would
/// bump the schema version rather than silently reshaping the vec0 table.
pub(crate) const KNOWLEDGE_EMBEDDING_DIMS: usize = 384;

const MIGRATION_V1_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY NOT NULL,
    chat_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_chat_timestamp
    ON messages(chat_id, timestamp, id);
"#;

// vec0 virtual table: vector column + metadata (`id`, `created_at`) + auxiliary
// long text (`+text_content`). Requires sqlite-vec auto-extension (see
// `sqlite_vec_ext`). Dimension is fixed at KNOWLEDGE_EMBEDDING_DIMS.
const MIGRATION_V2_SQL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks USING vec0(
    embedding float[384],
    id TEXT,
    created_at INTEGER,
    +text_content TEXT
);
"#;

const MIGRATION_V3_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS gap_analysis_runs (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    schema_version TEXT NOT NULL,
    data_sufficiency REAL NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gap_analysis_created
    ON gap_analysis_runs(created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS tensor_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    schema_version TEXT NOT NULL,
    model_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tensor_profiles_created
    ON tensor_profiles(created_at DESC, id DESC);
"#;

const MIGRATION_V4_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS interaction_pulse_runs (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    affinity_score INTEGER,
    interaction_tendency TEXT NOT NULL,
    next_best_action TEXT NOT NULL,
    metrics_json TEXT NOT NULL,
    input_hash TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pulse_runs_created
    ON interaction_pulse_runs(created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS rasch_filter_runs (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    artifact_sha256 TEXT NOT NULL,
    posterior_json TEXT NOT NULL,
    excluded_json TEXT NOT NULL,
    last_selection_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_rasch_runs_created
    ON rasch_filter_runs(created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS probe_store (
    id TEXT PRIMARY KEY NOT NULL,
    updated_at INTEGER NOT NULL,
    payload_json TEXT NOT NULL
);
"#;

const MIGRATION_V5_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS twin_scenario_runs (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    schema_version TEXT NOT NULL,
    gate_passed INTEGER NOT NULL,
    bss REAL NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_twin_runs_created
    ON twin_scenario_runs(created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS oracle_payload_runs (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    schema_version TEXT NOT NULL,
    gate_passed INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    provenance_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oracle_runs_created
    ON oracle_payload_runs(created_at DESC, id DESC);
"#;

const MIGRATION_V6_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS interview_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    updated_at INTEGER NOT NULL,
    stage TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_interview_sessions_updated
    ON interview_sessions(updated_at DESC, id DESC);
"#;

const MIGRATION_V7_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS distortion_tags (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    category TEXT NOT NULL,
    snippet TEXT NOT NULL,
    confidence_score REAL NOT NULL,
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    run_id TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_distortion_tags_created
    ON distortion_tags(created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_distortion_tags_category
    ON distortion_tags(category, created_at DESC);
"#;

const MIGRATION_V8_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS purchases (
    id TEXT PRIMARY KEY NOT NULL,
    occurred_at INTEGER NOT NULL,
    merchant_norm TEXT NOT NULL,
    total_amount INTEGER NOT NULL,
    tax INTEGER NOT NULL,
    verified INTEGER NOT NULL,
    r_at_decision REAL NOT NULL,
    active_distortions_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_purchases_occurred
    ON purchases(occurred_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS purchase_lines (
    id TEXT PRIMARY KEY NOT NULL,
    purchase_id TEXT NOT NULL,
    item_name TEXT NOT NULL,
    unit_price INTEGER NOT NULL,
    qty INTEGER NOT NULL,
    amount INTEGER NOT NULL,
    FOREIGN KEY (purchase_id) REFERENCES purchases(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_purchase_lines_purchase
    ON purchase_lines(purchase_id, id);
"#;

const MIGRATION_V9_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS commitments (
    id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    condition_json TEXT NOT NULL,
    action_type TEXT NOT NULL,
    custom_prompt TEXT NOT NULL,
    delay_seconds INTEGER NOT NULL,
    source_relation_id TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    origin TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_commitments_enabled
    ON commitments(enabled DESC, created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_commitments_source_relation
    ON commitments(source_relation_id);
"#;

const MIGRATION_V10_SQL: &str = r#"
ALTER TABLE interview_sessions ADD COLUMN artifact_json TEXT NOT NULL DEFAULT '';
ALTER TABLE interview_sessions ADD COLUMN artifact_fingerprint TEXT NOT NULL DEFAULT '';
"#;

/// V11 — EDINET lane vault tables (DESIGN_V3 §5–§6). Additive; vec0 untouched.
const MIGRATION_V11_SQL: &str = r#"
CREATE TABLE company_fact_cells (
    subject_key TEXT NOT NULL,
    field TEXT NOT NULL,
    value TEXT NOT NULL,
    origin TEXT NOT NULL,
    storage TEXT NOT NULL,
    doc_id TEXT,
    submitted_at TEXT,
    fetched_at INTEGER,
    revision INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (subject_key, field)
);

CREATE TABLE subject_alias_candidate (
    alias_key TEXT NOT NULL,
    canonical_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (alias_key, canonical_key)
);

CREATE TABLE company_doc_pointers (
    subject_key TEXT NOT NULL PRIMARY KEY,
    latest_selected_doc TEXT,
    latest_selected_rev INTEGER NOT NULL DEFAULT 0,
    rag_ready_doc TEXT,
    rag_ready_rev INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);

CREATE TABLE company_filing_index (
    subject_key TEXT NOT NULL,
    doc_id TEXT NOT NULL,
    list_date TEXT NOT NULL,
    edinet_code TEXT,
    filer_name TEXT,
    ordinance_code TEXT,
    form_code TEXT,
    doc_type_code TEXT,
    period_start TEXT,
    period_end TEXT,
    submit_date_time TEXT,
    parent_doc_id TEXT,
    withdrawal_status TEXT,
    doc_info_edit_status TEXT,
    disclosure_status TEXT,
    xbrl_flag TEXT,
    csv_flag TEXT,
    legal_status TEXT,
    doc_description TEXT,
    sec_code TEXT,
    process_date_time TEXT,
    indexed_at INTEGER NOT NULL,
    PRIMARY KEY (subject_key, doc_id)
);
CREATE INDEX IF NOT EXISTS idx_filing_index_subject_date
    ON company_filing_index(subject_key, list_date);

CREATE TABLE edinet_list_coverage (
    subject_key TEXT NOT NULL,
    date TEXT NOT NULL,
    status TEXT NOT NULL,
    fetched_at INTEGER,
    process_date_time TEXT,
    error_class TEXT,
    revalidate_after INTEGER,
    PRIMARY KEY (subject_key, date)
);

CREATE TABLE edinet_scan_cursor (
    subject_key TEXT NOT NULL PRIMARY KEY,
    anchor_date TEXT NOT NULL,
    window_days_back INTEGER NOT NULL,
    next_date TEXT,
    status TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE knowledge_chunk_meta (
    chunk_id TEXT NOT NULL PRIMARY KEY,
    source_id TEXT NOT NULL,
    subject_key TEXT,
    doc_id TEXT,
    revision INTEGER,
    section_id TEXT,
    concept TEXT,
    period_start TEXT,
    period_end TEXT,
    unit TEXT,
    submitted_at TEXT,
    truncated INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE TABLE edinet_evidence_pending (
    id TEXT NOT NULL PRIMARY KEY,
    subject_key TEXT NOT NULL,
    revision INTEGER NOT NULL,
    doc_id TEXT NOT NULL,
    section_id TEXT,
    concept TEXT,
    text TEXT NOT NULL,
    state TEXT NOT NULL,
    claim_id TEXT,
    claimed_at INTEGER,
    claim_deadline INTEGER,
    retry_count INTEGER NOT NULL DEFAULT 0,
    submitted_at TEXT,
    period_start TEXT,
    period_end TEXT,
    unit TEXT,
    truncated INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_evidence_pending_subject_rev
    ON edinet_evidence_pending(subject_key, revision);
CREATE INDEX IF NOT EXISTS idx_evidence_pending_state_deadline
    ON edinet_evidence_pending(state, claim_deadline);
"#;

/// V12 — BLACKBOX SIMULATOR record lanes (docs/SPEC_BLACKBOX_SIMULATOR.md §11).
/// Additive; vec0 untouched.
///
/// Three tables, one per kind of thing the simulator produces:
///
/// - `blackbox_campaigns` — one row per campaign, keyed by the Genesis
///   fingerprint. Note what is absent: the campaign *seed*. The seed
///   regenerates the world's true parameters, so storing it beside the
///   analysis would put the answer key one join away from anything that reads
///   this lane (wall W-a).
/// - `blackbox_decisions` — the append-only decision log (第八律). `seq` is
///   assigned by the engine and is unique within a campaign; the primary key
///   makes a replayed flush idempotent instead of duplicating history.
/// - `blackbox_stimuli` — the answer key the estimators join against. Carries
///   `params_digest` so the pointer/payload binding survives the round trip
///   (SPEC §16.3): a row whose parameters no longer hash to the digest the
///   decision recorded is a corrupted measurement, not a hint.
///
/// `channel` is stamped on every decision row and exists to keep wall W-c
/// mechanical: gap_analysis selects by channel, and simulator behaviour is
/// behaviour under a controlled instrument, not life. There is no free-text
/// column anywhere in this migration, so PII cannot land here by construction.
const MIGRATION_V12_SQL: &str = r#"
CREATE TABLE blackbox_campaigns (
    campaign_fingerprint BLOB NOT NULL PRIMARY KEY,
    schema_version TEXT NOT NULL,
    scenario_id INTEGER NOT NULL,
    difficulty INTEGER NOT NULL,
    campaign_index INTEGER NOT NULL,
    created_date TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE blackbox_decisions (
    campaign_fingerprint BLOB NOT NULL,
    seq INTEGER NOT NULL,
    tick INTEGER NOT NULL,
    phase INTEGER NOT NULL,
    action_kind INTEGER NOT NULL,
    action_json TEXT NOT NULL,
    stimulus_seq INTEGER,
    stimulus_kind INTEGER,
    stimulus_digest BLOB,
    latency_ms INTEGER,
    forced_default INTEGER NOT NULL,
    state_digest BLOB NOT NULL,
    channel TEXT NOT NULL,
    persisted_at INTEGER NOT NULL,
    PRIMARY KEY (campaign_fingerprint, seq),
    FOREIGN KEY (campaign_fingerprint)
        REFERENCES blackbox_campaigns(campaign_fingerprint) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_blackbox_decisions_stimulus
    ON blackbox_decisions(campaign_fingerprint, stimulus_seq);

CREATE TABLE blackbox_stimuli (
    campaign_fingerprint BLOB NOT NULL,
    stimulus_seq INTEGER NOT NULL,
    tick INTEGER NOT NULL,
    kind INTEGER NOT NULL,
    params_json TEXT NOT NULL,
    params_digest BLOB NOT NULL,
    persisted_at INTEGER NOT NULL,
    PRIMARY KEY (campaign_fingerprint, stimulus_seq),
    FOREIGN KEY (campaign_fingerprint)
        REFERENCES blackbox_campaigns(campaign_fingerprint) ON DELETE CASCADE
);
"#;

const READ_CHATS_COLUMNS_SQL: &str =
    "SELECT name, type, \"notnull\", pk FROM pragma_table_info('chats') ORDER BY cid;";
const READ_MESSAGES_COLUMNS_SQL: &str =
    "SELECT name, type, \"notnull\", pk FROM pragma_table_info('messages') ORDER BY cid;";
const READ_MESSAGES_FOREIGN_KEYS_SQL: &str = "SELECT \"table\", \"from\", \"to\", on_delete \
     FROM pragma_foreign_key_list('messages') ORDER BY id, seq;";
const READ_MESSAGES_INDEX_SQL: &str = "SELECT count(*) FROM sqlite_schema \
     WHERE type = 'index' AND name = 'idx_messages_chat_timestamp' AND tbl_name = 'messages';";
const READ_MESSAGES_INDEX_COLUMNS_SQL: &str =
    "SELECT name FROM pragma_index_info('idx_messages_chat_timestamp') ORDER BY seqno;";
const READ_MESSAGES_INDEX_PROPERTIES_SQL: &str =
    "SELECT \"unique\", partial FROM pragma_index_list('messages') \
     WHERE name = 'idx_messages_chat_timestamp';";

#[derive(Clone, Copy)]
struct Migration {
    version: i64,
    sql: &'static str,
    verify: fn(&Connection) -> Result<(), MigrationError>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: MIGRATION_V1_SQL,
        verify: verify_v1_schema,
    },
    Migration {
        version: 2,
        sql: MIGRATION_V2_SQL,
        verify: verify_v2_schema,
    },
    Migration {
        version: 3,
        sql: MIGRATION_V3_SQL,
        verify: verify_v3_schema,
    },
    Migration {
        version: 4,
        sql: MIGRATION_V4_SQL,
        verify: verify_v4_schema,
    },
    Migration {
        version: 5,
        sql: MIGRATION_V5_SQL,
        verify: verify_v5_schema,
    },
    Migration {
        version: 6,
        sql: MIGRATION_V6_SQL,
        verify: verify_v6_schema,
    },
    Migration {
        version: 7,
        sql: MIGRATION_V7_SQL,
        verify: verify_v7_schema,
    },
    Migration {
        version: 8,
        sql: MIGRATION_V8_SQL,
        verify: verify_v8_schema,
    },
    Migration {
        version: 9,
        sql: MIGRATION_V9_SQL,
        verify: verify_v9_schema,
    },
    Migration {
        version: 10,
        sql: MIGRATION_V10_SQL,
        verify: verify_v10_schema,
    },
    Migration {
        version: 11,
        sql: MIGRATION_V11_SQL,
        verify: verify_v11_schema,
    },
    Migration {
        version: 12,
        sql: MIGRATION_V12_SQL,
        verify: verify_v12_schema,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MigrationError {
    VersionReadFailed,
    InvalidVersion { found: i64 },
    UnsupportedVersion { found: i64, latest: i64 },
    MissingMigration { expected: i64 },
    ForeignKeysEnableFailed,
    TransactionBeginFailed { version: i64 },
    MigrationApplyFailed { version: i64 },
    SchemaMismatch { version: i64 },
    VersionWriteFailed { version: i64 },
    CommitFailed { version: i64 },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionReadFailed => formatter.write_str("schema version read failed"),
            Self::InvalidVersion { found } => {
                write!(formatter, "invalid schema version: {found}")
            }
            Self::UnsupportedVersion { found, latest } => {
                write!(formatter, "unsupported schema version: {found} > {latest}")
            }
            Self::MissingMigration { expected } => {
                write!(formatter, "missing migration version: {expected}")
            }
            Self::ForeignKeysEnableFailed => {
                formatter.write_str("foreign key enforcement unavailable")
            }
            Self::TransactionBeginFailed { version } => {
                write!(formatter, "migration transaction begin failed: {version}")
            }
            Self::MigrationApplyFailed { version } => {
                write!(formatter, "migration apply failed: {version}")
            }
            Self::SchemaMismatch { version } => {
                write!(formatter, "schema verification failed: {version}")
            }
            Self::VersionWriteFailed { version } => {
                write!(formatter, "schema version write failed: {version}")
            }
            Self::CommitFailed { version } => {
                write!(formatter, "migration commit failed: {version}")
            }
        }
    }
}

impl Error for MigrationError {}

/// Apply every pending migration and verify the resulting schema.
///
/// A mutable connection is intentional: rusqlite's checked transaction API
/// uses `&mut Connection` to make nested transactions impossible at compile
/// time. The dedicated vault worker is the sole caller and connection owner.
pub(crate) fn run_migrations(connection: &mut Connection) -> Result<(), MigrationError> {
    // vec0 DDL in v2 requires the statically linked extension on this handle.
    crate::db::sqlite_vec_ext::activate_sqlite_vec(connection)
        .map_err(|_| MigrationError::MigrationApplyFailed { version: 2 })?;

    enable_and_verify_foreign_keys(connection)?;

    let mut current = read_user_version(connection)?;
    if current < 0 {
        return Err(MigrationError::InvalidVersion { found: current });
    }
    if current > LATEST_SCHEMA_VERSION {
        return Err(MigrationError::UnsupportedVersion {
            found: current,
            latest: LATEST_SCHEMA_VERSION,
        });
    }

    while current < LATEST_SCHEMA_VERSION {
        let expected = current + 1;
        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.version == expected)
            .ok_or(MigrationError::MissingMigration { expected })?;

        // vec0 creates shadow tables and may not participate cleanly in an
        // IMMEDIATE transaction; apply that DDL outside the checked txn, then
        // stamp user_version in a short Immediate txn after verify.
        if migration.version == 2 {
            connection.execute_batch(migration.sql).map_err(|_| {
                MigrationError::MigrationApplyFailed {
                    version: migration.version,
                }
            })?;
            (migration.verify)(connection)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| MigrationError::TransactionBeginFailed {
                    version: migration.version,
                })?;
            transaction
                .pragma_update(None, "user_version", migration.version)
                .map_err(|_| MigrationError::VersionWriteFailed {
                    version: migration.version,
                })?;
            let written = read_user_version(&transaction)?;
            if written != migration.version {
                return Err(MigrationError::VersionWriteFailed {
                    version: migration.version,
                });
            }
            transaction
                .commit()
                .map_err(|_| MigrationError::CommitFailed {
                    version: migration.version,
                })?;
            current = migration.version;
            continue;
        }

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| MigrationError::TransactionBeginFailed {
                version: migration.version,
            })?;

        transaction.execute_batch(migration.sql).map_err(|_| {
            MigrationError::MigrationApplyFailed {
                version: migration.version,
            }
        })?;
        (migration.verify)(&transaction)?;
        transaction
            .pragma_update(None, "user_version", migration.version)
            .map_err(|_| MigrationError::VersionWriteFailed {
                version: migration.version,
            })?;

        let written = read_user_version(&transaction)?;
        if written != migration.version {
            return Err(MigrationError::VersionWriteFailed {
                version: migration.version,
            });
        }

        transaction
            .commit()
            .map_err(|_| MigrationError::CommitFailed {
                version: migration.version,
            })?;
        current = migration.version;
    }

    let latest = MIGRATIONS
        .last()
        .filter(|migration| migration.version == LATEST_SCHEMA_VERSION)
        .ok_or(MigrationError::MissingMigration {
            expected: LATEST_SCHEMA_VERSION,
        })?;
    (latest.verify)(connection)
}

fn read_user_version(connection: &Connection) -> Result<i64, MigrationError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| MigrationError::VersionReadFailed)
}

fn enable_and_verify_foreign_keys(connection: &Connection) -> Result<(), MigrationError> {
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|_| MigrationError::ForeignKeysEnableFailed)?;
    let enabled: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|_| MigrationError::ForeignKeysEnableFailed)?;
    if enabled == 1 {
        Ok(())
    } else {
        Err(MigrationError::ForeignKeysEnableFailed)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ColumnShape {
    name: String,
    data_type: String,
    not_null: bool,
    primary_key_position: i64,
}

fn verify_v1_schema(connection: &Connection) -> Result<(), MigrationError> {
    let expected_chats = [
        ("id", "TEXT", true, 1),
        ("title", "TEXT", true, 0),
        ("created_at", "INTEGER", true, 0),
    ];
    let expected_messages = [
        ("id", "TEXT", true, 1),
        ("chat_id", "TEXT", true, 0),
        ("role", "TEXT", true, 0),
        ("content", "TEXT", true, 0),
        ("timestamp", "INTEGER", true, 0),
    ];

    let chats = read_columns(connection, READ_CHATS_COLUMNS_SQL)?;
    let messages = read_columns(connection, READ_MESSAGES_COLUMNS_SQL)?;
    if !columns_match(&chats, &expected_chats) || !columns_match(&messages, &expected_messages) {
        return Err(MigrationError::SchemaMismatch { version: 1 });
    }

    let mut foreign_key_statement = connection
        .prepare(READ_MESSAGES_FOREIGN_KEYS_SQL)
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    let foreign_keys = foreign_key_statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?
        .collect::<Result<Vec<(String, String, String, String)>, _>>()
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    if foreign_keys
        != [(
            "chats".into(),
            "chat_id".into(),
            "id".into(),
            "CASCADE".into(),
        )]
    {
        return Err(MigrationError::SchemaMismatch { version: 1 });
    }

    let index_count: i64 = connection
        .query_row(READ_MESSAGES_INDEX_SQL, [], |row| row.get(0))
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    if index_count != 1 {
        return Err(MigrationError::SchemaMismatch { version: 1 });
    }
    let mut index_statement = connection
        .prepare(READ_MESSAGES_INDEX_COLUMNS_SQL)
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    let index_columns = index_statement
        .query_map([], |row| row.get(0))
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    if index_columns != ["chat_id", "timestamp", "id"] {
        return Err(MigrationError::SchemaMismatch { version: 1 });
    }
    let (unique, partial): (i64, i64) = connection
        .query_row(READ_MESSAGES_INDEX_PROPERTIES_SQL, [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    if unique != 0 || partial != 0 {
        return Err(MigrationError::SchemaMismatch { version: 1 });
    }

    Ok(())
}

fn verify_v2_schema(connection: &Connection) -> Result<(), MigrationError> {
    // v2 is additive: chats/messages must still match v1.
    verify_v1_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 2 })?;

    crate::db::sqlite_vec_ext::verify_vec_extension(connection)
        .map_err(|_| MigrationError::SchemaMismatch { version: 2 })?;

    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'knowledge_chunks'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| MigrationError::SchemaMismatch { version: 2 })?;
    let normalized = sql.to_ascii_lowercase();
    if !(normalized.contains("vec0")
        && normalized.contains("embedding")
        && normalized.contains("float[384]")
        && normalized.contains("text_content")
        && normalized.contains("created_at")
        && normalized.contains(" id "))
    {
        return Err(MigrationError::SchemaMismatch { version: 2 });
    }

    // Debug assert keeps the constant and DDL string in lockstep.
    debug_assert_eq!(KNOWLEDGE_EMBEDDING_DIMS, 384);

    Ok(())
}

fn verify_v6_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v5_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 6 })?;

    let cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('interview_sessions') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 6 })?;
    // Base v6 columns must be present; v10 may append artifact columns.
    if !columns_include(
        &cols,
        &[
            ("id", "TEXT", true, 1),
            ("updated_at", "INTEGER", true, 0),
            ("stage", "TEXT", true, 0),
            ("status", "TEXT", true, 0),
            ("payload_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 6 });
    }
    Ok(())
}

fn verify_v7_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v6_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 7 })?;

    let cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('distortion_tags') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 7 })?;
    if !columns_match(
        &cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("category", "TEXT", true, 0),
            ("snippet", "TEXT", true, 0),
            ("confidence_score", "REAL", true, 0),
            ("source_kind", "TEXT", true, 0),
            ("source_id", "TEXT", true, 0),
            ("run_id", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 7 });
    }
    Ok(())
}

fn verify_v8_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v7_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 8 })?;

    let purchase_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('purchases') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 8 })?;
    if !columns_match(
        &purchase_cols,
        &[
            ("id", "TEXT", true, 1),
            ("occurred_at", "INTEGER", true, 0),
            ("merchant_norm", "TEXT", true, 0),
            ("total_amount", "INTEGER", true, 0),
            ("tax", "INTEGER", true, 0),
            ("verified", "INTEGER", true, 0),
            ("r_at_decision", "REAL", true, 0),
            ("active_distortions_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 8 });
    }

    let line_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('purchase_lines') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 8 })?;
    if !columns_match(
        &line_cols,
        &[
            ("id", "TEXT", true, 1),
            ("purchase_id", "TEXT", true, 0),
            ("item_name", "TEXT", true, 0),
            ("unit_price", "INTEGER", true, 0),
            ("qty", "INTEGER", true, 0),
            ("amount", "INTEGER", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 8 });
    }
    Ok(())
}

fn verify_v9_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v8_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 9 })?;

    let cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('commitments') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 9 })?;
    if !columns_match(
        &cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("condition_json", "TEXT", true, 0),
            ("action_type", "TEXT", true, 0),
            ("custom_prompt", "TEXT", true, 0),
            ("delay_seconds", "INTEGER", true, 0),
            ("source_relation_id", "TEXT", true, 0),
            ("enabled", "INTEGER", true, 0),
            ("origin", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 9 });
    }
    Ok(())
}

fn verify_v10_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v9_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 10 })?;

    let cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('interview_sessions') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 10 })?;
    if !columns_match(
        &cols,
        &[
            ("id", "TEXT", true, 1),
            ("updated_at", "INTEGER", true, 0),
            ("stage", "TEXT", true, 0),
            ("status", "TEXT", true, 0),
            ("payload_json", "TEXT", true, 0),
            ("artifact_json", "TEXT", true, 0),
            ("artifact_fingerprint", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 10 });
    }
    Ok(())
}

fn verify_v11_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v10_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 11 })?;

    let fact_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('company_fact_cells') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 11 })?;
    if !columns_match(
        &fact_cols,
        &[
            ("subject_key", "TEXT", true, 1),
            ("field", "TEXT", true, 2),
            ("value", "TEXT", true, 0),
            ("origin", "TEXT", true, 0),
            ("storage", "TEXT", true, 0),
            ("doc_id", "TEXT", false, 0),
            ("submitted_at", "TEXT", false, 0),
            ("fetched_at", "INTEGER", false, 0),
            ("revision", "INTEGER", true, 0),
            ("schema_version", "INTEGER", true, 0),
            ("updated_at", "INTEGER", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 11 });
    }

    let coverage_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('edinet_list_coverage') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 11 })?;
    if !columns_match(
        &coverage_cols,
        &[
            ("subject_key", "TEXT", true, 1),
            ("date", "TEXT", true, 2),
            ("status", "TEXT", true, 0),
            ("fetched_at", "INTEGER", false, 0),
            ("process_date_time", "TEXT", false, 0),
            ("error_class", "TEXT", false, 0),
            ("revalidate_after", "INTEGER", false, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 11 });
    }

    let cursor_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('edinet_scan_cursor') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 11 })?;
    if !columns_match(
        &cursor_cols,
        &[
            ("subject_key", "TEXT", true, 1),
            ("anchor_date", "TEXT", true, 0),
            ("window_days_back", "INTEGER", true, 0),
            ("next_date", "TEXT", false, 0),
            ("status", "TEXT", true, 0),
            ("updated_at", "INTEGER", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 11 });
    }

    let filing_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('company_filing_index') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 11 })?;
    if !columns_match(
        &filing_cols,
        &[
            ("subject_key", "TEXT", true, 1),
            ("doc_id", "TEXT", true, 2),
            ("list_date", "TEXT", true, 0),
            ("edinet_code", "TEXT", false, 0),
            ("filer_name", "TEXT", false, 0),
            ("ordinance_code", "TEXT", false, 0),
            ("form_code", "TEXT", false, 0),
            ("doc_type_code", "TEXT", false, 0),
            ("period_start", "TEXT", false, 0),
            ("period_end", "TEXT", false, 0),
            ("submit_date_time", "TEXT", false, 0),
            ("parent_doc_id", "TEXT", false, 0),
            ("withdrawal_status", "TEXT", false, 0),
            ("doc_info_edit_status", "TEXT", false, 0),
            ("disclosure_status", "TEXT", false, 0),
            ("xbrl_flag", "TEXT", false, 0),
            ("csv_flag", "TEXT", false, 0),
            ("legal_status", "TEXT", false, 0),
            ("doc_description", "TEXT", false, 0),
            ("sec_code", "TEXT", false, 0),
            ("process_date_time", "TEXT", false, 0),
            ("indexed_at", "INTEGER", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 11 });
    }

    for (table_sql, expected) in [
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('subject_alias_candidate') ORDER BY cid;",
            &[
                ("alias_key", "TEXT", true, 1),
                ("canonical_key", "TEXT", true, 2),
                ("created_at", "INTEGER", true, 0),
            ][..],
        ),
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('company_doc_pointers') ORDER BY cid;",
            &[
                ("subject_key", "TEXT", true, 1),
                ("latest_selected_doc", "TEXT", false, 0),
                ("latest_selected_rev", "INTEGER", true, 0),
                ("rag_ready_doc", "TEXT", false, 0),
                ("rag_ready_rev", "INTEGER", true, 0),
                ("updated_at", "INTEGER", true, 0),
            ][..],
        ),
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('knowledge_chunk_meta') ORDER BY cid;",
            &[
                ("chunk_id", "TEXT", true, 1),
                ("source_id", "TEXT", true, 0),
                ("subject_key", "TEXT", false, 0),
                ("doc_id", "TEXT", false, 0),
                ("revision", "INTEGER", false, 0),
                ("section_id", "TEXT", false, 0),
                ("concept", "TEXT", false, 0),
                ("period_start", "TEXT", false, 0),
                ("period_end", "TEXT", false, 0),
                ("unit", "TEXT", false, 0),
                ("submitted_at", "TEXT", false, 0),
                ("truncated", "INTEGER", true, 0),
                ("created_at", "INTEGER", true, 0),
            ][..],
        ),
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('edinet_evidence_pending') ORDER BY cid;",
            &[
                ("id", "TEXT", true, 1),
                ("subject_key", "TEXT", true, 0),
                ("revision", "INTEGER", true, 0),
                ("doc_id", "TEXT", true, 0),
                ("section_id", "TEXT", false, 0),
                ("concept", "TEXT", false, 0),
                ("text", "TEXT", true, 0),
                ("state", "TEXT", true, 0),
                ("claim_id", "TEXT", false, 0),
                ("claimed_at", "INTEGER", false, 0),
                ("claim_deadline", "INTEGER", false, 0),
                ("retry_count", "INTEGER", true, 0),
                ("submitted_at", "TEXT", false, 0),
                ("period_start", "TEXT", false, 0),
                ("period_end", "TEXT", false, 0),
                ("unit", "TEXT", false, 0),
                ("truncated", "INTEGER", true, 0),
                ("created_at", "INTEGER", true, 0),
                ("updated_at", "INTEGER", true, 0),
            ][..],
        ),
    ] {
        let cols = read_columns(connection, table_sql)
            .map_err(|_| MigrationError::SchemaMismatch { version: 11 })?;
        if !columns_match(&cols, expected) {
            return Err(MigrationError::SchemaMismatch { version: 11 });
        }
    }
    Ok(())
}

/// Exact column contracts, matching the v11 discipline: extra columns are a
/// mismatch, not a tolerated addition. These tables are the substrate of a
/// measurement instrument, and a stray column is exactly how an unreviewed
/// free-text field would arrive (SPEC §9.2 forbids one outright).
fn verify_v12_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v11_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 12 })?;

    for (table_sql, expected) in [
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('blackbox_campaigns') ORDER BY cid;",
            &[
                ("campaign_fingerprint", "BLOB", true, 1),
                ("schema_version", "TEXT", true, 0),
                ("scenario_id", "INTEGER", true, 0),
                ("difficulty", "INTEGER", true, 0),
                ("campaign_index", "INTEGER", true, 0),
                ("created_date", "TEXT", true, 0),
                ("started_at", "INTEGER", true, 0),
                ("updated_at", "INTEGER", true, 0),
            ][..],
        ),
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('blackbox_decisions') ORDER BY cid;",
            &[
                ("campaign_fingerprint", "BLOB", true, 1),
                ("seq", "INTEGER", true, 2),
                ("tick", "INTEGER", true, 0),
                ("phase", "INTEGER", true, 0),
                ("action_kind", "INTEGER", true, 0),
                ("action_json", "TEXT", true, 0),
                ("stimulus_seq", "INTEGER", false, 0),
                ("stimulus_kind", "INTEGER", false, 0),
                ("stimulus_digest", "BLOB", false, 0),
                ("latency_ms", "INTEGER", false, 0),
                ("forced_default", "INTEGER", true, 0),
                ("state_digest", "BLOB", true, 0),
                ("channel", "TEXT", true, 0),
                ("persisted_at", "INTEGER", true, 0),
            ][..],
        ),
        (
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('blackbox_stimuli') ORDER BY cid;",
            &[
                ("campaign_fingerprint", "BLOB", true, 1),
                ("stimulus_seq", "INTEGER", true, 2),
                ("tick", "INTEGER", true, 0),
                ("kind", "INTEGER", true, 0),
                ("params_json", "TEXT", true, 0),
                ("params_digest", "BLOB", true, 0),
                ("persisted_at", "INTEGER", true, 0),
            ][..],
        ),
    ] {
        let cols = read_columns(connection, table_sql)
            .map_err(|_| MigrationError::SchemaMismatch { version: 12 })?;
        if !columns_match(&cols, expected) {
            return Err(MigrationError::SchemaMismatch { version: 12 });
        }
    }
    Ok(())
}

fn verify_v5_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v4_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 5 })?;

    let twin_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('twin_scenario_runs') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 5 })?;
    if !columns_match(
        &twin_cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("schema_version", "TEXT", true, 0),
            ("gate_passed", "INTEGER", true, 0),
            ("bss", "REAL", true, 0),
            ("payload_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 5 });
    }

    let oracle_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('oracle_payload_runs') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 5 })?;
    if !columns_match(
        &oracle_cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("schema_version", "TEXT", true, 0),
            ("gate_passed", "INTEGER", true, 0),
            ("payload_json", "TEXT", true, 0),
            ("provenance_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 5 });
    }

    Ok(())
}

fn verify_v4_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v3_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 4 })?;

    let pulse_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('interaction_pulse_runs') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 4 })?;
    if !columns_match(
        &pulse_cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("affinity_score", "INTEGER", false, 0),
            ("interaction_tendency", "TEXT", true, 0),
            ("next_best_action", "TEXT", true, 0),
            ("metrics_json", "TEXT", true, 0),
            ("input_hash", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 4 });
    }

    let rasch_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('rasch_filter_runs') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 4 })?;
    if !columns_match(
        &rasch_cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("artifact_sha256", "TEXT", true, 0),
            ("posterior_json", "TEXT", true, 0),
            ("excluded_json", "TEXT", true, 0),
            ("last_selection_json", "TEXT", false, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 4 });
    }

    let probe_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('probe_store') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 4 })?;
    if !columns_match(
        &probe_cols,
        &[
            ("id", "TEXT", true, 1),
            ("updated_at", "INTEGER", true, 0),
            ("payload_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 4 });
    }

    Ok(())
}

fn verify_v3_schema(connection: &Connection) -> Result<(), MigrationError> {
    verify_v2_schema(connection).map_err(|_| MigrationError::SchemaMismatch { version: 3 })?;

    let gap_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('gap_analysis_runs') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 3 })?;
    if !columns_match(
        &gap_cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("schema_version", "TEXT", true, 0),
            ("data_sufficiency", "REAL", true, 0),
            ("payload_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 3 });
    }

    let tensor_cols = read_columns(
        connection,
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info('tensor_profiles') ORDER BY cid;",
    )
    .map_err(|_| MigrationError::SchemaMismatch { version: 3 })?;
    if !columns_match(
        &tensor_cols,
        &[
            ("id", "TEXT", true, 1),
            ("created_at", "INTEGER", true, 0),
            ("schema_version", "TEXT", true, 0),
            ("model_hash", "TEXT", true, 0),
            ("payload_json", "TEXT", true, 0),
        ],
    ) {
        return Err(MigrationError::SchemaMismatch { version: 3 });
    }

    Ok(())
}

fn read_columns(
    connection: &Connection,
    query: &'static str,
) -> Result<Vec<ColumnShape>, MigrationError> {
    let mut statement = connection
        .prepare(query)
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;
    let rows = statement
        .query_map([], |row| {
            Ok(ColumnShape {
                name: row.get(0)?,
                data_type: row.get(1)?,
                not_null: row.get::<_, i64>(2)? == 1,
                primary_key_position: row.get(3)?,
            })
        })
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| MigrationError::SchemaMismatch { version: 1 })
}

fn columns_match(actual: &[ColumnShape], expected: &[(&str, &str, bool, i64)]) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.name == expected.0
                && actual.data_type.eq_ignore_ascii_case(expected.1)
                && actual.not_null == expected.2
                && actual.primary_key_position == expected.3
        })
}

/// True when every required column is present (extra columns allowed — additive migrations).
fn columns_include(actual: &[ColumnShape], required: &[(&str, &str, bool, i64)]) -> bool {
    required.iter().all(|exp| {
        actual.iter().any(|col| {
            col.name == exp.0
                && col.data_type.eq_ignore_ascii_case(exp.1)
                && col.not_null == exp.2
                && col.primary_key_position == exp.3
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_versions_are_contiguous_unique_and_end_at_latest() {
        assert_eq!(MIGRATIONS.len() as i64, LATEST_SCHEMA_VERSION);
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version, index as i64 + 1);
        }
        assert_eq!(
            MIGRATIONS.last().map(|migration| migration.version),
            Some(LATEST_SCHEMA_VERSION)
        );
    }

    #[test]
    fn creates_and_versions_v1_schema() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;

        assert_eq!(read_user_version(&connection)?, LATEST_SCHEMA_VERSION);
        verify_v1_schema(&connection)?;
        verify_v2_schema(&connection)?;

        connection.execute(
            "INSERT INTO chats(id, title, created_at) VALUES (?1, ?2, ?3)",
            ("chat-1", "Test", 1_i64),
        )?;
        connection.execute(
            "INSERT INTO messages(id, chat_id, role, content, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            ("message-1", "chat-1", "user", "hello", 2_i64),
        )?;

        Ok(())
    }

    #[test]
    fn v12_blackbox_lane_is_exact_append_only_and_pii_free(
    ) -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        assert_eq!(read_user_version(&connection)?, 12);

        // The decision log is keyed so a replayed flush cannot duplicate
        // history: re-inserting the same (campaign, seq) must conflict.
        connection.execute(
            "INSERT INTO blackbox_campaigns VALUES (X'AA', 'blackbox_log.v1', 3, 0, 1, '2026-07-27', 1, 1);",
            [],
        )?;
        let insert = "INSERT INTO blackbox_decisions VALUES \
             (X'AA', 0, 1, 1, 12, '{}', NULL, NULL, NULL, NULL, 0, X'BB', 'blackbox_sim', 1);";
        connection.execute(insert, [])?;
        assert!(
            connection.execute(insert, []).is_err(),
            "the same decision must not be storable twice"
        );

        // A decision cannot exist without its campaign.
        assert!(
            connection
                .execute(
                    "INSERT INTO blackbox_decisions VALUES \
                     (X'CC', 0, 1, 1, 12, '{}', NULL, NULL, NULL, NULL, 0, X'BB', 'blackbox_sim', 1);",
                    [],
                )
                .is_err(),
            "orphan decisions must be refused"
        );

        // No free-text columns beyond the two explicitly-typed JSON payloads
        // and the channel tag (SPEC §9.2: PII cannot exist by construction).
        let cols = read_columns(
            &connection,
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('blackbox_decisions') ORDER BY cid;",
        )?;
        let text_columns: Vec<&str> = cols
            .iter()
            .filter(|c| c.data_type == "TEXT")
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            text_columns,
            vec!["action_json", "channel"],
            "a new free-text column appeared in the record lane"
        );

        // An extra column is a mismatch, not a tolerated addition.
        connection.execute("ALTER TABLE blackbox_stimuli ADD COLUMN note TEXT;", [])?;
        assert!(matches!(
            verify_v12_schema(&connection),
            Err(MigrationError::SchemaMismatch { version: 12 })
        ));
        Ok(())
    }

    #[test]
    fn v11_frozen_tables_have_exact_column_contracts() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;

        for (query, expected_names) in [
            (
                "SELECT name, type, \"notnull\", pk FROM pragma_table_info('subject_alias_candidate') ORDER BY cid;",
                &["alias_key", "canonical_key", "created_at"][..],
            ),
            (
                "SELECT name, type, \"notnull\", pk FROM pragma_table_info('company_doc_pointers') ORDER BY cid;",
                &[
                    "subject_key",
                    "latest_selected_doc",
                    "latest_selected_rev",
                    "rag_ready_doc",
                    "rag_ready_rev",
                    "updated_at",
                ][..],
            ),
            (
                "SELECT name, type, \"notnull\", pk FROM pragma_table_info('knowledge_chunk_meta') ORDER BY cid;",
                &[
                    "chunk_id",
                    "source_id",
                    "subject_key",
                    "doc_id",
                    "revision",
                    "section_id",
                    "concept",
                    "period_start",
                    "period_end",
                    "unit",
                    "submitted_at",
                    "truncated",
                    "created_at",
                ][..],
            ),
            (
                "SELECT name, type, \"notnull\", pk FROM pragma_table_info('edinet_evidence_pending') ORDER BY cid;",
                &[
                    "id",
                    "subject_key",
                    "revision",
                    "doc_id",
                    "section_id",
                    "concept",
                    "text",
                    "state",
                    "claim_id",
                    "claimed_at",
                    "claim_deadline",
                    "retry_count",
                    "submitted_at",
                    "period_start",
                    "period_end",
                    "unit",
                    "truncated",
                    "created_at",
                    "updated_at",
                ][..],
            ),
        ] {
            let actual = read_columns(&connection, query)?;
            let names = actual
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>();
            assert_eq!(names, expected_names);
        }

        connection.execute_batch("ALTER TABLE knowledge_chunk_meta ADD COLUMN unexpected TEXT;")?;
        assert_eq!(
            verify_v11_schema(&connection),
            Err(MigrationError::SchemaMismatch { version: 11 })
        );
        Ok(())
    }

    #[test]
    fn rerun_is_idempotent_and_preserves_rows() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        connection.execute(
            "INSERT INTO chats(id, title, created_at) VALUES (?1, ?2, ?3)",
            ("chat-1", "Keep", 1_i64),
        )?;

        run_migrations(&mut connection)?;
        let count: i64 =
            connection.query_row("SELECT count(*) FROM chats", [], |row| row.get(0))?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn rejects_newer_schema_without_mutation() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 1)?;

        let error = run_migrations(&mut connection)
            .err()
            .ok_or("newer schema unexpectedly accepted")?;
        assert_eq!(
            error,
            MigrationError::UnsupportedVersion {
                found: LATEST_SCHEMA_VERSION + 1,
                latest: LATEST_SCHEMA_VERSION,
            }
        );
        assert_eq!(read_user_version(&connection)?, LATEST_SCHEMA_VERSION + 1);
        Ok(())
    }

    #[test]
    fn incompatible_existing_table_rolls_back_entire_migration() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch("CREATE TABLE chats(id INTEGER PRIMARY KEY);")?;

        let error = run_migrations(&mut connection)
            .err()
            .ok_or("incompatible schema unexpectedly accepted")?;
        assert_eq!(error, MigrationError::SchemaMismatch { version: 1 });
        assert_eq!(read_user_version(&connection)?, 0);

        let messages_count: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'messages'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(messages_count, 0);
        Ok(())
    }

    #[test]
    fn enforces_messages_chat_foreign_key() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;

        let result = connection.execute(
            "INSERT INTO messages(id, chat_id, role, content, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            ("message-1", "missing-chat", "user", "hello", 2_i64),
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn rejects_same_name_index_with_wrong_columns() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        connection.execute_batch(
            "PRAGMA user_version = 0; \
             DROP INDEX idx_messages_chat_timestamp; \
             CREATE INDEX idx_messages_chat_timestamp ON messages(timestamp);",
        )?;

        let error = run_migrations(&mut connection)
            .err()
            .ok_or("wrong index shape unexpectedly accepted")?;
        assert_eq!(error, MigrationError::SchemaMismatch { version: 1 });
        assert_eq!(read_user_version(&connection)?, 0);
        Ok(())
    }

    #[test]
    fn rejects_same_name_unique_index() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        connection.execute_batch(
            "PRAGMA user_version = 0; \
             DROP INDEX idx_messages_chat_timestamp; \
             CREATE UNIQUE INDEX idx_messages_chat_timestamp \
             ON messages(chat_id, timestamp, id);",
        )?;

        let error = run_migrations(&mut connection)
            .err()
            .ok_or("unique index unexpectedly accepted")?;
        assert_eq!(error, MigrationError::SchemaMismatch { version: 1 });
        assert_eq!(read_user_version(&connection)?, 0);
        Ok(())
    }

    #[test]
    fn rejects_same_name_partial_index() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        connection.execute_batch(
            "PRAGMA user_version = 0; \
             DROP INDEX idx_messages_chat_timestamp; \
             CREATE INDEX idx_messages_chat_timestamp \
             ON messages(chat_id, timestamp, id) WHERE role = 'user';",
        )?;

        let error = run_migrations(&mut connection)
            .err()
            .ok_or("partial index unexpectedly accepted")?;
        assert_eq!(error, MigrationError::SchemaMismatch { version: 1 });
        assert_eq!(read_user_version(&connection)?, 0);
        Ok(())
    }
}
