//! Finite interviewer tactics — zero Vault payload by construction (Phase 14 / I-22).

use serde::{Deserialize, Serialize};

/// Abstract pressure moves for IB / quant / Big-Tech style interviews.
///
/// No dates, amounts, merchant names, CBT snippets, or Twin scalars live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterviewerTactic {
    /// Challenge sweeping claims; demand bounded scope and counter-examples.
    ProbeOvergeneralization { intensity: u8 },
    /// Fermi estimates, order-of-magnitude checks, unit / scaling discipline.
    StressTestQuantitative { intensity: u8 },
    /// Force explicit trade-offs; reject one-sided optimization narratives.
    ForceNuancedTradeoff { intensity: u8 },
    /// Algorithms / systems: boundaries, failure modes, edge cases.
    TechnicalEdgeCaseProbe { intensity: u8 },
}

impl InterviewerTactic {
    /// Clamp intensity into the closed band `[1, 5]` (deterministic).
    pub fn clamped(self) -> Self {
        let clamp = |i: u8| i.clamp(1, 5);
        match self {
            Self::ProbeOvergeneralization { intensity } => Self::ProbeOvergeneralization {
                intensity: clamp(intensity),
            },
            Self::StressTestQuantitative { intensity } => Self::StressTestQuantitative {
                intensity: clamp(intensity),
            },
            Self::ForceNuancedTradeoff { intensity } => Self::ForceNuancedTradeoff {
                intensity: clamp(intensity),
            },
            Self::TechnicalEdgeCaseProbe { intensity } => Self::TechnicalEdgeCaseProbe {
                intensity: clamp(intensity),
            },
        }
    }

    fn intensity(self) -> u8 {
        match self {
            Self::ProbeOvergeneralization { intensity }
            | Self::StressTestQuantitative { intensity }
            | Self::ForceNuancedTradeoff { intensity }
            | Self::TechnicalEdgeCaseProbe { intensity } => intensity,
        }
    }

    /// Stable sort key: discriminant ordinal then intensity.
    pub fn sort_key(self) -> (u8, u8) {
        let disc = match self {
            Self::ForceNuancedTradeoff { .. } => 0,
            Self::ProbeOvergeneralization { .. } => 1,
            Self::StressTestQuantitative { .. } => 2,
            Self::TechnicalEdgeCaseProbe { .. } => 3,
        };
        (disc, self.intensity())
    }

    /// LLM system-instruction fragment. Contains no private fossils.
    pub fn to_instruction(&self) -> String {
        let t = self.clamped();
        let intensity = t.intensity();
        match t {
            Self::ProbeOvergeneralization { .. } => format!(
                "【戦術: 過度一般化の検証 / intensity={intensity}】\
候補の断定・「いつも」「絶対」を拾い、適用範囲・反例・測定可能な境界を要求せよ。\
個人の私的記録や財務・心理スコアには言及するな。"
            ),
            Self::StressTestQuantitative { .. } => format!(
                "【戦術: 定量ストレステスト / intensity={intensity}】\
フェルミ推定・オーダーチェック・単位整合・感度（何が1桁動くと結論が崩れるか）を追及せよ。\
具体的な個人支出額・口座・取引明細は出さず、抽象的な定量思考のみを評価せよ。"
            ),
            Self::ForceNuancedTradeoff { .. } => format!(
                "【戦術: トレードオフ強制 / intensity={intensity}】\
単一最適の語りを拒否し、犠牲になる軸・制約・後悔関数を明示させよ。\
候補の私生活・購買・感情ログを引用・推測するな。"
            ),
            Self::TechnicalEdgeCaseProbe { .. } => format!(
                "【戦術: 技術エッジケース追及 / intensity={intensity}】\
アルゴリズム／システム設計の境界条件・失敗モード・負荷・整合性を具体例で詰めよ。\
Vault由来の個人識別子・固有名詞・金額は一切出してはならない。"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_omit_private_markers() {
        let t = InterviewerTactic::StressTestQuantitative { intensity: 3 };
        let s = t.to_instruction();
        assert!(s.contains("定量"));
        assert!(!s.contains("円"));
        assert!(!s.contains("VaultHandle"));
    }

    #[test]
    fn sort_key_is_total_order_friendly() {
        let a = InterviewerTactic::ProbeOvergeneralization { intensity: 2 };
        let b = InterviewerTactic::ProbeOvergeneralization { intensity: 5 };
        assert!(a.sort_key() < b.sort_key());
    }
}
