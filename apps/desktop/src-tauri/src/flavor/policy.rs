//! Flavor policy vessel (SPEC §3.3 / §4.4 / F-2 / F-3 FLV-R-9).
//!
//! Holds edition + char budget only. Audited exception terminals live as a
//! **version-frozen scanner constant** (`scan::tables::AUDITED_V1`) — not as
//! an injectable policy field (runtime injection would be a hole).

use crate::flavor::request::TemplateId;

/// Edition selector + length budget. No injectable exception table (F-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlavorPolicy {
    /// Policy edition number (meta; not a Fact payload).
    version: u32,
    /// Maximum Unicode scalar count (SPEC §4.1). Truncation is forbidden.
    max_chars: u16,
}

impl FlavorPolicy {
    /// Corpus / measurement vessel only (FLV-R-9).
    ///
    /// Edition 1 with a deliberately loose 256-char budget so Gate-G and the
    /// frozen reject/accept corpora are not length-limited. Exception terminals
    /// are **not** carried here — see `scan::tables::AUDITED_V1`.
    ///
    /// **Generation path must not call this.** Use [`Self::for_template`] so
    /// FLV-R-5 template budgets actually reach `policy.max_chars()` / `verify`.
    pub const fn v1_empty() -> Self {
        Self {
            version: 1,
            max_chars: 256,
        }
    }

    /// Generation-path budget authority (FLV-R-9 / FLV-I-15).
    ///
    /// `version` stays 1; `max_chars` is taken from [`TemplateId::max_chars`].
    /// This is the only constructor the LLM → verify path may use.
    pub const fn for_template(template_id: TemplateId) -> Self {
        Self {
            version: 1,
            max_chars: template_id.max_chars(),
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn max_chars(&self) -> u16 {
        self.max_chars
    }
}

impl Default for FlavorPolicy {
    fn default() -> Self {
        Self::v1_empty()
    }
}

#[cfg(test)]
mod tests {
    //! Frozen population P-1 (FLAVOR_F3_IMPLEMENTATION_DIRECTIVE §3.1 / LAW-25).
    //! One case per test fn (SPEC §9.1-2) — do not fold cases back into one body.
    //! Do not rewrite expectations to match a broken implementation.

    use super::*;
    use crate::flavor::scan::{self, Finding};
    use crate::flavor::verified::VerifiedFlavor;

    fn filler(n: usize) -> String {
        "あ".repeat(n)
    }

    fn accept(policy: &FlavorPolicy, n: usize) {
        let raw = filler(n);
        assert!(
            VerifiedFlavor::verify(&raw, policy).is_some(),
            "expected ACCEPT for len={n} under max={}",
            policy.max_chars()
        );
    }

    fn reject_over_budget(policy: &FlavorPolicy, n: usize, max: u16) {
        let raw = filler(n);
        assert!(
            VerifiedFlavor::verify(&raw, policy).is_none(),
            "expected REJECT for len={n} under max={}",
            policy.max_chars()
        );
        let findings = scan::scan(&raw, policy).findings;
        assert_eq!(
            findings,
            vec![Finding::OverBudget { chars: n, max }],
            "expected sole OverBudget finding"
        );
    }

    /// P-1-1: ArenaEventHeadline @ 48 → ACCEPT
    #[test]
    fn p1_1_headline_48_accepts() {
        accept(&FlavorPolicy::for_template(TemplateId::ArenaEventHeadline), 48);
    }

    /// P-1-2: ArenaEventHeadline @ 49 → REJECT OverBudget { chars: 49, max: 48 }
    #[test]
    fn p1_2_headline_49_rejects_over_budget() {
        reject_over_budget(
            &FlavorPolicy::for_template(TemplateId::ArenaEventHeadline),
            49,
            48,
        );
    }

    /// P-1-3: ArenaEventAside @ 72 → ACCEPT
    #[test]
    fn p1_3_aside_72_accepts() {
        accept(&FlavorPolicy::for_template(TemplateId::ArenaEventAside), 72);
    }

    /// P-1-4: ArenaEventAside @ 73 → REJECT OverBudget { chars: 73, max: 72 }
    #[test]
    fn p1_4_aside_73_rejects_over_budget() {
        reject_over_budget(
            &FlavorPolicy::for_template(TemplateId::ArenaEventAside),
            73,
            72,
        );
    }

    /// P-1-5: SettlementRipple @ 64 → ACCEPT
    #[test]
    fn p1_5_ripple_64_accepts() {
        accept(&FlavorPolicy::for_template(TemplateId::SettlementRipple), 64);
    }

    /// P-1-6: SettlementRipple @ 65 → REJECT OverBudget { chars: 65, max: 64 }
    #[test]
    fn p1_6_ripple_65_rejects_over_budget() {
        reject_over_budget(
            &FlavorPolicy::for_template(TemplateId::SettlementRipple),
            65,
            64,
        );
    }

    /// P-1-7: same "あ"×60 under Headline → REJECT (intent record; see audit note on redundancy)
    #[test]
    fn p1_7_headline_60_rejects_over_budget() {
        reject_over_budget(
            &FlavorPolicy::for_template(TemplateId::ArenaEventHeadline),
            60,
            48,
        );
    }

    /// P-1-8: same "あ"×60 under Aside → ACCEPT (paired with P-1-7)
    #[test]
    fn p1_8_aside_60_accepts() {
        accept(&FlavorPolicy::for_template(TemplateId::ArenaEventAside), 60);
    }

    /// P-1-9: v1_empty measurement vessel accepts "あ"×200
    #[test]
    fn p1_9_v1_empty_200_accepts() {
        accept(&FlavorPolicy::v1_empty(), 200);
    }

    #[test]
    fn for_template_max_chars_matches_template_id() {
        assert_eq!(
            FlavorPolicy::for_template(TemplateId::ArenaEventHeadline).max_chars(),
            TemplateId::ArenaEventHeadline.max_chars()
        );
        assert_eq!(
            FlavorPolicy::for_template(TemplateId::ArenaEventAside).max_chars(),
            TemplateId::ArenaEventAside.max_chars()
        );
        assert_eq!(
            FlavorPolicy::for_template(TemplateId::SettlementRipple).max_chars(),
            TemplateId::SettlementRipple.max_chars()
        );
        assert_eq!(FlavorPolicy::v1_empty().max_chars(), 256);
        assert_eq!(FlavorPolicy::for_template(TemplateId::ArenaEventHeadline).version(), 1);
    }
}
