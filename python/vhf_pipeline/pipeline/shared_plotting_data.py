"""Shared data helpers used by multiple pipeline figure modules.

Contains the canonical ``PROJECTION_DATE`` constant and data-preparation
functions that are not specific to any single pipeline figure script.
"""

import datetime as dt
import json
from pathlib import Path

import polars as pl

from vhf_pipeline import ProjectionContext
from vhf_pipeline.model import compute_epiweek_error
from vhf_pipeline.utils import get_cumulative_symptomatic_cases


def get_cumulative_cases(
    prevalence_report_df: pl.DataFrame,
    max_report_date: dt.date,
    projection_date: dt.date,
) -> pl.DataFrame:
    """Return cumulative symptomatic cases forward-filled to projection_date.

    Filters to dates strictly after *max_report_date* and drops the
    flat-plateau tail so only rising trajectories are returned.
    """
    return (
        get_cumulative_symptomatic_cases(prevalence_report_df, max_date=projection_date)
        .rename({"count": "cumulative_cases"})
        .filter(
            (pl.col("date") > max_report_date)
            & (
                pl.col("cumulative_cases")
                < pl.max("cumulative_cases").over("particle_id")
            )
        )
    )


def save_particle_detection_rate(
    target_data: pl.DataFrame,
    simulation_data: pl.DataFrame,
    products_dir: Path,
) -> None:
    """
    Save per-particle detection rates for the earliest and latest target checkpoints using `compute_epiweek_error`.
    """
    max_report_date = target_data.select(pl.max("report_date")).item()

    comparison_df = (
        simulation_data.pipe(
            compute_epiweek_error,
            target_df=target_data,
            report_date=max_report_date,
        )
        .with_columns(
            pl.when(pl.col("epiweek_startdate") == pl.min("epiweek_startdate"))
            .then(pl.lit("early"))
            .otherwise(pl.lit("late"))
            .alias("target_timing")
        )
        .select(
            "particle_id",
            "cumulative_confirmation_incidence",
            "observed_detection",
            "target_timing",
        )
        .pivot(
            "target_timing",
            index="particle_id",
            values=[
                "observed_detection",
                "cumulative_confirmation_incidence",
            ],
        )
        .sort("particle_id")
    )

    comparison_df.write_csv(products_dir / "particle_observed_detection.csv")


def load_scenario_data(
    output_dir: Path,
    calibration_subdir: str,
    products_dir: Path,
) -> dict:
    """Load all data files needed for per-scenario figure generation.

    Returns a dictionary with keys:
        - target_data: Target data with observed values
        - inputs_df: Simulation inputs
        - onset_data: Symptom onset data
        - prevalence_df: Prevalence data (Symptomatic/Presymptomatic/Removed)
        - projection_dir: Path to the projection output directory
        - mode: Mode string from the projection context
    """
    print("Loading configuration and target data...")
    with open(output_dir / calibration_subdir / "config.json", "r") as fp:
        config = json.load(fp)

    context = ProjectionContext(config, output_dir / calibration_subdir)
    _target_data_raw = context.get_target_data()
    _cast_exprs = [pl.col("report_date").cast(pl.Date)]
    if "epiweek_startdate" in _target_data_raw.columns:
        _cast_exprs.append(pl.col("epiweek_startdate").cast(pl.Date).alias("date"))
        target_data = _target_data_raw.with_columns(_cast_exprs)
    else:
        _cast_exprs.append(pl.col("date").cast(pl.Date))
        target_data = _target_data_raw.with_columns(_cast_exprs)

    if "cumulative_confirmation_incidence" not in target_data.columns:
        target_data = target_data.with_columns(
            pl.col("confirmation_incidence")
            .cum_sum()
            .over(order_by="date")
            .alias("cumulative_confirmation_incidence")
        )

    projection_dir = output_dir / calibration_subdir / context.mode

    print("Reading simulation inputs...")
    inputs_df = pl.read_csv(projection_dir / "all_simulation_inputs.csv")

    print("Reading symptom onset report...")
    symptom_onset_report_df = pl.read_csv(
        projection_dir / "all_symptom_onset_reports.csv"
    )
    _required_onset_cols = {"date", "particle_id", "count", "case_status"}
    if _required_onset_cols.issubset(set(symptom_onset_report_df.columns)):
        onset_data = (
            symptom_onset_report_df.group_by(["date", "particle_id"])
            .agg(
                pl.col("count").sum().alias("count"),
                pl.when(pl.col("case_status") == "Confirmed")
                .then(pl.col("count"))
                .otherwise(0)
                .sum()
                .alias("confirmed_count"),
            )
            .join(inputs_df, on="particle_id")
            .with_columns(pl.col("date").cast(pl.Date))
        )
    else:
        print(
            "Symptom onset report is missing expected columns; "
            "skipping onset-based figures for this scenario."
        )
        onset_data = pl.DataFrame(
            schema={
                "date": pl.Date,
                "particle_id": pl.Int64,
                "count": pl.Int64,
                "confirmed_count": pl.Int64,
            }
        )
    onset_data.write_csv(products_dir / "symptom_onset_over_time.csv")

    print("Reading prevalence report...")
    prevalence_report_df = pl.read_csv(projection_dir / "all_prevalence_reports.csv")
    prevalence_df = (
        prevalence_report_df.filter(
            pl.col("infection_status").is_in(
                ["Symptomatic", "Presymptomatic", "Removed"]
            )
        )
        .group_by("date", "particle_id")
        .agg(pl.sum("count").alias("cumulative_infections"))
    ).with_columns(pl.col("date").cast(pl.Date))
    prevalence_df.write_csv(products_dir / "prevalence_over_time.csv")

    return {
        "target_data": target_data,
        "inputs_df": inputs_df,
        "onset_data": onset_data,
        "prevalence_df": prevalence_df,
        "projection_dir": projection_dir,
        "mode": context.mode,
    }
