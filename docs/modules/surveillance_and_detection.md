# Surveillance and Detection

**Source files:** `src/detection_manager.rs`, `src/case_confirmation.rs`

---

## Overview

Surveillance determines which simulated cases are identified by the public health response. This has downstream consequences for contact tracing, quarantine, ring vaccination, and reports. Two modules handle this: `detection_manager` manages the *process* of being detected (when and how a case is identified), and `case_confirmation` manages *laboratory confirmation* (queuing samples and returning results).

---

## Detection manager (`detection_manager.rs`)

### Purpose

The detection manager tracks which persons are known to the surveillance system, schedules detection attempts, and implements contact tracing from detected index cases.

### Detection status and surveillance data

Every person carries a `SurveillanceData` property with two variants:

- **`Undetected`** — the person has not been identified by surveillance. Optionally stores the scheduled times of pending active and passive detection attempts.
- **`Detected`** — the person has been identified. Stores the detection time, detection type (`Active` or `Passive`), and the ID of the person through whom they were traced (if active detection).

A derived property `DetectionStatus` (`Detected` / `Undetected`) summarizes this for use by other modules and triggers.

### Two detection pathways

#### Passive detection

Passive detection represents a symptomatic person being identified in the community through ongoing interventions or by being reported to a response by another community member.

```
For each new Symptomatic person in the community:
    Draw Bernoulli(passive_detection_probability)
    Draw delay ~ passive_detection_delay_distribution
    If successful:
        At (current_time + delay): mark person as Detected (Passive)
    Else:
        Record the attempt time (for reports), but person remains undetected
```

#### Active detection (contact tracing)

Active detection represents the public health team following up contacts of a known case. This is only forward contact tracing, not backwards contact tracing

```
When a case is Detected:
    For each undetected secondary case (direct contacts):
        If not already being traced:
            Draw Bernoulli(active_detection_probability)
            Draw delay ~ active_detection_delay_distribution
            If successful:
                At (primary_detection_time + delay): mark contact as Detected (Active)

    For each tertiary case (contacts of contacts):
        Draw Bernoulli(shared_index_contact_probability)
        If successful and not already being traced:
            Apply same active detection sampling as above
```

The `shared_index_contact_probability` represents the probability that a tertiary case is also a direct contact of the original index case, justifying inclusion in their contact list.

#### Surveillance campaign

A surveillance campaign can be triggered by a `StateTrigger` (e.g., when the number of detected cases crosses a threshold). Once triggered:

```
After delay ~ surveillance_campaign_delay_distribution:
    Campaign becomes active
    All currently detected cases have their contacts traced
    All new detections trigger contact tracing immediately
```

The campaign start time is recorded for use in reports.

### Transmission chain and generation tracking

The detection manager also stores the `TransmissionChain` property for each person, which records:
- The infector's ID
- The time of infection
- The infector's generation number

A derived property `Generation` computes each person's epidemic generation from this chain (index cases are generation 0). A derived property `PrimaryInfection` gives the ID of the direct infector.

These are used in the Rₜ and offspring distribution reports.

---

## Case confirmation (`case_confirmation.rs`)

### Purpose

The case confirmation module simulates laboratory testing: once a case is `Suspected`, a sample is collected (after a delay), queued for processing, and the result returned after a capacity-dependent waiting time.

### Testing configuration

Testing is either **disabled** (no tests run) or **enabled** with:
- `sample_collection_delay` — time from when a case becomes Suspected to when the sample is collected from that case.
- `testing_rate` — laboratory throughput, in one of three modes:

| Mode | Description |
|---|---|
| `Unlimited` | Results are returned immediately upon sample collection (no queue capacity limit) |
| `DailyCapacity { results_per_day }` | Fixed throughput; samples queue up if demand exceeds capacity |
| `StepDailyCapacity { initial, updated, trigger }` | Capacity changes once when a `StateTrigger` fires (e.g., when 10 deaths are reached) |

### Testing algorithm

```
When a case becomes Suspected:
    Draw delay ~ sample_collection_delay_distribution
    At (current_time + delay):
        Add person to testing queue

When testing queue is processed:
    Pop person from front of queue
    Call confirm_case(person)  → sets CaseStatus → Confirmed
    If more people in queue:
        Schedule next result at (current_time + results_lag)
```

The `results_lag` is `1 / results_per_day` days. For `Unlimited` testing, results are returned instantly (lag of 0).

### Interaction with other modules

- A `Suspected` case status is set by either the clinical manager (hospitalization pathway) or the transmission manager (when a quarantined contact becomes symptomatic).
- Upon `Confirmed` status, the clinical manager schedules ETU transfer.
- Passive detection is triggered upon confirmation if the case was previously undetected (representing back-detection through testing).
