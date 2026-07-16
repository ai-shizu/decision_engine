//! STEP 6: networkless external-research orchestrator.
//! Production default: NetworkPolicy::Off (no transport / no egress-live).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use crate::knowledge::net_gateway::{SearchResult, MAX_SNIPPET_BYTES, MAX_TITLE_BYTES};
use crate::knowledge::render_guard::sanitize_external_text;

/// Production network policy for external research.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// Hard refuse — STEP 6 default. No Fake or live transport.
    Off,
    /// Test-only: allow injected FakeTransport / FakeResolver.
    FakeAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    PolicyOff,
    IntegrateRejected,
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PolicyOff => write!(f, "EGRESS_LIVE_NOT_READY"),
            Self::IntegrateRejected => write!(f, "E0B_VALIDATION_REJECTED"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

/// Production entry: always refuse under NetworkPolicy::Off.
pub fn refuse_if_policy_off(policy: NetworkPolicy) -> Result<(), OrchestratorError> {
    match policy {
        NetworkPolicy::Off => Err(OrchestratorError::PolicyOff),
        NetworkPolicy::FakeAllowed => Ok(()),
    }
}

/// Sanitize STEP 5 SearchResult fields before integrate.
pub fn sanitize_search_results(
    results: Vec<Vec<SearchResult>>,
) -> Result<Vec<Vec<SearchResult>>, OrchestratorError> {
    let mut out = Vec::with_capacity(results.len());
    for group in results {
        let mut sanitized = Vec::with_capacity(group.len());
        for item in group {
            let title = sanitize_external_text(&item.title, MAX_TITLE_BYTES)
                .map_err(|_| OrchestratorError::IntegrateRejected)?;
            let snippet = sanitize_external_text(&item.snippet, MAX_SNIPPET_BYTES)
                .map_err(|_| OrchestratorError::IntegrateRejected)?;
            sanitized.push(SearchResult { title, snippet });
        }
        out.push(sanitized);
    }
    Ok(out)
}
