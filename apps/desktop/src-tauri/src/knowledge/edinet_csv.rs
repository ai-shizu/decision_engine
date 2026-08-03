//! EDINET Step 7 — bounded financial TSV extraction (`type=5`).
//!
//! Runs only after Step 6 EOCD/CD preflight succeeds. Streams one
//! `XBRL_TO_CSV/*.csv` entry (UTF-16LE, tab, CRLF, quoted) through
//! `encoding_rs` + `csv-core` with fixed buffers. Never loads the TSV into a
//! single `Vec`, never uses the high-level `csv` crate, and does not touch
//! XBRL/XHTML, Heavy Coordinator, or RAG.
//!
//! Limits (V3 §4.1 / directive §Step 7 / Step 7 approval):
//! - single TSV entry ≤ [`MAX_SINGLE_TSV_BYTES`] (64 MiB)
//! - field output buffer = [`FIELD_BUF_BYTES`] (8 KiB); `OutputFull` ⇒ discard
//!   row + warning (no newline-search skip)
//! - record total ≤ [`MAX_RECORD_BYTES`] (64 KiB)
//! - rows ≤ [`MAX_TSV_ROWS`] (500_000)
//!
//! [`TempArchive`]'s permit is borrowed for the whole extract — the caller
//! must not drop the archive until this function returns.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::io::Read;
use std::time::{Duration, Instant};

use csv_core::{ReadFieldResult, ReaderBuilder as CsvReaderBuilder};
use encoding_rs::UTF_16LE;
use encoding_rs_io::DecodeReaderBytesBuilder;
use tokio_util::sync::CancellationToken;
use zip::ZipArchive;

use crate::knowledge::edinet_archive::{preflight_edinet_archive, TempArchive, ZipPreflight};
use crate::knowledge::edinet_client::{EdinetDocumentKind, EdinetError};
use crate::knowledge::net_gateway::GatewayError;

/// Uncompressed single TSV entry cap (V3 §4.1) — **raw** ZIP entry bytes
/// (UTF-16LE), measured before decode.
pub const MAX_SINGLE_TSV_BYTES: u64 = 64 * 1024 * 1024;
/// Fixed per-field output buffer for `csv-core` (approval).
pub const FIELD_BUF_BYTES: usize = 8 * 1024;
/// Max sum of field bytes in one record (approval).
pub const MAX_RECORD_BYTES: usize = 64 * 1024;
/// Max data rows (excluding header) to scan.
pub const MAX_TSV_ROWS: u64 = 500_000;
/// Read chunk into the UTF-8 side of the decoder / raw probe.
const INPUT_CHUNK_BYTES: usize = 8 * 1024;

const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];

#[derive(Clone, Copy, Debug)]
struct TsvLimits {
    max_raw_bytes: u64,
    max_rows: u64,
}

const PROD_TSV_LIMITS: TsvLimits = TsvLimits {
    max_raw_bytes: MAX_SINGLE_TSV_BYTES,
    max_rows: MAX_TSV_ROWS,
};

const EXPECTED_HEADERS: [&str; 9] = [
    "要素ID",
    "項目名",
    "コンテキストID",
    "相対年度",
    "連結・個別",
    "期間・時点",
    "ユニットID",
    "単位",
    "値",
];

/// Initial allowlisted financial concepts (local-name suffix match).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FinancialConcept {
    Revenue,
    OperatingIncome,
    ProfitLoss,
    Assets,
    Equity,
    OperatingCashFlow,
}

