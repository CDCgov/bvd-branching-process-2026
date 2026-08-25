use super::*;
use crate::{NonNegativeFinite, PositiveFinite, Probability};
use anyhow::Result;
use ixa::assert_almost_eq;
use rand::{distr::Distribution, rngs::StdRng, SeedableRng};
use statrs::distribution::ContinuousCDF;

#[test]
fn test_serde_discrete_parameterized() -> Result<()> {
    let deserialized_poisson = serde_json::from_str::<DiscreteDistributionParameterized>(
        "{\"Poisson\": {\"mean\": 1.0}}",
    )?;
    let deserialized_negative_binomial = serde_json::from_str::<DiscreteDistributionParameterized>(
        "{\"NegativeBinomial\": { \"mean\": 1.0, \"concentration\": 1.0}}",
    )?;
    assert_eq!(
        deserialized_poisson,
        DiscreteDistributionParameterized::poisson(1.0)?
    );
    assert_eq!(
        deserialized_negative_binomial,
        DiscreteDistributionParameterized::negative_binomial(1.0, 1.0)?
    );
    assert_eq!(
        serde_json::to_value(deserialized_negative_binomial)?,
        serde_json::json!({"NegativeBinomial": {"mean": 1.0, "concentration": 1.0}})
    );
    assert_eq!(
        serde_json::to_value(deserialized_poisson)?,
        serde_json::json!({"Poisson": {"mean": 1.0}})
    );
    Ok(())
}

#[test]
fn test_serde_continuous_parameterized() -> Result<()> {
    let exp =
        serde_json::from_str::<ContinuousDistributionParameterized>("{\"Exp\": {\"rate\": 1.0}}")?;
    let gamma = serde_json::from_str::<ContinuousDistributionParameterized>(
        "{\"Gamma\": {\"shape\": 1.0, \"rate\": 2.0}}",
    )?;
    let fixed = serde_json::from_str::<ContinuousDistributionParameterized>(
        "{\"Fixed\": {\"delay\": 1.0}}",
    )?;
    let uniform = serde_json::from_str::<ContinuousDistributionParameterized>(
        "{\"Uniform\": {\"min\": 1.0, \"max\": 3.0}}",
    )?;
    let offset_weibull = serde_json::from_str::<ContinuousDistributionParameterized>(
        "{\"OffsetWeibull\": {\"shape\": 1.0, \"scale\": 2.0, \"offset\": 3.0}}",
    )?;
    assert_eq!(exp, ContinuousDistributionParameterized::exp(1.0)?);
    assert_eq!(gamma, ContinuousDistributionParameterized::gamma(1.0, 2.0)?);
    assert_eq!(fixed, ContinuousDistributionParameterized::fixed(1.0)?);
    assert_eq!(
        uniform,
        ContinuousDistributionParameterized::uniform(1.0, 3.0)?
    );
    assert_eq!(
        serde_json::to_value(fixed)?,
        serde_json::json!({"Fixed": {"delay": 1.0}})
    );
    assert_eq!(
        offset_weibull,
        ContinuousDistributionParameterized::offset_weibull(1.0, 2.0, 3.0)?
    );
    assert_eq!(
        serde_json::to_value(exp)?,
        serde_json::json!({"Exp": {"rate": 1.0}})
    );
    assert_eq!(
        serde_json::to_value(gamma)?,
        serde_json::json!({"Gamma": {"shape": 1.0, "rate": 2.0}})
    );
    assert_eq!(
        serde_json::to_value(uniform)?,
        serde_json::json!({"Uniform": {"min": 1.0, "max": 3.0}})
    );
    assert_eq!(
        serde_json::to_value(offset_weibull)?,
        serde_json::json!({"OffsetWeibull": {"shape": 1.0, "scale": 2.0, "offset": 3.0}})
    );
    Ok(())
}

