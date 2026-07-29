//! LLM flavor generation adapter (SPEC §11 / F-3 T-3 / T-4).
//!
//! Single **file** under `llm/` — never a `llm/flavor/` directory (FLV-W-08).
//! Pure `render_prompt` / `decide` are model-free; `generate` is a thin impure
//! wrapper. Arena wiring is `blackbox_arena::flavor_slot` (T-4).
//!
//! # Seal discipline (trap 1)
//!
//! `VerifiedFlavor` still has exactly one construction path: `verify`.
//! On discard we call `scan` a second time for `Finding` telemetry only —
//! never a `verify_with_findings` that would mint witnesses.

use crate::flavor::policy::FlavorPolicy;
use crate::flavor::request::{FlavorRequest, SlotValue, TemplateId};
use crate::flavor::scan::{self, Finding};
use crate::flavor::verified::VerifiedFlavor;

/// Separable “do not write numerals” clause (FLV-R-12). T-8 arm B removes this.
pub(crate) const NO_NUMERALS_INSTRUCTION: &str =
    "数字や数量を表す語を書くな。漢数字も用いない。";

/// Generation / decision outcome. Keep `Discarded` and `Unavailable` distinct
/// (§14.3 denominator / FLV-R-11).
#[derive(Debug)]
pub(crate) enum FlavorOutcome {
    Accepted(VerifiedFlavor),
    /// Guard rejected the completion; findings are telemetry only.
    Discarded(Vec<Finding>),
    /// Model absent, timeout, feature off, or no completion candidate.
    Unavailable,
}

/// Deterministic prompt render. No numeric literals (FLV-W-02).
/// Single-line / no Cc: newlines are Invisible to the flavor scanner (P-5).
pub(crate) fn render_prompt(req: &FlavorRequest) -> String {
    let mut out = String::new();
    out.push_str("役割: アリーナの雰囲気を伝える短い散文を書く。 ");
    out.push_str("テンプレート: ");
    out.push_str(template_label(req.template_id));
    out.push_str("。 雰囲気: ");
    for (i, slot) in req.slots.iter().enumerate() {
        if i > 0 {
            out.push('、');
        }
        out.push_str(slot_label(slot.tag));
    }
    out.push_str("。 制約: ");
    out.push_str(NO_NUMERALS_INSTRUCTION);
    out.push_str(" 短く書け。事実や計器の読みを述べるな。雰囲気だけを書け。");
    let _ = (req.schema, req.locale); // closed enums; reserved for future locale copy
    out
}

/// Pure accept/discard/unavailable decision. Uses `for_template` only
/// (never `v1_empty`). Content-empty inputs are Unavailable here — not in
/// `generate` alone — so every entry path shares the judgment (T-3b).
pub(crate) fn decide(raw: &str, template_id: TemplateId) -> FlavorOutcome {
    // No non-whitespace scalars ⇒ no completion candidate (P-6-7…11).
    // Defined as Unicode White_Space only (`trim`); not a charset/Invisible concern.
    if raw.trim().is_empty() {
        return FlavorOutcome::Unavailable;
    }
    let policy = FlavorPolicy::for_template(template_id);
    match VerifiedFlavor::verify(raw, &policy) {
        Some(v) => FlavorOutcome::Accepted(v),
        None => {
            // Second scan on the discard path only — telemetry, not a mint path.
            let findings = scan::scan(raw, &policy).findings;
            FlavorOutcome::Discarded(findings)
        }
    }
}

/// Thin impure wrapper. `completion = None` means model absent / timeout /
/// feature off. Present text (including empty) is delegated to `decide`
/// once — no retry, no second emptiness policy.
pub(crate) fn generate(request: &FlavorRequest, completion: Option<&str>) -> FlavorOutcome {
    let _prompt = render_prompt(request);
    match completion {
        None => FlavorOutcome::Unavailable,
        Some(raw) => decide(raw, request.template_id),
    }
}

fn template_label(id: TemplateId) -> &'static str {
    match id {
        TemplateId::ArenaEventHeadline => "短い見出し",
        TemplateId::ArenaEventAside => "短い傍注",
        TemplateId::SettlementRipple => "短い余韻",
    }
}

