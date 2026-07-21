//! Adaptive mentor intensity via Zone of Proximal Development (ZPD).
//!
//! # Academic grounding (must not be diluted)
//!
//! 1. **Zone of Proximal Development (ZPD)** — Vygotsky (1978): scaffolding
//!    intensity must track the learner's current level; depleted cognitive
//!    resource ⇒ maximal scaffold (one concrete next action, low generative
//!    temperature to avoid hallucination under load).
//! 2. **Yerkes–Dodson law** — Yerkes & Dodson (1908): performance vs arousal is
//!    an inverted U; mid-range cognitive resource ⇒ neutral, analytic mentor
//!    load (two actions, moderate temperature).
//! 3. **Desirable Difficulties** — Bjork (1994): when resources are ample and
//!    lapse risk is low, introduce productive friction (Devil's Advocate, three
//!    actions, slightly higher temperature) to deepen encoding.
//!
//! Mapping is **fully deterministic** (F-14): no RNG, no external APIs. Inputs
//! are Twin `R(t)` and instantaneous `p_lapse` from the latest vault twin run
//! (or soft Neutral defaults when Twin is unavailable).

use serde::Serialize;

use crate::analytics::digital_twin::TwinScenarioResult;
use crate::db::{VaultErrorCode, VaultHandle};

/// Cognitive-resource floor below which scaffolding dominates (Vygotsky ZPD).
pub const R_DEPLETED: f64 = 0.40;
/// Resource ceiling for desirable-difficulty mode (Bjork).
pub const R_HIGH: f64 = 0.70;
/// Lapse probability that forces Depleted regardless of R (safety override).
pub const P_LAPSE_HIGH: f64 = 0.55;
/// Lapse probability required (with high R) to unlock High Resource mode.
pub const P_LAPSE_LOW: f64 = 0.25;

pub const TEMP_DEPLETED: f32 = 0.3;
pub const TEMP_NEUTRAL: f32 = 0.5;
pub const TEMP_HIGH: f32 = 0.7;

/// Three-rung adaptive mentor ladder (Vygotsky / Yerkes–Dodson / Bjork).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MentorZpdLevel {
    /// Low R or high p_lapse — full Scaffolding (Vygotsky 1978).
    Depleted,
    /// Mid arousal — analytic mentor (Yerkes–Dodson 1908).
    Neutral,
    /// High R and low p_lapse — Desirable Difficulties / Devil's Advocate (Bjork 1994).
    HighResource,
}

impl MentorZpdLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Depleted => "depleted",
            Self::Neutral => "neutral",
            Self::HighResource => "high_resource",
        }
    }

    /// Consult-mentor generative prior only. **Do not** use for 鬼モード difficulty —
    /// oni intensity is `InterviewerTactic`; coliseum temp is `COLISEUM_GENERATION_TEMP`.
    pub fn temperature(self) -> f32 {
        match self {
            Self::Depleted => TEMP_DEPLETED,
            Self::Neutral => TEMP_NEUTRAL,
            Self::HighResource => TEMP_HIGH,
        }
    }

    pub fn action_count(self) -> u8 {
        match self {
            Self::Depleted => 1,
            Self::Neutral => 2,
            Self::HighResource => 3,
        }
    }

    /// System preamble substituted for the former fixed MENTOR_PREAMBLE.
    pub fn preamble(self) -> &'static str {
        match self {
            Self::Depleted => PREAMBLE_DEPLETED,
            Self::Neutral => PREAMBLE_NEUTRAL,
            Self::HighResource => PREAMBLE_HIGH,
        }
    }
}

/// Snapshot used for the deterministic ladder decision.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MentorZpdSignal {
    pub r_now: f64,
    pub p_lapse: f64,
    pub level: MentorZpdLevel,
    pub temperature: f32,
    pub action_count: u8,
    /// True when Twin vault state was available; false ⇒ Neutral soft-default.
    pub twin_available: bool,
}

impl MentorZpdSignal {
    pub fn neutral_default() -> Self {
        let level = MentorZpdLevel::Neutral;
        Self {
            r_now: 0.55,
            p_lapse: 0.35,
            level,
            temperature: level.temperature(),
            action_count: level.action_count(),
            twin_available: false,
        }
    }
}

/// Map (R, p_lapse) → ZPD level. Pure function — unit-test surface.
///
/// Precedence: safety (high lapse / low R) wins over desirable difficulty.
pub fn map_mentor_zpd_level(r_now: f64, p_lapse: f64) -> MentorZpdLevel {
    let r = r_now.clamp(0.0, 1.0);
    let p = p_lapse.clamp(0.0, 1.0);
    if r < R_DEPLETED || p > P_LAPSE_HIGH {
        MentorZpdLevel::Depleted
    } else if r >= R_HIGH && p <= P_LAPSE_LOW {
        MentorZpdLevel::HighResource
    } else {
        MentorZpdLevel::Neutral
    }
}

pub fn signal_from_r_p(r_now: f64, p_lapse: f64, twin_available: bool) -> MentorZpdSignal {
    let level = map_mentor_zpd_level(r_now, p_lapse);
    MentorZpdSignal {
        r_now,
        p_lapse,
        level,
        temperature: level.temperature(),
        action_count: level.action_count(),
        twin_available,
    }
}

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

