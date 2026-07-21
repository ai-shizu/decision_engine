//! Phase 11 OCR module — Vision bridge + layout assembly.

pub mod layout;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod vision;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod commands;
