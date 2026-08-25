# Transmission

**Source files:** `src/branching_process.rs`, `src/transmission_manager.rs`

---

## Overview

Transmission is the engine of the epidemic model. Two modules work together: `branching_process` decides *how many* secondary infections a person could generate and *when*, while `transmission_manager` evaluates each infection attempt and decides *whether* a new person is actually infected, taking interventions into account.

---

## Branching process (`branching_process.rs`)

### Purpose

The branching process module implements the core epidemic mechanism. Each time a new person enters the simulation (regardless of how they were created), the module immediately draws a random number of secondary cases they will cause and schedules when those transmission attempts will occur.

### Key algorithm

When a new person is created:

```
Draw offspring_count ~ offspring_distribution
For i in 1..offspring_count:
    Draw infection_time ~ generation_interval_distribution
    Schedule infection_attempt(infector) at current_time + infection_time
```

The **offspring distribution** governs heterogeneity in transmission. It is configured as either:
- **Poisson(mean)** — homogeneous transmission, parameterized by the mean number of secondary cases R₀.
- **Negative Binomial(mean, concentration)** — overdispersed transmission, where a small fraction of individuals cause most infections (superspreading). The `concentration` (also called the dispersion parameter *k*) controls the degree of overdispersion; smaller values below one imply more superspreading.
- **Offspring Intervention scalar** — a scalar multiplier on the offspring distribution can be uniformly applied across all infectors at a certain trigger. The multiplier shifts the effective reproductive number. The true offspring mean is the product of the mean of the underlying distribution multiplied by the scalar, which must be above zero but can be greater than one, meaning it could feasibly increase transmission in the branching process.

The **generation interval distribution** is a continuous distribution (e.g., Gamma, offset Weibull) representing the time from one person's exposure to the moment they expose each of their secondary cases. It is *not* split explicitly into latent and infectious periods here — that splitting is handled in `disease_manager` using the `presymptomatic_transmission_probability` parameter to locate symptom onset within the generation interval.

### Interaction with other modules

Every infection attempt is passed to `transmission_manager.infection_attempt()`, which may prevent or modify it based on the infector's current health setting and vaccination status of the contact.

---

## Transmission manager (`transmission_manager.rs`)

### Purpose

The transmission manager evaluates each scheduled infection attempt, applies intervention logic, and creates new persons in the simulation when transmission succeeds.

### Key concepts

**Transmission attempt logic:**

```
Given an infector at an infection_attempt time:
  If infector is Presymptomatic or Symptomatic:
    1. Assess vaccine data for the potential contact
    2. Assess health-setting reduction:
       - Draw Bernoulli(transmission_probability_for_current_setting)
       - If success → transmission event occurs
       - If failure → case is "averted by health setting"; track susceptible if configured
    3. If transmission event:
       - If contact is vaccinated (geographic campaign) → create Vaccinated person
       - If contact will be detected in the future → create Presymptomatic + schedule surveillance update
       - Otherwise → create Presymptomatic contact with current surveillance data
```

The **transmission probability** that is used in step 2 depends on the infector's current `TreatmentLocation`:

| Treatment location | Parameter used |
|---|---|
| Community (none) | 1.0 (full probability) |
| Quarantine | `quarantine_transmission_probability` |
| Clinic | `clinical_transmission_probability` |
| ETU | `etu_transmission_probability` |

This means that isolating cases in treatment facilities directly reduces onward transmission in the model. Isolation or protection efficacy is equal to one minus the transmission probability in each setting.

### Contact intervention data

Before evaluating a transmission event, the module queries two concurrent intervention pathways for the potential contact:

1. **Vaccine intervention** — from `vaccination.rs`: has the contact received a vaccine, and has enough time elapsed for the vaccine to take effect?
2. **Surveillance intervention** — from `detection_manager.rs`: will the contact be detected and quarantined? If so, when?

If both apply, the vaccine takes precedence (preventing infection outright).

### Transmission chain tracking

Every person created by transmission carries a `TransmissionChain` record linking them to their infector, the time of infection, and the infector's generation number. Index cases (seeded by initialization) are generation 0; their contacts are generation 1; and so on. This chain underpins the transmission report and Rₜ calculation.

### Intervention counters

Two counters are maintained throughout the simulation:
- **vaccine_cases_averted** — cases prevented because the contact was effectively vaccinated.
- **health_setting_cases_averted** — transmission events that were blocked because the infector was isolated.

These are accessible to reports at the end of the simulation.

### Detection-triggered actions

When a person's `DetectionStatus` changes from `Undetected` to `Detected`, the transmission manager:
- Quarantines presymptomatic contacts.
- Marks symptomatic contacts as `Suspected` if they are not already in a treatment setting and moves them to isolation.
- Initiates contact tracing of the detected case's secondary cases (via `detection_manager`).
