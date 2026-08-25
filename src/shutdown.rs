use crate::parameters::ContextParametersExt;
use crate::state_trigger::{ContextTriggerExt, StateTrigger};
use crate::{NonNegativeFinite, PositiveCount};
use anyhow::Result;
use ixa::{define_data_plugin, Context, ExecutionPhase, IxaEvent, PluginContext};
use serde::{Deserialize, Serialize};

#[derive(IxaEvent)]
pub struct ShutdownEvent {
    pub time: f64,
}

define_data_plugin!(ShutdownPlanned, bool, false);

#[derive(Deserialize)]
struct UncheckedEndRunConditions {
    max_time: f64,
    max_deaths: Option<usize>,
    max_cases: Option<usize>,
    max_detections: Option<usize>,
}

#[derive(Deserialize, Serialize, Clone, Copy, Eq, PartialEq)]
#[serde(try_from = "UncheckedEndRunConditions")]
pub struct EndRunConditions {
    pub max_time: NonNegativeFinite,
    pub max_deaths: Option<PositiveCount>,
    pub max_cases: Option<PositiveCount>,
    pub max_detections: Option<PositiveCount>,
}

impl Default for EndRunConditions {
    fn default() -> Self {
        EndRunConditions {
            max_time: NonNegativeFinite::try_from(f64::MAX)
                .expect("default max_time should be finite and non-negative"),
            max_deaths: None,
            max_cases: None,
            max_detections: None,
        }
    }
}

impl EndRunConditions {
    pub fn from_max_time(max_time: f64) -> Self {
        EndRunConditions {
            max_time: NonNegativeFinite::try_from_with_error_label(max_time, "max_time")
                .expect("max_time should be finite and non-negative"),
            max_deaths: None,
            max_cases: None,
            max_detections: None,
        }
    }
}

impl TryFrom<UncheckedEndRunConditions> for EndRunConditions {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedEndRunConditions) -> Result<Self> {
        Ok(Self {
            max_time: NonNegativeFinite::try_from_with_error_label(value.max_time, "max_time")?,
            max_deaths: value
                .max_deaths
                .map(|count| PositiveCount::try_from_with_error_label(count, "max_deaths"))
                .transpose()?,
            max_cases: value
                .max_cases
                .map(|count| PositiveCount::try_from_with_error_label(count, "max_cases"))
                .transpose()?,
            max_detections: value
                .max_detections
                .map(|count| PositiveCount::try_from_with_error_label(count, "max_detections"))
                .transpose()?,
        })
    }
}

fn get_state_triggers(conditions: EndRunConditions) -> Vec<StateTrigger> {
    let mut triggers = vec![];
    let time = conditions.max_time;
    triggers.push(StateTrigger::Time { time });

    if let Some(count) = conditions.max_deaths {
        triggers.push(StateTrigger::Deaths { count });
    }
    if let Some(count) = conditions.max_cases {
        triggers.push(StateTrigger::Cases { count });
    }
    if let Some(count) = conditions.max_detections {
        triggers.push(StateTrigger::Detection { count })
    }
    triggers
}

trait ContextShutdownTriggerExt: PluginContext + ContextTriggerExt + ContextParametersExt {
    fn register_shutdown_triggers(&mut self, end_run_conditions: EndRunConditions) -> Result<()> {
        let triggers = get_state_triggers(end_run_conditions);
        for trigger in triggers.into_iter() {
            self.register_triggered_event(trigger, |context| {
                context.careful_shutdown(context.get_current_time());
            });
        }
        Ok(())
    }
    fn careful_shutdown(&mut self, time: f64) {
        if *self.get_data(ShutdownPlanned) {
            return;
        }
        *self.get_data_mut(ShutdownPlanned) = true;
        self.add_plan_with_phase(
            time,
            move |context| context.emit_event(ShutdownEvent { time }),
            ExecutionPhase::First,
        );
        self.add_plan_with_phase(
            time,
            move |context| {
                context.shutdown();
            },
            ExecutionPhase::Last,
        );
    }
}
impl ContextShutdownTriggerExt for Context {}

