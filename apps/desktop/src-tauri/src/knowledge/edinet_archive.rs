//! EDINET Step 5–6 — bounded stream-to-temp + ZIP EOCD/CD preflight.
//!
//! Downloads a document ZIP chunk-by-chunk into a temp file (never a full
//! `Vec<u8>`). Wikipedia's shared `MAX_RESPONSE_BYTES = 1MiB` is untouched;
//! this module owns the EDINET-only compressed-archive cap and the Step 6
//! EOCD / central-directory preflight (no extraction). Heavy Coordinator,
//! XBRL/CSV parsing and RAG embed are later steps.
//!
//! Invariants (docs/EDINET_LANE_IMPLEMENTATION_DIRECTIVE.md §Step 5–6):
//! - measured cumulative bytes enforce the cap (`Content-Length` is advisory);
//! - deadline / cancel / memory pressure are checked every chunk **and** around
//!   GET / write_all / flush / rewind (one absolute Instant from before GET);
//! - `HttpTransport::get(..., request_deadline)` receives the same budget so
//!   production reqwest cannot kill a legitimate large ZIP at a short fixed
//!   client timeout;
//! - a partial file is never success — error/cancel/timeout/drop delete it;
//! - success flushes, rewinds, and moves ownership into [`TempArchive`];
//! - temp files live under the caller-supplied dir (production: Tauri app
//!   cache dir) and their names never contain API key / company / docID;
//! - at most ONE archive download / `TempArchive` at a time ([`ArchiveGate`]);
//! - [`preflight_edinet_archive`] inspects EOCD in a fixed trailing buffer
//!   **before** any `ZipArchive::new()` / extract; ZIP64, multi-disk, and
//!   CD/entry caps are fail-closed.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::knowledge::edinet_client::{
    build_document_download_url, classify_document_response_meta, parse_document_api_error_status,
    validate_document_download_url, verify_zip_local_file_magic, DocumentResponseClass,
    EdinetDocumentKind, EdinetError,
};
use crate::knowledge::net_gateway::{
    fetch_bounded_with_deadline, GatewayError, HttpTransport, ResponseBody,
};

/// Compressed-archive cap (design V3 §4.1 initial limits). EDINET-only;
/// unrelated to the Wikipedia 1MiB gateway cap.
pub const MAX_EDINET_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Central-directory size cap (V3 §4.1). Checked from EOCD before ZipArchive.
pub const MAX_CENTRAL_DIRECTORY_BYTES: u64 = 2 * 1024 * 1024;

/// Max ZIP entries declared in EOCD (V3 §4.1).
pub const MAX_ZIP_ENTRIES: u16 = 512;

/// Default per-download deadline.
pub const DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE: Duration = Duration::from_secs(120);

/// EOCD minimum size (no comment).
const EOCD_MIN_LEN: usize = 22;
/// ZIP max comment length (u16) — bounds the trailing EOCD search window.
const ZIP_MAX_COMMENT_LEN: usize = 65_535;
/// Fixed trailing buffer: EOCD (22) + max comment (65535). Never grows with archive size.
const EOCD_SEARCH_WINDOW: usize = EOCD_MIN_LEN + ZIP_MAX_COMMENT_LEN;

const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06]; // PK\x05\x06
const ZIP64_LOCATOR_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x07]; // PK\x06\x07
const CENTRAL_DIR_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02]; // PK\x01\x02

/// Successful Step 6 EOCD / CD-bounds preflight (no entry bodies extracted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipPreflight {
    pub entry_count: u16,
    pub central_directory_bytes: u32,
    pub central_directory_offset: u32,
}

/// Single-flight gate: at most one archive download / live [`TempArchive`].
/// `type=5` and `type=1` are strictly serial — the second download can only
/// start after the first `TempArchive` is dropped (extract → delete).
#[derive(Debug, Default)]
pub struct ArchiveGate {
    busy: AtomicBool,
}

impl ArchiveGate {
    pub const fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
        }
    }

    /// Non-blocking acquire; `None` while another download/TempArchive lives.
    fn try_acquire(self: &Arc<Self>) -> Option<ArchivePermit> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(ArchivePermit {
                gate: Arc::clone(self),
            })
        } else {
            None
        }
    }

    /// Test/diagnostic visibility of the single-flight state.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

/// RAII permit for the single archive slot (released on drop).
#[derive(Debug)]
pub struct ArchivePermit {
    gate: Arc<ArchiveGate>,
}

impl Drop for ArchivePermit {
    fn drop(&mut self) {
        self.gate.busy.store(false, Ordering::Release);
    }
}

/// Process-wide production gate (one archive at a time across the whole app).
pub fn production_archive_gate() -> Arc<ArchiveGate> {
    static GATE: std::sync::OnceLock<Arc<ArchiveGate>> = std::sync::OnceLock::new();
    Arc::clone(GATE.get_or_init(|| Arc::new(ArchiveGate::new())))
}

