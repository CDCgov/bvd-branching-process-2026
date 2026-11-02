import polars as pl
from epiweeks import Week


def assign_epiweek(df: pl.DataFrame, date_column_name: str = "date") -> pl.DataFrame:
    if df.schema[date_column_name] != pl.Date:
        df = df.with_columns(pl.col(date_column_name).str.to_date())
    return df.with_columns(
        pl.col(date_column_name)
        .map_elements(
            lambda x: Week.fromdate(x, system="iso").startdate(),
            return_dtype=pl.Date,
        )
        .alias("epiweek_startdate")
    )
