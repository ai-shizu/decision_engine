//! Unverified inbound wire type (SPEC §3.1 / §3.3).
//!
//! May be constructed from any string and may `Deserialize`. Carries **no**
//! proof. The only safe transition is `VerifiedFlavor::verify`.

use serde::{Deserialize, Serialize};

use crate::flavor::policy::FlavorPolicy;
use crate::flavor::verified::VerifiedFlavor;

/// Raw flavor text with no verification proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiedFlavor(pub String);

impl UnverifiedFlavor {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Delegates to `VerifiedFlavor::verify` (F-1: always `None`).
    pub fn verify(&self, ctx: &FlavorPolicy) -> Option<VerifiedFlavor> {
        VerifiedFlavor::verify(self.as_str(), ctx)
    }
}
