//! BLACKBOX profile outlets (Phase 6-A step 6 / R-9).
//!
//! # Single-accessor shape
//!
//! Every legal outlet (consult / 講評 / PROFILE UI) formats a
//! [`LoadedProfile`] / [`ProfileMeta`] obtained **only** through
//! [`super::blackbox_repo::get_latest_profile`] /
//! [`super::blackbox_repo::list_profiles`]. This module never opens SQL.
//!
//! # Honest N/A
//!
//! `value_micro: None` renders as the literal 「未測定」 — never `0`,
//! never 「平均的」, never a blank that looks like a measured zero.

use serde::Serialize;

use crate::blackbox_sim::bias::{
    BiasAxis, BiasEstimate, CALIBRATION_UNCALIBRATED, INSTRUMENT_ID, SCHEMA_BLACKBOX_PROFILE_V1,
};
use crate::blackbox_sim::money::MICRO;

use super::blackbox_repo::{LoadedProfile, ProfileMeta};

/// Token budget for the consult / 講評 text block (mirrors Tensor section scale).
///
/// Production consumer is `llm::consult_context` (compiled only under
/// `pocket-brain`). Without that feature the constant is intentionally idle.
#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
const BLACKBOX_SECTION_TOKEN_BUDGET: usize = 400;

/// Newtype: the only value the three R-9 outlets are allowed to display.
///
/// Constructed from [`LoadedProfile`] (or the soft-absent sentinel). Downstream
/// code that wants a profile for prompt/UI must take this type — not a raw
/// SQL row — so interview/GD/es_review cannot quietly grow a second reader.
///
/// Production consumer: `llm::consult_context` (`pocket-brain`). PROFILE UI uses
/// [`profile_view_from_loaded`] / [`meta_view`] directly.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
pub(crate) struct BlackboxOutletSnapshot {
    inner: Option<LoadedProfile>,
}

#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
impl BlackboxOutletSnapshot {
    #[must_use]
    pub(crate) fn from_loaded(loaded: LoadedProfile) -> Self {
        Self {
            inner: Some(loaded),
        }
    }

    /// Soft absent — outlets degrade to 「未測定」, never Err.
    #[must_use]
    pub(crate) fn absent() -> Self {
        Self { inner: None }
    }

    #[must_use]
    pub(crate) fn as_loaded(&self) -> Option<&LoadedProfile> {
        self.inner.as_ref()
    }
}

/// Wire DTO for PROFILE UI (exact-key FE parser).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BlackboxLaneView {
    pub lane: u8,
    pub axis: &'static str,
    pub label_ja: &'static str,
    /// `None` → FE must render 「未測定」 (never coerce to 0).
    pub value_micro: Option<i64>,
    pub n_obs: u32,
    pub sufficiency_micro: i64,
    pub measured: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BlackboxProfileView {
    pub schema_version: String,
    pub instrument: String,
    pub calibration: String,
    pub pooled_campaigns: u32,
    pub estimated_at: i64,
    pub pool_digest_hex: String,
    pub lanes: Vec<BlackboxLaneView>,
    /// Authority boundary — always present so UI cannot drop it.
    pub authority_note: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BlackboxProfileMetaView {
    pub pool_digest_hex: String,
    pub schema_version: String,
    pub instrument: String,
    pub calibration: String,
    pub pooled_campaigns: u32,
    pub estimated_at: i64,
    pub updated_at: i64,
}

pub(crate) const AUTHORITY_NOTE: &str =
    "これは校正前の計測器（uncalibrated-instrument）による暫定値である。確定した性格として扱うな。";

fn axis_label_ja(axis: BiasAxis) -> &'static str {
    match axis {
        BiasAxis::LossAversion => "損失回避",
        BiasAxis::DispositionEffect => "処分効果",
        BiasAxis::Anchoring => "アンカリング",
        BiasAxis::Overconfidence => "過信",
        BiasAxis::EscalationCommitment => "エスカレーション",
        BiasAxis::PressureDegradation => "プレッシャー下劣化",
    }
}

fn axis_id(axis: BiasAxis) -> &'static str {
    match axis {
        BiasAxis::LossAversion => "loss_aversion",
        BiasAxis::DispositionEffect => "disposition_effect",
        BiasAxis::Anchoring => "anchoring",
        BiasAxis::Overconfidence => "overconfidence",
        BiasAxis::EscalationCommitment => "escalation_commitment",
        BiasAxis::PressureDegradation => "pressure_degradation",
    }
}

fn digest_hex(digest: &[u8; 8]) -> String {
    hex::encode(digest)
}

#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
fn format_value_micro(value_micro: Option<i64>) -> String {
    match value_micro {
        None => "未測定".to_string(),
        Some(v) => {
            // Integer-only (N-1): six decimal places from micro without f64.
            let sign = if v < 0 { "-" } else { "" };
            let abs = v.unsigned_abs();
            let whole = abs / (MICRO as u64);
            let frac = abs % (MICRO as u64);
            format!("{sign}{whole}.{frac:06}")
        }
    }
}

