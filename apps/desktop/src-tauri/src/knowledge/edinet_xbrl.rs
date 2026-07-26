//! EDINET Step 8 — bounded Inline XBRL / XBRL narrative extraction (`type=1`).
//!
//! Runs only after Step 6 preflight. Streams `XBRL/PublicDoc/*` entries
//! through [`BoundedXmlReader`] (never raw `quick_xml` from extract paths).
//! Does not touch Heavy Coordinator, RAG embed, or evidence vault (Step 9+).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::io::Read;
use std::time::{Duration, Instant};

use quick_xml::events::Event;
use quick_xml::name::{Namespace, NamespaceResolver, QName, ResolveResult};
use tokio_util::sync::CancellationToken;
use zip::ZipArchive;

use crate::knowledge::bounded_io::{
    check_deadline_cancel, BoundedXmlReader, CountingRead, XmlDrive,
};
use crate::knowledge::edinet_archive::{preflight_edinet_archive, TempArchive};
use crate::knowledge::edinet_client::{EdinetDocumentKind, EdinetError};
use crate::knowledge::edinet_csv::{
    EdinetEvidenceSection, EdinetWarning, ExtractedNarrative, NarrativeConcept, PartialEdinetFacts,
};
use crate::knowledge::render_guard::{sanitize_external_text, SanitizeError};

/// Uncompressed single XHTML/XBRL entry cap (V3 §4.1).
pub const MAX_SINGLE_XBRL_BYTES: u64 = 32 * 1024 * 1024;
/// Selected candidates' uncompressed aggregate (declared pre-check + measured).
pub const MAX_SELECTED_UNCOMPRESSED: u64 = 96 * 1024 * 1024;
/// Per-section capture cap (display; evidence store is Step 9).
pub const MAX_SECTION_BYTES: usize = 128 * 1024;
/// Max `XBRL/PublicDoc` candidates to try.
pub const MAX_CANDIDATE_FILES: usize = 16;
/// Max buffered `ix:continuation` fragments.
pub const MAX_CONTINUATIONS: usize = 32;
/// Max `continuedAt` chain hops.
pub const MAX_CONTINUATION_CHAIN: usize = 8;
/// Max facts retained until EOF for forward `continuedAt` resolution.
pub const MAX_PENDING_FACTS: usize = 32;
/// Max aggregate heap bytes retained by pending fact fields.
pub const MAX_PENDING_FACT_BYTES: usize = 512 * 1024;
/// Aggregate evidence text budget per document (Step 9, V3 §6).
pub const MAX_TOTAL_EVIDENCE_BYTES: usize = 256 * 1024;
/// Sanitize probe margin above [`MAX_SECTION_BYTES`]: sanitize can expand
/// bytes (NFKC / fullwidth mapping), and its internal truncation is silent.
/// Sanitizing with `cap + margin` keeps any overflow detectable (the result
/// stays > cap even after its UTF-8 boundary backoff of ≤3 bytes), so we can
/// re-truncate explicitly and set `truncated = true`.
const SANITIZE_PROBE_MARGIN: usize = 16;

const IX_NS_2013: &[u8] = b"http://www.xbrl.org/2013/inlineXBRL";
const IX_NS_2008: &[u8] = b"http://www.xbrl.org/2008/inlineXBRL";
const JPCRP_NS_PREFIX: &str = "http://disclosure.edinet-fsa.go.jp/taxonomy/jpcrp/";

/// Provenance keys (no secrets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NarrativeExtractMeta {
    pub doc_id: String,
    pub edinet_code: String,
    pub submitted_at: String,
    /// Filing period from list metadata (`periodStart` / `periodEnd`).
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

/// Extract narrative text blocks from a preflight-eligible `type=1` archive.
pub fn extract_narratives_from_type1_archive(
    archive: &mut TempArchive,
    meta: &NarrativeExtractMeta,
    parse_deadline: Duration,
    cancel: &CancellationToken,
) -> Result<PartialEdinetFacts, EdinetError> {
    if archive.kind() != EdinetDocumentKind::FilingAndXbrl {
        return Err(EdinetError::InvalidArgument);
    }
    let deadline_at = Instant::now() + parse_deadline;
    check_deadline_cancel(deadline_at, cancel)?;
    let _preflight = preflight_edinet_archive(archive)?;
    check_deadline_cancel(deadline_at, cancel)?;

    let file = archive.file();
    let mut zip = ZipArchive::new(file).map_err(|_| EdinetError::InvalidZip)?;
    let candidates = select_xbrl_entry_indices(&mut zip, deadline_at, cancel)?;
    if candidates.is_empty() {
        return Ok(PartialEdinetFacts {
            financials: Vec::new(),
            narratives: Vec::new(),
            evidence: Vec::new(),
            warnings: vec![EdinetWarning::NoEligibleXbrl],
        });
    }

    let mut warnings: Vec<EdinetWarning> = Vec::new();
    let mut narratives: Vec<ExtractedNarrative> = Vec::new();
    let mut selected_used: u64 = 0;

    for idx in candidates {
        check_deadline_cancel(deadline_at, cancel)?;
        let declared = {
            let entry = zip.by_index(idx).map_err(|_| EdinetError::InvalidZip)?;
            entry.size()
        };
        if !can_attempt_candidate(selected_used, declared) {
            break;
        }
        let attempt = extract_one_entry(&mut zip, idx, deadline_at, cancel);
        selected_used = charge_selected_bytes(selected_used, declared, attempt.measured_bytes)?;
        match attempt.result {
            Ok(mut got) => {
                narratives.append(&mut got.narratives);
                for w in got.warnings {
                    push_warning(&mut warnings, w);
                }
                if !narratives.is_empty() {
                    break;
                }
            }
            Err(EdinetError::TooLarge) => return Err(EdinetError::TooLarge),
            Err(EdinetError::Gateway(g)) => return Err(EdinetError::Gateway(g)),
            Err(_) => {
                push_warning(&mut warnings, EdinetWarning::XbrlEntrySoftFail);
            }
        }
    }

    if narratives.is_empty() && warnings.is_empty() {
        push_warning(&mut warnings, EdinetWarning::NoEligibleXbrl);
    }

    let evidence = build_evidence_sections(meta, &narratives, &mut warnings);

    Ok(PartialEdinetFacts {
        financials: Vec::new(),
        narratives,
        evidence,
        warnings,
    })
}

