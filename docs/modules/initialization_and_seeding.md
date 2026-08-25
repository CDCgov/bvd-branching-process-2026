# Initialization and Seeding

**Source files:** `src/infection_initialization.rs`, `src/importation.rs`

---

## Overview

Before the epidemic can unfold, the simulation must be seeded with one or more index cases. Two seeding mechanisms are available, configured through the `initialization` block in the parameter file. They are mutually exclusive: a simulation uses either a discrete spillover event or a continuous hazard-rate-driven importation process.

---

## Infection initialization (`infection_initialization.rs`)

### Purpose

This module reads the seeding configuration and sets up the initial infectious persons or the importation process at the start of the simulation.

### Seeding modes

#### Spillover event

A spillover event places a fixed number of infectious persons into the simulation at a specific point in time (measured in days since the simulation start date).

```
At time = days_since_start:
    For i in 1..exposures:
        Create a new Person (index case, generation 0)
```

Each index case generated this way immediately triggers the branching process and disease progression machinery. If `days_since_start` falls after `max_time`, the simulation ends before the spillover fires — the run simply produces no cases, which is a valid outcome for ABC (Approximate Bayesian Computation) calibration that draws spillover times from an unbounded prior.

#### Hazard rate importation

When a continuous importation hazard rate is configured, the simulation does not use a single spillover event. Instead, the `importation` module (described below) handles ongoing case introductions throughout the run.

#### None

No seeding; useful for testing or as a placeholder.

### Calendar date

Every `InfectionInitialization` includes a `start_date` (a calendar date in `YYYY-MM-DD` format). All simulation times are measured in days from this date, and the `timekeeping` module converts between simulation time and calendar dates for reports.

---

## Importation (`importation.rs`)

### Purpose

The importation module implements continuous case importation driven by a time-varying hazard rate. It uses the inverse transform method on the cumulative hazard to schedule the *next* importation event at the appropriate time.

### Algorithm

The method is equivalent to simulating a non-homogeneous Poisson process:

```
While within simulation time:
    Draw E ~ Exponential(1.0)       ← number of events to "spend"
    Find t such that ∫₀ᵗ λ(s) ds = E   ← using inverse cumulative rate
    If t is finite:
        Schedule creation of new index case at (current_time + t)
        At that time, repeat from the beginning
    Else:
        Stop (rate function is exhausted)
```

Where $\lambda(t)$ is the instantaneous hazard rate of a new importation at time $t$. In the current implementation, only a **fixed rate** (constant $\lambda$ over time) is supported, meaning exponential waiting times between events. The architecture allows for time-varying rate functions as data inputs.

The `ScaledRateFn` utility in `hazard_rates.rs` handles the shifting and scaling needed to evaluate the integral starting from the current elapsed time rather than from zero.

### Interaction with other modules

- Each imported case is created as a new `Person` with a `TransmissionChain` record indicating it is a primary case (no infector ID).
- The branching process immediately takes over and schedules secondary infections from the imported case.
- Importation events continue until the simulation stops.

---

## Rates module (`src/rates/`)

### Purpose

The `rates` module provides the mathematical infrastructure for hazard rate functions used by the importation module.

### Key types

| Type / Trait | Description |
|---|---|
| `HazardRateFn` | Trait defining `rate(t)`, `cum_rate(t)`, `inverse_cum_rate(events)` |
| `FixedRateFn` | Constant hazard rate over a fixed duration |
| `ScaledRateFn` | Wrapper that scales and time-shifts a base rate function |
| `RateFnType` | Serializable enum of supported rate function variants (`FixedRate`) |
| `RateFnId` | An index into the stored rate functions; used to retrieve a function from the context |

The `inverse_cum_rate` method is the key operation: given an expected number of events (drawn from an Exponential(1) distribution), it returns the time at which that many events will have accumulated, which is exactly the next event time in the non-homogeneous Poisson process.
