use crate::parameters::{ContextParametersExt, ParameterValues};
use anyhow::{anyhow, Result};
use chrono::NaiveDate;
use ixa::prelude::*;
use serde::{Deserialize, Serialize};

const DATE_FORMAT: &str = "%Y-%m-%d";

#[derive(Deserialize, Serialize, Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct TimeKeeper(pub NaiveDate);

impl TryFrom<String> for TimeKeeper {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self> {
        let date = NaiveDate::parse_from_str(&value, DATE_FORMAT).map_err(|e| {
            anyhow!("Invalid date '{value}' for TimeKeeper (expected YYYY-MM-DD): {e}")
        })?;
        Ok(TimeKeeper(date))
    }
}

impl From<TimeKeeper> for String {
    fn from(value: TimeKeeper) -> Self {
        value.0.format(DATE_FORMAT).to_string()
    }
}

impl TimeKeeper {
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self> {
        let date = NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
            anyhow!("Invalid date '{year}-{month}-{day}' for TimeKeeper (expected YYYY-MM-DD)")
        })?;
        Ok(TimeKeeper(date))
    }
    pub fn new_from_str(date: String) -> Result<Self> {
        let keeper = TimeKeeper::try_from(date)
            .map_err(|e| anyhow!("Invalid date for TimeKeeper (expected YYYY-MM-DD): {e}"))?;
        Ok(keeper)
    }
    pub fn time_since_start(&self, start_date: TimeKeeper) -> f64 {
        let date = self.0;
        let start_date = start_date.0;
        (date - start_date).num_days() as f64
    }
    pub fn apply_time_delta(&self, delta: f64) -> TimeKeeper {
        // Round float difference to nearest day difference
        let new_date = self.0 + chrono::Duration::days(delta.round() as i64);
        TimeKeeper(new_date)
    }
}

pub trait ContextTimeKeeperExt: PluginContext + ContextParametersExt {
    fn get_date_at_time(&self, time: f64) -> TimeKeeper {
        let ParameterValues { initialization, .. } = self.get_params();
        initialization.start_date.apply_time_delta(time)
    }
    fn get_current_date(&self) -> TimeKeeper {
        self.get_date_at_time(self.get_current_time())
    }
    fn add_plan_from_timekeeper(
        &mut self,
        timekeeper: TimeKeeper,
        callback: impl FnOnce(&mut Context) + Send + 'static,
    ) {
        let ParameterValues { initialization, .. } = self.get_params();
        let time = timekeeper.time_since_start(initialization.start_date);
        self.add_plan(time, callback);
    }
}
impl ContextTimeKeeperExt for Context {}

#[cfg(test)]
mod test {
    use super::*;
    use crate::infection_initialization::{InfectionInitialization, NovelInfectionSource};
    use crate::parameters::{ParameterValues, Parameters};
    use ixa::assert_almost_eq;

    #[test]
    fn test_timekeeper_from_str() {
        let keeper = TimeKeeper::try_from("2025-01-10".to_string()).unwrap();

        let true_date = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
        assert_eq!(keeper, TimeKeeper(true_date));
    }

    #[test]
    fn test_timekeeper_invalid() {
        let keeper = TimeKeeper::try_from("invalid-date".to_string());
        assert!(keeper.is_err());
    }

    #[test]
    fn test_timekeeper_invalid_date() {
        let keeper = TimeKeeper::try_from("2025-02-30".to_string());
        assert!(keeper.is_err());
    }

    #[test]
    fn test_time_since_start() {
        let start = TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        let later = TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap());
        assert_eq!(later.time_since_start(start), 9.0);
    }

    #[test]
    fn test_apply_time_delta() {
        let keeper = TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        let new_keeper = keeper.apply_time_delta(9.0);
        assert_eq!(
            new_keeper,
            TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap())
        );
    }

    #[test]
    fn test_apply_partial_time_delta() {
        let keeper = TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());

        let new_keeper = keeper.apply_time_delta(9.4);
        assert_eq!(
            new_keeper,
            TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap())
        );

        let new_keeper = keeper.apply_time_delta(9.6);
        assert_eq!(
            new_keeper,
            TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 11).unwrap())
        );

        let new_keeper = keeper.apply_time_delta(-1.2);
        assert_eq!(
            new_keeper,
            TimeKeeper(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap())
        );
    }

    #[test]
    fn test_get_date_at_time() {
        let mut context = Context::new();
        let params = ParameterValues {
            initialization: InfectionInitialization {
                start_date: TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
                initial_cases: NovelInfectionSource::None,
            },
            ..Default::default()
        };
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        let date = context.get_date_at_time(1.0);
        assert_eq!(
            date,
            TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 2).unwrap())
        );
    }

    #[test]
    fn test_get_current_date() {
        let mut context = Context::new();
        let params = ParameterValues {
            initialization: InfectionInitialization {
                start_date: TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
                initial_cases: NovelInfectionSource::None,
            },
            ..Default::default()
        };
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context.add_plan(9.0, |context| {
            let current_date = context.get_current_date();
            assert_eq!(
                current_date,
                TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap())
            );
        });
        context.execute();
    }

    #[test]
    fn test_add_plan_from_timekeeper() {
        let mut context = Context::new();
        let params = ParameterValues {
            initialization: InfectionInitialization {
                start_date: TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
                initial_cases: NovelInfectionSource::None,
            },
            ..Default::default()
        };
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        let timekeeper = TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap());
        context.add_plan_from_timekeeper(timekeeper, |context| {
            let current_date = context.get_current_date();
            let current_time = context.get_current_time();
            assert_almost_eq!(current_time, 9.0, 0.0);
            assert_eq!(
                current_date,
                TimeKeeper(NaiveDate::from_ymd_opt(2025, 1, 10).unwrap())
            );
        });
        context.execute();
    }
}