/// Build sanitized RAG evidence sections from extracted narratives (Step 9).
///
/// - each section is sanitized (`render_guard::sanitize_external_text`) and
///   capped at [`MAX_SECTION_BYTES`]; a sanitize-side cut sets `truncated` and
///   emits [`EdinetWarning::NarrativeTruncated`] — never silent
/// - aggregate text is capped at [`MAX_TOTAL_EVIDENCE_BYTES`]; a section that
///   overflows the remaining budget is truncated-to-fit (flagged) or dropped
///   when nothing fits, with [`EdinetWarning::EvidenceBudgetExceeded`]
/// - a sanitize failure discards **only that section**
///   ([`EdinetWarning::EvidenceSanitizeFailed`]); other sections survive
pub fn build_evidence_sections(
    meta: &NarrativeExtractMeta,
    narratives: &[ExtractedNarrative],
    warnings: &mut Vec<EdinetWarning>,
) -> Vec<EdinetEvidenceSection> {
    let mut out: Vec<EdinetEvidenceSection> = Vec::new();
    let mut total: usize = 0;
    for narrative in narratives {
        let (mut text, sanitize_truncated) = match sanitize_evidence_text(&narrative.text) {
            Ok(v) => v,
            Err(SanitizeError::Rejected) => {
                push_warning(warnings, EdinetWarning::EvidenceSanitizeFailed);
                continue;
            }
        };
        let mut truncated = narrative.truncated || sanitize_truncated;
        if sanitize_truncated {
            push_warning(warnings, EdinetWarning::NarrativeTruncated);
        }
        let remaining = MAX_TOTAL_EVIDENCE_BYTES.saturating_sub(total);
        if text.len() > remaining {
            push_warning(warnings, EdinetWarning::EvidenceBudgetExceeded);
            truncate_utf8(&mut text, remaining);
            truncated = true;
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        total = total.saturating_add(trimmed.len());
        out.push(EdinetEvidenceSection {
            doc_id: meta.doc_id.clone(),
            edinet_code: meta.edinet_code.clone(),
            submitted_at: meta.submitted_at.clone(),
            period_start: meta.period_start.clone(),
            period_end: meta.period_end.clone(),
            concept: narrative.concept,
            local_name: narrative.local_name.clone(),
            unit: None,
            text: trimmed.to_string(),
            truncated,
        });
    }
    out
}

/// Sanitize evidence text with the probe margin so any cap-side truncation is
/// explicit (see [`SANITIZE_PROBE_MARGIN`]).
fn sanitize_evidence_text(raw: &str) -> Result<(String, bool), SanitizeError> {
    let probe_cap = MAX_SECTION_BYTES.saturating_add(SANITIZE_PROBE_MARGIN);
    let mut text = sanitize_external_text(raw, probe_cap)?;
    let mut truncated = false;
    if text.len() > MAX_SECTION_BYTES {
        truncate_utf8(&mut text, MAX_SECTION_BYTES);
        truncated = true;
    }
    Ok((text, truncated))
}

/// Declared-size gate for the 96 MiB selected-uncompressed budget.
pub fn can_attempt_candidate(used: u64, declared: u64) -> bool {
    used.saturating_add(declared) <= MAX_SELECTED_UNCOMPRESSED
}

fn charge_selected_bytes(used: u64, declared: u64, measured: u64) -> Result<u64, EdinetError> {
    let charged = used.saturating_add(declared.max(measured));
    if charged > MAX_SELECTED_UNCOMPRESSED {
        return Err(EdinetError::TooLarge);
    }
    Ok(charged)
}

/// Exact normalized root: `XBRL/PublicDoc/...` (not a substring elsewhere).
pub fn is_public_doc_path(norm: &str) -> bool {
    let n = norm.trim_start_matches('/');
    if n.contains("..") {
        return false;
    }
    n.starts_with("XBRL/PublicDoc/")
}

struct EntryExtract {
    narratives: Vec<ExtractedNarrative>,
    warnings: Vec<EdinetWarning>,
}

struct EntryAttempt {
    measured_bytes: u64,
    result: Result<EntryExtract, EdinetError>,
}

impl EntryAttempt {
    fn finished(measured_bytes: u64, result: Result<EntryExtract, EdinetError>) -> Self {
        Self {
            measured_bytes,
            result,
        }
    }
}

fn extract_one_entry<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    index: usize,
    deadline_at: Instant,
    cancel: &CancellationToken,
) -> EntryAttempt {
    let entry = match zip.by_index(index) {
        Ok(entry) => entry,
        Err(_) => return EntryAttempt::finished(0, Err(EdinetError::InvalidZip)),
    };
    if entry.encrypted() {
        return EntryAttempt::finished(0, Err(EdinetError::UnsupportedArchive));
    }
    match entry.compression() {
        zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => {}
        _ => return EntryAttempt::finished(0, Err(EdinetError::UnsupportedArchive)),
    }
    if entry.size() > MAX_SINGLE_XBRL_BYTES || entry.compressed_size() > MAX_SINGLE_XBRL_BYTES {
        return EntryAttempt::finished(0, Err(EdinetError::TooLarge));
    }

    let counting = CountingRead::new(entry, MAX_SINGLE_XBRL_BYTES);
    let mut xml = BoundedXmlReader::new(counting);
    let mut parser = NarrativeParser::new();

    loop {
        let drive = xml.process_next(deadline_at, cancel, |resolver, ns, ev| {
            parser
                .on_event(resolver, ns, &ev)
                .map_err(|soft| match soft {
                    SoftFail::Doctype | SoftFail::PendingFactsLimit => EdinetError::Parse,
                })
        });
        match drive {
            Ok(XmlDrive::Continue) => {}
            Ok(XmlDrive::Eof) => break,
            Ok(XmlDrive::TooLargeEvent) => {
                let measured = xml.into_inner().bytes_read();
                return EntryAttempt::finished(
                    measured,
                    Ok(EntryExtract {
                        narratives: Vec::new(),
                        warnings: vec![EdinetWarning::XbrlEntrySoftFail],
                    }),
                );
            }
            Err(EdinetError::Parse) => {
                let measured = xml.into_inner().bytes_read();
                return EntryAttempt::finished(
                    measured,
                    Ok(EntryExtract {
                        narratives: Vec::new(),
                        warnings: vec![EdinetWarning::XbrlEntrySoftFail],
                    }),
                );
            }
            Err(e) => {
                let measured = xml.into_inner().bytes_read();
                return EntryAttempt::finished(measured, Err(e));
            }
        }
    }

    let measured = xml.into_inner().bytes_read();
    let (narratives, warnings) = parser.finish();
    EntryAttempt::finished(
        measured,
        Ok(EntryExtract {
            narratives,
            warnings,
        }),
    )
}

