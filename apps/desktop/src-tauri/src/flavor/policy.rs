//! Flavor policy vessel (SPEC §3.3 / §4.4).
//!
//! F-1: empty audited-exception table. Detector body lands in F-2.

/// Edition of the audited exception table + empty exceptions (F-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlavorPolicy {
    /// Policy edition number (meta; not a Fact payload).
    version: u32,
    /// Audited exception terminals. Empty in F-1.
    exceptions: &'static [&'static str],
}

impl FlavorPolicy {
    /// F-1 default vessel: version 1, empty exception table.
    pub const fn v1_empty() -> Self {
        Self {
            version: 1,
            exceptions: &[],
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn exceptions(&self) -> &'static [&'static str] {
        self.exceptions
    }
}

impl Default for FlavorPolicy {
    fn default() -> Self {
        Self::v1_empty()
    }
}
