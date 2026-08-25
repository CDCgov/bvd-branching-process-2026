use anyhow::Result;
use ixa::{define_data_plugin, define_rng, Context, ContextRandomExt, PluginContext};
use serde::{Deserialize, Serialize};

use crate::distributions::DiscreteDistributionParameterized;
use crate::parameters::{ContextParametersExt, ParameterValues};
use crate::state_trigger::{ContextTriggerExt, StateTrigger};
use crate::PositiveFinite;

define_rng!(OffspringDistributionRng);

// Runtime flag set to `true` when an `OffspringIntervention::Enabled` fires.
define_data_plugin!(OffspringInterventionActive, bool, false);

// ── Unchecked deserialization type ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct UncheckedOffspringIntervention {
    deploy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trigger: Option<StateTrigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scalar: Option<PositiveFinite>,
}

impl From<OffspringIntervention> for UncheckedOffspringIntervention {
    fn from(v: OffspringIntervention) -> Self {
        match v {
            OffspringIntervention::Disabled => Self {
                deploy: false,
                trigger: None,
                scalar: None,
            },
            OffspringIntervention::Enabled { scalar, trigger } => Self {
                deploy: true,
                trigger: Some(trigger),
                scalar: Some(scalar),
            },
        }
    }
}

// ── Public strategy enum ──────────────────────────────────────────────────────

/// Controls if and when the offspring distribution mean is scaled.
///
/// | Variant | Meaning |
/// |---------|----------|
/// | `Disabled` | No scaling — baseline offspring distribution used throughout |
/// | `Enabled` | Scale by `scalar` when `trigger` fires |
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(
    try_from = "UncheckedOffspringIntervention",
    into = "UncheckedOffspringIntervention"
)]
pub enum OffspringIntervention {
    #[default]
    Disabled,
    Enabled {
        scalar: PositiveFinite,
        trigger: StateTrigger,
    },
}

impl TryFrom<UncheckedOffspringIntervention> for OffspringIntervention {
    type Error = anyhow::Error;

    fn try_from(raw: UncheckedOffspringIntervention) -> Result<Self> {
        if !raw.deploy {
            return Ok(Self::Disabled);
        }
        let trigger = raw.trigger.ok_or_else(|| {
            anyhow::anyhow!("OffspringIntervention: trigger must be set when deploy is true")
        })?;
        let scalar = raw.scalar.ok_or_else(|| {
            anyhow::anyhow!("OffspringIntervention: scalar must be set when deploy is true")
        })?;
        Ok(Self::Enabled { scalar, trigger })
    }
}

pub trait ContextOffspringExt: PluginContext + ContextRandomExt + ContextParametersExt {
    /// Returns the offspring distribution in effect for this timestep, applying
    /// the configured [`OffspringIntervention`] strategy when appropriate.
    fn effective_offspring_distribution(&self) -> DiscreteDistributionParameterized {
        let &ParameterValues {
            offspring_distribution,
            offspring_intervention,
            ..
        } = self.get_params();

        let maybe_scalar = match offspring_intervention {
            OffspringIntervention::Disabled => None,
            OffspringIntervention::Enabled { scalar, .. } => {
                (*self.get_data(OffspringInterventionActive)).then_some(scalar.into_inner())
            }
        };

        match maybe_scalar {
            Some(scalar) => offspring_distribution
                .with_scaled_mean(scalar)
                .expect("scaled offspring distribution should be valid"),
            None => offspring_distribution,
        }
    }

    /// Samples a secondary-case count from the effective offspring distribution.
    ///
    /// Uses a dedicated [`OffspringDistributionRng`] stream, independent of
    /// other RNG consumers in `branching_process`.
    fn sample_effective_offspring(&mut self) -> usize {
        let distr = self.effective_offspring_distribution();
        self.sample_distr(OffspringDistributionRng, distr)
    }
}

impl ContextOffspringExt for Context {}

// ── Module initializer ────────────────────────────────────────────────────────