impl FinancialConcept {
    fn from_element_id(element_id: &str) -> Option<Self> {
        // Element IDs look like `jppfs_cor_NetSales` — match on the local suffix
        // only; never invent mappings from Japanese labels alone.
        let local = element_id.rsplit('_').next().unwrap_or(element_id);
        match local {
            "NetSales"
            | "NetSalesSummaryOfBusinessResults"
            | "Revenue"
            | "RevenueFromContractsWithCustomersIFRS"
            | "NetSalesIFRS" => Some(Self::Revenue),
            "OperatingIncome"
            | "OperatingIncomeLoss"
            | "OperatingProfitLoss"
            | "OperatingIncomeLossIFRS" => Some(Self::OperatingIncome),
            "ProfitLoss"
            | "ProfitLossAttributableToOwnersOfParent"
            | "ProfitLossAttributableToOwnersOfParentSummaryOfBusinessResults"
            | "ProfitLossIFRS" => Some(Self::ProfitLoss),
            "Assets" | "AssetsIFRS" => Some(Self::Assets),
            "NetAssets" | "Equity" | "EquityAttributableToOwnersOfParent" | "EquityIFRS" => {
                Some(Self::Equity)
            }
            "CashFlowsFromUsedInOperatingActivities"
            | "CashFlowsFromUsedInOperatingActivitiesIFRS" => Some(Self::OperatingCashFlow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consolidation {
    Consolidated,
    NonConsolidated,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeYear {
    Current,
    Prior,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodKind {
    Duration,
    Instant,
    Unknown,
}

/// One selected financial cell (concept + context + unit + value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedFinancialFact {
    pub concept: FinancialConcept,
    pub element_id: String,
    pub label: String,
    pub context_id: String,
    pub relative_year: RelativeYear,
    pub consolidation: Consolidation,
    pub period_kind: PeriodKind,
    pub unit_id: String,
    pub unit_label: String,
    pub value_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EdinetWarning {
    /// `csv-core` returned `OutputFull` — row discarded (no newline skip).
    OversizedFieldDiscardedRow,
    /// Record field-byte total exceeded [`MAX_RECORD_BYTES`].
    OversizedRecordDiscardedRow,
    /// Row contained U+FFFD after UTF-16 decode.
    ReplacementCharDiscardedRow,
    /// Not exactly 9 columns / header mismatch / malformed.
    MalformedRow,
    /// Same concept had multiple equally-best contexts — left unfetched.
    AmbiguousConcept,
    /// No `XBRL_TO_CSV/*.csv` entry found after preflight.
    NoEligibleTsv,
    /// Hit [`MAX_TSV_ROWS`] before EOF.
    RowLimitReached,
    /// No eligible `XBRL/PublicDoc` narrative entry (Step 8).
    NoEligibleXbrl,
    /// Entry soft-failed (DOCTYPE / too-large event / malformed); try next.
    XbrlEntrySoftFail,
    /// `continuedAt` target missing or cyclic after bounds.
    ContinuedAtUnresolved,
    /// Narrative text hit section byte cap.
    NarrativeTruncated,
    /// Evidence section failed sanitize — that section only was discarded (Step 9).
    EvidenceSanitizeFailed,
    /// Evidence aggregate budget (256 KiB) hit — section truncated or dropped,
    /// never silently (Step 9).
    EvidenceBudgetExceeded,
}

/// Step 7/8/9 partial extract (Vault/embedding/Heavy Coordinator are Step 10+).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartialEdinetFacts {
    pub financials: Vec<ExtractedFinancialFact>,
    /// Narrative text blocks from type=1 Inline XBRL / XBRL (Step 8).
    pub narratives: Vec<ExtractedNarrative>,
    /// Sanitized RAG-bound raw evidence (Step 9) — never merged into
    /// `CompanyFacts`, whose fields keep their small display caps.
    pub evidence: Vec<EdinetEvidenceSection>,
    pub warnings: Vec<EdinetWarning>,
}

/// Allowlisted narrative concept (local-name match; no prefix hard-coding).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NarrativeConcept {
    BusinessRisks,
    DescriptionOfBusiness,
    ManagementAnalysis,
}

/// One extracted narrative section (parser output; evidence built in Step 9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedNarrative {
    pub concept: NarrativeConcept,
    pub local_name: String,
    pub text: String,
    pub truncated: bool,
}

/// RAG-bound raw evidence section (Step 9) — sanitized full text plus
/// provenance. Not stored in `CompanyFacts` (V3 §6 / directive §Step 9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdinetEvidenceSection {
    pub doc_id: String,
    pub edinet_code: String,
    /// Submission timestamp from the filing metadata (e.g. `submitDateTime`).
    pub submitted_at: String,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub concept: NarrativeConcept,
    pub local_name: String,
    /// Unit of measure — always `None` for narrative text blocks; reserved for
    /// numeric evidence (chunk-meta contract keeps concept/period/unit).
    pub unit: Option<String>,
    /// Sanitized text, ≤ `edinet_xbrl::MAX_SECTION_BYTES` (128 KiB).
    pub text: String,
    /// True if capture, continuation, sanitize, or the 256 KiB aggregate budget
    /// cut the text. Truncation is never silent — do not label as "全文".
    pub truncated: bool,
}

/// Provenance keys for later merge (not logged with secrets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinancialExtractMeta {
    pub doc_id: String,
    pub edinet_code: String,
    pub submitted_at: String,
}

/// Extract financial facts from a preflight-eligible `type=5` archive.
///
/// Calls [`preflight_edinet_archive`] first (fail-closed). Holds the
/// `&mut TempArchive` (and thus its single-flight permit) for the entire
/// duration — caller drops the archive only after this returns.
///
/// `cancel` is checked on ZIP entry scan, each raw entry read, and each CSV
/// row / drain iteration (blocking work cannot rely on abort alone).
pub fn extract_financials_from_type5_archive(
    archive: &mut TempArchive,
    _meta: &FinancialExtractMeta,
    parse_deadline: Duration,
    cancel: &CancellationToken,
) -> Result<PartialEdinetFacts, EdinetError> {
    extract_financials_from_type5_archive_with_limits(
        archive,
        parse_deadline,
        cancel,
        PROD_TSV_LIMITS,
    )
}

fn extract_financials_from_type5_archive_with_limits(
    archive: &mut TempArchive,
    parse_deadline: Duration,
    cancel: &CancellationToken,
    limits: TsvLimits,
) -> Result<PartialEdinetFacts, EdinetError> {
    if archive.kind() != EdinetDocumentKind::XbrlCsv {
        return Err(EdinetError::InvalidArgument);
    }
    check_deadline_cancel(Instant::now() + parse_deadline, cancel)?;
    let preflight = preflight_edinet_archive(archive)?;
    extract_financials_after_preflight(archive, &preflight, parse_deadline, cancel, limits)
}

fn extract_financials_after_preflight(
    archive: &mut TempArchive,
    _preflight: &ZipPreflight,
    parse_deadline: Duration,
    cancel: &CancellationToken,
    limits: TsvLimits,
) -> Result<PartialEdinetFacts, EdinetError> {
    let deadline_at = Instant::now() + parse_deadline;
    check_deadline_cancel(deadline_at, cancel)?;
    let file = archive.file();
    let mut zip = ZipArchive::new(file).map_err(|_| EdinetError::InvalidZip)?;

    let tsv_index = select_tsv_entry_index(&mut zip, deadline_at, cancel)?;
    let Some(tsv_index) = tsv_index else {
        return Ok(PartialEdinetFacts {
            financials: Vec::new(),
            narratives: Vec::new(),
            evidence: Vec::new(),
            warnings: vec![EdinetWarning::NoEligibleTsv],
        });
    };

    check_deadline_cancel(deadline_at, cancel)?;
    let entry = zip
        .by_index(tsv_index)
        .map_err(|_| EdinetError::InvalidZip)?;
    if entry.encrypted() {
        return Err(EdinetError::UnsupportedArchive);
    }
    match entry.compression() {
        zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => {}
        _ => return Err(EdinetError::UnsupportedArchive),
    }
    if entry.size() > limits.max_raw_bytes || entry.compressed_size() > limits.max_raw_bytes {
        return Err(EdinetError::TooLarge);
    }

    // Raw-byte meter wraps the ZIP entry *before* UTF-16 decode.
    let mut decoder = open_utf16le_tsv_decoder(entry, limits.max_raw_bytes)?;
    parse_financial_tsv_reader(&mut decoder, deadline_at, cancel, limits.max_rows)
}

fn check_deadline_cancel(
    deadline_at: Instant,
    cancel: &CancellationToken,
) -> Result<(), EdinetError> {
    if cancel.is_cancelled() {
        return Err(EdinetError::Gateway(GatewayError::Cancelled));
    }
    if Instant::now() >= deadline_at {
        return Err(EdinetError::Gateway(GatewayError::Timeout));
    }
    Ok(())
}

fn map_entry_io(err: std::io::Error) -> EdinetError {
    if err.kind() == std::io::ErrorKind::InvalidData {
        // Our raw-byte meter uses this kind + marker; ZIP CRC uses the same
        // kind with "Invalid checksum" — do not conflate with TooLarge.
        let msg = err.to_string();
        if msg.contains("edinet_tsv_too_large") {
            return EdinetError::TooLarge;
        }
        return EdinetError::InvalidZip;
    }
    EdinetError::Parse
}

/// Measure **raw** uncompressed entry bytes; exact `cap` + EOF is allowed,
/// `cap + 1` is `TooLarge`.
struct CountingRead<R> {
    inner: R,
    read: u64,
    cap: u64,
}

impl<R: Read> Read for CountingRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.read > self.cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "edinet_tsv_too_large",
            ));
        }
        if self.read == self.cap {
            // Exact boundary: EOF ⇒ Ok(0); any further raw byte ⇒ TooLarge.
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "edinet_tsv_too_large",
                )),
            };
        }
        let remaining = (self.cap - self.read) as usize;
        let want = buf.len().min(remaining);
        let Some(dest) = buf.get_mut(..want) else {
            return Ok(0);
        };
        let n = self.inner.read(dest)?;
        self.read = self.read.saturating_add(n as u64);
        Ok(n)
    }
}

