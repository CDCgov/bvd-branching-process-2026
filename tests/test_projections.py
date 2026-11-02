"""Projection tests for particle inputs and dataframes."""

import copy

import polars as pl


def scenario_grid() -> pl.DataFrame:
    """The same four projection scenarios used by the BVD example."""
    return pl.DataFrame(
        {
            "passive_detection_probability": [0.2, 0.8, 0.2, 0.8],
            "offspring_distribution.NegativeBinomial.concentration": [
                0.1,
                0.1,
                1.0,
                1.0,
            ],
        }
    )


def projection_config(base_config: dict, model_binary) -> dict:
    config = copy.deepcopy(base_config)
    config["exe_file"] = str(model_binary)
    config.pop("target_data_file", None)
    config["projection"]["default_ixa_overrides"]["end_run_conditions"] = {
        "max_time": 120.0,
        "max_deaths": 500,
    }
    # The default ixa config has symptom_onset_report with trigger date 2026-07-06,
    # which exceeds max_time=120 days from start (2025-09-01 → ~2026-01-01).
    # Disable it so the model does not reject the configuration.
    config["projection"]["default_ixa_overrides"]["symptom_onset_report"] = {
        "write": False
    }
    return config


def read_projection_result(output_dir):
    inputs = pl.read_csv(output_dir / "projection" / "all_simulation_inputs.csv")
    # Use the raw prevalence report rather than all_simulations.csv, which is
    # empty for the detection_band strategy when max_time is too short to
    # accumulate any confirmed cases.
    outputs = pl.read_csv(output_dir / "projection" / "all_prevalence_reports.csv")
    return (
        inputs.join(outputs, on="particle_id")
        .sort(
            "seed",
            "passive_detection_probability",
            "offspring_distribution.NegativeBinomial.concentration",
            "date",
            "alive",
            "infection_status",
            "case_status",
        )
        .drop("particle_id")
    )
