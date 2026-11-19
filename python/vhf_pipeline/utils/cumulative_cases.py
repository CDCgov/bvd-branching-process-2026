import datetime as dt

import polars as pl

from .exponential_forward_fill import exponential_forward_fill


def get_cumulative_symptomatic_cases(
    df: pl.DataFrame,
    max_date: dt.date,
    detected_only: bool = False,
) -> pl.DataFrame:
    """Get cumulative symptomatic case counts from a prevalence report.

    Filters for Symptomatic/Removed infection status, groups by particle_id
    and date, and applies exponential forward fill to max_date.

    Args:
        df: Prevalence report DataFrame with columns infection_status, date,
            particle_id, count. The date column should already be typed as
            pl.Date (e.g. via a schema-declared CSV read); a string date
            column is still accepted and cast for convenience.
            Requires detection_status when detected_only=True.
        max_date: Latest date to forward-fill counts to.
        detected_only: If True, further filter for detection_status == "Detected".

    Returns:
        DataFrame with columns particle_id, date, count.
    """
    filtered = df.filter(pl.col("infection_status").is_in(["Symptomatic", "Removed"]))
    if detected_only:
        filtered = filtered.filter(pl.col("detection_status") == "Detected")
    grouped = filtered.group_by("particle_id", "date").agg(pl.sum("count"))
    if grouped.schema["date"] != pl.Date:
        grouped = grouped.with_columns(pl.col("date").str.to_date())
    return grouped.pipe(exponential_forward_fill, max_date=max_date)
