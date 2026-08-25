use ixa::{prelude::*, Context, PluginContext};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::disease_manager::DiseaseManagerExt;
use crate::Person;
use crate::{
    detection_manager::{DetectionStatus, SurveillanceCampaignStartEvent},
    disease_manager::{Alive, InfectionStatus},
    timekeeping::{ContextTimeKeeperExt, TimeKeeper},
};
use crate::{NonNegativeFinite, PositiveCount};

#[derive(Deserialize)]
enum UncheckedStateTrigger {
    Detection { count: usize },
    Deaths { count: usize },
    Cases { count: usize },
    Time { time: f64 },
    Date { date: TimeKeeper },
    SurveillanceCampaign,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UncheckedStateTrigger")]
pub enum StateTrigger {
    Detection {
        count: PositiveCount,
    },
    Deaths {
        count: PositiveCount,
    },
    Cases {
        count: PositiveCount,
    },
    Time {
        time: NonNegativeFinite,
    },
    Date {
        date: TimeKeeper,
    },
    /// Fires when the surveillance campaign becomes active.
    SurveillanceCampaign,
}

impl StateTrigger {
    pub fn detection(count: usize) -> Result<Self> {
        Ok(Self::Detection {
            count: PositiveCount::try_from_with_error_label(count, "Detection trigger count")?,
        })
    }

    pub fn deaths(count: usize) -> Result<Self> {
        Ok(Self::Deaths {
            count: PositiveCount::try_from_with_error_label(count, "Deaths trigger count")?,
        })
    }

    pub fn cases(count: usize) -> Result<Self> {
        Ok(Self::Cases {
            count: PositiveCount::try_from_with_error_label(count, "Cases trigger count")?,
        })
    }

    pub fn time(time: f64) -> Result<Self> {
        Ok(Self::Time {
            time: NonNegativeFinite::try_from_with_error_label(time, "State trigger time")?,
        })
    }

    pub fn date(date: TimeKeeper) -> Self {
        Self::Date { date }
    }
}

impl TryFrom<UncheckedStateTrigger> for StateTrigger {
    type Error = anyhow::Error;

    fn try_from(value: UncheckedStateTrigger) -> Result<Self> {
        match value {
            UncheckedStateTrigger::Detection { count } => Self::detection(count),
            UncheckedStateTrigger::Deaths { count } => Self::deaths(count),
            UncheckedStateTrigger::Cases { count } => Self::cases(count),
            UncheckedStateTrigger::Time { time } => Self::time(time),
            UncheckedStateTrigger::Date { date } => Ok(Self::date(date)),
            UncheckedStateTrigger::SurveillanceCampaign => Ok(Self::SurveillanceCampaign),
        }
    }
}

// Count-based triggers (Deaths/Cases) fire on `>=`, so the callback can run
// more than once. This flag keeps the callback idempotent so only one callback occurrence happens.
define_data_plugin!(TriggersFired, Vec<bool>, Vec::new());

pub trait ContextTriggerExt: PluginContext + ContextTimeKeeperExt {
    fn push_trigger(&mut self) -> usize {
        let fired = self.get_data_mut(TriggersFired);
        fired.push(false);
        fired.len() - 1
    }

    fn register_triggered_event(
        &mut self,
        trigger: StateTrigger,
        callback: impl Fn(&mut Context) + Send + Sync + 'static,
    ) {
        let id = self.push_trigger();
        let fire_once = move |context: &mut Context| {
            let fired = context.get_data_mut(TriggersFired);
            if !fired[id] {
                fired[id] = true;
                callback(context);
            }
        };
        match trigger {
            StateTrigger::Detection { count } => {
                self.subscribe_to_event(
                    move |context, event: PropertyChangeEvent<Person, DetectionStatus>| {
                        if event.current == DetectionStatus::Detected {
                            let detected_count = context
                                .query_entity_count(with!(Person, DetectionStatus::Detected));
                            if detected_count >= count.into_inner() {
                                fire_once(context);
                            }
                        }
                    },
                );
            }
            StateTrigger::Deaths { count } => {
                self.subscribe_to_event(
                    move |context, event: PropertyChangeEvent<Person, Alive>| {
                        if event.previous.0 && !event.current.0 {
                            let death_count =
                                context.query_entity_count(with!(Person, Alive(false)));
                            if death_count >= count.into_inner() {
                                fire_once(context);
                            }
                        }
                    },
                );
            }
            StateTrigger::Cases { count } => {
                self.subscribe_to_event(
                    move |context, event: PropertyChangeEvent<Person, InfectionStatus>| {
                        let case_count = context.get_cumulative_cases();
                        if event.current == InfectionStatus::Symptomatic
                            && case_count >= count.into_inner()
                        {
                            fire_once(context);
                        }
                    },
                );
            }
            StateTrigger::Time { time } => {
                self.add_plan(time.into_inner(), fire_once);
            }
            StateTrigger::Date { date } => {
                self.add_plan_from_timekeeper(date, fire_once);
            }
            StateTrigger::SurveillanceCampaign => {
                self.subscribe_to_event(move |context, _event: SurveillanceCampaignStartEvent| {
                    fire_once(context);
                });
            }
        }
    }
}
impl ContextTriggerExt for Context {}

#[cfg(test)]
mod test {
    use super::*;
    use ixa::IxaEvent;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(IxaEvent)]
    struct TestEvent;

    #[test]
    fn state_trigger_constructors_validate_raw_values() {
        assert!(matches!(
            StateTrigger::detection(1),
            Ok(StateTrigger::Detection { .. })
        ));
        assert!(matches!(
            StateTrigger::deaths(1),
            Ok(StateTrigger::Deaths { .. })
        ));
        assert!(matches!(
            StateTrigger::cases(1),
            Ok(StateTrigger::Cases { .. })
        ));
        assert!(matches!(
            StateTrigger::time(0.0),
            Ok(StateTrigger::Time { .. })
        ));

        assert!(StateTrigger::detection(0).is_err());
        assert!(StateTrigger::deaths(0).is_err());
        assert!(StateTrigger::cases(0).is_err());
        assert!(StateTrigger::time(-1.0).is_err());
    }

    #[test]
    fn test_trigger_event_emitted_once() {
        let mut context = Context::new();

        let emitted = Rc::new(RefCell::new(0usize));
        let counter = emitted.clone();

        context.register_triggered_event(StateTrigger::cases(2).unwrap(), |context| {
            let cases = context.get_cumulative_cases();
            assert_eq!(cases, 2);
            context.emit_event(TestEvent);
        });
        let fired = context.get_data(TriggersFired);
        assert_eq!(fired.len(), 1);
        assert!(!fired[0]);

        let person_0 = context.add_entity(Person).unwrap();
        let person_1 = context.add_entity(Person).unwrap();
        let person_2 = context.add_entity(Person).unwrap();

        crate::disease_manager::init(&mut context);

        context.add_plan(0.5, move |context| {
            context.set_property(person_0, InfectionStatus::Symptomatic);
            context.set_property(person_1, InfectionStatus::Symptomatic);
        });

        context.add_plan(1.0, move |context| {
            context.set_property(person_2, InfectionStatus::Symptomatic);
        });

        context.subscribe_to_event(move |_context, _event: TestEvent| {
            *counter.borrow_mut() += 1;
        });

        context.execute();
        let fired = context.get_data(TriggersFired);
        assert_eq!(fired.len(), 1);
        assert!(fired[0]);
        assert_eq!(*emitted.borrow(), 1);
    }
}
