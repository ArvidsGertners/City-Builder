//! Integer money (cents). Balances never use floats.

use crate::CoreError;
use serde::{Deserialize, Serialize};

/// An amount of in-game currency in cents.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    /// Zero.
    pub const ZERO: Money = Money(0);

    /// Whole currency units (e.g. `Money::units(25)` = 25.00).
    #[must_use]
    pub const fn units(units: i64) -> Self {
        Money(units * 100)
    }

    /// Cents.
    #[must_use]
    pub const fn cents(self) -> i64 {
        self.0
    }

    /// Checked addition.
    pub fn checked_add(self, other: Money) -> Result<Money, CoreError> {
        self.0
            .checked_add(other.0)
            .map(Money)
            .ok_or(CoreError::MoneyOverflow)
    }

    /// Checked subtraction.
    pub fn checked_sub(self, other: Money) -> Result<Money, CoreError> {
        self.0
            .checked_sub(other.0)
            .map(Money)
            .ok_or(CoreError::MoneyOverflow)
    }

    /// Saturating addition (for statistics).
    #[must_use]
    pub fn saturating_add(self, other: Money) -> Money {
        Money(self.0.saturating_add(other.0))
    }

    /// Whether the amount is negative.
    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Splits `self × numerator / denominator` with banker's rounding; returns the rounded amount
    /// and the residual (in 1/denominator cents) to carry to the next transaction.
    #[must_use]
    pub fn scale_round_even(self, numerator: i64, denominator: i64) -> (Money, i64) {
        if denominator == 0 {
            return (Money::ZERO, 0);
        }
        let product = i128::from(self.0) * i128::from(numerator);
        let d = i128::from(denominator);
        let mut q = product / d;
        let r = product % d;
        let twice = 2 * r.abs();
        if twice > d.abs() || (twice == d.abs() && q % 2 != 0) {
            q += if (product < 0) != (d < 0) { -1 } else { 1 };
        }
        let residual = product - q * d;
        (Money(q as i64), residual as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_arithmetic() {
        assert_eq!(Money::units(25).cents(), 2500);
        assert_eq!(Money(5).checked_add(Money(7)), Ok(Money(12)));
        assert!(Money(i64::MAX).checked_add(Money(1)).is_err());
        assert!(Money(-1).is_negative());
    }

    #[test]
    fn bankers_rounding() {
        assert_eq!(Money(5).scale_round_even(1, 2).0, Money(2)); // 2.5 -> 2
        assert_eq!(Money(7).scale_round_even(1, 2).0, Money(4)); // 3.5 -> 4
        assert_eq!(Money(10).scale_round_even(1, 3).0, Money(3)); // 3.33 -> 3
        assert_eq!(Money(-5).scale_round_even(1, 2).0, Money(-2));
        let (m, residual) = Money(10).scale_round_even(1, 3);
        assert_eq!(m.cents() * 3 + residual, 10);
    }
}
