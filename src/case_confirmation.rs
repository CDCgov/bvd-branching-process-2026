use crate::distributions::{ContinuousDistributionParameterized, Delay};
use crate::{
    clinical_manager::ContextClinicalExt,
    state_trigger::{ContextTriggerExt, StateTrigger},
};
use crate::{ContextParametersExt, ParameterValues, PersonId};
use crate::{NonNegativeFinite, PositiveFinite};
use anyhow::{bail, ensure, Context as _, Result};
use ixa::{define_data_plugin, prelude::*, Context, PluginContext};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::VecDeque;

#[derive(Serialize, Deserialize)]
struct TestingQueueContainer {
    pub samples: VecDeque<PersonId>,
    pub clearance_lag: Option<ContinuousDistributionParameterized>,
    pub scheduled: bool,
}

fn clearance_lag_from_capacity(
    capacity_per_day: f64,
) -> Result<Option<ContinuousDistributionParameterized>> {
    if capacity_per_day > 0.0 {
        let delay =
            Delay::try_from_with_error_label(1.0 / capacity_per_day, "testing results lag")?;
        Ok(Some(ContinuousDistributionParameterized::fixed_delay(
            delay,
        )))
    } else {
        Ok(None)
    }
}

define_data_plugin!(
    TestingQueuePlugin,
    TestingQueueContainer,
    TestingQueueContainer {
        samples: VecDeque::default(),
        clearance_lag: Some(ContinuousDistributionParameterized::fixed_delay(
            Delay::ZERO
        )),
        scheduled: false
    }
);

define_rng!(TestingDelayRng);
define_rng!(ResultsDelayRng);

#[derive(Deserialize)]
enum UncheckedTestingRate {
    Unlimited,
    DailyCapacity {
        results_per_day: f64,
    },
    StepDailyCapacity {
        initial_capacity: f64,
        updated_capacity: f64,
        trigger: StateTrigger,
    },
}

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Hash, Default)]
#[serde(try_from = "UncheckedTestingRate")]
pub enum TestingRate {
    #[default]
    Unlimited,
    DailyCapacity {
        results_per_day: PositiveFinite,
    },
    StepDailyCapacity {
        initial_capacity: NonNegativeFinite,
        updated_capacity: NonNegativeFinite,
        trigger: StateTrigger,
    },
}

impl TryFrom<UncheckedTestingRate> for TestingRate {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedTestingRate) -> Result<Self> {
        match value {
            UncheckedTestingRate::Unlimited => Ok(Self::Unlimited),
            UncheckedTestingRate::DailyCapacity { results_per_day } => Ok(Self::DailyCapacity {
                results_per_day: PositiveFinite::try_from_with_error_label(
                    results_per_day,
                    "results_per_day",
                )?,
            }),
            UncheckedTestingRate::StepDailyCapacity {
                initial_capacity,
                updated_capacity,
                trigger,
            } => {
                let initial_capacity = NonNegativeFinite::try_from_with_error_label(
                    initial_capacity,
                    "initial_capacity",
                )?;
                let updated_capacity = NonNegativeFinite::try_from_with_error_label(
                    updated_capacity,
                    "updated_capacity",
                )?;
                ensure!(
                    updated_capacity.into_inner() > 0.0 || initial_capacity.into_inner() > 0.0,
                    "At least one of initial_capacity or updated_capacity must be greater than 0"
                );
                ensure!(
                    updated_capacity != initial_capacity,
                    "updated_capacity must be different from initial_capacity. Otherwise, use constant DailyCapacity testing rate"
                );
                Ok(Self::StepDailyCapacity {
                    initial_capacity,
                    updated_capacity,
                    trigger,
                })
            }
        }
    }
}

#[derive(Deserialize)]
struct UncheckedTestingConfig {
    deploy: bool,
    sample_collection_delay: Option<ContinuousDistributionParameterized>,
    testing_rate: Option<TestingRate>,
}

#[derive(Copy, Clone, Default)]
pub enum TestingConfig {
    #[default]
    Disabled,
    Enabled {
        sample_collection_delay: ContinuousDistributionParameterized,
        testing_rate: TestingRate,
    },
}