fn slot_label(tag: SlotValue) -> &'static str {
    match tag {
        SlotValue::LossSeverityHigh => "損失感が高い",
        SlotValue::LossSeverityModerate => "損失感はややある",
        SlotValue::LossSeverityLow => "損失感は薄い",
        SlotValue::TrendRising => "上昇の気配",
        SlotValue::TrendFalling => "下降の気配",
        SlotValue::TrendFlat => "横ばいの気配",
        SlotValue::PhaseSettlement => "決着の気配",
        SlotValue::PhaseExpansion => "拡大の気配",
        SlotValue::PhaseContraction => "収縮の気配",
        SlotValue::MoodTense => "緊張した空気",
        SlotValue::MoodCalm => "静かな空気",
        SlotValue::MoodVolatile => "不安定な空気",
    }
}

#[cfg(test)]
mod tests {
    //! Frozen populations P-5 / P-6 / P-7 (LAW-25). Do not rewrite expectations.

    use super::*;
    use crate::flavor::request::{FlavorLocale, FlavorSchema, FlavorSlot, SlotId};

    fn slot_id_for(tag: SlotValue) -> SlotId {
        match tag {
            SlotValue::LossSeverityHigh
            | SlotValue::LossSeverityModerate
            | SlotValue::LossSeverityLow => SlotId::LossSeverity,
            SlotValue::TrendRising | SlotValue::TrendFalling | SlotValue::TrendFlat => SlotId::Trend,
            SlotValue::PhaseSettlement
            | SlotValue::PhaseExpansion
            | SlotValue::PhaseContraction => SlotId::Phase,
            SlotValue::MoodTense | SlotValue::MoodCalm | SlotValue::MoodVolatile => SlotId::Mood,
        }
    }

    const ALL_TEMPLATES: &[TemplateId] = &[
        TemplateId::ArenaEventHeadline,
        TemplateId::ArenaEventAside,
        TemplateId::SettlementRipple,
    ];

    const ALL_SLOT_VALUES: &[SlotValue] = &[
        SlotValue::LossSeverityHigh,
        SlotValue::LossSeverityModerate,
        SlotValue::LossSeverityLow,
        SlotValue::TrendRising,
        SlotValue::TrendFalling,
        SlotValue::TrendFlat,
        SlotValue::PhaseSettlement,
        SlotValue::PhaseExpansion,
        SlotValue::PhaseContraction,
        SlotValue::MoodTense,
        SlotValue::MoodCalm,
        SlotValue::MoodVolatile,
    ];

    fn request(template_id: TemplateId, tag: SlotValue) -> FlavorRequest {
        FlavorRequest {
            schema: FlavorSchema::V1,
            template_id,
            slots: vec![FlavorSlot {
                id: slot_id_for(tag),
                tag,
            }],
            locale: FlavorLocale::Ja,
        }
    }

    fn is_numeral_class(f: &Finding) -> bool {
        matches!(
            f,
            Finding::UnicodeNumeral { .. }
                | Finding::HanNumeral { .. }
                | Finding::LexicalQuantity { .. }
                | Finding::Invisible { .. }
                | Finding::Markup { .. }
        )
    }

    /// P-5: every closed TemplateId × SlotValue prompt is free of numeral-class findings.
    #[test]
    fn p5_rendered_prompts_have_no_numeral_class_findings() {
        let policy = FlavorPolicy::for_template(TemplateId::ArenaEventAside);
        for &tid in ALL_TEMPLATES {
            for &tag in ALL_SLOT_VALUES {
                let prompt = render_prompt(&request(tid, tag));
                let bad: Vec<_> = scan::scan(&prompt, &policy)
                    .findings
                    .into_iter()
                    .filter(is_numeral_class)
                    .collect();
                assert!(
                    bad.is_empty(),
                    "P-5 failed for {tid:?}/{tag:?}: {bad:?}\nprompt:\n{prompt}"
                );
            }
        }
    }

    fn filler(n: usize) -> String {
        "あ".repeat(n)
    }

    fn assert_accepted(raw: &str, tid: TemplateId) {
        match decide(raw, tid) {
            FlavorOutcome::Accepted(_) => {}
            other => panic!("expected Accepted, got {other:?} for {raw:?}"),
        }
    }

