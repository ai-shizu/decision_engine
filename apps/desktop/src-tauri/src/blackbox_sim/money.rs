//! Integer money for the fictional CRD currency (SPEC §3.3 / §6.1).
//!
//! Authoritative amounts are i64 minor units (100 minor = 1 CRD). Every
//! operation is checked — overflow is a typed error, never a wrap, never a
//! saturation, never a panic (Zero Panic). There is deliberately NO float
//! conversion API: floats enter the ledger nowhere.

use serde::{Deserialize, Serialize};

pub const MINOR_PER_CRD: i64 = 100;

/// Micro scale shared by score quantization (×1e6, §4.15 floor-half-up form).
pub const MICRO: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    Overflow,
    NegativeAmount,
    ZeroAmount,
    InvalidRatio,
}

impl Money {
    pub const ZERO: Money = Money(0);

    #[must_use]
    pub const fn from_minor(v: i64) -> Self {
        Money(v)
    }

    #[must_use]
    pub const fn minor(self) -> i64 {
        self.0
    }

    /// Strictly positive construction for posting amounts — the sign of a
    /// ledger movement lives in `Side`, never in the amount (SPEC §6.1).
    pub fn positive_minor(v: i64) -> Result<Money, MoneyError> {
        if v < 0 {
            return Err(MoneyError::NegativeAmount);
        }
        if v == 0 {
            return Err(MoneyError::ZeroAmount);
        }
        Ok(Money(v))
    }

    pub fn checked_add(self, rhs: Money) -> Result<Money, MoneyError> {
        self.0
            .checked_add(rhs.0)
            .map(Money)
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_sub(self, rhs: Money) -> Result<Money, MoneyError> {
        self.0
            .checked_sub(rhs.0)
            .map(Money)
            .ok_or(MoneyError::Overflow)
    }

    /// Multiply by basis points (1 bp = 1/10,000) with floor-half-up
    /// quantization on i128 intermediates. `bp` is capped at 1,000,000
    /// (= factor 100) so rate math cannot smuggle unbounded multipliers.
    pub fn mul_bp(self, bp: u32) -> Result<Money, MoneyError> {
        if bp > 1_000_000 {
            return Err(MoneyError::InvalidRatio);
        }
        let num = i128::from(self.0)
            .checked_mul(i128::from(bp))
            .ok_or(MoneyError::Overflow)?;
        let q = floor_half_up(num, 10_000)?;
        i64::try_from(q).map(Money).map_err(|_| MoneyError::Overflow)
    }
}

/// floor(n/d + 1/2) for d > 0, in pure integer arithmetic:
/// `q = floor((2n + d) / (2d))` via euclidean division (exact for negatives).
/// This is the §4.15 "floor-half-up" quantization, shared with score micro
/// units (see bias.rs).
pub(crate) fn floor_half_up(n: i128, d: i128) -> Result<i128, MoneyError> {
    if d <= 0 {
        return Err(MoneyError::InvalidRatio);
    }
    let two_n = n.checked_mul(2).ok_or(MoneyError::Overflow)?;
    let num = two_n.checked_add(d).ok_or(MoneyError::Overflow)?;
    let two_d = d.checked_mul(2).ok_or(MoneyError::Overflow)?;
    Ok(num.div_euclid(two_d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_minor_rejects_zero_and_negative() {
        assert!(matches!(
            Money::positive_minor(0),
            Err(MoneyError::ZeroAmount)
        ));
        assert!(matches!(
            Money::positive_minor(-1),
            Err(MoneyError::NegativeAmount)
        ));
        assert!(matches!(Money::positive_minor(1), Ok(m) if m.minor() == 1));
    }

    #[test]
    fn checked_add_overflow_is_typed_error() {
        let a = Money::from_minor(i64::MAX);
        let b = Money::from_minor(1);
        assert!(matches!(a.checked_add(b), Err(MoneyError::Overflow)));
        assert!(matches!(
            Money::from_minor(i64::MIN).checked_sub(Money::from_minor(1)),
            Err(MoneyError::Overflow)
        ));
    }

    #[test]
    fn mul_bp_floor_half_up_rounding() {
        // 150 minor × 50bp = 0.75 → floor(0.75 + 0.5) = 1
        assert!(matches!(
            Money::from_minor(150).mul_bp(50),
            Ok(m) if m.minor() == 1
        ));
        // 100 minor × 50bp = 0.50 → floor(0.5 + 0.5) = 1 (half rounds up)
        assert!(matches!(
            Money::from_minor(100).mul_bp(50),
            Ok(m) if m.minor() == 1
        ));
        // 99 minor × 50bp = 0.495 → 0
        assert!(matches!(
            Money::from_minor(99).mul_bp(50),
            Ok(m) if m.minor() == 0
        ));
        // −150 minor × 50bp = −0.75 → floor(−0.75 + 0.5) = −1
        assert!(matches!(
            Money::from_minor(-150).mul_bp(50),
            Ok(m) if m.minor() == -1
        ));
        // −100 minor × 50bp = −0.50 → floor(0) = 0 (half rounds toward +∞)
        assert!(matches!(
            Money::from_minor(-100).mul_bp(50),
            Ok(m) if m.minor() == 0
        ));
    }

    #[test]
    fn mul_bp_rejects_unbounded_ratio() {
        assert!(matches!(
            Money::from_minor(1).mul_bp(1_000_001),
            Err(MoneyError::InvalidRatio)
        ));
    }

    #[test]
    fn floor_half_up_rejects_nonpositive_denominator() {
        assert!(matches!(floor_half_up(1, 0), Err(MoneyError::InvalidRatio)));
        assert!(matches!(floor_half_up(1, -3), Err(MoneyError::InvalidRatio)));
    }

    #[test]
    fn serde_roundtrip_is_transparent_i64() {
        let m = Money::from_minor(-123_456);
        let json = match serde_json::to_string(&m) {
            Ok(s) => s,
            Err(e) => unreachable!("serialize failed: {e}"),
        };
        assert_eq!(json, "-123456");
        let back: Money = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => unreachable!("deserialize failed: {e}"),
        };
        assert_eq!(back, m);
    }
}
