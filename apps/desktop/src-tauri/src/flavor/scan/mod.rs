//! Four-stage flavor guard scanner (SPEC §4 / F-2).
//!
//! Classification only — no NFKC, no folding, no truncation.
//! Stage order is load-bearing: shield (2) before numeral (3) is why
//! `一気に` passes and `万が一、一万円` only shields the former.

use std::ops::Range;

use crate::flavor::policy::FlavorPolicy;

mod charset;
#[cfg(test)]
mod gate_g_measure;
mod numeral;
mod shield;
pub(crate) mod tables;

/// Reasoned rejection findings (FLV-R-3 telemetry material).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Finding {
    Invisible { at: usize, cp: u32 },
    Markup { span: Range<usize> },
    UnicodeNumeral { at: usize, ch: char },
    HanNumeral { at: usize, ch: char },
    LexicalQuantity { span: Range<usize> },
    OverBudget { chars: usize, max: u16 },
}

impl Finding {
    #[cfg(test)]
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Finding::Invisible { .. } => "Invisible",
            Finding::Markup { .. } => "Markup",
            Finding::UnicodeNumeral { .. } => "UnicodeNumeral",
            Finding::HanNumeral { .. } => "HanNumeral",
            Finding::LexicalQuantity { .. } => "LexicalQuantity",
            Finding::OverBudget { .. } => "OverBudget",
        }
    }
}

/// Full scan result — all findings accumulated even after the first hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scan {
    pub findings: Vec<Finding>,
}

impl Scan {
    pub(crate) fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Run stages 1→4. Findings are never truncated mid-input.
pub(crate) fn scan(raw: &str, policy: &FlavorPolicy) -> Scan {
    let mut findings = Vec::new();
    charset::scan(raw, &mut findings);
    let shields = shield::collect_shields(raw);
    numeral::scan(raw, &shields, &mut findings);
    let chars = raw.chars().count();
    let max = policy.max_chars();
    if chars > max as usize {
        findings.push(Finding::OverBudget { chars, max });
    }
    Scan { findings }
}

#[cfg(test)]
mod shield_boundary_detail {
    use super::*;
    use crate::flavor::policy::FlavorPolicy;

    #[test]
    fn shield_boundary_findings_expose_万円_not_万が一() {
        let policy = FlavorPolicy::v1_empty();
        let raw = "万が一、一万円がかかる。";
        let s = scan(raw, &policy);
        assert!(!s.is_clean(), "must reject: {raw}");

        // `一` of `一万円` must be HanNumeral; `一` inside `万が一` must not.
        let ichi_man = raw.find("一万円").expect("一万円");
        let man_ichi_ga = raw.find("万が一").expect("万が一");

        let han_ats: Vec<usize> = s
            .findings
            .iter()
            .filter_map(|f| match f {
                Finding::HanNumeral { at, ch: '一' } => Some(*at),
                _ => None,
            })
            .collect();

        assert!(
            han_ats.contains(&ichi_man),
            "expected HanNumeral at 一万円 (byte {ichi_man}); findings={:?} ats={han_ats:?}",
            s.findings
        );
        assert!(
            !han_ats.iter().any(|&at| (man_ichi_ga..man_ichi_ga + "万が一".len()).contains(&at)),
            "万が一's 一 must be shielded; findings={:?}",
            s.findings
        );

        // Telemetry dump for Gate E.
        eprintln!("GATE_E sample={raw:?}");
        for f in &s.findings {
            eprintln!("GATE_E finding={f:?} kind={}", f.kind_name());
        }
    }

    #[test]
    fn shield_boundary_ikki_ni_does_not_cover_ichi_man() {
        let policy = FlavorPolicy::v1_empty();
        let raw = "一気に一万が消えた。";
        let s = scan(raw, &policy);
        assert!(!s.is_clean(), "must reject: {raw}");
        let ichi_man = raw.find("一万").expect("一万");
        assert!(
            s.findings.iter().any(|f| matches!(
                f,
                Finding::HanNumeral { at, ch: '一' } if *at == ichi_man
            )),
            "expected HanNumeral at 一万; findings={:?}",
            s.findings
        );
        eprintln!("GATE_E sample={raw:?}");
        for f in &s.findings {
            eprintln!("GATE_E finding={f:?} kind={}", f.kind_name());
        }
    }
}