/// Register any runtime trigger required by the configured [`OffspringIntervention`].
///
/// Call once during model initialisation (see `model::load_init_fns`).
pub fn init(context: &mut Context) {
    let &ParameterValues {
        offspring_intervention,
        ..
    } = context.get_params();

    if let OffspringIntervention::Enabled { trigger, .. } = offspring_intervention {
        context.register_triggered_event(trigger, |context| {
            *context.get_data_mut(OffspringInterventionActive) = true;
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::detection_manager::{ContextDetectionExt, SurveillanceCampaignDelayConfig};
    use crate::distributions::DiscreteDistributionParameterized;
    use crate::parameters::{ParameterValues, Parameters};
    use crate::state_trigger::StateTrigger;
    use ixa::ContextGlobalPropertiesExt;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn poisson(mean: f64) -> DiscreteDistributionParameterized {
        DiscreteDistributionParameterized::poisson(mean).expect("valid Poisson mean")
    }

    fn positive(value: f64) -> PositiveFinite {
        PositiveFinite::try_from(value).expect("valid positive finite value")
    }

    /// Minimal context with given intervention strategy and no campaign machinery wired up.
    /// `surveillance_campaign_active()` will return `false` unless explicitly activated.
    fn setup(offspring_intervention: OffspringIntervention) -> Context {
        let params = ParameterValues {
            offspring_distribution: poisson(2.0),
            offspring_intervention,
            ..Default::default()
        };
        let mut context = Context::new();
        context.init_random(0);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context
    }

    /// Context wired with `OnTrigger { trigger: SurveillanceCampaign }` and a
    /// surveillance campaign that fires at `t = 1.0`. Both triggers are registered
    /// but `execute()` has not yet been called.
    fn setup_with_surveillance_trigger(scalar: f64) -> Context {
        let surveillance_campaign_delay: SurveillanceCampaignDelayConfig =
            serde_json::from_str(
                r#"{"deploy":true,"trigger":{"Time":{"time":1.0}},"distribution":{"Fixed":{"delay":0.0}}}"#,
            )
            .expect("valid surveillance campaign delay config");
        let params = ParameterValues {
            offspring_distribution: poisson(2.0),
            offspring_intervention: OffspringIntervention::Enabled {
                scalar: positive(scalar),
                trigger: StateTrigger::SurveillanceCampaign,
            },
            surveillance_campaign_delay,
            ..Default::default()
        };
        let mut context = Context::new();
        context.init_random(0);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context.setup_surveillance_campaign().unwrap();
        init(&mut context);
        context
    }

    // ── Serde and validation ──────────────────────────────────────────────────

    #[test]
    fn test_deploy_false_deserializes_and_is_default() {
        let json = r#"{"deploy":false}"#;
        let v: OffspringIntervention = serde_json::from_str(json).unwrap();
        assert!(matches!(v, OffspringIntervention::Disabled));
        assert_eq!(serde_json::to_string(&v).unwrap(), json);
        assert!(matches!(
            OffspringIntervention::default(),
            OffspringIntervention::Disabled
        ));
    }

    #[test]
    fn test_on_trigger_round_trips() {
        let json = r#"{"deploy":true,"trigger":{"Time":{"time":5.0}},"scalar":0.5}"#;
        let v: OffspringIntervention = serde_json::from_str(json).unwrap();
        assert!(matches!(v, OffspringIntervention::Enabled { .. }));
        assert_eq!(serde_json::to_string(&v).unwrap(), json);
    }

    #[test]
    fn test_on_trigger_with_surveillance_campaign_trigger_round_trips() {
        let json = r#"{"deploy":true,"trigger":"SurveillanceCampaign","scalar":0.5}"#;
        let v: OffspringIntervention = serde_json::from_str(json).unwrap();
        assert!(matches!(
            v,
            OffspringIntervention::Enabled {
                trigger: StateTrigger::SurveillanceCampaign,
                ..
            }
        ));
        assert_eq!(serde_json::to_string(&v).unwrap(), json);
    }

    #[test]
    fn test_rejects_zero_scalar() {
        let err = serde_json::from_str::<OffspringIntervention>(
            r#"{"deploy":true,"trigger":"SurveillanceCampaign","scalar":0.0}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("greater than 0.0"), "got: {err}");
    }

    #[test]
    fn test_rejects_negative_scalar() {
        let err = serde_json::from_str::<OffspringIntervention>(
            r#"{"deploy":true,"trigger":{"Time":{"time":5.0}},"scalar":-1.0}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("greater than 0.0"), "got: {err}");
    }

    // ── OffspringIntervention::None ───────────────────────────────────────────

    #[test]
    fn test_none_always_uses_baseline() {
        let context = setup(OffspringIntervention::default());
        let baseline = context.get_params().offspring_distribution;
        assert_eq!(context.effective_offspring_distribution(), baseline);
    }

    // ── OffspringIntervention::OnTrigger ──────────────────────────────────────

    #[test]
    fn test_on_trigger_returns_baseline_before_trigger_fires() {
        let scalar = 0.5;
        let trigger = StateTrigger::time(1.0).unwrap();
        let mut context = setup(OffspringIntervention::Enabled {
            scalar: positive(scalar),
            trigger,
        });
        init(&mut context);

        let baseline = context.get_params().offspring_distribution;
        // The trigger has not yet fired.
        assert!(!*context.get_data(OffspringInterventionActive));
        assert_eq!(context.effective_offspring_distribution(), baseline);
    }

    #[test]
    fn test_on_trigger_scales_distribution_after_trigger_fires() {
        let scalar = 0.5;
        let trigger = StateTrigger::time(1.0).unwrap();
        let mut context = setup(OffspringIntervention::Enabled {
            scalar: positive(scalar),
            trigger,
        });
        init(&mut context);
        let baseline = context.get_params().offspring_distribution;
        let expected = baseline.with_scaled_mean(scalar).unwrap();

        context.execute(); // fires the time trigger at t = 1.0

        assert!(*context.get_data(OffspringInterventionActive));
        assert_eq!(context.effective_offspring_distribution(), expected);
    }

    #[test]
    fn test_on_trigger_fires_on_surveillance_campaign_start() {
        let scalar = 0.5;
        let mut context = setup_with_surveillance_trigger(scalar);
        let baseline = context.get_params().offspring_distribution;
        let expected = baseline.with_scaled_mean(scalar).unwrap();

        assert!(!*context.get_data(OffspringInterventionActive));
        assert_eq!(context.effective_offspring_distribution(), baseline);

        context.execute(); // surveillance campaign fires at t = 1.0

        assert!(*context.get_data(OffspringInterventionActive));
        assert_eq!(context.effective_offspring_distribution(), expected);
    }

    // ── init ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_init_does_not_set_flag_for_none() {
        let mut context = setup(OffspringIntervention::default());
        init(&mut context);
        context.execute();
        assert!(!*context.get_data(OffspringInterventionActive));
    }
}
