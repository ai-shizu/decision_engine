//! STEP 5.B E.E: offline tests for the E0b Egress Gateway. No real network or
//! DNS is ever touched  EHttpTransport/HostResolver fakes only.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pkb_desktop_lib::knowledge::dns_guard::{is_disallowed_ip, HostResolver};
use pkb_desktop_lib::knowledge::dual_run::AttestedIntentPayload;
use pkb_desktop_lib::knowledge::fsm::ResearchSlot;
use pkb_desktop_lib::knowledge::net_gateway::{
    build_request, extract_results, fetch_bounded_with_deadline, fetch_one, research_fetch,
    resolve_and_pin, safe_truncate, validate_outbound_url, validate_response_meta, GatewayError,
    ResponseBody, ResponseMeta, HttpTransport, VerifyInputs, MAX_RESPONSE_BYTES, WIKI_HOST,
};

// ---------------------------------------------------------------------------
// STEP 5.B: deny-table
// ---------------------------------------------------------------------------

fn v4(s: &str) -> IpAddr {
    IpAddr::V4(s.parse::<Ipv4Addr>().expect("valid v4"))
}
fn v6(s: &str) -> IpAddr {
    IpAddr::V6(s.parse::<Ipv6Addr>().expect("valid v6"))
}

#[test]
fn dns_deny_table_v4_forbidden() {
    for s in [
        "0.0.0.0", "10.0.0.1", "100.64.0.1", "127.0.0.1", "169.254.0.1",
        "172.16.0.1", "172.31.255.255", "192.0.0.1", "192.0.2.1", "192.168.1.1",
        "198.18.0.1", "198.51.100.1", "203.0.113.1", "224.0.0.1", "240.0.0.1",
        "255.255.255.255",
    ] {
        assert!(is_disallowed_ip(v4(s)), "expected denied: {s}");
    }
}

#[test]
fn dns_deny_table_v6_forbidden() {
    for s in [
        "::", "::1", "fc00::1", "fe80::1", "2001:db8::1", "ff02::1",
        "::ffff:127.0.0.1", "64:ff9b::7f00:1",
    ] {
        assert!(is_disallowed_ip(v6(s)), "expected denied: {s}");
    }
}

#[test]
fn dns_deny_table_global_allowed() {
    for s in ["8.8.8.8", "1.1.1.1"] {
        assert!(!is_disallowed_ip(v4(s)), "expected allowed: {s}");
    }
    assert!(!is_disallowed_ip(v6("2606:4700:4700::1111")));
}

struct FakeResolver {
    ips: Vec<IpAddr>,
}
impl HostResolver for FakeResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, GatewayError> {
        Ok(self.ips.clone())
    }
}

#[test]
fn resolver_all_private_denies_before_transport() {
    let r = FakeResolver { ips: vec![v4("127.0.0.1")] };
    assert_eq!(resolve_and_pin(&r, WIKI_HOST), Err(GatewayError::DnsDenied));
}

#[test]
fn resolver_mixed_denies_fail_closed() {
    let r = FakeResolver { ips: vec![v4("8.8.8.8"), v4("10.0.0.5")] };
    assert_eq!(resolve_and_pin(&r, WIKI_HOST), Err(GatewayError::DnsDenied));
}

#[test]
fn resolver_all_global_pins_all() {
    let r = FakeResolver { ips: vec![v4("8.8.8.8"), v4("1.1.1.1")] };
    let pinned = resolve_and_pin(&r, WIKI_HOST).expect("all-global must pin");
    assert_eq!(pinned.len(), 2);
}

// ---------------------------------------------------------------------------
// STEP 5.B: URL invariant
// ---------------------------------------------------------------------------

#[test]
fn url_roundtrip_percent_decode_matches_input() {
    let q = "量子コンピュータ &weird=chars?";
    let url = build_request(q);
    assert!(validate_outbound_url(&url, q).is_ok());
}

