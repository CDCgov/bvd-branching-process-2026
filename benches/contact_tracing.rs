//! Wall-clock benchmark for contact tracing, the path the `PrimaryInfection` index
//! exists to keep sub-quadratic.
//!
//! `PrimaryInfection` is derived from `TransmissionChain`, so every scenario pays the
//! same per-infection index writes; they differ in how much tracing reads the index.
//!
//! | scenario              | campaign starts | index reads |
//! |-----------------------|-----------------|-------------|
//! | `no_tracing`          | never           | none        |
//! | `no_tracing_low_disp` | never           | none        |
//! | `late_campaign`       | 50% of cap      | some        |
//! | `early_campaign`      | 10% of cap      | many        |
//!
//! The index is built lazily, in the campaign-start plan rather than in `init`, so
//! `no_tracing` never pays for it. To re-measure that trade:
//! ```text
//! cargo bench --bench contact_tracing -- --save-baseline lazy
//! # move `index_property::<Person, PrimaryInfection>()` back to detection_manager::init,
//! # or drop it entirely
//! cargo bench --bench contact_tracing -- --baseline lazy
//! ```

use anyhow::Result;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use evdmodel::detection_manager::SurveillanceCampaignDelayConfig;
use evdmodel::distributions::DiscreteDistributionParameterized;
use evdmodel::infection_initialization::InfectionInitialization;
use evdmodel::shutdown::EndRunConditions;
use evdmodel::{
    initialize_model, ParameterValues, Parameters, PositiveCount, Probability, Reports,
};
use ixa::{Context, ContextGlobalPropertiesExt};
use std::time::Duration;

const SEEDS: [u64; 2] = [1, 2];

/// 15_000 is the sitrep calibration cap in `3_observed_data_config.json`.
const OUTBREAK_SIZES: [usize; 2] = [2_000, 15_000];

fn campaign(spec: &str) -> Result<SurveillanceCampaignDelayConfig> {
    Ok(serde_json::from_str(spec)?)
}

/// A case-count trigger rather than the config's calendar date, so the traced fraction
/// of a run is the same at every outbreak size.
fn campaign_at_cases(count: usize) -> Result<SurveillanceCampaignDelayConfig> {
    campaign(&format!(
        r#"{{"deploy": true, "trigger": {{"Cases": {{"count": {count}}}}}, "distribution": {{"Fixed": {{"delay": 0.0}}}}}}"#
    ))
}

fn base_params() -> Result<ParameterValues> {
    let config = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/experiments/sitrep_validation/default_ixa_config.json"
    );
    let mut params = Parameters::from_file(config)?;

    // A single spillover goes extinct ~40% of the time at concentration 0.45
    params.initialization = InfectionInitialization::from_spillover(0.0, 100);
    params.reports = Reports::disabled();
    params.end_run_conditions = EndRunConditions::default();

    Ok(params)
}

fn scenarios(
    base: &ParameterValues,
    max_cases: usize,
) -> Result<Vec<(&'static str, ParameterValues)>> {
    let mut no_tracing = base.clone();
    no_tracing.surveillance_campaign_delay = campaign(r#"{"deploy": false}"#)?;
    no_tracing.passive_detection_probability = Probability::ZERO;
    no_tracing.active_detection_probability = Probability::ZERO;

    let mut no_tracing_low_disp = no_tracing.clone();
    no_tracing_low_disp.offspring_distribution =
        DiscreteDistributionParameterized::negative_binomial(2.88, 100.0)?;

    let mut late_campaign = base.clone();
    late_campaign.surveillance_campaign_delay = campaign_at_cases(max_cases / 2)?;

    // Earlier than this and tracing suppresses the outbreak before it reaches the cap.
    let mut early_campaign = base.clone();
    early_campaign.surveillance_campaign_delay = campaign_at_cases(max_cases / 10)?;

    Ok(vec![
        ("no_tracing", no_tracing),
        ("no_tracing_low_disp", no_tracing_low_disp),
        ("late_campaign", late_campaign),
        ("early_campaign", early_campaign),
    ])
}

fn run_outbreak(base: &ParameterValues, seed: u64, max_cases: usize) {
    let mut params = base.clone();
    params.seed = seed;
    params.end_run_conditions.max_cases =
        Some(PositiveCount::try_from(max_cases).expect("case cap should be positive"));

    let mut context = Context::new();
    context
        .set_global_property_value(Parameters, params.clone())
        .expect("parameters valid");
    initialize_model(&mut context, params.seed, params.end_run_conditions)
        .expect("model initializes");
    context.execute();
}

fn bench_contact_tracing(c: &mut Criterion) {
    let base = base_params().expect("benchmark parameters should load");

    for max_cases in OUTBREAK_SIZES {
        for (name, params) in scenarios(&base, max_cases).expect("scenarios should build") {
            let mut group = c.benchmark_group(name);
            group.sample_size(10);
            group.warm_up_time(Duration::from_millis(300));
            group.measurement_time(Duration::from_secs(1));
            group.bench_with_input(
                BenchmarkId::from_parameter(max_cases),
                &max_cases,
                |b, &max_cases| {
                    b.iter(|| {
                        for seed in SEEDS {
                            run_outbreak(&params, seed, max_cases);
                        }
                    });
                },
            );
            group.finish();
        }
    }
}

criterion_group!(benches, bench_contact_tracing);
criterion_main!(benches);
