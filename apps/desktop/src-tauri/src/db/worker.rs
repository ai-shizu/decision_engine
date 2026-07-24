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
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::ipc_contract::MAX_TEXT_BYTES;

use super::{
    analytics_repo::{self, GapAnalysisRow, TensorProfileRow},
    connection::{
        maintain_encrypted_database, open_encrypted_database, verify_encrypted_connection,
        VaultConnectionError,
    },
    knowledge_repo::{self, KnowledgeChunkRow, KnowledgeSearchHit},
    migrations::{run_migrations, MigrationError},
    distortion_repo::{self, DistortionTagRow},
    purchase_repo::{self, PurchaseLineRow, PurchaseRow},
    commitment_repo::{self, CommitmentRow},
    oracle_repo::{self, InterviewSessionRow, OracleRunRow, TwinRunRow},
    psychometrics_repo::{self, PulseRunRow, ProbeStoreRow, RaschRunRow},
    repository::{
        self, ChatCreate, ChatRecord, MessageAppend, MessageCursor, MessageRecord, RepositoryError,
    },
    secure_vault::{SecureVault, SecureVaultError},
};

pub(crate) const VAULT_DATABASE_FILENAME: &str = "vault.sqlite3";

const COMMAND_QUEUE_CAPACITY: usize = 8;
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(120);
const SHORT_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_LIST_LIMIT: u32 = 50;
const MAX_LIST_LIMIT: u32 = 200;
const TITLE_MAX_BYTES: usize = 512;
pub(crate) const REPOSITORY_CONTENT_MAX_BYTES: usize = if MAX_TEXT_BYTES < 64 * 1024 {
    MAX_TEXT_BYTES
} else {
    64 * 1024
};

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
    UnsupportedSchema,
    VaultQuarantined,
    Unavailable,
    InvalidInput,
    NotFound,
    Conflict,
    StorageFailed,
    /// The OS denied storage access (iOS Data Protection sealed the file while
    /// the device was locked). The worker has self-locked; re-authentication is
    /// required.
    OsLockEngaged,
}

/// Push payload sent to the frontend when the worker's public state changes.
///
/// `Status` mirrors every lifecycle transition (including out-of-band ones such
/// as background auto-lock). `Error` additionally flags a fail-closed event the
/// UI must act on immediately (e.g. Data-Protection self-lock). No SQL, path,
/// key, or native error detail is ever included.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum VaultLifecycleEvent {
    Status {
        status: VaultStatus,
    },
    Error {
        code: VaultErrorCode,
        status: VaultStatus,
    },
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

enum VaultReply {
    Lifecycle(Result<VaultStatus, VaultErrorCode>),
    ChatCreate(Result<ChatRecord, VaultErrorCode>),
    ChatDelete(Result<(), VaultErrorCode>),
    ChatsList(Result<Vec<ChatRecord>, VaultErrorCode>),
    MessageAppend(Result<MessageRecord, VaultErrorCode>),
    MessagesList(Result<Vec<MessageRecord>, VaultErrorCode>),
    KnowledgeReplace(Result<usize, VaultErrorCode>),
    KnowledgeSearch(Result<Vec<KnowledgeSearchHit>, VaultErrorCode>),
    GapAnalysisInsert(Result<(), VaultErrorCode>),
    GapAnalysisLatest(Result<Option<GapAnalysisRow>, VaultErrorCode>),
    TensorProfileInsert(Result<(), VaultErrorCode>),
    TensorProfileLatest(Result<Option<TensorProfileRow>, VaultErrorCode>),
    PulseRunInsert(Result<(), VaultErrorCode>),
    RaschRunUpsert(Result<(), VaultErrorCode>),
    RaschRunLatest(Result<Option<RaschRunRow>, VaultErrorCode>),
    ProbeStoreGet(Result<Option<ProbeStoreRow>, VaultErrorCode>),
    ProbeStorePut(Result<(), VaultErrorCode>),
    PulseRunLatest(Result<Option<PulseRunRow>, VaultErrorCode>),
    TwinRunInsert(Result<(), VaultErrorCode>),
    TwinRunListPayloads(Result<Vec<String>, VaultErrorCode>),
    TwinRunLatestPayload(Result<Option<String>, VaultErrorCode>),
    OracleRunInsert(Result<(), VaultErrorCode>),
    OracleRunLatest(Result<Option<OracleRunRow>, VaultErrorCode>),
    InterviewSessionPut(Result<(), VaultErrorCode>),
    InterviewSessionGet(Result<Option<InterviewSessionRow>, VaultErrorCode>),
    DistortionTagsInsert(Result<usize, VaultErrorCode>),
    DistortionTagsList(Result<Vec<DistortionTagRow>, VaultErrorCode>),
    PurchaseInsert(Result<(), VaultErrorCode>),
    PurchaseListRange(Result<Vec<PurchaseRow>, VaultErrorCode>),
    PurchaseListRecent(Result<Vec<PurchaseRow>, VaultErrorCode>),
    CommitmentUpsert(Result<(), VaultErrorCode>),
    CommitmentList(Result<Vec<CommitmentRow>, VaultErrorCode>),
    CommitmentListEnabled(Result<Vec<CommitmentRow>, VaultErrorCode>),
    CommitmentFindBySource(Result<Option<String>, VaultErrorCode>),
}