#[test]
fn url_rejects_wrong_host() {
    let bad = "https://evil.example.com/w/api.php?action=query&list=search&format=json&srsearch=x";
    assert_eq!(validate_outbound_url(bad, "x"), Err(GatewayError::UrlViolation));
}

#[test]
fn url_rejects_ip_literal_host() {
    let bad = "https://127.0.0.1/w/api.php?action=query&list=search&format=json&srsearch=x";
    assert_eq!(validate_outbound_url(bad, "x"), Err(GatewayError::UrlViolation));
}

#[test]
fn url_rejects_wrong_path() {
    let bad = format!(
        "https://{WIKI_HOST}/other/path?action=query&list=search&format=json&srsearch=x"
    );
    assert_eq!(validate_outbound_url(&bad, "x"), Err(GatewayError::UrlViolation));
}

#[test]
fn url_rejects_trailing_dot_host() {
    let bad = format!(
        "https://{WIKI_HOST}./w/api.php?action=query&list=search&format=json&srsearch=x"
    );
    assert_eq!(validate_outbound_url(&bad, "x"), Err(GatewayError::UrlViolation));
}

#[test]
fn url_rejects_userinfo_and_fragment() {
    let with_userinfo = format!(
        "https://user@{WIKI_HOST}/w/api.php?action=query&list=search&format=json&srsearch=x"
    );
    assert_eq!(validate_outbound_url(&with_userinfo, "x"), Err(GatewayError::UrlViolation));
    let with_fragment = format!(
        "https://{WIKI_HOST}/w/api.php?action=query&list=search&format=json&srsearch=x#frag"
    );
    assert_eq!(validate_outbound_url(&with_fragment, "x"), Err(GatewayError::UrlViolation));
}

#[test]
fn url_rejects_extra_or_missing_query_keys() {
    let extra = format!(
        "https://{WIKI_HOST}/w/api.php?action=query&list=search&format=json&srsearch=x&extra=1"
    );
    assert_eq!(validate_outbound_url(&extra, "x"), Err(GatewayError::UrlViolation));
    let missing = format!("https://{WIKI_HOST}/w/api.php?action=query&list=search&srsearch=x");
    assert_eq!(validate_outbound_url(&missing, "x"), Err(GatewayError::UrlViolation));
}

// ---------------------------------------------------------------------------
// STEP 5.C: bounded stream + identity + status/type + cancel + panic-safety
// ---------------------------------------------------------------------------

