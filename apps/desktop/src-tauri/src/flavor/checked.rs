//! First key of the two-key witness seal (FLV-R-7 / SPEC §3.3).
//!
//! `Checked` is constructible only inside this module. Sibling `verified`
//! can name the type and call `as_str`, but cannot forge `Checked(raw)`.

use crate::flavor::policy::FlavorPolicy;

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

/// F-1 skeleton: always reject. F-2 implements the absolute numeric ban
/// (SPEC §4). The type chain `check` → `Checked` → `VerifiedFlavor` is
/// complete; only the detector body is deferred.
pub(crate) fn check(_raw: &str, _ctx: &FlavorPolicy) -> Option<Checked> {
    None
}
