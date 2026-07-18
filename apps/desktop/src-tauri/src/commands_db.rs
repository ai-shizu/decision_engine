//! M3 secure-vault Tauri commands.
//!
//! Tauri State contains only a cloneable worker capability. Commands expose
//! closed status/error enums; no connection, key, SQL, PRAGMA, or path crosses
//! IPC.

use crate::db;

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