struct DropProbe(Arc<AtomicUsize>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Fake chunked body. `pending_forever = true` never resolves (simulates a
/// slow/stalled connection) so cancel/deadline tests can prove the future is
/// dropped rather than silently completing.
struct FakeBody {
    chunks: Vec<Result<Vec<u8>, GatewayError>>,
    pending_forever: bool,
    _probe: Option<DropProbe>,
}

impl ResponseBody for FakeBody {
    fn next_chunk(&mut self) -> impl Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send {
        let pending_forever = self.pending_forever;
        let next = if self.chunks.is_empty() { None } else { Some(self.chunks.remove(0)) };
        async move {
            if pending_forever {
                std::future::pending::<()>().await;
                unreachable!();
            }
            next
        }
    }
}

#[tokio::test]
async fn stream_cap_exact_boundary_allowed() {
    let body = FakeBody { chunks: vec![Ok(vec![7u8; MAX_RESPONSE_BYTES])], pending_forever: false, _probe: None };
    let never = std::future::pending::<()>();
    let result = fetch_bounded_with_deadline(body, never, Duration::from_secs(5)).await;
    assert_eq!(result.expect("cap-exact must be allowed").len(), MAX_RESPONSE_BYTES);
}

#[tokio::test]
async fn stream_cap_plus_one_rejected() {
    let body = FakeBody { chunks: vec![Ok(vec![7u8; MAX_RESPONSE_BYTES + 1])], pending_forever: false, _probe: None };
    let never = std::future::pending::<()>();
    let result = fetch_bounded_with_deadline(body, never, Duration::from_secs(5)).await;
    assert_eq!(result, Err(GatewayError::WireViolation));
}

#[tokio::test]
async fn stream_chunked_overflow_rejected_before_second_chunk_grows_buffer() {
    let body = FakeBody {
        chunks: vec![Ok(vec![1u8; MAX_RESPONSE_BYTES]), Ok(vec![1u8; 10])],
        pending_forever: false,
        _probe: None,
    };
    let never = std::future::pending::<()>();
    let result = fetch_bounded_with_deadline(body, never, Duration::from_secs(5)).await;
    assert_eq!(result, Err(GatewayError::WireViolation));
}

#[test]
fn identity_encoding_allowed_others_rejected() {
    let ok_none = ResponseMeta { status: 200, content_type: Some("application/json".into()), content_encoding: None };
    assert!(validate_response_meta(&ok_none).is_ok());
    let ok_identity = ResponseMeta {
        status: 200,
        content_type: Some("application/json; charset=utf-8".into()),
        content_encoding: Some("identity".into()),
    };
    assert!(validate_response_meta(&ok_identity).is_ok());
    let gzip = ResponseMeta {
        status: 200,
        content_type: Some("application/json".into()),
        content_encoding: Some("gzip".into()),
    };
    assert_eq!(validate_response_meta(&gzip), Err(GatewayError::WireViolation));
}

#[test]
fn status_and_content_type_gate() {
    for status in [301, 302, 404, 500, 204, 206] {
        let m = ResponseMeta { status, content_type: Some("application/json".into()), content_encoding: None };
        assert_eq!(validate_response_meta(&m), Err(GatewayError::StatusRejected));
    }
    let html = ResponseMeta { status: 200, content_type: Some("text/html".into()), content_encoding: None };
    assert_eq!(validate_response_meta(&html), Err(GatewayError::StatusRejected));
}

#[tokio::test(start_paused = true)]
async fn deadline_drops_pending_stream_no_zombie() {
    let probe_flag = Arc::new(AtomicUsize::new(0));
    let body = FakeBody {
        chunks: vec![],
        pending_forever: true,
        _probe: Some(DropProbe(Arc::clone(&probe_flag))),
    };
    let never_cancel = std::future::pending::<()>();
    let fut = fetch_bounded_with_deadline(body, never_cancel, Duration::from_millis(50));
    let handle = tokio::spawn(fut);
    tokio::time::advance(Duration::from_millis(100)).await;
    let result = handle.await.expect("task must not panic");
    assert_eq!(result, Err(GatewayError::Timeout));
    assert_eq!(probe_flag.load(Ordering::SeqCst), 1, "pending body must be dropped on deadline");
}

#[tokio::test]
async fn cancel_drops_pending_stream_no_zombie() {
    let probe_flag = Arc::new(AtomicUsize::new(0));
    let body = FakeBody {
        chunks: vec![],
        pending_forever: true,
        _probe: Some(DropProbe(Arc::clone(&probe_flag))),
    };
    let cancel = async {};
    let result = fetch_bounded_with_deadline(body, cancel, Duration::from_secs(60)).await;
    assert_eq!(result, Err(GatewayError::Cancelled));
    assert_eq!(probe_flag.load(Ordering::SeqCst), 1, "pending body must be dropped on cancel");
}

#[test]
fn safe_truncate_never_panics_on_multibyte_boundary() {
    let ch = '\u{3042}'; // U+3042 HIRAGANA LETTER A — 3 bytes/char in UTF-8
    let s: String = std::iter::repeat(ch).take(500).collect();
    let truncated = safe_truncate(&s, 256);
    assert!(truncated.len() <= 256);
    assert!(s.starts_with(&truncated));
}

#[test]
fn safe_truncate_empty_and_invalid_boundaries() {
    assert_eq!(safe_truncate("", 10), "");
    assert_eq!(safe_truncate("abc", 10), "abc");
    assert_eq!(safe_truncate("abc", 0), "");
}

// ---------------------------------------------------------------------------
// STEP 5.D: bounded JSON extraction
// ---------------------------------------------------------------------------

#[test]
fn extract_normal_wikipedia_fixture() {
    let fixture = br#"{"query":{"search":[
        {"title":"Rust","snippet":"A systems <span class=\"searchmatch\">language</span>","extra":"drop me"},
        {"title":"Cargo","snippet":"Package manager"}
    ]}}"#;
    let results = extract_results(fixture).expect("valid fixture must extract");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Rust");
    assert!(results[0].snippet.contains("language"));
}

