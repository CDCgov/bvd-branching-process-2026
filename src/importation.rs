use crate::detection_manager::{TransmissionChain, TransmissionChainData};
use crate::rates::{
    hazard_rates::{HazardRateFn, ScaledRateFn},
    rate_fn_storage::{load_rate_fn, HazardRateExt, RateFnId},
    RateFnType,
};
use crate::Person;
use anyhow::{bail, Result};
use ixa::{define_rng, prelude::*, Context, ContextRandomExt};
use statrs::distribution::Exp;

// Specify multiple offspring distributions
struct ExternalInfectionEvent {
    // hazard: f64,
    event_time: f64,
}

define_rng!(HazardRng);

fn get_external_infection(
    context: &Context,
    hazard_rate: &dyn HazardRateFn,
) -> Option<ExternalInfectionEvent> {
    let elapsed = context.get_current_time();
    let total_rate_fn = ScaledRateFn::new(hazard_rate, 1.0, elapsed);

    // Draw an exponential and use that to determine the next time
    let exp = Exp::new(1.0).unwrap();
    let expected_cases = context.sample_distr(HazardRng, exp);

    if let Some(t) = total_rate_fn.inverse_cum_rate(expected_cases) {
        let _hazard = total_rate_fn.rate(t);
        let event_time = elapsed + t;

        Some(ExternalInfectionEvent {
            // hazard,
            event_time,
        })
    } else {
        None
    }
}

fn external_hazard_loop(context: &mut Context, hazard_rate_id: RateFnId) {
    let hazard_rate = context.get_rate_fn(hazard_rate_id);

    if let Some(import) = get_external_infection(context, hazard_rate) {
        context.add_plan(import.event_time, move |context| {
            context
                .add_entity(with!(
                    Person,
                    TransmissionChain(Some(TransmissionChainData::new(import.event_time)))
                ))
                .unwrap();
            external_hazard_loop(context, hazard_rate_id);
        });
    }
}

pub fn initiate_importation(context: &mut Context, rate_fn: RateFnType) -> Result<()> {
    if let Some(id) = load_rate_fn(context, rate_fn)? {
        external_hazard_loop(context, id);
    } else {
        bail!("Failed to load hazard rate function for hazard rate importation");
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::{external_hazard_loop, load_rate_fn};
    use crate::infection_initialization::InfectionInitialization;
    use crate::parameters::{ParameterValues, Parameters};
    use crate::shutdown::EndRunConditions;
    use crate::Person;
    use ixa::assert_almost_eq;
    use ixa::{Context, ContextEntitiesExt, ContextGlobalPropertiesExt, ContextRandomExt};
    use statrs::distribution::{ContinuousCDF, Discrete, Poisson};

    fn simulate_poisson_proc_hazard(rate: f64, max_time: f64, seed: u64) -> usize {
        let mut context = Context::new();

        // Sets up parameters for context, filling out defaults
        // Defaults are set in the parameters module or are implemented for each Type automatically in Rust
        // Floats and integers are set to 0 and Options are set to None, unless specified.
        let params = ParameterValues {
            initialization: InfectionInitialization::from_hazard_rate(rate),
            end_run_conditions: EndRunConditions::from_max_time(max_time),
            ..Default::default()
        };
        let hazard_rate_fn = params.initialization.clone().get_rate_fn().unwrap();
        context
            .set_global_property_value(Parameters, params)
            .unwrap();

        context.init_random(seed);
        let rate_id = load_rate_fn(&mut context, hazard_rate_fn).unwrap();
        external_hazard_loop(&mut context, rate_id.unwrap());
        context.execute();

        context.get_entity_count::<Person>()
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::cast_lossless, clippy::cast_precision_loss)]
    fn test_external_hazard_poisson_distribution() {
        let rate = 5.0;
        let max_time = 100.0;
        let sim_count = 10_000;
        let expected_mean = rate * max_time;

        let mut results = Vec::new();
        for i in 0..sim_count {
            let population = simulate_poisson_proc_hazard(rate, max_time, i);
            results.push(population);
        }

        // Calculate the sample mean
        let sample_mean: f64 = results.iter().sum::<usize>() as f64 / results.len() as f64;

        // Assert that the sample mean is close to the expected mean
        assert_almost_eq!(sample_mean, expected_mean, rate / 2.0);

        // Perform a chi-squared goodness-of-fit test
        let poisson = Poisson::new(expected_mean).unwrap();
        let mut observed_counts = vec![0; 1000];
        let mut expected_counts = vec![0.0; 1000];

        for count in results {
            if count < observed_counts.len() {
                observed_counts[count] += 1;
            }
        }

        for (i, value) in expected_counts.iter_mut().enumerate() {
            *value = poisson.pmf(i as u64) * sim_count as f64;
        }

        let chisq_stat: f64 = observed_counts
            .iter()
            .zip(expected_counts.iter())
            .filter(|(_, &expected)| expected > 0.0)
            .map(|(&observed, &expected)| {
                let diff = f64::from(observed) - expected;
                diff * diff / expected
            })
            .sum();

        let degrees_of_freedom = observed_counts.len() - 1;
        let critical_value = statrs::distribution::ChiSquared::new(degrees_of_freedom as f64)
            .unwrap()
            .inverse_cdf(0.95);

        assert!(
            chisq_stat < critical_value,
            "{}",
            format!("Chi-squared test failed: stat = {chisq_stat}, critical = {critical_value}")
        );
    }
}
