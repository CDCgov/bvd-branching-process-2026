use crate::parameters::{ContextParametersExt, ParameterValues};
use crate::Probability;
use crate::{Person, PersonId};
use anyhow::{ensure, Result};
use ixa::{define_property, define_rng, prelude::*, Context, PluginContext};
use ordered_float::OrderedFloat;

define_rng!(DiseaseRng);

define_property!(
    enum InfectionStatus {
        Susceptible,
        Presymptomatic,
        Symptomatic,
        Removed,
        Vaccinated,
    },
    Person,
    default_const = InfectionStatus::Presymptomatic
);

define_property!(
    struct SymptomOnsetTime(pub Option<OrderedFloat<f64>>),
    Person,
    default_const = SymptomOnsetTime(None)
);

define_property!(
    struct Alive(pub bool),
    Person,
    default_const = Alive(true)
);

struct CumulativeCaseData {
    cumulative_cases: usize,
}

define_data_plugin!(
    CumulativeCasePlugin,
    CumulativeCaseData,
    CumulativeCaseData {
        cumulative_cases: 0
    }
);

pub trait DiseaseManagerExt:
    PluginContext + ContextEntitiesExt + ContextParametersExt + ContextRandomExt
{
    fn discretize_generation_interval(&mut self) -> (f64, f64) {
        let &ParameterValues {
            generation_interval_distribution,
            presymptomatic_transmission_probability,
            ..
        } = self.get_params();

        let exposure_to_symptom_onset_delay =
            generation_interval_distribution.quantile(presymptomatic_transmission_probability);
        let exposure_to_max_infection_attempt_time =
            generation_interval_distribution.quantile(Probability::ONE);
        (
            exposure_to_symptom_onset_delay,
            exposure_to_max_infection_attempt_time,
        )
    }

    fn check_mortality(&mut self, person_id: PersonId) -> bool {
        let &ParameterValues {
            case_fatality_ratio,
            mortality_delay_distribution,
            ..
        } = self.get_params();

        if self.sample_bool(DiseaseRng, case_fatality_ratio) {
            let time_of_death = self.sample_distr(DiseaseRng, mortality_delay_distribution);
            self.add_plan(time_of_death + self.get_current_time(), move |context| {
                context.set_property(person_id, Alive(false))
            });
            return true;
        }
        false
    }

    fn schedule_recovery(&mut self, person_id: PersonId) {
        let &ParameterValues {
            recovery_delay_distribution,
            ..
        } = self.get_params();
        let recovery_time = self.sample_distr(DiseaseRng, recovery_delay_distribution);
        self.add_plan(recovery_time + self.get_current_time(), move |context| {
            context.set_property(person_id, InfectionStatus::Removed);
        });
    }

    fn plan_symptom_resolution(&mut self, person_id: PersonId, infectious_period_max_time: f64) {
        if !self.check_mortality(person_id) {
            self.schedule_recovery(person_id);
        } else {
            // If a case is going to die, use this as place-holder of removal time until a burial delay module is added.
            self.add_plan(infectious_period_max_time, move |context| {
                context.set_property(person_id, InfectionStatus::Removed);
            });
        }
    }

    fn make_health_plan(&mut self, person_id: PersonId) -> Result<()> {
        let (exposure_to_symptom_onset_delay, exposure_to_max_infection_attempt_time) =
            self.discretize_generation_interval();
        let latent_period_end = self.get_current_time() + exposure_to_symptom_onset_delay;
        let infectious_period_end =
            self.get_current_time() + exposure_to_max_infection_attempt_time;
        ensure!(
            latent_period_end <= infectious_period_end,
            "Latent period end must be before or equal to infectious period end"
        );

        let max_time = self.get_params().end_run_conditions.max_time.into_inner();

        // Manually avoid scheduling removal at infinite time
        let infectious_period_max_time = infectious_period_end.min(max_time + 1.0);

        let infection_status = self.get_property::<Person, InfectionStatus>(person_id);
        if infection_status == InfectionStatus::Presymptomatic {
            self.add_plan(latent_period_end, move |context| {
                context.set_property(person_id, InfectionStatus::Symptomatic);
                context.set_property(
                    person_id,
                    SymptomOnsetTime(Some(OrderedFloat(latent_period_end))),
                );
                context.plan_symptom_resolution(person_id, infectious_period_max_time);
            });
        }
        Ok(())
    }

    fn get_cumulative_cases(&self) -> usize {
        self.get_data(CumulativeCasePlugin).cumulative_cases
    }
}
impl DiseaseManagerExt for Context {}

