//! Single-owner SQLCipher worker and capability handle.
//!
//! Only [`VaultHandle`] crosses into Tauri State. `LAContext`, plaintext key
//! material, and `rusqlite::Connection` are created and used solely on this
//! dedicated worker thread.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use objc2::rc::autoreleasepool;
use rusqlite::Connection;
use serde::Serialize;

use super::{
    connection::{open_encrypted_database, verify_encrypted_connection, VaultConnectionError},
    secure_vault::{SecureVault, SecureVaultError},
};

pub(crate) const VAULT_DATABASE_FILENAME: &str = "vault.sqlite3";

const COMMAND_QUEUE_CAPACITY: usize = 8;
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(120);
const SHORT_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Public lifecycle state. No secret-bearing or filesystem detail is exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Canonical lifecycle states reserved for later M3 gates.
pub(crate) enum VaultStatus {
    Unprovisioned,
    Locked,
    Unlocking,
    Unlocked,
    Locking,
    RecoveryRequired,
    OrphanedKey,
    Quarantined,
    Unavailable,
}

/// Stable IPC-safe failures. Native statuses, SQL, paths, and key bytes never
/// cross the worker boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VaultErrorCode {
    Locked,
    Busy,
    Timeout,
    AuthenticationCancelled,
    AuthenticationFailed,
    InteractionNotAllowed,
    KeychainUnavailable,
    CorruptOrWrongKey,
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct VaultHandle {
    inner: Arc<VaultHandleInner>,
}

struct VaultHandleInner {
    sender: Mutex<Option<SyncSender<VaultRequest>>>,
    status: Arc<Mutex<VaultStatus>>,
}

#[derive(Clone, Copy)]
enum VaultOperation {
    Unlock,
    Lock,
    Health,
}

struct RequestControl {
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

struct VaultRequest {
    operation: VaultOperation,
    control: RequestControl,
    reply: SyncSender<Result<VaultStatus, VaultErrorCode>>,
}

impl VaultHandle {
    /// Spawn the bounded, single-connection worker. A thread creation failure
    /// produces an `Unavailable` handle instead of panicking or opening a
    /// fallback database.
    pub(crate) fn spawn(database_path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let status = Arc::new(Mutex::new(VaultStatus::Locked));
        let handle = Self {
            inner: Arc::new(VaultHandleInner {
                sender: Mutex::new(Some(sender)),
                status: Arc::clone(&status),
            }),
        };

        let spawn_result = thread::Builder::new()
            .name("pkb-vault-worker".to_string())
            .spawn(move || VaultWorker::new(database_path, status).run(receiver));

        if spawn_result.is_err() {
            if let Ok(mut sender_slot) = handle.inner.sender.lock() {
                *sender_slot = None;
            }
            publish_status(&handle.inner.status, VaultStatus::Unavailable);
        }

        handle
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            inner: Arc::new(VaultHandleInner {
                sender: Mutex::new(None),
                status: Arc::new(Mutex::new(VaultStatus::Unavailable)),
            }),
        }
    }

    pub(crate) fn status(&self) -> VaultStatus {
        snapshot_status(&self.inner.status)
    }

    pub(crate) fn unlock(&self) -> Result<VaultStatus, VaultErrorCode> {
        self.request(VaultOperation::Unlock, UNLOCK_TIMEOUT)
    }

    pub(crate) fn lock(&self) -> Result<VaultStatus, VaultErrorCode> {
        self.request(VaultOperation::Lock, SHORT_OPERATION_TIMEOUT)
    }

    pub(crate) fn check_health(&self) -> Result<VaultStatus, VaultErrorCode> {
        self.request(VaultOperation::Health, SHORT_OPERATION_TIMEOUT)
    }

    fn request(
        &self,
        operation: VaultOperation,
        timeout: Duration,
    ) -> Result<VaultStatus, VaultErrorCode> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest {
            operation,
            control: RequestControl {
                deadline: Instant::now() + timeout,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };

        let send_result = {
            let sender_guard = self
                .inner
                .sender
                .lock()
                .map_err(|_| VaultErrorCode::Unavailable)?;
            let sender = sender_guard.as_ref().ok_or(VaultErrorCode::Unavailable)?;
            sender.try_send(request)
        };

        match send_result {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(VaultErrorCode::Busy),
            Err(TrySendError::Disconnected(_)) => return Err(VaultErrorCode::Unavailable),
        }

        match reply_receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Release);
                Err(VaultErrorCode::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(VaultErrorCode::Unavailable),
        }
    }
}

struct VaultWorker {
    database_path: PathBuf,
    connection: Option<Connection>,
    status: Arc<Mutex<VaultStatus>>,
}

impl VaultWorker {
    fn new(database_path: PathBuf, status: Arc<Mutex<VaultStatus>>) -> Self {
        Self {
            database_path,
            connection: None,
            status,
        }
    }

