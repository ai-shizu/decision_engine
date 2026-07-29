//! A-1 deletability harness (SPEC_FLAVOR_LAYER.md §13 / F-3 T-6 / T-6b).
//!
//! Reuses the Decide-time digest **moment** from Phase 6-A /
//! `bridge::replay_verified` and `bridge::tests::{play, scripted_intent,
//! input_from}`: digests are taken immediately before `submit` — the same
//! instant `Session::submit_inner` stamps `DecisionEvent.state_digest`.
//!
//! Arms (T-6b):
//! - **none** — completion `None` (mechanism runs; nothing Accepted)
//! - **canned** — frozen [`CANNED_COMPLETION`] + take each turn (flavor exists)
//!
//! Does **not** modify the sealed calibration suite or `CalibrationCertificate`.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::blackbox_sim::director::{Session, CAMPAIGN_TICKS};
use crate::blackbox_sim::genesis::{Difficulty, GenesisRequest};
use crate::blackbox_sim::telemetry::ActionIntent;

/// Frozen genesis for A-1-d (identical across arms).
pub const A1_SCENARIO_ID: u32 = 7;
pub const A1_CAMPAIGN_INDEX: u32 = 0;
pub const A1_CREATED_DATE: &str = "2026-07-29";

/// Canned Headline prose for A-1-2b (12×「あ」— within 48-char budget, no numerals).
/// Fixed in-source so CI cannot silently skip the "flavor exists" arm.
pub const CANNED_COMPLETION: &str = "ああああああああああああ";

/// Which ambient completion path to exercise under `flavor-live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlavorArm {
    /// A-1-2: model-absent / empty candidate.
    None,
    /// A-1-2b: real Accepted flavor + take each turn.
    Canned,
}

#[must_use]
pub fn a1_genesis_request() -> GenesisRequest {
    GenesisRequest {
        scenario_id: A1_SCENARIO_ID,
        difficulty: Difficulty::Standard,
        campaign_index: A1_CAMPAIGN_INDEX,
        created_date: A1_CREATED_DATE.to_string(),
    }
}

/// Same scripted intents as `bridge::tests::scripted_intent` (read-only reuse).
fn scripted_intent(turn: u32) -> ActionIntent {
    match turn % 8 {
        0 => ActionIntent::OrderInventory {
            sku: 0,
            units: 400,
        },
        1 => ActionIntent::SetPrice {
            sku: 0,
            tick_price: 7_000 + i64::from(turn) * 11,
        },
        2 => ActionIntent::Borrow {
            facility: 0,
            amount_minor: 120_000,
        },
        3 => ActionIntent::OrderInventory {
            sku: 1,
            units: 250,
        },
        4 => ActionIntent::Invest {
            project_id: 0,
            amount_minor: 60_000,
        },
        5 => ActionIntent::ForecastInterval {
            lo_minor: 1_000,
            hi_minor: 5_000,
        },
        6 => ActionIntent::OrderInventory {
            sku: 2,
            units: 120,
        },
        _ => ActionIntent::Abstain,
    }
}

struct DumpResult {
    series: Vec<[u8; 8]>,
    accepted: u32,
}

fn play_and_collect(arm: FlavorArm) -> Result<DumpResult, String> {
    let mut session = Session::start(a1_genesis_request()).map_err(|e| format!("start: {e:?}"))?;
    let mut series: Vec<[u8; 8]> = Vec::with_capacity(CAMPAIGN_TICKS as usize);
    #[cfg(feature = "flavor-live")]
    let mut accepted: u32 = 0;
    #[cfg(not(feature = "flavor-live"))]
    let accepted: u32 = 0;

    for turn in 0..CAMPAIGN_TICKS {
        session
            .observe()
            .map_err(|e| format!("observe@{turn}: {e:?}"))?;
        let digest = session
            .state_digest()
            .map_err(|e| format!("digest@{turn}: {e:?}"))?
            .short();
        series.push(digest);
        if session.submit(scripted_intent(turn), None).is_err() {
            session
                .submit(ActionIntent::Abstain, None)
                .map_err(|e| format!("abstain@{turn}: {e:?}"))?;
        }
        session
            .execute()
            .map_err(|e| format!("execute@{turn}: {e:?}"))?;
        session
            .settle()
            .map_err(|e| format!("settle@{turn}: {e:?}"))?;
        session
            .report()
            .map_err(|e| format!("report@{turn}: {e:?}"))?;

        #[cfg(feature = "flavor-live")]
        {
            accepted = accepted.saturating_add(exercise_ambient_slot(&session, arm)?);
        }
        #[cfg(not(feature = "flavor-live"))]
        {
            let _ = arm;
        }
    }
    Ok(DumpResult { series, accepted })
}

