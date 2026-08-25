use anyhow::Result;
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};
use statrs::{
    distribution::ContinuousCDF,
    statistics::{Max, Min},
};

use super::{Delay, ExpParams, FixedParams, GammaParams, OffsetWeibullParams, UniformParams};
use crate::Probability;

/// Represents a parameterized continuous probability distribution with various types of distributions.
///
/// ## Variants
/// - `Exp`: Exponential distribution with a given rate parameter (`lambda`).
/// - `Gamma`: Gamma distribution with shape and rate parameters.
/// - `Fixed`: A fixed value distribution that always returns the same value.
/// - `Uniform`: Uniform distribution within a specified range (`min`, `max`).
/// - `OffsetWeibull`: Weibull distribution with shape, scale, and an offset.
///
/// ## Implemented Traits
///
/// ### `Distribution<f64>`
/// Allows `sample` random values from the distribution using a random number generator (`Rng`).
///
/// ### `Default`
/// Provides a default value for the distribution, which is `Fixed(1.0)`.
///
/// ### `Min<f64>`
/// Returns the minimum value of the distribution:
/// - Always `0.0` for all distributions.
///
/// ### `Max<f64>`
/// Returns the maximum value of the distribution:
/// - Always `f64::INFINITY` for all distributions.
///
/// ### `ContinuousCDF<f64, f64>`
/// Provides methods for computing the cumulative distribution function (CDF) and the inverse CDF:
/// - `cdf(x)`: Computes the probability that a random variable is less than or equal to `x`.
/// - `inverse_cdf(p)`: Computes the value `x` such that the CDF at `x` equals `p`.
///
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Copy)]
pub enum ContinuousDistributionParameterized {
    Exp(ExpParams),
    Gamma(GammaParams),
    Fixed(FixedParams),
    Uniform(UniformParams),
    OffsetWeibull(OffsetWeibullParams),
}

impl ContinuousDistributionParameterized {
    pub fn exp(rate: f64) -> Result<Self> {
        Ok(Self::Exp(ExpParams::try_new(rate)?))
    }

    pub fn gamma(shape: f64, rate: f64) -> Result<Self> {
        Ok(Self::Gamma(GammaParams::try_new(shape, rate)?))
    }

    pub fn fixed(delay: f64) -> Result<Self> {
        Ok(Self::Fixed(FixedParams::try_new(delay)?))
    }

    pub fn fixed_delay(delay: Delay) -> Self {
        Self::Fixed(FixedParams::new(delay))
    }

    pub fn uniform(min: f64, max: f64) -> Result<Self> {
        Ok(Self::Uniform(UniformParams::try_new(min, max)?))
    }

    pub fn offset_weibull(shape: f64, scale: f64, offset: f64) -> Result<Self> {
        Ok(Self::OffsetWeibull(OffsetWeibullParams::try_new(
            shape, scale, offset,
        )?))
    }

    pub fn probability_at(&self, x: f64) -> Probability {
        Probability::try_from(self.cdf(x)).expect("CDF should return a valid probability")
    }

    pub fn quantile(&self, p: Probability) -> f64 {
        self.inverse_cdf(p.into())
    }
}

impl Distribution<f64> for ContinuousDistributionParameterized {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        match self {
            ContinuousDistributionParameterized::Exp(params) => params.sample(rng),
            ContinuousDistributionParameterized::Gamma(params) => params.sample(rng),
            ContinuousDistributionParameterized::Fixed(params) => params.sample(),
            ContinuousDistributionParameterized::Uniform(params) => params.sample(rng),
            ContinuousDistributionParameterized::OffsetWeibull(params) => params.sample(rng),
        }
    }
}

impl Default for ContinuousDistributionParameterized {
    fn default() -> Self {
        Self::fixed_delay(Delay::ONE)
    }
}

impl Min<f64> for ContinuousDistributionParameterized {
    fn min(&self) -> f64 {
        0.0
    }
}

impl Max<f64> for ContinuousDistributionParameterized {
    fn max(&self) -> f64 {
        f64::INFINITY
    }
}

impl ContinuousCDF<f64, f64> for ContinuousDistributionParameterized {
    fn cdf(&self, x: f64) -> f64 {
        match self {
            ContinuousDistributionParameterized::Exp(params) => params.cdf(x),
            ContinuousDistributionParameterized::Gamma(params) => params.cdf(x),
            ContinuousDistributionParameterized::Fixed(params) => params.cdf(x),
            ContinuousDistributionParameterized::Uniform(params) => params.cdf(x),
            ContinuousDistributionParameterized::OffsetWeibull(params) => params.cdf(x),
        }
    }

    fn inverse_cdf(&self, p: f64) -> f64 {
        match self {
            ContinuousDistributionParameterized::Exp(params) => params.inverse_cdf(p),
            ContinuousDistributionParameterized::Gamma(params) => params.inverse_cdf(p),
            ContinuousDistributionParameterized::Fixed(params) => params.inverse_cdf(p),
            ContinuousDistributionParameterized::Uniform(params) => params.inverse_cdf(p),
            ContinuousDistributionParameterized::OffsetWeibull(params) => params.inverse_cdf(p),
        }
    }
}
