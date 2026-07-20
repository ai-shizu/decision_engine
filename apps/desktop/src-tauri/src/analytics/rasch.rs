//! Dynamic Ordinal Rasch filter (`dynamic_ordinal_rasch.v1`) — bit-faithful PCM.
//!
//! Discrimination anchored at 1.0. Artifact constants match
//! `src/python/core/artifacts/dynamic_ordinal_rasch.v1.json`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RASCH_SCHEMA: &str = "dynamic_ordinal_rasch.v1";
pub const GRID_LEN: usize = 17;
pub const STAY: f64 = 0.75;
pub const ADJACENT: f64 = 0.125;
pub const EIG_FACTOR: f64 = 1_000_000.0;

pub const ABILITY_GRID: [f64; GRID_LEN] = [
    -4.0, -3.5, -3.0, -2.5, -2.0, -1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0,
];

#[derive(Debug, Clone, Copy)]
pub struct RaschItem {
    pub item_id: &'static str,
    pub thresholds: [f64; 4],
}

/// Sorted by item_id (lexicographic) — same order as the Python artifact.
pub const RASCH_ITEMS: &[RaschItem] = &[
    RaschItem {
        item_id: "pq-decision_threshold-CONTEXT-01",
        thresholds: [-1.4, -0.4, 0.6, 1.6],
    },
    RaschItem {
        item_id: "pq-decision_threshold-EMOTION-01",
        thresholds: [-1.3, -0.3, 0.7, 1.7],
    },
    RaschItem {
        item_id: "pq-decision_threshold-FACT-01",
        thresholds: [-1.5, -0.5, 0.5, 1.5],
    },
    RaschItem {
        item_id: "pq-decision_threshold-MEANING-01",
        thresholds: [-1.2, -0.2, 0.8, 1.8],
    },
    RaschItem {
        item_id: "pq-friction_energy_ledger-CONTEXT-01",
        thresholds: [0.2, 1.2, 2.2, 3.2],
    },
    RaschItem {
        item_id: "pq-friction_energy_ledger-EMOTION-01",
        thresholds: [0.3, 1.3, 2.3, 3.3],
    },
    RaschItem {
        item_id: "pq-friction_energy_ledger-FACT-01",
        thresholds: [0.1, 1.1, 2.1, 3.1],
    },
    RaschItem {
        item_id: "pq-friction_energy_ledger-MEANING-01",
        thresholds: [0.4, 1.4, 2.4, 3.4],
    },
    RaschItem {
        item_id: "pq-locus_of_control-CONTEXT-01",
        thresholds: [-0.6, 0.4, 1.4, 2.4],
    },
    RaschItem {
        item_id: "pq-locus_of_control-EMOTION-01",
        thresholds: [-0.5, 0.5, 1.5, 2.5],
    },
    RaschItem {
        item_id: "pq-locus_of_control-FACT-01",
        thresholds: [-0.7, 0.3, 1.3, 2.3],
    },
    RaschItem {
        item_id: "pq-locus_of_control-MEANING-01",
        thresholds: [-0.4, 0.6, 1.6, 2.6],
    },
    RaschItem {
        item_id: "pq-reward_bias-CONTEXT-01",
        thresholds: [-1.0, 0.0, 1.0, 2.0],
    },
    RaschItem {
        item_id: "pq-reward_bias-EMOTION-01",
        thresholds: [-0.9, 0.1, 1.1, 2.1],
    },
    RaschItem {
        item_id: "pq-reward_bias-FACT-01",
        thresholds: [-1.1, -0.1, 0.9, 1.9],
    },
    RaschItem {
        item_id: "pq-reward_bias-MEANING-01",
        thresholds: [-0.8, 0.2, 1.2, 2.2],
    },
    RaschItem {
        item_id: "pq-unlearning_rate-CONTEXT-01",
        thresholds: [-0.2, 0.8, 1.8, 2.8],
    },
    RaschItem {
        item_id: "pq-unlearning_rate-EMOTION-01",
        thresholds: [-0.1, 0.9, 1.9, 2.9],
    },
    RaschItem {
        item_id: "pq-unlearning_rate-FACT-01",
        thresholds: [-0.3, 0.7, 1.7, 2.7],
    },
    RaschItem {
        item_id: "pq-unlearning_rate-MEANING-01",
        thresholds: [0.0, 1.0, 2.0, 3.0],
    },
];

pub fn artifact_fingerprint() -> String {
    let mut hasher = Sha256::new();
    hasher.update(RASCH_SCHEMA.as_bytes());
    for item in RASCH_ITEMS {
        hasher.update(item.item_id.as_bytes());
        for t in item.thresholds {
            hasher.update(t.to_bits().to_le_bytes());
        }
    }
    hex::encode(hasher.finalize())
}

pub fn initial_posterior() -> [f64; GRID_LEN] {
    [1.0 / GRID_LEN as f64; GRID_LEN]
}

fn transition(i: usize, j: usize) -> f64 {
    if i == j {
        if i == 0 || i + 1 == GRID_LEN {
            return STAY + ADJACENT;
        }
        return STAY;
    }
    if i > 0 && j + 1 == i {
        return ADJACENT;
    }
    if i + 1 < GRID_LEN && j == i + 1 {
        return ADJACENT;
    }
    0.0
}

