//! A-1 deletability harness (SPEC_FLAVOR_LAYER.md §13 / F-3 T-6 / T-6b / T-8).
//!
//! Reuses the Decide-time digest **moment** from Phase 6-A /
//! `bridge::replay_verified` and `bridge::tests::{play, scripted_intent,
//! input_from}`: digests are taken immediately before `submit` — the same
//! instant `Session::submit_inner` stamps `DecisionEvent.state_digest`.
//!
//! Arms:
//! - **none** — completion `None` (mechanism runs; nothing Accepted)
//! - **canned** — frozen [`CANNED_COMPLETION`] + take each turn (flavor exists)
//! - **live** — real model loaded + generate attempted (T-8 proposition b)
//!
//! Does **not** modify the sealed calibration suite or `CalibrationCertificate`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

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
    /// A-1-3: model loaded; generation attempted (accepted may be 0).
    Live,
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
    /// A-1-f: generation attempts under a loaded model (Live arm only).
    attempts: u32,
}

fn play_and_collect(
    arm: FlavorArm,
    #[cfg(feature = "flavor-live")] llm: Option<&crate::llm::LlmHandle>,
) -> Result<DumpResult, String> {
    let mut session = Session::start(a1_genesis_request()).map_err(|e| format!("start: {e:?}"))?;
    let mut series: Vec<[u8; 8]> = Vec::with_capacity(CAMPAIGN_TICKS as usize);
    #[cfg(feature = "flavor-live")]
    let mut accepted: u32 = 0;
    #[cfg(feature = "flavor-live")]
    let mut attempts: u32 = 0;
    #[cfg(not(feature = "flavor-live"))]
    let accepted: u32 = 0;
    #[cfg(not(feature = "flavor-live"))]
    let attempts: u32 = 0;

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
            let (acc, att) = exercise_ambient_slot(&session, arm, llm)?;
            accepted = accepted.saturating_add(acc);
            attempts = attempts.saturating_add(att);
        }
        #[cfg(not(feature = "flavor-live"))]
        {
            let _ = arm;
        }
    }
    Ok(DumpResult {
        series,
        accepted,
        attempts,
    })
}

/// Returns `(accepted_take, generation_attempts)`.
#[cfg(feature = "flavor-live")]
fn exercise_ambient_slot(
    session: &Session,
    arm: FlavorArm,
    llm: Option<&crate::llm::LlmHandle>,
) -> Result<(u32, u32), String> {
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
        Admit::DroppedBusy => Ok((0, 0)),
        Admit::Started => {
            let (completion, attempts) = match arm {
                FlavorArm::None => (None, 0u32),
                FlavorArm::Canned => (Some(CANNED_COMPLETION.to_string()), 0u32),
                FlavorArm::Live => {
                    let llm = llm.ok_or_else(|| {
                        "A-1-3 live arm requires a loaded LlmHandle".to_string()
                    })?;
                    let prompt = flavor_gen::render_prompt(&request);
                    match flavor_generate_blocking(llm, prompt) {
                        Ok(text) => (Some(text), 1u32),
                        Err(e) => {
                            // Model was invoked (or refused after load). Count the try.
                            let _ = writeln!(
                                io::stderr(),
                                "flavor-a1-digest: live generate err: {e}"
                            );
                            (None, 1u32)
                        }
                    }
                }
            };
            let outcome = flavor_gen::generate(&request, completion.as_deref());
            let is_accepted = matches!(outcome, FlavorOutcome::Accepted(_));
            slot.finish(outcome);
            let taken = if matches!(arm, FlavorArm::Canned | FlavorArm::Live) && is_accepted {
                match slot.take(&corr) {
                    Some(_) => 1u32,
                    None => 0u32,
                }
            } else {
                0u32
            };
            Ok((taken, attempts))
        }
    }
}

#[cfg(feature = "flavor-live")]
fn flavor_generate_blocking(
    llm: &crate::llm::LlmHandle,
    prompt: String,
) -> Result<String, String> {
    flavor_generate_blocking_seeded(llm, prompt, 0)
}

