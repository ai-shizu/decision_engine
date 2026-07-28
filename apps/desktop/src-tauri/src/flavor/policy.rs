//! Flavor policy vessel (SPEC §3.3 / §4.4 / F-2).
//!
//! Holds edition + char budget only. Audited exception terminals live as a
//! **version-frozen scanner constant** (`scan::tables::AUDITED_V1`) — not as
//! an injectable policy field (runtime injection would be a hole).

/// Edition selector + length budget. No injectable exception table (F-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlavorPolicy {
    /// Policy edition number (meta; not a Fact payload).
    version: u32,
    /// Maximum Unicode scalar count (SPEC §4.1). Truncation is forbidden.
    max_chars: u16,
}

impl FlavorPolicy {
    /// Version 1 vessel: edition 1, generous budget for corpus / measurement.
    /// Exception terminals are **not** carried here — see `scan::tables::AUDITED_V1`.
    pub const fn v1_empty() -> Self {
        Self {
            version: 1,
            max_chars: 256,
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn max_chars(&self) -> u16 {
        self.max_chars
    }
}

impl Default for FlavorPolicy {
    fn default() -> Self {
        Self::v1_empty()
    }
}