fn logsumexp(xs: &[f64]) -> f64 {
    let mut max = f64::NEG_INFINITY;
    for &x in xs {
        if x > max {
            max = x;
        }
    }
    if !max.is_finite() {
        return max;
    }
    let mut sum = 0.0;
    for &x in xs {
        sum += (x - max).exp();
    }
    max + sum.ln()
}

pub fn response_probabilities(theta: f64, thresholds: &[f64; 4]) -> [f64; 5] {
    let mut logits = [0.0_f64; 5];
    let mut cum = 0.0;
    logits[0] = 0.0;
    for (k, &delta) in thresholds.iter().enumerate() {
        cum += delta;
        let cat = (k + 1) as f64;
        logits[k + 1] = cat * theta - cum;
    }
    let lse = logsumexp(&logits);
    let mut out = [0.0_f64; 5];
    for i in 0..5 {
        out[i] = (logits[i] - lse).exp();
    }
    out
}

fn find_item(item_id: &str) -> Option<&'static RaschItem> {
    RASCH_ITEMS.iter().find(|i| i.item_id == item_id)
}

pub fn evaluate_rasch_scale(
    posterior: &[f64; GRID_LEN],
    item_id: &str,
    response: u8,
) -> Result<[f64; GRID_LEN], String> {
    if response > 4 {
        return Err("invalid response".into());
    }
    let item = find_item(item_id).ok_or_else(|| "unknown item_id".to_string())?;

    let mut log_belief = [0.0_f64; GRID_LEN];
    for i in 0..GRID_LEN {
        log_belief[i] = if posterior[i] > 0.0 {
            posterior[i].ln()
        } else {
            f64::NEG_INFINITY
        };
    }

    let mut log_pred = [f64::NEG_INFINITY; GRID_LEN];
    for t in 0..GRID_LEN {
        let mut terms = Vec::with_capacity(3);
        for s in 0..GRID_LEN {
            let tij = transition(s, t);
            if tij > 0.0 {
                terms.push(log_belief[s] + tij.ln());
            }
        }
        if !terms.is_empty() {
            log_pred[t] = logsumexp(&terms);
        }
    }

    let mut log_joint = [0.0_f64; GRID_LEN];
    for t in 0..GRID_LEN {
        let probs = response_probabilities(ABILITY_GRID[t], &item.thresholds);
        let p = probs[response as usize].max(0.0);
        log_joint[t] = log_pred[t]
            + if p > 0.0 {
                p.ln()
            } else {
                f64::NEG_INFINITY
            };
    }
    let lse = logsumexp(&log_joint);
    let mut out = [0.0_f64; GRID_LEN];
    for t in 0..GRID_LEN {
        out[t] = (log_joint[t] - lse).exp();
    }
    Ok(out)
}

fn entropy(ps: &[f64]) -> f64 {
    let mut h = 0.0;
    for &p in ps {
        if p > 0.0 {
            h -= p * p.ln();
        }
    }
    h
}

pub fn expected_information_gain(posterior: &[f64; GRID_LEN], item: &RaschItem) -> f64 {
    let mut marginal = [0.0_f64; 5];
    let mut expected_h = 0.0;
    for i in 0..GRID_LEN {
        let probs = response_probabilities(ABILITY_GRID[i], &item.thresholds);
        for c in 0..5 {
            marginal[c] += posterior[i] * probs[c];
        }
        expected_h += posterior[i] * entropy(&probs);
    }
    let mut eig = entropy(&marginal) - expected_h;
    if eig < 0.0 && eig > -1e-15 {
        eig = 0.0;
    }
    eig
}

fn quantize_eig(eig: f64) -> i64 {
    (eig * EIG_FACTOR + 0.5).floor() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ItemSelection {
    pub item_id: String,
    pub eig: f64,
    pub quantized_eig: i64,
}

pub fn rasch_select_next(
    posterior: &[f64; GRID_LEN],
    excluded: &std::collections::BTreeSet<String>,
) -> Option<ItemSelection> {
    let mut best: Option<ItemSelection> = None;
    for item in RASCH_ITEMS {
        if excluded.contains(item.item_id) {
            continue;
        }
        let eig = expected_information_gain(posterior, item);
        let q = quantize_eig(eig);
        let cand = ItemSelection {
            item_id: item.item_id.to_string(),
            eig,
            quantized_eig: q,
        };
        let take = match &best {
            None => true,
            Some(b) => q > b.quantized_eig || (q == b.quantized_eig && cand.item_id < b.item_id),
        };
        if take {
            best = Some(cand);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prior_sums_to_one() {
        let p = initial_posterior();
        let s: f64 = p.iter().sum();
        assert!((s - 1.0).abs() < 1e-12);
    }

    #[test]
    fn update_preserves_mass() {
        let p0 = initial_posterior();
        let p1 = evaluate_rasch_scale(&p0, "pq-decision_threshold-FACT-01", 4).unwrap();
        let s: f64 = p1.iter().sum();
        assert!((s - 1.0).abs() < 1e-9);
    }
}
