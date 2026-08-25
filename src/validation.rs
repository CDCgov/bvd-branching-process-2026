use anyhow::{ensure, Result};
use num_traits::{ConstOne, ConstZero, One, Zero};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::{Add, Mul};

pub(crate) fn ensure_positive(name: &str, value: f64) -> Result<()> {
    ensure!(value > 0.0, "{name} must be greater than 0.0");
    Ok(())
}

pub(crate) fn ensure_non_negative(name: &str, value: f64) -> Result<()> {
    ensure!(value >= 0.0, "{name} must be greater than or equal to 0.0");
    Ok(())
}

pub(crate) fn ensure_finite(name: &str, value: f64) -> Result<()> {
    ensure!(value.is_finite(), "{name} must be finite");
    Ok(())
}

pub(crate) fn ensure_positive_finite(name: &str, value: f64) -> Result<()> {
    ensure_finite(name, value)?;
    ensure_positive(name, value)
}

pub(crate) fn ensure_non_negative_finite(name: &str, value: f64) -> Result<()> {
    ensure_finite(name, value)?;
    ensure_non_negative(name, value)
}

fn ensure_probability(name: &str, value: f64) -> Result<()> {
    ensure_finite(name, value)?;
    ensure!(
        (0.0..=1.0).contains(&value),
        "{name} must be between 0.0 and 1.0"
    );
    Ok(())
}

macro_rules! constrained_f64 {
    ($type_name:ident, $validator:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $type_name(OrderedFloat<f64>);

        impl $type_name {
            pub fn into_inner(self) -> f64 {
                self.0.into_inner()
            }

            pub(crate) fn try_from_with_error_label(value: f64, error_label: &str) -> Result<Self> {
                $validator(error_label, value)?;
                Ok(Self(OrderedFloat(value)))
            }
        }

        impl TryFrom<f64> for $type_name {
            type Error = anyhow::Error;

            fn try_from(value: f64) -> Result<Self> {
                Self::try_from_with_error_label(value, "value")
            }
        }

        impl From<$type_name> for f64 {
            fn from(value: $type_name) -> Self {
                value.into_inner()
            }
        }

        impl Add for $type_name {
            type Output = Self;

            fn add(self, rhs: Self) -> Self::Output {
                Self::try_from_with_error_label(
                    self.into_inner() + rhs.into_inner(),
                    concat!(stringify!($type_name), " addition result"),
                )
                .unwrap_or_else(|error| panic!("{error}"))
            }
        }

        impl Mul for $type_name {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self::Output {
                Self::try_from_with_error_label(
                    self.into_inner() * rhs.into_inner(),
                    concat!(stringify!($type_name), " multiplication result"),
                )
                .unwrap_or_else(|error| panic!("{error}"))
            }
        }

        impl Serialize for $type_name {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_f64(self.into_inner())
            }
        }

        impl<'de> Deserialize<'de> for $type_name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = f64::deserialize(deserializer)?;
                Self::try_from(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

constrained_f64!(PositiveFinite, ensure_positive_finite);
constrained_f64!(NonNegativeFinite, ensure_non_negative_finite);
constrained_f64!(Probability, ensure_probability);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositiveCount(usize);

impl PositiveCount {
    pub const ONE: Self = Self(1);

    pub fn into_inner(self) -> usize {
        self.0
    }

    pub(crate) fn try_from_with_error_label(value: usize, error_label: &str) -> Result<Self> {
        ensure!(value > 0, "{error_label} must be greater than 0");
        Ok(Self(value))
    }
}

impl Add for PositiveCount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_add(rhs.0)
                .expect("PositiveCount addition result must fit in usize"),
        )
    }
}

impl Mul for PositiveCount {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_mul(rhs.0)
                .expect("PositiveCount multiplication result must fit in usize"),
        )
    }
}

impl One for PositiveCount {
    fn one() -> Self {
        Self::ONE
    }
}

impl ConstOne for PositiveCount {
    const ONE: Self = Self::ONE;
}

impl TryFrom<usize> for PositiveCount {
    type Error = anyhow::Error;