/// Returns 1 if this turn produced a delivered Accepted flavor that was taken.
#[cfg(feature = "flavor-live")]
fn exercise_ambient_slot(session: &Session, arm: FlavorArm) -> Result<u32, String> {
    use super::flavor_slot::{correlation_from_genesis, Admit, FlavorAmbientSlot};
    use crate::flavor::request::{
        FlavorLocale, FlavorRequest, FlavorSchema, FlavorSlot, SlotId, SlotValue, TemplateId,
    };
    use crate::llm::flavor_gen::{self, FlavorOutcome};

    let mut slot = FlavorAmbientSlot::new();
    let corr = correlation_from_genesis(
        session.genesis(),
        session.turns_completed(),
        TemplateId::ArenaEventHeadline,
    );
    let request = FlavorRequest {
        schema: FlavorSchema::V1,
        template_id: TemplateId::ArenaEventHeadline,
        slots: vec![FlavorSlot {
            id: SlotId::Mood,
            tag: SlotValue::MoodCalm,
        }],
        locale: FlavorLocale::Ja,
    };
    match slot.begin_request(corr) {
        Admit::DroppedBusy => Ok(0),
        Admit::Started => {
            let completion = match arm {
                FlavorArm::None => None,
                FlavorArm::Canned => Some(CANNED_COMPLETION),
            };
            let outcome = flavor_gen::generate(&request, completion);
            let is_accepted = matches!(outcome, FlavorOutcome::Accepted(_));
            slot.finish(outcome);
            if matches!(arm, FlavorArm::Canned) && is_accepted {
                match slot.take(&corr) {
                    Some(_) => Ok(1),
                    None => Ok(0),
                }
            } else {
                Ok(0)
            }
        }
    }
}

fn format_series(series: &[[u8; 8]]) -> String {
    let mut out = String::new();
    for d in series {
        for b in d {
            out.push_str(&format!("{b:02x}"));
        }
        out.push('\n');
    }
    out
}

/// Write Decide-time series. Returns `(line_count, accepted_count)`.
pub fn dump_to_file(out_path: &Path, arm: FlavorArm) -> Result<(usize, u32), String> {
    let DumpResult { series, accepted } = play_and_collect(arm)?;
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    fs::write(out_path, format_series(&series)).map_err(|e| format!("write: {e}"))?;
    Ok((series.len(), accepted))
}

/// CLI entry used by `bin/flavor_a1_digest`.
pub fn run_cli(args: &[String]) -> i32 {
    let mut out: Option<String> = None;
    let mut arm = FlavorArm::None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("flavor-a1-digest: --out needs a path");
                    return 2;
                }
                out = Some(args[i].clone());
            }
            "--arm" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("flavor-a1-digest: --arm needs none|canned");
                    return 2;
                }
                arm = match args[i].as_str() {
                    "none" => FlavorArm::None,
                    "canned" => FlavorArm::Canned,
                    other => {
                        eprintln!("flavor-a1-digest: unknown arm {other} (none|canned)");
                        return 2;
                    }
                };
            }
            "--help" | "-h" => {
                eprintln!(
                    "Usage: flavor-a1-digest --out PATH [--arm none|canned]\n\
                     --arm none    A-1-2  (completion None)\n\
                     --arm canned  A-1-2b (CANNED_COMPLETION + take)"
                );
                return 0;
            }
            other => {
                eprintln!("flavor-a1-digest: unknown arg {other}");
                return 2;
            }
        }
        i += 1;
    }
    let Some(out) = out else {
        eprintln!("flavor-a1-digest: --out PATH is required");
        return 2;
    };
    match dump_to_file(Path::new(&out), arm) {
        Ok((n, accepted)) => {
            let _ = writeln!(
                io::stderr(),
                "flavor-a1-digest: wrote {n} decide-time digests to {out} accepted={accepted}"
            );
            if n == 0 {
                eprintln!("flavor-a1-digest: refusing empty series (A-1-b)");
                return 3;
            }
            0
        }
        Err(e) => {
            eprintln!("flavor-a1-digest: {e}");
            1
        }
    }
}

#[cfg(all(test, feature = "flavor-live"))]
mod tests {
    use super::CANNED_COMPLETION;
    use crate::flavor::request::TemplateId;
    use crate::llm::flavor_gen::{decide, FlavorOutcome};

    #[test]
    fn canned_completion_is_accepted_for_headline() {
        match decide(CANNED_COMPLETION, TemplateId::ArenaEventHeadline) {
            FlavorOutcome::Accepted(v) => {
                assert_eq!(v.as_str(), CANNED_COMPLETION);
            }
            other => panic!("CANNED_COMPLETION must be Accepted, got {other:?}"),
        }
    }
}
