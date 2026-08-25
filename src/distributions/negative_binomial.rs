use anyhow::Result;
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};
use statrs::distribution::NegativeBinomial;

use crate::PositiveFinite;

#[derive(Deserialize)]
struct UncheckedNegativeBinomialParams {
    mean: f64,
    concentration: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedNegativeBinomialParams")]
pub struct NegativeBinomialParams {
    mean: PositiveFinite,
    concentration: PositiveFinite,
}

impl NegativeBinomialParams {
    pub fn new(mean: PositiveFinite, concentration: PositiveFinite) -> Self {
        Self {
            mean,
            concentration,
        }
    }

    pub fn try_new(mean: f64, concentration: f64) -> Result<Self> {
        Ok(Self::new(
            PositiveFinite::try_from_with_error_label(mean, "Negative binomial mean")?,
            PositiveFinite::try_from_with_error_label(
                concentration,
                "Negative binomial concentration",
            )?,
        ))
    }

    pub fn mean(&self) -> f64 {
        self.mean.into_inner()
    }

    pub fn concentration(&self) -> f64 {
        self.concentration.into_inner()
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> usize {
        let r = self.concentration();
        let p = self.concentration() / (self.concentration() + self.mean());

        // statrs master fixed the previous parameter convention; pass `p` directly now.
        NegativeBinomial::new(r, p)
            .expect(
                "validated negative binomial parameters should construct statrs negative binomial",
            )
            .sample(rng) as usize
    }
}

impl TryFrom<UncheckedNegativeBinomialParams> for NegativeBinomialParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedNegativeBinomialParams) -> Result<Self> {
        Self::try_new(value.mean, value.concentration)
    }
}
