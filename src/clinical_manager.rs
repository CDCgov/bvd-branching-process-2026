use crate::case_confirmation::ContextTestingQueueExt;
use crate::disease_manager::InfectionStatus;
use crate::parameters::{ContextParametersExt, ParameterValues};
use crate::{Person, PersonId, Probability};
use anyhow::{bail, Result};
use ixa::{define_property, define_rng, prelude::*, Context, PluginContext};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HealthSetting {
    Quarantine,
    Clinic,
    EbolaTreatmentUnit,
}

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum CaseStatusValue {
    Suspected,
    Confirmed,
}

define_property!(
    struct CaseStatus(pub Option<CaseStatusValue>),
    Person,
    default_const = CaseStatus(None)
);

define_property!(
    struct TreatmentLocation(pub Option<HealthSetting>),
    Person,
    default_const = TreatmentLocation(None)
);

define_rng!(ClinicalRng);

pub fn evaluate_transmission_event(context: &Context, person_id: PersonId) -> bool {
    let p = context.get_current_transmission_prob_for_person(person_id);
    context.sample_bool(ClinicalRng, p)
}

pub trait ContextClinicalExt:
    PluginContext + ContextRandomExt + ContextEntitiesExt + ContextParametersExt
{
    fn attempt_hospitalization_plan(&mut self, person_id: PersonId) -> Result<()> {
        let &ParameterValues {
            hospitalization_probability,
            hospitalization_delay_distribution,
            ..
        } = self.get_params();

        let current = self.get_property::<Person, TreatmentLocation>(person_id).0;
        if current.is_some() {
            bail!(
                "{person_id:?} was transferred to {current:?} prior to scheduling hospitalization"
            );
        }

        if self.sample_bool(ClinicalRng, hospitalization_probability) {
            let report_time = self.get_current_time()
                + self.sample_distr(ClinicalRng, hospitalization_delay_distribution);
            self.add_plan(report_time, move |context| {
                // Hospitalize if the case is still outside a treatment facility and has unassigned case status
                if context
                    .get_property::<Person, TreatmentLocation>(person_id)
                    .0
                    .is_none()
                    && context
                        .get_property::<Person, CaseStatus>(person_id)
                        .0
                        .is_none()
                {
                    context.set_property(person_id, TreatmentLocation(Some(HealthSetting::Clinic)));
                    context.set_property(person_id, CaseStatus(Some(CaseStatusValue::Suspected)));
                }
            });
        }

        Ok(())
    }

    fn confirm_case(&mut self, person_id: PersonId) {
        let status = self.get_property::<Person, InfectionStatus>(person_id);
        match status {
            // False negatives are not implemented
            InfectionStatus::Presymptomatic
            | InfectionStatus::Symptomatic
            | InfectionStatus::Removed => {
                if self.get_property::<Person, CaseStatus>(person_id).0
                    != Some(CaseStatusValue::Confirmed)
                {
                    self.set_property(person_id, CaseStatus(Some(CaseStatusValue::Confirmed)));
                }
            }
            // False positives are not implemented
            InfectionStatus::Susceptible | InfectionStatus::Vaccinated => (),
        }
    }

    fn transfer_case_to_etu(&mut self, person_id: PersonId) {
        let &ParameterValues {
            etu_transfer_delay, ..
        } = self.get_params();

        self.add_plan(
            self.get_current_time() + etu_transfer_delay,
            move |context| {
                context.set_property(
                    person_id,
                    TreatmentLocation(Some(HealthSetting::EbolaTreatmentUnit)),
                );
            },
        );
    }

    fn get_current_transmission_prob_for_person(&self, person_id: PersonId) -> Probability {
        let &ParameterValues {
            quarantine_transmission_probability,
            clinical_transmission_probability,
            etu_transmission_probability,
            ..
        } = self.get_params();

        match self.get_property::<Person, TreatmentLocation>(person_id).0 {
            None => Probability::ONE,
            Some(HealthSetting::Quarantine) => quarantine_transmission_probability,
            Some(HealthSetting::Clinic) => clinical_transmission_probability,
            Some(HealthSetting::EbolaTreatmentUnit) => etu_transmission_probability,
        }
    }
}
impl ContextClinicalExt for Context {}

