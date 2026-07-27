//! FirmState — the operating model Phase 1 deliberately deferred (SPEC §6,
//! Commander's ruling 2026-07-27).
//!
//! This is the non-monetary half of the authoritative state: how many units
//! sit in which product line, which projects the player has committed to,
//! which offers are on the table, which hedges are open. Everything here is
//! integer and fixed-capacity — a campaign allocates nothing after genesis
//! (SPEC §13: bounds come before allocation, and `with_capacity` is not a
//! bound).
//!
//! Two rules earn their own names because breaking either is silent:
//!
//! - **Inventory reconciliation (BXS-I-17).** Each product line carries BOTH
//!   its unit count and its carrying value, and the value moves by exactly the
//!   integer that is posted to the `Inventory` account. Selling the last unit
//!   sweeps the entire remaining value into COGS rather than computing a
//!   proportional share, so `units == 0` implies `value == 0` exactly. A
//!   moving-average cost recomputed by division would leave rounding residue
//!   behind in an "empty" product line, and that residue would sit in the
//!   balance sheet forever.
//! - **No mark-to-market postings (BXS-W-04).** An open position stores its
//!   entry level and nothing else. Unrealised P&L is a derived view, never a
//!   journal entry; auto-posting revaluation would break the cash-flow
//!   identity (BXS-I-03) because the "gain" has no cash counterpart.

use serde::Serialize;

use super::genesis::{FirmParams, MAX_SKUS};
use super::money::floor_half_up;

pub const MAX_PROJECTS: usize = 8;
pub const MAX_OFFERS: usize = 8;
pub const MAX_POSITIONS: usize = 8;

