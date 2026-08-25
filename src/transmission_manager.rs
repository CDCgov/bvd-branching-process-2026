use crate::clinical_manager::{
    evaluate_transmission_event, CaseStatus, CaseStatusValue, HealthSetting, TreatmentLocation,
};
use crate::detection_manager::{
    ContextDetectionExt, DetectionStatus, Generation, PrimaryInfection, SurveillanceData,
    TransmissionChain, TransmissionChainData,
};
use crate::disease_manager::InfectionStatus;
use crate::parameters::{ContextParametersExt, ParameterValues};
use crate::vaccination::{ContactInterventionData, ContextVaccineExt, VaccineData};
use crate::vaccination_campaign::VaccinationCampaignType;
use crate::{Person, PersonId};
use anyhow::{bail, Result};
use ixa::{define_data_plugin, define_rng, prelude::*, Context, PluginContext};
use ordered_float::OrderedFloat;

struct InterventionData {
    vaccine_cases_averted: usize,
    health_setting_cases_averted: usize,
}

define_data_plugin!(
    InterventionPlugin,
    InterventionData,
    InterventionData {
        vaccine_cases_averted: 0,
        health_setting_cases_averted: 0,
    }
);

define_rng!(InterventionRng);
define_rng!(ConfirmationSamplingRng);

pub fn infection_attempt(context: &mut Context, infector_id: PersonId) -> Result<()> {
    // Get infector data relevant to transmission data
    let infector_infection_status = context.get_property::<Person, InfectionStatus>(infector_id);

    match infector_infection_status {
        InfectionStatus::Symptomatic | InfectionStatus::Presymptomatic => {
            // Get intervention data from vaccine module
            let intervention_data = context.sample_contact_intervention_data(infector_id)?;
            if evaluate_transmission_event(context, infector_id) {
                // Successful transmission event, move on to evaluate contact intervention data
                context.handle_transmission_event(infector_id, intervention_data)
            } else {
                // Transmission event averted due to health setting
                context.handle_setting_averted_case(infector_id, intervention_data)
            }
        }
        // Other infection statuses do not lead to transmission
        _ => Ok(()),
    }
}

