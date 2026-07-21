use std::net::Ipv4Addr;

use tauri::Url;

fn parse_ipv4(host: &str) -> Option<Ipv4Addr> {
    host.parse::<Ipv4Addr>().ok()
}

/// Loopback / RFC1918 / link-local — enough for Vite + TAURI_DEV_HOST on LAN.
fn is_loopback_or_private_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Some(addr) = parse_ipv4(host) {
        return addr.is_loopback() || addr.is_private() || addr.is_link_local();
    }
    // IPv6 loopback only (bracket form stripped by Url host_str).
    host == "::1"
}

pub fn navigation_allowed(raw: &str, development: bool) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let no_credentials = url.username().is_empty() && url.password().is_none();
    if !no_credentials {
        return false;
    }

    let production_origin = url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("tauri", Some("localhost")) | ("http", Some("tauri.localhost"))
        );
    if production_origin {
        return true;
    }

    // Dev frontend: Vite on :1420. iOS/Android set TAURI_DEV_HOST to a LAN IP —
    // refusing that host paints a black WKWebView (fail-silent).
    development
        && url.scheme() == "http"
        && url.port() == Some(1420)
        && url
            .host_str()
            .is_some_and(is_loopback_or_private_host)
}

pub fn current_navigation_allowed(raw: &str) -> bool {
    navigation_allowed(raw, cfg!(debug_assertions))
}

#[cfg(test)]
mod tests {
    use super::navigation_allowed;

    #[test]
    fn development_allows_localhost_and_private_lan() {
        assert!(navigation_allowed("http://localhost:1420/", true));
        assert!(navigation_allowed("http://127.0.0.1:1420/", true));
        assert!(navigation_allowed("http://192.168.1.10:1420/", true));
        assert!(navigation_allowed("http://10.0.0.2:1420/index.html", true));
        assert!(!navigation_allowed("http://8.8.8.8:1420/", true));
        assert!(!navigation_allowed("http://localhost:1421/", true));
        assert!(!navigation_allowed("https://localhost:1420/", true));
    }
}
