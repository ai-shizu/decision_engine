//! Bounded I/O helpers for EDINET ZIP/XML extract (V3 §4.2).
//!
//! Extract paths must not call raw `quick_xml::Reader` without going through
//! [`BoundedXmlReader`]. Per-event byte budget is enforced on the `BufRead`
//! path *before* scratch growth; attribute count/value caps are checked on Start.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::io::{BufRead, Read};
use std::time::Instant;

#[cfg(test)]
use std::time::Duration;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{NamespaceResolver, ResolveResult};
use quick_xml::reader::NsReader;
use tokio_util::sync::CancellationToken;

use crate::knowledge::edinet_client::EdinetError;
use crate::knowledge::net_gateway::GatewayError;

/// Max bytes delivered from the input stream for a single XML event (V3).
pub const MAX_XML_EVENT_BYTES: usize = 64 * 1024;
/// Hard stop on total XML events per entry.
pub const MAX_XML_EVENTS: u64 = 500_000;
/// Element nesting depth cap.
pub const MAX_XML_DEPTH: u16 = 64;
/// Max attributes on one start/empty tag.
pub const MAX_XML_ATTRIBUTES: usize = 64;
/// Max bytes of one attribute value (raw attribute bytes).
pub const MAX_XML_ATTR_VALUE_BYTES: usize = 4 * 1024;

/// Raw uncompressed entry meter: exact `cap` + EOF ok; `cap + 1` ⇒ too large.
pub struct CountingRead<R> {
    inner: R,
    read: u64,
    cap: u64,
}

impl<R> CountingRead<R> {
    pub fn new(inner: R, cap: u64) -> Self {
        Self {
            inner,
            read: 0,
            cap,
        }
    }

    pub fn bytes_read(&self) -> u64 {
        self.read
    }
}

impl<R: Read> Read for CountingRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.read > self.cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "edinet_entry_too_large",
            ));
        }
        if self.read == self.cap {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "edinet_entry_too_large",
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

/// Fixed-cap `BufRead` that charges buffered leftovers against the next event
/// budget so a huge text/attribute event cannot grow past [`MAX_XML_EVENT_BYTES`].
pub struct EventBudgetBufReader<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    end: usize,
    /// Bytes still allowed for the current event (already-buffered counts first).
    event_remaining: usize,
    max_per_event: usize,
    tripped: bool,
}

impl<R> EventBudgetBufReader<R> {
    pub fn new(inner: R, max_per_event: usize) -> Self {
        let cap = max_per_event.saturating_add(1);
        Self {
            inner,
            buf: vec![0u8; cap],
            pos: 0,
            end: 0,
            event_remaining: max_per_event,
            max_per_event,
            tripped: false,
        }
    }

    pub fn reset_event_budget(&mut self) {
        let buffered = self.end.saturating_sub(self.pos);
        if buffered > self.max_per_event {
            self.tripped = true;
            self.event_remaining = 0;
            return;
        }
        self.event_remaining = self.max_per_event.saturating_sub(buffered);
        self.tripped = false;
    }

    pub fn tripped(&self) -> bool {
        self.tripped
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }
}

impl<R: Read> Read for EventBudgetBufReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let avail = self.fill_buf()?;
        let n = avail.len().min(out.len());
        if n == 0 {
            return Ok(0);
        }
        let Some(src) = avail.get(..n) else {
            return Ok(0);
        };
        let Some(dst) = out.get_mut(..n) else {
            return Ok(0);
        };
        dst.copy_from_slice(src);
        self.consume(n);
        Ok(n)
    }
}

impl<R: Read> BufRead for EventBudgetBufReader<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.pos < self.end {
            let Some(slice) = self.buf.get(self.pos..self.end) else {
                return Ok(&[]);
            };
            return Ok(slice);
        }
        self.pos = 0;
        self.end = 0;
        if self.tripped {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "edinet_xml_event_too_large",
            ));
        }
        if self.event_remaining == 0 {
            self.tripped = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "edinet_xml_event_too_large",
            ));
        }
        let space = self.buf.len().min(self.event_remaining);
        let Some(dest) = self.buf.get_mut(..space) else {
            return Ok(&[]);
        };
        let n = self.inner.read(dest)?;
        if n == 0 {
            return Ok(&[]);
        }
        self.end = n;
        self.event_remaining = self.event_remaining.saturating_sub(n);
        let Some(slice) = self.buf.get(self.pos..self.end) else {
            return Ok(&[]);
        };
        Ok(slice)
    }

    fn consume(&mut self, amt: usize) {
        let next = self.pos.saturating_add(amt);
        self.pos = next.min(self.end);
    }
}

