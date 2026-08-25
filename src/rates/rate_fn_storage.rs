use super::{fixed_rate::FixedRateFn, hazard_rates::HazardRateFn};
use crate::parameters::{ContextParametersExt, ParameterValues};
use anyhow::{bail, Result};
use ixa::{define_data_plugin, define_rng, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RateFnId(usize);

struct RateFnContainer {
    rates: Vec<Box<dyn HazardRateFn>>,
}

define_data_plugin!(
    RateFnPlugin,
    RateFnContainer,
    RateFnContainer { rates: Vec::new() }
);

define_rng!(RateFnRng);

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub enum RateFnType {
    FixedRate { rate: f64 },
}

impl RateFnType {
    pub fn is_zero(&self) -> bool {
        match self {
            RateFnType::FixedRate { rate } => *rate == 0.0,
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            RateFnType::FixedRate { rate } => {
                if *rate < 0.0 {
                    bail!("Rate must be non-negative for FixedRate");
                }
            }
        }
        Ok(())
    }
}

impl Default for RateFnType {
    fn default() -> Self {
        RateFnType::FixedRate { rate: 0.0 }
    }
}

pub trait HazardRateExt {
    fn add_rate_fn(&mut self, dist: Box<dyn HazardRateFn>) -> RateFnId;
    fn get_rate_fn(&self, index: RateFnId) -> &dyn HazardRateFn;
}

impl HazardRateExt for Context {
    fn add_rate_fn(&mut self, dist: Box<dyn HazardRateFn>) -> RateFnId {
        let container = self.get_data_mut(RateFnPlugin);
        container.rates.push(dist);
        RateFnId(container.rates.len() - 1)
    }

    fn get_rate_fn(&self, index: RateFnId) -> &dyn HazardRateFn {
        self.get_data(RateFnPlugin).rates[index.0].as_ref()
    }
}

/// Turn the information specified in a `RateFnType` into a useabel rate function
/// # Errors
/// - If the parameters used to specify the rate functions are invalid
/// - If the file specified in the parameters cannot be read and turned into `EmpiricalRate` objects
pub fn load_rate_fn(context: &mut Context, hazard_rate_fn: RateFnType) -> Result<Option<RateFnId>> {
    // Get the max time if necessary
    let &ParameterValues {
        end_run_conditions, ..
    } = context.get_params();

    let rate_id = match hazard_rate_fn {
        RateFnType::FixedRate { rate } => {
            let max_time = end_run_conditions.max_time.into_inner();
            let id = context.add_rate_fn(Box::new(FixedRateFn::new(rate, max_time)?));
            Some(id)
        }
    };

    Ok(rate_id)
}
