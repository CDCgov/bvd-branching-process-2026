import datetime as dt

import polars as pl


def exponential_forward_fill(
    df: pl.DataFrame, max_date: dt.date, temporal_averaging_days: int = 1
) -> pl.DataFrame:
    """Extend a cumulative count series to max_date, filling exponentially.

    Expects columns count (cumulative), particle_id, and date. Each particle
    is extended from its own last observation, so the fill is skipped only
    when every particle already reaches max_date; a global date check would
    let one long-running particle suppress the fill for all the others, and
    the short ones would then drop out of any date-indexed aggregate.

    Each particle's grid also starts at its own first observation. Starting
    every particle at the frame-wide minimum would leave the late starters
    with null counts before their first case, and the fill anchors on the
    last observation, so those would be extrapolated backwards from the end
    of the series -- putting cases on dates before the particle had any and
    making the cumulative series fall where the invented rows meet the real
    ones.
    """
    bounds = df.group_by("particle_id").agg(
        pl.col("date").min().alias("first_date"),
        pl.col("date").max().alias("final_date"),
    )
    if bounds.select(pl.col("final_date").min()).item() >= max_date:
        return df
    min_date = df.select(pl.col("date").min()).item()

    # Build expanded dataframe with growth rates
    expanded = (
        bounds.select("particle_id", "first_date")
        .join(
            pl.DataFrame(
                {
                    "date": pl.datetime_range(
                        dt.datetime.combine(min_date, dt.time()),
                        dt.datetime.combine(max_date, dt.time()),
                        interval="1d",
                        eager=True,
                    ).cast(pl.Date)
                }
            ),
            how="cross",
        )
        .filter(pl.col("date") >= pl.col("first_date"))
        .drop("first_date")
        .join(df, on=["particle_id", "date"], how="left", coalesce=True)
        .with_columns(pl.col("count").log().alias("log_count"))
        .with_columns(
            (
                pl.col("log_count")
                - pl.col("log_count").shift(1).over("particle_id", order_by="date")
            ).alias("growth_rate")
        )
    )

    # Fill missing counts exponentially using last observed values
    return (
        expanded.join(
            expanded.filter(pl.col("count").is_not_null())
            .group_by("particle_id")
            .agg(
                pl.col("date").max().alias("last_date"),
                pl.col("count").last().alias("last_count"),
                pl.col("growth_rate")
                .tail(temporal_averaging_days)
                .mean()
                .alias("last_growth_rate"),
            ),
            on="particle_id",
            how="left",
        )
        .with_columns(
            pl.when(pl.col("count").is_null())
            .then(
                pl.col("last_count")
                * (
                    (pl.col("date") - pl.col("last_date")).dt.total_days()
                    * pl.col("last_growth_rate")
                ).exp()
            )
            .otherwise(pl.col("count"))
            .alias("filled_count")
        )
        .select(
            pl.exclude(
                "log_count",
                "count",
                "growth_rate",
                "last_date",
                "last_count",
                "last_growth_rate",
            )
        )
        .rename({"filled_count": "count"})
    )
