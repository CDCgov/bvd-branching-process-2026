import datetime as dt

import polars as pl
from vhf_pipeline.utils import get_cumulative_symptomatic_cases


def _make_prevalence_df(rows: list[dict]) -> pl.DataFrame:
    """Build a minimal prevalence report DataFrame from a list of row dicts."""
    return pl.DataFrame(rows).with_columns(pl.col("date").cast(pl.Date))


# ---------------------------------------------------------------------------
# Infection-status filtering
# ---------------------------------------------------------------------------


def test_excludes_presymptomatic_and_other_statuses():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 10,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 5,
                "infection_status": "Presymptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 3,
                "infection_status": "Exposed",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"].sum() == 10


def test_includes_removed_status():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 7,
                "infection_status": "Removed",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"].sum() == 7


def test_includes_both_symptomatic_and_removed():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 4,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 6,
                "infection_status": "Removed",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"].sum() == 10


# ---------------------------------------------------------------------------
# detected_only filtering
# ---------------------------------------------------------------------------


def test_detected_only_false_includes_all_detection_statuses():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 8,
                "infection_status": "Symptomatic",
                "detection_status": "Detected",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 12,
                "infection_status": "Symptomatic",
                "detection_status": "Not Detected",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(
        df, max_date=dt.date(2026, 1, 1), detected_only=False
    )
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"].sum() == 20


def test_detected_only_true_excludes_undetected():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 8,
                "infection_status": "Symptomatic",
                "detection_status": "Detected",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 12,
                "infection_status": "Symptomatic",
                "detection_status": "Not Detected",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(
        df, max_date=dt.date(2026, 1, 1), detected_only=True
    )
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"].sum() == 8


def test_detected_only_true_with_removed_status():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 3,
                "infection_status": "Removed",
                "detection_status": "Detected",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 5,
                "infection_status": "Removed",
                "detection_status": "Not Detected",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 2,
                "infection_status": "Symptomatic",
                "detection_status": "Detected",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(
        df, max_date=dt.date(2026, 1, 1), detected_only=True
    )
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"].sum() == 5


# ---------------------------------------------------------------------------
# Grouping and aggregation
# ---------------------------------------------------------------------------


def test_groups_by_particle_and_date():
    # Two rows for the same particle+date should be summed into one row.
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 4,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 6,
                "infection_status": "Removed",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    on_date = result.filter(pl.col("date") == dt.date(2026, 1, 1))
    assert len(on_date) == 1
    assert on_date["count"][0] == 10


def test_separate_particles_kept_separate():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 10,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 2,
                "date": dt.date(2026, 1, 1),
                "count": 20,
                "infection_status": "Symptomatic",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    on_date = result.filter(pl.col("date") == dt.date(2026, 1, 1))
    assert len(on_date) == 2
    assert set(on_date["particle_id"].to_list()) == {1, 2}


def test_separate_dates_kept_separate():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 5,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 2),
                "count": 8,
                "infection_status": "Symptomatic",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 2))
    assert result.filter(pl.col("date") == dt.date(2026, 1, 1))["count"][0] == 5
    assert result.filter(pl.col("date") == dt.date(2026, 1, 2))["count"][0] == 8


# ---------------------------------------------------------------------------
# Output schema
# ---------------------------------------------------------------------------


def test_output_has_required_columns():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 5,
                "infection_status": "Symptomatic",
            },
        ]
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    assert "particle_id" in result.columns
    assert "date" in result.columns
    assert "count" in result.columns


def test_date_column_is_date_type():
    df = pl.DataFrame(
        {
            "particle_id": [1],
            "date": ["2026-01-01"],
            "count": [5],
            "infection_status": ["Symptomatic"],
        }
    )
    result = get_cumulative_symptomatic_cases(df, max_date=dt.date(2026, 1, 1))
    assert result["date"].dtype == pl.Date


# ---------------------------------------------------------------------------
# Forward fill to max_date
# ---------------------------------------------------------------------------


def test_extends_to_max_date():
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 10,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 2),
                "count": 20,
                "infection_status": "Symptomatic",
            },
        ]
    )
    max_date = dt.date(2026, 1, 5)
    result = get_cumulative_symptomatic_cases(df, max_date=max_date)
    assert result["date"].max() == max_date


def test_no_forward_fill_when_data_reaches_max_date():
    # When the data already covers max_date, the result should not be extended.
    df = _make_prevalence_df(
        [
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 1),
                "count": 10,
                "infection_status": "Symptomatic",
            },
            {
                "particle_id": 1,
                "date": dt.date(2026, 1, 3),
                "count": 20,
                "infection_status": "Symptomatic",
            },
        ]
    )
    max_date = dt.date(2026, 1, 3)
    result = get_cumulative_symptomatic_cases(df, max_date=max_date)
    assert result["date"].max() == max_date
