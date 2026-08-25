use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::Delay;

#[derive(Deserialize)]
struct UncheckedFixedParams {
    delay: f64,
}

#[derive(Clone, Debug, PartialEq, Copy, Serialize, Deserialize)]
#[serde(try_from = "UncheckedFixedParams")]
pub struct FixedParams {
    delay: Delay,
}

impl FixedParams {
    pub fn new(delay: Delay) -> Self {
        Self { delay }
    }

    pub fn try_new(delay: f64) -> Result<Self> {
        Ok(Self::new(Delay::try_from_with_error_label(
            delay,
            "Fixed delay",
        )?))
    }

    pub fn delay(&self) -> f64 {
        self.delay.into_inner()
    }

    pub(crate) fn sample(&self) -> f64 {
        self.delay()
    }

    pub(crate) fn cdf(&self, x: f64) -> f64 {
        if x < self.delay() {
            0.0
        } else {
            1.0
        }
    }

    pub(crate) fn inverse_cdf(&self, p: f64) -> f64 {
        self.delay() * p
    }
}

impl TryFrom<UncheckedFixedParams> for FixedParams {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedFixedParams) -> Result<Self> {
        Self::try_new(value.delay)
    }
}
