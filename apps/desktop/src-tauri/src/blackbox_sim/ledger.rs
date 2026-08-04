//! Double-entry bookkeeping core (SPEC §6, BXS-I-02/03/04/05).
//!
//! Design stance: a Transaction is balanced-by-construction — an unbalanced
//! value cannot exist. Balances use the signed debit-positive convention so
//! the trial-balance identity is a single zero-sum. The identity holds by
//! induction over balanced applies, yet it is still re-verified at settle and
//! load boundaries (第六律: the receiver verifies; writer health exempts
//! nothing). Corruption is a typed error — never repaired (§16.2/§16.5).
//!
//! The only writer of transactions is the Phase 2 ActionCompiler expanding
//! the closed `ActionIntent` catalog; UI and LLM can never author postings
//! (wall W-d).

use serde::Serialize;

use super::money::{Money, MoneyError};
use super::ring::{FixedRing, RingConfigError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountClass {
    Asset,
    Liability,
    Equity,
    Revenue,
    Expense,
}

/// Chart of accounts. Discriminants are frozen wire ids (BXS-I-13):
/// never renumber, never reuse a retired number. Additions append only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum AccountCode {
    Cash = 0,
    AccountsReceivable = 1,
    Inventory = 2,
    PrepaidExpenses = 3,
    PropertyPlantEquipment = 4,
    /// Contra-asset (Asset class, credit-normal balance).
    AccumulatedDepreciation = 5,
    AccountsPayable = 6,
    AccruedLiabilities = 7,
    SeniorDebt = 8,
    MezzanineDebt = 9,
    TaxPayable = 10,
    ShareCapital = 11,
    RetainedEarnings = 12,
    SalesRevenue = 13,
    OtherIncome = 14,
    CostOfGoodsSold = 15,
    OperatingExpense = 16,
    DepreciationExpense = 17,
    InterestExpense = 18,
    TaxExpense = 19,
}

pub const N_ACCOUNTS: usize = 20;

impl AccountCode {
    pub const ALL: [AccountCode; N_ACCOUNTS] = [
        AccountCode::Cash,
        AccountCode::AccountsReceivable,
        AccountCode::Inventory,
        AccountCode::PrepaidExpenses,
        AccountCode::PropertyPlantEquipment,
        AccountCode::AccumulatedDepreciation,
        AccountCode::AccountsPayable,
        AccountCode::AccruedLiabilities,
        AccountCode::SeniorDebt,
        AccountCode::MezzanineDebt,
        AccountCode::TaxPayable,
        AccountCode::ShareCapital,
        AccountCode::RetainedEarnings,
        AccountCode::SalesRevenue,
        AccountCode::OtherIncome,
        AccountCode::CostOfGoodsSold,
        AccountCode::OperatingExpense,
        AccountCode::DepreciationExpense,
        AccountCode::InterestExpense,
        AccountCode::TaxExpense,
    ];

    #[must_use]
    pub const fn class(self) -> AccountClass {
        match self {
            AccountCode::Cash
            | AccountCode::AccountsReceivable
            | AccountCode::Inventory
            | AccountCode::PrepaidExpenses
            | AccountCode::PropertyPlantEquipment
            | AccountCode::AccumulatedDepreciation => AccountClass::Asset,
            AccountCode::AccountsPayable
            | AccountCode::AccruedLiabilities
            | AccountCode::SeniorDebt
            | AccountCode::MezzanineDebt
            | AccountCode::TaxPayable => AccountClass::Liability,
            AccountCode::ShareCapital | AccountCode::RetainedEarnings => AccountClass::Equity,
            AccountCode::SalesRevenue | AccountCode::OtherIncome => AccountClass::Revenue,
            AccountCode::CostOfGoodsSold
            | AccountCode::OperatingExpense
            | AccountCode::DepreciationExpense
            | AccountCode::InterestExpense
            | AccountCode::TaxExpense => AccountClass::Expense,
        }
    }

    #[inline]
    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Debit,
    Credit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Posting {
    pub account: AccountCode,
    pub side: Side,
    pub amount: Money,
}

/// Closed business-event catalog (SPEC §6.1). Additions are schema-versioned;
/// UI and LLM never see this type — they submit `ActionIntent` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TxKind {
    OpeningEquity,
    PurchaseInventory,
    RecognizeSale,
    PayOpex,
    RecordDepreciation,
    DrawDebt,
    ServiceDebt,
    AccrueInterest,
    PayTax,
    CapexPurchase,
    HedgeSettlement,
}

