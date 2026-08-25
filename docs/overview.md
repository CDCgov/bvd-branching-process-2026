# Model Overview

This document describes the overall structure of the VHF (Viral Hemorrhagic Fever) branching process model. The model was developed to support EVD (Ebolavirus Disease) outbreak response and analysis, simulating epidemic dynamics under various intervention scenarios.

## What the model does

The model is a **stochastic, individual-based branching process** that tracks simulated persons through disease states, surveillance pathways, clinical care, and vaccination. Each simulated run uses a random seed for reproducibility. The model time unit is **days**, anchored to a calendar start date.

The model is built on the [ixa](https://github.com/CDCgov/ixa) simulation framework. The core loop is event-driven: each person generates future events (infection attempts, disease progression, detection, etc.) that are scheduled on a timeline and executed in order.

---

## Module map

The table below summarizes the purpose of each source module and links to detailed documentation.

| Module(s) | Purpose | Documentation |
|---|---|---|
| `model.rs`, `main.rs` | Entry point; wires together all sub-modules | [Simulation Control](modules/simulation_control.md) |
| `parameters.rs` | Global configuration loaded from JSON | [Simulation Control](modules/simulation_control.md) |
| `branching_process.rs` | Samples offspring counts and schedules infection attempts | [Transmission](modules/transmission.md) |
| `transmission_manager.rs` | Evaluates transmission events, applies interventions, creates contacts | [Transmission](modules/transmission.md) |
| `disease_manager.rs` | Disease progression (Presymptomatic → Symptomatic → Removed/Dead) | [Disease Progression](modules/disease_progression.md) |
| `clinical_manager.rs` | Health settings, hospitalization, ETU transfer, case status | [Disease Progression](modules/disease_progression.md) |
| `detection_manager.rs` | Active and passive surveillance, contact tracing | [Surveillance & Detection](modules/surveillance_and_detection.md) |
| `case_confirmation.rs` | Laboratory testing queue and capacity | [Surveillance & Detection](modules/surveillance_and_detection.md) |
| `infection_initialization.rs` | Seeds the outbreak (spillover event or continuous importation) | [Initialization & Seeding](modules/initialization_and_seeding.md) |
| `importation.rs` | Hazard-rate-driven continuous importation | [Initialization & Seeding](modules/initialization_and_seeding.md) |
| `vaccination.rs`, `vaccination_campaign.rs` | Ring and geographic vaccination campaigns | [Vaccination](modules/vaccination.md) |
| `shutdown.rs`, `state_trigger.rs` | Stop conditions and threshold-based event triggers | [Simulation Control](modules/simulation_control.md) |
| `timekeeping.rs` | Calendar date ↔ simulation time conversion | [Simulation Control](modules/simulation_control.md) |
| `distributions/` | Parameterized probability distributions | [Distributions & Rates](modules/distributions_and_rates.md) |
| `rates/` | Hazard rate functions for continuous importation | [Distributions & Rates](modules/distributions_and_rates.md) |
| `reports/` | Output report generation | [Reports](modules/reports.md) |
| `validation.rs` | Constrained numeric types (`Probability`, `PositiveFinite`, etc.) | [Simulation Control](modules/simulation_control.md) |

---

## Simulation lifecycle

A single simulation run proceeds as follows:

```
Load parameters from JSON config
Initialize random number generator (seed)
Seed outbreak (spillover event or continuous importation)
  └─► For each seeded person:
        Schedule infection attempts (branching process)
        Schedule disease progression (latent → infectious → removed)
        Schedule surveillance checks (passive/active detection)
        Schedule clinical pathway (hospitalization → ETU)
        Schedule vaccination assessment
Run event loop until stop condition is met
  [stop conditions: max time, max cases, max deaths, max detections]
Write output reports
```

---

## Person states

Every person in the simulation carries several concurrent state variables:

| State property | Possible values |
|---|---|
| **InfectionStatus** | `Presymptomatic`, `Symptomatic`, `Removed`, `Susceptible`, `Vaccinated` |
| **TreatmentLocation** | `None` (community), `Quarantine`, `Clinic`, `EbolaTreatmentUnit` |
| **CaseStatus** | `None`, `Suspected`, `Confirmed` |
| **SurveillanceData** | `Undetected` (with optional attempt times), `Detected` (with detection time and type) |
| **VaccineData** | `Unvaccinated`, `Vaccinated` (with inoculation time, campaign type, efficacy outcome) |
| **Alive** | `true` / `false` |

States change in response to scheduled plans and event subscriptions. For example, when a person becomes `Symptomatic`, the clinical manager is notified and may schedule hospitalization.

---

## Key parameters

All parameters are loaded from a JSON configuration file at runtime. The full parameter set is described in [`parameters.rs`](../src/parameters.rs). The most important epidemiological parameters are:

| Parameter | Description |
|---|---|
| `offspring_distribution` | Distribution of secondary cases per infectious individual (Poisson or Negative Binomial) |
| `generation_interval_distribution` | Distribution of time from exposure to infectiousness (continuous) |
| `presymptomatic_transmission_probability` | Fraction of the generation interval that occurs before symptom onset |
| `case_fatality_ratio` | Probability that a symptomatic case dies |
| `hospitalization_probability` | Probability that a symptomatic case is hospitalized |
| `active_detection_probability` | Probability that a contact of a detected case is traced |
| `passive_detection_probability` | Probability that a symptomatic community case presents for care |
| `quarantine_transmission_probability` | Transmission probability while in quarantine (relative modifier) |
| `etu_transmission_probability` | Transmission probability while in an ETU |

See [Simulation Control](modules/simulation_control.md) for the full list.