pub fn map_entry_io(err: std::io::Error) -> EdinetError {
    if err.kind() == std::io::ErrorKind::InvalidData {
        let msg = err.to_string();
        if msg.contains("edinet_tsv_too_large")
            || msg.contains("edinet_entry_too_large")
            || msg.contains("edinet_xml_event_too_large")
        {
            return EdinetError::TooLarge;
        }
        return EdinetError::InvalidZip;
    }
    EdinetError::Parse
}

pub fn check_deadline_cancel(
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

fn check_start_attributes(e: &BytesStart<'_>) -> Result<(), EdinetError> {
    let mut count = 0usize;
    for attr in e.attributes() {
        let a = attr.map_err(|_| EdinetError::Parse)?;
        count = count.saturating_add(1);
        if count > MAX_XML_ATTRIBUTES {
            return Err(EdinetError::TooLarge);
        }
        if a.value.len() > MAX_XML_ATTR_VALUE_BYTES {
            return Err(EdinetError::TooLarge);
        }
    }
    Ok(())
}

/// Control flow from [`BoundedXmlReader::process_next`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlDrive {
    Continue,
    Eof,
    TooLargeEvent,
}

/// Sole allowed XML adapter for EDINET extract (V3 §4.2).
pub struct BoundedXmlReader<R> {
    reader: NsReader<EventBudgetBufReader<R>>,
    scratch: Vec<u8>,
    events: u64,
    max_events: u64,
    max_event_bytes: usize,
    depth: u16,
    max_depth: u16,
}

impl<R: Read> BoundedXmlReader<R> {
    pub fn new(inner: R) -> Self {
        let budget = EventBudgetBufReader::new(inner, MAX_XML_EVENT_BYTES);
        let mut ns = NsReader::from_reader(budget);
        ns.config_mut().expand_empty_elements = false;
        ns.config_mut().check_end_names = true;
        ns.config_mut().trim_text(false);
        Self {
            reader: ns,
            scratch: Vec::with_capacity(MAX_XML_EVENT_BYTES.saturating_add(1)),
            events: 0,
            max_events: MAX_XML_EVENTS,
            max_event_bytes: MAX_XML_EVENT_BYTES,
            depth: 0,
            max_depth: MAX_XML_DEPTH,
        }
    }

    pub fn depth(&self) -> u16 {
        self.depth
    }

    pub fn into_inner(self) -> R {
        self.reader.into_inner().into_inner()
    }