#[test]
fn test_deserialize_rejects_invalid_distribution_params() {
    let invalid_discrete = [
        (
            "{\"Poisson\": {\"mean\": 0.0}}",
            "Poisson mean must be greater than 0.0",
        ),
        (
            "{\"NegativeBinomial\": {\"mean\": 1.0, \"concentration\": 0.0}}",
            "Negative binomial concentration must be greater than 0.0",
        ),
    ];
    for (raw, expected) in invalid_discrete {
        let e = serde_json::from_str::<DiscreteDistributionParameterized>(raw).unwrap_err();
        assert!(e.to_string().contains(expected), "got: {e}");
    }

    let invalid_continuous = [
        (
            "{\"Exp\": {\"rate\": 0.0}}",
            "Exp rate must be greater than 0.0",
        ),
        (
            "{\"Gamma\": {\"shape\": 0.0, \"rate\": 1.0}}",
            "Gamma shape must be greater than 0.0",
        ),
        (
            "{\"Fixed\": {\"delay\": -1.0}}",
            "Fixed delay must be greater than or equal to 0.0",
        ),
        (
            "{\"Uniform\": {\"min\": 2.0, \"max\": 2.0}}",
            "Uniform max must be greater than min",
        ),
        (
            "{\"OffsetWeibull\": {\"shape\": 1.0, \"scale\": 1.0, \"offset\": -1.0}}",
            "Weibull offset must be greater than or equal to 0.0",
        ),
    ];
    for (raw, expected) in invalid_continuous {
        let e = serde_json::from_str::<ContinuousDistributionParameterized>(raw).unwrap_err();
        assert!(e.to_string().contains(expected), "got: {e}");
    }
}

#[test]
fn test_constructors_reject_invalid_distribution_params() {
    assert!(DiscreteDistributionParameterized::poisson(f64::INFINITY).is_err());
    assert!(DiscreteDistributionParameterized::negative_binomial(1.0, f64::NAN).is_err());
    assert!(ContinuousDistributionParameterized::exp(f64::INFINITY).is_err());
    assert!(ContinuousDistributionParameterized::gamma(1.0, 0.0).is_err());
    assert!(ContinuousDistributionParameterized::fixed(-1.0).is_err());
    assert!(ContinuousDistributionParameterized::uniform(1.0, 1.0).is_err());
    assert!(ContinuousDistributionParameterized::offset_weibull(1.0, 1.0, f64::NAN).is_err());
}

#[test]
fn test_typed_params_constructors_accept_constrained_values() -> Result<()> {
    let positive = PositiveFinite::try_from(1.0)?;
    let delay = Delay::try_from(1.0)?;
    let offset = Offset::try_from(2.0)?;
    let min = NonNegativeFinite::try_from(0.0)?;
    let max = NonNegativeFinite::try_from(3.0)?;

    assert_eq!(PoissonParams::new(positive).mean(), 1.0);
    assert_eq!(NegativeBinomialParams::new(positive, positive).mean(), 1.0);
    assert_eq!(ExpParams::new(positive).rate(), 1.0);
    assert_eq!(GammaParams::new(positive, positive).shape(), 1.0);
    assert_eq!(FixedParams::new(delay).delay(), 1.0);
    assert_eq!(
        ContinuousDistributionParameterized::fixed_delay(delay),
        ContinuousDistributionParameterized::fixed(1.0)?
    );
    assert_eq!(UniformParams::new(min, max)?.max(), 3.0);
    assert_eq!(
        OffsetWeibullParams::new(positive, positive, offset).offset(),
        2.0
    );
    assert!(UniformParams::new(max, min).is_err());
    Ok(())
}

#[test]
fn test_continuous_distribution_typed_probability_methods() -> Result<()> {
    let distribution = ContinuousDistributionParameterized::uniform(0.0, 10.0)?;
    let probability = Probability::try_from(0.25)?;

    assert_eq!(distribution.probability_at(2.5), probability);
    assert_eq!(distribution.quantile(probability), 2.5);
    Ok(())
}

#[test]
#[allow(clippy::cast_lossless, clippy::cast_precision_loss)]
fn test_weibull_distribution() -> Result<()> {
    let shape = 2.0;
    let scale = 2.0;
    let offset = 3.0;
    let weibull = ContinuousDistributionParameterized::offset_weibull(shape, scale, offset)?;

    let seed = 42;
    let mut rng = StdRng::seed_from_u64(seed);
    let mut samples = Vec::new();

    let n = 1_000_000;
    for _ in 0..n {
        samples.push(weibull.sample(&mut rng));
    }

    let weibull_dist = statrs::distribution::Weibull::new(shape, scale).unwrap();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mut empirical_cdf = Vec::new();
    for (i, sample) in samples.iter().enumerate() {
        empirical_cdf.push((sample, (i + 1) as f64 / n as f64));
    }
    let tolerance = 1.0 / (0.001 * n as f64);
    for (sample, emp_cdf) in empirical_cdf {
        let theoretical_cdf = weibull_dist.cdf(sample - 3.0);
        assert_almost_eq!(emp_cdf, theoretical_cdf, tolerance);
    }
    Ok(())
}
