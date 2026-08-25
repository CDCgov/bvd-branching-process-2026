use crate::{
    detection_manager::{
        ContextDetectionExt, PrimaryInfection, SurveillanceData, SurveillanceType,
    },
    parameters::{ContextParametersExt, ParameterValues},
    state_trigger::StateTrigger,
    vaccination_campaign::{
        GeographicVaccinationCampaign, RingVaccinationCampaign, Vaccination,
        VaccinationCampaignType, VaccinationSchedule,
    },
};
use crate::{Person, PersonId};
use anyhow::{ensure, Context as _, Result};
use ixa::{
    define_data_plugin, define_rng, impl_property, prelude::*, rand::Rng, Context, HashMap,
    HashMapExt, PluginContext, RngId,
};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

define_rng!(RingVaccineRng);
define_rng!(GeoVaccineRng);
define_rng!(GeoVaccineEfficacyRng);
define_rng!(RingVaccineEfficacyRng);
define_rng!(SharedIndexForRingRng);

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum VaccineData {
    Vaccinated {
        inoculation_time: OrderedFloat<f64>,
        effective_time: Option<OrderedFloat<f64>>,
        campaign_deploy_method: VaccinationCampaignType,
        vaccine_success: bool,
    },
    Unvaccinated,
}

pub struct ContactInterventionData {
    pub vaccine_data: VaccineData,
    pub surveillance_data: SurveillanceData,
}

impl_property!(
    VaccineData,
    Person,
    default_const = VaccineData::Unvaccinated
);

enum CampaignState {
    Active { start_time: f64 },
    Inactive { start_time: Option<f64> },
}

struct VaccineCampaignData {
    campaign_data: HashMap<VaccinationCampaignType, CampaignState>,
    schedule_data: HashMap<VaccinationCampaignType, VaccinationSchedule>,
}

define_data_plugin!(
    VaccinePlugin,
    VaccineCampaignData,
    VaccineCampaignData {
        campaign_data: HashMap::new(),
        schedule_data: HashMap::new()
    }
);

fn get_earliest_inoculation_vaccine_data(
    updated_vaccine_data: VaccineData,
    current_vaccine_data: VaccineData,
) -> VaccineData {
    match (updated_vaccine_data, current_vaccine_data) {
        (
            VaccineData::Vaccinated {
                inoculation_time: updated_inoculation_time,
                ..
            },
            VaccineData::Vaccinated {
                inoculation_time: current_inoculation_time,
                ..
            },
        ) => {
            if updated_inoculation_time < current_inoculation_time {
                updated_vaccine_data
            } else {
                current_vaccine_data
            }
        }
        (VaccineData::Vaccinated { .. }, VaccineData::Unvaccinated) => updated_vaccine_data,
        (VaccineData::Unvaccinated, VaccineData::Vaccinated { .. }) => current_vaccine_data,
        (VaccineData::Unvaccinated, VaccineData::Unvaccinated) => VaccineData::Unvaccinated,
    }
}

