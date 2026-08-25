import datetime as dt

import polars as pl
import pytest
from polars.testing import assert_frame_equal
from vhf_pipeline.utils import exponential_forward_fill


def test_exponential_forward_fill_basic():
    """Test exponential_forward_fill with a simple dummy dataframe."""
    # Create dummy data with known exponential growth
    data = {
        "particle_id": [1, 1, 1, 2, 2, 2],
        "date": [
            dt.date(2026, 1, 1),
            dt.date(2026, 1, 2),
            dt.date(2026, 1, 3),
            dt.date(2026, 1, 1),
            dt.date(2026, 1, 2),
            dt.date(2026, 1, 3),
        ],
        "count": [100, 200, 400, 50, 100, 200],
    }
    df = pl.DataFrame(data)
    max_date = dt.date(2026, 1, 5)

    result = exponential_forward_fill(df, max_date)

    # Verify result has correct structure
    assert "particle_id" in result.columns
    assert "date" in result.columns
    assert "count" in result.columns

    # Verify result extends to max_date
    assert result["date"].max() == max_date

    # Verify original data points are preserved
    original_data = result.filter((pl.col("date") <= dt.date(2026, 1, 3))).sort(
        "particle_id", "date"
    )

    assert len(original_data) == 6  # Should have all original rows

    # Verify that we have filled data for both particles through max_date
    particle_1_full = result.filter(pl.col("particle_id") == 1)
    particle_2_full = result.filter(pl.col("particle_id") == 2)

    assert len(particle_1_full) == 5  # 5 days of data
    assert len(particle_2_full) == 5  # 5 days of data

    # Verify counts are positive and increasing exponentially
    particle_1_sorted = particle_1_full.sort("date")
    counts = particle_1_sorted["count"].to_list()

    # Verify all counts are positive
    assert all(c > 0 for c in counts)

    # Verify counts are non-decreasing (with exponential growth)
    for i in range(len(counts) - 1):
        assert counts[i + 1] >= counts[i]

    particle_2_sorted = particle_2_full.sort("date")
    counts += particle_2_sorted["count"].to_list()

    # growth rate given the last observation is 2
    counts_expected = [100, 200, 400, 800, 1600, 50, 100, 200, 400, 800]
    assert all(abs(c - e) < 1e-6 for c, e in zip(counts, counts_expected))


def test_exponential_forward_fill_single_particle():
    """Test with a single particle_id."""
    data = {
        "particle_id": [1, 1, 1],
        "date": [
            dt.date(2026, 2, 1),
            dt.date(2026, 2, 2),
            dt.date(2026, 2, 3),
        ],
        "count": [100, 110, 121],
    }
    df = pl.DataFrame(data)
    max_date = dt.date(2026, 2, 5)

    result = exponential_forward_fill(df, max_date)

    # Should have 5 rows (one for each day)
    assert len(result) == 5
    assert result["date"].min() == dt.date(2026, 2, 1)
    assert result["date"].max() == max_date

    # All counts should be positive
    assert (result["count"] > 0).all()


def test_exponential_forward_fill_no_extension_needed():
    """Test when max_date is before the last observed date."""
    data = {
        "particle_id": [1, 1],
        "date": [dt.date(2026, 3, 1), dt.date(2026, 3, 5)],
        "count": [100, 200],
    }
    df = pl.DataFrame(data)
    max_date = dt.date(2026, 3, 5)

    result = exponential_forward_fill(df, max_date)

    # Should extend to include all dates
    assert result["date"].max() == max_date
    # Should return the same data frame
    assert_frame_equal(result, df)


def test_one_long_particle_does_not_suppress_the_fill_for_the_others():
    """A global date check would leave the short particle absent at max_date.

    Every particle must be present on every date, or date-indexed aggregates
    silently compute over a subset biased toward the slowest growers.
    """
    data = {
        "particle_id": [1, 1, 1, 2, 2],
        "date": [
            dt.date(2026, 1, 1),
            dt.date(2026, 1, 2),
            dt.date(2026, 1, 3),
            dt.date(2026, 1, 1),
            dt.date(2026, 1, 2),
        ],
        "count": [100, 200, 400, 50, 100],
    }
    max_date = dt.date(2026, 1, 3)

    result = exponential_forward_fill(pl.DataFrame(data), max_date)

    at_max = result.filter(pl.col("date") == max_date).sort("particle_id")
    assert at_max["particle_id"].to_list() == [1, 2]
    assert at_max["count"].to_list() == pytest.approx([400, 200])


def test_fill_is_skipped_only_when_every_particle_reaches_max_date():
    data = {
        "particle_id": [1, 1, 2, 2],
        "date": [
            dt.date(2026, 1, 1),
            dt.date(2026, 1, 2),
            dt.date(2026, 1, 1),
            dt.date(2026, 1, 2),
        ],
        "count": [100, 200, 50, 100],
    }
    df = pl.DataFrame(data)

    assert_frame_equal(exponential_forward_fill(df, dt.date(2026, 1, 2)), df)


def _late_starter():
    """Particle 2 starts late and decelerates, as a capped epidemic does.

    The deceleration matters: backfilling from the last observation uses the
    terminal growth rate, so a series that slows down gets a large invented
    count before its own first case.
    """
    return pl.DataFrame(
        {
            "particle_id": [1, 1, 1, 2, 2, 2],
            "date": [
                dt.date(2026, 1, 1),
                dt.date(2026, 1, 2),
                dt.date(2026, 1, 3),
                dt.date(2026, 1, 3),
                dt.date(2026, 1, 4),
                dt.date(2026, 1, 5),
            ],
            "count": [100, 200, 400, 10, 900, 1000],
        }
    )


def test_no_rows_are_invented_before_a_particle_starts():
    result = exponential_forward_fill(_late_starter(), dt.date(2026, 1, 6))

    late = result.filter(pl.col("particle_id") == 2).sort("date")
    assert late["date"].min() == dt.date(2026, 1, 3)
    assert late["count"].to_list() == pytest.approx([10, 900, 1000, 10000 / 9])


def test_cumulative_counts_never_decrease():
    """Backfilling from the last observation made the series fall.

    Particle 2 would otherwise pick up roughly 657 cases on 1 January against
    a real first count of 10 on 3 January.
    """
    result = exponential_forward_fill(_late_starter(), dt.date(2026, 1, 6))

    steps = result.select(
        pl.col("count").diff().over("particle_id", order_by="date").alias("step")
    ).drop_nulls()
    assert (steps["step"] >= 0).all()


def test_growth_rate_is_averaged_over_multiple_days():
    """The last growth rate is averaged over the last N days.

    This is important for particles that have a single observation on the last
    day of the series, which would otherwise get a growth rate of 0 and be
    filled with a flat line.
    """
    result = exponential_forward_fill(
        _late_starter(), dt.date(2026, 1, 6), temporal_averaging_days=2
    )

    late = result.filter(pl.col("particle_id") == 2).sort("date")
    # The count now goes to 10_000, which is 1_000 * ( 1_000 / 10 )
    assert late["count"].to_list() == pytest.approx([10, 900, 1000, 10000])