/// A fully-downloaded, magic-verified, rewound archive temp file.
///
/// Owns the temp path (deleted on drop) and the single-flight permit, so the
/// next download in the `type=5` → `type=1` sequence can only start after
/// this value is dropped.
#[derive(Debug)]
pub struct TempArchive {
    file: std::fs::File,
    path: tempfile::TempPath,
    len: u64,
    kind: EdinetDocumentKind,
    _permit: ArchivePermit,
}

impl TempArchive {
    /// Rewound file handle for the (Step 6) preflight/extract reader.
    pub fn file(&mut self) -> &mut std::fs::File {
        &mut self.file
    }

    /// Measured archive size in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn kind(&self) -> EdinetDocumentKind {
        self.kind
    }

    /// Temp-file path (random name; no docID/company/key). Deleted on drop.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Step 6 EOCD / CD preflight; rewinds the file on both success and error.
    pub fn preflight(&mut self) -> Result<ZipPreflight, EdinetError> {
        let result = preflight_zip_file(&mut self.file, self.len);
        // Always leave the handle rewound for the next consumer (or drop).
        let _ = self.file.seek(SeekFrom::Start(0));
        result
    }
}

fn read_u16_le(buf: &[u8], at: usize) -> Option<u16> {
    let slice = buf.get(at..at.checked_add(2)?)?;
    let b0 = *slice.first()?;
    let b1 = *slice.get(1)?;
    Some(u16::from_le_bytes([b0, b1]))
}

fn read_u32_le(buf: &[u8], at: usize) -> Option<u32> {
    let slice = buf.get(at..at.checked_add(4)?)?;
    let b0 = *slice.first()?;
    let b1 = *slice.get(1)?;
    let b2 = *slice.get(2)?;
    let b3 = *slice.get(3)?;
    Some(u32::from_le_bytes([b0, b1, b2, b3]))
}

