//! Closed SQL repository for the vault worker.
//!
//! This module accepts only typed domain operations. It never accepts SQL,
//! PRAGMA names, table names, or column names from callers. Write functions
//! operate on worker-owned IMMEDIATE transactions; read functions borrow the
//! worker-owned connection.

use std::{error::Error, fmt};

use rusqlite::{ffi, params, Connection, OptionalExtension, Row, Transaction};

use super::sqlite_error::is_data_protection_error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatCreate {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatRecord {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessageAppend {
    pub(crate) id: String,
    pub(crate) chat_id: String,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessageRecord {
    pub(crate) id: String,
    pub(crate) chat_id: String,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessageCursor {
    pub(crate) timestamp: i64,
    pub(crate) id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepositoryError {
    NotFound,
    Conflict,
    IdentityAmbiguous,
    StorageFailed,
    /// OS-level storage access denial (e.g. iOS Data Protection sealing the
    /// database file while the device is locked). The worker treats this as a
    /// fail-closed self-lock trigger, not a generic storage failure.
    DataProtection,
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "repository record not found",
            Self::Conflict => "repository record conflict",
            Self::IdentityAmbiguous => "repository identity ambiguous",
            Self::StorageFailed => "repository storage failed",
            Self::DataProtection => "repository storage access denied by the OS",
        };
        formatter.write_str(message)
    }
}

/// Classify a storage error, preferring the fail-closed `DataProtection` code.
pub(crate) fn map_storage_error(error: rusqlite::Error) -> RepositoryError {
    if is_data_protection_error(&error) {
        RepositoryError::DataProtection
    } else {
        RepositoryError::StorageFailed
    }
}

impl Error for RepositoryError {}

pub(crate) fn chat_create(
    transaction: &Transaction<'_>,
    input: &ChatCreate,
) -> Result<ChatRecord, RepositoryError> {
    if let Some(existing) = find_chat(transaction, &input.id)? {
        return if existing.title == input.title && existing.created_at == input.created_at {
            Ok(existing)
        } else {
            Err(RepositoryError::Conflict)
        };
    }

    transaction
        .execute(
            "INSERT INTO chats(id, title, created_at) VALUES (?1, ?2, ?3)",
            params![input.id, input.title, input.created_at],
        )
        .map_err(map_write_error)?;

    Ok(ChatRecord {
        id: input.id.clone(),
        title: input.title.clone(),
        created_at: input.created_at,
    })
}

pub(crate) fn chat_delete(transaction: &Transaction<'_>, id: &str) -> Result<(), RepositoryError> {
    let changed = transaction
        .execute("DELETE FROM chats WHERE id = ?1", params![id])
        .map_err(map_storage_error)?;
    if changed == 0 {
        Err(RepositoryError::NotFound)
    } else {
        Ok(())
    }
}

pub(crate) fn chats_list(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<ChatRecord>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT id, title, created_at FROM chats \
             ORDER BY created_at DESC, id ASC LIMIT ?1",
        )
        .map_err(map_storage_error)?;
    let rows = statement
        .query_map(params![i64::from(limit)], read_chat_row)
        .map_err(map_storage_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(map_storage_error)
}

pub(crate) fn message_append(
    transaction: &Transaction<'_>,
    input: &MessageAppend,
) -> Result<MessageRecord, RepositoryError> {
    if let Some(existing) = find_message(transaction, &input.id)? {
        return if existing.chat_id == input.chat_id
            && existing.role == input.role
            && existing.content == input.content
            && existing.timestamp == input.timestamp
        {
            Ok(existing)
        } else {
            Err(RepositoryError::Conflict)
        };
    }

    let parent_count: i64 = transaction
        .query_row(
            "SELECT count(*) FROM chats WHERE id = ?1",
            params![input.chat_id],
            |row| row.get(0),
        )
        .map_err(map_storage_error)?;
    if parent_count != 1 {
        return Err(RepositoryError::NotFound);
    }

    transaction
        .execute(
            "INSERT INTO messages(id, chat_id, role, content, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                input.id,
                input.chat_id,
                input.role,
                input.content,
                input.timestamp
            ],
        )
        .map_err(map_write_error)?;

    Ok(MessageRecord {
        id: input.id.clone(),
        chat_id: input.chat_id.clone(),
        role: input.role.clone(),
        content: input.content.clone(),
        timestamp: input.timestamp,
    })
}

pub(crate) fn messages_list(
    connection: &Connection,
    chat_id: &str,
    cursor: Option<&MessageCursor>,
    limit: u32,
) -> Result<Vec<MessageRecord>, RepositoryError> {
    if let Some(cursor) = cursor {
        let mut statement = connection
            .prepare(
                "SELECT id, chat_id, role, content, timestamp FROM messages \
                 WHERE chat_id = ?1 AND (timestamp, id) > (?2, ?3) \
                 ORDER BY timestamp ASC, id ASC LIMIT ?4",
            )
            .map_err(map_storage_error)?;
        let rows = statement
            .query_map(
                params![chat_id, cursor.timestamp, cursor.id, i64::from(limit)],
                read_message_row,
            )
            .map_err(map_storage_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_storage_error)
    } else {
        let mut statement = connection
            .prepare(
                "SELECT id, chat_id, role, content, timestamp FROM messages \
                 WHERE chat_id = ?1 ORDER BY timestamp ASC, id ASC LIMIT ?2",
            )
            .map_err(map_storage_error)?;
        let rows = statement
            .query_map(params![chat_id, i64::from(limit)], read_message_row)
            .map_err(map_storage_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_storage_error)
    }
}

fn find_chat(connection: &Connection, id: &str) -> Result<Option<ChatRecord>, RepositoryError> {
    connection
        .query_row(
            "SELECT id, title, created_at FROM chats WHERE id = ?1",
            params![id],
            read_chat_row,
        )
        .optional()
        .map_err(map_storage_error)
}

fn find_message(
    connection: &Connection,
    id: &str,
) -> Result<Option<MessageRecord>, RepositoryError> {
    connection
        .query_row(
            "SELECT id, chat_id, role, content, timestamp FROM messages WHERE id = ?1",
            params![id],
            read_message_row,
        )
        .optional()
        .map_err(map_storage_error)
}

fn read_chat_row(row: &Row<'_>) -> rusqlite::Result<ChatRecord> {
    Ok(ChatRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
    })
}

fn read_message_row(row: &Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        timestamp: row.get(4)?,
    })
}