impl TestingConfig {
    pub fn enabled(
        sample_collection_delay: ContinuousDistributionParameterized,
        testing_rate: TestingRate,
    ) -> Self {
        Self::Enabled {
            sample_collection_delay,
            testing_rate,
        }
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    fn sample_collection_delay(&self) -> Option<ContinuousDistributionParameterized> {
        match self {
            Self::Enabled {
                sample_collection_delay,
                ..
            } => Some(*sample_collection_delay),
            Self::Disabled => None,
        }
    }
}

impl TryFrom<UncheckedTestingConfig> for TestingConfig {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedTestingConfig) -> Result<Self> {
        if value.deploy {
            Ok(Self::Enabled {
                sample_collection_delay: value.sample_collection_delay.with_context(|| {
                    "sample_collection_delay must be provided if deploy is true"
                })?,
                testing_rate: value
                    .testing_rate
                    .with_context(|| "testing_rate must be provided if deploy is true")?,
            })
        } else {
            Ok(Self::Disabled)
        }
    }
}

impl<'de> Deserialize<'de> for TestingConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = UncheckedTestingConfig::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for TestingConfig {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TestingConfig", 3)?;
        match self {
            TestingConfig::Disabled => {
                state.serialize_field("deploy", &false)?;
            }
            TestingConfig::Enabled {
                sample_collection_delay,
                testing_rate,
            } => {
                state.serialize_field("deploy", &true)?;
                state.serialize_field("sample_collection_delay", sample_collection_delay)?;
                state.serialize_field("testing_rate", testing_rate)?;
            }
        }
        state.end()
    }
}

pub trait ContextTestingQueueExt:
    PluginContext + ContextEntitiesExt + ContextParametersExt + ContextRandomExt + ContextTriggerExt
{
    fn add_to_testing_queue(&mut self, person_id: PersonId) {
        let container = self.get_data_mut(TestingQueuePlugin);
        let schedule_flag = container.samples.is_empty() && !container.scheduled;
        container.samples.push_back(person_id);
        if schedule_flag {
            self.schedule_test_results_for_queue();
        }
    }

    fn get_biological_sample_time(&self) -> Result<f64> {
        let &ParameterValues { testing_config, .. } = self.get_params();
        if let Some(sample_collection_delay) = testing_config.sample_collection_delay() {
            Ok(self.get_current_time()
                + self.sample_distr(TestingDelayRng, sample_collection_delay))
        } else {
            bail!("sample_collection_delay must be provided in parameters to get biological sample time")
        }
    }

    fn schedule_test_for_person(&mut self, person_id: PersonId) -> Result<()> {
        // When testing is disabled there is no confirmation-by-testing pathway, so a
        // suspected case is simply never confirmed (matching a testing delay past the run).
        if !self.get_params().testing_config.is_enabled() {
            return Ok(());
        }
        let biopsy_time = self.get_biological_sample_time()?;
        self.add_plan(biopsy_time, move |context| {
            context.add_to_testing_queue(person_id);
        });
        Ok(())
    }

    fn sample_results_time_lag(&self) -> Option<f64> {
        let container = self.get_data(TestingQueuePlugin);
        container
            .clearance_lag
            .map(|clearance_lag| self.sample_distr(ResultsDelayRng, clearance_lag))
    }

    fn schedule_test_results_for_queue(&mut self) {
        if let Some(results_time_lag) = self.sample_results_time_lag() {
            let results_time = results_time_lag + self.get_current_time();
            let container = self.get_data_mut(TestingQueuePlugin);
            container.scheduled = true;

            self.add_plan(results_time, move |context| {
                let container = context.get_data_mut(TestingQueuePlugin);
                if let Some(person_id) = container.samples.pop_front() {
                    context.confirm_case(person_id);
                    context.schedule_test_results_for_queue();
                } else {
                    container.scheduled = false;
                }
            });
        }
    }

    fn register_testing_rate(&mut self, testing_rate: TestingRate) -> Result<()> {
        // Set the lag distribution for generating test results after being added to the queue
        let clearance_lag = match testing_rate {
            TestingRate::Unlimited => Some(ContinuousDistributionParameterized::fixed_delay(
                Delay::ZERO,
            )),
            TestingRate::DailyCapacity { results_per_day } => {
                clearance_lag_from_capacity(results_per_day.into_inner())?
            }
            TestingRate::StepDailyCapacity {
                initial_capacity, ..
            } => clearance_lag_from_capacity(initial_capacity.into_inner())?,
        };
        self.get_data_mut(TestingQueuePlugin).clearance_lag = clearance_lag;

        // Trigger the switch from the initial testing capacity to the updated capacity, if applicable
        if let TestingRate::StepDailyCapacity {
            updated_capacity,
            trigger,
            initial_capacity,
        } = testing_rate
        {
            let updated_clearance_lag = clearance_lag_from_capacity(updated_capacity.into_inner())?;
            let should_restart_queue = initial_capacity.into_inner() == 0.0;
            self.register_triggered_event(trigger, move |context| {
                let container = context.get_data_mut(TestingQueuePlugin);

                container.clearance_lag = updated_clearance_lag;

                if should_restart_queue {
                    context.schedule_test_results_for_queue();
                }
            });
        }
        Ok(())
    }
}
impl ContextTestingQueueExt for Context {}