pub const MAX_POSTINGS_PER_TX: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerError {
    EmptyTransaction,
    SinglePosting,
    TooManyPostings { got: usize },
    NonPositiveAmount,
    Unbalanced { debit_minor: i128, credit_minor: i128 },
    Overflow,
    JournalTailFull,
    Money(MoneyError),
    RingConfig(RingConfigError),
}

impl From<MoneyError> for LedgerError {
    fn from(e: MoneyError) -> Self {
        LedgerError::Money(e)
    }
}

/// Balanced-by-construction transaction (BXS-I-05). Postings are private:
/// the only way to obtain a `Transaction` is `new()`, and `new()` refuses
/// empty / single-legged / non-positive / oversized / unbalanced input
/// outright (no silent sanitization — §16.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    kind: TxKind,
    postings: Vec<Posting>,
}

impl Transaction {
    pub fn new(kind: TxKind, postings: Vec<Posting>) -> Result<Self, LedgerError> {
        match postings.len() {
            0 => return Err(LedgerError::EmptyTransaction),
            1 => return Err(LedgerError::SinglePosting),
            n if n > MAX_POSTINGS_PER_TX => return Err(LedgerError::TooManyPostings { got: n }),
            _ => {}
        }
        let mut debit: i128 = 0;
        let mut credit: i128 = 0;
        for p in &postings {
            if p.amount.minor() <= 0 {
                return Err(LedgerError::NonPositiveAmount);
            }
            // ≤16 postings × i64 magnitude cannot overflow i128; plain adds
            // are total here by construction.
            match p.side {
                Side::Debit => debit += i128::from(p.amount.minor()),
                Side::Credit => credit += i128::from(p.amount.minor()),
            }
        }
        if debit != credit {
            return Err(LedgerError::Unbalanced {
                debit_minor: debit,
                credit_minor: credit,
            });
        }
        Ok(Self { kind, postings })
    }

    #[must_use]
    pub fn kind(&self) -> TxKind {
        self.kind
    }

    #[must_use]
    pub fn postings(&self) -> &[Posting] {
        &self.postings
    }
}

/// Natural-sign statement totals (all non-negative for a healthy book).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassTotals {
    pub asset: i128,
    pub liability: i128,
    pub equity: i128,
    pub revenue: i128,
    pub expense: i128,
}

/// Signed balances, debit-positive convention (SPEC §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Balances {
    minor: [i64; N_ACCOUNTS],
}

impl Default for Balances {
    fn default() -> Self {
        Self::new()
    }
}