pub fn init(context: &mut Context) {
    // context.index_property::<Person, InfectionStatus>();
    // Index Alive so the Deaths trigger's query_entity_count(Alive(false)) hits ixa's
    // O(1) indexed fast path (bucket count) instead of an O(N) population scan per death.
    context.index_property::<Person, Alive>();
    context.subscribe_to_event(|context, event: EntityCreatedEvent<Person>| {
        context.make_health_plan(event.entity_id).unwrap();
    });
    context.subscribe_to_event(
        |context, event: PropertyChangeEvent<Person, InfectionStatus>| {
            if event.current == InfectionStatus::Symptomatic
                && event.previous == InfectionStatus::Presymptomatic
            {
                let case_data = context.get_data_mut(CumulativeCasePlugin);
                case_data.cumulative_cases += 1;
            }
        },
    );
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        disease_manager::InfectionStatus,
        distributions::ContinuousDistributionParameterized,
        parameters::{ParameterValues, Parameters},
        shutdown::EndRunConditions,
        Probability,
    };
    use ixa::assert_almost_eq;
    use ixa::{Context, ContextGlobalPropertiesExt, ContextRandomExt};

    fn setup(params: ParameterValues) -> Context {
        // Sets up parameters for context, filling out defaults
        // Defaults are set in the parameters module or are implemented for each Type automatically in Rust
        // Floats and integers are set to 0 and Options are set to None, unless specified.

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

    fn offset_weibull(shape: f64, scale: f64, offset: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::offset_weibull(shape, scale, offset)
            .expect("valid offset Weibull parameters")
    }

    fn uniform(min: f64, max: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::uniform(min, max).expect("valid uniform parameters")
    }

    fn probability(value: f64) -> Probability {
        Probability::try_from(value).expect("valid probability")
    }

    #[test]
    fn test_recovery() {
        let mut context = setup(ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.0),
            recovery_delay_distribution: fixed(1.0),
            ..Default::default()
        });
        context.init_random(0);
        let p: PersonId = context.add_entity(Person).unwrap();
        assert_eq!(
            context.get_property::<Person, InfectionStatus>(p),
            InfectionStatus::Presymptomatic
        );
        context.add_plan(0.0, move |context| context.schedule_recovery(p));
        context.execute();
        assert_eq!(
            context.get_property::<Person, InfectionStatus>(p),
            InfectionStatus::Removed
        );
    }

    #[test]
    fn test_mortality() {
        let mut death_count = 0;
        let n = 1000;
        let case_fatality_ratio = 0.75;
        for seed in 0..n {
            let mut context = setup(ParameterValues {
                end_run_conditions: EndRunConditions::from_max_time(2.0),
                case_fatality_ratio: probability(case_fatality_ratio),
                mortality_delay_distribution: fixed(1.0),
                ..Default::default()
            });
            context.init_random(seed);
            let p: PersonId = context.add_entity(Person).unwrap();
            assert!(context.get_property::<Person, Alive>(p).0);
            context.add_plan(0.0, move |context| {
                context.check_mortality(p);
            });
            context.execute();
            if !context.get_property::<Person, Alive>(p).0 {
                death_count += 1;
            }
        }

        let obs = death_count as f64 / n as f64;

        assert_almost_eq!(obs, case_fatality_ratio, 0.05);
    }

    fn assert_status_progression(recover: bool, delay: f64, latent_period: f64) {
        let mut context = setup(ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(100.0),
            recovery_delay_distribution: fixed(delay),
            mortality_delay_distribution: fixed(delay),
            generation_interval_distribution: offset_weibull(1.0, 1.0, latent_period),
            case_fatality_ratio: probability(if recover { 0.0 } else { 1.0 }),
            ..Default::default()
        });
        context.init_random(0);
        let p: PersonId = context.add_entity(Person).unwrap();
        context.make_health_plan(p).unwrap();
        assert_eq!(context.get_property::<Person, SymptomOnsetTime>(p).0, None);
        context.subscribe_to_event(
            |_context, event: PropertyChangeEvent<Person, InfectionStatus>| match event.current {
                InfectionStatus::Presymptomatic => {
                    panic!("Person should start as exposed, not transition to exposed")
                }
                InfectionStatus::Symptomatic => {
                    assert_eq!(event.previous, InfectionStatus::Presymptomatic)
                }
                InfectionStatus::Removed => {
                    assert_eq!(event.previous, InfectionStatus::Symptomatic)
                }
                _ => panic!("Unexpected status change"),
            },
        );
        context.subscribe_to_event(|context, event: PropertyChangeEvent<Person, Alive>| {
            if !event.current.0 {
                assert_eq!(
                    context.get_property::<Person, InfectionStatus>(event.entity_id),
                    InfectionStatus::Symptomatic
                );
            }
        });
        context.execute();
        assert_eq!(context.get_property::<Person, Alive>(p).0, recover);
        assert!(context.get_property::<Person, InfectionStatus>(p) == InfectionStatus::Removed);
        assert_eq!(
            context.get_property::<Person, SymptomOnsetTime>(p).0,
            Some(OrderedFloat(latent_period))
        );
    }

    #[test]
    fn test_status_order_progression() {
        // Test that recovery occurs in the correct order with respect to symptom onset
        assert_status_progression(true, 0.0, 2.0);
        assert_status_progression(true, 1.0, 2.0);
        assert_status_progression(true, 3.0, 2.0);

        // Test that death occurs in the correct order with respect to symptom onset
        assert_status_progression(false, 0.0, 2.0);
        assert_status_progression(false, 1.0, 2.0);
        assert_status_progression(false, 3.0, 2.0);
    }

    #[test]
    fn test_health_plan() {
        // Set up a fixed disease progression with
        // - infectiousness starting at time 0.0 and ending at time 3.0
        // - if mortality occurs, it occurs at time 1.0
        // - else recovery occurs at time 2.0
        let mut death_count = 0;
        let n = 1000;
        let case_fatality_ratio = 0.75;
        let symptom_onset_time = 0.5;
        let dead_removal_time = 3.0;
        let recovery_delay = 2.0;
        let death_delay = 1.0;
        for seed in 0..n {
            let mut context = setup(ParameterValues {
                end_run_conditions: EndRunConditions::from_max_time(4.0),
                case_fatality_ratio: probability(case_fatality_ratio),
                mortality_delay_distribution: fixed(death_delay),
                recovery_delay_distribution: fixed(recovery_delay),
                generation_interval_distribution: uniform(symptom_onset_time, dead_removal_time),
                ..Default::default()
            });
            context.init_random(seed);
            let p: PersonId = context.add_entity(Person).unwrap();
            assert!(context.get_property::<Person, Alive>(p).0);

            context.add_plan(0.0, move |context| {
                context.make_health_plan(p).unwrap();
            });

            // Check that removed time is correct for each case of alive and dead
            context.subscribe_to_event(
                move |context, event: PropertyChangeEvent<Person, InfectionStatus>| {
                    let alive = context.get_property::<Person, Alive>(event.entity_id).0;
                    match event.current {
                        InfectionStatus::Removed => {
                            if alive {
                                assert_almost_eq!(
                                    context.get_current_time(),
                                    recovery_delay + symptom_onset_time,
                                    0.0
                                );
                            } else {
                                assert_almost_eq!(
                                    context.get_current_time(),
                                    dead_removal_time,
                                    0.0
                                );
                            }
                        }
                        InfectionStatus::Symptomatic => {
                            assert!(alive);
                            assert_almost_eq!(context.get_current_time(), symptom_onset_time, 0.0);
                        }
                        _ => panic!("Unexpected status change"),
                    }
                },
            );

            // Check that a person dies at the correct time if they are going to die
            context.subscribe_to_event(
                move |context, event: PropertyChangeEvent<Person, Alive>| {
                    if !event.current.0 {
                        assert_almost_eq!(
                            context.get_current_time(),
                            death_delay + symptom_onset_time,
                            0.0
                        );
                    }
                },
            );

            context.execute();

            if !context.get_property::<Person, Alive>(p).0 {
                death_count += 1;
            }
        }

        let obs = death_count as f64 / n as f64;

        assert_almost_eq!(obs, case_fatality_ratio, 0.05);
    }
}