fn select_xbrl_entry_indices<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    deadline_at: Instant,
    cancel: &CancellationToken,
) -> Result<Vec<usize>, EdinetError> {
    let mut chosen: Vec<(String, usize)> = Vec::new();
    for i in 0..zip.len() {
        check_deadline_cancel(deadline_at, cancel)?;
        if chosen.len() >= MAX_CANDIDATE_FILES {
            break;
        }
        let entry = zip.by_index(i).map_err(|_| EdinetError::InvalidZip)?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let path_str = path.to_string_lossy();
        let norm = path_str.replace('\\', "/");
        if !is_public_doc_path(&norm) {
            continue;
        }
        let lower = norm.to_ascii_lowercase();
        if !(lower.ends_with(".htm")
            || lower.ends_with(".html")
            || lower.ends_with(".xhtml")
            || lower.ends_with(".xbrl"))
        {
            continue;
        }
        chosen.push((norm, i));
    }
    chosen.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(chosen.into_iter().map(|(_, i)| i).collect())
}

#[derive(Debug)]
enum SoftFail {
    Doctype,
    PendingFactsLimit,
}

struct ContFragment {
    id: String,
    text: String,
    next_id: Option<String>,
    truncated: bool,
}

struct PendingFact {
    concept: NarrativeConcept,
    local_name: String,
    text: String,
    truncated: bool,
    continued_at: Option<String>,
}

enum CaptureState {
    Fact {
        concept: NarrativeConcept,
        local_name: String,
        text: String,
        truncated: bool,
        continued_at: Option<String>,
        rel_depth: u16,
    },
    Continuation {
        id: String,
        next_id: Option<String>,
        text: String,
        truncated: bool,
        rel_depth: u16,
    },
}

struct NarrativeParser {
    continuations: Vec<ContFragment>,
    continuation_bytes: usize,
    pending_facts: Vec<PendingFact>,
    pending_fact_bytes: usize,
    capturing: Option<CaptureState>,
    skip_depth: u16,
    in_skip: bool,
    warnings: Vec<EdinetWarning>,
}

impl NarrativeParser {
    fn new() -> Self {
        Self {
            continuations: Vec::new(),
            continuation_bytes: 0,
            pending_facts: Vec::new(),
            pending_fact_bytes: 0,
            capturing: None,
            skip_depth: 0,
            in_skip: false,
            warnings: Vec::new(),
        }
    }

    fn on_event(
        &mut self,
        resolver: &NamespaceResolver,
        ns: ResolveResult<'_>,
        ev: &Event<'_>,
    ) -> Result<(), SoftFail> {
        match ev {
            Event::DocType(_) => return Err(SoftFail::Doctype),
            Event::Start(e) | Event::Empty(e) => {
                let local = local_name_of(e.name());
                if is_skip_tag(&local) {
                    if matches!(ev, Event::Start(_)) {
                        self.in_skip = true;
                        self.skip_depth = 1;
                    }
                    return Ok(());
                }
                if self.in_skip {
                    if matches!(ev, Event::Start(_)) {
                        self.skip_depth = self.skip_depth.saturating_add(1);
                    }
                    return Ok(());
                }

                if is_ix_ns(&ns) && local.eq_ignore_ascii_case("continuation") {
                    if let Some(id) = attr_value(e, b"id") {
                        let next_id = attr_value(e, b"continuedAt");
                        if matches!(ev, Event::Empty(_)) {
                            self.store_continuation(id, String::new(), next_id, false);
                        } else {
                            self.capturing = Some(CaptureState::Continuation {
                                id,
                                next_id,
                                text: String::new(),
                                truncated: false,
                                rel_depth: 0,
                            });
                        }
                    }
                    return Ok(());
                }

                if is_ix_ns(&ns) && local.eq_ignore_ascii_case("nonNumeric") {
                    if let Some((concept, loc)) = resolve_concept_name_attr(resolver, e) {
                        let continued_at = attr_value(e, b"continuedAt");
                        self.ensure_pending_fact_slot(&loc, &continued_at)?;
                        if matches!(ev, Event::Empty(_)) {
                            self.store_pending_fact(PendingFact {
                                concept,
                                local_name: loc,
                                text: String::new(),
                                truncated: false,
                                continued_at,
                            })?;
                        } else {
                            self.capturing = Some(CaptureState::Fact {
                                concept,
                                local_name: loc,
                                text: String::new(),
                                truncated: false,
                                continued_at,
                                rel_depth: 0,
                            });
                        }
                    }
                    return Ok(());
                }

                // Classic XBRL: resolved URI + allowlisted local name.
                if let Some(concept) = concept_from_bound_ns(&ns, &local) {
                    if self.capturing.is_none() && matches!(ev, Event::Start(_)) {
                        self.ensure_pending_fact_slot(&local, &None)?;
                        self.capturing = Some(CaptureState::Fact {
                            concept,
                            local_name: local,
                            text: String::new(),
                            truncated: false,
                            continued_at: None,
                            rel_depth: 0,
                        });
                    }
                    return Ok(());
                }

                if let Some(cap) = self.capturing.as_mut() {
                    if matches!(ev, Event::Start(_)) {
                        match cap {
                            CaptureState::Fact { rel_depth, .. }
                            | CaptureState::Continuation { rel_depth, .. } => {
                                *rel_depth = rel_depth.saturating_add(1);
                            }
                        }
                    }
                }
            }
            Event::End(e) => {
                let local = local_name_of(e.name());
                if self.in_skip {
                    self.skip_depth = self.skip_depth.saturating_sub(1);
                    if self.skip_depth == 0 {
                        self.in_skip = false;
                    }
                    return Ok(());
                }

                let Some(cap) = self.capturing.as_mut() else {
                    return Ok(());
                };

                let rel = match cap {
                    CaptureState::Fact { rel_depth, .. }
                    | CaptureState::Continuation { rel_depth, .. } => *rel_depth,
                };
                if rel > 0 {
                    match cap {
                        CaptureState::Fact { rel_depth, .. }
                        | CaptureState::Continuation { rel_depth, .. } => {
                            *rel_depth = rel_depth.saturating_sub(1);
                        }
                    }
                    return Ok(());
                }

                match self.capturing.take() {
                    Some(CaptureState::Continuation {
                        id,
                        next_id,
                        text,
                        truncated,
                        ..
                    }) => {
                        if local.eq_ignore_ascii_case("continuation") {
                            self.store_continuation(id, text, next_id, truncated);
                        }
                    }
                    Some(CaptureState::Fact {
                        concept,
                        local_name,
                        text,
                        truncated,
                        continued_at,
                        ..
                    }) => {
                        let closing_fact = local.eq_ignore_ascii_case("nonNumeric")
                            || NarrativeConcept::from_local(&local).is_some();
                        if closing_fact {
                            self.store_pending_fact(PendingFact {
                                concept,
                                local_name,
                                text,
                                truncated,
                                continued_at,
                            })?;
                        }
                    }
                    None => {}
                }
            }
            Event::Text(t) => {
                if self.in_skip {
                    return Ok(());
                }
                if let Some(cap) = self.capturing.as_mut() {
                    let raw = t.decode().unwrap_or_default();
                    match cap {
                        CaptureState::Fact {
                            text, truncated, ..
                        }
                        | CaptureState::Continuation {
                            text, truncated, ..
                        } => {
                            append_capped(text, &raw, truncated, &mut self.warnings);
                        }
                    }
                }
            }
            Event::CData(t) => {
                if self.in_skip {
                    return Ok(());
                }
                if let Some(cap) = self.capturing.as_mut() {
                    let raw = String::from_utf8_lossy(t.as_ref());
                    match cap {
                        CaptureState::Fact {
                            text, truncated, ..
                        }
                        | CaptureState::Continuation {
                            text, truncated, ..
                        } => {
                            append_capped(text, &raw, truncated, &mut self.warnings);
                        }
                    }
                }
            }
            Event::Comment(_) | Event::PI(_) | Event::Decl(_) => {}
            Event::GeneralRef(_) => {}
            Event::Eof => {}
        }
        Ok(())
    }

