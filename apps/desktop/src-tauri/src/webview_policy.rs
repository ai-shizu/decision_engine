use tauri::Url;

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

    development
        && url.scheme() == "http"
        && url.host_str() == Some("localhost")
        && url.port() == Some(1420)
}

pub fn current_navigation_allowed(raw: &str) -> bool {
    navigation_allowed(raw, cfg!(debug_assertions))
}
