//! BLACKBOX SIMULATOR Phase 2 — cross-module contracts.
//!
//! These live outside the crate on purpose: they may only touch the module's
//! public surface, which is the same surface the future IPC layer will use.
//! Anything they cannot reach is, by construction, unreachable by the UI.
//!
//! Coverage:
//! - wall W-a / BXS-I-14 — neither the market view nor a persisted snapshot
//!   carries genesis truth,
//! - BXS-I-01 / BXS-W-01 — state is a fold of genesis and decisions; latency
//!   is an observation and never an input,
//! - wall W-d / BXS-I-04 / BXS-I-16 — intents reach the books only through the
//!   ActionCompiler, atomically across ledger, firm and journal,
//! - BXS-I-02 — the trial balance survives a full mixed campaign,
//! - BXS-I-03 — every period close balances its cash-flow statement,
//! - BXS-I-17 — inventory units and carrying value stay reconciled.
#![cfg(feature = "blackbox-sim")]

use pkb_desktop_lib::blackbox_sim::action::{compile, execute_intent, ActionContext, CompileError, SimBooks};
use pkb_desktop_lib::blackbox_sim::director::{
    replay_digest, DirectorError, Session, TurnReport, CAMPAIGN_TICKS,
};
use pkb_desktop_lib::blackbox_sim::firm::FirmError;
use pkb_desktop_lib::blackbox_sim::genesis::{
    build_campaign_genesis, CampaignGenesis, Difficulty, GenesisRequest,
};
use pkb_desktop_lib::blackbox_sim::ledger::AccountCode;
use pkb_desktop_lib::blackbox_sim::market::{MarketKernel, MarketTickView};
use pkb_desktop_lib::blackbox_sim::settle::TICKS_PER_QUARTER;
use pkb_desktop_lib::blackbox_sim::telemetry::ActionIntent;

fn request(campaign_index: u32) -> GenesisRequest {
    GenesisRequest {
        scenario_id: 11,
        difficulty: Difficulty::Standard,
        campaign_index,
        created_date: "2026-07-27".to_string(),
    }
}

fn genesis(campaign_index: u32) -> CampaignGenesis {
    match build_campaign_genesis(request(campaign_index)) {
        Ok(g) => g,
        Err(e) => unreachable!("genesis construction failed: {e:?}"),
    }
}

fn kernel(g: &CampaignGenesis) -> MarketKernel {
    match MarketKernel::new(g) {
        Ok(k) => k,
        Err(e) => unreachable!("kernel construction failed: {e:?}"),
    }
}

fn json<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_string(value) {
        Ok(s) => s,
        Err(e) => unreachable!("serialization failed: {e}"),
    }
}

fn session(campaign_index: u32) -> Session {
    match Session::start(request(campaign_index)) {
        Ok(s) => s,
        Err(e) => unreachable!("session start failed: {e:?}"),
    }
}

/// A solvent book plus the current market tick, assembled only from public
/// API — the same way an IPC command handler would have to do it.
fn seeded(campaign_index: u32, cash: i64) -> (ActionContext, SimBooks) {
    let g = genesis(campaign_index);
    let mut k = kernel(&g);
    let market = match k.step() {
        Ok(v) => v,
        Err(e) => unreachable!("market step failed: {e:?}"),
    };
    let mut books = match SimBooks::new(&g.firm) {
        Ok(b) => b,
        Err(e) => unreachable!("books init failed: {e:?}"),
    };
    match books.seed_capital(cash) {
        Ok(()) => {}
        Err(e) => unreachable!("seeding failed: {e:?}"),
    }
    (
        ActionContext {
            market,
            firm_params: g.firm,
        },
        books,
    )
}

/// An independent fingerprint of the books, built here rather than in the
/// crate so the test cannot agree with a buggy production digest (§16.6).
fn balance_vector(books: &SimBooks) -> Vec<i64> {
    AccountCode::ALL
        .iter()
        .map(|a| books.balances.balance_minor(*a))
        .collect()
}

/// Drive one scripted turn through the public phase API.
///
/// The script names entities blindly and the Director now plants probe
/// projects into the same id space, so some turns are legitimately refused.
/// A refusal is deterministic — the fallback keeps every run reproducible,
/// which is exactly what the replay tests below depend on.
fn drive_turn(s: &mut Session, turn: u32, latency: Option<u32>) -> TurnReport {
    match s.observe() {
        Ok(_) => {}
        Err(e) => unreachable!("turn {turn} observe failed: {e:?}"),
    }
    if s.submit(scripted_intent(turn), latency).is_err() {
        match s.submit(ActionIntent::Abstain, latency) {
            Ok(_) => {}
            Err(e) => unreachable!("turn {turn} fallback failed: {e:?}"),
        }
    }
    match s.execute().and_then(|_| s.settle()).and_then(|_| s.report()) {
        Ok(report) => report,
        Err(e) => unreachable!("turn {turn} failed: {e:?}"),
    }
}