    fn assert_discarded_with(
        raw: &str,
        tid: TemplateId,
        pred: impl Fn(&[Finding]) -> bool,
        label: &str,
    ) {
        match decide(raw, tid) {
            FlavorOutcome::Discarded(findings) => {
                assert!(pred(&findings), "{label}: findings={findings:?}");
            }
            other => panic!("expected Discarded ({label}), got {other:?}"),
        }
    }

    #[test]
    fn p6_1_headline_48_accepted() {
        assert_accepted(&filler(48), TemplateId::ArenaEventHeadline);
    }

    #[test]
    fn p6_2_headline_49_discarded_over_budget() {
        assert_discarded_with(
            &filler(49),
            TemplateId::ArenaEventHeadline,
            |f| f.iter().any(|x| matches!(x, Finding::OverBudget { .. })),
            "OverBudget",
        );
    }

    #[test]
    fn p6_3_headline_han_numeral_discarded() {
        assert_discarded_with(
            "損失は二千五百に達した",
            TemplateId::ArenaEventHeadline,
            |f| f.iter().any(|x| matches!(x, Finding::HanNumeral { .. })),
            "HanNumeral",
        );
    }

    #[test]
    fn p6_4_headline_lexical_quantity_discarded() {
        assert_discarded_with(
            "市場が大幅に動いた",
            TemplateId::ArenaEventHeadline,
            |f| {
                f.iter()
                    .any(|x| matches!(x, Finding::LexicalQuantity { .. }))
            },
            "LexicalQuantity",
        );
    }

    #[test]
    fn p6_5_aside_60_accepted() {
        assert_accepted(&filler(60), TemplateId::ArenaEventAside);
    }

    #[test]
    fn p6_6_headline_60_discarded() {
        assert_discarded_with(
            &filler(60),
            TemplateId::ArenaEventHeadline,
            |f| f.iter().any(|x| matches!(x, Finding::OverBudget { .. })),
            "OverBudget",
        );
    }

    /// P-6-7: empty completion → Unavailable (not Accepted, not Discarded).
    #[test]
    fn p6_7_empty_string_is_unavailable() {
        let req = request(
            TemplateId::ArenaEventHeadline,
            SlotValue::MoodCalm,
        );
        match generate(&req, Some("")) {
            FlavorOutcome::Unavailable => {}
            other => panic!("P-6-7 expected Unavailable, got {other:?}"),
        }
    }

    fn assert_decide_unavailable(raw: &str, label: &str) {
        match decide(raw, TemplateId::ArenaEventHeadline) {
            FlavorOutcome::Unavailable => {}
            other => panic!("{label}: expected Unavailable from decide, got {other:?}"),
        }
    }

    /// P-6-8: empty via **decide** directly (not generate).
    #[test]
    fn p6_8_decide_empty_is_unavailable() {
        assert_decide_unavailable("", "P-6-8");
    }

    /// P-6-9: ASCII spaces only via decide.
    #[test]
    fn p6_9_decide_ascii_spaces_unavailable() {
        assert_decide_unavailable("   ", "P-6-9");
    }

    /// P-6-10: one ideographic space via decide.
    #[test]
    fn p6_10_decide_ideographic_space_unavailable() {
        assert_decide_unavailable("\u{3000}", "P-6-10");
    }

    /// P-6-11: two ideographic spaces via decide.
    #[test]
    fn p6_11_decide_ideographic_spaces_unavailable() {
        assert_decide_unavailable("\u{3000}\u{3000}", "P-6-11");
    }

    /// P-6-12: ideographic space + content → Accepted (positive companion).
    #[test]
    fn p6_12_decide_space_plus_content_accepted() {
        assert_accepted("\u{3000}あ", TemplateId::ArenaEventHeadline);
    }

    #[test]
    fn p7_1_model_absent_is_unavailable() {
        let req = request(
            TemplateId::ArenaEventHeadline,
            SlotValue::MoodCalm,
        );
        match generate(&req, None) {
            FlavorOutcome::Unavailable => {}
            other => panic!("P-7-1 expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn p7_2_guard_failure_is_discarded_not_unavailable() {
        let req = request(
            TemplateId::ArenaEventHeadline,
            SlotValue::MoodCalm,
        );
        match generate(&req, Some("損失は二千五百に達した")) {
            FlavorOutcome::Discarded(findings) => {
                assert!(!findings.is_empty(), "Discarded must carry findings");
            }
            other => panic!("P-7-2 expected Discarded, got {other:?}"),
        }
    }
}