    fn try_from(value: usize) -> Result<Self> {
        Self::try_from_with_error_label(value, "value")
    }
}

impl From<PositiveCount> for usize {
    fn from(value: PositiveCount) -> Self {
        value.0
    }
}

impl Serialize for PositiveCount {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.0 as u64)
    }
}

impl<'de> Deserialize<'de> for PositiveCount {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = usize::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

impl PositiveFinite {
    pub const ONE: Self = Self(OrderedFloat(1.0));
}

impl One for PositiveFinite {
    fn one() -> Self {
        Self::ONE
    }
}

impl ConstOne for PositiveFinite {
    const ONE: Self = Self::ONE;
}

impl NonNegativeFinite {
    pub const ZERO: Self = Self(OrderedFloat(0.0));
    pub const ONE: Self = Self(OrderedFloat(1.0));
}

impl Zero for NonNegativeFinite {
    fn zero() -> Self {
        Self::ZERO
    }

    fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }
}

impl ConstZero for NonNegativeFinite {
    const ZERO: Self = Self::ZERO;
}

impl One for NonNegativeFinite {
    fn one() -> Self {
        Self::ONE
    }
}

impl ConstOne for NonNegativeFinite {
    const ONE: Self = Self::ONE;
}

impl Default for NonNegativeFinite {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Probability {
    pub const ZERO: Self = Self(OrderedFloat(0.0));
    pub const ONE: Self = Self(OrderedFloat(1.0));
}

impl Zero for Probability {
    fn zero() -> Self {
        Self::ZERO
    }

    fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }
}

impl ConstZero for Probability {
    const ZERO: Self = Self::ZERO;
}

impl One for Probability {
    fn one() -> Self {
        Self::ONE
    }
}

impl ConstOne for Probability {
    const ONE: Self = Self::ONE;
}

