use crate::value::Value;
use std::ops::{Add, AddAssign};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(
    any(test, feature = "property-test-api"),
    derive(test_strategy::Arbitrary)
)]
pub struct Stake(pub u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StakeUnit(Stake);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PercentStake {
    pub stake: Stake,
    pub total: Stake,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SplitValueIn {
    pub parts: StakeUnit,
    pub remaining: Stake,
}

impl Stake {
    pub fn from_value(v: Value) -> Self {
        Stake(v.0)
    }

    pub fn zero() -> Self {
        Stake(0)
    }

    pub fn sum<I>(values: I) -> Self
    where
        I: Iterator<Item = Self>,
    {
        values.fold(Stake(0), |acc, v| acc + v)
    }

    #[must_use = "internal state is not modified"]
    pub fn checked_add(&self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    #[must_use = "internal state is not modified"]
    pub fn checked_sub(&self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }

    #[must_use = "internal state is not modified"]
    pub fn wrapping_add(&self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0))
    }

    #[must_use = "internal state is not modified"]
    pub fn wrapping_sub(&self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0))
    }

    /// Divide a value by n equals parts, with a potential remainder
    pub fn split_in(self, n: u32) -> SplitValueIn {
        let n = n as u64;
        SplitValueIn {
            parts: StakeUnit(Stake(self.0 / n)),
            remaining: Stake(self.0 % n),
        }
    }
}

impl From<Stake> for u64 {
    fn from(s: Stake) -> u64 {
        s.0
    }
}

impl AsRef<u64> for Stake {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl StakeUnit {
    #[must_use = "operation does not change the value state"]
    pub fn scale(self, n: u32) -> Stake {
        Stake((self.0).0.checked_mul(n as u64).unwrap())
    }
}

impl PercentStake {
    pub fn new(stake: Stake, total: Stake) -> Self {
        assert!(stake <= total);
        PercentStake { stake, total }
    }

    pub fn as_float(&self) -> f64 {
        (self.stake.0 as f64) / (self.total.0 as f64)
    }

    /// Apply this ratio to a value
    ///
    /// Returned Value = (Value / Total) * Stake
    ///
    /// note that we augment the precision by 10^18 to prevent
    /// early zeroing, as we do the operation using fixed sized integers
    pub fn scale_value(&self, v: Value) -> Value {
        const SCALE: u128 = 1_000_000_000_000_000_000;
        let scaled_divided = (v.0 as u128) * SCALE / self.total.0 as u128;
        let r = (scaled_divided * self.stake.0 as u128) / SCALE;
        Value(r as u64)
    }
}

impl Add for Stake {
    type Output = Stake;

    fn add(self, other: Self) -> Self {
        Stake(self.0 + other.0)
    }
}

impl AddAssign for Stake {
    fn add_assign(&mut self, other: Self) {
        *self = Self(self.0 + other.0)
    }
}

impl std::fmt::Display for Stake {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen};

    impl Arbitrary for Stake {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Stake::from_value(Arbitrary::arbitrary(g))
        }
    }
}
