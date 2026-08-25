use anyhow::{bail, ensure, Result};
use ixa::{
    define_data_plugin, define_rng, impl_derived_property, impl_property, prelude::*, Context,
    HashSet, HashSetExt, IxaEvent, PluginContext,
};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::disease_manager::DiseaseManagerExt;
use crate::state_trigger::ContextTriggerExt;
use crate::PositiveCount;
use crate::{
    disease_manager::{Alive, InfectionStatus},
    distributions::{ContinuousDistributionParameterized, Delay},
    parameters::{ContextParametersExt, ParameterValues},
    state_trigger::StateTrigger,
};
use crate::{Person, PersonId};

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TransmissionChainData {
    pub infector_id: Option<PersonId>,
    pub infection_time: OrderedFloat<f64>,
    pub infector_generation: Option<usize>,
}

impl TransmissionChainData {
    pub fn new(time: f64) -> Self {
        TransmissionChainData {
            infector_id: None,
            infection_time: OrderedFloat(time),
            infector_generation: None,
        }
    }
    pub fn new_with_infector(
        time: f64,
        infector_id: PersonId,
        infector_generation: Option<usize>,
    ) -> Self {
        TransmissionChainData {
            infector_id: Some(infector_id),
            infection_time: OrderedFloat(time),
            infector_generation,
        }
    }
}

define_property!(
    struct TransmissionChain(Option<TransmissionChainData>),
    Person,
    default_const = TransmissionChain(None)
);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SurveillanceCampaignDelayConfig {
    deploy: bool,
    trigger: Option<StateTrigger>,
    distribution: Option<ContinuousDistributionParameterized>,
}

impl Default for SurveillanceCampaignDelayConfig {
    fn default() -> Self {
        SurveillanceCampaignDelayConfig {
            trigger: Some(StateTrigger::Detection {
                count: PositiveCount::ONE,
            }),
            distribution: Some(ContinuousDistributionParameterized::fixed_delay(
                Delay::ZERO,
            )),
            deploy: true,
        }
    }
}