trait ContextTransmissionInternalExt:
    PluginContext
    + ContextEntitiesExt
    + ContextDetectionExt
    + ContextVaccineExt
    + ContextTransmissionExt
    + ContextParametersExt
{
    fn track_susceptibles(&self) -> bool {
        let &ParameterValues {
            track_susceptibles, ..
        } = self.get_params();
        track_susceptibles
    }
    /// Handle a transmission event by creating a new infectious contact or vaccinated person
    /// based on the intervention data provided.
    /// If the contact is detected in the future, a plan is created to update their surveillance data.
    /// If the contact has vaccine protection, they are created as vaccinated.
    /// Otherwise, they are created as an `Presymptomatic` contact.
    fn handle_transmission_event(
        &mut self,
        infector_id: PersonId,
        intervention_data: ContactInterventionData,
    ) -> Result<()> {
        let mut default_health_setting = None;

        match intervention_data.surveillance_data {
            SurveillanceData::Detected { detection_time, .. } => {
                if detection_time.into_inner() > self.get_current_time() {
                    // Create contact and modify with plan to detect in the future
                    self.make_new_contact_with_future(
                        detection_time.into_inner(),
                        infector_id,
                        intervention_data,
                    )?;
                    return Ok(());
                } else {
                    default_health_setting = Some(HealthSetting::Quarantine);
                }
            }
            SurveillanceData::Undetected { .. } => {}
        }

        // Create contact with surveillance data that is current: undetected case or detection in the past
        self.make_new_contact_from_data(infector_id, intervention_data, default_health_setting)?;
        Ok(())
    }

    /// Creates the contact with presymptomatic or vaccinated status and sets a plan to update their surveillance data at the future detection time.
    fn make_new_contact_with_future(
        &mut self,
        detection_time: f64,
        infector_id: PersonId,
        intervention_data: ContactInterventionData,
    ) -> Result<()> {
        let mut contact_infection_status = InfectionStatus::Presymptomatic;
        let mut infector_generation = self.get_property::<Person, Generation>(infector_id).0;

        // Assess possible geographic vaccination protection
        if self.get_vaccine_protection(intervention_data.vaccine_data) {
            self.avert_case();
            contact_infection_status = InfectionStatus::Vaccinated;
            infector_generation = None;

            if let VaccineData::Vaccinated {
                campaign_deploy_method,
                ..
            } = intervention_data.vaccine_data
            {
                if campaign_deploy_method == VaccinationCampaignType::Ring {
                    bail!("Ring vaccination should not lead to immunity prior to detection time");
                }
            }
        }

        // Add person with plan to implement surveillance data - vaccination will fail due to timing
        let contact = self
            .add_entity(with!(
                Person,
                TransmissionChain(Some(TransmissionChainData::new_with_infector(
                    self.get_current_time(),
                    infector_id,
                    infector_generation
                ))),
                intervention_data.vaccine_data,
                SurveillanceData::Undetected {
                    active_detection_attempt_time: Some(OrderedFloat(self.get_current_time())),
                    passive_detection_attempt_time: None,
                },
                contact_infection_status
            ))
            .unwrap();

        self.add_plan(detection_time, move |context| {
            context.set_property(contact, intervention_data.surveillance_data)
        });

        Ok(())
    }

    /// Handle the case where a contact is created due to a transmission event that was averted
    /// due to health setting interventions. The contact is always created as susceptible.
    fn handle_setting_averted_case(
        &mut self,
        infector_id: PersonId,
        intervention_data: ContactInterventionData,
    ) -> Result<()> {
        let container = self.get_data_mut(InterventionPlugin);
        container.health_setting_cases_averted += 1;

        if self.track_susceptibles() {
            self.create_contact(
                infector_id,
                intervention_data,
                InfectionStatus::Susceptible,
                None,
            )?;
        }
        Ok(())
    }

    /// Handle the case where a contact is created with up-to-date surveillance data,
    /// meaning the case is known to be undetected or a detection time occurred in the past.
    /// Vaccine protection is assessed to determine if the contact is created as vaccinated or presymptomatic.
    fn make_new_contact_from_data(
        &mut self,
        infector_id: PersonId,
        intervention_data: ContactInterventionData,
        health_setting: Option<HealthSetting>,
    ) -> Result<()> {
        if self.get_vaccine_protection(intervention_data.vaccine_data) {
            self.avert_case();
            self.create_contact(
                infector_id,
                intervention_data,
                InfectionStatus::Vaccinated,
                health_setting,
            )?;
            return Ok(());
        }

        self.create_contact(
            infector_id,
            intervention_data,
            InfectionStatus::Presymptomatic,
            health_setting,
        )?;
        Ok(())
    }

    /// Create a contact person with the specified initial infection status and health setting.
    /// The contact is linked to the infector via the transmission chain data.
    /// Generation is set based on the initial infection status, being a None if the individual is protected by vaccination.
    fn create_contact(
        &mut self,
        infector_id: PersonId,
        intervention_data: ContactInterventionData,
        initial_status: InfectionStatus,
        health_setting: Option<HealthSetting>,
    ) -> Result<()> {
        let transmission_chain_data = match initial_status {
            InfectionStatus::Presymptomatic => Some(TransmissionChainData::new_with_infector(
                self.get_current_time(),
                infector_id,
                self.get_property::<Person, Generation>(infector_id).0,
            )),
            InfectionStatus::Susceptible | InfectionStatus::Vaccinated => None,
            _ => bail!("Invalid initial status for contact creation"),
        };
        self.add_entity(with!(
            Person,
            TransmissionChain(transmission_chain_data),
            intervention_data.vaccine_data,
            intervention_data.surveillance_data,
            TreatmentLocation(health_setting),
            initial_status
        ))
        .unwrap();

        Ok(())
    }
}
impl ContextTransmissionInternalExt for Context {}