/// Require UTF-16LE BOM (`FF FE`); reject missing BOM and UTF-16BE (`FE FF`).
fn verify_utf16le_bom<R: Read>(raw: &mut R) -> Result<(), EdinetError> {
    let mut bom = [0u8; 2];
    let mut filled = 0usize;
    while filled < 2 {
        match raw.read(bom.get_mut(filled..).unwrap_or(&mut [])) {
            Ok(0) => return Err(EdinetError::UnsupportedEncoding),
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) => return Err(map_entry_io(e)),
        }
    }
    if bom == UTF16LE_BOM {
        Ok(())
    } else if bom == UTF16BE_BOM {
        Err(EdinetError::UnsupportedEncoding)
    } else {
        Err(EdinetError::UnsupportedEncoding)
    }
}

/// Wrap a raw ZIP entry (or test cursor): count raw bytes → verify BOM → UTF-16LE decoder.
fn open_utf16le_tsv_decoder<R: Read>(raw: R, max_raw_bytes: u64) -> Result<impl Read, EdinetError> {
    let mut counting = CountingRead {
        inner: raw,
        read: 0,
        cap: max_raw_bytes,
    };
    verify_utf16le_bom(&mut counting)?;
    DecodeReaderBytesBuilder::new()
        .encoding(Some(UTF_16LE))
        .bom_sniffing(false)
        .build_with_buffer(counting, [0u8; INPUT_CHUNK_BYTES])
        .map_err(|_| EdinetError::Parse)
}

fn select_tsv_entry_index<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    deadline_at: Instant,
    cancel: &CancellationToken,
) -> Result<Option<usize>, EdinetError> {
    let mut chosen: Option<(usize, String)> = None;
    for i in 0..zip.len() {
        check_deadline_cancel(deadline_at, cancel)?;
        let entry = zip.by_index(i).map_err(|_| EdinetError::InvalidZip)?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let path_str = path.to_string_lossy();
        if !path_str.starts_with("XBRL_TO_CSV/") && !path_str.starts_with("XBRL_TO_CSV\\") {
            continue;
        }
        let lower = path_str.to_ascii_lowercase();
        if !lower.ends_with(".csv") {
            continue;
        }
        let name = path_str.to_string();
        match &chosen {
            None => chosen = Some((i, name)),
            Some((_, prev)) => {
                // Deterministic: lexicographically smaller enclosed path wins.
                if name < *prev {
                    chosen = Some((i, name));
                }
            }
        }
    }
    Ok(chosen.map(|(i, _)| i))
}

/// Drain remaining UTF-8 (and thus raw entry) to EOF so ZIP CRC can complete.
fn drain_reader_to_eof<R: Read>(
    reader: &mut R,
    deadline_at: Instant,
    cancel: &CancellationToken,
) -> Result<(), EdinetError> {
    let mut buf = [0u8; INPUT_CHUNK_BYTES];
    loop {
        check_deadline_cancel(deadline_at, cancel)?;
        match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(map_entry_io(e)),
        }
    }
}

