use anyhow::Result;
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};
use statrs::distribution::{ContinuousCDF, Gamma};

use crate::PositiveFinite;

#[derive(Deserialize)]
struct UncheckedGammaParams {
    shape: f64,
    rate: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedGammaParams")]
pub struct GammaParams {
    shape: PositiveFinite,
    rate: PositiveFinite,
}

impl GammaParams {
    pub fn new(shape: PositiveFinite, rate: PositiveFinite) -> Self {
        Self { shape, rate }
    }

    pub fn try_new(shape: f64, rate: f64) -> Result<Self> {
        Ok(Self::new(
            PositiveFinite::try_from_with_error_label(shape, "Gamma shape")?,
            PositiveFinite::try_from_with_error_label(rate, "Gamma rate")?,
        ))
    }

    pub fn shape(&self) -> f64 {
        self.shape.into_inner()
    }

    pub fn rate(&self) -> f64 {
        self.rate.into_inner()
    }

    pub(crate) fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        Gamma::new(self.shape(), self.rate())
            .expect("validated Gamma parameters should construct statrs Gamma")
            .sample(rng)
    }

    pub(crate) fn cdf(&self, x: f64) -> f64 {
        Gamma::new(self.shape(), self.rate())
            .expect("validated Gamma parameters should construct statrs Gamma")
            .cdf(x)
    }

    pub(crate) fn inverse_cdf(&self, p: f64) -> f64 {
        Gamma::new(self.shape(), self.rate())
            .expect("validated Gamma parameters should construct statrs Gamma")
            .inverse_cdf(p)
    }
}

impl TryFrom<UncheckedGammaParams> for GammaParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedGammaParams) -> Result<Self> {
        Self::try_new(value.shape, value.rate)
    }
}