/// Prices are bounded so a fat-fingered or adversarial intent cannot drive the
/// revenue arithmetic toward i64 limits (第六律 4).
pub const MIN_PRICE_MINOR: i64 = 1;
pub const MAX_PRICE_MINOR: i64 = 10_000_000;
pub const MAX_ORDER_UNITS: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmError {
    UnknownSku { sku: u8 },
    UnknownProject { project_id: u32 },
    UnknownOffer { offer_id: u32 },
    UnknownPosition { position_id: u32 },
    RegistryFull,
    ProjectNotActive { project_id: u32 },
    OfferNotOpen { offer_id: u32 },
    PositionNotOpen { position_id: u32 },
    PriceOutOfRange { price_minor: i64 },
    OrderTooLarge { units: u32 },
    InventoryShort { have: u32, want: u32 },
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkuState {
    pub unit_price_minor: i64,
    pub inventory_units: u32,
    /// Carrying value of `inventory_units`, mirroring the ledger to the minor
    /// unit (BXS-I-17). Never derived by division.
    pub inventory_value_minor: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum ProjectStatus {
    Active = 0,
    /// Written off. The capitalised spend is expensed on the way out.
    Abandoned = 1,
    /// Ran its course. Terminal like `Abandoned`, but carries no write-off:
    /// the capitalised spend stays on the balance sheet and depreciates.
    ///
    /// Appended, never renumbered (BXS-I-13). Distinguishing this from
    /// `Abandoned` matters to lane 4: a project the Director retired on
    /// schedule is not evidence about the player's escalation behaviour, while
    /// one the player abandoned is exactly that.
    Completed = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Capital already sunk. This is the sunk-cost stimulus's whole point:
    /// the estimator compares continue-rates against the identical decision
    /// with this field at zero (SPEC §10 lane 4).
    pub committed_minor: i64,
    pub status: ProjectStatus,
    /// Number of times the player explicitly chose to continue.
    pub continue_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum OfferKind {
    /// Pay a premium now against a modelled loss — the mixed-gamble vehicle.
    Insurance = 0,
    /// Buy capacity now for future throughput.
    Expansion = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum OfferStatus {
    Open = 0,
    Accepted = 1,
    Declined = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub kind: OfferKind,
    pub cost_minor: i64,
    pub status: OfferStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum PositionStatus {
    Open = 0,
    Closed = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub instrument: u8,
    pub notional_minor: i64,
    /// Equity index level at entry, in centi-points. Held so realised P&L can
    /// be computed at close; never used to post a revaluation (BXS-W-04).
    pub entry_index_centi: i64,
    pub status: PositionStatus,
}

/// The exhaustive catalog of non-monetary mutations. `compile` produces one of
/// these; `apply_effect` is the only thing that performs it. Keeping the two
/// apart is what lets compilation stay side-effect free and lets the whole
/// action be staged and rolled back (BXS-I-16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmEffect {
    None,
    SetPrice {
        sku: u8,
        price_minor: i64,
    },
    AddInventory {
        sku: u8,
        units: u32,
        cost_minor: i64,
    },
    ConsumeInventory {
        sku: u8,
        units: u32,
        cogs_minor: i64,
    },
    CommitToProject {
        project_id: u32,
        amount_minor: i64,
    },
    ContinueProject {
        project_id: u32,
    },
    AbandonProject {
        project_id: u32,
    },
    /// Director-side retirement of a planted probe project. No write-off:
    /// unlike `AbandonProject` this is not the player giving up.
    CompleteProject {
        project_id: u32,
    },
    ResolveOffer {
        offer_id: u32,
        accepted: bool,
    },
    OpenPosition {
        instrument: u8,
        notional_minor: i64,
        entry_index_centi: i64,
    },
    ClosePosition {
        position_id: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmState {
    skus: [SkuState; MAX_SKUS],
    projects: [Option<Project>; MAX_PROJECTS],
    offers: [Option<Offer>; MAX_OFFERS],
    positions: [Option<Position>; MAX_POSITIONS],
}

fn slot(id: u32) -> Result<usize, FirmError> {
    usize::try_from(id).map_err(|_| FirmError::Overflow)
}

impl FirmState {
    /// Opening configuration: every line priced at its reference level with an
    /// empty warehouse. Prices start somewhere defensible so an anchoring
    /// measurement has a baseline that was not chosen by the player.
    #[must_use]
    pub fn new(params: &FirmParams) -> Self {
        let mut skus = [SkuState {
            unit_price_minor: MIN_PRICE_MINOR,
            inventory_units: 0,
            inventory_value_minor: 0,
        }; MAX_SKUS];
        for (state, cfg) in skus.iter_mut().zip(params.skus.iter()) {
            state.unit_price_minor = cfg.reference_price_minor;
        }
        Self {
            skus,
            projects: [None; MAX_PROJECTS],
            offers: [None; MAX_OFFERS],
            positions: [None; MAX_POSITIONS],
        }
    }

    pub fn sku(&self, sku: u8) -> Result<SkuState, FirmError> {
        self.skus
            .get(usize::from(sku))
            .copied()
            .ok_or(FirmError::UnknownSku { sku })
    }

    pub fn skus(&self) -> impl Iterator<Item = (u8, SkuState)> + '_ {
        self.skus
            .iter()
            .enumerate()
            .filter_map(|(i, s)| u8::try_from(i).ok().map(|id| (id, *s)))
    }

    pub fn project(&self, project_id: u32) -> Result<Project, FirmError> {
        self.projects
            .get(slot(project_id)?)
            .copied()
            .flatten()
            .ok_or(FirmError::UnknownProject { project_id })
    }

    pub fn offer(&self, offer_id: u32) -> Result<Offer, FirmError> {
        self.offers
            .get(slot(offer_id)?)
            .copied()
            .flatten()
            .ok_or(FirmError::UnknownOffer { offer_id })
    }

    pub fn position(&self, position_id: u32) -> Result<Position, FirmError> {
        self.positions
            .get(slot(position_id)?)
            .copied()
            .flatten()
            .ok_or(FirmError::UnknownPosition { position_id })
    }

    /// Total carrying value across product lines. Must equal the `Inventory`
    /// ledger balance at all times (BXS-I-17); the settle loop asserts it.
    #[must_use]
    pub fn inventory_value_minor(&self) -> i64 {
        self.skus
            .iter()
            .fold(0_i64, |acc, s| acc.saturating_add(s.inventory_value_minor))
    }

    /// Open positions in ascending id order. The order is part of the contract:
    /// the disposition stimulus picks a position by scanning this, so a change
    /// of iteration order would silently repoint a planted measurement.
    pub fn open_positions(&self) -> impl Iterator<Item = (u32, Position)> + '_ {
        self.positions
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.map(|p| (i, p)))
            .filter(|(_, p)| p.status == PositionStatus::Open)
            .filter_map(|(i, p)| u32::try_from(i).ok().map(|id| (id, p)))
    }

    /// Director-side seeding of a fresh project. Returns the slot it landed in.
    ///
    /// See [`FirmState::place_offer`] for why a full registry recycles rather
    /// than refuses, and for the id-reuse hazard that comes with it.
    pub fn open_project(&mut self, committed_minor: i64) -> Result<u32, FirmError> {
        if committed_minor < 0 {
            return Err(FirmError::Overflow);
        }
        let index = Self::claim_slot(&mut self.projects, |p| p.status != ProjectStatus::Active)?;
        let cell = self
            .projects
            .get_mut(index)
            .ok_or(FirmError::RegistryFull)?;
        *cell = Some(Project {
            committed_minor,
            status: ProjectStatus::Active,
            continue_count: 0,
        });
        u32::try_from(index).map_err(|_| FirmError::Overflow)
    }

    /// Director-side seeding (offers are planted, never player-created).
    ///
    /// # Slots are a working set, not an archive
    ///
    /// A campaign plants more probes than there are slots, so when every slot
    /// is taken the lowest-index *settled* entry is retired to make room. That
    /// is safe here and only here: the registry holds what is currently live,
    /// while the record of what happened lives in the decision log and the
    /// stimulus ledger, both append-only (第八律). Retiring a live entry is
    /// impossible by construction — `retired` only matches terminal states.
    ///
    /// The consequence to remember: **ids are reused within a campaign.** Join
    /// an analysis on `stimulus_seq`, which is unique forever; joining on
    /// `offer_id` or `project_id` will silently merge two different probes.
    pub fn place_offer(&mut self, offer: Offer) -> Result<u32, FirmError> {
        let index = Self::claim_slot(&mut self.offers, |o| o.status != OfferStatus::Open)?;
        let cell = self.offers.get_mut(index).ok_or(FirmError::RegistryFull)?;
        *cell = Some(offer);
        u32::try_from(index).map_err(|_| FirmError::Overflow)
    }

    /// Lowest empty slot, else the lowest slot whose occupant is `retired`.
    fn claim_slot<T: Copy>(
        slots: &mut [Option<T>],
        retired: impl Fn(&T) -> bool,
    ) -> Result<usize, FirmError> {
        if let Some(free) = slots.iter().position(Option::is_none) {
            return Ok(free);
        }
        slots
            .iter()
            .position(|slot| slot.as_ref().is_some_and(&retired))
            .ok_or(FirmError::RegistryFull)
    }

    /// How many units this line can sell given stock on hand.
    pub fn sellable_units(&self, sku: u8, wanted: u32) -> Result<u32, FirmError> {
        Ok(self.sku(sku)?.inventory_units.min(wanted))
    }

    /// Cost of goods for `units` out of this line, exact by construction:
    /// selling the whole line releases the whole carrying value, so no
    /// rounding residue can survive in an emptied warehouse (BXS-I-17).
    pub fn cogs_for(&self, sku: u8, units: u32) -> Result<i64, FirmError> {
        let s = self.sku(sku)?;
        if units > s.inventory_units {
            return Err(FirmError::InventoryShort {
                have: s.inventory_units,
                want: units,
            });
        }
        if units == 0 {
            return Ok(0);
        }
        if units == s.inventory_units {
            return Ok(s.inventory_value_minor);
        }
        let num = i128::from(s.inventory_value_minor)
            .checked_mul(i128::from(units))
            .ok_or(FirmError::Overflow)?;
        let q = floor_half_up(num, i128::from(s.inventory_units))
            .map_err(|_| FirmError::Overflow)?;
        i64::try_from(q).map_err(|_| FirmError::Overflow)
    }

    /// Perform a planned mutation. Every arm validates before it writes; a
    /// rejected effect leaves the state untouched.
    pub fn apply_effect(&mut self, effect: FirmEffect) -> Result<(), FirmError> {
        match effect {
            FirmEffect::None => Ok(()),
            FirmEffect::SetPrice { sku, price_minor } => {
                if !(MIN_PRICE_MINOR..=MAX_PRICE_MINOR).contains(&price_minor) {
                    return Err(FirmError::PriceOutOfRange { price_minor });
                }
                let cell = self
                    .skus
                    .get_mut(usize::from(sku))
                    .ok_or(FirmError::UnknownSku { sku })?;
                cell.unit_price_minor = price_minor;
                Ok(())
            }
            FirmEffect::AddInventory {
                sku,
                units,
                cost_minor,
            } => {
                if units > MAX_ORDER_UNITS {
                    return Err(FirmError::OrderTooLarge { units });
                }
                let cell = self
                    .skus
                    .get_mut(usize::from(sku))
                    .ok_or(FirmError::UnknownSku { sku })?;
                let next_units = cell
                    .inventory_units
                    .checked_add(units)
                    .ok_or(FirmError::Overflow)?;
                let next_value = cell
                    .inventory_value_minor
                    .checked_add(cost_minor)
                    .ok_or(FirmError::Overflow)?;
                cell.inventory_units = next_units;
                cell.inventory_value_minor = next_value;
                Ok(())
            }
            FirmEffect::ConsumeInventory {
                sku,
                units,
                cogs_minor,
            } => {
                let cell = self
                    .skus
                    .get_mut(usize::from(sku))
                    .ok_or(FirmError::UnknownSku { sku })?;
                if units > cell.inventory_units {
                    return Err(FirmError::InventoryShort {
                        have: cell.inventory_units,
                        want: units,
                    });
                }
                let next_units = cell
                    .inventory_units
                    .checked_sub(units)
                    .ok_or(FirmError::Overflow)?;
                let next_value = cell
                    .inventory_value_minor
                    .checked_sub(cogs_minor)
                    .ok_or(FirmError::Overflow)?;
                if next_units == 0 && next_value != 0 {
                    // The sweep rule was violated upstream; refuse rather than
                    // leave phantom value in an empty line (BXS-I-17).
                    return Err(FirmError::Overflow);
                }
                cell.inventory_units = next_units;
                cell.inventory_value_minor = next_value;
                Ok(())
            }
            FirmEffect::CommitToProject {
                project_id,
                amount_minor,
            } => {
                let index = slot(project_id)?;
                let cell = self
                    .projects
                    .get_mut(index)
                    .ok_or(FirmError::UnknownProject { project_id })?;
                match cell {
                    Some(p) => {
                        if p.status != ProjectStatus::Active {
                            return Err(FirmError::ProjectNotActive { project_id });
                        }
                        p.committed_minor = p
                            .committed_minor
                            .checked_add(amount_minor)
                            .ok_or(FirmError::Overflow)?;
                    }
                    None => {
                        *cell = Some(Project {
                            committed_minor: amount_minor,
                            status: ProjectStatus::Active,
                            continue_count: 0,
                        });
                    }
                }
                Ok(())
            }
            FirmEffect::ContinueProject { project_id } => {
                let index = slot(project_id)?;
                let cell = self
                    .projects
                    .get_mut(index)
                    .ok_or(FirmError::UnknownProject { project_id })?;
                let p = cell.as_mut().ok_or(FirmError::UnknownProject { project_id })?;
                if p.status != ProjectStatus::Active {
                    return Err(FirmError::ProjectNotActive { project_id });
                }
                p.continue_count = p.continue_count.saturating_add(1);
                Ok(())
            }
            FirmEffect::AbandonProject { project_id } => {
                let index = slot(project_id)?;
                let cell = self
                    .projects
                    .get_mut(index)
                    .ok_or(FirmError::UnknownProject { project_id })?;
                let p = cell.as_mut().ok_or(FirmError::UnknownProject { project_id })?;
                if p.status != ProjectStatus::Active {
                    return Err(FirmError::ProjectNotActive { project_id });
                }
                p.status = ProjectStatus::Abandoned;
                Ok(())
            }
            FirmEffect::CompleteProject { project_id } => {
                let index = slot(project_id)?;
                let cell = self
                    .projects
                    .get_mut(index)
                    .ok_or(FirmError::UnknownProject { project_id })?;
                let p = cell.as_mut().ok_or(FirmError::UnknownProject { project_id })?;
                if p.status != ProjectStatus::Active {
                    return Err(FirmError::ProjectNotActive { project_id });
                }
                p.status = ProjectStatus::Completed;
                Ok(())
            }
            FirmEffect::ResolveOffer { offer_id, accepted } => {
                let index = slot(offer_id)?;
                let cell = self
                    .offers
                    .get_mut(index)
                    .ok_or(FirmError::UnknownOffer { offer_id })?;
                let o = cell.as_mut().ok_or(FirmError::UnknownOffer { offer_id })?;
                if o.status != OfferStatus::Open {
                    return Err(FirmError::OfferNotOpen { offer_id });
                }
                o.status = if accepted {
                    OfferStatus::Accepted
                } else {
                    OfferStatus::Declined
                };
                Ok(())
            }
            FirmEffect::OpenPosition {
                instrument,
                notional_minor,
                entry_index_centi,
            } => {
                let free = self
                    .positions
                    .iter_mut()
                    .find(|s| s.is_none())
                    .ok_or(FirmError::RegistryFull)?;
                *free = Some(Position {
                    instrument,
                    notional_minor,
                    entry_index_centi,
                    status: PositionStatus::Open,
                });
                Ok(())
            }
            FirmEffect::ClosePosition { position_id } => {
                let index = slot(position_id)?;
                let cell = self
                    .positions
                    .get_mut(index)
                    .ok_or(FirmError::UnknownPosition { position_id })?;
                let p = cell
                    .as_mut()
                    .ok_or(FirmError::UnknownPosition { position_id })?;
                if p.status != PositionStatus::Open {
                    return Err(FirmError::PositionNotOpen { position_id });
                }
                p.status = PositionStatus::Closed;
                Ok(())
            }
        }
    }
}

/// Realised profit on closing `position` at `index_centi`, in minor units.
/// Positive = gain. Kept free-standing (not a method) because it is also the
/// derived unrealised figure the view layer shows — the same arithmetic, with
/// the crucial difference that the view never posts it (BXS-W-04).
pub fn position_pnl_minor(position: &Position, index_centi: i64) -> Result<i64, FirmError> {
    let delta = i128::from(index_centi)
        .checked_sub(i128::from(position.entry_index_centi))
        .ok_or(FirmError::Overflow)?;
    let num = delta
        .checked_mul(i128::from(position.notional_minor))
        .ok_or(FirmError::Overflow)?;
    let q = floor_half_up(num, i128::from(position.entry_index_centi.max(1)))
        .map_err(|_| FirmError::Overflow)?;
    i64::try_from(q).map_err(|_| FirmError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::genesis::{build_campaign_genesis, Difficulty, GenesisRequest};

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("firm test setup failed: {e:?}"),
        }
    }

    fn state() -> (FirmState, FirmParams) {
        let g = ok(build_campaign_genesis(GenesisRequest {
            scenario_id: 7,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }));
        (FirmState::new(&g.firm), g.firm)
    }

    #[test]
    fn opens_at_reference_prices_with_empty_warehouse() {
        let (s, params) = state();
        for (id, sku) in s.skus() {
            let cfg = ok(params
                .skus
                .get(usize::from(id))
                .copied()
                .ok_or("missing sku config"));
            assert_eq!(sku.unit_price_minor, cfg.reference_price_minor);
            assert_eq!(sku.inventory_units, 0);
            assert_eq!(sku.inventory_value_minor, 0);
        }
    }

    #[test]
    fn unknown_ids_fail_closed() {
        let (mut s, _) = state();
        assert!(matches!(s.sku(99), Err(FirmError::UnknownSku { sku: 99 })));
        assert!(matches!(
            s.project(3),
            Err(FirmError::UnknownProject { project_id: 3 })
        ));
        assert!(matches!(
            s.offer(3),
            Err(FirmError::UnknownOffer { offer_id: 3 })
        ));
        assert!(matches!(
            s.position(3),
            Err(FirmError::UnknownPosition { position_id: 3 })
        ));
        assert!(matches!(
            s.apply_effect(FirmEffect::SetPrice {
                sku: 99,
                price_minor: 100
            }),
            Err(FirmError::UnknownSku { sku: 99 })
        ));
    }

    #[test]
    fn prices_are_clamped_by_refusal_not_by_clamping() {
        let (mut s, _) = state();
        let before = s.clone();
        for bad in [0_i64, -5, MAX_PRICE_MINOR + 1] {
            assert!(matches!(
                s.apply_effect(FirmEffect::SetPrice {
                    sku: 0,
                    price_minor: bad
                }),
                Err(FirmError::PriceOutOfRange { .. })
            ));
        }
        assert_eq!(s, before, "a refused price must not move the state");
        ok(s.apply_effect(FirmEffect::SetPrice {
            sku: 0,
            price_minor: 12_345,
        }));
        assert_eq!(ok(s.sku(0)).unit_price_minor, 12_345);
    }

    /// BXS-I-17: emptying a product line must leave exactly zero value, even
    /// when the per-unit cost does not divide evenly.
    #[test]
    fn selling_the_last_unit_sweeps_all_residual_value() {
        let (mut s, _) = state();
        // 7 units for 1000 minor => 142.857… per unit, deliberately awkward.
        ok(s.apply_effect(FirmEffect::AddInventory {
            sku: 1,
            units: 7,
            cost_minor: 1_000,
        }));
        let partial = ok(s.cogs_for(1, 3));
        ok(s.apply_effect(FirmEffect::ConsumeInventory {
            sku: 1,
            units: 3,
            cogs_minor: partial,
        }));
        let remaining = ok(s.sku(1));
        assert_eq!(remaining.inventory_units, 4);
        assert_eq!(remaining.inventory_value_minor, 1_000 - partial);
        let rest = ok(s.cogs_for(1, 4));
        assert_eq!(rest, 1_000 - partial, "final draw must release everything");
        ok(s.apply_effect(FirmEffect::ConsumeInventory {
            sku: 1,
            units: 4,
            cogs_minor: rest,
        }));
        let empty = ok(s.sku(1));
        assert_eq!(empty.inventory_units, 0);
        assert_eq!(
            empty.inventory_value_minor, 0,
            "an empty line holding value is the residue bug BXS-I-17 forbids"
        );
    }

    #[test]
    fn cannot_sell_stock_that_is_not_there() {
        let (mut s, _) = state();
        ok(s.apply_effect(FirmEffect::AddInventory {
            sku: 0,
            units: 2,
            cost_minor: 100,
        }));
        assert!(matches!(
            s.cogs_for(0, 3),
            Err(FirmError::InventoryShort { have: 2, want: 3 })
        ));
        assert!(matches!(
            s.apply_effect(FirmEffect::ConsumeInventory {
                sku: 0,
                units: 3,
                cogs_minor: 100
            }),
            Err(FirmError::InventoryShort { have: 2, want: 3 })
        ));
        assert_eq!(ok(s.sku(0)).inventory_units, 2);
    }

    #[test]
    fn project_lifecycle_is_a_one_way_door() {
        let (mut s, _) = state();
        ok(s.apply_effect(FirmEffect::CommitToProject {
            project_id: 2,
            amount_minor: 5_000,
        }));
        ok(s.apply_effect(FirmEffect::CommitToProject {
            project_id: 2,
            amount_minor: 3_000,
        }));
        assert_eq!(ok(s.project(2)).committed_minor, 8_000);
        ok(s.apply_effect(FirmEffect::ContinueProject { project_id: 2 }));
        assert_eq!(ok(s.project(2)).continue_count, 1);
        ok(s.apply_effect(FirmEffect::AbandonProject { project_id: 2 }));
        assert_eq!(ok(s.project(2)).status, ProjectStatus::Abandoned);
        // Abandoned is terminal: no resurrection, no further spend.
        for effect in [
            FirmEffect::ContinueProject { project_id: 2 },
            FirmEffect::AbandonProject { project_id: 2 },
            FirmEffect::CommitToProject {
                project_id: 2,
                amount_minor: 1,
            },
        ] {
            assert!(matches!(
                s.apply_effect(effect),
                Err(FirmError::ProjectNotActive { project_id: 2 })
            ));
        }
        // The sunk figure survives abandonment — lane 4 needs it.
        assert_eq!(ok(s.project(2)).committed_minor, 8_000);
    }

    #[test]
    fn an_offer_resolves_exactly_once() {
        let (mut s, _) = state();
        let id = ok(s.place_offer(Offer {
            kind: OfferKind::Insurance,
            cost_minor: 4_200,
            status: OfferStatus::Open,
        }));
        ok(s.apply_effect(FirmEffect::ResolveOffer {
            offer_id: id,
            accepted: true,
        }));
        assert_eq!(ok(s.offer(id)).status, OfferStatus::Accepted);
        assert!(matches!(
            s.apply_effect(FirmEffect::ResolveOffer {
                offer_id: id,
                accepted: false
            }),
            Err(FirmError::OfferNotOpen { .. })
        ));
    }

    #[test]
    fn registries_reject_rather_than_overwrite_when_full() {
        let (mut s, _) = state();
        for _ in 0..MAX_OFFERS {
            let _ = ok(s.place_offer(Offer {
                kind: OfferKind::Expansion,
                cost_minor: 1,
                status: OfferStatus::Open,
            }));
        }
        assert!(matches!(
            s.place_offer(Offer {
                kind: OfferKind::Expansion,
                cost_minor: 1,
                status: OfferStatus::Open
            }),
            Err(FirmError::RegistryFull)
        ));
        for _ in 0..MAX_POSITIONS {
            ok(s.apply_effect(FirmEffect::OpenPosition {
                instrument: 0,
                notional_minor: 10,
                entry_index_centi: 1_000_000,
            }));
        }
        assert!(matches!(
            s.apply_effect(FirmEffect::OpenPosition {
                instrument: 0,
                notional_minor: 10,
                entry_index_centi: 1_000_000
            }),
            Err(FirmError::RegistryFull)
        ));
    }

    #[test]
    fn position_pnl_is_signed_and_symmetric() {
        let p = Position {
            instrument: 0,
            notional_minor: 1_000_000,
            entry_index_centi: 1_000_000,
            status: PositionStatus::Open,
        };
        assert_eq!(ok(position_pnl_minor(&p, 1_000_000)), 0);
        assert_eq!(ok(position_pnl_minor(&p, 1_100_000)), 100_000);
        assert_eq!(ok(position_pnl_minor(&p, 900_000)), -100_000);
    }

    #[test]
    fn closing_a_position_twice_is_refused() {
        let (mut s, _) = state();
        ok(s.apply_effect(FirmEffect::OpenPosition {
            instrument: 1,
            notional_minor: 500,
            entry_index_centi: 1_000_000,
        }));
        ok(s.apply_effect(FirmEffect::ClosePosition { position_id: 0 }));
        assert_eq!(ok(s.position(0)).status, PositionStatus::Closed);
        assert!(matches!(
            s.apply_effect(FirmEffect::ClosePosition { position_id: 0 }),
            Err(FirmError::PositionNotOpen { position_id: 0 })
        ));
    }

    #[test]
    fn state_stays_inside_the_memory_budget() {
        // SPEC §13: SimCore resident total < 1 MiB. FirmState is fixed-size,
        // so this is a compile-time-ish guard against silent field bloat.
        assert!(
            std::mem::size_of::<FirmState>() <= 1_024,
            "FirmState grew to {} bytes",
            std::mem::size_of::<FirmState>()
        );
    }
}