#[cfg(feature = "flavor-live")]
fn flavor_generate_blocking_seeded(
    llm: &crate::llm::LlmHandle,
    prompt: String,
    seed: u32,
) -> Result<String, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    llm.enqueue_flavor_generate_seeded(
        prompt,
        64,
        seed,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    )?;
    match rx.recv_timeout(Duration::from_secs(180)) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => Err("flavor generate timed out".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("flavor generate disconnected".into()),
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

/// Write Decide-time series. Returns `(line_count, accepted_count, attempts)`.
pub fn dump_to_file(
    out_path: &Path,
    arm: FlavorArm,
    #[cfg(feature = "flavor-live")] llm: Option<&crate::llm::LlmHandle>,
) -> Result<(usize, u32, u32), String> {
    #[cfg(feature = "flavor-live")]
    let DumpResult {
        series,
        accepted,
        attempts,
    } = play_and_collect(arm, llm)?;
    #[cfg(not(feature = "flavor-live"))]
    let DumpResult {
        series,
        accepted,
        attempts,
    } = play_and_collect(arm)?;
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    fs::write(out_path, format_series(&series)).map_err(|e| format!("write: {e}"))?;
    Ok((series.len(), accepted, attempts))
}

#[cfg(feature = "flavor-live")]
fn load_llm(model_path: &Path) -> Result<crate::llm::LlmHandle, String> {
    use crate::llm::params::LoadParams;
    use crate::llm::LlmHandle;
    use crate::monitor::MemoryMonitor;

    let monitor = Arc::new(MemoryMonitor::new());
    let handle = LlmHandle::spawn(monitor)?;
    handle.load(
        model_path.to_path_buf(),
        LoadParams {
            n_gpu_layers: 999,
            use_mmap: true,
        },
    )?;
    if !handle.is_loaded()? {
        return Err("model load reported success but is_loaded=false".into());
    }
    Ok(handle)
}

/// T-8-3 discard-rate measurement (§14.3). Does not author a frozen population.
#[cfg(feature = "flavor-live")]
fn measure_discard(arm_label: &str, include_no_numerals: bool, n: u32, llm: &crate::llm::LlmHandle) {
    use crate::flavor::policy::FlavorPolicy;
    use crate::flavor::request::{
        FlavorLocale, FlavorRequest, FlavorSchema, FlavorSlot, SlotId, SlotValue, TemplateId,
    };
    use crate::flavor::scan::{self, Finding};
    use crate::llm::flavor_gen::{self, FlavorOutcome};

    let request = FlavorRequest {
        schema: FlavorSchema::V1,
        template_id: TemplateId::ArenaEventHeadline,
        slots: vec![FlavorSlot {
            id: SlotId::Mood,
            tag: SlotValue::MoodCalm,
        }],
        locale: FlavorLocale::Ja,
    };
    let prompt = flavor_gen::render_prompt_ex(&request, include_no_numerals);
    let policy = FlavorPolicy::for_template(TemplateId::ArenaEventHeadline);

    let mut accepted = 0u32;
    let mut discarded = 0u32;
    let mut unavailable = 0u32;
    let mut leaked = 0u32;
    let mut inv = 0u32;
    let mut markup = 0u32;
    let mut uni = 0u32;
    let mut han = 0u32;
    let mut lex = 0u32;
    let mut over = 0u32;

    for i in 0..n {
        let raw = match flavor_generate_blocking_seeded(llm, prompt.clone(), i.wrapping_mul(9973) + 1)
        {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(io::stderr(), "measure: trial {i} unavailable: {e}");
                unavailable += 1;
                continue;
            }
        };
        match flavor_gen::decide(&raw, TemplateId::ArenaEventHeadline) {
            FlavorOutcome::Accepted(v) => {
                accepted += 1;
                // Belt: Accepted must be clean under verify; any residual numeral is a leak.
                let findings = scan::scan(v.as_str(), &policy).findings;
                if !findings.is_empty() {
                    leaked += 1;
                }
            }
            FlavorOutcome::Discarded(findings) => {
                discarded += 1;
                for f in &findings {
                    match f {
                        Finding::Invisible { .. } => inv += 1,
                        Finding::Markup { .. } => markup += 1,
                        Finding::UnicodeNumeral { .. } => uni += 1,
                        Finding::HanNumeral { .. } => han += 1,
                        Finding::LexicalQuantity { .. } => lex += 1,
                        Finding::OverBudget { .. } => over += 1,
                    }
                }
            }
            FlavorOutcome::Unavailable => unavailable += 1,
        }
    }

    println!("arm            = {arm_label}");
    println!("N              = {n}");
    println!("accepted       = {accepted}");
    println!("discarded      = {discarded}");
    println!("unavailable    = {unavailable}");
    println!(
        "findings       = Invisible={inv} / Markup={markup} / UnicodeNumeral={uni} / \
         HanNumeral={han} / LexicalQuantity={lex} / OverBudget={over}"
    );
    println!("leaked         = {leaked}");
}