/// A deterministic, varied script covering every intent family.
fn scripted_intent(turn: u32) -> ActionIntent {
    match turn % 10 {
        0 => ActionIntent::OrderInventory { sku: 0, units: 350 },
        1 => ActionIntent::Borrow {
            facility: 0,
            amount_minor: 200_000,
        },
        2 => ActionIntent::SetPrice {
            sku: 1,
            tick_price: 8_000 + i64::from(turn) * 7,
        },
        3 => ActionIntent::OrderInventory { sku: 1, units: 200 },
        4 => ActionIntent::Invest {
            project_id: 0,
            amount_minor: 90_000,
        },
        5 => ActionIntent::OpenHedge {
            instrument: 0,
            notional_minor: 40_000,
        },
        6 => ActionIntent::OrderInventory { sku: 2, units: 150 },
        7 => ActionIntent::ForecastInterval {
            lo_minor: 500,
            hi_minor: 40_000,
        },
        8 => ActionIntent::Repay {
            facility: 0,
            amount_minor: 25_000,
        },
        _ => ActionIntent::Abstain,
    }
}

// ---------------------------------------------------------------------------
// Wall W-a — the hidden oracle stays hidden (BXS-I-14)
// ---------------------------------------------------------------------------

/// Genesis parameter names. If any of these ever appears in an outward
/// artifact, the optimality-gap measurement has degraded into self-report
/// (SPEC §2 W-a). Phase 2 added the firm block, so it is guarded too.
const GENESIS_FIELD_MARKERS: [&str; 23] = [
    "kappaMicro",
    "thetaLogMicro",
    "sigmaCalmMicro",
    "sigmaStressMicro",
    "driftMicro",
    "aMicro",
    "bBp",
    "sigmaBpMicro",
    "phiMicro",
    "seasonAmpMicro",
    "pCalmToStressMicro",
    "pStressToCalmMicro",
    "intensityStressMicro",
    "muMicro",
    "corrCommodityEquityMicro",
    "fingerprint",
    "baseUnitsPerTick",
    "referencePriceMinor",
    "costMultiplierMicro",
    "openingCapitalMinor",
    "depreciationRateBp",
    "taxRateBp",
    "mezzRateBp",
];

/// The complete outward vocabulary of the market view. An exact allowlist, not
/// a denylist: a newly added field fails this test until someone consciously
/// decides it is safe to publish (§16.4 exact-key discipline).
const VIEW_ALLOWED_KEYS: [&str; 7] = [
    "tick",
    "regime",
    "commodityPriceMinor",
    "equityIndexCenti",
    "demandIndexMicro",
    "rateBp",
    "jumpOccurred",
];

#[test]
fn market_view_never_carries_genesis_truth() {
    let g = genesis(0);
    let mut k = kernel(&g);
    // Sanity: the markers really do occur in the genesis artifact, so a pass
    // below means "absent from the view", not "absent from the vocabulary".
    let genesis_json = json(&g);
    for marker in GENESIS_FIELD_MARKERS {
        assert!(
            genesis_json.contains(marker),
            "{marker} missing from genesis — the leak guard is scanning for a \
             name that no longer exists and would pass vacuously"
        );
    }
    for _ in 0..64 {
        let view: MarketTickView = match k.step() {
            Ok(v) => v,
            Err(e) => unreachable!("market step failed: {e:?}"),
        };
        let view_json = json(&view);
        for marker in GENESIS_FIELD_MARKERS {
            assert!(
                !view_json.contains(marker),
                "wall W-a breached: view leaks {marker} in {view_json}"
            );
        }
    }
}

#[test]
fn market_view_key_set_is_an_exact_allowlist() {
    let g = genesis(2);
    let mut k = kernel(&g);
    let view = match k.step() {
        Ok(v) => v,
        Err(e) => unreachable!("market step failed: {e:?}"),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json(&view)) {
        Ok(v) => v,
        Err(e) => unreachable!("view is not an object: {e}"),
    };
    let obj = match parsed.as_object() {
        Some(o) => o,
        None => unreachable!("view must serialize to a JSON object"),
    };
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut allowed: Vec<&str> = VIEW_ALLOWED_KEYS.to_vec();
    allowed.sort_unstable();
    assert_eq!(
        keys, allowed,
        "market view vocabulary changed; publishing a new field needs a wall \
         W-a review, not a test update"
    );
}

