//! Coliseum leak hard-gate (Phase 14 / I-22).
//!
//! Separate from `knowledge::render_guard` (egress sanitizer). This module only
//! rejects payloads that re-introduce Vault fossils into the interview stream.

use std::fmt;

/// Fail-closed leakage detection for oni / interviewer render paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderGuardError {
    /// `term` is a blacklist hit found in `payload_text` (deterministic scan).
    LeakageDetected { term: String },
}

impl fmt::Display for RenderGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LeakageDetected { term } => {
                write!(f, "vault fossil leakage detected: {term}")
            }
        }
    }
}

impl std::error::Error for RenderGuardError {}

/// Deterministic substring scan (Unicode case-fold via lowercase mapping).
///
/// Empty blacklist terms are ignored. Matching is literal after trim; no regex.
pub fn verify_no_leakage(
    payload_text: &str,
    blacklisted_terms: &[&str],
) -> Result<(), RenderGuardError> {
    let hay = normalize_for_scan(payload_text);
    for term in blacklisted_terms {
        let t = term.trim();
        if t.chars().count() < 2 {
            continue;
        }
        let needle = normalize_for_scan(t);
        if needle.is_empty() {
            continue;
        }
        if hay.contains(&needle) {
            return Err(RenderGuardError::LeakageDetected {
                term: t.to_string(),
            });
        }
    }
    Ok(())
}

fn normalize_for_scan(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
}

/// Oni-mode render: instructions + public brief, then leak gate against compile blacklist.
pub fn seal_oni_payload(
    instructions: &str,
    blacklisted_terms: &[String],
) -> Result<String, RenderGuardError> {
    let refs: Vec<&str> = blacklisted_terms.iter().map(String::as_str).collect();
    verify_no_leakage(instructions, &refs)?;
    Ok(instructions.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_payload_passes() {
        assert!(verify_no_leakage("抽象的なトレードオフを述べよ", &["山田商事", "4200"]).is_ok());
    }

    #[test]
    fn amount_leak_is_rejected() {
        let err = verify_no_leakage("昨日は4200円使った", &["4200"]).unwrap_err();
        assert!(matches!(err, RenderGuardError::LeakageDetected { .. }));
    }

    #[test]
    fn case_fold_detects_ascii_names() {
        assert!(verify_no_leakage("Acme Corp plan", &["acme corp"]).is_err());
    }
}
