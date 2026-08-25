use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use statrs::distribution::Poisson;

use crate::PositiveFinite;

#[derive(Deserialize)]
struct UncheckedPoissonParams {
    mean: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedPoissonParams")]
pub struct PoissonParams {
    mean: PositiveFinite,
}

impl PoissonParams {
    pub fn new(mean: PositiveFinite) -> Self {
        Self { mean }
    }

    pub fn try_new(mean: f64) -> Result<Self> {
        Ok(Self::new(PositiveFinite::try_from_with_error_label(
            mean,
            "Poisson mean",
        )?))
    }

    pub fn mean(&self) -> f64 {
        self.mean.into_inner()
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> usize {
        <Poisson as rand::distr::Distribution<f64>>::sample::<R>(
            &Poisson::new(self.mean())
                .expect("validated Poisson parameters should construct statrs Poisson"),
            rng,
        ) as usize
    }
}

impl TryFrom<UncheckedPoissonParams> for PoissonParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedPoissonParams) -> Result<Self> {
        Self::try_new(value.mean)
    }
}