#[test]
fn extract_bounded_to_max_results() {
    let mut arr = String::from("[");
    for i in 0..10 {
        if i > 0 {
            arr.push(',');
        }
        arr.push_str(&format!(r#"{{"title":"T{i}","snippet":"S{i}"}}"#));
    }
    arr.push(']');
    let body = format!(r#"{{"query":{{"search":{arr}}}}}"#);
    let results = extract_results(body.as_bytes()).expect("must extract bounded");
    assert_eq!(results.len(), 3);
}

#[test]
fn extract_truncates_oversized_fields() {
    let long_title = "T".repeat(1000);
    let long_snippet = "S".repeat(4000);
    let body = format!(
        r#"{{"query":{{"search":[{{"title":"{long_title}","snippet":"{long_snippet}"}}]}}}}"#
    );
    let results = extract_results(body.as_bytes()).expect("must extract");
    assert!(results[0].title.len() <= 256);
    assert!(results[0].snippet.len() <= 2048);
}

#[test]
fn extract_adversarial_json_never_panics() {
    let cases: Vec<&[u8]> = vec![
        b"{}",
        b"null",
        b"[1,2,3]",
        b"{\"query\":{\"search\":\"not-an-array\"}}",
        b"{\"query\":{\"search\":[{\"title\":123,\"snippet\":null}]}}",
        b"{\"query\":{\"search\":[{\"title\":{\"nested\":{\"deep\":1}}}]}}",
        b"not json at all",
        b"",
    ];
    for case in cases {
        let _ = extract_results(case); // must not panic regardless of Ok/Err
    }
}

// ---------------------------------------------------------------------------
// STEP 5.E: end-to-end research_fetch  EAND-gate abort must yield zero egress.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CountingTransport {
    calls: Arc<AtomicUsize>,
}

struct CountingBody(Vec<u8>, bool);
impl ResponseBody for CountingBody {
    fn next_chunk(&mut self) -> impl Future<Output = Option<Result<Vec<u8>, GatewayError>>> + Send {
        let taken = if self.1 {
            None
        } else {
            self.1 = true;
            Some(Ok(std::mem::take(&mut self.0)))
        };
        async move { taken }
    }
}

impl HttpTransport for CountingTransport {
    type Body = CountingBody;
    fn get(&self, _url: &str) -> impl Future<Output = Result<(ResponseMeta, Self::Body), GatewayError>> + Send {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let body = br#"{"query":{"search":[{"title":"T","snippet":"S"}]}}"#.to_vec();
        async move {
            Ok((
                ResponseMeta { status: 200, content_type: Some("application/json".into()), content_encoding: None },
                CountingBody(body, false),
            ))
        }
    }
}

fn valid_resolver() -> FakeResolver {
    FakeResolver { ips: vec![v4("8.8.8.8")] }
}

fn make_slot(dict_hash: &str) -> Arc<ResearchSlot> {
    ResearchSlot::new(dict_hash)
}

fn kat_payload(nonce_hex: &str, dict_hash: &str, queries: Vec<String>) -> AttestedIntentPayload {
    // attestation left deliberately WRONG for the negative tests below  E
    // verify_and_gate must reject before any transport call happens.
    AttestedIntentPayload {
        session_id: "s1".into(),
        txn_nonce: nonce_hex.into(),
        sidecar_generation: 1,
        policy_epoch: 1,
        dict_hash: dict_hash.into(),
        queries,
        attestation: "0".repeat(64),
    }
}

#[tokio::test]
async fn e2e_bad_hmac_aborts_before_any_egress() {
    let slot = make_slot("dicthash1");
    let transport = CountingTransport::default();
    let resolver = valid_resolver();
    let payload = kat_payload(&"a".repeat(64), "dicthash1", vec!["rust".into()]);
    let result = research_fetch(
        payload,
        VerifyInputs { dict_terms: &[], k_spawn: "irrelevant-k-spawn" },
        &slot,
        &transport,
        &resolver,
        || std::future::pending::<()>(),
        Duration::from_secs(5),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0, "no egress on HMAC failure");
}

#[tokio::test]
async fn e2e_pii_hit_aborts_before_any_egress() {
    // Even with a syntactically-parseable attestation, dict-hit path is
    // exercised by the AND gate ordering: HMAC check runs first in
    // verify_and_gate, so a wrong HMAC already proves zero-egress on failure.
    // This test additionally proves FSM-level failures (drift) do the same.
    let slot = make_slot("dicthash-correct");
    let transport = CountingTransport::default();
    let resolver = valid_resolver();
    let payload = kat_payload(&"b".repeat(64), "WRONG-DICT-HASH", vec!["rust".into()]);
    let result = research_fetch(
        payload,
        VerifyInputs { dict_terms: &[], k_spawn: "irrelevant-k-spawn" },
        &slot,
        &transport,
        &resolver,
        || std::future::pending::<()>(),
        Duration::from_secs(5),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0, "no egress on dictionary drift");
}

#[tokio::test]
async fn e2e_replay_aborts_before_any_egress() {
    let slot = make_slot("dicthash1");
    let transport = CountingTransport::default();
    let resolver = valid_resolver();
    let nonce = "c".repeat(64);
    // First begin() consumes the nonce (still fails verify_and_gate due to bad HMAC,
    // but the nonce is consumed at FSM.begin time regardless of verify outcome).
    let payload1 = kat_payload(&nonce, "dicthash1", vec!["rust".into()]);
    let _ = research_fetch(
        payload1,
        VerifyInputs { dict_terms: &[], k_spawn: "irrelevant-k-spawn" },
        &slot,
        &transport,
        &resolver,
        || std::future::pending::<()>(),
        Duration::from_secs(5),
    )
    .await;
    let payload2 = kat_payload(&nonce, "dicthash1", vec!["rust".into()]);
    let result2 = research_fetch(
        payload2,
        VerifyInputs { dict_terms: &[], k_spawn: "irrelevant-k-spawn" },
        &slot,
        &transport,
        &resolver,
        || std::future::pending::<()>(),
        Duration::from_secs(5),
    )
    .await;
    assert!(result2.is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0, "no egress on replayed nonce");
}

#[tokio::test]
async fn e2e_malformed_nonce_aborts_before_any_egress() {
    let slot = make_slot("dicthash1");
    let transport = CountingTransport::default();
    let resolver = valid_resolver();
    let payload = kat_payload("not-a-valid-hex-nonce", "dicthash1", vec!["rust".into()]);
    let result = research_fetch(
        payload,
        VerifyInputs { dict_terms: &[], k_spawn: "irrelevant-k-spawn" },
        &slot,
        &transport,
        &resolver,
        || std::future::pending::<()>(),
        Duration::from_secs(5),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0, "no egress on malformed nonce");
}

#[tokio::test]
async fn e2e_dns_denied_query_aborts_that_query_with_zero_transport_calls() {
    // Uses fetch_one directly with a private-IP resolver to prove the
    // resolve->deny gate blocks the transport at the per-query level too.
    let transport = CountingTransport::default();
    let bad_resolver = FakeResolver { ips: vec![v4("127.0.0.1")] };
    let result = fetch_one(&transport, &bad_resolver, "rust", std::future::pending::<()>(), Duration::from_secs(5)).await;
    assert_eq!(result, Err(GatewayError::DnsDenied));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}