impl SurveillanceCampaignDelayConfig {
    pub fn validate(&self) -> Result<()> {
        if self.deploy {
            ensure!(
                self.trigger.is_some(),
                "Surveillance campaign must have a trigger if deploy is true"
            );
            ensure!(
                self.distribution.is_some(),
                "Surveillance campaign must have a delay distribution if deploy is true"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation(pub Option<usize>);

impl_derived_property!(Generation, Person, [TransmissionChain], [], |data| {
    if let TransmissionChain(Some(chain_data)) = data {
        if chain_data.infector_id.is_some() {
            // If there is an infector, increment their generation by 1
            Generation(chain_data.infector_generation.map(|gen| gen + 1))
        } else {
            // If there is no infector, this is a primary case at generation 0
            Generation(Some(0))
        }
    } else {
        Generation(None)
    }
});

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimaryInfection(pub Option<PersonId>);

impl_derived_property!(
    PrimaryInfection,
    Person,
    [TransmissionChain],
    [],
    |data| match data {
        TransmissionChain(Some(chain_data)) => PrimaryInfection(chain_data.infector_id),
        _ => PrimaryInfection(None),
    }
);

struct SurveillanceCampaignData {
    surveillance_campaign_active: bool,
    surveillance_campaign_start_time: Option<f64>,
    cases_traced: HashSet<PersonId>,
}

#[derive(IxaEvent)]
pub struct SurveillanceCampaignStartEvent {
    pub time: f64,
}

define_data_plugin!(
    SurveillancePlugin,
    SurveillanceCampaignData,
    SurveillanceCampaignData {
        surveillance_campaign_active: false,
        surveillance_campaign_start_time: None,
        cases_traced: HashSet::new()
    }
);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurveillanceType {
    Active,
    Passive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum SurveillanceData {
    Detected {
        detection_time: OrderedFloat<f64>,
        detection_type: SurveillanceType,
        surveillance_contact_id: Option<PersonId>,
    },
    Undetected {
        passive_detection_attempt_time: Option<OrderedFloat<f64>>,
        active_detection_attempt_time: Option<OrderedFloat<f64>>,
    },
}

impl_property!(
    SurveillanceData,
    Person,
    default_const = SurveillanceData::Undetected {
        passive_detection_attempt_time: None,
        active_detection_attempt_time: None,
    }
);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectionStatus {
    Detected,
    Undetected,
}

impl_derived_property!(
    DetectionStatus,
    Person,
    [SurveillanceData],
    [],
    |data| match data {
        SurveillanceData::Detected { .. } => DetectionStatus::Detected,
        SurveillanceData::Undetected { .. } => DetectionStatus::Undetected,
    }
);

define_rng!(ActiveDetectionRng);
define_rng!(PassiveDetectionRng);
define_rng!(SharedIndexRng);
define_rng!(ActiveDetectionDelayRng);
define_rng!(PassiveDetectionDelayRng);
define_rng!(SurveillanceCampaignRng);

pub trait ContextDetectionExt:
    PluginContext
    + ContextEntitiesExt
    + ContextRandomExt
    + ContextParametersExt
    + DiseaseManagerExt
    + ContextTriggerExt
{
    fn get_detection_time(&self, person_id: PersonId) -> Option<f64> {
        let primary_detection_data = self.get_property::<Person, SurveillanceData>(person_id);
        match primary_detection_data {
            SurveillanceData::Detected { detection_time, .. } => Some(detection_time.into_inner()),
            SurveillanceData::Undetected { .. } => None,
        }
    }

    /// Samples a time that a contact would be detected for a given index case, accounting for failure to trace the contact.
    ///
    /// # Parameters
    /// - `index_case`: The `PersonId` of the index case who is having their contacts traced
    fn sample_contact_trace_time(&self, index_case: PersonId) -> Result<Option<f64>> {
        let &ParameterValues {
            active_detection_probability,
            active_detection_delay_distribution,
            ..
        } = self.get_params();

        match self.get_detection_time(index_case) {
            Some(primary_detection_time) => {
                if self.sample_bool(ActiveDetectionRng, active_detection_probability) {
                    let detection_time = primary_detection_time
                        + self.sample_distr(ActiveDetectionDelayRng, active_detection_delay_distribution);
                    return Ok(Some(detection_time));
                }
            },
            None => bail!("Active detection times sampled for index {index_case:?} even though case is undetected"),
        }

        Ok(None)
    }
    /// Samples an active detection plan for a given contact.
    ///
    /// # Parameters
    /// - `contact`: The `PersonId` of the contact for whom the detection plan is sampled.
    ///
    /// This method retrieves the global parameters and detection delay distribution,
    /// samples a boolean to decide if an active detection plan should be created,
    /// and schedules a detection plan if the sample is positive.
    fn make_active_detection_plan(&mut self, primary: PersonId, contact: PersonId) -> Result<()> {
        if let Some(detection_time) = self.sample_contact_trace_time(primary)? {
            if detection_time > self.get_current_time() {
                self.add_plan(detection_time, move |context| {
                    context.set_property(
                        contact,
                        SurveillanceData::Detected {
                            detection_time: OrderedFloat(detection_time),
                            detection_type: SurveillanceType::Active,
                            surveillance_contact_id: Some(primary),
                        },
                    );
                });
            } else {
                self.set_property(
                    contact,
                    SurveillanceData::Detected {
                        detection_time: OrderedFloat(detection_time),
                        detection_type: SurveillanceType::Active,
                        surveillance_contact_id: Some(primary),
                    },
                );
            }
        }
        Ok(())
    }

    /// Traces contacts of a detected case.
    ///
    /// # Parameters
    /// - `detected_case`: The `PersonId` of the detected case.
    ///
    /// This method queries for secondary cases that have been in contact with the detected case
    /// and are currently undetected, and then samples active detection plans for each of them.
    fn trace_contacts(&mut self, detected_case: PersonId) -> Result<()> {
        let &ParameterValues {
            shared_index_contact_probability,
            ..
        } = self.get_params();

        let container = self.get_data_mut(SurveillancePlugin);

        if container.cases_traced.insert(detected_case) {
            let secondary_cases: Vec<_> = self
                .query_result_iterator(with!(
                    Person,
                    PrimaryInfection(Some(detected_case)),
                    DetectionStatus::Undetected
                ))
                .collect();

            for contact in secondary_cases {
                if !self.is_tracking_contacts_of(&contact) {
                    self.make_active_detection_plan(detected_case, contact)?;
                }

                // Check if downstream contacts should be traced with probability that the tertiary case is a shared contact of index and the secondary contact
                let tertiary_cases: Vec<_> = self
                    .query_result_iterator(with!(
                        Person,
                        PrimaryInfection(Some(contact)),
                        DetectionStatus::Undetected
                    ))
                    .collect();

                for tertiary in tertiary_cases {
                    if self.sample_bool(SharedIndexRng, shared_index_contact_probability)
                        & !self.is_tracking_contacts_of(&tertiary)
                    {
                        self.make_active_detection_plan(detected_case, tertiary)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn set_passive_detection(&mut self, person_id: PersonId) {
        self.set_property(
            person_id,
            SurveillanceData::Detected {
                detection_time: OrderedFloat(self.get_current_time()),
                detection_type: SurveillanceType::Passive,
                surveillance_contact_id: None,
            },
        );
    }

    fn make_passive_detection_plan(&mut self, person_id: PersonId) {
        let &ParameterValues {
            passive_detection_probability,
            passive_detection_delay_distribution,
            ..
        } = self.get_params();

        let detection_time = self.get_current_time()
            + self.sample_distr(
                PassiveDetectionDelayRng,
                passive_detection_delay_distribution,
            );
        if self.sample_bool(PassiveDetectionRng, passive_detection_probability) {
            self.add_plan(detection_time, move |context| {
                if context.get_property::<Person, DetectionStatus>(person_id)
                    == DetectionStatus::Undetected
                {
                    context.set_passive_detection(person_id);
                }
            });
        } else {
            self.add_plan(detection_time, move |context| {
                match context.get_property::<Person, SurveillanceData>(person_id) {
                    SurveillanceData::Undetected {
                        active_detection_attempt_time,
                        ..
                    } => {
                        context.set_property(
                            person_id,
                            SurveillanceData::Undetected {
                                passive_detection_attempt_time: Some(OrderedFloat(detection_time)),
                                active_detection_attempt_time,
                            },
                        );
                    }
                    SurveillanceData::Detected { .. } => {}
                }
            });
        }
    }

    fn is_tracking_contacts_of(&self, person_id: &PersonId) -> bool {
        let container = self.get_data(SurveillancePlugin);
        container.cases_traced.contains(person_id)
    }

    fn plan_surveillance_campaign(&mut self) -> Result<()> {
        let ParameterValues {
            surveillance_campaign_delay,
            ..
        } = self.get_params();

        if let Some(surveillance_campaign_delay_distribution) =
            surveillance_campaign_delay.distribution
        {
            // With a `Fixed` delay distribution, this sample calls the same delay period, once only, for each simulation
            let start_time = self.sample_distr(
                SurveillanceCampaignRng,
                surveillance_campaign_delay_distribution,
            ) + self.get_current_time();
            let container = self.get_data_mut(SurveillancePlugin);
            container.surveillance_campaign_start_time = Some(start_time);

            self.add_plan(start_time, | context | {
                // Only contact tracing reads `PrimaryInfection`, so the index is built
                // here rather than in `init`: runs that end before the campaign starts
                // never pay to maintain it. Indexing backfills existing people.
                context.index_property::<Person, PrimaryInfection>();
                let container = context.get_data_mut(SurveillancePlugin);
                container.surveillance_campaign_active = true;
                let detected: Vec<_> = context.query_result_iterator(with!(Person, DetectionStatus::Detected)).collect();
                for person_id in detected {
                    if context.get_property::<Person, InfectionStatus>(person_id) == InfectionStatus::Presymptomatic {
                        panic!("Person {:?} was detected before the surveillance campaign began but is still presymptomatic at the time the campaign starts. This should not be possible.", person_id);
                    }
                    if !context.is_tracking_contacts_of(&person_id) {
                        context.set_passive_detection(person_id);
                        context.trace_contacts(person_id).unwrap();
                    } else {
                        panic!("Person {:?} was being tracked before the surveillance campaign began.", person_id)
                    }
                }
                let undetected: Vec<_> = context.query_result_iterator(with!(Person, DetectionStatus::Undetected, InfectionStatus::Symptomatic, Alive(true))).collect();
                for person_id in undetected {
                    context.make_passive_detection_plan(person_id);
                }
                context.emit_event(SurveillanceCampaignStartEvent {
                    time: context.get_current_time(),
                });
            });
        } else {
            bail!("Surveillance campaign set to deploy but no distribution specified");
        }
        Ok(())
    }

    fn surveillance_campaign_active(&self) -> bool {
        self.get_data(SurveillancePlugin)
            .surveillance_campaign_active
    }

    fn surveillance_campaign_planned(&self) -> bool {
        self.get_data(SurveillancePlugin)
            .surveillance_campaign_start_time
            .is_some()
    }

    fn setup_surveillance_campaign(&mut self) -> Result<()> {
        let ParameterValues {
            surveillance_campaign_delay,
            ..
        } = self.get_params();

        if surveillance_campaign_delay.deploy {
            if let Some(trigger) = surveillance_campaign_delay.trigger {
                self.register_triggered_event(trigger, |context| {
                    if !context.surveillance_campaign_planned() {
                        context.plan_surveillance_campaign().unwrap();
                    }
                });
            } else {
                bail!("Surveillance campaign set to deploy but no trigger specified");
            }
        }
        Ok(())
    }
}
impl ContextDetectionExt for Context {}

fn handle_detected_case(context: &mut Context, person_id: PersonId) {
    if context.surveillance_campaign_active() {
        context.trace_contacts(person_id).unwrap();
    }
}

pub fn init(context: &mut Context) -> Result<()> {
    context.subscribe_to_event(
        |context, event: PropertyChangeEvent<Person, InfectionStatus>| {
            if event.current == InfectionStatus::Symptomatic
                && context.surveillance_campaign_active()
            {
                match context.get_property::<Person, SurveillanceData>(event.entity_id) {
                    SurveillanceData::Undetected {
                        passive_detection_attempt_time,
                        ..
                    } => {
                        if passive_detection_attempt_time.is_none() {
                            context.make_passive_detection_plan(event.entity_id);
                        }
                    }
                    SurveillanceData::Detected { .. } => {}
                }
            }
        },
    );

    context.subscribe_to_event(
        |context, event: PropertyChangeEvent<Person, SurveillanceData>| match event.current {
            SurveillanceData::Detected { .. } => {
                handle_detected_case(context, event.entity_id);
            }
            SurveillanceData::Undetected { .. } => {}
        },
    );

    context.setup_surveillance_campaign()?;

    context.subscribe_to_event::<EntityCreatedEvent<Person>>(|context, event| {
        let detection_data = context.get_property::<Person, SurveillanceData>(event.entity_id);
        // Only make an active detection plan if the default values haven't been touched already
        match detection_data {
            SurveillanceData::Undetected {
                active_detection_attempt_time,
                ..
            } => {
                if active_detection_attempt_time.is_none() && context.surveillance_campaign_active()
                {
                    if let Some(infector) = context
                        .get_property::<Person, PrimaryInfection>(event.entity_id)
                        .0
                    {
                        if context.is_tracking_contacts_of(&infector) {
                            context
                                .make_active_detection_plan(infector, event.entity_id)
                                .unwrap();
                        }
                    }
                }
            }
            SurveillanceData::Detected { .. } => {
                handle_detected_case(context, event.entity_id);
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::{
        branching_process::BranchingProcessExt,
        disease_manager::Alive,
        distributions::{ContinuousDistributionParameterized, DiscreteDistributionParameterized},
        infection_initialization::InfectionInitialization,
        parameters::{ParameterValues, Parameters},
        NonNegativeFinite, Probability,
    };
    use ixa::{assert_almost_eq, ContextGlobalPropertiesExt};

    fn fixed(delay: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::fixed(delay).expect("valid fixed delay")
    }

    fn uniform(min: f64, max: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::uniform(min, max).expect("valid uniform parameters")
    }

    fn probability(value: f64) -> Probability {
        Probability::try_from(value).expect("valid probability")
    }

    fn poisson(mean: f64) -> DiscreteDistributionParameterized {
        DiscreteDistributionParameterized::poisson(mean).expect("valid Poisson mean")
    }

    fn setup_context_with_campaign(params: ParameterValues) -> Context {
        let mut context = Context::new();
        context.init_random(params.seed);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context.setup_surveillance_campaign().unwrap();
        context
    }

    #[test]
    fn test_surveillance_campaign_trigger_on_detection() {
        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Detection {
                    count: PositiveCount::ONE,
                }),
                distribution: Some(fixed(1.0)),
            },
            ..Default::default()
        });

        let person_id = context.add_entity(Person).unwrap();
        context.add_plan(1.0, move |context| {
            context.set_property(person_id, InfectionStatus::Symptomatic);
            context.set_property(
                person_id,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(1.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None,
                },
            );
        });

        assert!(!context.surveillance_campaign_active());
        context.execute();
        assert!(context.surveillance_campaign_active());
        assert_almost_eq!(
            context
                .get_data(SurveillancePlugin)
                .surveillance_campaign_start_time
                .unwrap(),
            2.0,
            0.0
        );
    }

    #[test]
    fn test_surveillance_campaign_trigger_on_time() {
        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Time {
                    time: NonNegativeFinite::try_from(5.0).unwrap(),
                }),
                distribution: Some(fixed(2.0)),
            },
            ..Default::default()
        });

        assert!(!context.surveillance_campaign_active());
        context.execute();
        assert!(context.surveillance_campaign_active());
        assert_almost_eq!(
            context
                .get_data(SurveillancePlugin)
                .surveillance_campaign_start_time
                .unwrap(),
            7.0,
            0.0
        );
    }

    #[test]
    fn test_surveillance_campaign_trigger_on_deaths() {
        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Deaths {
                    count: PositiveCount::ONE,
                }),
                distribution: Some(fixed(1.0)),
            },
            ..Default::default()
        });

        let person_id = context.add_entity(Person).unwrap();
        context.add_plan(1.0, move |context| {
            context.set_property(person_id, Alive(false));
        });

        assert!(!context.surveillance_campaign_active());
        context.execute();
        assert!(context.surveillance_campaign_active());
        assert_almost_eq!(
            context
                .get_data(SurveillancePlugin)
                .surveillance_campaign_start_time
                .unwrap(),
            2.0,
            0.0
        );
    }

    #[test]
    fn test_surveillance_campaign_does_not_trigger_on_infections() {
        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Cases {
                    count: PositiveCount::ONE,
                }),
                distribution: Some(fixed(1.0)),
            },
            ..Default::default()
        });

        let person_id = context.add_entity(Person).unwrap();
        context.add_plan(1.0, move |context| {
            context.set_property(person_id, InfectionStatus::Symptomatic);
        });

        assert!(!context.surveillance_campaign_active());
        context.execute();
        assert!(!context.surveillance_campaign_active());
    }

    #[test]
    fn test_surveillance_campaign_trigger_on_cases() {
        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Cases {
                    count: PositiveCount::ONE,
                }),
                distribution: Some(fixed(1.0)),
            },
            ..Default::default()
        });
        crate::disease_manager::init(&mut context);

        let person_id = context.add_entity(Person).unwrap();
        context.add_plan(1.0, move |context| {
            context.set_property(person_id, InfectionStatus::Symptomatic);
        });

        assert!(!context.surveillance_campaign_active());
        context.execute();
        assert!(context.surveillance_campaign_active());
        assert_almost_eq!(
            context
                .get_data(SurveillancePlugin)
                .surveillance_campaign_start_time
                .unwrap(),
            2.0,
            0.0
        );
    }

    #[test]
    fn test_passive_surveillance_only_finds_symptomatic_cases() -> Result<()> {
        let first_symptom_onset = 0.5;
        let campaign_start_time = 1.0;
        let second_symptom_onset = 1.5;
        let detection_delay = 0.5;

        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Time {
                    time: NonNegativeFinite::try_from(campaign_start_time)?,
                }),
                distribution: Some(fixed(0.0)),
            },
            passive_detection_probability: probability(1.0),
            active_detection_probability: probability(0.0),
            passive_detection_delay_distribution: fixed(detection_delay),
            ..Default::default()
        });

        let early_symptoms_person = context.add_entity(Person).unwrap();
        let late_symptoms_person = context.add_entity(Person).unwrap();

        context.add_plan(first_symptom_onset, move |context| {
            context.set_property(early_symptoms_person, InfectionStatus::Symptomatic);
        });
        context.add_plan(second_symptom_onset, move |context| {
            context.set_property(late_symptoms_person, InfectionStatus::Symptomatic);
        });

        context.subscribe_to_event(
            move |context, event: PropertyChangeEvent<Person, DetectionStatus>| {
                if event.current == DetectionStatus::Detected {
                    assert_eq!(
                        context.get_property::<Person, InfectionStatus>(event.entity_id),
                        InfectionStatus::Symptomatic
                    );
                    let detection_time = context.get_detection_time(event.entity_id).unwrap();
                    // Early symptomatic case should be detected after the campaign starts, but late symptomatic case should be detected after their symptoms start
                    if event.entity_id == early_symptoms_person {
                        assert_almost_eq!(
                            detection_time,
                            campaign_start_time + detection_delay,
                            0.0
                        );
                    } else if event.entity_id == late_symptoms_person {
                        assert_almost_eq!(
                            detection_time,
                            second_symptom_onset + detection_delay,
                            0.0
                        );
                    } else {
                        panic!("Unexpected person detected in test");
                    }
                } else {
                    panic!("Unexpected detection status change in test");
                }
            },
        );

        context.execute();
        Ok(())
    }

    #[test]
    fn test_surveillance_fails_with_presymptomatic_detections() -> Result<()> {
        let mut context = setup_context_with_campaign(ParameterValues {
            surveillance_campaign_delay: SurveillanceCampaignDelayConfig {
                deploy: true,
                trigger: Some(StateTrigger::Time {
                    time: NonNegativeFinite::try_from(1.0)?,
                }),
                distribution: Some(fixed(0.0)),
            },
            ..Default::default()
        });

        let _ = context
            .add_entity(with!(
                Person,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(0.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None,
                }
            ))
            .unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
            context.plan_surveillance_campaign()?;
            assert!(!context.surveillance_campaign_active());
            context.execute();
            Ok(())
        }));
        assert!(result.is_err());
        Ok(())
    }

    fn setup_default_campaign(
        r_0: f64,
        random_seed: u64,
        active_detection: f64,
        passive_detection: f64,
        generation_interval_distribution: ContinuousDistributionParameterized,
        campaign_delay: f64,
        detection_delay: f64,
    ) -> Context {
        // Sets up parameters for context, filling out defaults
        // Defaults are set in the parameters module or are implemented for each Type automatically in Rust
        // Floats and integers are set to 0 and Options are set to None, unless specified.
        let surveillance_campaign_delay = SurveillanceCampaignDelayConfig {
            deploy: true,
            trigger: Some(StateTrigger::Detection {
                count: PositiveCount::ONE,
            }),
            distribution: Some(fixed(campaign_delay)),
        };
        let params = ParameterValues {
            seed: random_seed,
            initialization: InfectionInitialization::from_spillover(0.0, 1),
            offspring_distribution: poisson(r_0),
            generation_interval_distribution,
            active_detection_probability: probability(active_detection),
            active_detection_delay_distribution: fixed(detection_delay),
            passive_detection_probability: probability(passive_detection),
            surveillance_campaign_delay,
            ..Default::default()
        };

        let mut context = Context::new();
        context.init_random(params.seed);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context
    }

    fn experiment_trace_contact(
        active_detection: f64,
        detection_status: DetectionStatus,
    ) -> Result<bool> {
        let mut context =
            setup_default_campaign(1.0, 42, active_detection, 0.0, fixed(1.0), 2.5, 1.0);
        let index_id = context
            .add_entity(with!(
                Person,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(0.0),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None
                }
            ))
            .unwrap();

        let secondary_id = context
            .add_entity(with!(
                Person,
                TransmissionChain(Some(TransmissionChainData::new_with_infector(
                    0.0,
                    index_id,
                    Some(0)
                )))
            ))
            .unwrap();

        context.trace_contacts(index_id)?;
        context.execute();

        Ok(context.get_property::<Person, DetectionStatus>(secondary_id) == detection_status)
    }

    #[test]
    fn test_trace_contact() -> Result<()> {
        assert!(experiment_trace_contact(1.0, DetectionStatus::Detected)?);

        assert!(experiment_trace_contact(0.0, DetectionStatus::Undetected)?);
        Ok(())
    }

    #[allow(clippy::cast_precision_loss)]
    fn trace_secondaries_experiment(
        active_detection: f64,
        r_0: f64,
        seed: u64,
        generation_interval_distribution: ContinuousDistributionParameterized,
        detection_delay: f64,
        campaign_delay: f64,
        index_detection_time: f64,
    ) -> Result<(f64, SurveillanceData)> {
        let mut context = setup_default_campaign(
            r_0,
            seed,
            active_detection,
            0.0,
            generation_interval_distribution,
            campaign_delay,
            detection_delay,
        );
        let index_id = context.add_entity(Person).unwrap();

        context.generate_secondaries(index_id);
        crate::detection_manager::init(&mut context)?;

        context.add_plan(index_detection_time, move |context| {
            context.set_property(index_id, InfectionStatus::Symptomatic);
            context.set_property(
                index_id,
                SurveillanceData::Detected {
                    detection_time: OrderedFloat(index_detection_time),
                    detection_type: SurveillanceType::Passive,
                    surveillance_contact_id: None,
                },
            );
        });

        context.execute();

        let detections = context.query_entity_count(with!(Person, DetectionStatus::Detected)) - 1;
        let secondaries =
            context.query_entity_count(with!(Person, PrimaryInfection(Some(index_id))));
        let secondary_contact = context
            .sample_entity(
                ActiveDetectionRng,
                with!(
                    Person,
                    PrimaryInfection(Some(index_id)),
                    DetectionStatus::Detected
                ),
            )
            .unwrap();
        let detection_data = context.get_property::<Person, SurveillanceData>(secondary_contact);
        Ok((detections as f64 / secondaries as f64, detection_data))
    }

    fn test_trace_secondaries_experiment(campaign_delay: f64) -> Result<()> {
        let n = 30;
        let active_detection = 0.25;
        let index_detection_time = 2.0;
        let generation_interval = 1.0;
        let detection_delay = 0.5;
        let expected_detection_time = index_detection_time + campaign_delay + detection_delay;

        for i in 0..n {
            let (observed_detection, observed_detection_data) = trace_secondaries_experiment(
                active_detection,
                1000.0,
                i,
                fixed(generation_interval),
                detection_delay,
                campaign_delay,
                index_detection_time,
            )?;
            // Active detection probability is independent of campaign start time
            assert_almost_eq!(observed_detection, active_detection, 5e-2);
            // Timing of detections depends on campaign start time
            match observed_detection_data {
                SurveillanceData::Detected {
                    detection_time,
                    detection_type,
                    ..
                } => {
                    assert_almost_eq!(detection_time.into_inner(), expected_detection_time, 0.0);
                    assert_eq!(detection_type, SurveillanceType::Active);
                }
                SurveillanceData::Undetected { .. } => {
                    panic!("Expected a detected contact from experiment")
                }
            };
        }
        Ok(())
    }

    #[test]
    fn test_trace_secondaries_experiment_campaign_timing() -> Result<()> {
        // No delay from first detected case
        test_trace_secondaries_experiment(0.0)?;
        // Five day delay to start of surveillance from first detected case
        test_trace_secondaries_experiment(5.0)?;
        Ok(())
    }

    #[test]
    fn test_trace_secondaries_experiment_create_after() -> Result<()> {
        let n = 30;
        let active_detection = 0.25;

        for i in 0..n {
            let (observed_detection, _observed_detection_data) = trace_secondaries_experiment(
                active_detection,
                1000.0,
                i,
                uniform(1.0, 3.0),
                1.0,
                0.0,
                2.0,
            )?;
            assert_almost_eq!(observed_detection, active_detection, 5e-2);
        }
        Ok(())
    }
}