    fn ensure_pending_fact_slot(
        &self,
        local_name: &str,
        continued_at: &Option<String>,
    ) -> Result<(), SoftFail> {
        let metadata_bytes = local_name
            .len()
            .saturating_add(continued_at.as_ref().map_or(0, String::len));
        if self.pending_facts.len() >= MAX_PENDING_FACTS
            || self.pending_fact_bytes.saturating_add(metadata_bytes) > MAX_PENDING_FACT_BYTES
        {
            return Err(SoftFail::PendingFactsLimit);
        }
        Ok(())
    }

    fn store_pending_fact(&mut self, fact: PendingFact) -> Result<(), SoftFail> {
        let retained_bytes = fact
            .local_name
            .len()
            .saturating_add(fact.text.len())
            .saturating_add(fact.continued_at.as_ref().map_or(0, String::len));
        if self.pending_facts.len() >= MAX_PENDING_FACTS
            || self.pending_fact_bytes.saturating_add(retained_bytes) > MAX_PENDING_FACT_BYTES
        {
            return Err(SoftFail::PendingFactsLimit);
        }
        self.pending_fact_bytes = self.pending_fact_bytes.saturating_add(retained_bytes);
        self.pending_facts.push(fact);
        Ok(())
    }

    fn store_continuation(
        &mut self,
        id: String,
        body: String,
        next_id: Option<String>,
        truncated: bool,
    ) {
        if self.continuations.len() >= MAX_CONTINUATIONS {
            return;
        }
        // First-ID-wins: ignore later duplicates.
        if self.continuations.iter().any(|c| c.id == id) {
            return;
        }
        let add = body.len();
        if self.continuation_bytes.saturating_add(add) > MAX_SECTION_BYTES {
            return;
        }
        self.continuation_bytes = self.continuation_bytes.saturating_add(add);
        self.continuations.push(ContFragment {
            id,
            text: body,
            next_id,
            truncated,
        });
    }

    fn finish(mut self) -> (Vec<ExtractedNarrative>, Vec<EdinetWarning>) {
        let mut best: Vec<ExtractedNarrative> = Vec::new();
        for fact in self.pending_facts.drain(..) {
            let resolved = resolve_continued(
                &self.continuations,
                &fact.continued_at,
                &fact.text,
                &mut self.warnings,
            );
            let mut text = resolved.text;
            let mut truncated = fact.truncated || resolved.truncated;
            if text.len() > MAX_SECTION_BYTES {
                truncate_utf8(&mut text, MAX_SECTION_BYTES);
                truncated = true;
                push_warning(&mut self.warnings, EdinetWarning::NarrativeTruncated);
            }
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            if best.iter().any(|n| n.concept == fact.concept) {
                continue;
            }
            best.push(ExtractedNarrative {
                concept: fact.concept,
                local_name: fact.local_name,
                text: trimmed.to_string(),
                truncated,
            });
        }
        (best, self.warnings)
    }
}

struct ResolvedContinuation {
    text: String,
    truncated: bool,
}

fn resolve_continued(
    continuations: &[ContFragment],
    continued_at: &Option<String>,
    own: &str,
    warnings: &mut Vec<EdinetWarning>,
) -> ResolvedContinuation {
    let mut out = String::from(own);
    let mut truncated = false;
    let mut next = continued_at.clone();
    let mut seen: Vec<String> = Vec::new();
    let mut hops = 0usize;
    while let Some(hop) = next {
        if hops >= MAX_CONTINUATION_CHAIN {
            push_warning(warnings, EdinetWarning::ContinuedAtUnresolved);
            break;
        }
        if seen.iter().any(|s| s == &hop) {
            push_warning(warnings, EdinetWarning::ContinuedAtUnresolved);
            break;
        }
        seen.push(hop.clone());
        let Some(frag) = continuations.iter().find(|c| c.id == hop) else {
            push_warning(warnings, EdinetWarning::ContinuedAtUnresolved);
            break;
        };
        if !out.is_empty() && !frag.text.is_empty() {
            out.push(' ');
        }
        out.push_str(&frag.text);
        truncated |= frag.truncated;
        if out.len() > MAX_SECTION_BYTES {
            truncate_utf8(&mut out, MAX_SECTION_BYTES);
            truncated = true;
            push_warning(warnings, EdinetWarning::NarrativeTruncated);
            break;
        }
        next = frag.next_id.clone();
        hops = hops.saturating_add(1);
    }
    ResolvedContinuation {
        text: out,
        truncated,
    }
}

fn append_capped(
    dst: &mut String,
    add: &str,
    truncated: &mut bool,
    warnings: &mut Vec<EdinetWarning>,
) {
    if *truncated {
        return;
    }
    let room = MAX_SECTION_BYTES.saturating_sub(dst.len());
    if add.len() <= room {
        dst.push_str(add);
        return;
    }
    if room > 0 {
        let mut piece = add.to_string();
        truncate_utf8(&mut piece, room);
        dst.push_str(&piece);
    }
    *truncated = true;
    push_warning(warnings, EdinetWarning::NarrativeTruncated);
}

