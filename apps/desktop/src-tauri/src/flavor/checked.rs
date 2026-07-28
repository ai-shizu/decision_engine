//! First key of the two-key witness seal (FLV-R-7 / SPEC §3.3).
//!
//! `Checked` is constructible only inside this module. Sibling `verified`
//! can name the type and call `as_str`, but cannot forge `Checked(raw)`.

use crate::flavor::policy::FlavorPolicy;
use crate::flavor::scan;

/// Opaque proof that `raw` passed `check` under a given policy.
///
/// The tuple field is **private to this module** — that is key 1.
#[derive(Debug)]
pub(crate) struct Checked(String);

impl Checked {
    /// Read-only access for the sibling `verified` module (Serialize path).
    /// Does not weaken the construction seal.
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Absolute numeric ban + charset / budget (SPEC §4). Returns `Some` only
/// when the scanner accumulates zero findings.
pub(crate) fn check(raw: &str, ctx: &FlavorPolicy) -> Option<Checked> {
    let result = scan::scan(raw, ctx);
    if result.is_clean() {
        Some(Checked(raw.to_string()))
    } else {
        None
    }
}