    fn run(mut self, receiver: Receiver<VaultRequest>) {
        while let Ok(request) = receiver.recv() {
            let result = if request_expired(&request.control) {
                Err(VaultErrorCode::Timeout)
            } else {
                match request.operation {
                    VaultOperation::Unlock => self.unlock(&request.control),
                    VaultOperation::Lock => self.lock(),
                    VaultOperation::Health => self.check_health(),
                }
            };
            let _ = request.reply.send(result);
        }

        self.connection.take();
        publish_status(&self.status, VaultStatus::Locked);
    }

    fn unlock(&mut self, control: &RequestControl) -> Result<VaultStatus, VaultErrorCode> {
        if self.connection.is_some() {
            return Ok(VaultStatus::Unlocked);
        }

        publish_status(&self.status, VaultStatus::Unlocking);
        let database_path = self.database_path.clone();
        let open_result = autoreleasepool(move |_| {
            let context = SecureVault::new_authentication_context();
            open_encrypted_database(&database_path, &context)
        });

        if request_expired(control) {
            drop(open_result);
            publish_status(&self.status, VaultStatus::Locked);
            return Err(VaultErrorCode::Timeout);
        }

        match open_result {
            Ok(connection) => {
                self.connection = Some(connection);
                publish_status(&self.status, VaultStatus::Unlocked);
                Ok(VaultStatus::Unlocked)
            }
            Err(error) => {
                let (status, code) = classify_connection_error(error);
                publish_status(&self.status, status);
                Err(code)
            }
        }
    }

    fn lock(&mut self) -> Result<VaultStatus, VaultErrorCode> {
        publish_status(&self.status, VaultStatus::Locking);
        self.connection.take();
        publish_status(&self.status, VaultStatus::Locked);
        Ok(VaultStatus::Locked)
    }

    fn check_health(&mut self) -> Result<VaultStatus, VaultErrorCode> {
        let connection = self.connection.as_ref().ok_or(VaultErrorCode::Locked)?;

        if let Err(error) = verify_encrypted_connection(connection) {
            let (status, code) = classify_connection_error(error);
            self.connection.take();
            publish_status(&self.status, status);
            return Err(code);
        }

        Ok(VaultStatus::Unlocked)
    }
}

fn request_expired(control: &RequestControl) -> bool {
    control.cancelled.load(Ordering::Acquire) || Instant::now() >= control.deadline
}

fn publish_status(status: &Mutex<VaultStatus>, next: VaultStatus) {
    if let Ok(mut current) = status.lock() {
        *current = next;
    }
}

fn snapshot_status(status: &Mutex<VaultStatus>) -> VaultStatus {
    status
        .lock()
        .map(|current| *current)
        .unwrap_or(VaultStatus::Unavailable)
}

fn classify_connection_error(error: VaultConnectionError) -> (VaultStatus, VaultErrorCode) {
    match error {
        VaultConnectionError::SecureVault(SecureVaultError::AuthenticationCancelled) => {
            (VaultStatus::Locked, VaultErrorCode::AuthenticationCancelled)
        }
        VaultConnectionError::SecureVault(SecureVaultError::AuthenticationFailed) => {
            (VaultStatus::Locked, VaultErrorCode::AuthenticationFailed)
        }
        VaultConnectionError::SecureVault(SecureVaultError::InteractionNotAllowed) => {
            (VaultStatus::Locked, VaultErrorCode::InteractionNotAllowed)
        }
        VaultConnectionError::SchemaVerificationFailed
        | VaultConnectionError::KeyApplicationFailed
        | VaultConnectionError::InvalidKeyLength { .. } => {
            (VaultStatus::Quarantined, VaultErrorCode::CorruptOrWrongKey)
        }
        VaultConnectionError::SecureVault(_) => (
            VaultStatus::Unavailable,
            VaultErrorCode::KeychainUnavailable,
        ),
        VaultConnectionError::OpenFailed | VaultConnectionError::CipherIdentityUnavailable => {
            (VaultStatus::Unavailable, VaultErrorCode::Unavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_handle_fails_closed() {
        let handle = VaultHandle::unavailable();
        assert_eq!(handle.status(), VaultStatus::Unavailable);
        assert_eq!(handle.check_health(), Err(VaultErrorCode::Unavailable));
    }

    #[test]
    fn worker_starts_locked_without_authentication() {
        let handle = VaultHandle::spawn(std::env::temp_dir().join("pkb-worker-unused.sqlite3"));
        assert_eq!(handle.status(), VaultStatus::Locked);
        assert_eq!(handle.check_health(), Err(VaultErrorCode::Locked));
        assert_eq!(handle.lock(), Ok(VaultStatus::Locked));
    }
}