pub trait ContextVaccineExt:
    PluginContext + ContextRandomExt + ContextEntitiesExt + ContextParametersExt + ContextDetectionExt
{
    fn sample_vaccine_protection_delay<R>(&self, rng: R) -> Option<f64>
    where
        R: RngId + 'static,
        R::RngType: Rng,
    {
        let ParameterValues { vaccination, .. } = self.get_params();

        match vaccination {
            Vaccination::Unavailable => (),
            Vaccination::Available {
                efficacy,
                protection_delay_distribution,
                ..
            } => {
                let success = self.sample_bool(rng, *efficacy);
                let delay = self.sample_distr(rng, *protection_delay_distribution);
                if success {
                    return Some(delay);
                }
            }
        }
        None
    }

    fn sample_geographic_vaccine_status<R>(&self, rng: R) -> bool
    where
        R: RngId + 'static,
        R::RngType: Rng,
    {
        let ParameterValues { vaccination, .. } = self.get_params();

        if let Some(campaign) = vaccination.geographic_campaign() {
            match campaign {
                GeographicVaccinationCampaign::Enabled { max_prevalence, .. } => {
                    return self.sample_bool(rng, *max_prevalence);
                }
                GeographicVaccinationCampaign::Disabled => (),
            }
        }
        false
    }

    fn sample_geographic_vaccination(&self) -> Result<VaccineData> {
        if self.vaccine_campaign_active(VaccinationCampaignType::Geographic) {
            let campaign_start = self
                .get_vaccine_campaign_start_time(VaccinationCampaignType::Geographic)
                .with_context(|| {
                    "Geographic campaign must be active to sample an inoculation time"
                })?;

            if self.sample_geographic_vaccine_status(GeoVaccineRng) {
                let inoculation_delay = self.sample_inoculation_delay(
                    GeoVaccineEfficacyRng,
                    VaccinationCampaignType::Geographic,
                )?;
                let inoculation_time = inoculation_delay + campaign_start;

                let protection_delay = self.sample_vaccine_protection_delay(GeoVaccineEfficacyRng);
                let effective_time =
                    protection_delay.map(|lag| OrderedFloat(inoculation_time + lag));
                return Ok(VaccineData::Vaccinated {
                    inoculation_time: OrderedFloat(inoculation_time),
                    effective_time,
                    campaign_deploy_method: VaccinationCampaignType::Geographic,
                    vaccine_success: protection_delay.is_some(),
                });
            }
        }

        Ok(VaccineData::Unvaccinated)
    }

    // Active contact tracing of uninitialized people occurs here as well
    fn get_earliest_surveillance_success(
        &self,
        infector: PersonId,
    ) -> Result<Option<SurveillanceData>> {
        let ParameterValues {
            shared_index_contact_probability,
            ..
        } = self.get_params();
        // Get the detection time for the direct infector case
        let surveillance_from_infector = if self.is_tracking_contacts_of(&infector) {
            self.sample_contact_trace_time(infector)?
                .map(|detection_time| SurveillanceData::Detected {
                    detection_time: OrderedFloat(detection_time),
                    detection_type: SurveillanceType::Active,
                    surveillance_contact_id: Some(infector),
                })
        } else {
            None
        };

        let surveillance_by_primary_index = if let Some(relative_index_case) =
            self.get_property::<Person, PrimaryInfection>(infector).0
        {
            if self.is_tracking_contacts_of(&relative_index_case)
                && self.sample_bool(SharedIndexForRingRng, *shared_index_contact_probability)
            {
                self.sample_contact_trace_time(relative_index_case)?
                    .map(|detection_time| SurveillanceData::Detected {
                        detection_time: OrderedFloat(detection_time),
                        detection_type: SurveillanceType::Active,
                        surveillance_contact_id: Some(relative_index_case),
                    })
            } else {
                None
            }
        } else {
            None
        };

        // Get earliest possible detection time and return the corresponding SurveillanceData Option
        let earliest_surveillance_success =
            match (surveillance_from_infector, surveillance_by_primary_index) {
                (
                    Some(SurveillanceData::Detected {
                        detection_time: infector_time,
                        ..
                    }),
                    Some(SurveillanceData::Detected {
                        detection_time: index_time,
                        ..
                    }),
                ) => {
                    if infector_time < index_time {
                        surveillance_from_infector
                    } else {
                        surveillance_by_primary_index
                    }
                }
                (Some(SurveillanceData::Detected { .. }), None) => surveillance_from_infector,
                (None, Some(SurveillanceData::Detected { .. })) => surveillance_by_primary_index,
                _ => None,
            };
        Ok(earliest_surveillance_success)
    }

    fn sample_contact_intervention_data(
        &self,
        infector: PersonId,
    ) -> Result<ContactInterventionData> {
        let default_vaccine_data =
            if self.vaccine_campaign_active(VaccinationCampaignType::Geographic) {
                self.sample_geographic_vaccination()?
            } else {
                VaccineData::Unvaccinated
            };

        if self.vaccine_campaign_active(VaccinationCampaignType::Ring) {
            match self.get_earliest_surveillance_success(infector)? {
                Some(SurveillanceData::Detected {
                    detection_time,
                    detection_type,
                    surveillance_contact_id,
                }) => {
                    // Sample vaccine efficacy and delay to protection
                    let protection_delay =
                        self.sample_vaccine_protection_delay(RingVaccineEfficacyRng);

                    // Assume inoculation occurs at detection through contact tracing
                    let effective_time =
                        protection_delay.map(|lag| OrderedFloat(detection_time.into_inner() + lag));

                    let ring_vaccine_data = VaccineData::Vaccinated {
                        inoculation_time: detection_time,
                        effective_time,
                        campaign_deploy_method: VaccinationCampaignType::Ring,
                        vaccine_success: protection_delay.is_some(),
                    };

                    let vaccine_data = get_earliest_inoculation_vaccine_data(
                        ring_vaccine_data,
                        default_vaccine_data,
                    );

                    return Ok(ContactInterventionData {
                        vaccine_data,
                        surveillance_data: SurveillanceData::Detected {
                            detection_time,
                            detection_type,
                            surveillance_contact_id,
                        },
                    });
                }
                _ => {
                    // Modify the detection data from default so that active detection is not attempted twice
                    return Ok(ContactInterventionData {
                        vaccine_data: default_vaccine_data,
                        surveillance_data: SurveillanceData::Undetected {
                            active_detection_attempt_time: Some(OrderedFloat(
                                self.get_current_time(),
                            )),
                            passive_detection_attempt_time: None,
                        },
                    });
                }
            }
        }

        Ok(ContactInterventionData {
            vaccine_data: default_vaccine_data,
            surveillance_data: SurveillanceData::Undetected {
                active_detection_attempt_time: None,
                passive_detection_attempt_time: None,
            },
        })
    }

    fn sample_inoculation_delay<R>(
        &self,
        rng: R,
        campaign_type: VaccinationCampaignType,
    ) -> Result<f64>
    where
        R: RngId + 'static,
        R::RngType: Rng,
    {
        let container = self.get_data(VaccinePlugin);
        let schedule = container.schedule_data.get(&campaign_type)
            .with_context(|| format!("No schedule found for campaign type {:?}. Must be assigned when the campaign is enabled.", campaign_type))?;
        let delay = match schedule {
            VaccinationSchedule::Empirical { weights, .. } => {
                let sampled_index = self.sample_weighted(rng, weights);
                sampled_index as f64
            }
            VaccinationSchedule::Parameterized { distribution } => {
                self.sample_distr(rng, distribution)
            }
            VaccinationSchedule::Unlimited => 0.0,
        };
        Ok(delay)
    }

    fn vaccine_campaign_active(&self, campaign_type: VaccinationCampaignType) -> bool {
        if let Some(campaign_data) = self
            .get_data(VaccinePlugin)
            .campaign_data
            .get(&campaign_type)
        {
            return match campaign_data {
                CampaignState::Active { .. } => true,
                CampaignState::Inactive { .. } => false,
            };
        }
        false
    }

    fn get_vaccine_protection(&self, vaccine_data: VaccineData) -> bool {
        match vaccine_data {
            VaccineData::Vaccinated { effective_time, .. } => {
                if let Some(protection_time) = effective_time.map(|t| t.into_inner()) {
                    return protection_time < self.get_current_time();
                }
            }
            VaccineData::Unvaccinated => (),
        }
        false
    }

    fn get_vaccine_campaign_start_time(
        &self,
        campaign_type: VaccinationCampaignType,
    ) -> Option<f64> {
        if let Some(campaign_data) = self
            .get_data(VaccinePlugin)
            .campaign_data
            .get(&campaign_type)
        {
            return match campaign_data {
                CampaignState::Active { start_time } => Some(*start_time),
                CampaignState::Inactive { start_time } => *start_time,
            };
        }
        None
    }

    fn plan_geographic_vaccination_campaign(&mut self) -> Result<()> {
        let ParameterValues { vaccination, .. } = self.get_params();
        if let Vaccination::Available {
            geo_vaccination_campaign,
            ..
        } = vaccination
        {
            if let GeographicVaccinationCampaign::Enabled {
                trigger,
                delay,
                schedule,
                ..
            } = geo_vaccination_campaign.clone()
            {
                self.register_vaccination_campaign(
                    VaccinationCampaignType::Geographic,
                    trigger,
                    delay.into_inner(),
                );
                self.register_vaccination_schedule(VaccinationCampaignType::Geographic, schedule)?;
                return Ok(());
            }
        }
        self.register_vaccination_campaign_inactive(VaccinationCampaignType::Geographic);
        Ok(())
    }

    fn plan_ring_vaccination_campaign(&mut self) {
        let ParameterValues { vaccination, .. } = self.get_params();
        if let Vaccination::Available {
            ring_vaccination_campaign: RingVaccinationCampaign::Enabled { trigger, delay },
            ..
        } = vaccination
        {
            self.register_vaccination_campaign(
                VaccinationCampaignType::Ring,
                *trigger,
                delay.into_inner(),
            );
            return;
        }
        self.register_vaccination_campaign_inactive(VaccinationCampaignType::Ring);
    }

    fn register_vaccination_campaign(
        &mut self,
        campaign_type: VaccinationCampaignType,
        trigger: StateTrigger,
        delay: f64,
    ) {
        self.register_triggered_event(trigger, move |context| {
            let start_time = context.get_current_time() + delay;
            let container = context.get_data_mut(VaccinePlugin);
            container.campaign_data.insert(
                campaign_type,
                CampaignState::Inactive {
                    start_time: Some(start_time),
                },
            );
            context.add_plan(start_time, move |context| {
                let container = context.get_data_mut(VaccinePlugin);
                container
                    .campaign_data
                    .entry(campaign_type)
                    .and_modify(|data| {
                        if let CampaignState::Inactive { .. } = data {
                            *data = CampaignState::Active { start_time };
                        }
                    });
            });
        });
    }

    fn register_vaccination_campaign_inactive(&mut self, campaign_type: VaccinationCampaignType) {
        let container = self.get_data_mut(VaccinePlugin);
        container
            .campaign_data
            .insert(campaign_type, CampaignState::Inactive { start_time: None });
    }

    fn register_vaccination_schedule(
        &mut self,
        campaign_type: VaccinationCampaignType,
        schedule: VaccinationSchedule,
    ) -> Result<()> {
        ensure!(campaign_type == VaccinationCampaignType::Geographic);
        let container = self.get_data_mut(VaccinePlugin);
        container.schedule_data.insert(campaign_type, schedule);
        Ok(())
    }
}
impl ContextVaccineExt for Context {}