/// Parse a UTF-8 TSV stream (already decoded) with `csv-core` bounds.
///
/// On `max_rows`, facts are kept only after the reader is drained to EOF
/// (CRC / raw-cap / cancel / deadline still apply).
pub fn parse_financial_tsv_reader<R: Read>(
    reader: &mut R,
    deadline_at: Instant,
    cancel: &CancellationToken,
    max_rows: u64,
) -> Result<PartialEdinetFacts, EdinetError> {
    let mut csv = CsvReaderBuilder::new().delimiter(b'\t').quote(b'"').build();

    let mut input = [0u8; INPUT_CHUNK_BYTES];
    let mut input_len = 0usize;
    let mut input_pos = 0usize;
    let mut field_buf = [0u8; FIELD_BUF_BYTES];
    // `csv-core` writes into the *suffix* of the caller buffer across
    // InputEmpty refills — accumulate at `field_out_pos` (see crate tests).
    let mut field_out_pos = 0usize;
    let mut eof = false;
    let mut warnings: Vec<EdinetWarning> = Vec::new();

    let mut header_done = false;
    let mut row_count: u64 = 0;
    let mut current: Vec<String> = Vec::with_capacity(9);
    let mut current_bytes: usize = 0;
    let mut discarding = false;
    let mut discard_bytes: u64 = 0;

    let mut best: Vec<(FinancialConcept, i32, ExtractedFinancialFact)> = Vec::new();
    let mut ambiguous: Vec<FinancialConcept> = Vec::new();

    loop {
        check_deadline_cancel(deadline_at, cancel)?;

        if input_pos >= input_len {
            if eof {
                let out_slice = if discarding {
                    field_buf.as_mut_slice()
                } else {
                    field_buf.get_mut(field_out_pos..).unwrap_or(&mut [])
                };
                let (res, _nin, nout) = csv.read_field(&[], out_slice);
                match res {
                    ReadFieldResult::End => break,
                    ReadFieldResult::InputEmpty => break,
                    ReadFieldResult::OutputFull => {
                        enter_discard(
                            &mut discarding,
                            &mut discard_bytes,
                            &mut field_out_pos,
                            &mut current,
                            &mut current_bytes,
                            &mut warnings,
                            nout,
                        )?;
                        continue;
                    }
                    ReadFieldResult::Field { record_end } => {
                        if discarding {
                            discard_bytes = discard_bytes.saturating_add(nout as u64);
                            check_discard_budget(discard_bytes)?;
                            field_out_pos = 0;
                            if record_end {
                                discarding = false;
                                discard_bytes = 0;
                            }
                        } else {
                            field_out_pos = field_out_pos.saturating_add(nout);
                            if let Err(w) = finish_field(
                                &mut current,
                                &mut current_bytes,
                                &field_buf,
                                field_out_pos,
                            ) {
                                discarding = true;
                                push_warning(&mut warnings, w);
                                current.clear();
                                current_bytes = 0;
                                field_out_pos = 0;
                                if record_end {
                                    discarding = false;
                                }
                            } else {
                                field_out_pos = 0;
                                if record_end {
                                    finish_record(
                                        &mut current,
                                        &mut current_bytes,
                                        &mut header_done,
                                        &mut best,
                                        &mut ambiguous,
                                        &mut warnings,
                                        &mut row_count,
                                    )?;
                                    if row_count >= max_rows {
                                        push_warning(&mut warnings, EdinetWarning::RowLimitReached);
                                        drain_reader_to_eof(reader, deadline_at, cancel)?;
                                        break;
                                    }
                                }
                            }
                        }
                        continue;
                    }
                }
            }
            check_deadline_cancel(deadline_at, cancel)?;
            match reader.read(&mut input) {
                Ok(0) => {
                    eof = true;
                    input_len = 0;
                    input_pos = 0;
                }
                Ok(n) => {
                    input_len = n;
                    input_pos = 0;
                }
                Err(e) => return Err(map_entry_io(e)),
            }
            continue;
        }

        let out_slice = if discarding {
            field_buf.as_mut_slice()
        } else {
            field_buf.get_mut(field_out_pos..).unwrap_or(&mut [])
        };
        let (res, nin, nout) =
            csv.read_field(input.get(input_pos..input_len).unwrap_or(&[]), out_slice);
        input_pos = input_pos.saturating_add(nin);

        match res {
            ReadFieldResult::InputEmpty => {
                if discarding {
                    discard_bytes = discard_bytes.saturating_add(nout as u64);
                    check_discard_budget(discard_bytes)?;
                    field_out_pos = 0;
                } else {
                    field_out_pos = field_out_pos.saturating_add(nout);
                }
            }
            ReadFieldResult::OutputFull => {
                enter_discard(
                    &mut discarding,
                    &mut discard_bytes,
                    &mut field_out_pos,
                    &mut current,
                    &mut current_bytes,
                    &mut warnings,
                    nout,
                )?;
            }
            ReadFieldResult::Field { record_end } => {
                if discarding {
                    discard_bytes = discard_bytes.saturating_add(nout as u64);
                    check_discard_budget(discard_bytes)?;
                    field_out_pos = 0;
                    if record_end {
                        discarding = false;
                        discard_bytes = 0;
                        current.clear();
                        current_bytes = 0;
                    }
                } else {
                    field_out_pos = field_out_pos.saturating_add(nout);
                    if let Err(w) =
                        finish_field(&mut current, &mut current_bytes, &field_buf, field_out_pos)
                    {
                        discarding = true;
                        push_warning(&mut warnings, w);
                        current.clear();
                        current_bytes = 0;
                        field_out_pos = 0;
                        if record_end {
                            discarding = false;
                            discard_bytes = 0;
                        }
                    } else {
                        field_out_pos = 0;
                        if record_end {
                            finish_record(
                                &mut current,
                                &mut current_bytes,
                                &mut header_done,
                                &mut best,
                                &mut ambiguous,
                                &mut warnings,
                                &mut row_count,
                            )?;
                            if row_count >= max_rows {
                                push_warning(&mut warnings, EdinetWarning::RowLimitReached);
                                // Remaining UTF-8 already pulled into `input` is
                                // abandoned; drain the decoder so the raw ZIP
                                // entry reaches EOF (CRC).
                                drain_reader_to_eof(reader, deadline_at, cancel)?;
                                break;
                            }
                        }
                    }
                }
            }
            ReadFieldResult::End => break,
        }
    }

    if !header_done {
        return Err(EdinetError::Parse);
    }

    if !ambiguous.is_empty() {
        push_warning(&mut warnings, EdinetWarning::AmbiguousConcept);
    }

    let financials = best
        .into_iter()
        .filter(|(c, _, _)| !ambiguous.contains(c))
        .map(|(_, _, f)| f)
        .collect();

    Ok(PartialEdinetFacts {
        financials,
        narratives: Vec::new(),
        evidence: Vec::new(),
        warnings,
    })
}

/// Soft-fail if discard churn exceeds one TSV entry budget (V3 §4.3).
fn check_discard_budget(discard_bytes: u64) -> Result<(), EdinetError> {
    if discard_bytes > MAX_SINGLE_TSV_BYTES {
        return Err(EdinetError::TooLarge);
    }
    Ok(())
}

fn enter_discard(
    discarding: &mut bool,
    discard_bytes: &mut u64,
    field_out_pos: &mut usize,
    current: &mut Vec<String>,
    current_bytes: &mut usize,
    warnings: &mut Vec<EdinetWarning>,
    nout: usize,
) -> Result<(), EdinetError> {
    if !*discarding {
        push_warning(warnings, EdinetWarning::OversizedFieldDiscardedRow);
    }
    *discarding = true;
    *discard_bytes = discard_bytes.saturating_add(nout as u64);
    *discard_bytes = discard_bytes.saturating_add(*field_out_pos as u64);
    *field_out_pos = 0;
    current.clear();
    *current_bytes = 0;
    check_discard_budget(*discard_bytes)
}

fn finish_field(
    current: &mut Vec<String>,
    current_bytes: &mut usize,
    field_buf: &[u8; FIELD_BUF_BYTES],
    field_len: usize,
) -> Result<(), EdinetWarning> {
    let bytes = field_buf.get(..field_len).unwrap_or(&[]);
    push_field(current, current_bytes, bytes)
}

fn finish_record(
    current: &mut Vec<String>,
    current_bytes: &mut usize,
    header_done: &mut bool,
    best: &mut Vec<(FinancialConcept, i32, ExtractedFinancialFact)>,
    ambiguous: &mut Vec<FinancialConcept>,
    warnings: &mut Vec<EdinetWarning>,
    row_count: &mut u64,
) -> Result<(), EdinetError> {
    if !*header_done {
        if !validate_header(current) {
            return Err(EdinetError::Parse);
        }
        *header_done = true;
    } else {
        handle_data_row(current, best, ambiguous, warnings);
        *row_count = row_count.saturating_add(1);
    }
    current.clear();
    *current_bytes = 0;
    Ok(())
}

fn push_warning(warnings: &mut Vec<EdinetWarning>, w: EdinetWarning) {
    // One-shot warnings stay unique; per-row discard/malformed may repeat (cap).
    let repeatable = matches!(
        w,
        EdinetWarning::OversizedFieldDiscardedRow
            | EdinetWarning::OversizedRecordDiscardedRow
            | EdinetWarning::ReplacementCharDiscardedRow
            | EdinetWarning::MalformedRow
    );
    if warnings.len() >= 64 {
        return;
    }
    if repeatable || !warnings.contains(&w) {
        warnings.push(w);
    }
}

