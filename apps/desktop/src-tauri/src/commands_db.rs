//! M3 secure-vault Tauri commands.
//!
//! Tauri State contains only a cloneable worker capability. Commands expose
//! closed DTO/status/error types; no connection, key, SQL, PRAGMA, or path
//! crosses IPC.

use serde::{Deserialize, Serialize};

use crate::db;

const IPC_TITLE_MAX_BYTES: usize = 2_048;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultChatCreateRequest {
    pub id: String,
    pub title: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultChatDeleteRequest {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultChatsListRequest {
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultMessageAppendRequest {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultMessageCursorRequest {
    pub timestamp: i64,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultMessagesListRequest {
    pub chat_id: String,
    pub cursor: Option<VaultMessageCursorRequest>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct VaultChatRecord {
    pub id: String,
    pub title: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct VaultMessageRecord {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

impl From<VaultChatCreateRequest> for db::ChatCreate {
    fn from(request: VaultChatCreateRequest) -> Self {
        Self {
            id: request.id,
            title: request.title,
            created_at: request.created_at,
        }
    }
}

impl From<VaultMessageAppendRequest> for db::MessageAppend {
    fn from(request: VaultMessageAppendRequest) -> Self {
        Self {
            id: request.id,
            chat_id: request.chat_id,
            role: request.role,
            content: request.content,
            timestamp: request.timestamp,
        }
    }
}

impl From<VaultMessageCursorRequest> for db::MessageCursor {
    fn from(request: VaultMessageCursorRequest) -> Self {
        Self {
            timestamp: request.timestamp,
            id: request.id,
        }
    }
}

impl From<db::ChatRecord> for VaultChatRecord {
    fn from(record: db::ChatRecord) -> Self {
        Self {
            id: record.id,
            title: record.title,
            created_at: record.created_at,
        }
    }
}

impl From<db::MessageRecord> for VaultMessageRecord {
    fn from(record: db::MessageRecord) -> Self {
        Self {
            id: record.id,
            chat_id: record.chat_id,
            role: record.role,
            content: record.content,
            timestamp: record.timestamp,
        }
    }
}

fn require_wire_id(value: &str) -> Result<(), db::VaultErrorCode> {
    if value.len() == 36 {
        Ok(())
    } else {
        Err(db::VaultErrorCode::InvalidInput)
    }
}

fn validate_chat_create_wire(request: &VaultChatCreateRequest) -> Result<(), db::VaultErrorCode> {
    require_wire_id(&request.id)?;
    if request.title.len() > IPC_TITLE_MAX_BYTES {
        return Err(db::VaultErrorCode::InvalidInput);
    }
    Ok(())
}

fn validate_chat_delete_wire(request: &VaultChatDeleteRequest) -> Result<(), db::VaultErrorCode> {
    require_wire_id(&request.id)
}

fn validate_message_append_wire(
    request: &VaultMessageAppendRequest,
) -> Result<(), db::VaultErrorCode> {
    require_wire_id(&request.id)?;
    require_wire_id(&request.chat_id)?;
    if request.content.len() > db::REPOSITORY_CONTENT_MAX_BYTES {
        return Err(db::VaultErrorCode::InvalidInput);
    }
    Ok(())
}

fn validate_messages_list_wire(
    request: &VaultMessagesListRequest,
) -> Result<(), db::VaultErrorCode> {
    require_wire_id(&request.chat_id)?;
    if let Some(cursor) = request.cursor.as_ref() {
        require_wire_id(&cursor.id)?;
    }
    Ok(())
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub fn vault_status(state: tauri::State<'_, db::VaultHandle>) -> db::VaultStatus {
    state.status()
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_unlock(
    state: tauri::State<'_, db::VaultHandle>,
) -> Result<db::VaultStatus, db::VaultErrorCode> {
    let handle = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.unlock())
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)?
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_lock(
    state: tauri::State<'_, db::VaultHandle>,
) -> Result<db::VaultStatus, db::VaultErrorCode> {
    let handle = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.lock())
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)?
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn check_db_health(
    state: tauri::State<'_, db::VaultHandle>,
) -> Result<db::VaultStatus, db::VaultErrorCode> {
    let handle = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.check_health())
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)?
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_chat_create(
    state: tauri::State<'_, db::VaultHandle>,
    request: VaultChatCreateRequest,
) -> Result<VaultChatRecord, db::VaultErrorCode> {
    validate_chat_create_wire(&request)?;
    let handle = state.inner().clone();
    let input = db::ChatCreate::from(request);
    let record = tauri::async_runtime::spawn_blocking(move || handle.chat_create(input))
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)??;
    Ok(VaultChatRecord::from(record))
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_chat_delete(
    state: tauri::State<'_, db::VaultHandle>,
    request: VaultChatDeleteRequest,
) -> Result<(), db::VaultErrorCode> {
    validate_chat_delete_wire(&request)?;
    let handle = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.chat_delete(request.id))
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)?
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_chats_list(
    state: tauri::State<'_, db::VaultHandle>,
    request: VaultChatsListRequest,
) -> Result<Vec<VaultChatRecord>, db::VaultErrorCode> {
    let handle = state.inner().clone();
    let records = tauri::async_runtime::spawn_blocking(move || handle.chats_list(request.limit))
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)??;
    Ok(records.into_iter().map(VaultChatRecord::from).collect())
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_message_append(
    state: tauri::State<'_, db::VaultHandle>,
    request: VaultMessageAppendRequest,
) -> Result<VaultMessageRecord, db::VaultErrorCode> {
    validate_message_append_wire(&request)?;
    let handle = state.inner().clone();
    let input = db::MessageAppend::from(request);
    let record = tauri::async_runtime::spawn_blocking(move || handle.message_append(input))
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)??;
    Ok(VaultMessageRecord::from(record))
}

#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_messages_list(
    state: tauri::State<'_, db::VaultHandle>,
    request: VaultMessagesListRequest,
) -> Result<Vec<VaultMessageRecord>, db::VaultErrorCode> {
    validate_messages_list_wire(&request)?;
    let handle = state.inner().clone();
    let chat_id = request.chat_id;
    let cursor = request.cursor.map(db::MessageCursor::from);
    let limit = request.limit;
    let records =
        tauri::async_runtime::spawn_blocking(move || handle.messages_list(chat_id, cursor, limit))
            .await
            .map_err(|_| db::VaultErrorCode::Unavailable)??;
    Ok(records.into_iter().map(VaultMessageRecord::from).collect())
}

/// Register a frontend event sink and return the current status. The worker
/// keeps a single sink, so re-invocation replaces (never accumulates) it. No
/// SQL, key, path, or native error crosses this boundary.
#[cfg(target_vendor = "apple")]
#[tauri::command]
pub async fn vault_events(
    state: tauri::State<'_, db::VaultHandle>,
    channel: tauri::ipc::Channel<db::VaultLifecycleEvent>,
) -> Result<db::VaultStatus, db::VaultErrorCode> {
    let handle = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.register_events(channel))
        .await
        .map_err(|_| db::VaultErrorCode::Unavailable)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::error::Error;

    const ID: &str = "00000000-0000-4000-8000-000000000001";
    const MESSAGE_ID: &str = "10000000-0000-4000-8000-000000000001";

    #[test]
    fn wire_size_checks_accept_boundaries_and_reject_excess() {
        let boundary_chat = VaultChatCreateRequest {
            id: ID.to_string(),
            title: "t".repeat(IPC_TITLE_MAX_BYTES),
            created_at: 0,
        };
        assert_eq!(validate_chat_create_wire(&boundary_chat), Ok(()));

        let short_id = VaultChatCreateRequest {
            id: "x".repeat(35),
            title: "title".to_string(),
            created_at: 0,
        };
        assert_eq!(
            validate_chat_create_wire(&short_id),
            Err(db::VaultErrorCode::InvalidInput)
        );

        let oversized_title = VaultChatCreateRequest {
            id: ID.to_string(),
            title: "t".repeat(IPC_TITLE_MAX_BYTES + 1),
            created_at: 0,
        };
        assert_eq!(
            validate_chat_create_wire(&oversized_title),
            Err(db::VaultErrorCode::InvalidInput)
        );

        let boundary_message = VaultMessageAppendRequest {
            id: MESSAGE_ID.to_string(),
            chat_id: ID.to_string(),
            role: "user".to_string(),
            content: "c".repeat(db::REPOSITORY_CONTENT_MAX_BYTES),
            timestamp: 0,
        };
        assert_eq!(validate_message_append_wire(&boundary_message), Ok(()));

        let oversized_message = VaultMessageAppendRequest {
            content: "c".repeat(db::REPOSITORY_CONTENT_MAX_BYTES + 1),
            ..boundary_message
        };
        assert_eq!(
            validate_message_append_wire(&oversized_message),
            Err(db::VaultErrorCode::InvalidInput)
        );
    }

    #[test]
    fn request_dtos_reject_unknown_fields() {
        assert!(serde_json::from_value::<VaultChatCreateRequest>(json!({
            "id": ID, "title": "title", "created_at": 0, "extra": true
        }))
        .is_err());
        assert!(serde_json::from_value::<VaultChatDeleteRequest>(json!({
            "id": ID, "extra": true
        }))
        .is_err());
        assert!(serde_json::from_value::<VaultChatsListRequest>(json!({
            "limit": 50, "extra": true
        }))
        .is_err());
        assert!(serde_json::from_value::<VaultMessageAppendRequest>(json!({
            "id": MESSAGE_ID, "chat_id": ID, "role": "user", "content": "content",
            "timestamp": 0, "extra": true
        }))
        .is_err());
        assert!(serde_json::from_value::<VaultMessagesListRequest>(json!({
            "chat_id": ID, "cursor": null, "limit": 50, "extra": true
        }))
        .is_err());
        assert!(serde_json::from_value::<VaultMessagesListRequest>(json!({
            "chat_id": ID,
            "cursor": {"timestamp": 0, "id": MESSAGE_ID, "extra": true},
            "limit": 50
        }))
        .is_err());
    }

    #[test]
    fn request_dtos_round_trip() -> Result<(), Box<dyn Error>> {
        let requests = [
            serde_json::to_value(VaultChatCreateRequest {
                id: ID.to_string(),
                title: "title".to_string(),
                created_at: 1,
            })?,
            serde_json::to_value(VaultChatDeleteRequest { id: ID.to_string() })?,
            serde_json::to_value(VaultChatsListRequest { limit: Some(50) })?,
            serde_json::to_value(VaultMessageAppendRequest {
                id: MESSAGE_ID.to_string(),
                chat_id: ID.to_string(),
                role: "assistant".to_string(),
                content: "content".to_string(),
                timestamp: 2,
            })?,
            serde_json::to_value(VaultMessagesListRequest {
                chat_id: ID.to_string(),
                cursor: Some(VaultMessageCursorRequest {
                    timestamp: 2,
                    id: MESSAGE_ID.to_string(),
                }),
                limit: Some(25),
            })?,
        ];

        assert_eq!(
            serde_json::from_value::<VaultChatCreateRequest>(requests[0].clone())?.id,
            ID
        );
        assert_eq!(
            serde_json::from_value::<VaultChatDeleteRequest>(requests[1].clone())?.id,
            ID
        );
        assert_eq!(
            serde_json::from_value::<VaultChatsListRequest>(requests[2].clone())?.limit,
            Some(50)
        );
        assert_eq!(
            serde_json::from_value::<VaultMessageAppendRequest>(requests[3].clone())?.role,
            "assistant"
        );
        assert_eq!(
            serde_json::from_value::<VaultMessagesListRequest>(requests[4].clone())?.limit,
            Some(25)
        );
        Ok(())
    }

    #[test]
    fn response_conversions_preserve_all_fields() {
        let chat = VaultChatRecord::from(db::ChatRecord {
            id: ID.to_string(),
            title: "title".to_string(),
            created_at: 1,
        });
        assert_eq!(chat.id, ID);
        assert_eq!(chat.title, "title");
        assert_eq!(chat.created_at, 1);

        let message = VaultMessageRecord::from(db::MessageRecord {
            id: MESSAGE_ID.to_string(),
            chat_id: ID.to_string(),
            role: "user".to_string(),
            content: "content".to_string(),
            timestamp: 2,
        });
        assert_eq!(message.id, MESSAGE_ID);
        assert_eq!(message.chat_id, ID);
        assert_eq!(message.role, "user");
        assert_eq!(message.content, "content");
        assert_eq!(message.timestamp, 2);
    }
}
