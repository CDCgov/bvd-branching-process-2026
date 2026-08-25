# Disease Progression & Clinical Manager

**Source files:** `src/disease_manager.rs`, `src/clinical_manager.rs`

---

## Overview

The disease progression and clinical manager modules govern the evolution of a newly infected person, including disease progression and healthcare status.

---

## Disease manager (`disease_manager.rs`)

### Purpose

The disease manager handles the biological course of infection for each person.
### Disease states

Each person has an `InfectionStatus` that moves through the following states:

```
Presymptomatic → Symptomatic → Removed
                    ↓
                  (Dead)
```

Additionally, `Susceptible` and `Vaccinated` statuses exist for contacts who are traced through the surveillance system but are not infected.

### Generation interval discretization

We modeled the course infectiousness using a generation interval distribution. We assumed that an infected individual becomes infectious at the onset of symptoms. Based on the generation interval and a probability of pre-symptomatic transmission, we derived the time from exposure to symptom onset as
```
exposure_to_symptom_onset = quantile(generation_interval_distribution,
                                     presymptomatic_transmission_probability).
```
This means symptom onset is placed at the `presymptomatic_transmission_probability`-th quantile of the generation interval. For example, if 30% of transmission occurs before symptoms, symptom onset is at the 30th percentile of the generation interval.

We also derived the infectiousness period based on the generation interval so that
```
exposure_to_max_infectious = quantile(generation_interval_distribution, 1.0).
```

Hence, a person's full disease timeline is:

```
At time of exposure:
  latent_period_end   = current_time + exposure_to_symptom_onset_delay
  infectious_period_end = current_time + exposure_to_max_infectious

Schedule: become Symptomatic at latent_period_end
Schedule: resolve at infectious_period_end (or sooner if recovered)
```

### Mortality

A symptomatic individual can die with a probability based on the case fatality ratio. Individuals who recover from the disease are removed from the transition process with time based on a recovery delay distribution. Concretely, the algorithm looks like:

```
- Draw `Bernoulli(case_fatality_ratio)`.
- If they die: draw a time of death from `mortality_delay_distribution` and schedule `Alive → false` at that time.
- If they survive: schedule transition to `Removed` using `recovery_delay_distribution`.
```

### Cumulative case count

The disease manager maintains a global count of cumulative symptomatic cases. This count is incremented each time a person transitions from `Presymptomatic` to `Symptomatic`, and it is used by `state_trigger` to fire threshold-based events (e.g., to trigger a surveillance campaign when a certain number of cases is reached).

### Interaction with other modules

When a new person is added to the simulation, the disease manager configures a disease timeline for them by subscribing to an event called `EntityCreated`. To determine the correct healthcare pathways, the clinical manager tracks changes in a person's infection status to respond when a person becomes symptomatic or removed.

## Clinical manager (`clinical_manager.rs`)

The clinical manager determines actions related to a person's healthcare. For instance, whether a person is hospitalized and what setting they are in,  and how transmission probability changes with treatment location. A person can be located in one of four healthcare locations: community, quarantine facilities, clinic, or Ebola Treatment Unit. The clinical manager also tracks a person's case status (Suspected vs. Confirmed) is assigned,

### Health settings

| Health Setting | Description |
|---|---|
| `None` (community) | Person is in the community; full transmission probability applies |
| `Quarantine` | Person is isolated at home or in a community quarantine facility; specifiable reduced transmission |
| `Clinic` | Person has presented to a health clinic; specifiable reduced transmission |
| `EbolaTreatmentUnit` | Person is in a dedicated ETU; specifiable reduced transmission |

### Case status

| Case Status | Meaning |
|---|---|
| `None` | Not yet entered into the surveillance system |
| `Suspected` | Clinically identified as a probable case or detected through other means; sample collection initiated |
| `Confirmed` | Laboratory-confirmed; triggers ETU transfer |

### Hospitalization pathway

When a person becomes symptomatic and is in the community:

```
Draw Bernoulli(hospitalization_probability)
If hospitalized:
    Draw delay ~ hospitalization_delay_distribution
    At (current_time + delay):
        If still in community with no case status:
            Set TreatmentLocation → Clinic
            Set CaseStatus → Suspected
```

This represents voluntary healthcare-seeking behavior. Cases that are detected via contact tracing are quarantined before they might present to a clinic.

### Case confirmation and ETU transfer

Once a case is `Suspected`, the testing module schedules a laboratory test. When the test returns positive (`Confirmed`), then, if the person is `Symptomatic` and in a treatment location, a transfer to ETU is scheduled. This transfer occurs after a delay (`etu_transfer_delay`). The model does not consider true negatives in the testing module given that only those exposed to the pathogen are considered for testing.

### Transmission probability by setting

The clinical manager provides the current transmission probability for a given person based on their treatment location. The baseline probability is determined based on the offspring distribution, which already encodes the expected `R_0`. Changes in the probability of transmissionfor individuals in quarantine, a clinic, or an Ebola treatment unit are defined by input parameters.

| Location | Transmission probability |
|---|---|
| Community | 1.0 (baseline; the offspring distribution already encodes the expected R₀) |
| Quarantine | `quarantine_transmission_probability` |
| Clinic | `clinical_transmission_probability` |
| ETU | `etu_transmission_probability` |

A value of 0.0 means no transmission from that setting, akin to perfect isolation and protective measures.

### Interaction with other modules

The clinical manager subscribes to:
- `InfectionStatus` changes: triggers hospitalization planning when a person becomes symptomatic; clears treatment location when a person is removed.
- `CaseStatus` changes: when a case becomes `Suspected`, notifies the testing queue; when `Confirmed`, triggers ETU transfer and passive detection.