fn push_field(
    current: &mut Vec<String>,
    current_bytes: &mut usize,
    bytes: &[u8],
) -> Result<(), EdinetWarning> {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Err(EdinetWarning::MalformedRow),
    };
    if text.contains('\u{FFFD}') {
        return Err(EdinetWarning::ReplacementCharDiscardedRow);
    }
    let add = text.len();
    if current_bytes.saturating_add(add) > MAX_RECORD_BYTES {
        return Err(EdinetWarning::OversizedRecordDiscardedRow);
    }
    *current_bytes = current_bytes.saturating_add(add);
    current.push(text.to_string());
    Ok(())
}

fn validate_header(fields: &[String]) -> bool {
    if fields.len() != EXPECTED_HEADERS.len() {
        return false;
    }
    fields
        .iter()
        .zip(EXPECTED_HEADERS.iter())
        .all(|(got, want)| got.trim() == *want)
}

fn parse_consolidation(s: &str) -> Consolidation {
    let t = s.trim();
    if t.contains('連') && t.contains('結') {
        Consolidation::Consolidated
    } else if t.contains('個') && t.contains('別') {
        Consolidation::NonConsolidated
    } else if t.eq_ignore_ascii_case("Consolidated") {
        Consolidation::Consolidated
    } else if t.eq_ignore_ascii_case("NonConsolidated")
        || t.eq_ignore_ascii_case("Non-Consolidated")
    {
        Consolidation::NonConsolidated
    } else {
        Consolidation::Unknown
    }
}

fn parse_relative_year(s: &str) -> RelativeYear {
    let t = s.trim();
    if t.contains('当') {
        RelativeYear::Current
    } else if t.contains('前') {
        RelativeYear::Prior
    } else if t.eq_ignore_ascii_case("CurrentYear") || t.eq_ignore_ascii_case("Current") {
        RelativeYear::Current
    } else if t.eq_ignore_ascii_case("PriorYear") || t.eq_ignore_ascii_case("Previous") {
        RelativeYear::Prior
    } else {
        RelativeYear::Other
    }
}

fn parse_period_kind(s: &str) -> PeriodKind {
    let t = s.trim();
    if t.contains('期') && t.contains('間') {
        PeriodKind::Duration
    } else if t.contains('時') && t.contains('点') {
        PeriodKind::Instant
    } else if t.eq_ignore_ascii_case("Duration") {
        PeriodKind::Duration
    } else if t.eq_ignore_ascii_case("Instant") {
        PeriodKind::Instant
    } else {
        PeriodKind::Unknown
    }
}

fn score_candidate(
    concept: FinancialConcept,
    relative: RelativeYear,
    consol: Consolidation,
    period: PeriodKind,
) -> Option<i32> {
    // Never select prior-period as "current" facts.
    if relative == RelativeYear::Prior {
        return None;
    }
    let mut score = 0i32;
    match relative {
        RelativeYear::Current => score += 100,
        RelativeYear::Other => score += 10,
        RelativeYear::Prior => return None,
    }
    match consol {
        Consolidation::Consolidated => score += 50,
        Consolidation::NonConsolidated => score += 10,
        Consolidation::Unknown => score += 0,
    }
    // Flow concepts prefer duration; stock concepts prefer instant.
    let prefer_duration = matches!(
        concept,
        FinancialConcept::Revenue
            | FinancialConcept::OperatingIncome
            | FinancialConcept::ProfitLoss
            | FinancialConcept::OperatingCashFlow
    );
    match (prefer_duration, period) {
        (true, PeriodKind::Duration) | (false, PeriodKind::Instant) => score += 20,
        (_, PeriodKind::Unknown) => score += 0,
        _ => score += 5,
    }
    Some(score)
}

