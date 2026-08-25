use anyhow::{ensure, Result};
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};
use statrs::distribution::Uniform;

use crate::NonNegativeFinite;

#[derive(Deserialize)]
struct UncheckedUniformParams {
    min: f64,
    max: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedUniformParams")]
pub struct UniformParams {
    min: NonNegativeFinite,
    max: NonNegativeFinite,
}

impl UniformParams {
    pub fn new(min: NonNegativeFinite, max: NonNegativeFinite) -> Result<Self> {
        ensure!(
            max.into_inner() > min.into_inner(),
            "Uniform max must be greater than min"
        );
        Ok(Self { min, max })
    }

    pub fn try_new(min: f64, max: f64) -> Result<Self> {
        Self::new(
            NonNegativeFinite::try_from_with_error_label(min, "Uniform min")?,
            NonNegativeFinite::try_from_with_error_label(max, "Uniform max")?,
        )
    }

    pub fn min(&self) -> f64 {
        self.min.into_inner()
    }

    pub fn max(&self) -> f64 {
        self.max.into_inner()
    }

    pub(crate) fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        Uniform::new(self.min(), self.max())
            .expect("validated Uniform parameters should construct statrs Uniform")
            .sample(rng)
    }

    pub(crate) fn cdf(&self, x: f64) -> f64 {
        if x < self.min() {
            0.0
        } else if x > self.max() {
            1.0
        } else {
            (x - self.min()) / (self.max() - self.min())
        }
    }

    pub(crate) fn inverse_cdf(&self, p: f64) -> f64 {
        self.min() + (self.max() - self.min()) * p
    }
}

impl TryFrom<UncheckedUniformParams> for UniformParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedUniformParams) -> Result<Self> {
        Self::try_new(value.min, value.max)
    }
}