pub fn plan_shutdown(context: &mut Context, end_run_conditions: EndRunConditions) -> Result<()> {
    context.register_shutdown_triggers(end_run_conditions)?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        detection_manager::{SurveillanceData, SurveillanceType},
        disease_manager::Alive,
        Person, PositiveCount,
    };
    use ixa::{assert_almost_eq, prelude::*};
    use ordered_float::OrderedFloat;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_shutdown_event_emitted_once_with_tied_deaths() {
        let mut context = Context::new();
        let end_run_conditions = EndRunConditions {
            max_time: NonNegativeFinite::try_from(100.0).unwrap(),
            max_deaths: Some(PositiveCount::try_from(2).unwrap()),
            max_cases: None,
            max_detections: None,
        };
        context
            .register_shutdown_triggers(end_run_conditions)
            .unwrap();

        let emitted = Rc::new(RefCell::new(0usize));
        let counter = emitted.clone();
        context.subscribe_to_event::<ShutdownEvent>(move |_context, _event| {
            *counter.borrow_mut() += 1;
        });

        // Three people all die at the same time with the threshold set to 2.
        let people: Vec<_> = (0..3)
            .map(|_| context.add_entity(Person).unwrap())
            .collect();
        context.add_plan(50.0, move |context| {
            for p in &people {
                context.set_property(*p, Alive(false));
            }
        });
        context.execute();

        assert_eq!(
            *emitted.borrow(),
            1,
            "ShutdownEvent should be emitted exactly once, even when multiple deaths tie at the threshold time"
        );
    }

    #[test]
    fn test_shutdown_event_emitted_once_with_tied_death_and_case() {
        let mut context = Context::new();
        let end_run_conditions = EndRunConditions {
            max_time: NonNegativeFinite::try_from(100.0).unwrap(),
            max_deaths: Some(PositiveCount::ONE),
            max_cases: Some(PositiveCount::ONE),
            max_detections: None,
        };
        context
            .register_shutdown_triggers(end_run_conditions)
            .unwrap();

        let emitted = Rc::new(RefCell::new(0usize));
        let counter = emitted.clone();
        context.subscribe_to_event::<ShutdownEvent>(move |_context, _event| {
            *counter.borrow_mut() += 1;
        });

        // One person gets detected and another dies at the same time with the threshold set to 1 for both.
        let person1 = context.add_entity(Person).unwrap();
        let person2 = context.add_entity(Person).unwrap();

        crate::disease_manager::init(&mut context);

        context.add_plan(50.0, move |context| {
            context.set_property(
                person1,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(50.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None,
                },
            );
            context.set_property(person2, Alive(false));
        });
        context.execute();

        assert_eq!(
            *emitted.borrow(),
            1,
            "ShutdownEvent should be emitted exactly once, even when a death and a case tie at the threshold time"
        );
    }

    #[test]
    fn test_model_shutdown_on_death() {
        let mut context = Context::new();
        let max_time = 100.0;
        let overflow_time = 50.0;

        let end_run_conditions = EndRunConditions {
            max_time: NonNegativeFinite::try_from(max_time).unwrap(),
            max_deaths: Some(PositiveCount::ONE),
            max_cases: None,
            max_detections: None,
        };

        context
            .register_shutdown_triggers(end_run_conditions)
            .unwrap();
        let person = context.add_entity(Person).unwrap();

        context.add_plan(overflow_time, move |context| {
            context.set_property(person, Alive(false));
        });
        context.execute();
        assert_almost_eq!(context.get_current_time(), overflow_time, 0.0);
    }

    #[test]
    fn test_model_shutdown_on_max_time() {
        let mut context = Context::new();
        let max_time = 50.0;
        let overflow_time = 100.0;

        let end_run_conditions = EndRunConditions {
            max_time: NonNegativeFinite::try_from(max_time).unwrap(),
            max_deaths: Some(PositiveCount::ONE),
            max_cases: None,
            max_detections: None,
        };

        context
            .register_shutdown_triggers(end_run_conditions)
            .unwrap();
        let person = context.add_entity(Person).unwrap();

        context.add_plan(overflow_time, move |context| {
            context.set_property(person, Alive(false));
        });
        context.execute();
        assert_almost_eq!(context.get_current_time(), max_time, 0.0);
    }

    #[test]
    fn test_model_shutdown_on_first_condition() {
        let mut context = Context::new();
        let max_time = 500.0;
        let case_overflow_time = 100.0;
        let death_overflow_time = 200.0;

        let end_run_conditions = EndRunConditions {
            max_time: NonNegativeFinite::try_from(max_time).unwrap(),
            max_deaths: Some(PositiveCount::ONE),
            max_cases: None,
            max_detections: Some(PositiveCount::ONE),
        };

        context
            .register_shutdown_triggers(end_run_conditions)
            .unwrap();
        let person = context.add_entity(Person).unwrap();

        context.add_plan(case_overflow_time, move |context| {
            context.set_property(
                person,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(case_overflow_time),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None,
                },
            );
        });
        context.add_plan(death_overflow_time, move |context| {
            context.set_property(person, Alive(false));
        });
        context.execute();

        assert_almost_eq!(context.get_current_time(), case_overflow_time, 0.0);
        assert!(context.get_property::<Person, Alive>(person).0);
    }
}
