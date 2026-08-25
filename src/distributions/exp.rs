use anyhow::Result;
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};
use statrs::distribution::{ContinuousCDF, Exp};

use crate::PositiveFinite;

#[derive(Deserialize)]
struct UncheckedExpParams {
    rate: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedExpParams")]
pub struct ExpParams {
    rate: PositiveFinite,
}

impl ExpParams {
    pub fn new(rate: PositiveFinite) -> Self {
        Self { rate }
    }

    pub fn try_new(rate: f64) -> Result<Self> {
        Ok(Self::new(PositiveFinite::try_from_with_error_label(
            rate, "Exp rate",
        )?))
    }

    pub fn rate(&self) -> f64 {
        self.rate.into_inner()
    }

    pub(crate) fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        Exp::new(self.rate())
            .expect("validated Exp parameters should construct statrs Exp")
            .sample(rng)
    }

    pub(crate) fn cdf(&self, x: f64) -> f64 {
        Exp::new(self.rate())
            .expect("validated Exp parameters should construct statrs Exp")
            .cdf(x)
    }

    pub(crate) fn inverse_cdf(&self, p: f64) -> f64 {
        Exp::new(self.rate())
            .expect("validated Exp parameters should construct statrs Exp")
            .inverse_cdf(p)
    }
}

impl TryFrom<UncheckedExpParams> for ExpParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedExpParams) -> Result<Self> {
        Self::try_new(value.rate)
    }
}