pub fn init(context: &mut Context) -> Result<()> {
    let &ParameterValues { testing_config, .. } = context.get_params();

    if let TestingConfig::Enabled { testing_rate, .. } = testing_config {
        context.register_testing_rate(testing_rate)?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::clinical_manager::{CaseStatus, CaseStatusValue};
    use crate::distributions::ContinuousDistributionParameterized;
    use crate::parameters::Parameters;
    use crate::NonNegativeFinite;
    use crate::Person;
    use ixa::assert_almost_eq;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn setup(params: ParameterValues) -> Context {
        let mut context = Context::new();
        context.init_random(params.seed);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context
    }

    fn fixed(delay: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::fixed(delay).expect("valid fixed delay")
    }

    fn positive(value: f64) -> PositiveFinite {
        PositiveFinite::try_from(value).expect("valid positive finite value")
    }

    fn non_negative(value: f64) -> NonNegativeFinite {
        NonNegativeFinite::try_from(value).expect("valid non-negative finite value")
    }

    #[test]
    fn test_default_testing_rate() {
        let default_rate = TestingRate::default();
        assert_eq!(default_rate, TestingRate::Unlimited);
    }

    #[test]
    fn test_validate_testing_rate() {
        let valid_rates = vec![
            TestingRate::Unlimited,
            TestingRate::DailyCapacity {
                results_per_day: positive(1.0),
            },
            TestingRate::StepDailyCapacity {
                initial_capacity: non_negative(1.0),
                updated_capacity: non_negative(2.0),
                trigger: StateTrigger::Time {
                    time: NonNegativeFinite::try_from(10.0).unwrap(),
                },
            },
            TestingRate::StepDailyCapacity {
                initial_capacity: non_negative(0.0),
                updated_capacity: non_negative(2.0),
                trigger: StateTrigger::Time {
                    time: NonNegativeFinite::try_from(10.0).unwrap(),
                },
            },
            TestingRate::StepDailyCapacity {
                initial_capacity: non_negative(2.0),
                updated_capacity: non_negative(0.0),
                trigger: StateTrigger::Time {
                    time: NonNegativeFinite::try_from(10.0).unwrap(),
                },
            },
        ];

        for rate in valid_rates {
            assert!(matches!(
                serde_json::to_value(rate),
                Ok(serde_json::Value::String(_)) | Ok(serde_json::Value::Object(_))
            ));
        }
    }

    #[test]
    fn test_invalid_daily_capacity() {
        let err =
            serde_json::from_str::<TestingRate>(r#"{"DailyCapacity":{"results_per_day":-1.0}}"#)
                .unwrap_err();
        assert!(err.to_string().contains("results_per_day"));
    }

    #[test]
    fn test_no_zero_daily_capacity() {
        let err =
            serde_json::from_str::<TestingRate>(r#"{"DailyCapacity":{"results_per_day":0.0}}"#)
                .unwrap_err();
        assert!(err.to_string().contains("results_per_day"));
    }

    #[test]
    fn test_invalid_step_daily_capacity_negative_initial() {
        let err = serde_json::from_str::<TestingRate>(
            r#"{"StepDailyCapacity":{"initial_capacity":-1.0,"updated_capacity":2.0,"trigger":{"Time":{"time":10.0}}}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("initial_capacity"));
    }

    #[test]
    fn test_invalid_step_daily_equal_capacity() {
        let err = serde_json::from_str::<TestingRate>(
            r#"{"StepDailyCapacity":{"initial_capacity":1.0,"updated_capacity":1.0,"trigger":{"Time":{"time":10.0}}}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("updated_capacity"));
    }

    #[test]
    fn test_invalid_step_daily_zero_updated_capacity() {
        let err = serde_json::from_str::<TestingRate>(
            r#"{"StepDailyCapacity":{"initial_capacity":0.0,"updated_capacity":0.0,"trigger":{"Time":{"time":10.0}}}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("At least one"));
    }

    #[test]
    fn test_add_to_queue() {
        let mut context = setup(ParameterValues {
            ..Default::default()
        });
        let p1 = context.add_entity(Person).unwrap();
        let p2 = context.add_entity(Person).unwrap();

        context.add_to_testing_queue(p1);
        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 1);
        assert_eq!(container.samples[0], p1);

        context.add_to_testing_queue(p2);
        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 2);
        assert_eq!(container.samples[1], p2);
        assert_eq!(container.samples[0], p1);
    }

    #[test]
    fn test_add_to_queue_and_schedule() {
        let mut context = setup(ParameterValues {
            ..Default::default()
        });
        let p1 = context.add_entity(Person).unwrap();
        let p2 = context.add_entity(Person).unwrap();

        context.add_to_testing_queue(p1);
        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 1);
        assert_eq!(container.samples[0], p1);

        context.add_to_testing_queue(p2);
        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 2);
        assert_eq!(container.samples[1], p2);
        assert_eq!(container.samples[0], p1);

        context.execute();
        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 0);
        assert!(!container.scheduled);
        assert_eq!(
            context.query_entity_count(with!(Person, CaseStatus(Some(CaseStatusValue::Confirmed)))),
            2
        );
    }

    #[test]
    fn test_schedule_sample_only_for_person() -> Result<()> {
        let mut context = setup(ParameterValues {
            testing_config: TestingConfig::enabled(fixed(1.0), TestingRate::Unlimited),
            ..Default::default()
        });
        let p1 = context.add_entity(Person).unwrap();
        // Block scheduling from occuring by setting scheduled to true
        let container = context.get_data_mut(TestingQueuePlugin);
        container.scheduled = true;

        context.schedule_test_for_person(p1)?;

        context.execute();

        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 1);
        assert_eq!(context.get_current_time(), 1.0);
        Ok(())
    }

    #[test]
    fn test_schedule_test_results() -> Result<()> {
        let mut context = setup(ParameterValues {
            testing_config: TestingConfig::enabled(fixed(1.0), TestingRate::Unlimited),
            ..Default::default()
        });
        crate::case_confirmation::init(&mut context)?;
        for _ in 0..5 {
            let p = context.add_entity(Person).unwrap();
            context.schedule_test_for_person(p)?;
        }
        context.execute();

        let container = context.get_data(TestingQueuePlugin);
        assert_eq!(container.samples.len(), 0);
        assert!(!container.scheduled);
        assert_eq!(
            context.query_entity_count(with!(Person, CaseStatus(Some(CaseStatusValue::Confirmed)))),
            5
        );
        assert_eq!(context.get_current_time(), 1.0);
        Ok(())
    }

    #[test]
    fn test_schedule_results_with_fixed_capacity() -> Result<()> {
        let results_per_day = 1.5;
        let testing_delay = 0.5;
        let n_people = 5;
        let mut context = setup(ParameterValues {
            testing_config: TestingConfig::enabled(
                fixed(testing_delay),
                TestingRate::DailyCapacity {
                    results_per_day: positive(results_per_day),
                },
            ),
            ..Default::default()
        });
        crate::case_confirmation::init(&mut context)?;
        for _ in 0..n_people {
            let p = context.add_entity(Person).unwrap();
            context.schedule_test_for_person(p)?;
        }

        let emitted = Rc::new(RefCell::new(n_people));
        let samples_queued = emitted.clone();

        context.subscribe_to_event(
            move |context, event: PropertyChangeEvent<Person, CaseStatus>| {
                if let CaseStatus(Some(CaseStatusValue::Confirmed)) = event.current {
                    let container = context.get_data(TestingQueuePlugin);
                    assert_eq!(*samples_queued.borrow() - 1, container.samples.len());
                    *samples_queued.borrow_mut() -= 1;
                }
            },
        );

        context.execute();

        let container = context.get_data(TestingQueuePlugin);

        // Current time should be the number of calls t othe sampling queue (n_people + 1 for the last empty queue check) multiplied by the time to receive each result, plus the initial testing delay
        let results_time_lag = 1.0 / results_per_day;
        assert_almost_eq!(
            context.get_current_time(),
            (n_people + 1) as f64 * results_time_lag + testing_delay,
            1e-12
        );
        assert_eq!(container.samples.len(), 0);
        assert!(!container.scheduled);
        assert_eq!(
            context.query_entity_count(with!(Person, CaseStatus(Some(CaseStatusValue::Confirmed)))),
            n_people
        );
        Ok(())
    }

    #[test]
    fn test_schedule_results_with_step_capacity() -> Result<()> {
        let initial_results_per_day = 0.0;
        let updated_results_per_day = 1.5;
        let testing_delay = 0.5;
        let trigger_time = 5.0;
        let n_people = 5;

        let mut context = setup(ParameterValues {
            testing_config: TestingConfig::enabled(
                fixed(testing_delay),
                TestingRate::StepDailyCapacity {
                    initial_capacity: non_negative(initial_results_per_day),
                    updated_capacity: non_negative(updated_results_per_day),
                    trigger: StateTrigger::Time {
                        time: NonNegativeFinite::try_from(trigger_time)?,
                    },
                },
            ),
            ..Default::default()
        });
        crate::case_confirmation::init(&mut context)?;

        for _ in 0..n_people {
            let p = context.add_entity(Person).unwrap();
            context.schedule_test_for_person(p)?;
        }

        let emitted = Rc::new(RefCell::new(n_people));
        let samples_queued = emitted.clone();

        context.subscribe_to_event(
            move |context, event: PropertyChangeEvent<Person, CaseStatus>| {
                if let CaseStatus(Some(CaseStatusValue::Confirmed)) = event.current {
                    let container = context.get_data(TestingQueuePlugin);
                    assert_eq!(*samples_queued.borrow() - 1, container.samples.len());
                    *samples_queued.borrow_mut() -= 1;
                }
            },
        );

        context.execute();

        let container = context.get_data(TestingQueuePlugin);

        // Current time should be the trigger time plus the number of calls to the sampling queue after the trigger (n_people + 1 for the last empty queue check) multiplied by the time to receive each result
        // The testing delay to get biological samples is less than the trigger time, so that delay doens't factor into total time
        assert!(testing_delay < trigger_time);
        let results_time_lag = 1.0 / updated_results_per_day;
        assert_almost_eq!(
            context.get_current_time(),
            trigger_time + (n_people + 1) as f64 * results_time_lag,
            1e-12
        );
        assert_eq!(container.samples.len(), 0);
        assert!(!container.scheduled);
        assert_eq!(
            context.query_entity_count(with!(Person, CaseStatus(Some(CaseStatusValue::Confirmed)))),
            n_people
        );
        Ok(())
    }
}
