//! E0b SSRF deny-table (STEP 5.B). Pure, manual-octet IP classification —
//! deliberately NOT using the unstable `IpAddr::is_global()` so behavior is
//! stable across toolchains. Parent: docs/SPEC_E0B_STEP5_EGRESS_GATEWAY.md §1.3.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// True if `ip` must NOT be connected to (loopback / private / link-local /
/// CGNAT / documentation / multicast / broadcast / unspecified), including
/// IPv4-mapped and NAT64-embedded IPv4 addresses (re-judged on the embedded v4).
pub fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_disallowed_v4(v4),
        IpAddr::V6(v6) => is_disallowed_v6(v6),
    }
}

fn is_disallowed_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let a = o[0];
    let b = o[1];
    if a == 0 {
        return true; // 0.0.0.0/8
    }
    if a == 10 {
        return true; // RFC1918
    }
    if a == 100 && (64..=127).contains(&b) {
        return true; // 100.64.0.0/10 CGNAT
    }
    if a == 127 {
        return true; // loopback
    }
    if a == 169 && b == 254 {
        return true; // link-local
    }
    if a == 172 && (16..=31).contains(&b) {
        return true; // RFC1918
    }
    if a == 192 && b == 0 && o[2] == 0 {
        return true; // 192.0.0.0/24 IETF protocol assignments
    }
    if a == 192 && b == 0 && o[2] == 2 {
        return true; // 192.0.2.0/24 TEST-NET-1
    }
    if a == 192 && b == 168 {
        return true; // RFC1918
    }
    if a == 198 && b == 18 {
        return true; // 198.18.0.0/15 benchmarking
    }
    if a == 198 && b == 19 {
        return true; // 198.18.0.0/15 benchmarking (upper half)
    }
    if a == 198 && b == 51 && o[2] == 100 {
        return true; // TEST-NET-2
    }
    if a == 203 && b == 0 && o[2] == 113 {
        return true; // TEST-NET-3
    }
    if a >= 224 {
        return true; // multicast (224-239) and reserved/broadcast (240-255)
    }
    false
}

fn is_disallowed_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = extract_embedded_v4(ip) {
        return is_disallowed_v4(v4);
    }
    if ip.is_unspecified() || ip.is_loopback() {
        return true;
    }
    let seg = ip.segments();
    if seg[0] & 0xfe00 == 0xfc00 {
        return true; // fc00::/7 ULA
    }
    if seg[0] & 0xffc0 == 0xfe80 {
        return true; // fe80::/10 link-local
    }
    if seg[0] == 0x2001 && seg[1] == 0x0db8 {
        return true; // 2001:db8::/32 documentation
    }
    if seg[0] & 0xff00 == 0xff00 {
        return true; // ff00::/8 multicast
    }
    false
}

/// Extract embedded IPv4 from v4-mapped (`::ffff:a.b.c.d`) or NAT64
/// (`64:ff9b::a.b.c.d`) addresses, so the deny-table judges the real target.
fn extract_embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let seg = ip.segments();
    // v4-mapped: 0:0:0:0:0:ffff:a.b:c.d
    if seg[0] == 0 && seg[1] == 0 && seg[2] == 0 && seg[3] == 0 && seg[4] == 0 && seg[5] == 0xffff
    {
        let b = ip.octets();
        return Some(Ipv4Addr::new(b[12], b[13], b[14], b[15]));
    }
    // NAT64 well-known prefix: 64:ff9b::/96
    if seg[0] == 0x0064 && seg[1] == 0xff9b && seg[2] == 0 && seg[3] == 0 && seg[4] == 0 && seg[5] == 0
    {
        let b = ip.octets();
        return Some(Ipv4Addr::new(b[12], b[13], b[14], b[15]));
    }
    None
}

/// DNS resolution seam — production impl lives behind `egress-live` (STEP 5.B/5.E);
/// tests inject fakes so no real DNS resolver runs in the default test suite.
pub trait HostResolver: Send + Sync {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, crate::knowledge::net_gateway::GatewayError>;
}