pub fn init(context: &mut Context) {
    context.subscribe_to_event(|context, event: PropertyChangeEvent<Person, InfectionStatus>| {
        match event.current {
            InfectionStatus::Symptomatic => {
                match context.get_property::<Person, TreatmentLocation>(event.entity_id).0 {
                    None => context.attempt_hospitalization_plan(event.entity_id).unwrap(),
                    Some(HealthSetting::Quarantine) => context.set_property(event.entity_id, CaseStatus(Some(CaseStatusValue::Suspected))),
                    _ => panic!("Person {:?} became infectious while already in a symptomatic treatment setting {:?}", event.entity_id, context.get_property::<Person, TreatmentLocation>(event.entity_id).0)
                };
            },
            InfectionStatus::Removed => context.set_property(event.entity_id, TreatmentLocation(None)),
            _ => ()
        };
    });
    context.subscribe_to_event(|context, event: PropertyChangeEvent<Person, CaseStatus>| {
        match event.current.0 {
            Some(CaseStatusValue::Suspected) => {
                context.schedule_test_for_person(event.entity_id).unwrap()
            }
            Some(CaseStatusValue::Confirmed) => {
                // Upon confirmation and until ETU transfer is complete, person remains at their current location, which cannot be None
                match context.get_property::<Person, InfectionStatus>(event.entity_id) {
                    InfectionStatus::Symptomatic => {
                        assert!(context
                            .get_property::<Person, TreatmentLocation>(event.entity_id)
                            .0
                            .is_some());
                        context.transfer_case_to_etu(event.entity_id);
                    }
                    InfectionStatus::Removed => (),
                    _ => panic!(
                        "Person {:?} was confirmed without developing symptoms: {:?}",
                        event.entity_id,
                        context.get_property::<Person, InfectionStatus>(event.entity_id)
                    ),
                }
            }
            None => (),
        }
    });
}

#[cfg(test)]
mod test {
    use ixa::ContextGlobalPropertiesExt;

    use crate::case_confirmation::{TestingConfig, TestingRate};
    use crate::distributions::ContinuousDistributionParameterized;
    use crate::parameters::{ParameterValues, Parameters};
    use crate::shutdown::EndRunConditions;
    use crate::Probability;
    use ixa::assert_almost_eq;

    use super::*;

    fn setup(seed: u64, params: ParameterValues) -> Context {
        let mut context = Context::new();
        context.init_random(seed);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context
    }

    fn fixed(delay: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::fixed(delay).expect("valid fixed delay")
    }

    fn probability(value: f64) -> Probability {
        Probability::try_from(value).expect("valid probability")
    }

    #[test]
    fn test_init_attempt_hospitalization_plan() {
        let hospitalization_probability = Probability::ONE;
        let hosp_delay = 2.0;
        let test_delay = 2.0;
        let hospitalization_delay_distribution = fixed(hosp_delay);
        let testing_delay_distribution = fixed(test_delay);
        let etu_transfer_delay = 1.0;
        let mut context = setup(
            0,
            ParameterValues {
                end_run_conditions: EndRunConditions::from_max_time(6.0),
                hospitalization_probability,
                hospitalization_delay_distribution,
                testing_config: TestingConfig::enabled(
                    testing_delay_distribution,
                    TestingRate::Unlimited,
                ),
                etu_transfer_delay,
                ..Default::default()
            },
        );

        // subscribe to changes in case status and infection status
        init(&mut context);

        let quarantined = context
            .add_entity(with!(
                Person,
                TreatmentLocation(Some(HealthSetting::Quarantine))
            ))
            .unwrap();
        let community_member = context.add_entity(Person).unwrap();

        context.add_plan(0.0, move |context| {
            context.set_property(quarantined, InfectionStatus::Symptomatic);
            context.set_property(community_member, InfectionStatus::Symptomatic);
        });

        context.subscribe_to_event(move |context, event: PropertyChangeEvent<Person, TreatmentLocation>| {
            if event.entity_id == quarantined {
                match event.current.0 {
                    Some(HealthSetting::EbolaTreatmentUnit) => assert_almost_eq!(context.get_current_time(), test_delay + etu_transfer_delay, 0.0),
                    other => panic!("Hospitalization changes shouldn't occur in other settings like {:?} for the Quarantined individual", other)
                }
            }
            if event.entity_id == community_member {
                match event.current.0 {
                    Some(HealthSetting::Clinic) => assert_almost_eq!(context.get_current_time(), hosp_delay, 0.0),
                    Some(HealthSetting::EbolaTreatmentUnit) => assert_almost_eq!(context.get_current_time(), hosp_delay + test_delay + etu_transfer_delay, 0.0),
                    other => panic!("Hospitalization changes shouldn't occur in other settings like {:?} for the community member", other)
                }
            }
        });

        context.execute();

        assert_eq!(
            context
                .get_property::<Person, TreatmentLocation>(community_member)
                .0,
            Some(HealthSetting::EbolaTreatmentUnit)
        );
        assert_eq!(
            context
                .get_property::<Person, TreatmentLocation>(quarantined)
                .0,
            Some(HealthSetting::EbolaTreatmentUnit)
        );

        let e = context
            .attempt_hospitalization_plan(community_member)
            .unwrap_err();
        let msg = e.to_string();
        assert!(
            msg.contains("EbolaTreatmentUnit"),
            "Expected error mentioning EbolaTreatmentUnit, got: {msg}"
        );
    }

