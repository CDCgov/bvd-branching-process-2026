use anyhow::Result;
use rand::{distr::Distribution, Rng};
use serde::{Deserialize, Serialize};
use statrs::distribution::ContinuousCDF;

use super::Offset;
use crate::PositiveFinite;

#[derive(Deserialize)]
struct UncheckedOffsetWeibullParams {
    shape: f64,
    scale: f64,
    offset: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedOffsetWeibullParams")]
pub struct OffsetWeibullParams {
    shape: PositiveFinite,
    scale: PositiveFinite,
    offset: Offset,
}

impl OffsetWeibullParams {
    pub fn new(shape: PositiveFinite, scale: PositiveFinite, offset: Offset) -> Self {
        Self {
            shape,
            scale,
            offset,
        }
    }

    pub fn try_new(shape: f64, scale: f64, offset: f64) -> Result<Self> {
        Ok(Self::new(
            PositiveFinite::try_from_with_error_label(shape, "Weibull shape")?,
            PositiveFinite::try_from_with_error_label(scale, "Weibull scale")?,
            Offset::try_from_with_error_label(offset, "Weibull offset")?,
        ))
    }

    pub fn shape(&self) -> f64 {
        self.shape.into_inner()
    }

    pub fn scale(&self) -> f64 {
        self.scale.into_inner()
    }

    pub fn offset(&self) -> f64 {
        self.offset.into_inner()
    }

    pub(crate) fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        self.weibull().sample(rng) + self.offset()
    }

    pub(crate) fn cdf(&self, x: f64) -> f64 {
        self.weibull().cdf(x - self.offset())
    }

    pub(crate) fn inverse_cdf(&self, p: f64) -> f64 {
        self.weibull().inverse_cdf(p) + self.offset()
    }

    fn weibull(&self) -> statrs::distribution::Weibull {
        statrs::distribution::Weibull::new(self.shape(), self.scale())
            .expect("validated Weibull parameters should construct statrs Weibull")
    }
}

impl TryFrom<UncheckedOffsetWeibullParams> for OffsetWeibullParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedOffsetWeibullParams) -> Result<Self> {
        Self::try_new(value.shape, value.scale, value.offset)
    }
}