fn truncate_utf8(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

fn local_name_of(q: QName<'_>) -> String {
    let raw = q.local_name();
    String::from_utf8_lossy(raw.as_ref()).into_owned()
}

fn attr_value(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key || a.key.local_name().as_ref() == key {
            let raw = String::from_utf8_lossy(a.value.as_ref());
            let unescaped = quick_xml::escape::unescape(&raw)
                .unwrap_or(std::borrow::Cow::Borrowed(raw.as_ref()));
            let t = unescaped.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn is_skip_tag(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "script" | "style" | "noscript"
    )
}

fn is_ix_ns(ns: &ResolveResult<'_>) -> bool {
    match ns {
        ResolveResult::Bound(Namespace(uri)) => *uri == IX_NS_2013 || *uri == IX_NS_2008,
        _ => false,
    }
}

fn is_jpcrp_cor_uri(uri: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(uri) else {
        return false;
    };
    s.starts_with(JPCRP_NS_PREFIX) && s.contains("jpcrp_cor")
}

fn concept_from_bound_ns(ns: &ResolveResult<'_>, local: &str) -> Option<NarrativeConcept> {
    match ns {
        ResolveResult::Bound(Namespace(uri)) if is_jpcrp_cor_uri(uri) => {
            NarrativeConcept::from_local(local)
        }
        _ => None,
    }
}

fn resolve_concept_name_attr(
    resolver: &NamespaceResolver,
    e: &quick_xml::events::BytesStart<'_>,
) -> Option<(NarrativeConcept, String)> {
    let name_attr = attr_value(e, b"name")?;
    let (ns, local) = resolver.resolve(QName(name_attr.as_bytes()), true);
    let local_s = String::from_utf8_lossy(local.as_ref()).into_owned();
    let concept = concept_from_bound_ns(&ns, &local_s)?;
    Some((concept, local_s))
}

impl NarrativeConcept {
    fn from_local(local: &str) -> Option<Self> {
        match local {
            "BusinessRisksTextBlock" => Some(Self::BusinessRisks),
            "DescriptionOfBusinessTextBlock" => Some(Self::DescriptionOfBusiness),
            "ManagementAnalysisOfFinancialPositionOperatingResultsAndCashFlowsTextBlock" => {
                Some(Self::ManagementAnalysis)
            }
            _ => None,
        }
    }
}

fn push_warning(warnings: &mut Vec<EdinetWarning>, w: EdinetWarning) {
    if warnings.len() >= 64 {
        return;
    }
    if !warnings.contains(&w) {
        warnings.push(w);
    }
}

/// Parse an already-decoded UTF-8 Inline XBRL / XBRL stream (tests / callers).
pub fn parse_narrative_xml_reader<R: Read>(
    reader: R,
    deadline_at: Instant,
    cancel: &CancellationToken,
) -> Result<PartialEdinetFacts, EdinetError> {
    let mut xml = BoundedXmlReader::new(reader);
    let mut parser = NarrativeParser::new();
    loop {
        let drive = xml.process_next(deadline_at, cancel, |resolver, ns, ev| {
            parser
                .on_event(resolver, ns, &ev)
                .map_err(|soft| match soft {
                    SoftFail::Doctype | SoftFail::PendingFactsLimit => EdinetError::Parse,
                })
        });
        match drive {
            Ok(XmlDrive::Continue) => {}
            Ok(XmlDrive::Eof) => break,
            Ok(XmlDrive::TooLargeEvent) => {
                return Ok(PartialEdinetFacts {
                    financials: Vec::new(),
                    narratives: Vec::new(),
                    evidence: Vec::new(),
                    warnings: vec![EdinetWarning::XbrlEntrySoftFail],
                });
            }
            Err(EdinetError::Parse) => {
                return Ok(PartialEdinetFacts {
                    financials: Vec::new(),
                    narratives: Vec::new(),
                    evidence: Vec::new(),
                    warnings: vec![EdinetWarning::XbrlEntrySoftFail],
                });
            }
            Err(e) => return Err(e),
        }
    }
    let (narratives, warnings) = parser.finish();
    // No filing metadata at this layer — evidence is built only by the
    // archive-level entry point (`extract_narratives_from_type1_archive`).
    Ok(PartialEdinetFacts {
        financials: Vec::new(),
        narratives,
        evidence: Vec::new(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::edinet_archive::{
        download_edinet_archive_to_temp, ArchiveGate, MAX_EDINET_ARCHIVE_BYTES,
    };
    use crate::knowledge::net_gateway::{GatewayError, HttpTransport, ResponseBody, ResponseMeta};
    use std::io::Write;
    use std::sync::Arc;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    const JPCRP_COR: &str =
        "http://disclosure.edinet-fsa.go.jp/taxonomy/jpcrp/2023-12-01/jpcrp_cor";
    const IX: &str = "http://www.xbrl.org/2013/inlineXBRL";

    fn cancel_live() -> CancellationToken {
        CancellationToken::new()
    }

    fn parse_xml(xml: &str) -> PartialEdinetFacts {
        let cancel = cancel_live();
        parse_narrative_xml_reader(
            std::io::Cursor::new(xml.as_bytes()),
            Instant::now() + Duration::from_secs(5),
            &cancel,
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn extracts_ix_nonnumeric_by_resolved_ns_ignoring_prefix() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<html xmlns:ix="{IX}"
      xmlns:jpcrp_cor="{JPCRP_COR}">
<body>
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock" escape="true">
Risk A &amp; B
</ix:nonNumeric>
<script>evil()</script>
<a:nonNumeric xmlns:a="{IX}" name="jpcrp_cor:DescriptionOfBusinessTextBlock">Biz desc</a:nonNumeric>
</body></html>"#
        );
        let out = parse_xml(&xml);
        assert_eq!(out.narratives.len(), 2);
        let risks = out
            .narratives
            .iter()
            .find(|n| n.concept == NarrativeConcept::BusinessRisks)
            .expect("risks");
        assert!(risks.text.contains("Risk A"));
        assert!(risks.text.contains("B"));
        assert!(!risks.text.contains("evil"));
        let desc = out
            .narratives
            .iter()
            .find(|n| n.concept == NarrativeConcept::DescriptionOfBusiness)
            .expect("desc");
        assert_eq!(desc.text, "Biz desc");
    }

    #[test]
    fn rejects_unknown_namespace_impersonation() {
        let xml = r#"<?xml version="1.0"?>
<html xmlns:ix="http://evil.example/ix"
      xmlns:jpcrp_cor="http://evil.example/jpcrp">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">should not extract</ix:nonNumeric>
</html>"#;
        let out = parse_xml(xml);
        assert!(out.narratives.is_empty());
    }

    #[test]
    fn rejects_undeclared_concept_prefix() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<html xmlns:ix="{IX}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">no ns</ix:nonNumeric>
</html>"#
        );
        let out = parse_xml(&xml);
        assert!(out.narratives.is_empty());
    }

    #[test]
    fn doctype_soft_fails_entry() {
        let xml = format!(
            r#"<!DOCTYPE html><root xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}"><ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">x</ix:nonNumeric></root>"#
        );
        let out = parse_xml(&xml);
        assert!(out.narratives.is_empty());
        assert!(out.warnings.contains(&EdinetWarning::XbrlEntrySoftFail));
    }

    #[test]
    fn continued_at_forward_resolves_after_eof() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock" continuedAt="c1">Base</ix:nonNumeric>
<ix:continuation id="c1"> more risks</ix:continuation>
</div>"#
        );
        let out = parse_xml(&xml);
        assert_eq!(out.narratives.len(), 1);
        let n = out.narratives.first().expect("n");
        assert!(n.text.contains("Base"));
        assert!(n.text.contains("more risks"));
    }

    #[test]
    fn continued_at_multi_hop_chain() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock" continuedAt="c1">A</ix:nonNumeric>
<ix:continuation id="c1" continuedAt="c2"> B</ix:continuation>
<ix:continuation id="c2" continuedAt="c3"> C</ix:continuation>
<ix:continuation id="c3"> D</ix:continuation>
</div>"#
        );
        let out = parse_xml(&xml);
        assert_eq!(out.narratives.len(), 1);
        let n = out.narratives.first().expect("n");
        assert!(n.text.contains("A"));
        assert!(n.text.contains("B"));
        assert!(n.text.contains("C"));
        assert!(n.text.contains("D"));
    }

    #[test]
    fn continued_at_cycle_stops_with_warning() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock" continuedAt="c1">A</ix:nonNumeric>