    #[test]
    fn test_evaluate_transmission_event() {
        let quarantine_transmission_probability = probability(0.25);
        let clinical_transmission_probability = probability(0.5);
        let etu_transmission_probability = probability(0.75);
        let n_replicates = 1000;
        let mut counts = [0, 0, 0, 0];
        let expected = [
            quarantine_transmission_probability,
            clinical_transmission_probability,
            etu_transmission_probability,
            Probability::ONE,
        ];
        for seed in 0..n_replicates {
            let mut context = setup(
                seed,
                ParameterValues {
                    quarantine_transmission_probability,
                    clinical_transmission_probability,
                    etu_transmission_probability,
                    ..Default::default()
                },
            );

            let quarantined = context
                .add_entity(with!(
                    Person,
                    TreatmentLocation(Some(HealthSetting::Quarantine))
                ))
                .unwrap();
            let hospitalized = context
                .add_entity(with!(
                    Person,
                    TreatmentLocation(Some(HealthSetting::Clinic))
                ))
                .unwrap();
            let treated = context
                .add_entity(with!(
                    Person,
                    TreatmentLocation(Some(HealthSetting::EbolaTreatmentUnit))
                ))
                .unwrap();
            let community_member = context.add_entity(Person).unwrap();

            if evaluate_transmission_event(&context, quarantined) {
                counts[0] += 1;
            }
            if evaluate_transmission_event(&context, hospitalized) {
                counts[1] += 1;
            }
            if evaluate_transmission_event(&context, treated) {
                counts[2] += 1;
            }
            if evaluate_transmission_event(&context, community_member) {
                counts[3] += 1;
            }
        }

        for i in 0..=3 {
            let observed = counts[i] as f64 / n_replicates as f64;
            assert_almost_eq!(observed, expected[i].into_inner(), 0.05);
        }
    }

    #[test]
    fn test_confirm_case() {
        let mut context = setup(
            0,
            ParameterValues {
                end_run_conditions: EndRunConditions::from_max_time(10.0),
                ..Default::default()
            },
        );

        let p = context
            .add_entity(with!(Person, InfectionStatus::Symptomatic))
            .unwrap();
        context.confirm_case(p);
        assert_eq!(
            context.get_property::<Person, CaseStatus>(p).0,
            Some(CaseStatusValue::Confirmed)
        );
    }

    #[test]
    fn test_confirm_case_no_effect_on_susceptible() {
        let mut context = setup(
            0,
            ParameterValues {
                end_run_conditions: EndRunConditions::from_max_time(10.0),
                ..Default::default()
            },
        );

        let p = context
            .add_entity(with!(Person, InfectionStatus::Susceptible))
            .unwrap();
        context.confirm_case(p);
        assert_eq!(context.get_property::<Person, CaseStatus>(p).0, None);
    }
}
