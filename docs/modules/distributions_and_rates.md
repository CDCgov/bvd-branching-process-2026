# Distributions and Rates

**Source files:** `src/distributions/`, `src/rates/`

---

## Overview

The `distributions` module provides a unified interface for probability distributions used throughout the model. The `rates` module provides hazard rate functions for the continuous importation mechanism. Both are parameterized via JSON configuration, validated on load, and can be sampled during simulation.

---

## Distributions (`src/distributions/`)

### Purpose

Distributions are used for all stochastic delay times and count variables in the model, such as generation intervals, disease progression delays, detection delays, vaccination protection delays, etc. Each distribution is fully specified by its parameters in the JSON configuration file.

### Continuous distributions

`ContinuousDistributionParameterized` is a single type that wraps any of the supported continuous distributions. It supports:
- Sampling (drawing a random value)
- CDF evaluation (probability that a random variable is ≤ x)
- Inverse CDF / quantile function (finding x such that CDF(x) = p)

The quantile function is used in `disease_manager` to locate symptom onset within the generation interval.

| Variant | Parameters | Notes |
|---|---|---|
| `Exp` | `rate` (λ > 0) | Mean = 1/λ |
| `Gamma` | `shape`, `rate` | Mean = shape/rate; flexible for skewed delays |
| `Fixed` | `delay` (≥ 0) | Deterministic; always returns the same value |
| `Uniform` | `min`, `max` | Equal probability across an interval |
| `OffsetWeibull` | `shape`, `scale`, `offset` | Weibull shifted right by `offset`; useful for delays with a minimum incubation time |

#### Offset Weibull

The offset Weibull is a three-parameter family that shifts a standard Weibull distribution by a constant, such that

$$X = \text{Weibull}(\text{shape}, \text{scale}) + \text{offset}.$$

This is useful for modeling delays that cannot be shorter than some biological minimum (e.g., a minimum incubation period). The CDF and inverse CDF are implemented by applying the offset after the standard Weibull computation.

### Discrete distributions

`DiscreteDistributionParameterized` wraps the distributions used for offspring counts (secondary cases per index).

| Variant | Parameters | Notes |
|---|---|---|
| `Poisson` | `mean` (λ > 0) | Variance = mean; no superspreading |
| `NegativeBinomial` | `mean`, `concentration` (k > 0) | Variance = mean + mean²/k; lower k = more overdispersion/superspreading |

#### Negative Binomial parameterization

The model uses a mean–concentration parameterization:

$$p = \frac{k}{k + \mu}, \quad r = k$$

where $\mu$ is the mean and $k$ is the concentration (dispersion) parameter. This is converted internally to the shape–probability parameterization used by the underlying statistical library. The probability of extinction in a single generation is:

$$P(\text{offspring} = 0) = \left(\frac{k}{k + \mu}\right)^k$$.

---

## Rates (`src/rates/`)

### Purpose

Hazard rate functions describe the instantaneous rate of case importation as a function of time. They are used exclusively by the `importation` module to drive continuous, non-homogeneous Poisson importation.

### `HazardRateFn` trait

All rate functions implement three operations:

| Method | Description |
|---|---|
| `rate(t)` | Instantaneous rate λ(t) at time t |
| `cum_rate(t)` | Cumulative rate ∫₀ᵗ λ(s) ds |
| `inverse_cum_rate(events)` | Inverse of cumulative rate: time at which `events` expected events have accumulated |

The `inverse_cum_rate` is the critical method for the non-homogeneous Poisson simulation algorithm. It allows the simulation to directly compute the next event time without iterating over small time steps.

### `FixedRateFn`

A constant rate λ over a fixed duration $[0, T]$:

$$\lambda(t) = \lambda \text{ for } t \leq T, \quad 0 \text{ otherwise}$$

$$\int_0^t \lambda(s)\, ds = \lambda \cdot \min(t, T)$$

$$\text{inverse\_cum\_rate}(E) = E / \lambda \quad \text{(if } E/\lambda \leq T\text{)}$$

### `ScaledRateFn`

A utility wrapper (`ScaledRateFn`) shifts and scales an underlying rate function. It is used to evaluate the integral starting from the current elapsed time rather than from the beginning, without modifying the underlying function:

$$\lambda_\text{scaled}(t) = \lambda_\text{base}(t + \text{elapsed}) \times \text{scale}$$.

This allows the importation loop to draw the time to the *next* event from the current time without creating a new rate function object.

### `RateFnType` and storage

`RateFnType` is the serializable enum of supported rate function types (currently `FixedRate { rate }`). At runtime, rate functions are stored in the simulation context by `RateFnId`, an index into a list of boxed `HazardRateFn` trait objects. This design supports adding new time-varying rate function types without changing the importation module.