/// Phase 2 opened a new leak surface. A snapshot is written by serializing
/// state wholesale, which is exactly the kind of code that acquires an extra
/// field without anyone noticing — and a persisted oracle is worse than a
/// displayed one, because it survives the session.
#[test]
fn snapshots_never_serialize_the_oracle() {
    let mut s = session(4);
    for turn in 0..TICKS_PER_QUARTER {
        let _ = drive_turn(&mut s, turn, None);
    }
    let generation = match s.generations().latest() {
        Ok(g) => g,
        Err(e) => unreachable!("a quarter closed without checkpointing: {e:?}"),
    };
    let payload = String::from_utf8_lossy(&generation.payload);
    for marker in GENESIS_FIELD_MARKERS {
        assert!(
            !payload.contains(marker),
            "wall W-a breached: snapshot persists {marker}"
        );
    }
    // Guard against a vacuous pass: the payload must really hold state.
    assert!(payload.contains("inventoryUnits"), "payload looks empty");
}

// ---------------------------------------------------------------------------
// BXS-I-01 — state is a fold of genesis and the decision log
// ---------------------------------------------------------------------------

#[test]
fn a_campaign_replays_bit_identically_through_the_public_api() {
    let mut live = session(5);
    for turn in 0..30 {
        let _ = drive_turn(&mut live, turn, Some(turn * 13 + 40));
    }
    let live_digest = match live.state_digest() {
        Ok(d) => d,
        Err(e) => unreachable!("digest failed: {e:?}"),
    };
    let replayed = match replay_digest(request(5), live.decisions()) {
        Ok(d) => d,
        Err(e) => unreachable!("replay failed: {e:?}"),
    };
    assert_eq!(
        live_digest, replayed,
        "state(t) must equal fold(Genesis, decisions[0..t])"
    );
}

#[test]
fn different_campaigns_produce_different_worlds() {
    let mut a = kernel(&genesis(0));
    let mut b = kernel(&genesis(1));
    let mut differed = false;
    for _ in 0..52 {
        match (a.step(), b.step()) {
            (Ok(va), Ok(vb)) => {
                if va != vb {
                    differed = true;
                }
            }
            (x, y) => unreachable!("market step failed: {x:?} / {y:?}"),
        }
    }
    assert!(
        differed,
        "distinct campaign indices must yield distinct worlds — identical \
         series would mean the seed is not reaching the kernel"
    );
}

/// BXS-W-01: latency is recorded but must never steer the simulation. Two
/// campaigns differing ONLY in observed latency must end bit-identical.
#[test]
fn latency_is_observed_never_an_input() {
    let run = |latency: &dyn Fn(u32) -> Option<u32>| -> (Vec<i64>, Vec<u64>) {
        let mut s = session(9);
        for turn in 0..24 {
            let _ = drive_turn(&mut s, turn, latency(turn));
        }
        let digest = match s.state_digest() {
            Ok(d) => d,
            Err(e) => unreachable!("digest failed: {e:?}"),
        };
        (
            balance_vector(s.books()),
            digest.0.iter().map(|b| u64::from(*b)).collect(),
        )
    };
    let fast = run(&|_| Some(1));
    let slow = run(&|t| Some(90_000 + t));
    let absent = run(&|_| None);
    assert_eq!(
        fast, slow,
        "latency altered the simulation — BXS-I-01 replay identity is dead"
    );
    assert_eq!(fast, absent);
}

// ---------------------------------------------------------------------------
// Wall W-d — the compiler is the only road from intent to the books
// ---------------------------------------------------------------------------

#[test]
fn a_full_mixed_campaign_preserves_every_accounting_invariant() {
    let mut s = session(3);
    let mut closes = 0;
    for turn in 0..CAMPAIGN_TICKS {
        let report = drive_turn(&mut s, turn, None);
        // Every turn, not just at settle (第六律: the receiver verifies).
        match s.books().balances.verify_zero_sum() {
            Ok(()) => {}
            Err(e) => unreachable!("trial balance broke at turn {turn}: {e:?}"),
        }
        match s.books().balances.verify_accounting_identity() {
            Ok(()) => {}
            Err(e) => unreachable!("accounting identity broke at turn {turn}: {e:?}"),
        }
        if let Some(close) = report.period_close {
            match close.cash_flow.verify() {
                Ok(()) => {}
                Err(e) => unreachable!("BXS-I-03 broke at turn {turn}: {e:?}"),
            }
            let cf = close.cash_flow;
            assert_eq!(
                cf.operating_minor + cf.investing_minor + cf.financing_minor,
                cf.cash_delta_minor
            );
            closes += 1;
        }
    }
    assert_eq!(closes, 4, "a campaign is four quarters");
    assert!(s.books().balances.balance_minor(AccountCode::Cash) > 0);
    assert!(
        s.books()
            .balances
            .balance_minor(AccountCode::PropertyPlantEquipment)
            > 0
    );
    assert!(s.books().balances.balance_minor(AccountCode::SalesRevenue) < 0);
    // BXS-I-17 through the public surface.
    assert_eq!(
        s.books().balances.balance_minor(AccountCode::Inventory),
        s.books().firm.inventory_value_minor()
    );
}

