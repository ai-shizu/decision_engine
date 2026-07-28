//! Gate G — pass-rate measurement against BLACKBOX arena FE fixed Japanese
//! strings under `apps/desktop/src/components/consult/coliseum/`.
//!
//! Not a pass/fail gate: report totals + Finding kind breakdown.
//! Do not quietly loosen tables when the rate is low — report the number.

use crate::flavor::policy::FlavorPolicy;
use crate::flavor::scan::{self, Finding};
use crate::flavor::verified::VerifiedFlavor;

/// Japanese fixed-template / UI prose extracted from coliseum/ (Gate G).
/// Source paths are comments only — keep in sync when FE templates change.
const COLISEUM_JA_STRINGS: &[&str] = &[
    // ColiseumArena.tsx — GD mock turns
    "まず論点を MECE に分割してください。",
    "需要・供給・規制の三層で切ります。",
    "定量: オーダーは何度ですか。感度を示せ。",
    "10^7 規模。感度は価格弾性に依存。",
    "その前提が崩れたとき、結論はどう変わる？",
    "需要側が半減すればオーダーは一桁下がる。",
    "（お題未設定）",
    "同意。ただし顧客獲得コストを無視した議論は無意味だ。",
    "既存顧客の LTV 改善を先に置く案はどうか。",
    "フレームワークで整理すると、3C→4P が標準手順だ。",
    "その案の定量根拠は？感度を示せ。",
    "リピート率が 5pt 上がれば売上は約 1.3 倍。",
    "GDへ発言…",
    "モデル準備中…",
    // ColiseumDebrief.tsx
    "圧迫下の視野狭窄",
    "面接での一問詰まりは、先月の破局視ログと同型の縮退パターン。",
    "定量回避の再発",
    "数字要求への遅延は、Vault 上の filter 傾向と位相が一致。",
    // ColiseumRoot.tsx
    "近日公開 — BLACKBOX アリーナは上部タブから起動",
    // BlackboxSetupPanel.tsx
    "プロセス内セッションのみ。再起動・タブ離脱で失われます（vault 再開は未結線）。",
    // GdSetupPanel.tsx
    "例: 売上を2倍にする施策について",
    "お題を入力すると初期化できます",
    // AsymmetryProbe.tsx
    "破局視:「また全部ダメになる」 snippet#d-8841",
];

#[test]
fn gate_g_coliseum_ja_pass_rate_measurement() {
    let policy = FlavorPolicy::v1_empty();
    let total = COLISEUM_JA_STRINGS.len();
    let mut accepted = 0usize;
    let mut kind_counts: Vec<(&str, usize)> = Vec::new();

    for sample in COLISEUM_JA_STRINGS {
        let ok = VerifiedFlavor::verify(sample, &policy).is_some();
        if ok {
            accepted += 1;
            eprintln!("GATE_G ACCEPT: {sample:?}");
        } else {
            let findings = scan::scan(sample, &policy).findings;
            eprintln!("GATE_G REJECT: {sample:?} findings={findings:?}");
            for f in findings {
                bump_kind(&mut kind_counts, f.kind_name());
            }
        }
    }

    let rate = if total == 0 {
        0.0
    } else {
        (accepted as f64) * 100.0 / (total as f64)
    };
    eprintln!("GATE_G TOTAL={total} ACCEPTED={accepted} RATE={rate:.1}%");
    eprintln!("GATE_G FINDING_BREAKDOWN:");
    for (k, n) in &kind_counts {
        eprintln!("GATE_G   {k}={n}");
    }

    // Measurement only — no pass-rate threshold (directive §4 Gate G).
    assert!(total > 0, "coliseum JA corpus must be non-empty");
}

fn bump_kind(counts: &mut Vec<(&str, usize)>, kind: &'static str) {
    if let Some((_, n)) = counts.iter_mut().find(|(k, _)| *k == kind) {
        *n += 1;
    } else {
        counts.push((kind, 1));
    }
}

#[test]
fn gate_g_rejects_are_reasoned_not_bool() {
    // Sanity: rejected samples always carry at least one typed Finding.
    let policy = FlavorPolicy::v1_empty();
    for sample in COLISEUM_JA_STRINGS {
        if VerifiedFlavor::verify(sample, &policy).is_none() {
            let s = scan::scan(sample, &policy);
            assert!(
                !s.findings.is_empty(),
                "reject without findings: {sample:?}"
            );
            let _ = s.findings.iter().map(Finding::kind_name).collect::<Vec<_>>();
        }
    }
}
