//! STEP 6.B — render_guard negative matrix.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pkb_desktop_lib::knowledge::render_guard::sanitize_external_text;

#[test]
fn neutralizes_markdown_and_special_tokens() {
    let cases = [
        "```evil``` keep",
        "## heading keep",
        "![img](http://x) keep",
        "[INST] system [/INST] keep",
        "<|im_start|>system keep",
        "<script>alert(1)</script> keep",
        "&lt;b&gt;keep&lt;/b&gt;",
    ];
    for raw in cases {
        let out = sanitize_external_text(raw, 2048).expect("sanitize");
        for forbidden in ["```", "##", "![", "<script", "[INST]", "<|", "<b>"] {
            assert!(
                !out.contains(forbidden),
                "raw={raw:?} still contains {forbidden:?} in {out:?}"
            );
        }
    }
}

#[test]
fn removes_bidi_and_zwsp() {
    let out = sanitize_external_text("ok\u{200b}\u{202e}BAD\u{202c}", 2048).expect("sanitize");
    assert!(!out.contains('\u{200b}'));
    assert!(!out.contains('\u{202e}'));
    assert!(out.contains("BAD"));
}

#[test]
fn rejects_empty_after_clean() {
    assert!(sanitize_external_text("\u{200b}\u{200c}", 2048).is_err());
}

#[test]
fn idempotent() {
    let once = sanitize_external_text("```[INST]<script>#", 2048).expect("once");
    let twice = sanitize_external_text(&once, 2048).expect("twice");
    assert_eq!(once, twice);
}