fn map_write_error(error: rusqlite::Error) -> RepositoryError {
    // OS-level access denial takes precedence: it is a fail-closed self-lock
    // signal, not a constraint outcome.
    if is_data_protection_error(&error) {
        return RepositoryError::DataProtection;
    }
    match error {
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
        {
            RepositoryError::NotFound
        }
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                || code.extended_code == ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            RepositoryError::Conflict
        }
        _ => RepositoryError::StorageFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use rusqlite::TransactionBehavior;

    const CHAT_1: &str = "00000000-0000-4000-8000-000000000001";
    const CHAT_2: &str = "00000000-0000-4000-8000-000000000002";
    const CHAT_3: &str = "00000000-0000-4000-8000-000000000003";
    const MESSAGE_1: &str = "10000000-0000-4000-8000-000000000001";
    const MESSAGE_2: &str = "10000000-0000-4000-8000-000000000002";
    const MESSAGE_3: &str = "10000000-0000-4000-8000-000000000003";

    fn migrated_connection() -> Result<Connection, Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        Ok(connection)
    }

    fn sqlite_failure(primary_code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(ffi::Error::new(primary_code), None)
    }

    #[test]
    fn maps_access_denied_before_constraint_or_storage() {
        assert_eq!(
            map_storage_error(sqlite_failure(ffi::SQLITE_IOERR)),
            RepositoryError::DataProtection
        );
        assert_eq!(
            map_storage_error(sqlite_failure(ffi::SQLITE_BUSY)),
            RepositoryError::StorageFailed
        );
        assert_eq!(
            map_write_error(sqlite_failure(ffi::SQLITE_CANTOPEN)),
            RepositoryError::DataProtection
        );
    }

    fn create_chat(
        connection: &mut Connection,
        id: &str,
        title: &str,
        created_at: i64,
    ) -> Result<ChatRecord, Box<dyn Error>> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = chat_create(
            &transaction,
            &ChatCreate {
                id: id.to_string(),
                title: title.to_string(),
                created_at,
            },
        )?;
        transaction.commit()?;
        Ok(record)
    }

    fn append_message(
        connection: &mut Connection,
        id: &str,
        chat_id: &str,
        role: &str,
        content: &str,
        timestamp: i64,
    ) -> Result<MessageRecord, Box<dyn Error>> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = message_append(
            &transaction,
            &MessageAppend {
                id: id.to_string(),
                chat_id: chat_id.to_string(),
                role: role.to_string(),
                content: content.to_string(),
                timestamp,
            },
        )?;
        transaction.commit()?;
        Ok(record)
    }

    #[test]
    fn create_and_list_round_trip_in_required_order() -> Result<(), Box<dyn Error>> {
        let mut connection = migrated_connection()?;
        create_chat(&mut connection, CHAT_1, "Older", 10)?;
        create_chat(&mut connection, CHAT_2, "Newer", 20)?;
        create_chat(&mut connection, CHAT_3, "Same time", 20)?;
        append_message(&mut connection, MESSAGE_2, CHAT_1, "user", "second", 30)?;
        append_message(&mut connection, MESSAGE_1, CHAT_1, "assistant", "first", 30)?;
        append_message(&mut connection, MESSAGE_3, CHAT_1, "user", "third", 40)?;

        let chats = chats_list(&connection, 50)?;
        assert_eq!(
            chats
                .iter()
                .map(|chat| chat.id.as_str())
                .collect::<Vec<_>>(),
            [CHAT_2, CHAT_3, CHAT_1]
        );

        let messages = messages_list(&connection, CHAT_1, None, 50)?;
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            [MESSAGE_1, MESSAGE_2, MESSAGE_3]
        );
        Ok(())
    }

    #[test]
    fn chat_create_is_idempotent_and_detects_conflict() -> Result<(), Box<dyn Error>> {
        let mut connection = migrated_connection()?;
        let first = create_chat(&mut connection, CHAT_1, "Same", 10)?;
        let replay = create_chat(&mut connection, CHAT_1, "Same", 10)?;
        assert_eq!(first, replay);

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let conflict = chat_create(
            &transaction,
            &ChatCreate {
                id: CHAT_1.to_string(),
                title: "Different".to_string(),
                created_at: 10,
            },
        );
        assert_eq!(conflict, Err(RepositoryError::Conflict));
        Ok(())
    }

    #[test]
    fn message_append_is_idempotent_and_detects_conflict() -> Result<(), Box<dyn Error>> {
        let mut connection = migrated_connection()?;
        create_chat(&mut connection, CHAT_1, "Chat", 10)?;
        let first = append_message(&mut connection, MESSAGE_1, CHAT_1, "user", "same", 20)?;
        let replay = append_message(&mut connection, MESSAGE_1, CHAT_1, "user", "same", 20)?;
        assert_eq!(first, replay);

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let conflict = message_append(
            &transaction,
            &MessageAppend {
                id: MESSAGE_1.to_string(),
                chat_id: CHAT_1.to_string(),
                role: "user".to_string(),
                content: "different".to_string(),
                timestamp: 20,
            },
        );
        assert_eq!(conflict, Err(RepositoryError::Conflict));
        Ok(())
    }

    #[test]
    fn append_to_missing_chat_returns_not_found() -> Result<(), Box<dyn Error>> {
        let mut connection = migrated_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = message_append(
            &transaction,
            &MessageAppend {
                id: MESSAGE_1.to_string(),
                chat_id: CHAT_1.to_string(),
                role: "user".to_string(),
                content: "content".to_string(),
                timestamp: 20,
            },
        );
        assert_eq!(result, Err(RepositoryError::NotFound));
        Ok(())
    }

    #[test]
    fn chat_delete_cascades_and_missing_is_not_found() -> Result<(), Box<dyn Error>> {
        let mut connection = migrated_connection()?;
        create_chat(&mut connection, CHAT_1, "Chat", 10)?;
        append_message(&mut connection, MESSAGE_1, CHAT_1, "user", "content", 20)?;

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        chat_delete(&transaction, CHAT_1)?;
        transaction.commit()?;

        assert!(messages_list(&connection, CHAT_1, None, 50)?.is_empty());
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        assert_eq!(
            chat_delete(&transaction, CHAT_1),
            Err(RepositoryError::NotFound)
        );
        Ok(())
    }

    #[test]
    fn message_cursor_pages_without_gaps_or_duplicates() -> Result<(), Box<dyn Error>> {
        let mut connection = migrated_connection()?;
        create_chat(&mut connection, CHAT_1, "Chat", 10)?;
        append_message(&mut connection, MESSAGE_2, CHAT_1, "user", "two", 20)?;
        append_message(&mut connection, MESSAGE_1, CHAT_1, "user", "one", 20)?;
        append_message(&mut connection, MESSAGE_3, CHAT_1, "assistant", "three", 30)?;

        let first = messages_list(&connection, CHAT_1, None, 2)?;
        assert_eq!(first.len(), 2);
        let last = first.last().ok_or("first page unexpectedly empty")?;
        let cursor = MessageCursor {
            timestamp: last.timestamp,
            id: last.id.clone(),
        };
        let second = messages_list(&connection, CHAT_1, Some(&cursor), 2)?;

        let ids = first
            .iter()
            .chain(second.iter())
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, [MESSAGE_1, MESSAGE_2, MESSAGE_3]);
        Ok(())
    }
}