pub trait ContextTransmissionExt: PluginContext + ContextEntitiesExt + ContextDetectionExt {
    /// Calculate if the index case is confirmed
    /// Currenlty does not assume that the shared index contact probability affects results
    /// If including shared index probability, the result on each smaple should be cached and referenced for future calls
    fn is_from_confirmed(&self, person_id: PersonId) -> bool {
        if let Some(primary) = self.get_property::<Person, PrimaryInfection>(person_id).0 {
            return self.get_property::<Person, CaseStatus>(primary)
                == CaseStatus(Some(CaseStatusValue::Confirmed));
        }
        false
    }

    fn handle_detection(&mut self, person_id: PersonId) {
        let infection_status = self.get_property::<Person, InfectionStatus>(person_id);
        let current_setting = self.get_property::<Person, TreatmentLocation>(person_id).0;
        match infection_status {
            // Quarantine the case if not yet infectious and not previously assigned a health setting treatment location
            InfectionStatus::Presymptomatic => {
                if current_setting.is_none() {
                    self.set_property(
                        person_id,
                        TreatmentLocation(Some(HealthSetting::Quarantine)),
                    );
                }
            }
            // If infectious, set case status to confirmed in order to trigger transfer to ETU if not already in a treatment location
            // Otherwise initiate quarantine and suspected case status
            InfectionStatus::Symptomatic => {
                if current_setting.is_none() {
                    self.set_property(
                        person_id,
                        TreatmentLocation(Some(HealthSetting::Quarantine)),
                    );
                    self.set_property(person_id, CaseStatus(Some(CaseStatusValue::Suspected)));
                } else {
                    match self.get_property::<Person, CaseStatus>(person_id).0 {
                        Some(CaseStatusValue::Confirmed) => (), // Already confirmed case, no action needed
                        Some(CaseStatusValue::Suspected) => self.set_property(person_id, CaseStatus(Some(CaseStatusValue::Confirmed))),
                        None => panic!("Symptomatic person {:?} in treatment location {:?} has no case status assigned.", person_id, current_setting),
                    }
                }
            }
            // No action to be taken for newly detected and previously recovered
            InfectionStatus::Removed => (),
            InfectionStatus::Vaccinated => (),
            InfectionStatus::Susceptible => (),
        }
    }

    fn avert_case(&mut self) {
        let container = self.get_data_mut(InterventionPlugin);
        container.vaccine_cases_averted += 1;
    }

    fn vaccine_cases_averted(&self) -> usize {
        let container = self.get_data(InterventionPlugin);
        container.vaccine_cases_averted
    }

    fn health_setting_cases_averted(&self) -> usize {
        let container = self.get_data(InterventionPlugin);
        container.health_setting_cases_averted
    }
}
impl ContextTransmissionExt for Context {}

