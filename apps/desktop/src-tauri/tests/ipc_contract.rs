use pkb_desktop_lib::ipc_contract::{
    CalendarIcsRequest, ConsultRequest, ImportDocumentRequest, ProbeAnswerRequest,
    RecordLoadRequest, ScopeRequest, ValidateRequest,
};
use pkb_desktop_lib::webview_policy::navigation_allowed;

#[test]
fn renderer_requests_reject_unknown_keys_and_wrong_types() {
    assert!(
        serde_json::from_value::<RecordLoadRequest>(serde_json::json!({
            "date": "2026-07-14"
        }))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<RecordLoadRequest>(serde_json::json!({
            "date": "2026-07-14",
            "cmd": "record.save"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RecordLoadRequest>(serde_json::json!({
            "date": 20260714
        }))
        .is_err()
    );

    assert!(serde_json::from_value::<ConsultRequest>(serde_json::json!({
        "query": "local question",
        "mode": "consult"
    }))
    .is_ok());
    assert!(serde_json::from_value::<ConsultRequest>(serde_json::json!({
        "query": "local question",
        "mode": "arbitrary"
    }))
    .is_err());
    assert!(serde_json::from_value::<ConsultRequest>(serde_json::json!({
        "query": "local question",
        "config": {
            "industry": "",
            "genre": "",
            "difficulty": "standard",
            "stance": "standard",
            "injected": true
        }
    }))
    .is_err());

    assert!(
        serde_json::from_value::<ImportDocumentRequest>(serde_json::json!({
            "content": "text",
            "filename": "note.md",
            "dest": "knowledge"
        }))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<ImportDocumentRequest>(serde_json::json!({
            "content": "text",
            "filename": "note.md",
            "dest": "outside"
        }))
        .is_err()
    );

    assert!(
        serde_json::from_value::<ProbeAnswerRequest>(serde_json::json!({
            "session_id": "session",
            "question_id": "question",
            "answer": "answer",
            "today": "2026-07-14",
            "extra": "forbidden"
        }))
        .is_err()
    );
}

#[test]
fn semantic_request_validation_fails_closed() {
    let empty_query: ConsultRequest = serde_json::from_value(serde_json::json!({
        "query": "   "
    }))
    .unwrap();
    assert!(empty_query.validate().is_err());

    let no_ics_payload: CalendarIcsRequest = serde_json::from_value(serde_json::json!({
        "mode": "append"
    }))
    .unwrap();
    assert!(no_ics_payload.validate().is_err());

    let both_ics_payloads: CalendarIcsRequest = serde_json::from_value(serde_json::json!({
        "mode": "append",
        "ics_content": "BEGIN:VCALENDAR",
        "ics_files": [{"content": "BEGIN:VCALENDAR", "filename": "a.ics"}]
    }))
    .unwrap();
    assert!(both_ics_payloads.validate().is_err());

    let valid_ics: CalendarIcsRequest = serde_json::from_value(serde_json::json!({
        "mode": "overwrite",
        "ics_content": "BEGIN:VCALENDAR"
    }))
    .unwrap();
    assert!(valid_ics.validate().is_ok());

    let dyad_without_alias: ScopeRequest = serde_json::from_value(serde_json::json!({
        "scope": "dyad",
        "alias": null
    }))
    .unwrap();
    assert!(dyad_without_alias.validate().is_err());
}

#[test]
fn production_navigation_is_exact_origin_only() {
    for allowed in [
        "tauri://localhost/",
        "tauri://localhost/index.html#profile",
        "http://tauri.localhost/",
    ] {
        assert!(navigation_allowed(allowed, false), "{allowed}");
    }
    for rejected in [
        "https://example.com/",
        "http://tauri.localhost.evil.example/",
        "http://tauri.localhost:81/",
        "http://user@tauri.localhost/",
        "javascript:alert(1)",
        "data:text/html,exfiltrate",
        "file:///C:/secret.txt",
        "http://localhost:1420/",
        "not a url",
    ] {
        assert!(!navigation_allowed(rejected, false), "{rejected}");
    }
}

#[test]
fn development_navigation_adds_one_exact_local_origin() {
    assert!(navigation_allowed("http://localhost:1420/", true));
    assert!(!navigation_allowed("http://127.0.0.1:1420/", true));
    assert!(!navigation_allowed("http://localhost:1421/", true));
    assert!(!navigation_allowed("https://localhost:1420/", true));
}
