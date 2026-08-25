use crate::case_confirmation::TestingConfig;
use crate::detection_manager::SurveillanceCampaignDelayConfig;
use crate::distributions::{
    ContinuousDistributionParameterized, DiscreteDistributionParameterized,
};
use crate::infection_initialization::InfectionInitialization;
use crate::offspring_distribution::OffspringIntervention;
use crate::reports::Reports;
use crate::shutdown::EndRunConditions;
use crate::vaccination_campaign::Vaccination;
use crate::Probability;
use anyhow::{Context as _, Result};
use ixa::{define_global_property, ContextGlobalPropertiesExt, HashMap, PluginContext};
use serde::{Deserialize, Serialize};

// Define global properties from args
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct ParameterValues {
    pub seed: u64,
    pub initialization: InfectionInitialization,
    pub track_susceptibles: bool,
    pub offspring_distribution: DiscreteDistributionParameterized,
    pub generation_interval_distribution: ContinuousDistributionParameterized,
    pub presymptomatic_transmission_probability: Probability,
    pub hospitalization_probability: Probability,
    pub hospitalization_delay_distribution: ContinuousDistributionParameterized,
    pub testing_config: TestingConfig,
    pub etu_transfer_delay: f64,
    pub quarantine_transmission_probability: Probability,
    pub clinical_transmission_probability: Probability,
    pub etu_transmission_probability: Probability,
    pub active_detection_probability: Probability,
    pub passive_detection_probability: Probability,
    pub active_detection_delay_distribution: ContinuousDistributionParameterized,
    pub passive_detection_delay_distribution: ContinuousDistributionParameterized,
    pub surveillance_campaign_delay: SurveillanceCampaignDelayConfig,
    pub recovery_delay_distribution: ContinuousDistributionParameterized,
    pub case_fatality_ratio: Probability,
    pub mortality_delay_distribution: ContinuousDistributionParameterized,
    #[serde(flatten)]
    pub reports: Reports,
    pub vaccination: Vaccination,
    pub shared_index_contact_probability: Probability,
    pub end_run_conditions: EndRunConditions,
    /// Strategy controlling if and when the offspring distribution mean is scaled.
    /// Defaults to `deploy: false` (no scaling) when absent from config.
    #[serde(default)]
    pub offspring_intervention: OffspringIntervention,
}

fn validate_inputs(p: &ParameterValues) -> Result<()> {
    p.surveillance_campaign_delay.validate()?;
    p.initialization.validate()?;

    Ok(())
}

/// Thin shim so the validator signature matches what `define_global_property!` expects:
/// `Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>`.
fn validate_inputs_for_property(
    p: &ParameterValues,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    validate_inputs(p).map_err(Into::into)
}

define_global_property!(Parameters, ParameterValues, validate_inputs_for_property);

impl Parameters {
    /// The key this global property is registered under in a config file, derived the same
    /// way ixa derives it (`<crate>.Parameters`)
    fn config_key() -> String {
        format!("{}.Parameters", module_path!().split("::").next().unwrap())
    }

    /// Load and validate [`ParameterValues`] from a JSON config file
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<ParameterValues> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading params file {}", path.display()))?;
        Self::from_json_str(&raw)
    }

    /// Build params from JSON text
    pub fn from_json_str(raw: &str) -> Result<ParameterValues> {
        let key = Self::config_key();
        let mut config: HashMap<String, serde_json::Value> =
            serde_json::from_str(raw).context("parsing params config JSON")?;
        let value = config
            .remove(&key)
            .with_context(|| format!("config missing `{key}` block"))?;
        let params: ParameterValues =
            serde_json::from_value(value).with_context(|| format!("deserializing `{key}`"))?;
        validate_inputs(&params)?;
        Ok(params)
    }
}

pub trait ContextParametersExt: PluginContext + ContextGlobalPropertiesExt {
    fn get_params(&self) -> &ParameterValues {
        self.get_global_property_value(Parameters)
            .expect("Expected Parameters to be set")
    }
}
impl ContextParametersExt for ixa::Context {}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn from_json_str_rejects_invalid_offspring_distribution() {
        let raw = include_str!("../input/input.json")
            .replace("\"concentration\": 0.45", "\"concentration\": 0.0");
        let e = match Parameters::from_json_str(&raw) {
            Ok(_) => panic!("expected invalid offspring distribution to fail"),
            Err(e) => e,
        };
        let msg = format!("{e:?}");
        assert!(msg.contains("concentration"), "got: {msg}");
    }

    #[test]
    fn from_json_str_rejects_invalid_probability() {
        let raw = include_str!("../input/input.json").replace(
            "\"active_detection_probability\": 0.8",
            "\"active_detection_probability\": 1.5",
        );
        let e = match Parameters::from_json_str(&raw) {
            Ok(_) => panic!("expected invalid probability to fail"),
            Err(e) => e,
        };
        let msg = format!("{e:?}");
        assert!(msg.contains("between 0.0 and 1.0"), "got: {msg}");
    }

    #[test]
    fn from_json_str_reads_the_parameters_block() {
        let raw = include_str!("../input/input.json");
        let params = Parameters::from_json_str(raw).expect("loads default params");
        // `#[serde(flatten)]` must read the report keys into `reports`, not drop them to default.
        assert!(params.reports.prevalence_report.write);
    }

    #[test]
    fn from_json_str_errors_when_block_missing() {
        let err = match Parameters::from_json_str("{}") {
            Ok(_) => panic!("expected an error for an empty config"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("missing"), "got: {err}");
    }
}