fn find_structural_eocd_rel(buf: &[u8], archive_len: u64, window_start: u64) -> Option<usize> {
    // Scan trailing window from the end. A ZIP comment may contain raw
    // `PK\x05\x06` bytes — only accept a candidate whose comment_len reaches EOF:
    //   window_start + candidate + 22 + comment_len == archive_len
    if buf.len() < EOCD_MIN_LEN {
        return None;
    }
    let mut i = buf.len().saturating_sub(EOCD_MIN_LEN);
    loop {
        if let Some(sig) = buf.get(i..i.saturating_add(4)) {
            if sig == EOCD_SIG.as_slice() {
                if let Some(comment_len) = read_u16_le(buf, i.saturating_add(20)) {
                    let eocd_abs = window_start.saturating_add(i as u64);
                    if let Some(end) = eocd_abs
                        .checked_add(EOCD_MIN_LEN as u64)
                        .and_then(|v| v.checked_add(u64::from(comment_len)))
                    {
                        if end == archive_len {
                            return Some(i);
                        }
                    }
                }
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

/// EOCD / CD-bounds preflight on an open archive file (no `ZipArchive::new`,
/// no entry extraction). `archive_len` must be the measured on-disk size.
pub fn preflight_zip_file(
    file: &mut std::fs::File,
    archive_len: u64,
) -> Result<ZipPreflight, EdinetError> {
    if archive_len < EOCD_MIN_LEN as u64 {
        return Err(EdinetError::InvalidZip);
    }
    if archive_len > MAX_EDINET_ARCHIVE_BYTES {
        return Err(EdinetError::TooLarge);
    }

    let window = core::cmp::min(archive_len, EOCD_SEARCH_WINDOW as u64) as usize;
    let start = archive_len.saturating_sub(window as u64);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| EdinetError::TempFileIo)?;
    let mut buf = vec![0u8; window];
    file.read_exact(&mut buf)
        .map_err(|_| EdinetError::InvalidZip)?;

    // Do NOT scan the whole trailing window for ZIP64/EOCD signatures — ZIP
    // comments are arbitrary bytes. Select EOCD structurally first.
    let eocd_rel =
        find_structural_eocd_rel(&buf, archive_len, start).ok_or(EdinetError::InvalidZip)?;
    let eocd = buf.get(eocd_rel..).ok_or(EdinetError::InvalidZip)?;
    if eocd.len() < EOCD_MIN_LEN {
        return Err(EdinetError::InvalidZip);
    }

    let disk_number = read_u16_le(eocd, 4).ok_or(EdinetError::InvalidZip)?;
    let disk_with_cd = read_u16_le(eocd, 6).ok_or(EdinetError::InvalidZip)?;
    let entries_on_disk = read_u16_le(eocd, 8).ok_or(EdinetError::InvalidZip)?;
    let total_entries = read_u16_le(eocd, 10).ok_or(EdinetError::InvalidZip)?;
    let cd_size = read_u32_le(eocd, 12).ok_or(EdinetError::InvalidZip)?;
    let cd_offset = read_u32_le(eocd, 16).ok_or(EdinetError::InvalidZip)?;
    let comment_len = read_u16_le(eocd, 20).ok_or(EdinetError::InvalidZip)?;

    let eocd_abs = start.saturating_add(eocd_rel as u64);
    // Structural ZIP64 locator sits in the 20 bytes immediately before EOCD.
    // Comment/CD bytes that happen to contain PK\x06\x06 / PK\x06\x07 must not
    // trip this — only the EOCD-relative slot is inspected.
    const ZIP64_LOCATOR_LEN: u64 = 20;
    if eocd_abs >= ZIP64_LOCATOR_LEN {
        let mut locator_head = [0u8; 4];
        file.seek(SeekFrom::Start(eocd_abs.saturating_sub(ZIP64_LOCATOR_LEN)))
            .map_err(|_| EdinetError::TempFileIo)?;
        file.read_exact(&mut locator_head)
            .map_err(|_| EdinetError::InvalidZip)?;
        if locator_head == ZIP64_LOCATOR_SIG {
            return Err(EdinetError::UnsupportedArchive);
        }
    }

    // Multi-disk / spanning archives are out of scope.
    if disk_number != 0 || disk_with_cd != 0 || entries_on_disk != total_entries {
        return Err(EdinetError::UnsupportedArchive);
    }

    // ZIP64 sentinel values in classic EOCD fields.
    if disk_number == 0xffff
        || disk_with_cd == 0xffff
        || entries_on_disk == 0xffff
        || total_entries == 0xffff
        || cd_size == 0xffff_ffff
        || cd_offset == 0xffff_ffff
    {
        return Err(EdinetError::UnsupportedArchive);
    }

    if u64::from(cd_size) > MAX_CENTRAL_DIRECTORY_BYTES {
        return Err(EdinetError::TooLarge);
    }
    if total_entries > MAX_ZIP_ENTRIES {
        return Err(EdinetError::TooLarge);
    }

    // CD must sit entirely before the EOCD and within the file.
    let cd_end = u64::from(cd_offset)
        .checked_add(u64::from(cd_size))
        .ok_or(EdinetError::InvalidZip)?;
    if cd_end > eocd_abs {
        return Err(EdinetError::InvalidZip);
    }

    // Empty archive (0 entries / 0 CD) is structurally valid.
    if total_entries == 0 {
        if cd_size != 0 || cd_offset != 0 {
            return Err(EdinetError::InvalidZip);
        }
        return Ok(ZipPreflight {
            entry_count: 0,
            central_directory_bytes: 0,
            central_directory_offset: 0,
        });
    }

    if cd_size == 0 {
        return Err(EdinetError::InvalidZip);
    }

    // Bound-check the CD start signature without loading the full directory.
    file.seek(SeekFrom::Start(u64::from(cd_offset)))
        .map_err(|_| EdinetError::TempFileIo)?;
    let mut cd_magic = [0u8; 4];
    file.read_exact(&mut cd_magic)
        .map_err(|_| EdinetError::InvalidZip)?;
    if cd_magic != CENTRAL_DIR_SIG {
        return Err(EdinetError::InvalidZip);
    }

    // comment_len already validated by structural EOCD selection.
    let _ = comment_len;

    Ok(ZipPreflight {
        entry_count: total_entries,
        central_directory_bytes: cd_size,
        central_directory_offset: cd_offset,
    })
}

/// Step 6 entry point: preflight a downloaded [`TempArchive`].
pub fn preflight_edinet_archive(archive: &mut TempArchive) -> Result<ZipPreflight, EdinetError> {
    archive.preflight()
}

fn map_gateway(e: GatewayError) -> EdinetError {
    EdinetError::Gateway(e)
}

/// Stream one EDINET document archive to a temp file with hard bounds.
///
/// Absolute deadline starts **before** `transport.get` and covers GET wait,
/// every `next_chunk`, every `write_all`, and final flush/rewind. The same
/// budget is passed into `HttpTransport::get` so production reqwest timeout
/// cannot silently truncate a legitimate large ZIP at a shorter fixed client
/// default. `memory_pressure` is polled before every chunk write. `temp_dir`
/// must be under the Tauri app cache directory in production.
#[allow(clippy::too_many_arguments)]
pub async fn download_edinet_archive_to_temp<T, F>(
    transport: &T,
    doc_id: &str,
    kind: EdinetDocumentKind,
    subscription_key: &str,
    temp_dir: &Path,
    gate: &Arc<ArchiveGate>,
    cancel: &tokio_util::sync::CancellationToken,
    deadline: Duration,
    max_compressed_bytes: u64,
    memory_pressure: F,
) -> Result<TempArchive, EdinetError>
where
    T: HttpTransport,
    F: Fn() -> bool,
{
    download_edinet_archive_to_temp_inner(
        transport,
        doc_id,
        kind,
        subscription_key,
        temp_dir,
        gate,
        cancel,
        deadline,
        max_compressed_bytes,
        memory_pressure,
        HangWriteMode::Never,
    )
    .await
}

/// Test-only write injection: production always uses [`HangWriteMode::Never`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HangWriteMode {
    Never,
    /// First `write_all` future pending forever (deadline/cancel must win).
    #[cfg(test)]
    FirstWrite,
}

#[allow(clippy::too_many_arguments)]
async fn download_edinet_archive_to_temp_inner<T, F>(
    transport: &T,
    doc_id: &str,
    kind: EdinetDocumentKind,
    subscription_key: &str,
    temp_dir: &Path,
    gate: &Arc<ArchiveGate>,
    cancel: &tokio_util::sync::CancellationToken,
    deadline: Duration,
    max_compressed_bytes: u64,
    memory_pressure: F,
    hang_write: HangWriteMode,
) -> Result<TempArchive, EdinetError>
where
    T: HttpTransport,
    F: Fn() -> bool,
{
    // Single-flight FIRST: no HTTP GET may start while another archive lives.
    let permit = gate.try_acquire().ok_or(EdinetError::TempArchiveBusy)?;

    let url = build_document_download_url(doc_id, kind, subscription_key)?;
    validate_document_download_url(&url, doc_id.trim(), kind)?;

    // One absolute budget from before GET through flush/rewind.
    let deadline_at = tokio::time::Instant::now() + deadline;

    let (meta, body) = tokio::select! {
        _ = cancel.cancelled() => return Err(map_gateway(GatewayError::Cancelled)),
        _ = tokio::time::sleep_until(deadline_at) => {
            return Err(map_gateway(GatewayError::Timeout));
        }
        result = transport.get(&url, deadline) => result.map_err(map_gateway)?,
    };

    match classify_document_response_meta(&meta)? {
        DocumentResponseClass::ApiErrorJson => {
            let remaining = deadline_at.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(map_gateway(GatewayError::Timeout));
            }
            let bytes = tokio::select! {
                _ = cancel.cancelled() => return Err(map_gateway(GatewayError::Cancelled)),
                _ = tokio::time::sleep_until(deadline_at) => {
                    return Err(map_gateway(GatewayError::Timeout));
                }
                result = fetch_bounded_with_deadline(
                    body,
                    std::future::pending::<()>(),
                    remaining,
                ) => result.map_err(map_gateway)?,
            };
            Err(EdinetError::ApiResponse {
                status: parse_document_api_error_status(&bytes),
            })
        }
        DocumentResponseClass::ExpectedZip => {
            if let Some(declared) = meta.content_length {
                if declared > max_compressed_bytes {
                    return Err(EdinetError::TooLarge);
                }
            }
            stream_zip_body_to_temp(
                body,
                kind,
                temp_dir,
                permit,
                cancel,
                deadline_at,
                max_compressed_bytes,
                memory_pressure,
                hang_write,
            )
            .await
        }
    }
}

/// Chunk loop: temp file is created only after meta classification, and its
/// `TempPath` guarantees deletion on every non-success path (incl. panic-free
/// early returns) because success moves it into the returned [`TempArchive`].
async fn stream_zip_body_to_temp<B, F>(
    mut body: B,
    kind: EdinetDocumentKind,
    temp_dir: &Path,
    permit: ArchivePermit,
    cancel: &tokio_util::sync::CancellationToken,
    deadline_at: tokio::time::Instant,
    max_compressed_bytes: u64,
    memory_pressure: F,
    hang_write: HangWriteMode,
) -> Result<TempArchive, EdinetError>
where
    B: ResponseBody,
    F: Fn() -> bool,
{
    // Random name only — never docID / company name / API key.
    let named = tempfile::Builder::new()
        .prefix("edinet-archive-")
        .suffix(".part")
        .tempfile_in(temp_dir)
        .map_err(|_| EdinetError::TempFileIo)?;
    let (std_file, temp_path) = named.into_parts();
    let mut file = tokio::fs::File::from_std(std_file);

    let mut total: u64 = 0;
    let mut magic_prefix: Vec<u8> = Vec::with_capacity(4);
    let mut writes_done: usize = 0;

    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(map_gateway(GatewayError::Cancelled)),
            _ = tokio::time::sleep_until(deadline_at) => {
                return Err(map_gateway(GatewayError::Timeout));
            }
            chunk = body.next_chunk() => chunk,
        };
        let chunk = match chunk {
            None => break,
            Some(Err(e)) => return Err(map_gateway(e)),
            Some(Ok(c)) => c,
        };
        if memory_pressure() {
            return Err(EdinetError::MemoryPressure);
        }
        // Measured cap: reject BEFORE writing the offending chunk.
        total = total.saturating_add(chunk.len() as u64);
        if total > max_compressed_bytes {
            return Err(EdinetError::TooLarge);
        }
        if magic_prefix.len() < 4 {
            let need = 4usize.saturating_sub(magic_prefix.len());
            magic_prefix.extend(chunk.iter().take(need));
        }

        let hang_this_write = match hang_write {
            HangWriteMode::Never => false,
            #[cfg(test)]
            HangWriteMode::FirstWrite => writes_done == 0,
        };
        tokio::select! {
            _ = cancel.cancelled() => return Err(map_gateway(GatewayError::Cancelled)),
            _ = tokio::time::sleep_until(deadline_at) => {
                return Err(map_gateway(GatewayError::Timeout));
            }
            result = async {
                if hang_this_write {
                    std::future::pending::<()>().await;
                }
                file.write_all(&chunk).await
            } => {
                result.map_err(|_| EdinetError::TempFileIo)?;
            }
        }
        writes_done = writes_done.saturating_add(1);
    }

    // EOF is not success by itself: ZIP magic must hold (content-type alone
    // was already checked; short/HTML-ish bodies die here).
    verify_zip_local_file_magic(&magic_prefix)?;

    tokio::select! {
        _ = cancel.cancelled() => return Err(map_gateway(GatewayError::Cancelled)),
        _ = tokio::time::sleep_until(deadline_at) => {
            return Err(map_gateway(GatewayError::Timeout));
        }
        result = file.flush() => {
            result.map_err(|_| EdinetError::TempFileIo)?;
        }
    }
    tokio::select! {
        _ = cancel.cancelled() => return Err(map_gateway(GatewayError::Cancelled)),
        _ = tokio::time::sleep_until(deadline_at) => {
            return Err(map_gateway(GatewayError::Timeout));
        }
        result = file.rewind() => {
            result.map_err(|_| EdinetError::TempFileIo)?;
        }
    }
    let std_file = file.into_std().await;

    Ok(TempArchive {
        file: std_file,
        path: temp_path,
        len: total,
        kind,
        _permit: permit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::net_gateway::ResponseMeta;
    use std::io::Read;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    struct ChunkBody {
        chunks: Vec<Result<Vec<u8>, GatewayError>>,
        /// When true, hang forever after the scripted chunks (deadline tests).
        hang_after: bool,
    }

    impl ResponseBody for ChunkBody {
        fn next_chunk(
            &mut self,
        ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
        {
            let next = if self.chunks.is_empty() {
                None
            } else {
                Some(self.chunks.remove(0))
            };
            let hang = next.is_none() && self.hang_after;
            async move {
                if hang {
                    std::future::pending::<()>().await;
                }
                next
            }
        }
    }

    struct ArchiveTransport {
        meta: ResponseMeta,
        chunks: Mutex<Vec<Result<Vec<u8>, GatewayError>>>,
        hang_after: bool,
        /// When true, `get` never returns (absolute deadline must stop it).
        hang_get: bool,
        gets: AtomicUsize,
    }

    impl ArchiveTransport {
        fn zip_ok(chunks: Vec<Result<Vec<u8>, GatewayError>>) -> Self {
            Self {
                meta: ResponseMeta {
                    status: 200,
                    content_type: Some("application/octet-stream".into()),
                    content_encoding: Some("identity".into()),
                    content_length: None,
                },
                chunks: Mutex::new(chunks),
                hang_after: false,
                hang_get: false,
                gets: AtomicUsize::new(0),
            }
        }
    }

    impl HttpTransport for ArchiveTransport {
        type Body = ChunkBody;
        fn get(
            &self,
            _url: &str,
            _request_deadline: Duration,
        ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
        {
            self.gets.fetch_add(1, Ordering::SeqCst);
            let meta = self.meta.clone();
            let chunks = self
                .chunks
                .lock()
                .map(|mut c| std::mem::take(&mut *c))
                .unwrap_or_default();
            let hang_after = self.hang_after;
            let hang_get = self.hang_get;
            async move {
                if hang_get {
                    std::future::pending::<()>().await;
                }
                Ok((meta, ChunkBody { chunks, hang_after }))
            }
        }
    }

    fn test_dir() -> tempfile::TempDir {
        tempfile::TempDir::new().unwrap_or_else(|e| panic!("tempdir: {e}"))
    }

    fn no_pressure() -> bool {
        false
    }

    async fn run_download(
        transport: &ArchiveTransport,
        dir: &Path,
        gate: &Arc<ArchiveGate>,
        cancel: &tokio_util::sync::CancellationToken,
        deadline: Duration,
        max_bytes: u64,
        pressure: fn() -> bool,
    ) -> Result<TempArchive, EdinetError> {
        download_edinet_archive_to_temp(
            transport,
            "S100ARCH",
            EdinetDocumentKind::FilingAndXbrl,
            "test-key",
            dir,
            gate,
            cancel,
            deadline,
            max_bytes,
            pressure,
        )
        .await
    }

    fn leftover_files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|it| it.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn happy_path_streams_chunks_rewinds_and_deletes_on_drop() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![
            Ok(b"PK".to_vec()),
            Ok(b"\x03\x04".to_vec()),
            Ok(b"payload-bytes".to_vec()),
        ]);

        let mut archive = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .unwrap_or_else(|e| panic!("download: {e}"));

        assert_eq!(archive.len(), 17);
        assert_eq!(archive.kind(), EdinetDocumentKind::FilingAndXbrl);
        assert!(gate.is_busy(), "permit held while TempArchive lives");
        let name = archive
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        assert!(name.starts_with("edinet-archive-"));
        assert!(!name.contains("S100ARCH"), "docID must not leak into name");
        assert!(!name.contains("test-key"), "key must not leak into name");

        // Rewound: reading from the handle yields the full body from byte 0.
        let mut read_back = Vec::new();
        archive
            .file()
            .read_to_end(&mut read_back)
            .unwrap_or_else(|e| panic!("read: {e}"));
        assert_eq!(read_back, b"PK\x03\x04payload-bytes");

        let path = archive.path().to_path_buf();
        assert!(path.exists());
        drop(archive);
        assert!(!path.exists(), "RAII delete on drop");
        assert!(!gate.is_busy(), "permit released on drop");
        assert!(leftover_files(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn declared_content_length_over_cap_rejects_before_body_read() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut transport = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04".to_vec())]);
        transport.meta.content_length = Some(MAX_EDINET_ARCHIVE_BYTES + 1);

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("declared too large");
        assert_eq!(err, EdinetError::TooLarge);
        assert!(leftover_files(dir.path()).is_empty(), "no temp created");
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn measured_bytes_over_cap_reject_even_with_small_declared_length() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        // Lying Content-Length: declares 4, streams 24 against a 16-byte cap.
        let mut transport =
            ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04".to_vec()), Ok(vec![0u8; 20])]);
        transport.meta.content_length = Some(4);

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            16,
            no_pressure,
        )
        .await
        .expect_err("measured too large");
        assert_eq!(err, EdinetError::TooLarge);
        assert!(leftover_files(dir.path()).is_empty(), "partial deleted");
        assert!(!gate.is_busy());
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_fires_and_deletes_partial_file() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut transport = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04part".to_vec())]);
        transport.hang_after = true;

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_millis(50),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("deadline");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Timeout));
        assert!(leftover_files(dir.path()).is_empty(), "partial deleted");
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn cancel_token_aborts_and_deletes_partial_file() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let mut transport = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04part".to_vec())]);
        transport.hang_after = true;

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("cancelled");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Cancelled));
        assert!(leftover_files(dir.path()).is_empty());
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn memory_pressure_between_chunks_aborts_and_deletes() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport =
            ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04".to_vec()), Ok(b"more".to_vec())]);

        fn always_pressure() -> bool {
            true
        }
        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            always_pressure,
        )
        .await
        .expect_err("pressure");
        assert_eq!(err, EdinetError::MemoryPressure);
        assert!(leftover_files(dir.path()).is_empty());
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn chunk_error_propagates_and_deletes_partial_file() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![
            Ok(b"PK\x03\x04".to_vec()),
            Err(GatewayError::WireViolation),
        ]);

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("wire error");
        assert_eq!(err, EdinetError::Gateway(GatewayError::WireViolation));
        assert!(leftover_files(dir.path()).is_empty());
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn bad_zip_magic_after_full_stream_is_invalid_zip() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![Ok(b"<html>Sorry</html>".to_vec())]);

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("bad magic");
        assert_eq!(err, EdinetError::InvalidZip);
        assert!(leftover_files(dir.path()).is_empty());
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn empty_body_is_invalid_zip_not_success() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![]);

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("empty body");
        assert_eq!(err, EdinetError::InvalidZip);
        assert!(leftover_files(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn json_error_envelope_maps_to_api_response_without_temp_file() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport {
            meta: ResponseMeta {
                status: 200,
                content_type: Some("application/json".into()),
                content_encoding: None,
                content_length: None,
            },
            chunks: Mutex::new(vec![Ok(
                br#"{"metadata":{"status":"404"},"results":null}"#.to_vec()
            )]),
            hang_after: false,
            hang_get: false,
            gets: AtomicUsize::new(0),
        };

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("api error");
        assert_eq!(
            err,
            EdinetError::ApiResponse {
                status: "404".into()
            }
        );
        assert!(leftover_files(dir.path()).is_empty(), "no temp for JSON");
        assert!(!gate.is_busy());
    }

    #[tokio::test]
    async fn second_download_is_busy_with_zero_http_while_archive_lives() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04data".to_vec())]);

        let archive = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .unwrap_or_else(|e| panic!("first: {e}"));
        let gets_after_first = transport.gets.load(Ordering::SeqCst);
        assert_eq!(gets_after_first, 1);

        let second = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04two".to_vec())]);
        let err = run_download(
            &second,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("busy");
        assert_eq!(err, EdinetError::TempArchiveBusy);
        assert_eq!(
            second.gets.load(Ordering::SeqCst),
            0,
            "busy check must precede any HTTP GET"
        );

        // Serial type=5 → type=1: after drop the next download succeeds.
        drop(archive);
        let third = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04three".to_vec())]);
        let ok = download_edinet_archive_to_temp(
            &third,
            "S100ARCH",
            EdinetDocumentKind::XbrlCsv,
            "test-key",
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .unwrap_or_else(|e| panic!("after drop: {e}"));
        assert_eq!(ok.kind(), EdinetDocumentKind::XbrlCsv);
        drop(ok);
        assert!(leftover_files(dir.path()).is_empty(), "temp leftover 0");
    }

    #[tokio::test]
    async fn html_content_type_is_rejected_without_temp_file() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport {
            meta: ResponseMeta {
                status: 200,
                content_type: Some("text/html".into()),
                content_encoding: None,
                content_length: None,
            },
            chunks: Mutex::new(vec![Ok(b"<html>Sorry</html>".to_vec())]),
            hang_after: false,
            hang_get: false,
            gets: AtomicUsize::new(0),
        };

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("html");
        assert_eq!(err, EdinetError::InvalidContentType);
        assert!(leftover_files(dir.path()).is_empty());
        assert!(!gate.is_busy());
    }

    #[tokio::test(start_paused = true)]
    async fn hanging_get_is_stopped_by_absolute_deadline_before_headers() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut transport = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04".to_vec())]);
        transport.hang_get = true;

        let err = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_millis(50),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .expect_err("hanging get");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Timeout));
        assert!(leftover_files(dir.path()).is_empty(), "no temp on hang-get");
        assert!(!gate.is_busy());
        assert_eq!(transport.gets.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn hanging_write_all_is_stopped_by_absolute_deadline() {
        let dir = test_dir();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![Ok(b"PK\x03\x04payload".to_vec())]);

        let err = download_edinet_archive_to_temp_inner(
            &transport,
            "S100ARCH",
            EdinetDocumentKind::FilingAndXbrl,
            "test-key",
            dir.path(),
            &gate,
            &cancel,
            Duration::from_millis(50),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
            HangWriteMode::FirstWrite,
        )
        .await
        .expect_err("hanging write");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Timeout));
        assert!(leftover_files(dir.path()).is_empty(), "partial deleted");
        assert!(!gate.is_busy());
    }

    // ----- Step 6: EOCD / CD preflight (no extract) -----

    fn write_temp_bytes(dir: &Path, bytes: &[u8]) -> (std::fs::File, u64) {
        let path = dir.join("fixture.zip");
        std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write: {e}"));
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("open: {e}"));
        (file, bytes.len() as u64)
    }

    /// Minimal stored ZIP with one empty entry named `a`.
    fn minimal_one_entry_zip() -> Vec<u8> {
        let mut out = Vec::new();
        // Local file header
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes()); // version
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc
        out.extend_from_slice(&0u32.to_le_bytes()); // comp
        out.extend_from_slice(&0u32.to_le_bytes()); // uncomp
        out.extend_from_slice(&1u16.to_le_bytes()); // name len
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.push(b'a');
        let cd_offset = out.len() as u32;
        // Central directory
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        out.push(b'a');
        let cd_size = (out.len() as u32).saturating_sub(cd_offset);
        // EOCD
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn eocd_only(entries: u16, cd_size: u32, cd_offset: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&entries.to_le_bytes());
        out.extend_from_slice(&entries.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn preflight_accepts_minimal_stored_zip() {
        let dir = test_dir();
        let bytes = minimal_one_entry_zip();
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        let pf = preflight_zip_file(&mut file, len).unwrap_or_else(|e| panic!("preflight: {e}"));
        assert_eq!(pf.entry_count, 1);
        assert!(pf.central_directory_bytes > 0);
        // Rewound by caller of TempArchive::preflight; raw helper leaves offset
        // at CD magic — TempArchive::preflight always seeks Start(0).
    }

    #[test]
    fn preflight_accepts_empty_archive_eocd_only() {
        let dir = test_dir();
        let bytes = eocd_only(0, 0, 0);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        let pf = preflight_zip_file(&mut file, len).unwrap_or_else(|e| panic!("empty: {e}"));
        assert_eq!(
            pf,
            ZipPreflight {
                entry_count: 0,
                central_directory_bytes: 0,
                central_directory_offset: 0,
            }
        );
    }

    #[test]
    fn preflight_rejects_missing_eocd() {
        let dir = test_dir();
        let (mut file, len) = write_temp_bytes(dir.path(), b"PK\x03\x04not-a-complete-zip");
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::InvalidZip)
        );
    }

    fn zip_with_comment(base: &[u8], comment: &[u8]) -> Vec<u8> {
        assert!(
            comment.len() <= ZIP_MAX_COMMENT_LEN,
            "comment exceeds ZIP max"
        );
        // `base` must end with EOCD comment_len == 0 (last 2 bytes).
        let mut out = base.to_vec();
        let len = out.len();
        assert!(len >= EOCD_MIN_LEN, "base too short for EOCD");
        let cl = comment.len() as u16;
        if let Some(b0) = out.get_mut(len - 2) {
            *b0 = (cl & 0xff) as u8;
        }
        if let Some(b1) = out.get_mut(len - 1) {
            *b1 = (cl >> 8) as u8;
        }
        out.extend_from_slice(comment);
        out
    }

    #[test]
    fn preflight_accepts_eocd_signature_inside_zip_comment() {
        let dir = test_dir();
        // Comment embeds a decoy EOCD signature; structural comment_len must win.
        let comment = b"decoy PK\x05\x06 not the real end";
        let bytes = zip_with_comment(&minimal_one_entry_zip(), comment);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        let pf = preflight_zip_file(&mut file, len).unwrap_or_else(|e| panic!("accept: {e}"));
        assert_eq!(pf.entry_count, 1);
    }

    #[test]
    fn preflight_accepts_zip64_signatures_inside_zip_comment() {
        let dir = test_dir();
        let comment = b"noise PK\x06\x06 and PK\x06\x07 decoys in comment only";
        let bytes = zip_with_comment(&minimal_one_entry_zip(), comment);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        let pf = preflight_zip_file(&mut file, len).unwrap_or_else(|e| panic!("accept: {e}"));
        assert_eq!(pf.entry_count, 1);
    }

    #[test]
    fn preflight_rejects_zip64_locator_immediately_before_eocd() {
        let dir = test_dir();
        // ZIP64 end-of-central-directory locator immediately before a classic EOCD.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PK\x06\x07"); // locator sig
        bytes.extend_from_slice(&0u32.to_le_bytes()); // disk with zip64 eocd
        bytes.extend_from_slice(&0u64.to_le_bytes()); // relative offset
        bytes.extend_from_slice(&1u32.to_le_bytes()); // total disks
        bytes.extend_from_slice(&eocd_only(0, 0, 0));
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::UnsupportedArchive)
        );
    }

    #[test]
    fn preflight_rejects_zip64_sentinel_entry_count() {
        let dir = test_dir();
        let bytes = eocd_only(0xffff, 0, 0);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::UnsupportedArchive)
        );
    }

    #[test]
    fn preflight_rejects_zip64_sentinel_cd_size() {
        let dir = test_dir();
        let bytes = eocd_only(1, 0xffff_ffff, 0);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::UnsupportedArchive)
        );
    }

    #[test]
    fn preflight_rejects_multi_disk() {
        let dir = test_dir();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&1u16.to_le_bytes()); // disk number != 0
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::UnsupportedArchive)
        );
    }

    #[test]
    fn preflight_rejects_central_directory_over_cap() {
        let dir = test_dir();
        let over = (MAX_CENTRAL_DIRECTORY_BYTES + 1) as u32;
        // EOCD claims a huge CD but ZIP64 sentinels are not used — TooLarge.
        let bytes = eocd_only(1, over, 0);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::TooLarge)
        );
    }

    #[test]
    fn preflight_rejects_entry_count_over_cap() {
        let dir = test_dir();
        let over = MAX_ZIP_ENTRIES.saturating_add(1);
        // Need a plausible CD size so we don't fail earlier on empty/nonzero mismatch.
        // cd_size=46 keeps us under 2MiB; CD magic check will fail unless we skip —
        // entry cap is checked before CD magic, so this is fine.
        let bytes = eocd_only(over, 46, 0);
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::TooLarge)
        );
    }

    #[test]
    fn preflight_rejects_cd_past_eocd() {
        let dir = test_dir();
        // cd_offset + cd_size extends past EOCD.
        let bytes = eocd_only(1, 100, 0);
        // File is only 22 bytes (EOCD); cd_end=100 > eocd_abs=0 → InvalidZip.
        let (mut file, len) = write_temp_bytes(dir.path(), &bytes);
        assert_eq!(
            preflight_zip_file(&mut file, len),
            Err(EdinetError::InvalidZip)
        );
    }

    #[tokio::test]
    async fn temp_archive_preflight_rewinds_on_success() {
        let dir = test_dir();
        let bytes = minimal_one_entry_zip();
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let transport = ArchiveTransport::zip_ok(vec![Ok(bytes)]);
        let mut archive = run_download(
            &transport,
            dir.path(),
            &gate,
            &cancel,
            Duration::from_secs(5),
            MAX_EDINET_ARCHIVE_BYTES,
            no_pressure,
        )
        .await
        .unwrap_or_else(|e| panic!("dl: {e}"));
        let pf = archive.preflight().unwrap_or_else(|e| panic!("pf: {e}"));
        assert_eq!(pf.entry_count, 1);
        // After preflight the handle must be at byte 0 (local header magic).
        let mut magic = [0u8; 4];
        archive
            .file()
            .read_exact(&mut magic)
            .unwrap_or_else(|e| panic!("read: {e}"));
        assert_eq!(&magic, b"PK\x03\x04");
    }
}
