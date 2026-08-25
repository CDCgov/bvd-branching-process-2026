import datetime as dt
from pathlib import Path

import numpy as np
import polars as pl

from ...utils import assign_epiweek, exponential_forward_fill
from .base_class import NaturalHistoryProcessor

MAX_FLOAT = np.finfo(np.float32).max
TDELTA_FIRST_ENDPOINT_POSTINTERVENTION = 21
TDELTA_STARTPOINT_POSTINTERVENTION = 8


def collect_epiweek_startdate(df: pl.DataFrame) -> pl.DataFrame:
    """Assign the ISO epiweek start date to each row, stripping leading zero-count rows."""
    return (
        df.filter(pl.col("count").cum_sum().over(order_by="date") > 0)
        .pipe(assign_epiweek)
        .group_by("epiweek_startdate")
        .agg(
            pl.sum("count").alias("confirmation_incidence"),
            pl.len().alias("num_days_in_epiweek"),
        )
    )


class DetectionBandProcessor(NaturalHistoryProcessor):
    """
    Align the known cumulative confirmation dates of a report to the date that the report was generated
    """

    required_reports = (
        "confirmation_incidence_report",
        "prevalence_report",
    )

    def __init__(
        self,
        detection_band: tuple[float, float],
    ):
        if len(detection_band) != 2:
            raise ValueError(
                "detection_band must be a tuple of two floats (lower, upper)"
            )
        assert detection_band[0] < detection_band[1], (
            "Lower bound must be less than upper bound"
        )
        self.detection_band = detection_band

    def process_outputs(
        self,
        outputs: dict[str, pl.DataFrame],
        report_date: pl.Date | None = None,
        recording_start: pl.Date | None = None,
    ) -> pl.DataFrame:
        """Process the model outputs to extract per-epiweek confirmation incidence."""
        assert "confirmation_incidence_report" in outputs, (
            "confirmation_incidence_report not found in outputs, set `write` to `true` in defaults for mode."
        )

        confirmation_df = outputs["confirmation_incidence_report"]
        default_empty = pl.DataFrame(
            schema={"confirmation_incidence": pl.Int64, "epiweek_startdate": pl.Date}
        )
        if confirmation_df.height == 0:
            return default_empty

        if report_date is None:
            report_date = confirmation_df.select(pl.max("date").cast(pl.Date)).item()
        if recording_start is None:
            recording_start = confirmation_df.select(
                pl.min("date").cast(pl.Date)
            ).item()

        confirmation_df = confirmation_df.with_columns(
            pl.col("date").cast(pl.Date)
        ).filter(
            (pl.col("case_status") == "Confirmed")
            & (pl.col("date") <= report_date)
            & (pl.col("date") >= recording_start)
        )

        if confirmation_df.height == 0:
            return default_empty

        return self._aggregate_to_epiweeks(confirmation_df, report_date)

    def _aggregate_to_epiweeks(
        self, confirmation_df: pl.DataFrame, report_date: dt.date
    ) -> pl.DataFrame:
        """Forward-fill cumulative confirmations to report_date, then aggregate to epiweeks."""
        return (
            confirmation_df.select(["date", "count"])
            .with_columns(
                pl.col("count").cum_sum().over(order_by="date").alias("count"),
                pl.lit(0).alias("particle_id"),
            )
            .pipe(
                exponential_forward_fill,
                max_date=report_date,
                temporal_averaging_days=4,
            )
            # Convert cumulative back to daily incidence
            .rename({"count": "cumulative_confirmations"})
            .with_columns(
                pl.col("cumulative_confirmations")
                .diff()
                .over(order_by="date")
                .fill_null(0)
                .cast(
                    pl.Int64, strict=False
                )  # Allow for precision loss on large floats
                .alias("count")
            )
            .drop("particle_id", "cumulative_confirmations")
            .pipe(collect_epiweek_startdate)
            .with_columns(
                pl.col("confirmation_incidence")
                .cum_sum()
                .over(order_by="epiweek_startdate")
                .alias("cumulative_confirmation_incidence"),
            )
        )

    def _deaths_within_threshold(
        self,
        prevalence_report: pl.DataFrame,
        intervention_start_date: dt.date,
        threshold: int,
    ) -> bool:
        """Return True if cumulative deaths at intervention_start_date do not exceed threshold."""
        cumulative_deaths = (
            prevalence_report.with_columns(pl.col("date").cast(pl.Date))
            .filter(pl.col("date") == intervention_start_date)
            .group_by("date")
            .agg(
                pl.when(~pl.col("alive"))
                .then(pl.col("count"))
                .otherwise(0)
                .sum()
                .alias("cumulative_deaths")
            )
        )
        if cumulative_deaths.height == 0:
            return False
        return cumulative_deaths["cumulative_deaths"][0] <= threshold

    def _compute_epiweek_error(
        self,
        output_df: pl.DataFrame,
        target_df: pl.DataFrame,
        report_date: dt.date,
        intervention_start_date: dt.date,
    ) -> pl.DataFrame:
        """Join model output with target and compute relative cumulative error per epiweek.

        Keeps only the first and last full epiweeks that are at least one week
        after intervention_start_date (to avoid surveillance ramp-up mismatch).
        """
        return (
            output_df.join(
                target_df.select(
                    ["epiweek_startdate", "cumulative_confirmation_incidence"]
                ),
                on="epiweek_startdate",
                how="right",
                coalesce=True,
                suffix="_target",
            )
            .with_columns(
                pl.col("cumulative_confirmation_incidence").fill_null(0),
            )
            .with_columns(
                (
                    pl.col("cumulative_confirmation_incidence")
                    - pl.col("cumulative_confirmation_incidence_target")
                ).alias("error_cumulative_confirmation"),
                pl.lit(report_date).cast(pl.Date).alias("report_date"),
            )
            .with_columns(
                (
                    pl.col("error_cumulative_confirmation").abs()
                    / pl.col("cumulative_confirmation_incidence_target")
                ).alias("relative_error_cumulative_confirmation"),
            )
            .with_columns(
                (
                    pl.col("cumulative_confirmation_incidence_target")
                    / pl.col("cumulative_confirmation_incidence")
                ).alias("observed_detection")
            )
        )

    def _covers_target_window(
        self, confirmation_report: pl.DataFrame, target_df: pl.DataFrame
    ) -> bool:
        """Return True if the simulation ran through the last scored epiweek.

        A run stopped early by ``max_cases`` would otherwise be scored on
        ``exponential_forward_fill`` extrapolation rather than simulated cases.
        """
        if confirmation_report.height == 0:
            return False
        sim_end = confirmation_report.select(pl.col("date").cast(pl.Date).max()).item()
        last_epiweek_end = target_df.select(
            pl.col("epiweek_startdate").cast(pl.Date).max()
        ).item() + dt.timedelta(days=6)
        return sim_end >= last_epiweek_end

    def estimate_error(
        self, outputs: dict[str, pl.DataFrame], target_df: pl.DataFrame
    ) -> float:
        """Estimate the error between the model outputs and the target data.
        Returns relative difference between weekly cumulative confirmation."""
        report_date = target_df.select("report_date").max().cast(pl.Date).item()
        assert target_df.select("threshold_date").n_unique() == 1, (
            "Multiple unique threshold_date values in target_df are not supported."
        )
        intervention_start_date = target_df.select("threshold_date").unique().item()

        if outputs["prevalence_report"].height == 0:
            return MAX_FLOAT

        if not self._covers_target_window(
            outputs["confirmation_incidence_report"], target_df
        ):
            return MAX_FLOAT

        if not self._deaths_within_threshold(
            outputs["prevalence_report"],
            intervention_start_date,
            target_df["threshold"][0],
        ):
            return MAX_FLOAT

        output_df = self.process_outputs(
            outputs,
            report_date=report_date,
            recording_start=intervention_start_date
            + dt.timedelta(days=TDELTA_STARTPOINT_POSTINTERVENTION),
        )
        if output_df.height == 0:
            return MAX_FLOAT

        output_df = self._compute_epiweek_error(
            output_df, target_df, report_date, intervention_start_date
        )

        if output_df.height == 0:
            return MAX_FLOAT

        if any(output_df["error_cumulative_confirmation"] < 0):
            return MAX_FLOAT  # Model underestimates cumulative confirmations, which is not allowed

        if output_df.height == 0:
            return MAX_FLOAT

        # Detection band distance function is calculated here once main filtering is done
        lower_detection, upper_detection = self.detection_band
        outside_bounds = output_df.filter(
            # Find any epiweeks where observed detction is outside the bounded range
            ~pl.col("observed_detection").is_between(lower_detection, upper_detection)
        )

        if outside_bounds.height > 0:
            return MAX_FLOAT
        else:
            # Accept all simulations with each data point inside acceptable bounds
            return 0.0

    def get_target_data(self, target_data_file: dict[str, Path]) -> pl.DataFrame:
        """
        Collect cumulative confirmations inside the target data since threshold date
        Return the cumulative confirmations for the first epiweek that is at least 3 weeks after the threshold date and the final epiweek
        """
        confirmation_data = pl.read_csv(target_data_file["confirmation"]).filter(
            pl.col("case_status") == "Confirmed"
        )
        deaths_threshold_data = pl.read_csv(target_data_file["deaths_threshold"])

        report_date = pl.DataFrame(
            {"report_date": [confirmation_data.select("date").max().item()]}
        )

        confirmation_data = (
            confirmation_data.join(
                deaths_threshold_data.select("threshold", "threshold_date"),
                how="cross",
            )
            .with_columns(
                pl.col("date").cast(pl.Date), pl.col("threshold_date").cast(pl.Date)
            )
            # Drop target data before the start of interventions
            .filter(
                pl.col("date")
                >= (
                    pl.col("threshold_date")
                    + dt.timedelta(days=TDELTA_STARTPOINT_POSTINTERVENTION)
                )
            )
            # Accumulate confirmed cases since the intervention start date of "threshold date"
            .pipe(collect_epiweek_startdate)
        ).join(report_date, how="cross")

        return (
            confirmation_data.join(
                deaths_threshold_data.select(
                    "threshold", "threshold_date"
                ).with_columns(pl.col("threshold_date").cast(pl.Date)),
                how="cross",
            )
            .with_columns(
                pl.col("confirmation_incidence")
                .cum_sum()
                .over(order_by="epiweek_startdate")
                .alias("cumulative_confirmation_incidence"),
            )
            .filter(
                (
                    # Filter to epiweeks that are at least 3 weeks after the threshold date
                    pl.col("epiweek_startdate")
                    >= pl.col("threshold_date")
                    + dt.timedelta(days=TDELTA_FIRST_ENDPOINT_POSTINTERVENTION)
                )
                & (pl.col("num_days_in_epiweek") == 7)  # only include full epiweeks
            )
            .filter(
                (pl.col("epiweek_startdate") == pl.max("epiweek_startdate"))
                | (pl.col("epiweek_startdate") == pl.min("epiweek_startdate"))
            )
        )
