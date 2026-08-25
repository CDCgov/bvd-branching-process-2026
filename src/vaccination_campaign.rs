use anyhow::{ensure, Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{
    distributions::ContinuousDistributionParameterized, state_trigger::StateTrigger,
    NonNegativeFinite, Probability,
};

#[derive(Deserialize, Serialize)]
struct UncheckedVaccineConfig {
    available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    efficacy: Option<Probability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protection_delay_distribution: Option<ContinuousDistributionParameterized>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vaccination_campaigns: Option<UncheckedVaccinationCampaigns>,
}

#[derive(Deserialize, Serialize, Clone)]
struct VaccinationScheduleData {
    #[serde(skip_serializing_if = "Option::is_none")]
    distribution: Option<ContinuousDistributionParameterized>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<PathBuf>,
}

#[derive(Deserialize, Serialize)]
struct UncheckedVaccinationCampaigns {
    geographic: GeographicCampaignConfig,
    ring: RingCampaignConfig,
}

#[derive(Deserialize, Serialize)]
struct GeographicCampaignConfig {
    deploy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger: Option<StateTrigger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_delay: Option<NonNegativeFinite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_prevalence: Option<Probability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schedule: Option<VaccinationScheduleData>,
}

#[derive(Deserialize, Serialize)]
struct RingCampaignConfig {
    deploy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger: Option<StateTrigger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_delay: Option<NonNegativeFinite>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Vaccination {
    #[default]
    Unavailable,
    Available {
        efficacy: Probability,
        protection_delay_distribution: ContinuousDistributionParameterized,
        ring_vaccination_campaign: RingVaccinationCampaign,
        geo_vaccination_campaign: GeographicVaccinationCampaign,
    },
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum VaccinationCampaignType {
    Ring,
    Geographic,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum GeographicVaccinationCampaign {
    #[default]
    Disabled,
    Enabled {
        trigger: StateTrigger,
        delay: NonNegativeFinite,
        max_prevalence: Probability,
        schedule: VaccinationSchedule,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum RingVaccinationCampaign {
    #[default]
    Disabled,
    Enabled {
        trigger: StateTrigger,
        delay: NonNegativeFinite,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum VaccinationSchedule {
    Empirical {
        weights: Vec<usize>,
        filename: PathBuf,
    },
    Parameterized {
        distribution: ContinuousDistributionParameterized,
    },
    Unlimited,
}

#[derive(Deserialize, Debug)]
struct ScheduleRecord {
    day: usize,
    count: usize,
}

fn read_empirical_vaccination_schedule_from_file(path: &PathBuf) -> Result<VaccinationSchedule> {
    let mut reader = csv::Reader::from_path(path).with_context(|| {
        format!(
            "Failed to open vaccination schedule file: {}",
            path.display()
        )
    })?;

    let headers = reader
        .byte_headers()
        .with_context(|| "Vaccination schedule CSV must have headers (day, count)")?
        .clone();
    let mut raw_record = csv::ByteRecord::new();

    let mut weights = Vec::new();

    let mut day = 0;
    let mut previous_observed_day = 0;
    while reader.read_byte_record(&mut raw_record)? {
        let record: ScheduleRecord = raw_record.deserialize(Some(&headers))?;
        if day > 0 {
            ensure!(
                record.day > previous_observed_day,
                "Days in deployment schedule data must be in ascending order and unique"
            );
        }
        previous_observed_day = record.day;
        // Handle gaps in the schedule day record by filling zero values
        while day < record.day {
            weights.push(0);
            day += 1;
        }
        weights.push(record.count);
        day += 1;
    }

    ensure!(
        !weights.is_empty(),
        "Vaccination schedule file must contain at least one record"
    );
    ensure!(
        weights.iter().any(|w| *w > 0),
        "Vaccination schedule must have at least one non-zero weight"
    );

    Ok(VaccinationSchedule::Empirical {
        weights,
        filename: path.clone(),
    })
}

impl TryFrom<VaccinationScheduleData> for VaccinationSchedule {
    type Error = anyhow::Error;

    fn try_from(data: VaccinationScheduleData) -> Result<Self> {
        ensure!(
            data.distribution.is_some() || data.filename.is_some(),
            "Neither distribution nor filename provided; one must be specified"
        );
        ensure!(
            data.distribution.is_none() || data.filename.is_none(),
            "Both distribution and filename provided; only one must be specified"
        );

        if let Some(distribution) = data.distribution {
            Ok(VaccinationSchedule::Parameterized { distribution })
        } else if let Some(filename) = data.filename {
            Ok(read_empirical_vaccination_schedule_from_file(&filename)?)
        } else {
            unreachable!("Either distribution or filename must be provided");
        }
    }
}

impl TryFrom<GeographicCampaignConfig> for GeographicVaccinationCampaign {
    type Error = anyhow::Error;

    fn try_from(config: GeographicCampaignConfig) -> Result<Self, Self::Error> {
        if !config.deploy {
            return Ok(GeographicVaccinationCampaign::Disabled);
        }

        let trigger = config.trigger.with_context(|| {
            "Geographic campaign trigger must be provided when geographic vaccination is deployed"
        })?;
        let delay = config
            .start_delay
            .with_context(|| "Geographic campaign start delay must be provided when geographic vaccination is deployed")?;
        let max_prevalence = config
            .max_prevalence
            .with_context(|| "Geographic campaign max prevalence must be provided when geographic vaccination is deployed")?;
        let schedule = config.schedule.with_context(|| {
            "Geographic campaign schedule must be provided when geographic vaccination is deployed"
        })?;

        Ok(GeographicVaccinationCampaign::Enabled {
            trigger,
            delay,
            max_prevalence,
            schedule: VaccinationSchedule::try_from(schedule)?,
        })
    }
}

impl TryFrom<RingCampaignConfig> for RingVaccinationCampaign {
    type Error = anyhow::Error;

    fn try_from(config: RingCampaignConfig) -> Result<Self, Self::Error> {
        if !config.deploy {
            return Ok(RingVaccinationCampaign::Disabled);
        }

        let trigger = config.trigger.with_context(|| {
            "Ring campaign trigger must be provided when ring vaccination is deployed"
        })?;
        let delay = config.start_delay.with_context(|| {
            "Ring campaign start delay must be provided when ring vaccination is deployed"
        })?;

        Ok(RingVaccinationCampaign::Enabled { trigger, delay })
    }
}

impl TryFrom<UncheckedVaccineConfig> for Vaccination {
    type Error = anyhow::Error;

    fn try_from(config: UncheckedVaccineConfig) -> Result<Self, Self::Error> {
        if !config.available {
            return Ok(Vaccination::Unavailable);
        }

        let efficacy = config
            .efficacy
            .with_context(|| "Efficacy must be provided when vaccine is available")?;
        let protection_delay_distribution =
            config.protection_delay_distribution.with_context(|| {
                "Protection delay distribution must be provided when vaccine is available"
            })?;

        let campaigns = config
            .vaccination_campaigns
            .with_context(|| "Vaccination campaigns must be provided when vaccine is available")?;

        let ring_vaccination_campaign = RingVaccinationCampaign::try_from(campaigns.ring)
            .with_context(|| "Failed to parse ring vaccination campaign configuration")?;

        let geo_vaccination_campaign =
            GeographicVaccinationCampaign::try_from(campaigns.geographic)
                .with_context(|| "Failed to parse geographic vaccination campaign configuration")?;

        Ok(Vaccination::Available {
            efficacy,
            protection_delay_distribution,
            ring_vaccination_campaign,
            geo_vaccination_campaign,
        })
    }
}

impl Vaccination {
    pub fn geographic_campaign(&self) -> Option<&GeographicVaccinationCampaign> {
        match self {
            Vaccination::Unavailable => None,
            Vaccination::Available {
                geo_vaccination_campaign,
                ..
            } => Some(geo_vaccination_campaign),
        }
    }
    pub fn ring_campaign(&self) -> Option<&RingVaccinationCampaign> {
        match self {
            Vaccination::Unavailable => None,
            Vaccination::Available {
                ring_vaccination_campaign,
                ..
            } => Some(ring_vaccination_campaign),
        }
    }
}

impl<'de> Deserialize<'de> for Vaccination {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let unchecked_config =
            UncheckedVaccineConfig::deserialize(deserializer).map_err(serde::de::Error::custom)?;
        Vaccination::try_from(unchecked_config).map_err(serde::de::Error::custom)
    }
}

impl From<&Vaccination> for UncheckedVaccineConfig {
    fn from(vaccination: &Vaccination) -> Self {
        match vaccination {
            Vaccination::Unavailable => UncheckedVaccineConfig {
                available: false,
                efficacy: None,
                protection_delay_distribution: None,
                vaccination_campaigns: None,
            },
            Vaccination::Available {
                efficacy,
                protection_delay_distribution,
                ring_vaccination_campaign,
                geo_vaccination_campaign,
            } => {
                let ring_config = match ring_vaccination_campaign {
                    RingVaccinationCampaign::Disabled => RingCampaignConfig {
                        deploy: false,
                        trigger: None,
                        start_delay: None,
                    },
                    RingVaccinationCampaign::Enabled { trigger, delay, .. } => RingCampaignConfig {
                        deploy: true,
                        trigger: Some(*trigger),
                        start_delay: Some(*delay),
                    },
                };

                let geo_config = match geo_vaccination_campaign {
                    GeographicVaccinationCampaign::Disabled => GeographicCampaignConfig {
                        deploy: false,
                        trigger: None,
                        start_delay: None,
                        max_prevalence: None,
                        schedule: None,
                    },
                    GeographicVaccinationCampaign::Enabled {
                        trigger,
                        delay,
                        max_prevalence,
                        schedule,
                    } => {
                        let schedule = match schedule {
                            VaccinationSchedule::Unlimited => None,
                            VaccinationSchedule::Parameterized { distribution } => {
                                Some(VaccinationScheduleData {
                                    distribution: Some(*distribution),
                                    filename: None,
                                })
                            }
                            VaccinationSchedule::Empirical { filename, .. } => {
                                Some(VaccinationScheduleData {
                                    distribution: None,
                                    filename: Some(filename.clone()),
                                })
                            }
                        };

                        GeographicCampaignConfig {
                            deploy: true,
                            trigger: Some(*trigger),
                            start_delay: Some(*delay),
                            max_prevalence: Some(*max_prevalence),
                            schedule,
                        }
                    }
                };

                UncheckedVaccineConfig {
                    available: true,
                    efficacy: Some(*efficacy),
                    protection_delay_distribution: Some(*protection_delay_distribution),
                    vaccination_campaigns: Some(UncheckedVaccinationCampaigns {
                        geographic: geo_config,
                        ring: ring_config,
                    }),
                }
            }
        }
    }
}

impl Serialize for Vaccination {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        UncheckedVaccineConfig::from(self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::PositiveCount;

    #[test]
    fn test_vaccination_unavailable_parsing() {
        let json = r#"{"available": false}"#;
        let result: Result<Vaccination, _> = serde_json::from_str(json);
        assert!(result.is_ok());

        match result.unwrap() {
            Vaccination::Unavailable => (),
            _ => panic!("Expected Vaccination::Unavailable"),
        }
    }

    #[test]
    fn test_vaccination_available_missing_efficacy() {
        let json = r#"{"available": true}"#;
        let result: Result<Vaccination, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Efficacy must be provided"));
    }

    #[test]
    fn test_vaccination_available_missing_protection_delay() {
        let json = r#"{"available": true, "efficacy": 0.95}"#;
        let result: Result<Vaccination, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Protection delay distribution must be provided"));
    }

    #[test]
    fn test_vaccination_available_missing_campaigns() {
        let json = r#"{"available": true, "efficacy": 0.95, "protection_delay_distribution": {"Fixed": {"delay": 10.0}}}"#;
        let result: Result<Vaccination, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Vaccination campaigns must be provided"));
    }

    #[test]
    fn test_vaccination_schedule_neither_distribution_nor_filename() {
        let schedule_data = VaccinationScheduleData {
            distribution: None,
            filename: None,
        };
        let result = VaccinationSchedule::try_from(schedule_data);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Neither distribution nor filename provided"));
    }

    #[test]
    fn test_vaccination_schedule_both_distribution_and_filename() {
        let schedule_data = VaccinationScheduleData {
            distribution: Some(ContinuousDistributionParameterized::exp(0.1).unwrap()),
            filename: Some(PathBuf::from("/some/path.csv")),
        };
        let result = VaccinationSchedule::try_from(schedule_data);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Both distribution and filename provided"));
    }

    #[test]
    fn test_vaccination_campaign_type_serde() {
        let ring = VaccinationCampaignType::Ring;
        let json = serde_json::to_string(&ring).unwrap();
        let deserialized: VaccinationCampaignType = serde_json::from_str(&json).unwrap();
        assert_eq!(ring, deserialized);

        let geo = VaccinationCampaignType::Geographic;
        let json = serde_json::to_string(&geo).unwrap();
        let deserialized: VaccinationCampaignType = serde_json::from_str(&json).unwrap();
        assert_eq!(geo, deserialized);
    }

    #[test]
    fn test_vaccination_serde_round_trip() {
        let campaign = Vaccination::Available {
            efficacy: Probability::try_from(0.95).unwrap(),
            protection_delay_distribution: ContinuousDistributionParameterized::exp(0.1).unwrap(),
            ring_vaccination_campaign: RingVaccinationCampaign::Disabled,
            geo_vaccination_campaign: GeographicVaccinationCampaign::Enabled {
                trigger: StateTrigger::Cases {
                    count: PositiveCount::try_from(10).unwrap(),
                },
                delay: NonNegativeFinite::try_from(5.0).unwrap(),
                max_prevalence: Probability::try_from(0.5).unwrap(),
                schedule: VaccinationSchedule::Parameterized {
                    distribution: ContinuousDistributionParameterized::exp(0.1).unwrap(),
                },
            },
        };

        let json = serde_json::to_string(&campaign).unwrap();
        let deserialized: Vaccination = serde_json::from_str(&json).unwrap();
        assert_eq!(campaign, deserialized);
    }

    #[test]
    fn test_vaccination_default() {
        let vaccination = Vaccination::default();
        assert!(matches!(vaccination, Vaccination::Unavailable));
    }

    #[test]
    fn test_vaccination_get_campaign() {
        let vaccination = Vaccination::Unavailable;
        assert!(vaccination.ring_campaign().is_none());
        assert!(vaccination.geographic_campaign().is_none());
    }

    #[test]
    fn test_vaccination_schedule_unlimited() {
        let schedule = VaccinationSchedule::Unlimited;
        assert!(matches!(schedule, VaccinationSchedule::Unlimited));
    }

    #[test]
    fn test_vaccination_schedule_empirical() {
        let schedule = VaccinationSchedule::Empirical {
            weights: vec![10, 20, 30],
            filename: PathBuf::from("schedule.csv"),
        };
        assert!(matches!(schedule, VaccinationSchedule::Empirical { .. }));
    }

    #[test]
    fn test_vaccination_schedule_parameterized() {
        let schedule = VaccinationSchedule::Parameterized {
            distribution: ContinuousDistributionParameterized::exp(0.1).unwrap(),
        };
        assert!(matches!(
            schedule,
            VaccinationSchedule::Parameterized { .. }
        ));
    }

    #[test]
    fn test_vaccination_campaign_disabled() {
        let campaign = RingVaccinationCampaign::Disabled;
        assert_eq!(campaign, RingVaccinationCampaign::Disabled);
    }
}