/// CLI entry used by `bin/flavor_a1_digest`.
pub fn run_cli(args: &[String]) -> i32 {
    let mut out: Option<String> = None;
    let mut arm = FlavorArm::None;
    let mut model: Option<PathBuf> = None;
    let mut measure: Option<String> = None;
    let mut n: u32 = 16;
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
                    eprintln!("flavor-a1-digest: --arm needs none|canned|live");
                    return 2;
                }
                arm = match args[i].as_str() {
                    "none" => FlavorArm::None,
                    "canned" => FlavorArm::Canned,
                    "live" => FlavorArm::Live,
                    other => {
                        eprintln!("flavor-a1-digest: unknown arm {other} (none|canned|live)");
                        return 2;
                    }
                };
            }
            "--model" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("flavor-a1-digest: --model needs a path");
                    return 2;
                }
                model = Some(PathBuf::from(&args[i]));
            }
            "--measure-discard" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("flavor-a1-digest: --measure-discard needs A|B");
                    return 2;
                }
                measure = Some(args[i].clone());
            }
            "--n" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("flavor-a1-digest: --n needs a count");
                    return 2;
                }
                n = match args[i].parse() {
                    Ok(v) if v > 0 => v,
                    _ => {
                        eprintln!("flavor-a1-digest: invalid --n");
                        return 2;
                    }
                };
            }
            "--help" | "-h" => {
                eprintln!(
                    "Usage:\n  flavor-a1-digest --out PATH [--arm none|canned|live] [--model GGUF]\n  \
                     flavor-a1-digest --measure-discard A|B --model GGUF [--n N]\n\
                     --arm none    A-1-2  (completion None)\n\
                     --arm canned  A-1-2b (CANNED_COMPLETION + take)\n\
                     --arm live    A-1-3  (real model; attempts counted)"
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

    #[cfg(feature = "flavor-live")]
    if let Some(arm_label) = measure {
        let Some(model_path) = model else {
            eprintln!("flavor-a1-digest: --measure-discard requires --model");
            return 2;
        };
        let include = match arm_label.as_str() {
            "A" | "a" => true,
            "B" | "b" => false,
            other => {
                eprintln!("flavor-a1-digest: measure arm must be A or B, got {other}");
                return 2;
            }
        };
        let llm = match load_llm(&model_path) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("flavor-a1-digest: model load failed: {e}");
                return 1;
            }
        };
        let meta = fs::metadata(&model_path).ok();
        let size = meta.map(|m| m.len()).unwrap_or(0);
        println!("model          = {}", model_path.display());
        println!("model_size_bytes = {size}");
        measure_discard(&arm_label.to_uppercase(), include, n, &llm);
        return 0;
    }

    #[cfg(not(feature = "flavor-live"))]
    if measure.is_some() {
        eprintln!("flavor-a1-digest: --measure-discard requires flavor-live");
        return 2;
    }

    let Some(out) = out else {
        eprintln!("flavor-a1-digest: --out PATH is required");
        return 2;
    };

    #[cfg(feature = "flavor-live")]
    let llm_owned = if matches!(arm, FlavorArm::Live) {
        let Some(model_path) = model else {
            eprintln!("flavor-a1-digest: --arm live requires --model PATH");
            return 2;
        };
        match load_llm(&model_path) {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("flavor-a1-digest: A-1-3 model load failed: {e}");
                return 4;
            }
        }
    } else {
        None
    };
    #[cfg(feature = "flavor-live")]
    let llm_ref = llm_owned.as_ref();

    #[cfg(feature = "flavor-live")]
    let dump = dump_to_file(Path::new(&out), arm, llm_ref);
    #[cfg(not(feature = "flavor-live"))]
    let dump = dump_to_file(Path::new(&out), arm);

    match dump {
        Ok((n_lines, accepted, attempts)) => {
            let _ = writeln!(
                io::stderr(),
                "flavor-a1-digest: wrote {n_lines} decide-time digests to {out} \
                 accepted={accepted} attempts={attempts}"
            );
            if n_lines == 0 {
                eprintln!("flavor-a1-digest: refusing empty series (A-1-b)");
                return 3;
            }
            if matches!(arm, FlavorArm::Live) && attempts == 0 {
                eprintln!("flavor-a1-digest: A-1-f RED: attempts=0 under live arm");
                return 5;
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