enum VaultRequest {
    Lifecycle {
        operation: VaultOperation,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    ChatCreate {
        input: ChatCreate,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    ChatDelete {
        id: String,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    ChatsList {
        limit: Option<u32>,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    MessageAppend {
        input: MessageAppend,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    MessagesList {
        chat_id: String,
        cursor: Option<MessageCursor>,
        limit: Option<u32>,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    KnowledgeReplace {
        source_id: String,
        rows: Vec<KnowledgeChunkRow>,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    KnowledgeSearch {
        embedding: Vec<f32>,
        limit: u32,
        namespace: crate::db::KnowledgeNamespace,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    GapAnalysisInsert {
        row: GapAnalysisRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    GapAnalysisLatest {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    TensorProfileInsert {
        row: TensorProfileRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    TensorProfileLatest {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    PulseRunInsert {
        row: PulseRunRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    RaschRunUpsert {
        row: RaschRunRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    RaschRunLatest {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    ProbeStoreGet {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    ProbeStorePut {
        row: ProbeStoreRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    PulseRunLatest {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    TwinRunInsert {
        row: TwinRunRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    TwinRunListPayloads {
        limit: u32,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    TwinRunLatestPayload {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    OracleRunInsert {
        row: OracleRunRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    OracleRunLatest {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    InterviewSessionPut {
        row: InterviewSessionRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    InterviewSessionGet {
        id: String,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    DistortionTagsInsert {
        rows: Vec<DistortionTagRow>,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    DistortionTagsList {
        limit: u32,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    PurchaseInsert {
        purchase: PurchaseRow,
        lines: Vec<PurchaseLineRow>,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    PurchaseListRange {
        start_unix: i64,
        end_unix: i64,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    PurchaseListRecent {
        limit: u32,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    CommitmentUpsert {
        row: CommitmentRow,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    CommitmentList {
        limit: u32,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    CommitmentListEnabled {
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    CommitmentFindBySource {
        source_relation_id: String,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
    RegisterEvents {
        channel: Channel<VaultLifecycleEvent>,
        control: RequestControl,
        reply: SyncSender<VaultReply>,
    },
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

    /// Register a frontend event sink and return the current status. The worker
    /// keeps only one sink; a later registration replaces the earlier one, so
    /// re-mounts never accumulate senders.
    pub(crate) fn register_events(
        &self,
        channel: Channel<VaultLifecycleEvent>,
    ) -> Result<VaultStatus, VaultErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::RegisterEvents {
            channel,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::Lifecycle(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    fn request(
        &self,
        operation: VaultOperation,
        timeout: Duration,
    ) -> Result<VaultStatus, VaultErrorCode> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::Lifecycle {
            operation,
            control: RequestControl {
                deadline: Instant::now() + timeout,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };

        match self.submit(request, reply_receiver, cancelled, timeout)? {
            VaultReply::Lifecycle(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    fn submit(
        &self,
        request: VaultRequest,
        reply_receiver: Receiver<VaultReply>,
        cancelled: Arc<AtomicBool>,
        timeout: Duration,
    ) -> Result<VaultReply, VaultErrorCode> {
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
            Ok(reply) => Ok(reply),
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Release);
                Err(VaultErrorCode::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(VaultErrorCode::Unavailable),
        }
    }
}

impl VaultHandle {
    pub(crate) fn chat_create(&self, input: ChatCreate) -> Result<ChatRecord, VaultErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::ChatCreate {
            input,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::ChatCreate(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn chat_delete(&self, id: String) -> Result<(), VaultErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::ChatDelete {
            id,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::ChatDelete(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn chats_list(&self, limit: Option<u32>) -> Result<Vec<ChatRecord>, VaultErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::ChatsList {
            limit,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::ChatsList(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn message_append(
        &self,
        input: MessageAppend,
    ) -> Result<MessageRecord, VaultErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::MessageAppend {
            input,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::MessageAppend(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn messages_list(
        &self,
        chat_id: String,
        cursor: Option<MessageCursor>,
        limit: Option<u32>,
    ) -> Result<Vec<MessageRecord>, VaultErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::MessagesList {
            chat_id,
            cursor,
            limit,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::MessagesList(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    /// Replace all chunks for `source_id` then insert `rows` in one transaction.
    pub(crate) fn knowledge_replace(
        &self,
        source_id: String,
        rows: Vec<KnowledgeChunkRow>,
    ) -> Result<usize, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::KnowledgeReplace {
            source_id,
            rows,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::KnowledgeReplace(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    /// KNN search over `knowledge_chunks`.
    pub(crate) fn knowledge_search(
        &self,
        embedding: Vec<f32>,
        limit: u32,
        namespace: crate::db::KnowledgeNamespace,
    ) -> Result<Vec<KnowledgeSearchHit>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::KnowledgeSearch {
            embedding,
            limit,
            namespace,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::KnowledgeSearch(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn gap_analysis_insert(&self, row: GapAnalysisRow) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::GapAnalysisInsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::GapAnalysisInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn gap_analysis_latest(
        &self,
    ) -> Result<Option<GapAnalysisRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::GapAnalysisLatest {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::GapAnalysisLatest(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn tensor_profile_insert(
        &self,
        row: TensorProfileRow,
    ) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::TensorProfileInsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::TensorProfileInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn tensor_profile_latest(
        &self,
    ) -> Result<Option<TensorProfileRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::TensorProfileLatest {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::TensorProfileLatest(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn pulse_run_insert(&self, row: PulseRunRow) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::PulseRunInsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::PulseRunInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn rasch_run_upsert(&self, row: RaschRunRow) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::RaschRunUpsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::RaschRunUpsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn rasch_run_latest(&self) -> Result<Option<RaschRunRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::RaschRunLatest {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::RaschRunLatest(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn probe_store_get(&self) -> Result<Option<ProbeStoreRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::ProbeStoreGet {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::ProbeStoreGet(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn probe_store_put(&self, row: ProbeStoreRow) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::ProbeStorePut {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::ProbeStorePut(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn pulse_run_latest(&self) -> Result<Option<PulseRunRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::PulseRunLatest {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::PulseRunLatest(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn twin_run_insert(&self, row: TwinRunRow) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::TwinRunInsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::TwinRunInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn twin_run_list_payloads(&self, limit: u32) -> Result<Vec<String>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::TwinRunListPayloads {
            limit,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::TwinRunListPayloads(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn twin_run_latest_payload(&self) -> Result<Option<String>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::TwinRunLatestPayload {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::TwinRunLatestPayload(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn oracle_run_insert(&self, row: OracleRunRow) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::OracleRunInsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::OracleRunInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn oracle_run_latest(&self) -> Result<Option<OracleRunRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::OracleRunLatest {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::OracleRunLatest(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn interview_session_put(
        &self,
        row: InterviewSessionRow,
    ) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::InterviewSessionPut {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::InterviewSessionPut(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn interview_session_get(
        &self,
        id: String,
    ) -> Result<Option<InterviewSessionRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::InterviewSessionGet {
            id,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::InterviewSessionGet(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn distortion_tags_insert(
        &self,
        rows: Vec<DistortionTagRow>,
    ) -> Result<usize, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::DistortionTagsInsert {
            rows,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::DistortionTagsInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn distortion_tags_list(
        &self,
        limit: u32,
    ) -> Result<Vec<DistortionTagRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::DistortionTagsList {
            limit,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::DistortionTagsList(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn purchase_insert(
        &self,
        purchase: PurchaseRow,
        lines: Vec<PurchaseLineRow>,
    ) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::PurchaseInsert {
            purchase,
            lines,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::PurchaseInsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn purchase_list_range(
        &self,
        start_unix: i64,
        end_unix: i64,
    ) -> Result<Vec<PurchaseRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::PurchaseListRange {
            start_unix,
            end_unix,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::PurchaseListRange(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn purchase_list_recent(
        &self,
        limit: u32,
    ) -> Result<Vec<PurchaseRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::PurchaseListRecent {
            limit,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::PurchaseListRecent(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn commitment_upsert(
        &self,
        row: CommitmentRow,
    ) -> Result<(), VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::CommitmentUpsert {
            row,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::CommitmentUpsert(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn commitment_list(
        &self,
        limit: u32,
    ) -> Result<Vec<CommitmentRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::CommitmentList {
            limit,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::CommitmentList(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn commitment_list_enabled(
        &self,
    ) -> Result<Vec<CommitmentRow>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::CommitmentListEnabled {
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::CommitmentListEnabled(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }

    pub(crate) fn commitment_find_by_source(
        &self,
        source_relation_id: String,
    ) -> Result<Option<String>, VaultErrorCode> {
        let (reply_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = VaultRequest::CommitmentFindBySource {
            source_relation_id,
            control: RequestControl {
                deadline: Instant::now() + SHORT_OPERATION_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            },
            reply: reply_sender,
        };
        match self.submit(request, receiver, cancelled, SHORT_OPERATION_TIMEOUT)? {
            VaultReply::CommitmentFindBySource(result) => result,
            _ => Err(VaultErrorCode::Unavailable),
        }
    }
}


struct VaultWorker {
    database_path: PathBuf,
    connection: Option<Connection>,
    status: Arc<Mutex<VaultStatus>>,
    event_sink: Option<Channel<VaultLifecycleEvent>>,
}

impl VaultWorker {
    fn new(database_path: PathBuf, status: Arc<Mutex<VaultStatus>>) -> Self {
        Self {
            database_path,
            connection: None,
            status,
            event_sink: None,
        }
    }

    fn run(mut self, receiver: Receiver<VaultRequest>) {
        while let Ok(request) = receiver.recv() {
            match request {
                VaultRequest::Lifecycle {
                    operation,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        match operation {
                            VaultOperation::Unlock => self.unlock(&control),
                            VaultOperation::Lock => self.lock(),
                            VaultOperation::Health => self.check_health(),
                        }
                    };
                    let _ = reply.send(VaultReply::Lifecycle(result));
                }
                VaultRequest::ChatCreate {
                    input,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.create_chat(input)
                    };
                    let _ = reply.send(VaultReply::ChatCreate(result));
                }
                VaultRequest::ChatDelete { id, control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.delete_chat(id)
                    };
                    let _ = reply.send(VaultReply::ChatDelete(result));
                }
                VaultRequest::ChatsList {
                    limit,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_chats(limit)
                    };
                    let _ = reply.send(VaultReply::ChatsList(result));
                }
                VaultRequest::MessageAppend {
                    input,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.append_message(input)
                    };
                    let _ = reply.send(VaultReply::MessageAppend(result));
                }
                VaultRequest::MessagesList {
                    chat_id,
                    cursor,
                    limit,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_messages(chat_id, cursor, limit)
                    };
                    let _ = reply.send(VaultReply::MessagesList(result));
                }
                VaultRequest::KnowledgeReplace {
                    source_id,
                    rows,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.replace_knowledge(source_id, rows)
                    };
                    let _ = reply.send(VaultReply::KnowledgeReplace(result));
                }
                VaultRequest::KnowledgeSearch {
                    embedding,
                    limit,
                    namespace,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.search_knowledge(embedding, limit, namespace)
                    };
                    let _ = reply.send(VaultReply::KnowledgeSearch(result));
                }
                VaultRequest::GapAnalysisInsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_gap_analysis(row)
                    };
                    let _ = reply.send(VaultReply::GapAnalysisInsert(result));
                }
                VaultRequest::GapAnalysisLatest { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.latest_gap_analysis()
                    };
                    let _ = reply.send(VaultReply::GapAnalysisLatest(result));
                }
                VaultRequest::TensorProfileInsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_tensor_profile(row)
                    };
                    let _ = reply.send(VaultReply::TensorProfileInsert(result));
                }
                VaultRequest::TensorProfileLatest { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.latest_tensor_profile()
                    };
                    let _ = reply.send(VaultReply::TensorProfileLatest(result));
                }
                VaultRequest::PulseRunInsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_pulse_run(row)
                    };
                    let _ = reply.send(VaultReply::PulseRunInsert(result));
                }
                VaultRequest::RaschRunUpsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.upsert_rasch_run(row)
                    };
                    let _ = reply.send(VaultReply::RaschRunUpsert(result));
                }
                VaultRequest::RaschRunLatest { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.latest_rasch_run()
                    };
                    let _ = reply.send(VaultReply::RaschRunLatest(result));
                }
                VaultRequest::ProbeStoreGet { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.get_probe_store()
                    };
                    let _ = reply.send(VaultReply::ProbeStoreGet(result));
                }
                VaultRequest::ProbeStorePut {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.put_probe_store(row)
                    };
                    let _ = reply.send(VaultReply::ProbeStorePut(result));
                }
                VaultRequest::PulseRunLatest { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.latest_pulse_run()
                    };
                    let _ = reply.send(VaultReply::PulseRunLatest(result));
                }
                VaultRequest::TwinRunInsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_twin_run(row)
                    };
                    let _ = reply.send(VaultReply::TwinRunInsert(result));
                }
                VaultRequest::TwinRunListPayloads {
                    limit,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_twin_run_payloads(limit)
                    };
                    let _ = reply.send(VaultReply::TwinRunListPayloads(result));
                }
                VaultRequest::TwinRunLatestPayload { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.latest_twin_run_payload()
                    };
                    let _ = reply.send(VaultReply::TwinRunLatestPayload(result));
                }
                VaultRequest::OracleRunInsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_oracle_run(row)
                    };
                    let _ = reply.send(VaultReply::OracleRunInsert(result));
                }
                VaultRequest::OracleRunLatest { control, reply } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.latest_oracle_run()
                    };
                    let _ = reply.send(VaultReply::OracleRunLatest(result));
                }
                VaultRequest::InterviewSessionPut {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.put_interview_session(row)
                    };
                    let _ = reply.send(VaultReply::InterviewSessionPut(result));
                }
                VaultRequest::InterviewSessionGet {
                    id,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.get_interview_session(id)
                    };
                    let _ = reply.send(VaultReply::InterviewSessionGet(result));
                }
                VaultRequest::DistortionTagsInsert {
                    rows,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_distortion_tags(rows)
                    };
                    let _ = reply.send(VaultReply::DistortionTagsInsert(result));
                }
                VaultRequest::DistortionTagsList {
                    limit,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_distortion_tags(limit)
                    };
                    let _ = reply.send(VaultReply::DistortionTagsList(result));
                }
                VaultRequest::PurchaseInsert {
                    purchase,
                    lines,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.insert_purchase(purchase, lines)
                    };
                    let _ = reply.send(VaultReply::PurchaseInsert(result));
                }
                VaultRequest::PurchaseListRange {
                    start_unix,
                    end_unix,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_purchases_in_range(start_unix, end_unix)
                    };
                    let _ = reply.send(VaultReply::PurchaseListRange(result));
                }
                VaultRequest::PurchaseListRecent {
                    limit,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_purchases_recent(limit)
                    };
                    let _ = reply.send(VaultReply::PurchaseListRecent(result));
                }
                VaultRequest::CommitmentUpsert {
                    row,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.upsert_commitment(row)
                    };
                    let _ = reply.send(VaultReply::CommitmentUpsert(result));
                }
                VaultRequest::CommitmentList {
                    limit,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_commitments(limit)
                    };
                    let _ = reply.send(VaultReply::CommitmentList(result));
                }
                VaultRequest::CommitmentListEnabled {
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.list_enabled_commitments()
                    };
                    let _ = reply.send(VaultReply::CommitmentListEnabled(result));
                }
                VaultRequest::CommitmentFindBySource {
                    source_relation_id,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        self.find_commitment_by_source(source_relation_id)
                    };
                    let _ = reply.send(VaultReply::CommitmentFindBySource(result));
                }
                VaultRequest::RegisterEvents {
                    channel,
                    control,
                    reply,
                } => {
                    let result = if request_expired(&control) {
                        Err(VaultErrorCode::Timeout)
                    } else {
                        Ok(self.register_events(channel))
                    };
                    let _ = reply.send(VaultReply::Lifecycle(result));
                }
            }
        }

        self.connection.take();
        self.set_status(VaultStatus::Locked);
    }

    /// Store the frontend event sink (replacing any prior one) and immediately
    /// push the current status so a freshly-subscribed UI syncs at once.
    fn register_events(&mut self, channel: Channel<VaultLifecycleEvent>) -> VaultStatus {
        let current = snapshot_status(&self.status);
        self.event_sink = Some(channel);
        self.emit(VaultLifecycleEvent::Status { status: current });
        current
    }

    /// Update the public status and push a `Status` lifecycle event.
    fn set_status(&self, next: VaultStatus) {
        publish_status(&self.status, next);
        self.emit(VaultLifecycleEvent::Status { status: next });
    }

    /// Update the public status and push an `Error` lifecycle event the UI must
    /// act on immediately (fail-closed).
    fn emit_error(&self, code: VaultErrorCode, next: VaultStatus) {
        publish_status(&self.status, next);
        self.emit(VaultLifecycleEvent::Error { code, status: next });
    }

    /// Fail-open push: a torn-down receiver must never panic or block the
    /// worker, so a send error is intentionally ignored.
    fn emit(&self, event: VaultLifecycleEvent) {
        if let Some(channel) = &self.event_sink {
            let _ = channel.send(event);
        }
    }

    fn unlock(&mut self, control: &RequestControl) -> Result<VaultStatus, VaultErrorCode> {
        if self.connection.is_some() {
            return Ok(VaultStatus::Unlocked);
        }

        self.set_status(VaultStatus::Unlocking);
        let database_path = self.database_path.clone();
        let open_result = autoreleasepool(move |_| {
            let context = SecureVault::new_authentication_context();
            open_encrypted_database(&database_path, &context)
        });

        if request_expired(control) {
            drop(open_result);
            self.set_status(VaultStatus::Locked);
            return Err(VaultErrorCode::Timeout);
        }

        match open_result {
            Ok(mut connection) => {
                if let Err(error) = run_migrations(&mut connection) {
                    let (status, code) = classify_migration_error(error);
                    drop(connection);
                    self.set_status(status);
                    return Err(code);
                }
                if request_expired(control) {
                    drop(connection);
                    self.set_status(VaultStatus::Locked);
                    return Err(VaultErrorCode::Timeout);
                }

                self.connection = Some(connection);
                self.set_status(VaultStatus::Unlocked);
                Ok(VaultStatus::Unlocked)
            }
            Err(error) => {
                let (status, code) = classify_connection_error(error);
                self.set_status(status);
                Err(code)
            }
        }
    }

    fn lock(&mut self) -> Result<VaultStatus, VaultErrorCode> {
        self.set_status(VaultStatus::Locking);
        if let Some(connection) = self.connection.take() {
            maintain_encrypted_database(&connection);
            drop(connection);
        }
        self.set_status(VaultStatus::Locked);
        Ok(VaultStatus::Locked)
    }

    fn check_health(&mut self) -> Result<VaultStatus, VaultErrorCode> {
        let connection = self.connection.as_ref().ok_or(VaultErrorCode::Locked)?;

        if let Err(error) = verify_encrypted_connection(connection) {
            let (status, code) = classify_connection_error(error);
            self.connection.take();
            self.set_status(status);
            return Err(code);
        }

        // Idle health probe also runs planner optimize + incremental vacuum.
        maintain_encrypted_database(connection);

        Ok(VaultStatus::Unlocked)
    }

    fn create_chat(&mut self, input: ChatCreate) -> Result<ChatRecord, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let input = validate_chat_create(input)?;
        let outcome =
            self.write_repository(|transaction| repository::chat_create(transaction, &input));
        self.resolve_repository(outcome)
    }

    fn delete_chat(&mut self, id: String) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        validate_uuid_v4(&id)?;
        let outcome =
            self.write_repository(|transaction| repository::chat_delete(transaction, &id));
        self.resolve_repository(outcome)
    }

    fn list_chats(&mut self, limit: Option<u32>) -> Result<Vec<ChatRecord>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome = self
            .read_repository(|connection| repository::chats_list(connection, clamp_limit(limit)));
        self.resolve_repository(outcome)
    }

    fn append_message(&mut self, input: MessageAppend) -> Result<MessageRecord, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let input = validate_message_append(input)?;
        let outcome =
            self.write_repository(|transaction| repository::message_append(transaction, &input));
        self.resolve_repository(outcome)
    }

    fn list_messages(
        &mut self,
        chat_id: String,
        cursor: Option<MessageCursor>,
        limit: Option<u32>,
    ) -> Result<Vec<MessageRecord>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        validate_uuid_v4(&chat_id)?;
        if let Some(cursor) = cursor.as_ref() {
            validate_uuid_v4(&cursor.id)?;
            validate_nonnegative(cursor.timestamp)?;
        }
        let outcome = self.read_repository(|connection| {
            repository::messages_list(connection, &chat_id, cursor.as_ref(), clamp_limit(limit))
        });
        self.resolve_repository(outcome)
    }

    fn replace_knowledge(
        &mut self,
        source_id: String,
        rows: Vec<KnowledgeChunkRow>,
    ) -> Result<usize, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if source_id.is_empty() || source_id.len() > TITLE_MAX_BYTES {
            return Err(VaultErrorCode::InvalidInput);
        }
        if source_id.contains('%') || source_id.contains('_') {
            // LIKE metacharacters would broaden DELETE — reject rather than escape.
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.write_repository(|transaction| {
            knowledge_repo::replace_source_chunks(transaction, &source_id, &rows)
        });
        self.resolve_repository(outcome)
    }

    fn search_knowledge(
        &mut self,
        embedding: Vec<f32>,
        limit: u32,
        namespace: crate::db::KnowledgeNamespace,
    ) -> Result<Vec<KnowledgeSearchHit>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if !(1..=MAX_LIST_LIMIT).contains(&limit) {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.read_repository(|connection| {
            knowledge_repo::search_chunks(connection, &embedding, limit, namespace)
        });
        self.resolve_repository(outcome)
    }

    fn insert_gap_analysis(&mut self, row: GapAnalysisRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.payload_json.len() > MAX_TEXT_BYTES * 4 {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.read_repository(|connection| {
            analytics_repo::insert_gap_analysis(connection, &row)
        });
        self.resolve_repository(outcome)
    }

    fn latest_gap_analysis(&mut self) -> Result<Option<GapAnalysisRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome =
            self.read_repository(|connection| analytics_repo::latest_gap_analysis(connection));
        self.resolve_repository(outcome)
    }

    fn insert_tensor_profile(&mut self, row: TensorProfileRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.payload_json.len() > MAX_TEXT_BYTES * 2 {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.read_repository(|connection| {
            analytics_repo::insert_tensor_profile(connection, &row)
        });
        self.resolve_repository(outcome)
    }

    fn latest_tensor_profile(&mut self) -> Result<Option<TensorProfileRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome =
            self.read_repository(|connection| analytics_repo::latest_tensor_profile(connection));
        self.resolve_repository(outcome)
    }

    fn insert_pulse_run(&mut self, row: PulseRunRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.metrics_json.len() > MAX_TEXT_BYTES * 2
            || row.interaction_tendency.len() > MAX_TEXT_BYTES
            || row.next_best_action.len() > MAX_TEXT_BYTES
        {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self
            .read_repository(|connection| psychometrics_repo::insert_pulse_run(connection, &row));
        self.resolve_repository(outcome)
    }

    fn upsert_rasch_run(&mut self, row: RaschRunRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.posterior_json.len() > MAX_TEXT_BYTES * 2
            || row.excluded_json.len() > MAX_TEXT_BYTES
        {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self
            .read_repository(|connection| psychometrics_repo::upsert_rasch_run(connection, &row));
        self.resolve_repository(outcome)
    }

    fn latest_rasch_run(&mut self) -> Result<Option<RaschRunRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome =
            self.read_repository(|connection| psychometrics_repo::latest_rasch_run(connection));
        self.resolve_repository(outcome)
    }

    fn get_probe_store(&mut self) -> Result<Option<ProbeStoreRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome =
            self.read_repository(|connection| psychometrics_repo::get_probe_store(connection));
        self.resolve_repository(outcome)
    }

    fn put_probe_store(&mut self, row: ProbeStoreRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.payload_json.len() > MAX_TEXT_BYTES * 4 {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self
            .read_repository(|connection| psychometrics_repo::put_probe_store(connection, &row));
        self.resolve_repository(outcome)
    }

    fn latest_pulse_run(&mut self) -> Result<Option<PulseRunRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome =
            self.read_repository(|connection| psychometrics_repo::latest_pulse_run(connection));
        self.resolve_repository(outcome)
    }

    fn insert_twin_run(&mut self, row: TwinRunRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.payload_json.len() > MAX_TEXT_BYTES * 4 {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome =
            self.read_repository(|connection| oracle_repo::insert_twin_run(connection, &row));
        self.resolve_repository(outcome)
    }

    fn list_twin_run_payloads(&mut self, limit: u32) -> Result<Vec<String>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome = self
            .read_repository(|connection| oracle_repo::list_twin_run_payloads(connection, limit));
        self.resolve_repository(outcome)
    }

    fn latest_twin_run_payload(&mut self) -> Result<Option<String>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome = self
            .read_repository(|connection| oracle_repo::latest_twin_run_payload(connection));
        self.resolve_repository(outcome)
    }

    fn insert_oracle_run(&mut self, row: OracleRunRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.payload_json.len() > MAX_TEXT_BYTES * 4
            || row.provenance_json.len() > MAX_TEXT_BYTES
        {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome =
            self.read_repository(|connection| oracle_repo::insert_oracle_run(connection, &row));
        self.resolve_repository(outcome)
    }

    fn latest_oracle_run(&mut self) -> Result<Option<OracleRunRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome =
            self.read_repository(|connection| oracle_repo::latest_oracle_run(connection));
        self.resolve_repository(outcome)
    }

    fn put_interview_session(&mut self, row: InterviewSessionRow) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.payload_json.len() > MAX_TEXT_BYTES * 4
            || row.artifact_json.len() > MAX_TEXT_BYTES * 4
            || row.artifact_fingerprint.len() > 128
        {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self
            .read_repository(|connection| oracle_repo::put_interview_session(connection, &row));
        self.resolve_repository(outcome)
    }

    fn get_interview_session(
        &mut self,
        id: String,
    ) -> Result<Option<InterviewSessionRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome = self
            .read_repository(|connection| oracle_repo::get_interview_session(connection, &id));
        self.resolve_repository(outcome)
    }

    fn insert_distortion_tags(
        &mut self,
        rows: Vec<DistortionTagRow>,
    ) -> Result<usize, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        for row in &rows {
            if row.snippet.len() > MAX_TEXT_BYTES
                || row.category.len() > 128
                || row.source_kind.len() > 64
                || row.source_id.len() > 512
                || row.run_id.len() > 128
                || row.id.len() > 128
            {
                return Err(VaultErrorCode::InvalidInput);
            }
        }
        let outcome = self
            .read_repository(|connection| distortion_repo::insert_distortion_tags(connection, &rows));
        self.resolve_repository(outcome)
    }

    fn list_distortion_tags(&mut self, limit: u32) -> Result<Vec<DistortionTagRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let lim = limit.clamp(1, 10_000);
        let outcome = self
            .read_repository(|connection| distortion_repo::list_distortion_tags(connection, lim));
        self.resolve_repository(outcome)
    }

    fn insert_purchase(
        &mut self,
        purchase: PurchaseRow,
        lines: Vec<PurchaseLineRow>,
    ) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if purchase.id.len() > 128
            || purchase.merchant_norm.len() > 512
            || purchase.active_distortions_json.len() > MAX_TEXT_BYTES
        {
            return Err(VaultErrorCode::InvalidInput);
        }
        for line in &lines {
            if line.id.len() > 128
                || line.purchase_id.len() > 128
                || line.item_name.len() > 512
            {
                return Err(VaultErrorCode::InvalidInput);
            }
        }
        let outcome = self.read_repository(|connection| {
            purchase_repo::insert_purchase_with_lines(connection, &purchase, &lines)
        });
        self.resolve_repository(outcome)
    }

    fn list_purchases_in_range(
        &mut self,
        start_unix: i64,
        end_unix: i64,
    ) -> Result<Vec<PurchaseRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if end_unix < start_unix {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.read_repository(|connection| {
            purchase_repo::list_purchases_in_range(connection, start_unix, end_unix)
        });
        self.resolve_repository(outcome)
    }

    fn list_purchases_recent(
        &mut self,
        limit: u32,
    ) -> Result<Vec<PurchaseRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome = self.read_repository(|connection| {
            purchase_repo::list_purchases_recent(connection, limit)
        });
        self.resolve_repository(outcome)
    }

    fn upsert_commitment(
        &mut self,
        row: CommitmentRow,
    ) -> Result<(), VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if row.id.len() > 128
            || row.condition_json.len() > MAX_TEXT_BYTES
            || row.action_type.len() > 64
            || row.custom_prompt.len() > MAX_TEXT_BYTES
            || row.source_relation_id.len() > 256
            || row.origin.len() > 64
        {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.read_repository(|connection| {
            commitment_repo::upsert_commitment(connection, &row)
        });
        self.resolve_repository(outcome)
    }

    fn list_commitments(
        &mut self,
        limit: u32,
    ) -> Result<Vec<CommitmentRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let lim = limit.clamp(1, 1_000);
        let outcome = self
            .read_repository(|connection| commitment_repo::list_commitments(connection, lim));
        self.resolve_repository(outcome)
    }

    fn list_enabled_commitments(
        &mut self,
    ) -> Result<Vec<CommitmentRow>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        let outcome = self
            .read_repository(|connection| commitment_repo::list_enabled_commitments(connection));
        self.resolve_repository(outcome)
    }

    fn find_commitment_by_source(
        &mut self,
        source_relation_id: String,
    ) -> Result<Option<String>, VaultErrorCode> {
        gate(snapshot_status(&self.status))?;
        if source_relation_id.len() > 256 {
            return Err(VaultErrorCode::InvalidInput);
        }
        let outcome = self.read_repository(|connection| {
            commitment_repo::find_id_by_source_relation(connection, &source_relation_id)
        });
        self.resolve_repository(outcome)
    }

    /// Borrow the live connection for a read. `None` means no connection is
    /// present (a status/connection desync), surfaced later as `Unavailable`.
    fn read_repository<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, RepositoryError>,
    ) -> Option<Result<T, RepositoryError>> {
        self.connection.as_ref().map(operation)
    }

    /// Run a write inside one IMMEDIATE transaction. Begin/commit failures are
    /// classified with the same Data-Protection-aware mapping as the repository
    /// body, so an OS-sealed file is detected at every boundary.
    fn write_repository<T>(
        &mut self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, RepositoryError>,
    ) -> Option<Result<T, RepositoryError>> {
        let connection = self.connection.as_mut()?;
        let transaction = match connection.transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(transaction) => transaction,
            Err(error) => return Some(Err(repository::map_storage_error(error))),
        };
        let value = match operation(&transaction) {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        match transaction.commit() {
            Ok(()) => Some(Ok(value)),
            Err(error) => Some(Err(repository::map_storage_error(error))),
        }
    }

    /// Convert a repository outcome to the IPC result. A `DataProtection` error
    /// engages a fail-closed self-lock: the sealed connection is dropped and the
    /// public status falls back to `Locked`, so the next access must re-run the
    /// OS user-presence ceremony (docs/m3_action_plan.md §0, §8).
    fn resolve_repository<T>(
        &mut self,
        outcome: Option<Result<T, RepositoryError>>,
    ) -> Result<T, VaultErrorCode> {
        match outcome {
            None => Err(VaultErrorCode::Unavailable),
            Some(Ok(value)) => Ok(value),
            Some(Err(RepositoryError::DataProtection)) => {
                self.connection.take();
                self.emit_error(VaultErrorCode::OsLockEngaged, VaultStatus::Locked);
                Err(VaultErrorCode::OsLockEngaged)
            }
            Some(Err(other)) => Err(map_repository_error(other)),
        }
    }
}

fn gate(status: VaultStatus) -> Result<(), VaultErrorCode> {
    match status {
        VaultStatus::Unlocked => Ok(()),
        VaultStatus::Quarantined | VaultStatus::RecoveryRequired | VaultStatus::OrphanedKey => {
            Err(VaultErrorCode::VaultQuarantined)
        }
        VaultStatus::Unavailable => Err(VaultErrorCode::Unavailable),
        VaultStatus::Unprovisioned
        | VaultStatus::Locked
        | VaultStatus::Unlocking
        | VaultStatus::Locking => Err(VaultErrorCode::Locked),
    }
}

fn validate_chat_create(mut input: ChatCreate) -> Result<ChatCreate, VaultErrorCode> {
    validate_uuid_v4(&input.id)?;
    validate_nonnegative(input.created_at)?;
    let title = input.title.trim();
    if title.is_empty() || title.len() > TITLE_MAX_BYTES {
        return Err(VaultErrorCode::InvalidInput);
    }
    input.title = title.to_string();
    Ok(input)
}

fn validate_message_append(input: MessageAppend) -> Result<MessageAppend, VaultErrorCode> {
    validate_uuid_v4(&input.id)?;
    validate_uuid_v4(&input.chat_id)?;
    if input.role != "user" && input.role != "assistant" {
        return Err(VaultErrorCode::InvalidInput);
    }
    if input.content.is_empty() || input.content.len() > REPOSITORY_CONTENT_MAX_BYTES {
        return Err(VaultErrorCode::InvalidInput);
    }
    validate_nonnegative(input.timestamp)?;
    Ok(input)
}

fn validate_uuid_v4(value: &str) -> Result<(), VaultErrorCode> {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
        || bytes[14] != b'4'
        || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
    {
        return Err(VaultErrorCode::InvalidInput);
    }

    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            continue;
        }
        if !matches!(byte, b'0'..=b'9' | b'a'..=b'f') {
            return Err(VaultErrorCode::InvalidInput);
        }
    }
    Ok(())
}

fn validate_nonnegative(value: i64) -> Result<(), VaultErrorCode> {
    if value < 0 {
        Err(VaultErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}

fn clamp_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT)
}

fn map_repository_error(error: RepositoryError) -> VaultErrorCode {
    match error {
        RepositoryError::NotFound => VaultErrorCode::NotFound,
        RepositoryError::Conflict => VaultErrorCode::Conflict,
        RepositoryError::StorageFailed => VaultErrorCode::StorageFailed,
        // Reached only if a caller bypasses `resolve_repository`; that path
        // performs the self-lock. Kept exhaustive and fail-closed regardless.
        RepositoryError::DataProtection => VaultErrorCode::OsLockEngaged,
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
        // OS-sealed storage (iOS Data Protection) is recoverable: fall back to
        // Locked and require re-authentication. It must NOT quarantine, which is
        // reserved for true corruption / wrong-key states.
        VaultConnectionError::OsAccessDenied => {
            (VaultStatus::Locked, VaultErrorCode::OsLockEngaged)
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
        VaultConnectionError::OpenFailed
        | VaultConnectionError::CipherIdentityUnavailable
        | VaultConnectionError::SqliteVecUnavailable => {
            (VaultStatus::Unavailable, VaultErrorCode::Unavailable)
        }
    }
}

fn classify_migration_error(error: MigrationError) -> (VaultStatus, VaultErrorCode) {
    match error {
        MigrationError::UnsupportedVersion { .. } | MigrationError::InvalidVersion { .. } => {
            (VaultStatus::Quarantined, VaultErrorCode::UnsupportedSchema)
        }
        MigrationError::VersionReadFailed
        | MigrationError::MissingMigration { .. }
        | MigrationError::ForeignKeysEnableFailed
        | MigrationError::TransactionBeginFailed { .. }
        | MigrationError::MigrationApplyFailed { .. }
        | MigrationError::SchemaMismatch { .. }
        | MigrationError::VersionWriteFailed { .. }
        | MigrationError::CommitFailed { .. } => {
            (VaultStatus::Quarantined, VaultErrorCode::VaultQuarantined)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT_ID: &str = "00000000-0000-4000-8000-000000000001";
    const MESSAGE_ID: &str = "10000000-0000-4000-8000-000000000001";

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

    #[test]
    fn repository_gate_uses_public_status() {
        assert_eq!(gate(VaultStatus::Unlocked), Ok(()));
        assert_eq!(gate(VaultStatus::Locked), Err(VaultErrorCode::Locked));
        assert_eq!(
            gate(VaultStatus::Quarantined),
            Err(VaultErrorCode::VaultQuarantined)
        );
        assert_eq!(
            gate(VaultStatus::Unavailable),
            Err(VaultErrorCode::Unavailable)
        );
    }

    #[test]
    fn canonical_uuid_v4_validation_is_strict() {
        assert_eq!(validate_uuid_v4(CHAT_ID), Ok(()));
        assert_eq!(
            validate_uuid_v4("00000000-0000-4000-8000-00000000000A"),
            Err(VaultErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_uuid_v4("00000000-0000-5000-8000-000000000001"),
            Err(VaultErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_uuid_v4("00000000-0000-4000-7000-000000000001"),
            Err(VaultErrorCode::InvalidInput)
        );
    }

    #[test]
    fn chat_validation_trims_and_rejects_invalid_fields() {
        let valid = validate_chat_create(ChatCreate {
            id: CHAT_ID.to_string(),
            title: "  title  ".to_string(),
            created_at: 0,
        });
        assert_eq!(
            valid,
            Ok(ChatCreate {
                id: CHAT_ID.to_string(),
                title: "title".to_string(),
                created_at: 0,
            })
        );
        assert_eq!(
            validate_chat_create(ChatCreate {
                id: "invalid".to_string(),
                title: "title".to_string(),
                created_at: 0,
            }),
            Err(VaultErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_chat_create(ChatCreate {
                id: CHAT_ID.to_string(),
                title: "   ".to_string(),
                created_at: 0,
            }),
            Err(VaultErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_chat_create(ChatCreate {
                id: CHAT_ID.to_string(),
                title: "x".repeat(TITLE_MAX_BYTES + 1),
                created_at: 0,
            }),
            Err(VaultErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_chat_create(ChatCreate {
                id: CHAT_ID.to_string(),
                title: "title".to_string(),
                created_at: -1,
            }),
            Err(VaultErrorCode::InvalidInput)
        );
    }

    #[test]
    fn message_validation_rejects_role_content_and_timestamp_violations() {
        let valid = MessageAppend {
            id: MESSAGE_ID.to_string(),
            chat_id: CHAT_ID.to_string(),
            role: "user".to_string(),
            content: "content".to_string(),
            timestamp: 0,
        };
        assert_eq!(validate_message_append(valid.clone()), Ok(valid.clone()));

        let mut invalid = valid.clone();
        invalid.role = "system".to_string();
        assert_eq!(
            validate_message_append(invalid),
            Err(VaultErrorCode::InvalidInput)
        );
        let mut invalid = valid.clone();
        invalid.content.clear();
        assert_eq!(
            validate_message_append(invalid),
            Err(VaultErrorCode::InvalidInput)
        );
        let mut invalid = valid.clone();
        invalid.content = "x".repeat(REPOSITORY_CONTENT_MAX_BYTES + 1);
        assert_eq!(
            validate_message_append(invalid),
            Err(VaultErrorCode::InvalidInput)
        );
        let mut invalid = valid;
        invalid.timestamp = -1;
        assert_eq!(
            validate_message_append(invalid),
            Err(VaultErrorCode::InvalidInput)
        );
    }

    #[test]
    fn list_limits_are_bounded() {
        assert_eq!(clamp_limit(None), DEFAULT_LIST_LIMIT);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(1)), 1);
        assert_eq!(clamp_limit(Some(MAX_LIST_LIMIT + 1)), MAX_LIST_LIMIT);
    }

    #[test]
    fn worker_executes_repository_only_while_unlocked() -> Result<(), VaultErrorCode> {
        let mut connection =
            Connection::open_in_memory().map_err(|_| VaultErrorCode::StorageFailed)?;
        run_migrations(&mut connection).map_err(|_| VaultErrorCode::StorageFailed)?;
        let status = Arc::new(Mutex::new(VaultStatus::Unlocked));
        let mut worker = VaultWorker {
            database_path: PathBuf::new(),
            connection: Some(connection),
            status: Arc::clone(&status),
            event_sink: None,
        };

        let created = worker.create_chat(ChatCreate {
            id: CHAT_ID.to_string(),
            title: "  Chat  ".to_string(),
            created_at: 1,
        })?;
        assert_eq!(created.title, "Chat");
        assert_eq!(worker.list_chats(None)?.len(), 1);

        publish_status(&status, VaultStatus::Quarantined);
        assert_eq!(
            worker.list_chats(None),
            Err(VaultErrorCode::VaultQuarantined)
        );
        Ok(())
    }

    #[test]
    fn data_protection_error_self_locks_and_drops_connection() -> Result<(), VaultErrorCode> {
        let mut connection =
            Connection::open_in_memory().map_err(|_| VaultErrorCode::StorageFailed)?;
        run_migrations(&mut connection).map_err(|_| VaultErrorCode::StorageFailed)?;
        let status = Arc::new(Mutex::new(VaultStatus::Unlocked));
        let mut worker = VaultWorker {
            database_path: PathBuf::new(),
            connection: Some(connection),
            status: Arc::clone(&status),
            event_sink: None,
        };

        // Model an OS access denial surfacing mid-operation (iOS Data Protection
        // sealing the file while the device is locked).
        let outcome: Option<Result<(), RepositoryError>> =
            Some(Err(RepositoryError::DataProtection));
        let result = worker.resolve_repository(outcome);

        assert_eq!(result, Err(VaultErrorCode::OsLockEngaged));
        assert!(
            worker.connection.is_none(),
            "sealed connection must be dropped"
        );
        assert_eq!(snapshot_status(&status), VaultStatus::Locked);
        Ok(())
    }

    #[test]
    fn os_access_denied_open_error_locks_without_quarantine() {
        // A device-locked open/verify must be recoverable (Locked), never
        // misclassified as corruption (Quarantined).
        assert_eq!(
            classify_connection_error(VaultConnectionError::OsAccessDenied),
            (VaultStatus::Locked, VaultErrorCode::OsLockEngaged)
        );
        assert_eq!(
            classify_connection_error(VaultConnectionError::SchemaVerificationFailed),
            (VaultStatus::Quarantined, VaultErrorCode::CorruptOrWrongKey)
        );
    }
}