#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
fn format_sufficiency(sufficiency_micro: i64) -> String {
    // Integer-only display (N-1): avoid f64 in the instrument path.
    // MICRO = 1_000_000 → three decimal places via /1000 of the fractional part.
    let clamped = sufficiency_micro.clamp(0, MICRO);
    let whole = clamped / MICRO;
    let frac = (clamped % MICRO) / 1_000;
    format!("{whole}.{frac:03}")
}

/// Consult / 講評 text. Absent snapshot → soft 「未測定」 block (no Err).
///
/// Production consumer: `llm::consult_context` (`pocket-brain`).
#[must_use]
#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
pub(crate) fn format_consult_block(snapshot: &BlackboxOutletSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!("instrument: {INSTRUMENT_ID}\n"));
    out.push_str(&format!("calibration: {CALIBRATION_UNCALIBRATED}\n"));
    out.push_str(&format!("schema: {SCHEMA_BLACKBOX_PROFILE_V1}\n"));
    out.push_str("authority: uncalibrated-instrument (no-llm-authority twin)\n");
    out.push_str(AUTHORITY_NOTE);
    out.push('\n');

    let Some(loaded) = snapshot.as_loaded() else {
        out.push_str("pooled_campaigns: 0\n");
        out.push_str("（BLACKBOX: Vault にプロファイルなし — 全レーン未測定。数値を推測で埋めるな）\n");
        return truncate_block(&out);
    };

    out.push_str(&format!(
        "pooled_campaigns: {}\n",
        loaded.meta.pooled_campaigns
    ));
    out.push_str(&format!(
        "pool_digest: {}\n",
        digest_hex(&loaded.meta.pool_digest)
    ));
    out.push_str(
        "1 キャンペーン由来でも「確定した性格」と読むな — pooled_campaigns を必ず添えよ。\n",
    );

    for axis in BiasAxis::ALL {
        let est = loaded.profile.axis(axis);
        out.push_str(&format_lane_line(axis, &est));
    }
    out.push_str(
        "「未測定」レーンは 0 でも平均でも空欄でもない。測定済みレーンには sufficiency を必ず随伴させよ。\n",
    );
    truncate_block(&out)
}

#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
fn format_lane_line(axis: BiasAxis, est: &BiasEstimate) -> String {
    let value = format_value_micro(est.value_micro);
    let suf = format_sufficiency(est.sufficiency_micro);
    format!(
        "- {} ({}): value={} n_obs={} sufficiency={}\n",
        axis_label_ja(axis),
        axis_id(axis),
        value,
        est.n_obs,
        suf
    )
}

#[cfg_attr(not(feature = "pocket-brain"), allow(dead_code))]
fn truncate_block(s: &str) -> String {
    // Soft cap: consult_context uses token budgets under pocket-brain; this
    // module must stay compileable without that feature (db → llm is forbidden
    // as a hard edge anyway). ~4 chars/token × budget ≈ char ceiling.
    let max_chars = BLACKBOX_SECTION_TOKEN_BUDGET.saturating_mul(4);
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect::<String>() + "…"
}

#[must_use]
pub(crate) fn profile_view_from_loaded(loaded: &LoadedProfile) -> BlackboxProfileView {
    let mut lanes = Vec::with_capacity(BiasAxis::ALL.len());
    for axis in BiasAxis::ALL {
        let est = loaded.profile.axis(axis);
        let measured = est.value_micro.is_some();
        lanes.push(BlackboxLaneView {
            lane: axis.lane(),
            axis: axis_id(axis),
            label_ja: axis_label_ja(axis),
            value_micro: est.value_micro,
            n_obs: est.n_obs,
            sufficiency_micro: est.sufficiency_micro,
            measured,
        });
    }
    BlackboxProfileView {
        schema_version: loaded.meta.schema_version.clone(),
        instrument: loaded.meta.instrument.clone(),
        calibration: loaded.meta.calibration.clone(),
        pooled_campaigns: loaded.meta.pooled_campaigns,
        estimated_at: loaded.meta.estimated_at,
        pool_digest_hex: digest_hex(&loaded.meta.pool_digest),
        lanes,
        authority_note: AUTHORITY_NOTE,
    }
}

#[must_use]
pub(crate) fn meta_view(meta: &ProfileMeta) -> BlackboxProfileMetaView {
    BlackboxProfileMetaView {
        pool_digest_hex: digest_hex(&meta.pool_digest),
        schema_version: meta.schema_version.clone(),
        instrument: meta.instrument.clone(),
        calibration: meta.calibration.clone(),
        pooled_campaigns: meta.pooled_campaigns,
        estimated_at: meta.estimated_at,
        updated_at: meta.updated_at,
    }
}

