use crate::detection_manager::{TransmissionChain, TransmissionChainData};
use anyhow::{bail, Result};
use ixa::prelude::*;
use ordered_float::OrderedFloat;

use crate::{
    importation::initiate_importation,
    parameters::{ContextParametersExt, ParameterValues},
    rates::RateFnType,
    timekeeping::TimeKeeper,
    Person,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Default)]
pub enum NovelInfectionSource {
    SpilloverEvent {
        days_since_start: OrderedFloat<f64>,
        exposures: usize,
    },
    HazardRate(RateFnType),
    #[default]
    None,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct InfectionInitialization {
    pub start_date: TimeKeeper,
    pub initial_cases: NovelInfectionSource,
}

impl Default for InfectionInitialization {
    fn default() -> Self {
        InfectionInitialization {
            start_date: TimeKeeper::new(2025, 1, 1).expect("Failed to create default start date"),
            initial_cases: NovelInfectionSource::default(),
        }
    }
}

impl InfectionInitialization {
    pub fn validate(&self) -> Result<()> {
        match self.initial_cases {
            NovelInfectionSource::SpilloverEvent {
                exposures,
                days_since_start,
            } => {
                if exposures == 0 {
                    bail!("Exposures must be greater than 0 for SpilloverEvent");
                }
                if days_since_start.into_inner() < 0.0 {
                    bail!("Days since start must be non-negative for SpilloverEvent");
                }
                // A spillover scheduled after max_time is intentionally allowed: the
                // plan is queued but the run ends (shutdown at max_time) before it
                // fires, so the outbreak simply never seeds. Calibration proposes
                // such particles from the (unbounded) spillover prior and rejects
                // them via the resulting empty run rather than crashing the ABC.
            }
            NovelInfectionSource::HazardRate(rate_fn) => {
                rate_fn.validate()?;
            }
            NovelInfectionSource::None => {}
        }
        Ok(())
    }
    pub fn from_spillover(days_since_start: f64, exposures: usize) -> Self {
        InfectionInitialization {
            start_date: TimeKeeper::new(2025, 1, 1).expect("Failed to create default start date"),
            initial_cases: NovelInfectionSource::SpilloverEvent {
                days_since_start: OrderedFloat(days_since_start),
                exposures,
            },
        }
    }
    pub fn from_hazard_rate(rate: f64) -> Self {
        InfectionInitialization {
            start_date: TimeKeeper::new(2025, 1, 1).expect("Failed to create default start date"),
            initial_cases: NovelInfectionSource::HazardRate(RateFnType::FixedRate { rate }),
        }
    }
    pub fn get_rate_fn(&self) -> Option<RateFnType> {
        match self.initial_cases {
            NovelInfectionSource::HazardRate(rate_fn) => Some(rate_fn),
            _ => None,
        }
    }
}

pub fn init(context: &mut Context) -> Result<()> {
    // Add index infection(s)
    let ParameterValues { initialization, .. } = context.get_params();
    match initialization.initial_cases {
        NovelInfectionSource::SpilloverEvent {
            days_since_start,
            exposures,
        } => {
            let time = days_since_start.into_inner();
            context.add_plan(time, move |context| {
                for _ in 0..exposures {
                    let _index = context
                        .add_entity(with!(
                            Person,
                            TransmissionChain(Some(TransmissionChainData::new(time)))
                        ))
                        .expect("Failed to add initial infection");
                }
            });
        }
        NovelInfectionSource::HazardRate(rate_fn) => {
            initiate_importation(context, rate_fn)?;
        }
        NovelInfectionSource::None => {}
    }
    Ok(())
}