/// Phase 2 lifted the Phase 1 deferral, so these intents must now COMPILE.
/// What remains refused is refused for a business reason, and that refusal
/// must still be free of side effects across all three stores (BXS-I-16).
#[test]
fn every_intent_now_compiles_and_refusals_stay_side_effect_free() {
    let (ctx, mut books) = seeded(6, 50_000_000);
    let supported = [
        ActionIntent::SetPrice {
            sku: 0,
            tick_price: 9_000,
        },
        ActionIntent::OrderInventory { sku: 0, units: 10 },
        ActionIntent::OpenHedge {
            instrument: 0,
            notional_minor: 1_000,
        },
        ActionIntent::Invest {
            project_id: 1,
            amount_minor: 5_000,
        },
        ActionIntent::ContinueProject { project_id: 1 },
        ActionIntent::AbandonProject { project_id: 1 },
        ActionIntent::ClosePosition { position_id: 0 },
    ];
    for intent in supported {
        match execute_intent(intent, &ctx, &mut books) {
            Ok(_) => {}
            Err(e) => unreachable!("{intent:?} must be supported in Phase 2: {e:?}"),
        }
    }

    let before_balances = books.balances.clone();
    let before_firm = books.firm.clone();
    let before_seq = books.journal.next_seq();
    let refused = [
        // Absent entities.
        ActionIntent::AcceptOffer { offer_id: 3 },
        ActionIntent::DeclineOffer { offer_id: 3 },
        ActionIntent::ClosePosition { position_id: 7 },
        ActionIntent::ContinueProject { project_id: 5 },
        // Dead project.
        ActionIntent::AbandonProject { project_id: 1 },
        // Out-of-range inputs.
        ActionIntent::SetPrice {
            sku: 0,
            tick_price: 0,
        },
        ActionIntent::OrderInventory { sku: 9, units: 1 },
        ActionIntent::Borrow {
            facility: 4,
            amount_minor: 10,
        },
        ActionIntent::Repay {
            facility: 0,
            amount_minor: 10,
        },
    ];
    for intent in refused {
        assert!(
            compile(intent, &ctx, &books.balances, &books.firm).is_err(),
            "{intent:?} must fail closed, never degrade to a silent no-op"
        );
        assert!(execute_intent(intent, &ctx, &mut books).is_err());
    }
    assert_eq!(books.balances, before_balances, "ledger moved on a refusal");
    assert_eq!(books.firm, before_firm, "firm state moved on a refusal");
    assert_eq!(books.journal.next_seq(), before_seq, "journal moved on a refusal");
}

/// The compiler must not let an unknown identifier resolve to a neighbouring
/// one — an off-by-one here would silently spend against the wrong project.
#[test]
fn unknown_identifiers_never_resolve_to_a_neighbour() {
    let (ctx, books) = seeded(7, 1_000_000);
    assert!(matches!(
        compile(
            ActionIntent::SetPrice {
                sku: 250,
                tick_price: 100
            },
            &ctx,
            &books.balances,
            &books.firm
        ),
        Err(CompileError::Firm(FirmError::UnknownSku { sku: 250 }))
    ));
    assert!(matches!(
        compile(
            ActionIntent::ClosePosition { position_id: 4_000 },
            &ctx,
            &books.balances,
            &books.firm
        ),
        Err(CompileError::Firm(FirmError::UnknownPosition {
            position_id: 4_000
        }))
    ));
}

/// The phase order is not advisory: an IPC caller cannot execute a turn by
/// skipping the observation that the decision was supposed to be based on.
#[test]
fn the_public_surface_enforces_the_phase_order() {
    let mut s = session(8);
    assert!(matches!(
        s.submit(ActionIntent::Abstain, None),
        Err(DirectorError::WrongPhase { .. })
    ));
    assert!(matches!(s.execute(), Err(DirectorError::WrongPhase { .. })));
    assert!(matches!(s.settle(), Err(DirectorError::WrongPhase { .. })));
    match s.observe() {
        Ok(_) => {}
        Err(e) => unreachable!("observe failed: {e:?}"),
    }
    assert!(matches!(s.execute(), Err(DirectorError::WrongPhase { .. })));
}
