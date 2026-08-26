import datetime as dt

import polars as pl
from vhf_pipeline.pipeline.load_1cdp_confirmation import (
    _process_1cdp_confirmation_data,
)


def _raw() -> pl.DataFrame:
    return pl.DataFrame(
        {
            "date": ["2026-06-01", "2026-06-01", "2026-06-02", "2026-06-02"],
            "iso3c": ["COD", "UGA", "COD", "FRA"],
            "cases_confirmed_incident": [10, 3, 20, 1],
        }
    )


def test_exported_cases_are_not_counted_towards_the_outbreak():
    out = _process_1cdp_confirmation_data(_raw(), dt.date(2026, 6, 2), iso3c=["COD"])
    assert out["date"].to_list() == [dt.date(2026, 6, 1), dt.date(2026, 6, 2)]
    assert out["count"].to_list() == [10, 20]


def test_the_counted_country_is_selectable():
    out = _process_1cdp_confirmation_data(_raw(), dt.date(2026, 6, 2), iso3c=["UGA"])
    assert out["date"].to_list() == [dt.date(2026, 6, 1)]
    assert out["count"].to_list() == [3]