fn handle_data_row(
    fields: &[String],
    best: &mut Vec<(FinancialConcept, i32, ExtractedFinancialFact)>,
    ambiguous: &mut Vec<FinancialConcept>,
    warnings: &mut Vec<EdinetWarning>,
) {
    if fields.len() != 9 {
        push_warning(warnings, EdinetWarning::MalformedRow);
        return;
    }
    let element_id = fields.first().map(|s| s.trim()).unwrap_or("");
    let label = fields.get(1).map(|s| s.trim()).unwrap_or("");
    let context_id = fields.get(2).map(|s| s.trim()).unwrap_or("");
    let relative_s = fields.get(3).map(|s| s.as_str()).unwrap_or("");
    let consol_s = fields.get(4).map(|s| s.as_str()).unwrap_or("");
    let period_s = fields.get(5).map(|s| s.as_str()).unwrap_or("");
    let unit_id = fields.get(6).map(|s| s.trim()).unwrap_or("");
    let unit_label = fields.get(7).map(|s| s.trim()).unwrap_or("");
    let value_text = fields.get(8).map(|s| s.trim()).unwrap_or("");

    if element_id.is_empty() || value_text.is_empty() {
        push_warning(warnings, EdinetWarning::MalformedRow);
        return;
    }
    if [element_id, label, context_id, value_text]
        .iter()
        .any(|s| s.contains('\u{FFFD}'))
    {
        push_warning(warnings, EdinetWarning::ReplacementCharDiscardedRow);
        return;
    }

    let Some(concept) = FinancialConcept::from_element_id(element_id) else {
        return;
    };
    if ambiguous.contains(&concept) {
        return;
    }

    let relative = parse_relative_year(relative_s);
    let consolidation = parse_consolidation(consol_s);
    let period_kind = parse_period_kind(period_s);
    let Some(score) = score_candidate(concept, relative, consolidation, period_kind) else {
        return;
    };

    let fact = ExtractedFinancialFact {
        concept,
        element_id: element_id.to_string(),
        label: label.to_string(),
        context_id: context_id.to_string(),
        relative_year: relative,
        consolidation,
        period_kind,
        unit_id: unit_id.to_string(),
        unit_label: unit_label.to_string(),
        value_text: value_text.to_string(),
    };

    match best.iter().position(|(c, _, _)| *c == concept) {
        Some(idx) => {
            let prev_score = best.get(idx).map(|(_, s, _)| *s).unwrap_or(0);
            if score > prev_score {
                if let Some(slot) = best.get_mut(idx) {
                    slot.1 = score;
                    slot.2 = fact;
                }
            } else if score == prev_score && !ambiguous.contains(&concept) {
                ambiguous.push(concept);
            }
        }
        None => best.push((concept, score, fact)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::edinet_archive::{ArchiveGate, MAX_EDINET_ARCHIVE_BYTES};
    use crate::knowledge::edinet_client::EdinetDocumentKind;
    use crate::knowledge::net_gateway::GatewayError;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn utf16le_bom_tsv(utf8: &str) -> Vec<u8> {
        let mut out = vec![0xff, 0xfe];
        for u in utf8.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    fn header_line() -> String {
        EXPECTED_HEADERS
            .iter()
            .map(|h| format!("\"{h}\""))
            .collect::<Vec<_>>()
            .join("\t")
    }

    fn data_line(
        element_id: &str,
        label: &str,
        context: &str,
        year: &str,
        consol: &str,
        period: &str,
        unit_id: &str,
        unit: &str,
        value: &str,
    ) -> String {
        [
            element_id, label, context, year, consol, period, unit_id, unit, value,
        ]
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join("\t")
    }

    fn live_cancel() -> CancellationToken {
        CancellationToken::new()
    }

    fn parse_utf8_tsv(utf8: &str) -> PartialEdinetFacts {
        let bytes = utf8.as_bytes();
        let mut cur = std::io::Cursor::new(bytes);
        let cancel = live_cancel();
        parse_financial_tsv_reader(
            &mut cur,
            Instant::now() + Duration::from_secs(5),
            &cancel,
            MAX_TSV_ROWS,
        )
        .unwrap_or_else(|e| panic!("parse: {e}"))
    }

    fn parse_utf16_raw(
        raw: &[u8],
        max_raw: u64,
        max_rows: u64,
        cancel: &CancellationToken,
    ) -> Result<PartialEdinetFacts, EdinetError> {
        let mut decoder = open_utf16le_tsv_decoder(std::io::Cursor::new(raw), max_raw)?;
        parse_financial_tsv_reader(
            &mut decoder,
            Instant::now() + Duration::from_secs(5),
            cancel,
            max_rows,
        )
    }

    #[test]
    fn selects_current_consolidated_over_prior_and_individual() {
        let tsv = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "PriorYearDuration",
                "前期",
                "連結",
                "期間",
                "JPY",
                "円",
                "1"
            ),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "CurrentYearDuration_NonConsolidatedMember",
                "当期",
                "個別",
                "期間",
                "JPY",
                "円",
                "2"
            ),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "100"
            ),
        );
        let out = parse_utf8_tsv(&tsv);
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("one fact");
        assert_eq!(fact.concept, FinancialConcept::Revenue);
        assert_eq!(fact.value_text, "100");
        assert_eq!(fact.unit_id, "JPY");
        assert_eq!(fact.consolidation, Consolidation::Consolidated);
    }

    #[test]
    fn ambiguous_equal_score_left_unfetched() {
        let tsv = format!(
            "{}\r\n{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_Assets",
                "資産",
                "CurrentYearInstant_A",
                "当期",
                "連結",
                "時点",
                "JPY",
                "円",
                "10"
            ),
            data_line(
                "jppfs_cor_Assets",
                "資産",
                "CurrentYearInstant_B",
                "当期",
                "連結",
                "時点",
                "JPY",
                "円",
                "20"
            ),
        );
        let out = parse_utf8_tsv(&tsv);
        assert!(out.financials.is_empty());
        assert!(out.warnings.contains(&EdinetWarning::AmbiguousConcept));
    }

    #[test]
    fn output_full_discards_row_with_warning_not_newline_skip() {
        // Field longer than 8 KiB inside quotes — OutputFull path.
        let big = "X".repeat(FIELD_BUF_BYTES + 64);
        let tsv = format!(
            "{}\r\n{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                &big,
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "999"
            ),
            data_line(
                "jppfs_cor_OperatingIncome",
                "営業利益",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "50"
            ),
        );
        let out = parse_utf8_tsv(&tsv);
        assert!(out
            .warnings
            .contains(&EdinetWarning::OversizedFieldDiscardedRow));
        // Oversized revenue row discarded; operating income still selected.
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("oi");
        assert_eq!(fact.concept, FinancialConcept::OperatingIncome);
        assert_eq!(fact.value_text, "50");
    }

    #[test]
    fn replacement_char_row_discarded() {
        let tsv = format!(
            "{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "1\u{FFFD}2"
            ),
        );
        let out = parse_utf8_tsv(&tsv);
        assert!(out.financials.is_empty());
        assert!(out
            .warnings
            .contains(&EdinetWarning::ReplacementCharDiscardedRow));
    }

    #[test]
    fn header_mismatch_is_parse_error() {
        let tsv = "\"a\"\t\"b\"\r\n\"1\"\t\"2\"\r\n";
        let mut cur = std::io::Cursor::new(tsv.as_bytes());
        let cancel = live_cancel();
        let err = parse_financial_tsv_reader(
            &mut cur,
            Instant::now() + Duration::from_secs(5),
            &cancel,
            MAX_TSV_ROWS,
        )
        .expect_err("bad header");
        assert_eq!(err, EdinetError::Parse);
    }

    #[test]
    fn utf16le_zip_entry_extracts_after_preflight() {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let tsv_utf8 = format!(
            "{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "12345"
            ),
        );
        let tsv_bytes = utf16le_bom_tsv(&tsv_utf8);

        // Build a minimal type=5 ZIP on disk.
        let zip_path = dir.path().join("t5.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap_or_else(|e| panic!("{e}"));
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("XBRL_TO_CSV/sample.csv", opts)
                .unwrap_or_else(|e| panic!("{e}"));
            zw.write_all(&tsv_bytes).unwrap_or_else(|e| panic!("{e}"));
            zw.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        let zip_bytes = std::fs::read(&zip_path).unwrap_or_else(|e| panic!("{e}"));

        // Reuse Step 5 download path to get a real TempArchive (permit held).
        use crate::knowledge::edinet_archive::download_edinet_archive_to_temp;
        use crate::knowledge::net_gateway::{
            GatewayError, HttpTransport, ResponseBody, ResponseMeta,
        };

        struct Body(Option<Vec<u8>>);
        impl ResponseBody for Body {
            fn next_chunk(
                &mut self,
            ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
            {
                let n = self.0.take().map(Ok);
                async move { n }
            }
        }
        struct Tr(Vec<u8>);
        impl HttpTransport for Tr {
            type Body = Body;
            fn get(
                &self,
                _url: &str,
                _d: Duration,
            ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
            {
                let body = self.0.clone();
                async move {
                    Ok((
                        ResponseMeta {
                            status: 200,
                            content_type: Some("application/octet-stream".into()),
                            content_encoding: Some("identity".into()),
                            content_length: None,
                        },
                        Body(Some(body)),
                    ))
                }
            }
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|e| panic!("{e}"));
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut archive = rt
            .block_on(download_edinet_archive_to_temp(
                &Tr(zip_bytes),
                "S100TSV1",
                EdinetDocumentKind::XbrlCsv,
                "test-key",
                dir.path(),
                &gate,
                &cancel,
                Duration::from_secs(5),
                MAX_EDINET_ARCHIVE_BYTES,
                || false,
            ))
            .unwrap_or_else(|e| panic!("dl: {e}"));

        assert!(gate.is_busy(), "permit held during extract");
        let meta = FinancialExtractMeta {
            doc_id: "S100TSV1".into(),
            edinet_code: "E02144".into(),
            submitted_at: "2024-06-25 15:00".into(),
        };
        let extract_cancel = live_cancel();
        let out = extract_financials_from_type5_archive(
            &mut archive,
            &meta,
            Duration::from_secs(5),
            &extract_cancel,
        )
        .unwrap_or_else(|e| panic!("extract: {e}"));
        assert!(gate.is_busy(), "permit still held until archive drop");
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("rev");
        assert_eq!(fact.value_text, "12345");
        drop(archive);
        assert!(!gate.is_busy());
    }

    #[test]
    fn oversized_record_discards_row() {
        // Each field < 8 KiB (avoid OutputFull) but sum > 64 KiB record budget.
        let chunk = "Y".repeat(7500);
        let fields = [
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
            chunk.as_str(),
        ];
        let row = fields
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join("\t");
        let tsv = format!(
            "{}\r\n{}\r\n{}\r\n",
            header_line(),
            row,
            data_line(
                "jppfs_cor_ProfitLoss",
                "利益",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "7"
            ),
        );
        let out = parse_utf8_tsv(&tsv);
        assert!(out
            .warnings
            .contains(&EdinetWarning::OversizedRecordDiscardedRow));
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("pl");
        assert_eq!(fact.concept, FinancialConcept::ProfitLoss);
        assert_eq!(fact.value_text, "7");
    }

    #[test]
    fn rejects_non_xbrl_csv_kind() {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let zip_path = dir.path().join("t1.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap_or_else(|e| panic!("{e}"));
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("dummy.txt", opts)
                .unwrap_or_else(|e| panic!("{e}"));
            zw.write_all(b"x").unwrap_or_else(|e| panic!("{e}"));
            zw.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        let zip_bytes = std::fs::read(&zip_path).unwrap_or_else(|e| panic!("{e}"));

        use crate::knowledge::edinet_archive::download_edinet_archive_to_temp;
        use crate::knowledge::net_gateway::{
            GatewayError, HttpTransport, ResponseBody, ResponseMeta,
        };

        struct Body(Option<Vec<u8>>);
        impl ResponseBody for Body {
            fn next_chunk(
                &mut self,
            ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
            {
                let n = self.0.take().map(Ok);
                async move { n }
            }
        }
        struct Tr(Vec<u8>);
        impl HttpTransport for Tr {
            type Body = Body;
            fn get(
                &self,
                _url: &str,
                _d: Duration,
            ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
            {
                let body = self.0.clone();
                async move {
                    Ok((
                        ResponseMeta {
                            status: 200,
                            content_type: Some("application/octet-stream".into()),
                            content_encoding: Some("identity".into()),
                            content_length: None,
                        },
                        Body(Some(body)),
                    ))
                }
            }
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|e| panic!("{e}"));
        let gate = Arc::new(ArchiveGate::new());
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut archive = rt
            .block_on(download_edinet_archive_to_temp(
                &Tr(zip_bytes),
                "S100KIND",
                EdinetDocumentKind::FilingAndXbrl,
                "test-key",
                dir.path(),
                &gate,
                &cancel,
                Duration::from_secs(5),
                MAX_EDINET_ARCHIVE_BYTES,
                || false,
            ))
            .unwrap_or_else(|e| panic!("dl: {e}"));
        let meta = FinancialExtractMeta {
            doc_id: "S100KIND".into(),
            edinet_code: "E02144".into(),
            submitted_at: "2024-06-25 15:00".into(),
        };
        let extract_cancel = live_cancel();
        let err = extract_financials_from_type5_archive(
            &mut archive,
            &meta,
            Duration::from_secs(5),
            &extract_cancel,
        )
        .expect_err("kind");
        assert_eq!(err, EdinetError::InvalidArgument);
        assert!(gate.is_busy());
        drop(archive);
    }

    #[test]
    fn field_spanning_input_chunks_accumulates() {
        // Label just under field cap, forced across multiple 8 KiB input fills.
        let label = "Z".repeat(FIELD_BUF_BYTES - 16);
        let tsv = format!(
            "{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                &label,
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "42"
            ),
        );
        assert!(tsv.len() > super::INPUT_CHUNK_BYTES);
        let out = parse_utf8_tsv(&tsv);
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("rev");
        assert_eq!(fact.label.len(), FIELD_BUF_BYTES - 16);
        assert_eq!(fact.value_text, "42");
    }

    #[test]
    fn counting_read_exact_cap_accepts_eof() {
        let raw = vec![0u8; 16];
        let mut c = CountingRead {
            inner: std::io::Cursor::new(raw),
            read: 0,
            cap: 16,
        };
        let mut buf = [0u8; 64];
        let mut total = 0usize;
        loop {
            let n = c.read(&mut buf).unwrap_or_else(|e| panic!("{e}"));
            if n == 0 {
                break;
            }
            total += n;
        }
        assert_eq!(total, 16);
        assert_eq!(c.read(&mut buf).unwrap_or_else(|e| panic!("{e}")), 0);
    }

    #[test]
    fn counting_read_rejects_one_byte_past_cap() {
        let raw = vec![0u8; 17];
        let mut c = CountingRead {
            inner: std::io::Cursor::new(raw),
            read: 0,
            cap: 16,
        };
        let mut buf = [0u8; 64];
        let mut total = 0usize;
        loop {
            match c.read(&mut buf) {
                Ok(0) => panic!("should have rejected before clean EOF"),
                Ok(n) => total += n,
                Err(e) => {
                    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
                    assert_eq!(total, 16);
                    return;
                }
            }
        }
    }

    #[test]
    fn raw_over_cap_rejected_even_when_utf8_would_fit() {
        // ASCII UTF-16LE: 2 raw bytes / char. Cap 20 includes BOM(2)+9 chars(18)=20
        // exactly for 9 chars; 10 chars ⇒ raw 22 > 20 while UTF-8 would be 10 < 20.
        let body = "ABCDEFGHIJ"; // 10 ASCII
        let raw = utf16le_bom_tsv(body);
        assert_eq!(raw.len(), 2 + body.len() * 2);
        assert!(raw.len() > 20);
        assert!(body.len() < 20);
        let cancel = live_cancel();
        let err = parse_utf16_raw(&raw, 20, MAX_TSV_ROWS, &cancel).expect_err("too large");
        assert_eq!(err, EdinetError::TooLarge);
    }

    #[test]
    fn raw_exact_cap_accepted() {
        let tsv = format!(
            "{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "A",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "9"
            ),
        );
        let raw = utf16le_bom_tsv(&tsv);
        let cancel = live_cancel();
        let out = parse_utf16_raw(&raw, raw.len() as u64, MAX_TSV_ROWS, &cancel)
            .unwrap_or_else(|e| panic!("exact cap: {e}"));
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("rev");
        assert_eq!(fact.value_text, "9");
    }

    #[test]
    fn rejects_missing_bom_and_utf16be() {
        let tsv = format!(
            "{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "売上",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "1"
            ),
        );
        // UTF-16LE payload without BOM.
        let mut no_bom = Vec::new();
        for u in tsv.encode_utf16() {
            no_bom.extend_from_slice(&u.to_le_bytes());
        }
        let cancel = live_cancel();
        assert_eq!(
            parse_utf16_raw(&no_bom, MAX_SINGLE_TSV_BYTES, MAX_TSV_ROWS, &cancel),
            Err(EdinetError::UnsupportedEncoding)
        );

        // UTF-16BE BOM + BE payload.
        let mut be = vec![0xFE, 0xFF];
        for u in tsv.encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(
            parse_utf16_raw(&be, MAX_SINGLE_TSV_BYTES, MAX_TSV_ROWS, &cancel),
            Err(EdinetError::UnsupportedEncoding)
        );
    }

    #[test]
    fn cancel_stops_csv_parse() {
        let tsv = format!(
            "{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "1"
            ),
        );
        let mut cur = std::io::Cursor::new(tsv.as_bytes());
        let cancel = live_cancel();
        cancel.cancel();
        let err = parse_financial_tsv_reader(
            &mut cur,
            Instant::now() + Duration::from_secs(5),
            &cancel,
            MAX_TSV_ROWS,
        )
        .expect_err("cancelled");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Cancelled));
    }

    #[test]
    fn cancel_stops_entry_scan() {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let zip_path = dir.path().join("t5.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap_or_else(|e| panic!("{e}"));
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("XBRL_TO_CSV/sample.csv", opts)
                .unwrap_or_else(|e| panic!("{e}"));
            zw.write_all(&utf16le_bom_tsv("x"))
                .unwrap_or_else(|e| panic!("{e}"));
            zw.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        let zip_bytes = std::fs::read(&zip_path).unwrap_or_else(|e| panic!("{e}"));

        use crate::knowledge::edinet_archive::download_edinet_archive_to_temp;
        use crate::knowledge::net_gateway::{HttpTransport, ResponseBody, ResponseMeta};

        struct Body(Option<Vec<u8>>);
        impl ResponseBody for Body {
            fn next_chunk(
                &mut self,
            ) -> impl std::future::Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send
            {
                let n = self.0.take().map(Ok);
                async move { n }
            }
        }
        struct Tr(Vec<u8>);
        impl HttpTransport for Tr {
            type Body = Body;
            fn get(
                &self,
                _url: &str,
                _d: Duration,
            ) -> impl std::future::Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send
            {
                let body = self.0.clone();
                async move {
                    Ok((
                        ResponseMeta {
                            status: 200,
                            content_type: Some("application/octet-stream".into()),
                            content_encoding: Some("identity".into()),
                            content_length: None,
                        },
                        Body(Some(body)),
                    ))
                }
            }
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|e| panic!("{e}"));
        let gate = Arc::new(ArchiveGate::new());
        let dl_cancel = live_cancel();
        let mut archive = rt
            .block_on(download_edinet_archive_to_temp(
                &Tr(zip_bytes),
                "S100CNCL",
                EdinetDocumentKind::XbrlCsv,
                "test-key",
                dir.path(),
                &gate,
                &dl_cancel,
                Duration::from_secs(5),
                MAX_EDINET_ARCHIVE_BYTES,
                || false,
            ))
            .unwrap_or_else(|e| panic!("dl: {e}"));
        let meta = FinancialExtractMeta {
            doc_id: "S100CNCL".into(),
            edinet_code: "E02144".into(),
            submitted_at: "2024-06-25 15:00".into(),
        };
        let extract_cancel = live_cancel();
        extract_cancel.cancel();
        let err = extract_financials_from_type5_archive(
            &mut archive,
            &meta,
            Duration::from_secs(5),
            &extract_cancel,
        )
        .expect_err("cancelled");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Cancelled));
    }

    struct TrackEof<R> {
        inner: R,
        hit_eof: Arc<AtomicBool>,
        reads: Arc<AtomicUsize>,
    }

    impl<R: std::io::Read> std::io::Read for TrackEof<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let n = self.inner.read(buf)?;
            if n == 0 {
                self.hit_eof.store(true, Ordering::SeqCst);
            }
            Ok(n)
        }
    }

    #[test]
    fn row_limit_drains_entry_to_eof() {
        let tsv = format!(
            "{}\r\n{}\r\n{}\r\n{}\r\n",
            header_line(),
            data_line(
                "jppfs_cor_NetSales",
                "売上高",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "1"
            ),
            data_line(
                "jppfs_cor_OperatingIncome",
                "営業利益",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "2"
            ),
            data_line(
                "jppfs_cor_ProfitLoss",
                "利益",
                "CurrentYearDuration",
                "当期",
                "連結",
                "期間",
                "JPY",
                "円",
                "3"
            ),
        );
        let raw = utf16le_bom_tsv(&tsv);
        let hit_eof = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let tracked = TrackEof {
            inner: std::io::Cursor::new(raw.clone()),
            hit_eof: Arc::clone(&hit_eof),
            reads: Arc::clone(&reads),
        };
        let cancel = live_cancel();
        let mut decoder =
            open_utf16le_tsv_decoder(tracked, raw.len() as u64).unwrap_or_else(|e| panic!("{e}"));
        let out = parse_financial_tsv_reader(
            &mut decoder,
            Instant::now() + Duration::from_secs(5),
            &cancel,
            1, // stop after first data row; must still drain
        )
        .unwrap_or_else(|e| panic!("row limit: {e}"));
        assert!(out.warnings.contains(&EdinetWarning::RowLimitReached));
        assert_eq!(out.financials.len(), 1);
        let fact = out.financials.first().expect("rev");
        assert_eq!(fact.value_text, "1");
        assert!(
            hit_eof.load(Ordering::SeqCst),
            "raw entry must reach EOF for CRC (reads={})",
            reads.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn invalid_checksum_maps_to_invalid_zip_not_too_large() {
        let err = std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid checksum");
        assert_eq!(super::map_entry_io(err), EdinetError::InvalidZip);
        let err = std::io::Error::new(std::io::ErrorKind::InvalidData, "edinet_tsv_too_large");
        assert_eq!(super::map_entry_io(err), EdinetError::TooLarge);
    }
}
