//! Wall-clock benchmark for a representative projection run.

use anyhow::Result;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use evdmodel::distributions::DiscreteDistributionParameterized;
use evdmodel::infection_initialization::InfectionInitialization;
use evdmodel::shutdown::EndRunConditions;
use evdmodel::{initialize_model, ParameterValues, Parameters, PositiveCount, Reports};
use ixa::{Context, ContextGlobalPropertiesExt};

/// Representative outbreak params
fn base_params() -> Result<ParameterValues> {
    let config = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/experiments/bvd_early_phase/default_params.json"
    );
    let mut params = Parameters::from_file(config)?;

    // Ensure take-off
    params.initialization = InfectionInitialization::from_spillover(0.0, 100);
    params.offspring_distribution =
        DiscreteDistributionParameterized::negative_binomial(2.0, 0.45)?;

    // No CSV I/0
    params.reports = Reports::disabled();

    // Death cap (set per-run in `run_projection`) is the only stop condition; clear any
    // max_time/max_cases/max_detections the config file may carry.
    params.end_run_conditions = EndRunConditions::default();

    Ok(params)
}

/// One full projection run, ending at exactly `max_deaths` deaths.
fn run_projection(base: &ParameterValues, seed: u64, max_deaths: usize) {
    let mut params = base.clone();
    params.seed = seed;
    params.end_run_conditions.max_deaths =
        Some(PositiveCount::try_from(max_deaths).expect("death cap should be positive"));

    let mut context = Context::new();
    context
        .set_global_property_value(Parameters, params.clone())
        .expect("parameters valid");
    initialize_model(&mut context, params.seed, params.end_run_conditions)
        .expect("model initializes");
    context.execute();
}

fn bench_projection(c: &mut Criterion) {
    let base = base_params().expect("benchmark parameters should load");
    const SEEDS: [u64; 3] = [1, 2, 3];

    let mut group = c.benchmark_group("projection_to_death_cap");
    group.sample_size(10);

    for max_deaths in [5_000usize, 20_000usize] {
        group.bench_with_input(
            BenchmarkId::from_parameter(max_deaths),
            &max_deaths,
            |b, &max_deaths| {
                b.iter(|| {
                    for seed in SEEDS {
                        run_projection(&base, seed, max_deaths);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_projection);
criterion_main!(benches);
