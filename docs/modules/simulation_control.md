# Simulation Control

**Source files:** `src/model.rs`, `src/main.rs`, `src/parameters.rs`, `src/shutdown.rs`, `src/state_trigger.rs`, `src/timekeeping.rs`, `src/validation.rs`

---

## Overview

These modules handle simulation setup, configuration management, stop conditions, threshold-based event triggers, and calendar time. They form the scaffolding that ties together the epidemiological modules.

---

## Entry point and initialization (`model.rs`, `main.rs`)

The simulation binary (`main.rs`) uses the `ixa` framework's `run_with_args` function, which reads a `--config` argument pointing to a JSON parameter file. The configuration is parsed and validated before the simulation starts.

`model.rs` exposes two key functions:

- **`load_init_fns(context)`** — registers all sub-module initialization functions in the correct order:
  1. `reports` — sets up output file handles
  2. `disease_manager` — registers disease progression event listeners
  3. `branching_process` — registers the offspring-generation listener
  4. `detection_manager` — registers surveillance event listeners
  5. `infection_initialization` — seeds the outbreak
  6. `clinical_manager` — registers healthcare pathway listeners
  7. `case_confirmation` — registers testing queue
  8. `vaccination` — registers vaccination campaign logic
  9. `transmission_manager` — registers transmission event listeners

- **`initialize_model(context, seed, end_run_conditions)`** — calls `load_init_fns`, seeds the random number generator, and schedules the simulation shutdown.

---

## Parameters (`parameters.rs`)

### Purpose

All epidemiological and operational parameters are stored in a single `ParameterValues` struct, which is loaded from a JSON configuration file at startup and stored as a global property accessible from anywhere in the simulation.

### Loading

The config file must contain a JSON object with a key `"evdmodel.Parameters"` whose value is the parameter block. Parameters are validated on load; invalid values (e.g., negative probabilities, missing required fields) cause the simulation to fail with a descriptive error.

### Full parameter list

| Parameter | Type | Description |
|---|---|---|
| `seed` | Integer | Random number generator seed |
| `initialization` | `InfectionInitialization` | Outbreak seeding configuration (start date, spillover or hazard rate) |
| `track_susceptibles` | Boolean | Whether to create explicit `Susceptible` persons for averted contacts (useful for comparing the real-world number of individuals in isolation) |
| `offspring_distribution` | Discrete distribution | Secondary case distribution (Poisson or Negative Binomial) |
| `generation_interval_distribution` | Continuous distribution | Distribution of time between successive infections |
| `presymptomatic_transmission_probability` | Probability | Fraction of transmission occurring before symptom onset |
| `hospitalization_probability` | Probability | Probability a symptomatic case presents to a clinic |
| `hospitalization_delay_distribution` | Continuous distribution | Time from symptom onset to clinic presentation |
| `testing_config` | `TestingConfig` | Lab testing deployment, delay, and capacity |
| `etu_transfer_delay` | Float (days) | Delay between case confirmation and ETU admission |
| `quarantine_transmission_probability` | Probability | Relative transmission probability while in quarantine |
| `clinical_transmission_probability` | Probability | Relative transmission probability while in a clinic |
| `etu_transmission_probability` | Probability | Relative transmission probability while in an ETU |
| `active_detection_probability` | Probability | Probability of successfully tracing a forward contact in the branching process |
| `passive_detection_probability` | Probability | Probability of a symptomatic individual detected in the community |
| `active_detection_delay_distribution` | Continuous distribution | Time from index detection to contact detection |
| `passive_detection_delay_distribution` | Continuous distribution | Time from symptom onset to passive detection |
| `surveillance_campaign_delay` | `SurveillanceCampaignDelayConfig` | Trigger and delay for activating enhanced contact tracing |
| `recovery_delay_distribution` | Continuous distribution | Time from symptom onset to recovery (for survivors) |
| `case_fatality_ratio` | Probability | Probability that a symptomatic case dies |
| `mortality_delay_distribution` | Continuous distribution | Time from symptom onset to death (for decedents) |
| `reports` | `Reports` | Output report configuration |
| `vaccination` | `Vaccination` | Vaccine and campaign configuration |
| `shared_index_contact_probability` | Probability | Probability a tertiary case is also a direct contact of the index |
| `end_run_conditions` | `EndRunConditions` | Stop conditions (max time, max cases, max deaths, max detections) |

---

## Shutdown and stop conditions (`shutdown.rs`)

### Purpose

The simulation can be stopped by any of four conditions. These are defined in `EndRunConditions` and checked throughout the run.

| Condition | Parameter | Description |
|---|---|---|
| Time limit | `max_time` | Stop at this number of days since start |
| Case threshold | `max_cases` | Stop when cumulative symptomatic cases reaches this count |
| Death threshold | `max_deaths` | Stop when total deaths reaches this count |
| Detection threshold | `max_detections` | Stop when total detected cases reaches this count |

All four conditions are optional except `max_time`, which must always be set. The first condition that fires ends the simulation. The shutdown mechanism emits a `ShutdownEvent` that allows reports to flush their data before the process exits.

---

## State triggers (`state_trigger.rs`)

### Purpose

State triggers are threshold-based events that can activate other simulation features (e.g., starting a surveillance campaign, launching a vaccination campaign). They are reusable — any configurable deployment can accept a `StateTrigger`.

### Trigger types

| Trigger | Fires when... |
|---|---|
| `Detection { count }` | The number of detected cases reaches `count` |
| `Deaths { count }` | The number of deaths reaches `count` |
| `Cases { count }` | Cumulative symptomatic cases reaches `count` |
| `Time { time }` | Simulation time reaches `time` (days since start) |
| `Date { date }` | Calendar date reaches `date` (YYYY-MM-DD) |

Each trigger fires **at most once**, even if the underlying count later decreases (which cannot happen) or the subscription fires multiple times.

---

## Timekeeping (`timekeeping.rs`)

### Purpose

The `TimeKeeper` type bridges calendar dates and simulation time. The simulation runs in fractional days counted from the `start_date` defined in `initialization`. Calendar dates appear in reports and can be used as trigger conditions.

### Key operations

| Operation | Description |
|---|---|
| `get_current_date()` | Returns the calendar date corresponding to the current simulation time |
| `get_date_at_time(t)` | Converts a simulation time `t` to a calendar date |
| `add_plan_from_timekeeper(date, callback)` | Schedules a callback to fire on a specific calendar date |
| `time_since_start(start_date)` | Computes the number of days between two calendar dates |

---

## Validation (`validation.rs`)

### Purpose

The validation module defines constrained numeric types that prevent invalid parameter values from entering the simulation. These types replace raw `f64` or `usize` values in all parameter structs.

| Type | Constraint |
|---|---|
| `Probability` | Float in [0.0, 1.0] |
| `PositiveFinite` | Finite float > 0.0 |
| `NonNegativeFinite` | Finite float ≥ 0.0 |
| `PositiveCount` | Integer ≥ 1 |

All of these types fail with a descriptive error message at deserialization time if an invalid value is provided, rather than silently producing incorrect results during the simulation.