impl Balances {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            minor: [0; N_ACCOUNTS],
        }
    }

    #[must_use]
    pub fn balance_minor(&self, account: AccountCode) -> i64 {
        self.minor.get(account.index()).copied().unwrap_or(0)
    }

    /// Test-only: mutate a single account outside double-entry.
    ///
    /// Production `apply` cannot break BXS-I-03 — balanced postings preserve the
    /// cash-flow identity by construction. The AccountingBreach / InventoryDesync
    /// die() sites exist to refuse *corruption*, so host positive-control tests
    /// must be able to plant corruption without inventing a production API for it.
    #[cfg(test)]
    pub(crate) fn test_add_unchecked(&mut self, account: AccountCode, delta: i64) {
        let idx = account.index();
        if let Some(slot) = self.minor.get_mut(idx) {
            *slot = slot.wrapping_add(delta);
        }
    }

    /// Atomic apply (BXS-I-04): stage every delta with overflow checks, then
    /// commit the whole staged array. On any error, `self` is untouched.
    pub fn apply(&mut self, tx: &Transaction) -> Result<(), LedgerError> {
        let mut staged = self.minor;
        for p in tx.postings() {
            let idx = p.account.index();
            let cur = staged.get(idx).copied().ok_or(LedgerError::Overflow)?;
            let delta = match p.side {
                Side::Debit => p.amount.minor(),
                Side::Credit => p.amount.minor().checked_neg().ok_or(LedgerError::Overflow)?,
            };
            let next = cur.checked_add(delta).ok_or(LedgerError::Overflow)?;
            match staged.get_mut(idx) {
                Some(slot) => *slot = next,
                None => return Err(LedgerError::Overflow),
            }
        }
        self.minor = staged;
        Ok(())
    }

    /// Trial-balance identity (BXS-I-02): Σ signed balances == 0.
    pub fn verify_zero_sum(&self) -> Result<(), LedgerError> {
        let sum: i128 = self.minor.iter().map(|v| i128::from(*v)).sum();
        if sum == 0 {
            Ok(())
        } else {
            Err(LedgerError::Unbalanced {
                debit_minor: sum.max(0),
                credit_minor: sum.checked_neg().unwrap_or(i128::MAX).max(0),
            })
        }
    }

    /// Natural-sign totals per statement class:
    /// A = Σ asset_signed, L = −Σ liability_signed, E = −Σ equity_signed,
    /// R = −Σ revenue_signed, X = Σ expense_signed.
    #[must_use]
    pub fn class_totals(&self) -> ClassTotals {
        let mut asset: i128 = 0;
        let mut liability: i128 = 0;
        let mut equity: i128 = 0;
        let mut revenue: i128 = 0;
        let mut expense: i128 = 0;
        for account in AccountCode::ALL {
            let signed = i128::from(self.balance_minor(account));
            match account.class() {
                AccountClass::Asset => asset += signed,
                AccountClass::Liability => liability -= signed,
                AccountClass::Equity => equity -= signed,
                AccountClass::Revenue => revenue -= signed,
                AccountClass::Expense => expense += signed,
            }
        }
        ClassTotals {
            asset,
            liability,
            equity,
            revenue,
            expense,
        }
    }

    /// Statement identity: A == L + E + (R − X). Equivalent to the zero-sum
    /// under the sign conventions of `class_totals`, verified independently
    /// (合計値の一致だけで正しさを宣言するな — §16.6).
    pub fn verify_accounting_identity(&self) -> Result<(), LedgerError> {
        let t = self.class_totals();
        let rhs = t.liability + t.equity + t.revenue - t.expense;
        if t.asset == rhs {
            Ok(())
        } else {
            Err(LedgerError::Unbalanced {
                debit_minor: t.asset,
                credit_minor: rhs,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub seq: u64,
    pub tx: Transaction,
}

pub const JOURNAL_TAIL_CAPACITY: usize = 1_024;

/// Bounded in-memory journal tail. Full history lives in vault generations
/// (Phase 3); this working set REJECTS appends when full — the Settle phase
/// must drain it first. Records are never silently evicted (第八律 /
/// BXS-I-08; contrast with the evicting market-series ring, trap BXS-W-03).
#[derive(Debug, Clone)]
pub struct Journal {
    next_seq: u64,
    tail: FixedRing<JournalEntry>,
}

impl Journal {
    pub fn new() -> Result<Self, LedgerError> {
        let tail = FixedRing::new(JOURNAL_TAIL_CAPACITY).map_err(LedgerError::RingConfig)?;
        Ok(Self { next_seq: 0, tail })
    }

    /// Append with monotonic sequencing; all-or-nothing (seq does not advance
    /// on rejection).
    pub fn append(&mut self, tx: Transaction) -> Result<u64, LedgerError> {
        let seq = self.next_seq;
        let next = seq.checked_add(1).ok_or(LedgerError::Overflow)?;
        if self.tail.is_full() {
            return Err(LedgerError::JournalTailFull);
        }
        self.tail
            .try_push(JournalEntry { seq, tx })
            .map_err(|_| LedgerError::JournalTailFull)?;
        self.next_seq = next;
        Ok(seq)
    }

    /// Explicit Settle-time drain (flush to vault in Phase 3).
    pub fn drain_settled(&mut self) -> Vec<JournalEntry> {
        self.tail.drain_all()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tail.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tail.is_empty()
    }

    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Slots still free in the tail. Callers that must post several entries
    /// as one indivisible act reserve up front: once the room is known to
    /// exist, `append` cannot fail, which is what lets a multi-transaction
    /// step be staged and committed atomically (BXS-I-16).
    #[must_use]
    pub fn remaining_capacity(&self) -> usize {
        JOURNAL_TAIL_CAPACITY.saturating_sub(self.tail.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("test setup failed: {e:?}"),
        }
    }

    fn posting(account: AccountCode, side: Side, minor: i64) -> Posting {
        Posting {
            account,
            side,
            amount: ok(Money::positive_minor(minor)),
        }
    }

    #[test]
    fn unbalanced_rejected() {
        let r = Transaction::new(
            TxKind::RecognizeSale,
            vec![
                posting(AccountCode::Cash, Side::Debit, 1_000),
                posting(AccountCode::SalesRevenue, Side::Credit, 999),
            ],
        );
        assert!(matches!(
            r,
            Err(LedgerError::Unbalanced {
                debit_minor: 1_000,
                credit_minor: 999
            })
        ));
    }

    #[test]
    fn degenerate_shapes_rejected() {
        assert!(matches!(
            Transaction::new(TxKind::PayOpex, vec![]),
            Err(LedgerError::EmptyTransaction)
        ));
        assert!(matches!(
            Transaction::new(
                TxKind::PayOpex,
                vec![posting(AccountCode::Cash, Side::Debit, 5)]
            ),
            Err(LedgerError::SinglePosting)
        ));
        let many: Vec<Posting> = (0..17)
            .map(|_| posting(AccountCode::Cash, Side::Debit, 1))
            .collect();
        assert!(matches!(
            Transaction::new(TxKind::PayOpex, many),
            Err(LedgerError::TooManyPostings { got: 17 })
        ));
    }

    #[test]
    fn balanced_apply_preserves_zero_sum_and_identity() {
        let mut b = Balances::new();
        let seed_equity = ok(Transaction::new(
            TxKind::OpeningEquity,
            vec![
                posting(AccountCode::Cash, Side::Debit, 1_000_000),
                posting(AccountCode::ShareCapital, Side::Credit, 1_000_000),
            ],
        ));
        let buy_inventory = ok(Transaction::new(
            TxKind::PurchaseInventory,
            vec![
                posting(AccountCode::Inventory, Side::Debit, 300_000),
                posting(AccountCode::Cash, Side::Credit, 250_000),
                posting(AccountCode::AccountsPayable, Side::Credit, 50_000),
            ],
        ));
        let sale = ok(Transaction::new(
            TxKind::RecognizeSale,
            vec![
                posting(AccountCode::Cash, Side::Debit, 480_000),
                posting(AccountCode::SalesRevenue, Side::Credit, 480_000),
                posting(AccountCode::CostOfGoodsSold, Side::Debit, 200_000),
                posting(AccountCode::Inventory, Side::Credit, 200_000),
            ],
        ));
        ok(b.apply(&seed_equity));
        ok(b.apply(&buy_inventory));
        ok(b.apply(&sale));
        ok(b.verify_zero_sum());
        ok(b.verify_accounting_identity());
        let t = b.class_totals();
        assert_eq!(t.asset, 1_330_000); // cash 1,230,000 + inventory 100,000
        assert_eq!(t.liability, 50_000);
        assert_eq!(t.equity, 1_000_000);
        assert_eq!(t.revenue, 480_000);
        assert_eq!(t.expense, 200_000);
        assert_eq!(b.balance_minor(AccountCode::Cash), 1_230_000);
    }

    #[test]
    fn overflow_apply_is_atomic() {
        let mut b = Balances::new();
        let near_max = i64::MAX - 10;
        let seed = ok(Transaction::new(
            TxKind::OpeningEquity,
            vec![
                posting(AccountCode::Cash, Side::Debit, near_max),
                posting(AccountCode::ShareCapital, Side::Credit, near_max),
            ],
        ));
        ok(b.apply(&seed));
        let before = b.clone();
        let bump = ok(Transaction::new(
            TxKind::RecognizeSale,
            vec![
                posting(AccountCode::Cash, Side::Debit, 1_000),
                posting(AccountCode::SalesRevenue, Side::Credit, 1_000),
            ],
        ));
        assert!(matches!(b.apply(&bump), Err(LedgerError::Overflow)));
        assert_eq!(b, before, "failed apply must leave state untouched");
        ok(b.verify_zero_sum());
    }

    #[test]
    fn journal_is_monotonic_and_rejects_when_full() {
        let mut j = ok(Journal::new());
        let tx = ok(Transaction::new(
            TxKind::PayOpex,
            vec![
                posting(AccountCode::OperatingExpense, Side::Debit, 10),
                posting(AccountCode::Cash, Side::Credit, 10),
            ],
        ));
        for expected_seq in 0..JOURNAL_TAIL_CAPACITY as u64 {
            let seq = ok(j.append(tx.clone()));
            assert_eq!(seq, expected_seq);
        }
        assert!(matches!(
            j.append(tx.clone()),
            Err(LedgerError::JournalTailFull)
        ));
        // seq must not advance on rejection; drain re-opens the tail.
        assert_eq!(j.next_seq(), JOURNAL_TAIL_CAPACITY as u64);
        let drained = j.drain_settled();
        assert_eq!(drained.len(), JOURNAL_TAIL_CAPACITY);
        assert!(matches!(
            drained.first(),
            Some(JournalEntry { seq: 0, .. })
        ));
        let seq = ok(j.append(tx));
        assert_eq!(seq, JOURNAL_TAIL_CAPACITY as u64);
    }

    #[test]
    fn account_discriminants_frozen() {
        // BXS-I-13: wire ids are a contract; renumbering is unconstitutional.
        assert_eq!(AccountCode::Cash as u8, 0);
        assert_eq!(AccountCode::AccumulatedDepreciation as u8, 5);
        assert_eq!(AccountCode::SeniorDebt as u8, 8);
        assert_eq!(AccountCode::TaxExpense as u8, 19);
        assert_eq!(AccountCode::ALL.len(), N_ACCOUNTS);
    }
}