pub fn init(context: &mut Context) {
    // Connect detected cases to confirmation and quarantine
    context.subscribe_to_event(
        |context, event: PropertyChangeEvent<Person, DetectionStatus>| {
            if event.current == DetectionStatus::Detected
                && event.previous == DetectionStatus::Undetected
            {
                context.handle_detection(event.entity_id);
            }
        },
    );

    // Connect newly arisen detected cases with confirmation and quarantine
    context.subscribe_to_event(|context, event: EntityCreatedEvent<Person>| {
        if context.get_property::<Person, DetectionStatus>(event.entity_id)
            == DetectionStatus::Detected
        {
            context.handle_detection(event.entity_id);
        }
    });

    // Connect undetected confirmed cases to detection
    context.subscribe_to_event(|context, event: PropertyChangeEvent<Person, CaseStatus>| {
        if let Some(CaseStatusValue::Confirmed) = event.current.0 {
            if context.get_property::<Person, DetectionStatus>(event.entity_id)
                == DetectionStatus::Undetected
            {
                context.set_passive_detection(event.entity_id);
            }
        }
    });
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::case_confirmation::{TestingConfig, TestingRate};
    use crate::clinical_manager::init as init_clinic;
    use crate::detection_manager::init as init_detect;
    use crate::detection_manager::{PrimaryInfection, SurveillanceData, SurveillanceType};
    use crate::distributions::ContinuousDistributionParameterized;
    use crate::parameters::{ParameterValues, Parameters};
    use crate::shutdown::EndRunConditions;
    use crate::state_trigger::StateTrigger;
    use crate::vaccination::init as init_vaccination;
    use crate::vaccination::VaccineData;
    use crate::vaccination_campaign::{
        GeographicVaccinationCampaign, RingVaccinationCampaign, Vaccination, VaccinationSchedule,
    };
    use crate::Probability;
    use anyhow::Result;
    use ixa::{assert_almost_eq, ContextGlobalPropertiesExt, ContextRandomExt, ExecutionPhase};

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

    fn probability(value: f64) -> Probability {
        Probability::try_from(value).expect("valid probability")
    }

    fn non_negative(value: f64) -> crate::NonNegativeFinite {
        crate::NonNegativeFinite::try_from(value).expect("valid non-negative finite")
    }

    #[test]
    fn test_track_susceptibles() -> Result<()> {
        let params = ParameterValues {
            track_susceptibles: true,
            ..Default::default()
        };
        let context = setup(params);
        assert!(context.track_susceptibles());

        let params = ParameterValues {
            track_susceptibles: false,
            ..Default::default()
        };
        let context = setup(params);
        assert!(!context.track_susceptibles());
        Ok(())
    }

    #[test]
    fn test_avert_case_untracked_susceptibles() {
        let params = ParameterValues {
            track_susceptibles: false,
            ..Default::default()
        };
        let mut context = setup(params);

        let infector_id = context
            .add_entity(with!(
                Person,
                InfectionStatus::Symptomatic,
                TreatmentLocation(Some(HealthSetting::Quarantine))
            ))
            .unwrap();
        let intervention_data = context
            .sample_contact_intervention_data(infector_id)
            .unwrap();

        context
            .handle_setting_averted_case(infector_id, intervention_data)
            .unwrap();

        let container = context.get_data(InterventionPlugin);
        let cases_averted = container.health_setting_cases_averted;

        assert_eq!(context.get_entity_count::<Person>(), 1);
        assert_eq!(cases_averted, 1);
    }

    #[test]
    fn test_avert_case_tracked_susceptibles() {
        let params = ParameterValues {
            track_susceptibles: true,
            ..Default::default()
        };
        let mut context = setup(params);

        let infector_id = context
            .add_entity(with!(
                Person,
                InfectionStatus::Symptomatic,
                TreatmentLocation(Some(HealthSetting::Quarantine))
            ))
            .unwrap();
        let intervention_data = context
            .sample_contact_intervention_data(infector_id)
            .unwrap();

        context
            .handle_setting_averted_case(infector_id, intervention_data)
            .unwrap();

        let container = context.get_data(InterventionPlugin);
        let cases_averted = container.health_setting_cases_averted;

        assert_eq!(context.get_entity_count::<Person>(), 2);
        assert_eq!(cases_averted, 1);
    }

    #[test]
    fn test_init() -> Result<()> {
        let test_delay = 0.5;
        let hosp_delay = 2.0;
        let etu_transfer_delay = 1.0;
        let detection_delay = 3.0;
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(6.0),
            hospitalization_probability: probability(1.0),
            hospitalization_delay_distribution: fixed(hosp_delay),
            testing_config: TestingConfig::enabled(fixed(test_delay), TestingRate::Unlimited),
            etu_transfer_delay,
            active_detection_probability: probability(1.0),
            active_detection_delay_distribution: fixed(detection_delay),
            ..Default::default()
        };
        let mut context = setup(params);

        init_clinic(&mut context);
        init_detect(&mut context)?;
        init(&mut context);
        context.plan_surveillance_campaign()?;

        let p = context.add_entity(Person).unwrap();
        let contact = context
            .add_entity(with!(
                Person,
                TransmissionChain(Some(TransmissionChainData::new_with_infector(
                    0.0,
                    p,
                    Some(0)
                ))),
            ))
            .unwrap();

        context.add_plan(0.0, move |context| {
            context.set_property(p, InfectionStatus::Symptomatic);
        });

        // Assert trajectory of primary case p through the intervention setup
        context.add_plan_with_phase(
            hosp_delay,
            move |context| {
                assert_eq!(
                    context.get_property::<Person, CaseStatus>(p).0,
                    Some(CaseStatusValue::Suspected)
                );
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(p).0,
                    Some(HealthSetting::Clinic)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(p),
                    DetectionStatus::Undetected
                );
            },
            ExecutionPhase::Last,
        );

        context.add_plan_with_phase(
            hosp_delay + test_delay,
            move |context| {
                assert_eq!(
                    context.get_property::<Person, CaseStatus>(p).0,
                    Some(CaseStatusValue::Confirmed)
                );
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(p).0,
                    Some(HealthSetting::Clinic)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(p),
                    DetectionStatus::Detected
                );
            },
            ExecutionPhase::Last,
        );

        context.add_plan_with_phase(
            hosp_delay + test_delay + etu_transfer_delay,
            move |context| {
                assert_eq!(
                    context.get_property::<Person, CaseStatus>(p).0,
                    Some(CaseStatusValue::Confirmed)
                );
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(p).0,
                    Some(HealthSetting::EbolaTreatmentUnit)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(p),
                    DetectionStatus::Detected
                );
            },
            ExecutionPhase::Last,
        );

        // Assert the proper trajectory for the contact individual with active contact tracing turned on
        let symptom_onset_time = hosp_delay + test_delay + detection_delay + 1.0;
        context.add_plan(symptom_onset_time, move |context| {
            context.set_property(contact, InfectionStatus::Symptomatic);
        });

        context.add_plan_with_phase(
            hosp_delay + test_delay + detection_delay,
            move |context| {
                assert_eq!(context.get_property::<Person, CaseStatus>(contact).0, None);
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(contact).0,
                    Some(HealthSetting::Quarantine)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(contact),
                    DetectionStatus::Detected
                );
            },
            ExecutionPhase::Last,
        );

        context.add_plan_with_phase(
            symptom_onset_time,
            move |context| {
                assert_eq!(
                    context.get_property::<Person, CaseStatus>(contact).0,
                    Some(CaseStatusValue::Suspected)
                );
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(contact).0,
                    Some(HealthSetting::Quarantine)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(contact),
                    DetectionStatus::Detected
                );
            },
            ExecutionPhase::Last,
        );

        context.add_plan_with_phase(
            symptom_onset_time + test_delay,
            move |context| {
                assert_eq!(
                    context.get_property::<Person, CaseStatus>(contact).0,
                    Some(CaseStatusValue::Confirmed)
                );
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(contact).0,
                    Some(HealthSetting::Quarantine)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(contact),
                    DetectionStatus::Detected
                );
            },
            ExecutionPhase::Last,
        );

        context.add_plan_with_phase(
            symptom_onset_time + test_delay + etu_transfer_delay,
            move |context| {
                assert_eq!(
                    context.get_property::<Person, CaseStatus>(contact).0,
                    Some(CaseStatusValue::Confirmed)
                );
                assert_eq!(
                    context.get_property::<Person, TreatmentLocation>(contact).0,
                    Some(HealthSetting::EbolaTreatmentUnit)
                );
                assert_eq!(
                    context.get_property::<Person, DetectionStatus>(contact),
                    DetectionStatus::Detected
                );
            },
            ExecutionPhase::Last,
        );

        context.execute();
        Ok(())
    }

    #[test]
    fn test_detection_init_respects_infection_attempt() -> Result<()> {
        // Contact tracing should be 50% of the time, with detect_init overwriting results 50% of the time if it were to fail
        let n = 1000;
        let mut contacts_traced = 0;
        let active_detection_probability = 0.5;
        for seed in 0..n {
            let params = ParameterValues {
                end_run_conditions: EndRunConditions::from_max_time(2.5),
                active_detection_probability: probability(active_detection_probability),
                active_detection_delay_distribution: fixed(1.0),
                vaccination: Vaccination::Available {
                    efficacy: probability(0.0),
                    protection_delay_distribution: fixed(1.0),
                    ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                        delay: non_negative(0.001),
                        trigger: StateTrigger::time(0.0).unwrap(),
                    },
                    geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                        delay: non_negative(0.001),
                        trigger: StateTrigger::time(0.0).unwrap(),
                        max_prevalence: Probability::try_from(1.0).unwrap(),
                        schedule: VaccinationSchedule::Parameterized {
                            distribution: fixed(1.0),
                        },
                    },
                },
                seed,
                ..Default::default()
            };
            let mut context = setup(params);

            let index = context
                .add_entity(with!(
                    Person,
                    SurveillanceData::Detected {
                        detection_time: OrderedFloat(-3.0),
                        detection_type: SurveillanceType::Passive,
                        surveillance_contact_id: None
                    },
                    InfectionStatus::Symptomatic
                ))
                .unwrap();

            init_vaccination(&mut context).unwrap();
            context.plan_surveillance_campaign()?;

            context.add_plan(0.01, move |context| {
                context.trace_contacts(index).unwrap();
                infection_attempt(context, index).unwrap();
            });

            context.subscribe_to_event::<PropertyChangeEvent<Person, DetectionStatus>>(move | context, event| {
                if event.entity_id != index {
                    let surveillance_data = context.get_property::<Person, SurveillanceData>(event.entity_id);
                    if event.current == event.previous {
                        match surveillance_data {
                            SurveillanceData::Detected { detection_time, detection_type, .. } =>  panic!("Detection status values should not change if the contact was detected. The change occured at time {} for {:?} with reported {:?} detection at time {}.", context.get_current_time(), event.entity_id, detection_type, detection_time),
                            SurveillanceData::Undetected { passive_detection_attempt_time, .. } => {
                                // Passive detection attempt time should be none
                                assert!(passive_detection_attempt_time.is_none());
                            }
                        }
                    } else {
                        match surveillance_data {
                            SurveillanceData::Detected { detection_time, detection_type, .. } => {
                                assert_eq!(detection_type, SurveillanceType::Active);
                                // Assert that the detection delay from surveillance campaign is used, not person created time + delay.
                                assert_almost_eq!(detection_time.into_inner(), 1.0, 0.0);
                            },
                            SurveillanceData::Undetected { .. } => panic!("Detection status values should never change in this example. All contacts should end with their original value when created.")
                        }
                    }
                }
            });

            init_detect(&mut context)?;
            context.execute();
            assert_eq!(context.get_entity_count::<Person>(), 2);
            if context.query_entity_count(with!(Person, DetectionStatus::Detected)) == 2 {
                contacts_traced += 1;
            }
        }
        let observed_probability = contacts_traced as f64 / n as f64;
        assert_almost_eq!(observed_probability, active_detection_probability, 0.05);
        Ok(())
    }

    #[test]
    fn test_handle_detection() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.5),
            active_detection_probability: probability(1.0),
            active_detection_delay_distribution: fixed(1.0),
            vaccination: Vaccination::Available {
                efficacy: probability(0.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                },
                geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                    max_prevalence: Probability::try_from(1.0).unwrap(),
                    schedule: VaccinationSchedule::Parameterized {
                        distribution: fixed(1.0),
                    },
                },
            }, // vaccine fails to always create the individual
            quarantine_transmission_probability: probability(1.0), // quarantine does not limit transmission to always create the individual
            ..Default::default()
        };
        let mut context = setup(params);

        let index = context.add_entity(Person).unwrap();
        init_vaccination(&mut context).unwrap();

        context.add_plan(0.0, move |context| {
            context.set_property(
                index,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(0.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None,
                },
            );
            context.set_property(index, CaseStatus(Some(CaseStatusValue::Confirmed)));
            context.trace_contacts(index).unwrap();
        });
        context.add_plan(0.1, move |context| {
            infection_attempt(context, index).unwrap();
        });

        init(&mut context);
        context.execute();
        assert_eq!(context.get_entity_count::<Person>(), 2);
        assert_eq!(
            context.query_entity_count(with!(
                Person,
                TreatmentLocation(Some(HealthSetting::Quarantine))
            )),
            2
        );
        Ok(())
    }

    // get population if vaccine is total failure
    #[test]
    fn test_infection_attempt_ring_vaccine_failure() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.5),
            active_detection_probability: probability(1.0),
            active_detection_delay_distribution: fixed(1.0),
            vaccination: Vaccination::Available {
                efficacy: probability(0.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                },
                geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                    max_prevalence: Probability::try_from(1.0).unwrap(),
                    schedule: VaccinationSchedule::Parameterized {
                        distribution: fixed(1.0),
                    },
                },
            },
            ..Default::default()
        };
        let mut context = setup(params);
        let index = context
            .add_entity(with!(
                Person,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(0.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None
                }
            ))
            .unwrap();
        context.trace_contacts(index)?;
        init_vaccination(&mut context).unwrap();
        context.execute();

        infection_attempt(&mut context, index)?;
        assert_eq!(context.get_entity_count::<Person>(), 2);
        assert_eq!(
            context.query_entity_count(with!(Person, PrimaryInfection(Some(index)))),
            1
        );

        let secondary = context
            .query_result_iterator(with!(Person, PrimaryInfection(Some(index))))
            .next()
            .unwrap();
        let vaccine_data = context.get_property::<Person, VaccineData>(secondary);
        let expected_data = VaccineData::Vaccinated {
            inoculation_time: OrderedFloat(1.0),
            effective_time: None,
            campaign_deploy_method: VaccinationCampaignType::Ring,
            vaccine_success: false,
        };
        assert_eq!(vaccine_data, expected_data);
        Ok(())
    }

    // get population if vaccine is success but too late
    #[test]
    fn test_infection_attempt_ring_vaccine_success_late() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.5),
            active_detection_probability: probability(1.0),
            active_detection_delay_distribution: fixed(1.0),
            vaccination: Vaccination::Available {
                efficacy: probability(1.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                },
                geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                    max_prevalence: Probability::try_from(1.0).unwrap(),
                    schedule: VaccinationSchedule::Parameterized {
                        distribution: fixed(1.0),
                    },
                },
            },
            ..Default::default()
        };
        let mut context = setup(params);
        let index = context
            .add_entity(with!(
                Person,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(0.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None
                }
            ))
            .unwrap();
        context.trace_contacts(index)?;
        init_vaccination(&mut context).unwrap();
        context.execute();

        infection_attempt(&mut context, index)?;
        assert_eq!(context.get_entity_count::<Person>(), 2);
        assert_eq!(
            context.query_entity_count(with!(Person, InfectionStatus::Vaccinated)),
            0
        );
        assert_eq!(
            context.query_entity_count(with!(Person, PrimaryInfection(Some(index)))),
            1
        );

        let secondary = context
            .query_result_iterator(with!(Person, PrimaryInfection(Some(index))))
            .next()
            .unwrap();
        let vaccine_data = context.get_property::<Person, VaccineData>(secondary);
        let expected_data = VaccineData::Vaccinated {
            inoculation_time: OrderedFloat(1.0),
            effective_time: Some(OrderedFloat(2.0)),
            campaign_deploy_method: VaccinationCampaignType::Ring,
            vaccine_success: true,
        };
        assert_eq!(vaccine_data, expected_data);
        Ok(())
    }
    // get population if vaccine is success and case averted
    #[test]
    fn test_infection_attempt_ring_vaccine_success() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.5),
            active_detection_probability: probability(1.0),
            active_detection_delay_distribution: fixed(1.0),
            vaccination: Vaccination::Available {
                efficacy: probability(1.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                },
                geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                    delay: non_negative(0.001),
                    trigger: StateTrigger::time(0.0).unwrap(),
                    max_prevalence: Probability::try_from(1.0).unwrap(),
                    schedule: VaccinationSchedule::Parameterized {
                        distribution: fixed(1.0),
                    },
                },
            },
            ..Default::default()
        };
        let mut context = setup(params);
        let index = context
            .add_entity(with!(
                Person,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(-3.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None
                }
            ))
            .unwrap();
        context.trace_contacts(index)?;
        init_vaccination(&mut context).unwrap();
        context.execute();

        infection_attempt(&mut context, index)?;
        assert_eq!(context.get_entity_count::<Person>(), 2);
        assert_eq!(
            context.query_entity_count(with!(Person, InfectionStatus::Vaccinated)),
            1
        );
        Ok(())
    }
}