pub fn init(context: &mut Context) -> Result<()> {
    context.plan_ring_vaccination_campaign();
    context.plan_geographic_vaccination_campaign()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use super::*;
    use crate::distributions::ContinuousDistributionParameterized;
    use crate::parameters::Parameters;
    use crate::shutdown::EndRunConditions;
    use crate::state_trigger::StateTrigger;
    use crate::Probability;
    use crate::{detection_manager::SurveillanceData, NonNegativeFinite};
    use anyhow::Result;
    use ixa::{assert_almost_eq, ContextGlobalPropertiesExt};

    fn setup(params: ParameterValues) -> Context {
        let mut context = Context::new();
        context.init_random(params.seed);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        let container = context.get_data_mut(VaccinePlugin);
        container.campaign_data.insert(
            VaccinationCampaignType::Ring,
            CampaignState::Active { start_time: 0.0 },
        );
        container.campaign_data.insert(
            VaccinationCampaignType::Geographic,
            CampaignState::Active { start_time: 0.0 },
        );
        context
    }

    fn fixed(delay: f64) -> ContinuousDistributionParameterized {
        ContinuousDistributionParameterized::fixed(delay).expect("valid fixed delay")
    }

    fn probability(value: f64) -> Probability {
        Probability::try_from(value).expect("valid probability")
    }

    #[test]
    fn test_get_earliest_inoculation_vaccine_data() {
        let updated_vaccine_data = VaccineData::Vaccinated {
            inoculation_time: OrderedFloat(1.0),
            effective_time: Some(OrderedFloat(2.0)),
            campaign_deploy_method: VaccinationCampaignType::Ring,
            vaccine_success: true,
        };
        let current_vaccine_data = VaccineData::Vaccinated {
            inoculation_time: OrderedFloat(2.0),
            effective_time: None,
            campaign_deploy_method: VaccinationCampaignType::Geographic,
            vaccine_success: false,
        };
        let result =
            get_earliest_inoculation_vaccine_data(updated_vaccine_data, current_vaccine_data);
        assert_eq!(
            result,
            VaccineData::Vaccinated {
                inoculation_time: OrderedFloat(1.0),
                effective_time: Some(OrderedFloat(2.0)),
                campaign_deploy_method: VaccinationCampaignType::Ring,
                vaccine_success: true
            }
        );
    }

    #[test]
    fn test_get_earliest_inoculation_vaccine_data_unvaccinated() {
        let updated_vaccine_data = VaccineData::Unvaccinated;
        let current_vaccine_data = VaccineData::Vaccinated {
            inoculation_time: OrderedFloat(2.0),
            effective_time: None,
            campaign_deploy_method: VaccinationCampaignType::Geographic,
            vaccine_success: false,
        };
        let result =
            get_earliest_inoculation_vaccine_data(updated_vaccine_data, current_vaccine_data);
        assert_eq!(
            result,
            VaccineData::Vaccinated {
                inoculation_time: OrderedFloat(2.0),
                effective_time: None,
                campaign_deploy_method: VaccinationCampaignType::Geographic,
                vaccine_success: false
            }
        );
    }

    #[test]
    fn test_get_earliest_inoculation_vaccine_data_both_unvaccinated() {
        let updated_vaccine_data = VaccineData::Unvaccinated;
        let current_vaccine_data = VaccineData::Unvaccinated;
        let result =
            get_earliest_inoculation_vaccine_data(updated_vaccine_data, current_vaccine_data);
        assert_eq!(result, VaccineData::Unvaccinated);
    }

    #[test]
    fn test_geographic_vaccination_only() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(1.0),
            vaccination: Vaccination::Available {
                efficacy: probability(0.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Disabled,
                geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                    trigger: StateTrigger::Time {
                        time: NonNegativeFinite::try_from(1.0)?,
                    },
                    delay: NonNegativeFinite::try_from(0.0)?,
                    max_prevalence: probability(1.0),
                    schedule: VaccinationSchedule::Parameterized {
                        distribution: fixed(1.0),
                    },
                },
            },
            ..Default::default()
        };
        let mut context = setup(params);
        init(&mut context).unwrap();

        let expected_vax_data = VaccineData::Vaccinated {
            inoculation_time: OrderedFloat(1.0),
            effective_time: None,
            campaign_deploy_method: VaccinationCampaignType::Geographic,
            vaccine_success: false,
        };

        let index = context.add_entity(Person).unwrap();
        let intervention_data = context.sample_contact_intervention_data(index)?;
        assert_eq!(intervention_data.vaccine_data, expected_vax_data);
        assert_eq!(
            intervention_data.surveillance_data,
            SurveillanceData::Undetected {
                passive_detection_attempt_time: None,
                active_detection_attempt_time: None
            }
        );
        Ok(())
    }

    #[test]
    fn test_detected_contact_intervention_data() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.5),
            vaccination: Vaccination::Available {
                efficacy: probability(1.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                    trigger: StateTrigger::Time {
                        time: NonNegativeFinite::try_from(0.0)?,
                    },
                    delay: NonNegativeFinite::try_from(0.0)?,
                },
                geo_vaccination_campaign: GeographicVaccinationCampaign::Disabled,
            },
            active_detection_probability: probability(1.0),
            active_detection_delay_distribution: fixed(1.0),
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

        let intervention_data = context.sample_contact_intervention_data(index)?;
        let expect = ContactInterventionData {
            vaccine_data: VaccineData::Vaccinated {
                inoculation_time: OrderedFloat(1.0),
                effective_time: Some(OrderedFloat(2.0)),
                campaign_deploy_method: VaccinationCampaignType::Ring,
                vaccine_success: true,
            },
            surveillance_data: SurveillanceData::Detected {
                detection_time: OrderedFloat(1.0),
                detection_type: SurveillanceType::Active,
                surveillance_contact_id: Some(index),
            },
        };

        assert_eq!(intervention_data.vaccine_data, expect.vaccine_data);
        assert_eq!(
            intervention_data.surveillance_data,
            expect.surveillance_data
        );
        Ok(())
    }

    #[test]
    fn test_undetected_contact_intervention_data() -> Result<()> {
        let params = ParameterValues {
            end_run_conditions: EndRunConditions::from_max_time(2.5),
            vaccination: Vaccination::Available {
                efficacy: probability(1.0),
                protection_delay_distribution: fixed(1.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Enabled {
                    trigger: StateTrigger::Time {
                        time: NonNegativeFinite::try_from(0.0)?,
                    },
                    delay: NonNegativeFinite::try_from(0.0)?,
                },
                geo_vaccination_campaign: GeographicVaccinationCampaign::Disabled,
            },
            active_detection_probability: probability(0.0),
            active_detection_delay_distribution: fixed(1.0),
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

        let intervention_data = context.sample_contact_intervention_data(index)?;
        let expect = ContactInterventionData {
            vaccine_data: VaccineData::Unvaccinated,
            surveillance_data: SurveillanceData::Undetected {
                active_detection_attempt_time: Some(OrderedFloat(0.0)),
                passive_detection_attempt_time: None,
            },
        };

        assert_eq!(intervention_data.vaccine_data, expect.vaccine_data);
        assert_eq!(
            intervention_data.surveillance_data,
            expect.surveillance_data
        );
        Ok(())
    }

    #[test]
    fn test_sample_inoculation_delay() {
        let n_replicates = 1000;
        // Expect zero fill for missing day data
        let schedule = VaccinationSchedule::Empirical {
            weights: vec![10, 30, 0, 10],
            filename: PathBuf::from(""),
        };
        let expected_proportion = [0.2, 0.6, 0.0, 0.2];
        let params = ParameterValues {
            vaccination: Vaccination::Available {
                efficacy: probability(1.0),
                protection_delay_distribution: fixed(0.0),
                ring_vaccination_campaign: RingVaccinationCampaign::Disabled,
                geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                    trigger: StateTrigger::Time {
                        time: NonNegativeFinite::try_from(0.0).unwrap(),
                    },
                    delay: NonNegativeFinite::try_from(0.0).unwrap(),
                    max_prevalence: probability(1.0),
                    schedule,
                },
            },
            ..Default::default()
        };
        let mut context = setup(params);
        init(&mut context).unwrap();
        let mut observed_counts = vec![0, 0, 0, 0];
        for _ in 0..n_replicates {
            let index = context
                .sample_inoculation_delay(GeoVaccineRng, VaccinationCampaignType::Geographic)
                .unwrap();
            observed_counts[index as usize] += 1;
        }

        for (i, observed) in observed_counts.into_iter().enumerate() {
            assert_almost_eq!(
                observed as f64 / n_replicates as f64,
                expected_proportion[i],
                0.05
            );
        }
    }
}