impl Default for Probability {
    fn default() -> Self {
        Self::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::{NonNegativeFinite, PositiveCount, PositiveFinite, Probability};
    use num_traits::{ConstOne, ConstZero, One, Zero};

    #[test]
    fn positive_finite_rejects_invalid_values() {
        assert!(PositiveFinite::try_from(1.0).is_ok());
        assert!(PositiveFinite::try_from(0.0).is_err());
        assert!(PositiveFinite::try_from(-1.0).is_err());
        assert!(PositiveFinite::try_from(f64::NAN).is_err());
        assert!(PositiveFinite::try_from(f64::INFINITY).is_err());
    }

    #[test]
    fn non_negative_finite_rejects_invalid_values() {
        assert!(NonNegativeFinite::try_from(0.0).is_ok());
        assert!(NonNegativeFinite::try_from(1.0).is_ok());
        assert!(NonNegativeFinite::try_from(-1.0).is_err());
        assert!(NonNegativeFinite::try_from(f64::NAN).is_err());
        assert!(NonNegativeFinite::try_from(f64::INFINITY).is_err());
    }

    #[test]
    fn probability_rejects_invalid_values() {
        assert!(Probability::try_from(0.0).is_ok());
        assert!(Probability::try_from(0.5).is_ok());
        assert!(Probability::try_from(1.0).is_ok());
        assert!(Probability::try_from(-0.1).is_err());
        assert!(Probability::try_from(1.1).is_err());
        assert!(Probability::try_from(f64::NAN).is_err());
        assert!(Probability::try_from(f64::INFINITY).is_err());
    }

    #[test]
    fn positive_count_rejects_zero() {
        assert!(PositiveCount::try_from(1).is_ok());
        assert!(PositiveCount::try_from(0).is_err());
    }

    #[test]
    fn probability_serializes_as_plain_number() -> anyhow::Result<()> {
        let probability = Probability::try_from(0.25)?;
        assert_eq!(serde_json::to_value(probability)?, 0.25);
        assert_eq!(serde_json::from_str::<Probability>("0.25")?, probability);
        assert!(serde_json::from_str::<Probability>("1.25").is_err());
        Ok(())
    }

    #[test]
    fn constrained_floats_expose_inherent_identity_constants() {
        assert_eq!(PositiveFinite::ONE.into_inner(), 1.0);
        assert_eq!(NonNegativeFinite::ZERO.into_inner(), 0.0);
        assert_eq!(NonNegativeFinite::ONE.into_inner(), 1.0);
        assert_eq!(Probability::ZERO.into_inner(), 0.0);
        assert_eq!(Probability::ONE.into_inner(), 1.0);
    }

    #[test]
    fn positive_count_exposes_inherent_identity_constant() {
        assert_eq!(PositiveCount::ONE.into_inner(), 1);
    }

    #[test]
    fn constrained_floats_implement_num_traits_identities() {
        assert_eq!(PositiveFinite::one(), PositiveFinite::ONE);
        assert_eq!(<PositiveFinite as ConstOne>::ONE, PositiveFinite::ONE);

        assert_eq!(NonNegativeFinite::zero(), NonNegativeFinite::ZERO);
        assert_eq!(NonNegativeFinite::one(), NonNegativeFinite::ONE);
        assert_eq!(
            <NonNegativeFinite as ConstZero>::ZERO,
            NonNegativeFinite::ZERO
        );
        assert_eq!(<NonNegativeFinite as ConstOne>::ONE, NonNegativeFinite::ONE);

        assert_eq!(Probability::zero(), Probability::ZERO);
        assert_eq!(Probability::one(), Probability::ONE);
        assert_eq!(<Probability as ConstZero>::ZERO, Probability::ZERO);
        assert_eq!(<Probability as ConstOne>::ONE, Probability::ONE);
    }

    #[test]
    fn positive_count_implements_num_traits_identity() {
        assert_eq!(PositiveCount::one(), PositiveCount::ONE);
        assert_eq!(<PositiveCount as ConstOne>::ONE, PositiveCount::ONE);
    }

    #[test]
    fn constrained_float_arithmetic_preserves_valid_results() -> anyhow::Result<()> {
        assert_eq!(
            (PositiveFinite::try_from(2.0)? + PositiveFinite::try_from(3.0)?).into_inner(),
            5.0
        );
        assert_eq!(
            (PositiveFinite::try_from(2.0)? * PositiveFinite::try_from(3.0)?).into_inner(),
            6.0
        );
        assert_eq!(
            (NonNegativeFinite::try_from(0.0)? + NonNegativeFinite::try_from(3.0)?).into_inner(),
            3.0
        );
        assert_eq!(
            (Probability::try_from(0.25)? + Probability::try_from(0.5)?).into_inner(),
            0.75
        );
        assert_eq!(
            (Probability::try_from(0.25)? * Probability::try_from(0.5)?).into_inner(),
            0.125
        );
        Ok(())
    }

    #[test]
    fn positive_count_arithmetic_preserves_valid_results() -> anyhow::Result<()> {
        assert_eq!(
            (PositiveCount::try_from(2)? + PositiveCount::try_from(3)?).into_inner(),
            5
        );
        assert_eq!(
            (PositiveCount::try_from(2)? * PositiveCount::try_from(3)?).into_inner(),
            6
        );
        Ok(())
    }

    #[test]
    #[should_panic(expected = "Probability addition result must be between 0.0 and 1.0")]
    fn probability_addition_panics_when_result_exceeds_one() {
        let _ = Probability::try_from(0.75).unwrap() + Probability::try_from(0.5).unwrap();
    }

    #[test]
    #[should_panic(expected = "PositiveFinite multiplication result must be finite")]
    fn constrained_float_arithmetic_panics_when_result_leaves_domain() {
        let _ = PositiveFinite::try_from(f64::MAX).unwrap()
            * PositiveFinite::try_from(f64::MAX).unwrap();
    }

    #[test]
    #[should_panic(expected = "PositiveCount addition result must fit in usize")]
    fn positive_count_addition_panics_on_overflow() {
        let _ = PositiveCount::try_from(usize::MAX).unwrap() + PositiveCount::ONE;
    }

    #[test]
    #[should_panic(expected = "PositiveCount multiplication result must fit in usize")]
    fn positive_count_multiplication_panics_on_overflow() {
        let _ = PositiveCount::try_from(usize::MAX).unwrap() * PositiveCount::try_from(2).unwrap();
    }
}
