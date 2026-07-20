//! Immutable, sequential SQLCipher schema migrations.
//!
//! Every migration runs in its own IMMEDIATE transaction. The schema is
//! verified before `user_version` is advanced, so an incompatible pre-existing
//! table cannot be silently accepted by `CREATE TABLE IF NOT EXISTS`.

use std::{error::Error, fmt};

use rusqlite::{Connection, TransactionBehavior};

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 4;

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
