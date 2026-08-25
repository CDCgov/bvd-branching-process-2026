use crate::offspring_distribution::ContextOffspringExt;
use crate::{Person, PersonId};
use ixa::{define_rng, prelude::*, Context, ContextRandomExt, PluginContext};

use crate::parameters::ContextParametersExt;

use crate::transmission_manager::infection_attempt;

define_rng!(BranchingProcessRng);

pub trait BranchingProcessExt:
    PluginContext + ContextEntitiesExt + ContextRandomExt + ContextParametersExt + ContextOffspringExt
{
    /// Generates secondary infections based on the offspring distribution.
    ///
    /// ## Parameters
    ///
    /// * `infector_id`: The identifier of the index case for the secondary infections.
    ///
    /// ## Details
    ///
    /// * Retrieves the offspring distribution from the global properties.
    /// * Samples the number of secondary infections from the offspring distribution.
    /// * For each secondary infection:
    ///   - Samples the generation interval from the generation interval distribution.
    ///   - Calculates the symptom onset time for the secondary infection.
    ///   - Schedules the addition of the secondary infection to the context at the calculated symptom onset time.
    fn generate_secondaries(&mut self, infector_id: PersonId) {
        let generation_interval_distribution = self.get_params().generation_interval_distribution;

        let offspring_count = self.sample_effective_offspring();

        for _ in 0..offspring_count {
            let infection_time = self.get_current_time()
                + self.sample_distr(BranchingProcessRng, generation_interval_distribution);
            self.add_plan(infection_time, move |context| {
                infection_attempt(context, infector_id).unwrap();
            });
        }
    }
}
impl BranchingProcessExt for Context {}

/// Initialize the branching process simulation and set global distributions for secondary cases and generation intervals.
pub fn init(context: &mut Context) {
    context.subscribe_to_event::<EntityCreatedEvent<Person>>(|context, event| {
        context.generate_secondaries(event.entity_id);
    });
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::distributions::DiscreteDistributionParameterized;
    use crate::infection_initialization::InfectionInitialization;
    use crate::parameters::{ParameterValues, Parameters};
    use ixa::{assert_almost_eq, ContextGlobalPropertiesExt};
    use statrs::distribution::{Discrete, Poisson};

    define_rng!(TestRng);

    fn setup(
        random_seed: u64,
        offspring_distribution: DiscreteDistributionParameterized,
    ) -> Context {
        // Sets up parameters for context, filling out defaults
        // Defaults are set in the parameters module or are implemented for each Type automatically in Rust
        // Floats and integers are set to 0 and Options are set to None, unless specified.
        let params = ParameterValues {
            seed: random_seed,
            initialization: InfectionInitialization::from_spillover(0.0, 1),
            offspring_distribution,
            ..Default::default()
        };
        let mut context = Context::new();
        context.init_random(params.seed);
        context
            .set_global_property_value(Parameters, params)
            .unwrap();
        context
    }

    fn poisson(mean: f64) -> DiscreteDistributionParameterized {
        DiscreteDistributionParameterized::poisson(mean).expect("valid Poisson mean")
    }

    fn negative_binomial(mean: f64, concentration: f64) -> DiscreteDistributionParameterized {
        DiscreteDistributionParameterized::negative_binomial(mean, concentration)
            .expect("valid negative binomial parameters")
    }

    fn generate_secondaries_poisson(r_0: f64, random_seed: u64) -> usize {
        let mut context = setup(random_seed, poisson(r_0));

        let index = context.add_entity(Person).unwrap();
        context.generate_secondaries(index);
        context.execute();
        context.get_entity_count::<Person>() - 1
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::cast_lossless)]
    fn test_generate_secondaries_poisson() {
        let r_0 = 2.0;
        let n = 100_000;
        let mut experiment = [0; 15];

        for seed in 0..n {
            let i = generate_secondaries_poisson(r_0, seed);
            experiment[i] += 1;
        }

        let theoretical = Poisson::new(r_0).unwrap();
        for (i, result) in experiment.iter().enumerate() {
            let observed = (*result as f64) / (n as f64);
            let expected = theoretical.pmf(i as u64);

            // Observed count matches expected PMF to accuracy of 1e-3
            assert_almost_eq!(observed, expected, 1e-2);
        }
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::cast_lossless)]
    fn test_negative_binomial_param_custom() {
        let r_0 = 10.0;
        let concentration = 0.5;
        let n = 100_000;
        let mut offspring_sum = 0;
        let mut zero_count = 0;

        let p = concentration / (r_0 + concentration);

        for i in 0..n {
            let context = setup(i, negative_binomial(r_0, concentration));
            let &ParameterValues {
                offspring_distribution,
                ..
            } = context.get_params();
            let offspring_count = context.sample_distr(TestRng, offspring_distribution);
            offspring_sum += offspring_count;
            if offspring_count == 0 {
                zero_count += 1;
            }
        }

        let observed_growth_rate = (offspring_sum as f64) / (n as f64);
        let branch_extinction_rate = (zero_count as f64) / (n as f64);
        let theoretical_extinction = p.powf(concentration);
        // let theroetical_variance = concentration * (1.0 - p) / (p * p);
        println!("Observed growth rate: {observed_growth_rate}, Expected: {r_0}");
        println!(
            "Branch extinction rate: {branch_extinction_rate}, Expected: {theoretical_extinction}"
        );
        assert_almost_eq!(observed_growth_rate, r_0, 1e-1);
        assert_almost_eq!(branch_extinction_rate, theoretical_extinction, 1e-2);
    }
}