/// Soft-absent UI payload — every lane 「未測定」, never numeric zeros as values.
#[must_use]
pub(crate) fn absent_profile_view() -> BlackboxProfileView {
    let lanes: Vec<BlackboxLaneView> = BiasAxis::ALL
        .iter()
        .copied()
        .map(|axis| BlackboxLaneView {
            lane: axis.lane(),
            axis: axis_id(axis),
            label_ja: axis_label_ja(axis),
            value_micro: None,
            n_obs: 0,
            sufficiency_micro: 0,
            measured: false,
        })
        .collect();
    BlackboxProfileView {
        schema_version: SCHEMA_BLACKBOX_PROFILE_V1.to_string(),
        instrument: INSTRUMENT_ID.to_string(),
        calibration: CALIBRATION_UNCALIBRATED.to_string(),
        pooled_campaigns: 0,
        estimated_at: 0,
        pool_digest_hex: String::new(),
        lanes,
        authority_note: AUTHORITY_NOTE,
    }
}

/// Guard used by unit tests: a formatted block must never present an
/// unmeasured lane as the numeric token `0` without the 未測定 marker.
#[cfg(test)]
pub(crate) fn unmeasured_lane_never_shows_bare_zero(block: &str) -> bool {
    // Every axis line that lacks a measured decimal must contain 未測定.
    for line in block.lines() {
        if !line.starts_with("- ") {
            continue;
        }
        if line.contains("value=未測定") {
            continue;
        }
        // Measured lines look like value=0.123456 — reject value=0 with no decimal
        // that would mean we stringified None as integer zero.
        if line.contains("value=0 ") || line.contains("value=0\n") || line.ends_with("value=0") {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::bias::BlackboxProfile;
    use crate::blackbox_sim::bridge::pool_digest;
    use crate::db::blackbox_repo::{LoadedProfile, ProfileMeta};

    fn sample_loaded(measured: bool) -> LoadedProfile {
        let fp = [0x42; 32];
        let digest = pool_digest(&[fp]);
        let mut profile = BlackboxProfile::empty(digest);
        if measured {
            profile.set_axis(
                BiasAxis::LossAversion,
                BiasEstimate::measured(1_500_000, 4, 800_000).expect("measured"),
            );
        }
        LoadedProfile {
            meta: ProfileMeta {
                pool_digest: digest,
                schema_version: SCHEMA_BLACKBOX_PROFILE_V1.to_string(),
                instrument: INSTRUMENT_ID.to_string(),
                calibration: CALIBRATION_UNCALIBRATED.to_string(),
                pooled_campaigns: 1,
                estimated_at: 10,
                updated_at: 10,
            },
            profile,
            sources: vec![fp],
        }
    }

    #[test]
    fn absent_block_declares_unmeasured_and_authority() {
        let block = format_consult_block(&BlackboxOutletSnapshot::absent());
        assert!(block.contains(CALIBRATION_UNCALIBRATED));
        assert!(block.contains(AUTHORITY_NOTE));
        assert!(block.contains("未測定"));
        assert!(unmeasured_lane_never_shows_bare_zero(&block));
    }

    #[test]
    fn unmeasured_axes_render_as_mismeasured_literal_not_zero() {
        let snap = BlackboxOutletSnapshot::from_loaded(sample_loaded(false));
        let block = format_consult_block(&snap);
        assert!(block.contains("value=未測定"));
        assert!(!block.contains("value=0 "));
        assert!(block.contains("pooled_campaigns: 1"));
        assert!(unmeasured_lane_never_shows_bare_zero(&block));
    }

    #[test]
    fn measured_axis_keeps_sufficiency_and_never_drops_authority() {
        let snap = BlackboxOutletSnapshot::from_loaded(sample_loaded(true));
        let block = format_consult_block(&snap);
        assert!(block.contains("loss_aversion"));
        assert!(block.contains("sufficiency="));
        assert!(block.contains(AUTHORITY_NOTE));
        let view = profile_view_from_loaded(snap.as_loaded().expect("loaded"));
        let loss = view.lanes.iter().find(|l| l.axis == "loss_aversion").unwrap();
        assert_eq!(loss.value_micro, Some(1_500_000));
        assert!(loss.measured);
        let other = view.lanes.iter().find(|l| l.axis == "anchoring").unwrap();
        assert_eq!(other.value_micro, None);
        assert!(!other.measured);
    }

    #[test]
    fn absent_ui_view_marks_every_lane_unmeasured() {
        let view = absent_profile_view();
        assert_eq!(view.pooled_campaigns, 0);
        assert!(view.lanes.iter().all(|l| !l.measured && l.value_micro.is_none()));
        assert_eq!(view.calibration, CALIBRATION_UNCALIBRATED);
    }
}
