# Vaccination

**Source files:** `src/vaccination.rs`, `src/vaccination_campaign.rs`

---

## Overview

The model supports two vaccination campaign strategies that can be enabled independently and concurrently: **ring vaccination** and **geographic (mass) vaccination**. Both campaigns are defined in configuration, may be triggered by a threshold event, and affect transmission by conferring immunity to contacts of infectious cases before infection attempts occur.

---

## Campaign types

### Ring vaccination

Ring vaccination targets the direct contacts of detected cases. When a case is detected, the ring vaccination campaign identifies that case's contacts (people who were generated as secondary cases of the detected index) and offers them vaccine.

Key parameters:
- **Trigger** — a `StateTrigger` (e.g., first detection, N deaths) that activates the campaign.
- **Start delay** — a fixed delay (in days) after the trigger fires before the campaign begins.

### Geographic vaccination

Geographic vaccination represents a broader, population-level campaign that is not restricted to known contacts. When the campaign is active, every potential contact of an infectious person has some probability of having been pre-vaccinated.

Key parameters:
- **Trigger** — as above.
- **Start delay** — as above.
- **Max prevalence** — the maximum fraction of contacts that could have been vaccinated; effectively the campaign coverage ceiling.
- **Schedule** — the rollout schedule, which controls how many doses are administered per day. Can be:
  - *Parameterized* — draws inoculation time from a specified distribution.
  - *Empirical* — reads a CSV file of `(day, count)` pairs; weights doses by when they were administered.
  - *Unlimited* — no schedule constraint; all contacts sampled as vaccinated are assumed to have been reached.

---

## Vaccine efficacy and protection delay

Vaccination does not confer instant protection. Each vaccine recipient has:
- **Efficacy probability** — the probability that their immune response is successful at all. Drawn as a Bernoulli at inoculation time.
- **Protection delay** — sampled from `protection_delay_distribution`. The vaccine only prevents infection if the transmission event occurs *after* `inoculation_time + protection_delay`.

If a contact's `VaccineData` indicates they are vaccinated and the current time is past their effective protection time, the transmission attempt is blocked and the case is counted as **vaccine-averted**.

---

## How vaccination is evaluated at transmission time

When an infection attempt occurs, the transmission manager calls into the vaccination module to retrieve `ContactInterventionData`, which bundles:
1. `VaccineData` — whether the contact has vaccine data, and if so, whether it is effective yet.
2. `SurveillanceData` — whether the contact would be detected (and quarantined) due to contact tracing.

The vaccination module proceeds as follows:

```
If geographic campaign is active:
    Draw Bernoulli(max_prevalence)  ← "was this contact reached?"
    If reached:
        Draw inoculation time from campaign schedule
        Draw vaccine efficacy ~ Bernoulli(efficacy)
        If efficacious:
            Calculate effective_time = inoculation_time + protection_delay
            Return VaccineData::Vaccinated { ..., vaccine_success: true }
        Else:
            Return VaccineData::Vaccinated { ..., vaccine_success: false }

If ring campaign is active and contact is from a detected case:
    Apply same logic as above, using ring-specific random streams
```

If both campaigns apply (because the contact qualifies under both), the one with the **earlier inoculation time** is retained.

---

## VaccineData property

Each person carries a `VaccineData` property:

| Variant | Description |
|---|---|
| `Unvaccinated` | No vaccine received |
| `Vaccinated { inoculation_time, effective_time, campaign_deploy_method, vaccine_success }` | Vaccinated. `vaccine_success` indicates whether the immune response was successful. `effective_time` is `inoculation_time + protection_delay` if successful, `None` if not. |

The `campaign_deploy_method` field records whether vaccination was via ring or geographic campaign, which appears in output reports.

---

## Interaction with other modules

- **Transmission manager** — queries vaccine status for each new contact before deciding whether the contact becomes infected.
- **Detection manager** — ring vaccination is only offered when an index case is detected. The detection manager's contact tracing triggers the ring vaccination assessment.
- **State triggers** — both campaigns can be activated by any `StateTrigger` (e.g., `Deaths { count: 10 }` or `Detection { count: 1 }`).
- **Reports** — the `vaccine_cases_averted` counter (maintained in `transmission_manager`) is available to the reporting system.