/// Load latest Twin scenario from vault and derive the ZPD signal.
/// Missing / unparsable twin ⇒ Neutral soft-default (never hard-fails consult).
pub fn load_mentor_zpd_signal(vault: &VaultHandle) -> Result<MentorZpdSignal, String> {
    let payload = vault.twin_run_latest_payload().map_err(map_vault)?;
    let Some(raw) = payload else {
        return Ok(MentorZpdSignal::neutral_default());
    };
    let Ok(twin) = serde_json::from_str::<TwinScenarioResult>(&raw) else {
        return Ok(MentorZpdSignal::neutral_default());
    };
    let r_now = twin.state.r_now;
    let p_lapse = twin
        .forecast
        .p_lapse
        .first()
        .copied()
        .unwrap_or_else(|| instantaneous_p_lapse(r_now, twin.params.theta_r, twin.params.kappa));
    Ok(signal_from_r_p(r_now, p_lapse, true))
}

fn instantaneous_p_lapse(r: f64, theta_r: Option<f64>, kappa: f64) -> f64 {
    let Some(th) = theta_r else {
        return 0.35;
    };
    let z = (kappa * (th - r)).clamp(-35.0, 35.0);
    1.0 / (1.0 + (-z).exp())
}

// --- Preambles (theory-tagged; keep action-count contract explicit) -----------

const PREAMBLE_DEPLETED: &str = "\
【適応レベル: Depleted / Scaffolding — Vygotsky ZPD (1978)】\
あなたは司令官の認知資源が枯渇している局面で足場をかけるメンターである。\
共感を先に置き、認知負荷を最小化する。長文の多肢選択や抽象論を避けよ。\
定量根拠（ギャップ・Tensor・Oracle）があれば1点だけ引用し、\
行動可能な次手は**必ず1つだけ**提示せよ。温度を抑え、断定的で短い文で書け。\
データ不足なら推測で埋めず、今日できる最小観測を1つ促せ。";

const PREAMBLE_NEUTRAL: &str = "\
【適応レベル: Neutral / Optimal arousal — Yerkes-Dodson (1908)】\
あなたは司令官の意思決定を支える冷徹なメンターである。同意・共感だけで終わらせるな。\
下記の「主観×客観ギャップ」「Tensorプロファイル」「Oracle予測」および参考情報に定量根拠がある場合はそれを優先し、\
ユーザーの自己申告と矛盾する事実があれば「本当にそうか？」と突き、過去メモとの食い違いを明示せよ。\
一般論でごまかすな。助言の自己検証を行い、行動可能な次手を**2つ**に絞れ。\
データが不足と明示されている場合は推測で埋めず、観測継続を促せ。";

const PREAMBLE_HIGH: &str = "\
【適応レベル: High Resource / Desirable Difficulties — Bjork (1994)】\
あなたは司令官の資源が十分ある局面で「望ましい困難」を設計する Devil's Advocate である。\
安易な同意を拒否し、ユーザーの前提・戦略・自己物語に意図的に異議を唱えよ。\
定量根拠を武器に、より高次元の再フレームを要求する。\
行動可能な次手は**3つ**示し、うち少なくとも1つはユーザーが避けたいが学習効果が高い選択肢にせよ。\
挑発は人格攻撃ではなく認知の拡張のためである。データ不足ならその限界を明示した上で仮説を対置せよ。";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depleted_on_low_r_or_high_lapse() {
        assert_eq!(map_mentor_zpd_level(0.20, 0.10), MentorZpdLevel::Depleted);
        assert_eq!(map_mentor_zpd_level(0.80, 0.70), MentorZpdLevel::Depleted);
    }

    #[test]
    fn high_resource_requires_both_gates() {
        assert_eq!(map_mentor_zpd_level(0.85, 0.10), MentorZpdLevel::HighResource);
        assert_eq!(map_mentor_zpd_level(0.85, 0.40), MentorZpdLevel::Neutral);
        assert_eq!(map_mentor_zpd_level(0.50, 0.10), MentorZpdLevel::Neutral);
    }

    #[test]
    fn temperatures_and_actions_match_ladder() {
        assert!((MentorZpdLevel::Depleted.temperature() - 0.3).abs() < f32::EPSILON);
        assert!((MentorZpdLevel::Neutral.temperature() - 0.5).abs() < f32::EPSILON);
        assert!((MentorZpdLevel::HighResource.temperature() - 0.7).abs() < f32::EPSILON);
        assert_eq!(MentorZpdLevel::Depleted.action_count(), 1);
        assert_eq!(MentorZpdLevel::Neutral.action_count(), 2);
        assert_eq!(MentorZpdLevel::HighResource.action_count(), 3);
    }

    #[test]
    fn oni_threshold_aligns_with_r_depleted() {
        use crate::coliseum::mentor_zpd::{
            evaluate_oni_mode_eligibility, r_t_from_unit_interval, ONI_R_T_THRESHOLD,
        };
        assert_eq!(ONI_R_T_THRESHOLD, ((R_DEPLETED * 100.0).round() as u8));
        assert!(evaluate_oni_mode_eligibility(ONI_R_T_THRESHOLD).is_err());
        assert!(evaluate_oni_mode_eligibility(ONI_R_T_THRESHOLD + 1).is_ok());
        assert_eq!(r_t_from_unit_interval(R_DEPLETED), ONI_R_T_THRESHOLD);
    }

    #[test]
    fn preambles_mention_theory_and_action_count() {
        assert!(MentorZpdLevel::Depleted.preamble().contains("Vygotsky"));
        assert!(MentorZpdLevel::Neutral.preamble().contains("Yerkes"));
        assert!(MentorZpdLevel::HighResource.preamble().contains("Bjork"));
        assert!(MentorZpdLevel::Depleted.preamble().contains("1つ"));
        assert!(MentorZpdLevel::Neutral.preamble().contains("2つ"));
        assert!(MentorZpdLevel::HighResource.preamble().contains("3つ"));
    }
}
