//! Philox4x32-10 counter-based PRNG (SPEC §3.1–§3.2, BXS-I-06).
//!
//! Self-contained — no RNG crates. Constants and round function follow
//! Salmon et al., "Parallel Random Numbers: As Easy as 1, 2, 3" (SC'11).
//! Correctness is pinned by the three published known-answer vectors below
//! and by the stdlib Python mirror in tests/test_blackbox_sim_contract.py
//! (both sides assert the same vectors — cross-language anchor, PKBVEC01
//! one-byte-sync discipline).
//!
//! Being counter-based, any block is addressable in O(1): replay verification
//! (BXS-I-01) can reposition a stream without regenerating its prefix.

const M0: u32 = 0xD251_1F53;
const M1: u32 = 0xCD9E_8D57;
const W0: u32 = 0x9E37_79B9;
const W1: u32 = 0xBB67_AE85;
const ROUNDS: u32 = 10;

/// Stream-domain constants (SPEC §3.2). Frozen forever; never renumber,
/// never reuse a retired value (BXS-I-13, PKBTEN01 lane discipline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SeedDomain {
    Genesis = 1,
    Market = 2,
    Events = 3,
    Stimuli = 4,
    Bots = 5,
}

#[inline]
fn mulhilo(a: u32, b: u32) -> (u32, u32) {
    let p = u64::from(a).wrapping_mul(u64::from(b));
    ((p >> 32) as u32, p as u32)
}

#[inline]
fn single_round(ctr: [u32; 4], key: [u32; 2]) -> [u32; 4] {
    let [x0, x1, x2, x3] = ctr;
    let [k0, k1] = key;
    let (hi0, lo0) = mulhilo(M0, x0);
    let (hi1, lo1) = mulhilo(M1, x2);
    [hi1 ^ x1 ^ k0, lo1, hi0 ^ x3 ^ k1, lo0]
}

/// One Philox4x32-10 block: 128 bits of counter + 64 bits of key → 4×u32.
#[must_use]
pub fn philox4x32_10(counter: [u32; 4], key: [u32; 2]) -> [u32; 4] {
    let mut ctr = counter;
    let mut k = key;
    let mut round = 0u32;
    loop {
        ctr = single_round(ctr, k);
        round = round.wrapping_add(1);
        if round >= ROUNDS {
            return ctr;
        }
        k = [k0_bump(k), k1_bump(k)];
    }
}

#[inline]
fn k0_bump(k: [u32; 2]) -> u32 {
    let [k0, _] = k;
    k0.wrapping_add(W0)
}

#[inline]
fn k1_bump(k: [u32; 2]) -> u32 {
    let [_, k1] = k;
    k1.wrapping_add(W1)
}

/// Derive the campaign key from the first 8 bytes of the content-derived
/// campaign seed digest (SPEC §3.2; SHA-256 of the canonical genesis request
/// is computed by the Phase 1 Genesis builder — I-17: seeds derive from input
/// content only, never from OS entropy or wall-clock).
#[must_use]
pub fn campaign_key_from_seed(seed: [u8; 8]) -> [u32; 2] {
    let [a, b, c, d, e, f, g, h] = seed;
    [
        u32::from_le_bytes([a, b, c, d]),
        u32::from_le_bytes([e, f, g, h]),
    ]
}

/// A deterministic, domain-separated, randomly addressable stream.
///
/// key     = campaign_key with the domain constant XORed into word 0,
/// counter = [block_lo, block_hi, substream_lo, substream_hi].
#[derive(Debug, Clone)]
pub struct PhiloxStream {
    key: [u32; 2],
    substream: u64,
    block: u64,
    buf: [u32; 4],
    /// Index of the next unconsumed word in `buf`; 4 = buffer exhausted.
    buf_next: u8,
}

impl PhiloxStream {
    #[must_use]
    pub fn new(campaign_key: [u32; 2], domain: SeedDomain, substream: u64) -> Self {
        let [k0, k1] = campaign_key;
        Self {
            key: [k0 ^ (domain as u32), k1],
            substream,
            block: 0,
            buf: [0; 4],
            buf_next: 4,
        }
    }

    /// Exact position in the stream: the counter block and how much of the
    /// current 4-word buffer has been consumed. A replay that reproduced every
    /// visible number but sat at a different offset would diverge on the very
    /// next draw, so the position belongs in the state digest (BXS-I-01).
    #[must_use]
    pub fn position(&self) -> (u64, u8) {
        (self.block, self.buf_next)
    }

