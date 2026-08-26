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
    and date, sums counts, casts the date column, and applies exponential
    forward fill to max_date.

    Args:
        df: Prevalence report DataFrame with columns infection_status, date,
            particle_id, count. Requires detection_status when detected_only=True.
        max_date: Latest date to forward-fill counts to.
        detected_only: If True, further filter for detection_status == "Detected".

    Returns:
        DataFrame with columns particle_id, date, count.
    """
    filtered = df.filter(pl.col("infection_status").is_in(["Symptomatic", "Removed"]))
    if detected_only:
        filtered = filtered.filter(pl.col("detection_status") == "Detected")
    return (
        filtered.group_by("particle_id", "date")
        .agg(pl.sum("count"))
        .with_columns(pl.col("date").cast(pl.Date))
        .pipe(exponential_forward_fill, max_date=max_date)
    )