<ix:continuation id="c1" continuedAt="c2"> B</ix:continuation>
<ix:continuation id="c2" continuedAt="c1"> C</ix:continuation>
</div>"#
        );
        let out = parse_xml(&xml);
        assert_eq!(out.narratives.len(), 1);
        assert!(out.warnings.contains(&EdinetWarning::ContinuedAtUnresolved));
        let n = out.narratives.first().expect("n");
        assert!(n.text.contains("A"));
        assert!(n.text.contains("B"));
        assert!(n.text.contains("C"));
    }

    #[test]
    fn continued_at_duplicate_id_first_wins() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:continuation id="c1"> first</ix:continuation>
<ix:continuation id="c1"> second</ix:continuation>
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock" continuedAt="c1">Base</ix:nonNumeric>
</div>"#
        );
        let out = parse_xml(&xml);
        assert_eq!(out.narratives.len(), 1);
        let n = out.narratives.first().expect("n");
        assert!(n.text.contains("first"));
        assert!(!n.text.contains("second"));
    }

    #[test]
    fn continuation_truncation_propagates_to_fact() {
        let piece = "x".repeat(48 * 1024);
        let xml = format!(
            r#"<?xml version="1.0"?>
<div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock" continuedAt="c1">Base</ix:nonNumeric>
<ix:continuation id="c1"><span>{piece}</span><span>{piece}</span><span>{piece}</span></ix:continuation>
</div>"#
        );
        let out = parse_xml(&xml);
        let narrative = out.narratives.first().expect("narrative");
        assert!(narrative.truncated);
        assert!(out.warnings.contains(&EdinetWarning::NarrativeTruncated));
    }

    #[test]
    fn pending_fact_count_limit_soft_fails() {
        let mut facts = String::new();
        for i in 0..=MAX_PENDING_FACTS {
            facts.push_str(&format!(
                "<ix:nonNumeric name=\"jpcrp_cor:BusinessRisksTextBlock\">fact-{i}</ix:nonNumeric>"
            ));
        }
        let xml = format!(
            r#"<?xml version="1.0"?><div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">{facts}</div>"#
        );
        let out = parse_xml(&xml);
        assert!(out.narratives.is_empty());
        assert!(out.warnings.contains(&EdinetWarning::XbrlEntrySoftFail));
    }

    #[test]
    fn pending_fact_byte_limit_soft_fails() {
        let piece = "z".repeat(20 * 1024);
        let mut facts = String::new();
        for _ in 0..MAX_PENDING_FACTS {
            facts.push_str(&format!(
                "<ix:nonNumeric name=\"jpcrp_cor:BusinessRisksTextBlock\">{piece}</ix:nonNumeric>"
            ));
        }
        let xml = format!(
            r#"<?xml version="1.0"?><div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">{facts}</div>"#
        );
        let out = parse_xml(&xml);
        assert!(out.narratives.is_empty());
        assert!(out.warnings.contains(&EdinetWarning::XbrlEntrySoftFail));
    }

    #[test]
    fn mismatched_end_tag_soft_fails() {
        let xml = format!(
            r#"<?xml version="1.0"?><div xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}"><ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">bad</div>"#
        );
        let out = parse_xml(&xml);
        assert!(out.narratives.is_empty());
        assert!(out.warnings.contains(&EdinetWarning::XbrlEntrySoftFail));
    }

    #[test]
    fn public_doc_path_requires_exact_root_prefix() {
        assert!(is_public_doc_path("XBRL/PublicDoc/a.htm"));
        assert!(is_public_doc_path("/XBRL/PublicDoc/a.htm"));
        assert!(!is_public_doc_path("evil/XBRL/PublicDoc/a.htm"));
        assert!(!is_public_doc_path("XBRL/PublicDoc/../secret.htm"));
        assert!(!is_public_doc_path("XBRL/Other/a.htm"));
    }

    #[test]
    fn selected_budget_rejects_over_aggregate() {
        let almost = MAX_SELECTED_UNCOMPRESSED - 1024;
        assert!(can_attempt_candidate(almost, 1024));
        assert!(!can_attempt_candidate(almost, 1025));
        assert!(!can_attempt_candidate(MAX_SELECTED_UNCOMPRESSED, 1));
        assert_eq!(
            charge_selected_bytes(0, 8, 16).expect("charge measured"),
            16
        );
        assert_eq!(
            charge_selected_bytes(0, 16, 8).expect("charge declared"),
            16
        );
    }

    #[test]
    fn crc_failure_returns_measured_bytes_for_aggregate_charge() {
        let cursor = std::io::Cursor::new(Vec::<u8>::new());
        let mut writer = ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file("XBRL/PublicDoc/bad_crc.xhtml", opts)
            .expect("start");
        writer
            .write_all(b"<?xml version=\"1.0\"?><root/>")
            .expect("write");
        let cursor = writer.finish().expect("finish");
        let mut bytes = cursor.into_inner();

        let central = bytes
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .expect("central directory");
        let crc_offset = central.saturating_add(16);
        let crc = bytes.get_mut(crc_offset).expect("central crc byte");
        *crc ^= 0xff;

        let mut zip = ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        let declared = zip.by_index(0).expect("entry").size();
        let cancel = cancel_live();
        let attempt = extract_one_entry(
            &mut zip,
            0,
            Instant::now() + Duration::from_secs(5),
            &cancel,
        );
        assert!(matches!(attempt.result, Err(EdinetError::InvalidZip)));
        assert!(attempt.measured_bytes > 0);
        assert_eq!(
            charge_selected_bytes(0, declared, attempt.measured_bytes).expect("charge"),
            declared.max(attempt.measured_bytes)
        );
    }

    #[test]
    fn cancel_stops_xml_parse() {
        let xml = format!(
            r#"<?xml version="1.0"?><a xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}"><ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">z</ix:nonNumeric></a>"#
        );
        let cancel = cancel_live();
        cancel.cancel();
        let err = parse_narrative_xml_reader(
            std::io::Cursor::new(xml.as_bytes()),
            Instant::now() + Duration::from_secs(5),
            &cancel,
        )
        .expect_err("cancelled");
        assert_eq!(err, EdinetError::Gateway(GatewayError::Cancelled));
    }

    #[test]
    fn type1_zip_extracts_after_preflight() {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let xml = format!(
            r#"<?xml version="1.0"?>
<html xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}">
<ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">ZIP risk</ix:nonNumeric>
</html>"#
        );
        let zip_path = dir.path().join("t1.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap_or_else(|e| panic!("{e}"));
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("XBRL/PublicDoc/0001_ixbrl.htm", opts)
                .unwrap_or_else(|e| panic!("{e}"));
            zw.write_all(xml.as_bytes())
                .unwrap_or_else(|e| panic!("{e}"));
            // Outside approved root — must be ignored even though path contains the marker.
            zw.start_file("evil/XBRL/PublicDoc/evil.htm", opts)
                .unwrap_or_else(|e| panic!("{e}"));
            zw.write_all(
                format!(
                    r#"<?xml version="1.0"?><html xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}"><ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">EVIL</ix:nonNumeric></html>"#
                )
                .as_bytes(),
            )
            .unwrap_or_else(|e| panic!("{e}"));
            zw.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        let zip_bytes = std::fs::read(&zip_path).unwrap_or_else(|e| panic!("{e}"));

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
        let dl_cancel = cancel_live();
        let mut archive = rt
            .block_on(download_edinet_archive_to_temp(
                &Tr(zip_bytes),
                "S100XBRL1",
                EdinetDocumentKind::FilingAndXbrl,
                "test-key",
                dir.path(),
                &gate,
                &dl_cancel,
                Duration::from_secs(5),
                MAX_EDINET_ARCHIVE_BYTES,
                || false,
            ))
            .unwrap_or_else(|e| panic!("dl: {e}"));
        let meta = NarrativeExtractMeta {
            doc_id: "S100XBRL1".into(),
            edinet_code: "E02144".into(),
            submitted_at: "2024-06-25 15:00".into(),
            period_start: Some("2023-04-01".into()),
            period_end: Some("2024-03-31".into()),
        };
        let extract_cancel = cancel_live();
        let out = extract_narratives_from_type1_archive(
            &mut archive,
            &meta,
            Duration::from_secs(5),
            &extract_cancel,
        )
        .unwrap_or_else(|e| panic!("extract: {e}"));
        assert_eq!(out.narratives.len(), 1);
        assert_eq!(
            out.narratives.first().map(|n| n.text.as_str()),
            Some("ZIP risk")
        );
        assert!(!out.narratives.iter().any(|n| n.text.contains("EVIL")));
        // Step 9: evidence section carries provenance, unit=None, truncated flag.
        assert_eq!(out.evidence.len(), 1);
        let ev = out.evidence.first().unwrap_or_else(|| panic!("evidence"));
        assert_eq!(ev.doc_id, "S100XBRL1");
        assert_eq!(ev.edinet_code, "E02144");
        assert_eq!(ev.submitted_at, "2024-06-25 15:00");
        assert_eq!(ev.period_start.as_deref(), Some("2023-04-01"));
        assert_eq!(ev.period_end.as_deref(), Some("2024-03-31"));
        assert_eq!(ev.concept, NarrativeConcept::BusinessRisks);
        assert_eq!(ev.unit, None);
        assert_eq!(ev.text, "ZIP risk");
        assert!(!ev.truncated);
        assert!(gate.is_busy());
        drop(archive);
    }

    #[test]
    fn aggregate_budget_skips_further_candidates() {
        let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let zip_path = dir.path().join("agg.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap_or_else(|e| panic!("{e}"));
            let mut zw = ZipWriter::new(f);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            // Three ~32MiB zero blobs — after measuring ~96MiB used, the fourth
            // distinctive XML must not be attempted.
            let blob = vec![0u8; (32 * 1024 * 1024) as usize];
            for name in ["a.bin.htm", "b.bin.htm", "c.bin.htm"] {
                zw.start_file(format!("XBRL/PublicDoc/{name}"), opts)
                    .unwrap_or_else(|e| panic!("{e}"));
                zw.write_all(&blob).unwrap_or_else(|e| panic!("{e}"));
            }
            let xml = format!(
                r#"<?xml version="1.0"?><html xmlns:ix="{IX}" xmlns:jpcrp_cor="{JPCRP_COR}"><ix:nonNumeric name="jpcrp_cor:BusinessRisksTextBlock">SHOULD_NOT_SEE</ix:nonNumeric></html>"#
            );
            zw.start_file("XBRL/PublicDoc/z_last.htm", opts)
                .unwrap_or_else(|e| panic!("{e}"));
            zw.write_all(xml.as_bytes())
                .unwrap_or_else(|e| panic!("{e}"));
            zw.finish().unwrap_or_else(|e| panic!("{e}"));
        }
        let zip_bytes = std::fs::read(&zip_path).unwrap_or_else(|e| panic!("{e}"));

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
        let dl_cancel = cancel_live();
        let mut archive = rt
            .block_on(download_edinet_archive_to_temp(
                &Tr(zip_bytes),
                "S100AGG1",
                EdinetDocumentKind::FilingAndXbrl,
                "test-key",
                dir.path(),
                &gate,
                &dl_cancel,
                Duration::from_secs(30),
                MAX_EDINET_ARCHIVE_BYTES,
                || false,
            ))
            .unwrap_or_else(|e| panic!("dl: {e}"));
        let meta = NarrativeExtractMeta {
            doc_id: "S100AGG1".into(),
            edinet_code: "E02144".into(),
            submitted_at: "2024-06-25 15:00".into(),
            period_start: None,
            period_end: None,
        };
        let extract_cancel = cancel_live();
        let out = extract_narratives_from_type1_archive(
            &mut archive,
            &meta,
            Duration::from_secs(60),
            &extract_cancel,
        );
        match out {
            Ok(facts) => {
                assert!(!facts
                    .narratives
                    .iter()
                    .any(|n| n.text.contains("SHOULD_NOT_SEE")));
            }
            Err(EdinetError::TooLarge) => {}
            Err(e) => panic!("unexpected {e}"),
        }
        drop(archive);
    }

    fn evidence_meta() -> NarrativeExtractMeta {
        NarrativeExtractMeta {
            doc_id: "S100EV01".into(),
            edinet_code: "E02144".into(),
            submitted_at: "2024-06-25 15:00".into(),
            period_start: Some("2023-04-01".into()),
            period_end: Some("2024-03-31".into()),
        }
    }

    fn narrative(concept: NarrativeConcept, text: &str, truncated: bool) -> ExtractedNarrative {
        ExtractedNarrative {
            concept,
            local_name: "BusinessRisksTextBlock".into(),
            text: text.into(),
            truncated,
        }
    }

    #[test]
    fn evidence_sanitize_failure_discards_only_that_section() {
        // Zero-width spaces survive extraction (`trim` keeps them) but sanitize
        // strips them all — an empty result is Rejected (hostile payload rule).
        let narratives = vec![
            narrative(NarrativeConcept::BusinessRisks, "\u{200B}\u{200B}", false),
            narrative(
                NarrativeConcept::DescriptionOfBusiness,
                "real business",
                false,
            ),
        ];
        let mut warnings = Vec::new();
        let out = build_evidence_sections(&evidence_meta(), &narratives, &mut warnings);
        assert_eq!(out.len(), 1);
        let kept = out.first().unwrap_or_else(|| panic!("kept"));
        assert_eq!(kept.concept, NarrativeConcept::DescriptionOfBusiness);
        assert_eq!(kept.text, "real business");
        assert!(warnings.contains(&EdinetWarning::EvidenceSanitizeFailed));
    }

    #[test]
    fn evidence_sanitize_expansion_truncated_explicitly() {
        // '!' (1 byte) sanitizes to '！' (3 bytes): 64 KiB in ⇒ ~192 KiB out,
        // past MAX_SECTION_BYTES. The cut must set `truncated`, never silent.
        let raw = "!".repeat(64 * 1024);
        let narratives = vec![narrative(NarrativeConcept::BusinessRisks, &raw, false)];
        let mut warnings = Vec::new();
        let out = build_evidence_sections(&evidence_meta(), &narratives, &mut warnings);
        assert_eq!(out.len(), 1);
        let ev = out.first().unwrap_or_else(|| panic!("ev"));
        assert!(ev.truncated);
        assert!(ev.text.len() <= MAX_SECTION_BYTES);
        assert!(warnings.contains(&EdinetWarning::NarrativeTruncated));
    }

    #[test]
    fn evidence_total_budget_truncates_overflowing_section() {
        // 3 × 100 KiB > 256 KiB: the third is truncated-to-fit with the flag.
        let piece = "a".repeat(100 * 1024);
        let narratives = vec![
            narrative(NarrativeConcept::BusinessRisks, &piece, false),
            narrative(NarrativeConcept::DescriptionOfBusiness, &piece, false),
            narrative(NarrativeConcept::ManagementAnalysis, &piece, false),
        ];
        let mut warnings = Vec::new();
        let out = build_evidence_sections(&evidence_meta(), &narratives, &mut warnings);
        assert_eq!(out.len(), 3);
        let total: usize = out.iter().map(|e| e.text.len()).sum();
        assert!(total <= MAX_TOTAL_EVIDENCE_BYTES);
        let third = out.get(2).unwrap_or_else(|| panic!("third"));
        assert!(third.truncated);
        assert_eq!(third.text.len(), MAX_TOTAL_EVIDENCE_BYTES - 2 * piece.len());
        assert!(warnings.contains(&EdinetWarning::EvidenceBudgetExceeded));
        assert!(!out.first().unwrap_or_else(|| panic!("first")).truncated);
    }

    #[test]
    fn evidence_total_budget_drops_section_when_nothing_fits() {
        // Two sections fill the 256 KiB budget exactly; the third is dropped
        // with an explicit warning (not silently truncated to nothing).
        let full = "b".repeat(MAX_SECTION_BYTES);
        let narratives = vec![
            narrative(NarrativeConcept::BusinessRisks, &full, false),
            narrative(NarrativeConcept::DescriptionOfBusiness, &full, false),
            narrative(NarrativeConcept::ManagementAnalysis, "tail", false),
        ];
        let mut warnings = Vec::new();
        let out = build_evidence_sections(&evidence_meta(), &narratives, &mut warnings);
        assert_eq!(out.len(), 2);
        assert!(warnings.contains(&EdinetWarning::EvidenceBudgetExceeded));
        assert!(!out
            .iter()
            .any(|e| e.concept == NarrativeConcept::ManagementAnalysis));
    }

    #[test]
    fn evidence_propagates_extraction_truncated_flag() {
        let narratives = vec![narrative(NarrativeConcept::BusinessRisks, "cut text", true)];
        let mut warnings = Vec::new();
        let out = build_evidence_sections(&evidence_meta(), &narratives, &mut warnings);
        assert_eq!(out.len(), 1);
        assert!(out.first().unwrap_or_else(|| panic!("ev")).truncated);
    }
}