    #[inline]
    fn counter_for(&self, block: u64) -> [u32; 4] {
        [
            block as u32,
            (block >> 32) as u32,
            self.substream as u32,
            (self.substream >> 32) as u32,
        ]
    }

    /// Reposition to an absolute block index (replay random access, BXS-I-01).
    pub fn seek_block(&mut self, block: u64) {
        self.block = block;
        self.buf_next = 4;
    }

    pub fn next_u32(&mut self) -> u32 {
        if let Some(word) = self.buf.get(usize::from(self.buf_next)) {
            let out = *word;
            self.buf_next = self.buf_next.saturating_add(1);
            return out;
        }
        self.buf = philox4x32_10(self.counter_for(self.block), self.key);
        self.block = self.block.wrapping_add(1);
        let [w0, _, _, _] = self.buf;
        self.buf_next = 1;
        w0
    }

    pub fn next_u64(&mut self) -> u64 {
        let lo = u64::from(self.next_u32());
        let hi = u64::from(self.next_u32());
        (hi << 32) | lo
    }

    /// Uniform in [0, 1) with 53-bit resolution: `(next_u64 >> 11) · 2⁻⁵³`.
    /// Uses only exact IEEE multiplication — bit-identical on every platform
    /// (SPEC §3.3 restricted-operation policy).
    pub fn next_unit_f64(&mut self) -> f64 {
        let bits53 = self.next_u64() >> 11;
        (bits53 as f64) * (1.0 / 9_007_199_254_740_992.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Random123 published known-answer vectors for philox4x32-10
    // (kat_vectors; also mirrored by numpy's test suite).
    #[test]
    fn philox_known_answer_zero() {
        let out = philox4x32_10([0, 0, 0, 0], [0, 0]);
        assert_eq!(out, [0x6627_e8d5, 0xe169_c58d, 0xbc57_ac4c, 0x9b00_dbd8]);
    }

    #[test]
    fn philox_known_answer_max() {
        let out = philox4x32_10(
            [0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff],
            [0xffff_ffff, 0xffff_ffff],
        );
        assert_eq!(out, [0x408f_276d, 0x41c8_3b0e, 0xa20b_c7c6, 0x6d54_51fd]);
    }

    #[test]
    fn philox_known_answer_pi_digits() {
        let out = philox4x32_10(
            [0x243f_6a88, 0x85a3_08d3, 0x1319_8a2e, 0x0370_7344],
            [0xa409_3822, 0x299f_31d0],
        );
        assert_eq!(out, [0xd16c_fe09, 0x94fd_cceb, 0x5001_e420, 0x2412_6ea1]);
    }

    #[test]
    fn stream_is_deterministic() {
        let key = campaign_key_from_seed([1, 2, 3, 4, 5, 6, 7, 8]);
        let mut a = PhiloxStream::new(key, SeedDomain::Market, 7);
        let mut b = PhiloxStream::new(key, SeedDomain::Market, 7);
        let seq_a: Vec<u32> = (0..16).map(|_| a.next_u32()).collect();
        let seq_b: Vec<u32> = (0..16).map(|_| b.next_u32()).collect();
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn domain_and_substream_separation() {
        let key = campaign_key_from_seed([9, 9, 9, 9, 9, 9, 9, 9]);
        let mut market = PhiloxStream::new(key, SeedDomain::Market, 0);
        let mut events = PhiloxStream::new(key, SeedDomain::Events, 0);
        let mut market_sub1 = PhiloxStream::new(key, SeedDomain::Market, 1);
        let first_market = market.next_u64();
        assert_ne!(first_market, events.next_u64());
        assert_ne!(first_market, market_sub1.next_u64());
    }

    #[test]
    fn seek_block_random_access() {
        let key = campaign_key_from_seed([11, 22, 33, 44, 55, 66, 77, 88]);
        let mut linear = PhiloxStream::new(key, SeedDomain::Stimuli, 3);
        // Consume blocks 0 and 1 (8 words), remember block 1's words.
        let _skip: Vec<u32> = (0..4).map(|_| linear.next_u32()).collect();
        let block1: Vec<u32> = (0..4).map(|_| linear.next_u32()).collect();
        let mut seeker = PhiloxStream::new(key, SeedDomain::Stimuli, 3);
        seeker.seek_block(1);
        let replay: Vec<u32> = (0..4).map(|_| seeker.next_u32()).collect();
        assert_eq!(block1, replay);
    }

    #[test]
    fn unit_f64_is_in_half_open_range() {
        let key = campaign_key_from_seed([0xAA; 8]);
        let mut s = PhiloxStream::new(key, SeedDomain::Bots, 0);
        for _ in 0..256 {
            let u = s.next_unit_f64();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
