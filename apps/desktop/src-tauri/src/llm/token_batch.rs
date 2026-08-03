//! Micro-batch coalescing for streamed token IPC (Phase 9).
//!
//! Sending one Channel message per sampled token saturates the IPC bridge and
//! forces React to re-render on every piece. This batcher flushes when either:
//! - at least [`BATCH_MAX_PIECES`] pieces accumulated, or
//! - [`BATCH_MAX_MS`] milliseconds elapsed since the last flush,
//! then invokes the caller-supplied sink once with concatenated text.
//!
//! Stream *content* order is preserved and deterministic. Wall-clock flush
//! timing affects only IPC cadence (F-14: no RNG).

use std::time::{Duration, Instant};

/// Max pieces coalesced before a forced flush.
pub const BATCH_MAX_PIECES: u32 = 8;
/// Max delay (ms) before a forced flush.
pub const BATCH_MAX_MS: u64 = 30;

pub struct TokenStreamBatcher<F>
where
    F: FnMut(u32, String) -> Result<(), String>,
{
    send: F,
    buf: String,
    pieces: u32,
    seq_last: u32,
    last_flush: Instant,
}

impl<F> TokenStreamBatcher<F>
where
    F: FnMut(u32, String) -> Result<(), String>,
{
    pub fn new(send: F) -> Self {
        Self {
            send,
            buf: String::new(),
            pieces: 0,
            seq_last: 0,
            last_flush: Instant::now(),
        }
    }

    pub fn push(&mut self, seq: u32, piece: &str) -> Result<(), String> {
        if piece.is_empty() {
            return Ok(());
        }
        if self.buf.is_empty() {
            self.last_flush = Instant::now();
        }
        self.buf.push_str(piece);
        self.pieces = self.pieces.saturating_add(1);
        self.seq_last = seq;
        if self.pieces >= BATCH_MAX_PIECES
            || self.last_flush.elapsed() >= Duration::from_millis(BATCH_MAX_MS)
        {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(&mut self.buf);
        let seq = self.seq_last;
        self.pieces = 0;
        self.last_flush = Instant::now();
        (self.send)(seq, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn flushes_after_max_pieces() {
        let out = RefCell::new(Vec::<(u32, String)>::new());
        let mut b = TokenStreamBatcher::new(|seq, text| {
            out.borrow_mut().push((seq, text));
            Ok(())
        });
        for i in 0..BATCH_MAX_PIECES {
            b.push(i, "x").unwrap();
        }
        assert_eq!(out.borrow().len(), 1);
        assert_eq!(out.borrow()[0].1, "x".repeat(BATCH_MAX_PIECES as usize));
    }

    #[test]
    fn flush_emits_remainder() {
        let out = RefCell::new(Vec::<(u32, String)>::new());
        let mut b = TokenStreamBatcher::new(|seq, text| {
            out.borrow_mut().push((seq, text));
            Ok(())
        });
        b.push(0, "a").unwrap();
        b.push(1, "b").unwrap();
        b.flush().unwrap();
        assert_eq!(out.borrow().len(), 1);
        assert_eq!(out.borrow()[0], (1, "ab".into()));
    }

    #[test]
    fn constants_match_phase9_spec() {
        assert_eq!(BATCH_MAX_MS, 30);
        assert_eq!(BATCH_MAX_PIECES, 8);
    }
}