    /// Read one resolved event; per-event read budget is reset first so a huge
    /// event cannot grow scratch unboundedly.
    pub fn process_next<F>(
        &mut self,
        deadline_at: Instant,
        cancel: &CancellationToken,
        mut on_event: F,
    ) -> Result<XmlDrive, EdinetError>
    where
        F: FnMut(&NamespaceResolver, ResolveResult<'_>, Event<'_>) -> Result<(), EdinetError>,
    {
        check_deadline_cancel(deadline_at, cancel)?;
        if self.events >= self.max_events {
            return Err(EdinetError::TooLarge);
        }
        self.reader.get_mut().reset_event_budget();
        if self.reader.get_ref().tripped() {
            return Ok(XmlDrive::TooLargeEvent);
        }
        self.scratch.clear();
        if self.scratch.capacity() > self.max_event_bytes.saturating_add(1) {
            self.scratch
                .shrink_to(self.max_event_bytes.saturating_add(1));
        }

        // `read_resolved_event_into` ties ResolveResult+Event to `&mut NsReader`,
        // blocking resolver access for `name=` QNames. Own the event first, then
        // resolve namespaces with shared borrows only.
        let raw = match self.reader.read_event_into(&mut self.scratch) {
            Ok(ev) => ev,
            Err(quick_xml::Error::Io(e)) => {
                if self.reader.get_ref().tripped()
                    || e.to_string().contains("edinet_xml_event_too_large")
                {
                    return Ok(XmlDrive::TooLargeEvent);
                }
                let owned = std::io::Error::new(e.kind(), e.to_string());
                return Err(map_entry_io(owned));
            }
            Err(_) => {
                if self.reader.get_ref().tripped() {
                    return Ok(XmlDrive::TooLargeEvent);
                }
                return Err(EdinetError::Parse);
            }
        };
        if self.reader.get_ref().tripped() {
            return Ok(XmlDrive::TooLargeEvent);
        }
        let ev_owned = raw.into_owned();
        let (ns, ev) = self.reader.resolver().resolve_event(ev_owned);
        self.events = self.events.saturating_add(1);

        if let Event::Start(ref e) | Event::Empty(ref e) = ev {
            if let Err(EdinetError::TooLarge) = check_start_attributes(e) {
                return Ok(XmlDrive::TooLargeEvent);
            }
        }

        match &ev {
            Event::Start(_) => {
                self.depth = self.depth.saturating_add(1);
                if self.depth > self.max_depth {
                    return Err(EdinetError::TooLarge);
                }
            }
            Event::End(_) => {
                self.depth = self.depth.saturating_sub(1);
            }
            Event::Eof => return Ok(XmlDrive::Eof),
            _ => {}
        }
        on_event(self.reader.resolver(), ns, ev)?;
        Ok(XmlDrive::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn event_budget_trips_before_huge_allocation() {
        let mut huge = String::from("<r>");
        huge.push_str(&"x".repeat(2 * 1024 * 1024));
        huge.push_str("</r>");
        let cancel = CancellationToken::new();
        let mut xml = BoundedXmlReader::new(Cursor::new(huge.into_bytes()));
        let mut saw_too_large = false;
        let mut guard = 0u32;
        loop {
            guard = guard.saturating_add(1);
            assert!(guard < 100, "must stop quickly");
            match xml.process_next(
                Instant::now() + Duration::from_secs(5),
                &cancel,
                |_res, _ns, _ev| Ok(()),
            ) {
                Ok(XmlDrive::TooLargeEvent) => {
                    saw_too_large = true;
                    break;
                }
                Ok(XmlDrive::Eof) => break,
                Ok(XmlDrive::Continue) => {}
                Err(EdinetError::TooLarge) => {
                    saw_too_large = true;
                    break;
                }
                Err(e) => panic!("unexpected {e}"),
            }
        }
        assert!(saw_too_large);
        assert!(xml.scratch.capacity() <= MAX_XML_EVENT_BYTES.saturating_add(1) * 4);
    }

    #[test]
    fn rejects_too_many_attributes() {
        let mut tag = String::from("<r");
        for i in 0..70 {
            tag.push_str(&format!(" a{i}=\"v\""));
        }
        tag.push_str("/>");
        let cancel = CancellationToken::new();
        let mut xml = BoundedXmlReader::new(Cursor::new(tag.into_bytes()));
        let drive = xml
            .process_next(
                Instant::now() + Duration::from_secs(5),
                &cancel,
                |_res, _ns, _ev| Ok(()),
            )
            .expect("drive");
        assert_eq!(drive, XmlDrive::TooLargeEvent);
    }

    #[test]
    fn rejects_long_attribute_value() {
        let long = "y".repeat(MAX_XML_ATTR_VALUE_BYTES + 8);
        let tag = format!("<r a=\"{long}\"/>");
        let cancel = CancellationToken::new();
        let mut xml = BoundedXmlReader::new(Cursor::new(tag.into_bytes()));
        let drive = xml
            .process_next(
                Instant::now() + Duration::from_secs(5),
                &cancel,
                |_res, _ns, _ev| Ok(()),
            )
            .expect("drive");
        assert_eq!(drive, XmlDrive::TooLargeEvent);
    }

    #[test]
    fn rejects_mismatched_end_names() {
        let cancel = CancellationToken::new();
        let mut xml = BoundedXmlReader::new(Cursor::new(b"<root><child></root>".to_vec()));
        let mut malformed = false;
        loop {
            match xml.process_next(
                Instant::now() + Duration::from_secs(5),
                &cancel,
                |_res, _ns, _ev| Ok(()),
            ) {
                Err(EdinetError::Parse) => {
                    malformed = true;
                    break;
                }
                Ok(XmlDrive::Continue) => {}
                Ok(XmlDrive::Eof | XmlDrive::TooLargeEvent) => break,
                Err(e) => panic!("unexpected {e}"),
            }
        }
        assert!(malformed);
    }
}
