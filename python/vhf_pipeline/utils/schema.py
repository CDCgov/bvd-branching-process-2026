"""Canonical dtype declarations for date columns read from CSV.

Report and target-data CSVs across this project share a small, fixed set of
column names that represent calendar dates (``date``, ``report_date``, etc.).
Rather than reading those columns as strings and re-casting them with
``.str.to_date()`` in many places downstream, every ``pl.read_csv`` call in
this codebase should pass ``schema_overrides=DATE_SCHEMA_OVERRIDES`` so the
column is typed as ``pl.Date`` immediately when it is read.

Polars silently ignores ``schema_overrides`` entries for columns that are not
present in the file being read, so this single mapping can be reused for
every CSV regardless of which (if any) of these date columns it contains.
"""

import polars as pl

#: Column names that represent dates across all report/target CSVs in this project.
DATE_SCHEMA_OVERRIDES: dict[str, pl.PolarsDataType] = {
    "date": pl.Date,
    "report_date": pl.Date,
    "infection_date": pl.Date,
    "epiweek_startdate": pl.Date,
    "threshold_date": pl.Date,
}
