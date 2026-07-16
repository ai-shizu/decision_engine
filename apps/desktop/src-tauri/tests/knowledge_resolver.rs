//! STEP 7.A: offline SafeKnowledgeResolver + enforce_deny_table (no real DNS/TLS).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;

use pkb_desktop_lib::knowledge::net_gateway::{
    enforce_deny_table, AsyncLookup, GatewayError, SafeKnowledgeResolver,
};
use reqwest::dns::Resolve;

fn sa4(ip: &str, port: u16) -> SocketAddr {
    SocketAddr::from((ip.parse::<Ipv4Addr>().expect("v4"), port))
}

fn sa6(ip: &str, port: u16) -> SocketAddr {
    SocketAddr::from((ip.parse::<Ipv6Addr>().expect("v6"), port))
}

// ---------------------------------------------------------------------------
// enforce_deny_table (fail-closed pure function)
// ---------------------------------------------------------------------------

#[test]
fn enforce_allows_all_global() {
    let addrs = vec![sa4("8.8.8.8", 443), sa4("1.1.1.1", 443)];
    let out = enforce_deny_table(addrs.clone()).expect("all global");
    assert_eq!(out, addrs);
}

#[test]
fn enforce_mixed_global_and_private_fails_closed() {
    let addrs = vec![sa4("8.8.8.8", 443), sa4("10.0.0.5", 443)];
    assert_eq!(enforce_deny_table(addrs), Err(GatewayError::DnsDenied));
}

#[test]
fn enforce_empty_fails_closed() {
    assert_eq!(enforce_deny_table(vec![]), Err(GatewayError::DnsDenied));
}

#[test]
fn enforce_v4_mapped_loopback_denied() {
    let addrs = vec![sa6("::ffff:127.0.0.1", 443)];
    assert_eq!(enforce_deny_table(addrs), Err(GatewayError::DnsDenied));
}

#[test]
fn enforce_nat64_loopback_denied() {
    let addrs = vec![sa6("64:ff9b::7f00:1", 443)];
    assert_eq!(enforce_deny_table(addrs), Err(GatewayError::DnsDenied));
}

// ---------------------------------------------------------------------------
// SafeKnowledgeResolver via reqwest::dns::Resolve (fake AsyncLookup)
// ---------------------------------------------------------------------------

struct FakeLookup {
    addrs: Vec<SocketAddr>,
}

impl AsyncLookup for FakeLookup {
    fn lookup(
        &self,
        _host: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, GatewayError>> + Send + '_>> {
        let addrs = self.addrs.clone();
        Box::pin(async move { Ok(addrs) })
    }
}

#[tokio::test]
async fn resolver_mixed_ips_fail_closed() {
    let resolver = SafeKnowledgeResolver::new(FakeLookup {
        addrs: vec![sa4("8.8.8.8", 443), sa4("10.0.0.5", 443)],
    });
    let name: reqwest::dns::Name = "ja.wikipedia.org".parse().expect("name");
    let result = resolver.resolve(name).await;
    assert!(result.is_err(), "mixed private must not return Ok");
}

#[tokio::test]
async fn resolver_all_global_ok() {
    let expected = vec![sa4("8.8.8.8", 443), sa6("2606:4700:4700::1111", 443)];
    let resolver = SafeKnowledgeResolver::new(FakeLookup {
        addrs: expected.clone(),
    });
    let name: reqwest::dns::Name = "ja.wikipedia.org".parse().expect("name");
    let addrs = resolver.resolve(name).await.expect("all global");
    let collected: Vec<SocketAddr> = addrs.collect();
    assert_eq!(collected, expected);
}

#[tokio::test]
async fn resolver_empty_lookup_fails_closed() {
    let resolver = SafeKnowledgeResolver::new(FakeLookup { addrs: vec![] });
    let name: reqwest::dns::Name = "ja.wikipedia.org".parse().expect("name");
    let result = resolver.resolve(name).await;
    assert!(result.is_err(), "empty must not return Ok");
}
