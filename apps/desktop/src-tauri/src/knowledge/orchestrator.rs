//! STEP 6: networkless external-research orchestrator.
//! Production default: NetworkPolicy::Off (no transport / no egress-live).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use crate::knowledge::dual_run::AttestedIntentPayload;
use crate::knowledge::fsm::{ReadyToIntegrate, ResearchSlot, Txn};
use crate::knowledge::net_gateway::{
    research_fetch, GatewayError, HttpTransport, SearchResult, VerifyInputs,
    MAX_SNIPPET_BYTES, MAX_TITLE_BYTES,
};
use crate::knowledge::render_guard::sanitize_external_text;

/// Production network policy for external research.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NetworkPolicy {
    /// Hard refuse — default until user consent (Settings toggle).
    Off,
    /// User consented via Settings; still requires egress-live build for real egress.
    Live,
    /// Test-only: allow injected FakeTransport (STEP 6.G).
    FakeAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    PolicyOff,
    IntegrateRejected,
    Gateway(GatewayError),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PolicyOff => write!(f, "EGRESS_LIVE_NOT_READY"),
            Self::IntegrateRejected => write!(f, "E0B_VALIDATION_REJECTED"),
            Self::Gateway(_) => write!(f, "E0B_VALIDATION_REJECTED"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

impl From<GatewayError> for OrchestratorError {
    fn from(value: GatewayError) -> Self {
        Self::Gateway(value)
    }
}

/// Refuse when policy is Off (no user consent).
pub fn refuse_if_policy_off(policy: NetworkPolicy) -> Result<(), OrchestratorError> {
    match policy {
        NetworkPolicy::Off => Err(OrchestratorError::PolicyOff),
        NetworkPolicy::Live | NetworkPolicy::FakeAllowed => Ok(()),
    }
}

/// Second factor: egress-live feature must be compiled in for real network.
pub fn refuse_if_egress_unavailable() -> Result<(), OrchestratorError> {
    #[cfg(not(feature = "egress-live"))]
    {
        let _ = ();
        Err(OrchestratorError::PolicyOff)
    }
    #[cfg(feature = "egress-live")]
    {
        Ok(())
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

/// Injected transport/cancel/deadline for test-only fake egress.
pub struct InjectedFetch<'a, T, CF> {
    pub transport: &'a T,
    pub cancel_factory: CF,
    pub deadline: Duration,
}

/// Networkless FakeAllowed path: verify ∧ FSM ∧ FakeTransport ∧ sanitize.
/// Production must never call this with live transport; callers inject fakes.
pub async fn run_networkless_research<T, CF, C>(
    policy: NetworkPolicy,
    payload: AttestedIntentPayload,
    verify: VerifyInputs<'_>,
    slot: &Arc<ResearchSlot>,
    fetch: InjectedFetch<'_, T, CF>,
) -> Result<(Txn<ReadyToIntegrate>, Vec<Vec<SearchResult>>), OrchestratorError>
where
    T: HttpTransport,
    CF: FnMut() -> C,
    C: Future<Output = ()>,
{
    refuse_if_policy_off(policy)?;
    let InjectedFetch {
        transport,
        cancel_factory,
        deadline,
    } = fetch;
    let (ready, raw) = research_fetch(
        payload,
        verify,
        slot,
        transport,
        cancel_factory,
        deadline,
    )
    .await?;
    let sanitized = sanitize_search_results(raw)?;
    Ok((ready, sanitized))
}
