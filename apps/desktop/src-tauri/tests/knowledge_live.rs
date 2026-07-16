//! STEP 7.C: opt-in live Wikipedia E2E (`#[ignore]` + `PKB_E0B_LIVE=1` only).
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(feature = "egress-live")]
mod live {
    use std::time::Duration;

    use pkb_desktop_lib::knowledge::net_gateway::{
        fetch_one, ReqwestTransport, MAX_SNIPPET_BYTES, MAX_TITLE_BYTES,
    };
    use pkb_desktop_lib::knowledge::render_guard::sanitize_external_text;

    #[ignore]
    #[tokio::test]
    async fn live_wikipedia_search_api_json_and_sanitize() {
        if std::env::var("PKB_E0B_LIVE").as_deref() != Ok("1") {
            return;
        }
        let transport = ReqwestTransport::new().expect("egress-live client");
        let results = fetch_one(
            &transport,
            "rust programming",
            std::future::pending::<()>(),
            Duration::from_secs(30),
        )
        .await
        .expect("live fetch");
        assert!(!results.is_empty(), "expected at least one search hit");
        for hit in &results {
            assert!(hit.title.len() <= MAX_TITLE_BYTES);
            assert!(hit.snippet.len() <= MAX_SNIPPET_BYTES);
            let title = sanitize_external_text(&hit.title, MAX_TITLE_BYTES).expect("title sanitize");
            let snippet =
                sanitize_external_text(&hit.snippet, MAX_SNIPPET_BYTES).expect("snippet sanitize");
            assert!(!title.contains("```"));
            assert!(!title.contains("[INST]"));
            assert!(!snippet.contains("<script>"));
        }
    }
}
