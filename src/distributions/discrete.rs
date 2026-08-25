use anyhow::Result;
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};

use super::{NegativeBinomialParams, PoissonParams};
use crate::PositiveFinite;

/// Represents a parameterized discrete probability distribution with two types of distributions:
///
/// ## Variants
/// - `Poisson`: A Poisson distribution parameterized by its mean (`lambda`).
/// - `NegativeBinomial`: A Negative Binomial distribution parameterized by its mean and concentration.
///
/// ## Implemented Traits
///
/// ### `Distribution<usize>`
/// Allows `sample` random values from the distribution using a random number generator (`Rng`).
///
/// ### `Default`
/// Provides a default value for the distribution, which is `Poisson(1.0)`.
///
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Copy)]
pub enum DiscreteDistributionParameterized {
    Poisson(PoissonParams),
    NegativeBinomial(NegativeBinomialParams),
}

impl DiscreteDistributionParameterized {
    pub fn poisson(mean: f64) -> Result<Self> {
        Ok(Self::Poisson(PoissonParams::try_new(mean)?))
    }

    pub fn negative_binomial(mean: f64, concentration: f64) -> Result<Self> {
        Ok(Self::NegativeBinomial(NegativeBinomialParams::try_new(
            mean,
            concentration,
        )?))
    }

    /// Return a copy of this distribution with its mean multiplied by `scalar`.
    /// The dispersion parameter (for NegativeBinomial) is left unchanged.
    pub fn with_scaled_mean(self, scalar: f64) -> Result<Self> {
        match self {
            DiscreteDistributionParameterized::Poisson(params) => Ok(Self::Poisson(
                PoissonParams::try_new(params.mean() * scalar)?,
            )),
            DiscreteDistributionParameterized::NegativeBinomial(params) => {
                Ok(Self::NegativeBinomial(NegativeBinomialParams::try_new(
                    params.mean() * scalar,
                    params.concentration(),
                )?))
            }
        }
    }
}

impl Distribution<usize> for DiscreteDistributionParameterized {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> usize {
        match self {
            DiscreteDistributionParameterized::Poisson(params) => params.sample(rng),
            DiscreteDistributionParameterized::NegativeBinomial(params) => params.sample(rng),
        }
    }
}

impl Default for DiscreteDistributionParameterized {
    fn default() -> Self {
        Self::Poisson(PoissonParams::new(PositiveFinite::ONE))
    }
}
