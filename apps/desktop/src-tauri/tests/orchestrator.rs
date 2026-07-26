//! STEP 6.G — networkless orchestrator (policy Off / Fake E2E / AND-gate abort).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pkb_desktop_lib::knowledge::dual_run::AttestedIntentPayload;
use pkb_desktop_lib::knowledge::fsm::ResearchSlot;
use pkb_desktop_lib::knowledge::net_gateway::{
    GatewayError, HttpTransport, ResponseBody, ResponseMeta, VerifyInputs,
};
use pkb_desktop_lib::knowledge::orchestrator::{
    refuse_if_policy_off, run_networkless_research, InjectedFetch, NetworkPolicy,
    OrchestratorError,
};

// KAT-2 from STEP 2 / STEP 3 golden (single query, valid HMAC).
const K_SPAWN: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const SESSION: &str = "0123456789abcdeffedcba9876543210";
const NONCE: &str = "00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100";
const DICT_HASH: &str = "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0";
const TAG: &str = "c36b1fe4c46564af2dfd5310c771b5bf95eb81daff071a2e8b9bb87aac284861";
const QUERY: &str = "python async";

#[test]
fn policy_off_refuses_without_touching_transport() {
    assert_eq!(
        refuse_if_policy_off(NetworkPolicy::Off),
        Err(OrchestratorError::PolicyOff)
    );
    assert_eq!(
        refuse_if_policy_off(NetworkPolicy::Off)
            .err()
            .unwrap()
            .to_string(),
        "EGRESS_LIVE_NOT_READY"
    );
}

struct CountingTransport {
    calls: AtomicUsize,
    body: Vec<u8>,
}

impl Default for CountingTransport {
    fn default() -> Self {
        let wiki = serde_json::json!({
            "query": {
                "search": [
                    {"title": "## evil [INST]", "snippet": "```code``` keep"}
                ]
            }
        });
        Self {
            calls: AtomicUsize::new(0),
            body: serde_json::to_vec(&wiki).unwrap(),
        }
    }
}

struct CountingBody {
    data: Option<Vec<u8>>,
}

impl ResponseBody for CountingBody {
    fn next_chunk(&mut self) -> impl Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send {
        let next = self.data.take().map(Ok);
        async move { next }
    }
}

impl HttpTransport for CountingTransport {
    type Body = CountingBody;

    async fn get(
        &self,
        _url: &str,
        _request_deadline: std::time::Duration,
    ) -> Result<(ResponseMeta, Self::Body), GatewayError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok((
            ResponseMeta {
                status: 200,
                content_type: Some("application/json".into()),
                content_encoding: Some("identity".into()),
                content_length: None,
            },
            CountingBody {
                data: Some(self.body.clone()),
            },
        ))
    }
}

fn valid_payload(nonce: &str) -> AttestedIntentPayload {
    AttestedIntentPayload {
        session_id: SESSION.into(),
        txn_nonce: nonce.into(),
        sidecar_generation: 1,
        policy_epoch: 1,
        dict_hash: DICT_HASH.into(),
        queries: vec![QUERY.into()],
        attestation: TAG.into(),
    }
}

#[tokio::test]
async fn policy_off_aborts_before_any_fake_transport_call() {
    let slot = ResearchSlot::new(DICT_HASH);
    let transport = CountingTransport::default();
    match run_networkless_research(
        NetworkPolicy::Off,
        valid_payload(NONCE),
        VerifyInputs {
            dict_terms: &[],
            k_spawn: K_SPAWN,
        },
        &slot,
        InjectedFetch {
            transport: &transport,
            cancel_factory: std::future::pending::<()>,
            deadline: Duration::from_secs(5),
        },
    )
    .await
    {
        Err(e) => assert_eq!(e, OrchestratorError::PolicyOff),
        Ok(_) => panic!("policy Off must refuse before transport"),
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn and_gate_bad_hmac_aborts_before_transport() {
    let slot = ResearchSlot::new(DICT_HASH);
    let transport = CountingTransport::default();
    let mut payload = valid_payload(&"a".repeat(64));
    payload.attestation = "0".repeat(64);
    match run_networkless_research(
        NetworkPolicy::FakeAllowed,
        payload,
        VerifyInputs {
            dict_terms: &[],
            k_spawn: K_SPAWN,
        },
        &slot,
        InjectedFetch {
            transport: &transport,
            cancel_factory: std::future::pending::<()>,
            deadline: Duration::from_secs(5),
        },
    )
    .await
    {
        Err(OrchestratorError::Gateway(_)) => {}
        Err(other) => panic!("expected gateway rejection, got {other:?}"),
        Ok(_) => panic!("bad HMAC must abort before transport"),
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn fake_e2e_sanitizes_results_with_zero_real_network() {
    let slot = ResearchSlot::new(DICT_HASH);
    let transport = CountingTransport::default();
    let (ready, results) = run_networkless_research(
        NetworkPolicy::FakeAllowed,
        valid_payload(NONCE),
        VerifyInputs {
            dict_terms: &[],
            k_spawn: K_SPAWN,
        },
        &slot,
        InjectedFetch {
            transport: &transport,
            cancel_factory: std::future::pending::<()>,
            deadline: Duration::from_secs(5),
        },
    )
    .await
    .expect("fake e2e");
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].len(), 1);
    let title = &results[0][0].title;
    let snippet = &results[0][0].snippet;
    assert!(!title.contains("##"));
    assert!(!title.contains("[INST]"));
    assert!(!snippet.contains("```"));
    assert!(snippet.contains("keep"));
    // Completing the typestate path must be possible after sanitize.
    let _completed = ready.transition_to_completed();
}
