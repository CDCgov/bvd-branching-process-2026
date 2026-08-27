"""Unit tests for rt_by_ratio_category.py.

These test pure functions and DataFrame transformations only — no file I/O,
no model binary, no calibration outputs required.
"""

import datetime as dt
import math

import numpy as np
import polars as pl
import pytest
from vhf_pipeline.pipeline.rt_by_ratio_category import (
    CATEGORY_ORDER,
    _build_pretty_inputs_summary,
    _build_pretty_summary,
    _category_grid,
    _compute_prior_stats,
    _format_date_median_iqi,
    _format_median_iqi,
    _iqi_of_sampled_gamma_delay,
    _iqi_of_sampled_offset_weibull_gi,
    _late_category_grid,
    _ratio_category,
    _resolve_rt_window,
    _sample_distribution,
    _sample_gamma_delays,
    _sample_offset_weibull_gi,
    _spillover_ordinals_expr,
    _summarize_by_late_category,
    _summarize_inputs_by_late_category,
    _summarize_outbreak_size_by_late_category,
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_INTERVENTION_DATE = dt.date(2026, 5, 24)


def _detection_df(rows: list[dict]) -> pl.DataFrame:
    """Minimal particle detection rate frame."""
    return pl.DataFrame(rows)


# ---------------------------------------------------------------------------
# _ratio_category
# ---------------------------------------------------------------------------


class TestRatioCategory:
    def _apply(self, values: list[float]) -> list[str | None]:
        df = pl.DataFrame({"ratio": values})
        return df.with_columns(_ratio_category("ratio").alias("cat"))["cat"].to_list()

    def test_lower_boundary_20_pct_is_included(self):
        assert self._apply([0.20]) == ["20-30%"]

    def test_upper_boundary_30_pct_excluded_from_first_bin(self):
        assert self._apply([0.30]) == ["30-40%"]

    def test_midpoints_land_in_correct_bins(self):
        assert self._apply([0.25, 0.35, 0.45, 0.55]) == [
            "20-30%",
            "30-40%",
            "40-50%",
            "50-60%",
        ]

    def test_upper_boundary_60_pct_is_included(self):
        assert self._apply([0.60]) == ["50-60%"]

    def test_values_outside_range_return_null(self):
        result = self._apply([0.10, 0.19, 0.61, 1.00])
        assert all(v is None for v in result)

    def test_exactly_0_50_belongs_to_50_60_bin(self):
        assert self._apply([0.50]) == ["50-60%"]


# ---------------------------------------------------------------------------
# _category_grid
# ---------------------------------------------------------------------------


class TestCategoryGrid:
    def test_has_all_16_combinations(self):
        grid = _category_grid()
        assert grid.height == 16

    def test_contains_expected_columns(self):
        grid = _category_grid()
        assert set(grid.columns) == {"earlier_ratio_category", "later_ratio_category"}

    def test_each_category_appears_exactly_four_times_per_column(self):
        grid = _category_grid()
        for col in ["earlier_ratio_category", "later_ratio_category"]:
            counts = grid[col].value_counts()
            assert (counts["count"] == 4).all()


# ---------------------------------------------------------------------------
# _late_category_grid
# ---------------------------------------------------------------------------


class TestLateCategoryGrid:
    def test_has_four_rows(self):
        assert _late_category_grid().height == 4

    def test_categories_match_category_order(self):
        assert _late_category_grid()["later_ratio_category"].to_list() == CATEGORY_ORDER


# ---------------------------------------------------------------------------
# _resolve_rt_window
# ---------------------------------------------------------------------------


class TestResolveRtWindow:
    def test_default_delay_zero_starts_on_intervention_date(self):
        start, end = _resolve_rt_window(_INTERVENTION_DATE, delay=0, duration=15)
        assert start == _INTERVENTION_DATE

    def test_end_date_is_duration_minus_one_days_after_start(self):
        start, end = _resolve_rt_window(_INTERVENTION_DATE, delay=0, duration=15)
        assert (end - start).days == 14

    def test_delay_shifts_window_by_that_many_days(self):
        start, end = _resolve_rt_window(_INTERVENTION_DATE, delay=3, duration=10)
        assert start == _INTERVENTION_DATE + dt.timedelta(days=3)
        assert (end - start).days == 9

    def test_duration_one_gives_same_start_and_end(self):
        start, end = _resolve_rt_window(_INTERVENTION_DATE, delay=0, duration=1)
        assert start == end


# ---------------------------------------------------------------------------
# _format_median_iqi
# ---------------------------------------------------------------------------


class TestFormatMedianIqi:
    def test_basic_formatting_two_decimals(self):
        assert _format_median_iqi(1.234, 0.987, 1.567) == "1.23 (0.99-1.57)"

    def test_decimals_zero_rounds_to_integer(self):
        assert _format_median_iqi(12.6, 10.2, 15.8, decimals=0) == "13 (10-16)"

    def test_negative_decimals_rounds_to_hundreds(self):
        result = _format_median_iqi(1234.0, 900.0, 1600.0, decimals=-2)
        assert result == "1200 (900-1600)"

    def test_returns_empty_string_when_any_value_is_none(self):
        assert _format_median_iqi(None, 1.0, 2.0) == ""
        assert _format_median_iqi(1.0, None, 2.0) == ""
        assert _format_median_iqi(1.0, 1.0, None) == ""
        assert _format_median_iqi(None, None, None) == ""

    def test_custom_decimal_places(self):
        result = _format_median_iqi(1.23456, 1.11111, 1.33333, decimals=4)
        assert result == "1.2346 (1.1111-1.3333)"


# ---------------------------------------------------------------------------
# _format_date_median_iqi
# ---------------------------------------------------------------------------


class TestFormatDateMedianIqi:
    def _ordinal(self, d: dt.date) -> float:
        return float(d.toordinal())

    def test_formats_known_dates_correctly(self):
        med = self._ordinal(dt.date(2026, 5, 24))
        q25 = self._ordinal(dt.date(2026, 5, 10))
        q75 = self._ordinal(dt.date(2026, 6, 7))
        result = _format_date_median_iqi(med, q25, q75)
        assert result == "May 24 (May 10-Jun 07)"

    def test_returns_empty_string_when_any_ordinal_is_none(self):
        base = float(dt.date(2026, 5, 24).toordinal())
        assert _format_date_median_iqi(None, base, base) == ""
        assert _format_date_median_iqi(base, None, base) == ""
        assert _format_date_median_iqi(base, base, None) == ""

    def test_rounding_of_fractional_ordinals(self):
        # 0.5 added should round up to next day
        base = dt.date(2026, 5, 24)
        result = _format_date_median_iqi(
            float(base.toordinal()) + 0.6,
            float(base.toordinal()),
            float(base.toordinal()) + 1.0,
        )
        assert "May 25" in result


# ---------------------------------------------------------------------------
# _compute_prior_stats
# ---------------------------------------------------------------------------


class TestComputePriorStats:
    def test_normal_median_equals_mean(self):
        spec = {"distribution": "normal", "parameters": {"mean": 5.0, "std_dev": 1.0}}
        median, q25, q75 = _compute_prior_stats(spec)
        assert median == pytest.approx(5.0, abs=1e-6)

    def test_normal_iqr_is_symmetric(self):
        spec = {"distribution": "normal", "parameters": {"mean": 5.0, "std_dev": 1.0}}
        _, q25, q75 = _compute_prior_stats(spec)
        assert (5.0 - q25) == pytest.approx(q75 - 5.0, abs=1e-6)

    def test_uniform_median_is_midpoint(self):
        spec = {"distribution": "uniform", "parameters": {"min": 2.0, "max": 6.0}}
        median, q25, q75 = _compute_prior_stats(spec)
        assert median == pytest.approx(4.0, abs=1e-6)
        assert q25 == pytest.approx(3.0, abs=1e-6)
        assert q75 == pytest.approx(5.0, abs=1e-6)

    def test_lognormal_median_equals_exp_mean(self):
        mu = 1.5
        spec = {"distribution": "lognormal", "parameters": {"mean": mu, "std_dev": 0.5}}
        median, _, _ = _compute_prior_stats(spec)
        assert median == pytest.approx(math.exp(mu), rel=1e-6)

    def test_beta_symmetric_median_is_half(self):
        spec = {"distribution": "beta", "parameters": {"alpha": 2.0, "beta": 2.0}}
        median, _, _ = _compute_prior_stats(spec)
        assert median == pytest.approx(0.5, abs=0.01)

    def test_unknown_distribution_raises_value_error(self):
        spec = {"distribution": "poisson", "parameters": {"rate": 3.0}}
        with pytest.raises(ValueError, match="Unknown prior distribution"):
            _compute_prior_stats(spec)

    def test_q25_less_than_median_less_than_q75(self):
        spec = {"distribution": "normal", "parameters": {"mean": 0.0, "std_dev": 1.0}}
        median, q25, q75 = _compute_prior_stats(spec)
        assert q25 < median < q75


# ---------------------------------------------------------------------------
# _sample_distribution
# ---------------------------------------------------------------------------


class TestSampleDistribution:
    def test_returns_array_of_requested_size(self):
        spec = {"distribution": "normal", "parameters": {"mean": 0.0, "std_dev": 1.0}}
        samples = _sample_distribution(spec, n=500)
        assert len(samples) == 500

    def test_uniform_samples_within_bounds(self):
        spec = {"distribution": "uniform", "parameters": {"min": 3.0, "max": 7.0}}
        samples = _sample_distribution(spec, n=1000)
        assert samples.min() >= 3.0
        assert samples.max() <= 7.0

    def test_beta_samples_in_unit_interval(self):
        spec = {"distribution": "beta", "parameters": {"alpha": 1.0, "beta": 1.0}}
        samples = _sample_distribution(spec, n=1000)
        assert (samples >= 0.0).all()
        assert (samples <= 1.0).all()

    def test_unknown_distribution_raises_value_error(self):
        spec = {"distribution": "dirichlet", "parameters": {}}
        with pytest.raises(ValueError, match="Unknown prior distribution"):
            _sample_distribution(spec, n=10)


# ---------------------------------------------------------------------------
# _sample_gamma_delays
# ---------------------------------------------------------------------------


class TestSampleGammaDelays:
    def test_output_length_is_sample_size_times_array_length(self):
        shape = np.full(10, 2.0)
        rate = np.full(10, 1.0)
        result = _sample_gamma_delays(shape, rate, sample_size=5)
        assert result.shape == (50,)

    def test_all_delays_are_non_negative(self):
        rng = np.random.default_rng(0)
        shape = rng.uniform(0.5, 3.0, size=20)
        rate = rng.uniform(0.5, 2.0, size=20)
        result = _sample_gamma_delays(shape, rate, sample_size=10)
        assert (result >= 0).all()


# ---------------------------------------------------------------------------
# _iqi_of_sampled_gamma_delay
# ---------------------------------------------------------------------------


class TestIqiOfSampledGammaDelay:
    def test_returns_non_empty_string(self):
        shape_arr = np.full(50, 2.0)
        rate_arr = np.full(50, 1.0)
        result = _iqi_of_sampled_gamma_delay(shape_arr, rate_arr)
        assert isinstance(result, str) and len(result) > 0

    def test_result_contains_parentheses_and_dash(self):
        shape_arr = np.full(50, 2.0)
        rate_arr = np.full(50, 1.0)
        result = _iqi_of_sampled_gamma_delay(shape_arr, rate_arr)
        assert "(" in result and "-" in result and ")" in result


# ---------------------------------------------------------------------------
# _sample_offset_weibull_gi
# ---------------------------------------------------------------------------


class TestSampleOffsetWeibullGi:
    def test_output_length_is_sample_size_times_array_length(self):
        offset = np.full(10, 1.0)
        scale = np.full(10, 3.0)
        result = _sample_offset_weibull_gi(offset, scale, shape=2.0, sample_size=5)
        assert result.shape == (50,)

    def test_all_values_exceed_offset(self):
        # Weibull is positive, so offset + Weibull > offset
        offset = np.full(20, 2.0)
        scale = np.full(20, 3.0)
        result = _sample_offset_weibull_gi(offset, scale, shape=2.0, sample_size=10)
        assert (result > 2.0).all()


# ---------------------------------------------------------------------------
# _iqi_of_sampled_offset_weibull_gi
# ---------------------------------------------------------------------------


class TestIqiOfSampledOffsetWeibullGi:
    def test_returns_formatted_string(self):
        offset = np.full(50, 1.0)
        scale = np.full(50, 3.0)
        result = _iqi_of_sampled_offset_weibull_gi(offset, scale, shape=2.0)
        assert isinstance(result, str) and "(" in result


# ---------------------------------------------------------------------------
# _spillover_ordinals_expr
# ---------------------------------------------------------------------------


class TestSpilloverOrdinalsExpr:
    def test_ordinal_roundtrip(self):
        base_date = dt.date(2026, 1, 1)
        base_ord = base_date.toordinal()
        df = pl.DataFrame({"days_since_start": [0.0, 10.0, 30.7]})
        col_name = "initialization.initial_cases.SpilloverEvent.days_since_start"
        df = df.rename({"days_since_start": col_name})
        result = df.with_columns(_spillover_ordinals_expr(base_ord))
        ordinals = result["spillover_ordinal"].to_list()
        # days 0.0 → base_ord, 10.0 → base_ord+10, 30.7 floored → base_ord+30
        assert ordinals[0] == base_ord
        assert ordinals[1] == base_ord + 10
        assert ordinals[2] == base_ord + 30


# ---------------------------------------------------------------------------
# _build_pretty_summary
# ---------------------------------------------------------------------------


class TestBuildPrettySummary:
    @pytest.fixture
    def summary_df(self) -> pl.DataFrame:
        return pl.DataFrame(
            {
                "earlier_ratio_category": ["20-30%", "30-40%"],
                "later_ratio_category": ["20-30%", "30-40%"],
                "particle_count": [10, 0],
                "median_rt": [1.234, 0.876],
                "lower_iqr_rt": [1.001, 0.750],
                "upper_iqr_rt": [1.456, 1.000],
            }
        )

    def test_non_zero_count_produces_summary_string(self, summary_df):
        result = _build_pretty_summary(summary_df)
        val = result.filter(pl.col("particle_count") == 10)["summary"][0]
        assert "1.23" in val
        assert "n=10" in val

    def test_zero_count_produces_empty_string(self, summary_df):
        result = _build_pretty_summary(summary_df)
        val = result.filter(pl.col("particle_count") == 0)["summary"][0]
        assert val == ""

    def test_summary_contains_iqr_range(self, summary_df):
        result = _build_pretty_summary(summary_df)
        val = result.filter(pl.col("particle_count") == 10)["summary"][0]
        # Should contain "(lower-upper)" pattern
        assert "(" in val and "-" in val and ")" in val


# ---------------------------------------------------------------------------
# _summarize_by_late_category
# ---------------------------------------------------------------------------


class TestSummarizeByLateCategory:
    @pytest.fixture
    def joined_df(self) -> pl.DataFrame:
        rng = np.random.default_rng(42)
        n = 40
        categories = np.tile(CATEGORY_ORDER, n // 4)
        rt_values = rng.uniform(0.5, 1.5, size=n)
        return pl.DataFrame(
            {
                "particle_id": list(range(n)),
                "later_ratio_category": categories.tolist(),
                "particle_rt": rt_values.tolist(),
            }
        )

    def test_returns_one_row_per_category(self, joined_df):
        result = _summarize_by_late_category(joined_df)
        assert result.height == 4

    def test_categories_are_in_order(self, joined_df):
        result = _summarize_by_late_category(joined_df)
        assert result["later_ratio_category"].to_list() == CATEGORY_ORDER

    def test_particle_counts_are_correct(self, joined_df):
        result = _summarize_by_late_category(joined_df)
        # 10 particles per category
        assert (result["particle_count"] == 10).all()

    def test_missing_category_gets_null_rt(self):
        # Only two of the four categories present
        df = pl.DataFrame(
            {
                "particle_id": [1, 2],
                "later_ratio_category": ["20-30%", "30-40%"],
                "particle_rt": [1.0, 1.2],
            }
        )
        result = _summarize_by_late_category(df)
        missing = result.filter(
            pl.col("later_ratio_category").is_in(["40-50%", "50-60%"])
        )
        assert missing["median_rt"].is_null().all()


# ---------------------------------------------------------------------------
# _summarize_inputs_by_late_category
# ---------------------------------------------------------------------------


class TestSummarizeInputsByLateCategory:
    _SPILLOVER_COL = "initialization.initial_cases.SpilloverEvent.days_since_start"
    _SCALAR_COL = "offspring_intervention.scalar"

    @pytest.fixture
    def joined_df(self) -> pl.DataFrame:
        return pl.DataFrame(
            {
                "particle_id": [1, 2, 3, 4],
                "later_ratio_category": ["20-30%", "30-40%", "40-50%", "50-60%"],
            }
        )

    @pytest.fixture
    def inputs_df(self) -> pl.DataFrame:
        return pl.DataFrame(
            {
                "particle_id": [1, 2, 3, 4],
                self._SPILLOVER_COL: [5.0, 10.0, 15.0, 20.0],
                self._SCALAR_COL: [0.8, 0.7, 0.6, 0.5],
            }
        )

    def test_returns_four_rows(self, joined_df, inputs_df):
        result = _summarize_inputs_by_late_category(joined_df, inputs_df)
        assert result.height == 4

    def test_categories_present_in_output(self, joined_df, inputs_df):
        result = _summarize_inputs_by_late_category(joined_df, inputs_df)
        assert set(result["later_ratio_category"].to_list()) == set(CATEGORY_ORDER)

    def test_graceful_when_no_relevant_columns(self, joined_df):
        inputs_no_cols = pl.DataFrame(
            {"particle_id": [1, 2, 3, 4], "other": [1, 2, 3, 4]}
        )
        result = _summarize_inputs_by_late_category(joined_df, inputs_no_cols)
        assert result.height == 4
        assert "later_ratio_category" in result.columns


# ---------------------------------------------------------------------------
# _summarize_outbreak_size_by_late_category
# ---------------------------------------------------------------------------


class TestSummarizeOutbreakSizeByLateCategory:
    @pytest.fixture
    def joined_df(self) -> pl.DataFrame:
        return pl.DataFrame(
            {
                "particle_id": [1, 2, 3, 4],
                "later_ratio_category": ["20-30%", "30-40%", "40-50%", "50-60%"],
            }
        )

    @pytest.fixture
    def outbreak_size_df(self) -> pl.DataFrame:
        return pl.DataFrame(
            {
                "particle_id": [1, 2, 3, 4],
                "final_outbreak_size": [100, 200, 300, 400],
            }
        )

    def test_returns_four_rows(self, joined_df, outbreak_size_df):
        result = _summarize_outbreak_size_by_late_category(joined_df, outbreak_size_df)
        assert result.height == 4

    def test_median_matches_single_particle_per_bin(self, joined_df, outbreak_size_df):
        result = _summarize_outbreak_size_by_late_category(joined_df, outbreak_size_df)
        row = result.filter(pl.col("later_ratio_category") == "20-30%").row(
            0, named=True
        )
        assert row["median_outbreak_size"] == pytest.approx(100.0)

    def test_categories_in_order(self, joined_df, outbreak_size_df):
        result = _summarize_outbreak_size_by_late_category(joined_df, outbreak_size_df)
        assert result["later_ratio_category"].to_list() == CATEGORY_ORDER


# ---------------------------------------------------------------------------
# _build_pretty_inputs_summary
# ---------------------------------------------------------------------------


class TestBuildPrettyInputsSummary:
    _SCALAR_COL = "offspring_intervention.scalar"

    def test_formats_median_iqi_per_column(self):
        summary_df = pl.DataFrame(
            {
                "later_ratio_category": CATEGORY_ORDER,
                f"median_{self._SCALAR_COL}": [0.80, 0.70, 0.60, 0.50],
                f"lower_iqr_{self._SCALAR_COL}": [0.75, 0.65, 0.55, 0.45],
                f"upper_iqr_{self._SCALAR_COL}": [0.85, 0.75, 0.65, 0.55],
            }
        )
        result = _build_pretty_inputs_summary(summary_df, [self._SCALAR_COL])
        assert "later_ratio_category" in result.columns
        assert self._SCALAR_COL in result.columns
        first_val = result[self._SCALAR_COL][0]
        assert "0.8" in first_val and "(" in first_val

    def test_null_median_produces_empty_string(self):
        summary_df = pl.DataFrame(
            {
                "later_ratio_category": ["20-30%"],
                f"median_{self._SCALAR_COL}": [None],
                f"lower_iqr_{self._SCALAR_COL}": [None],
                f"upper_iqr_{self._SCALAR_COL}": [None],
            }
        ).with_columns(
            pl.col(f"median_{self._SCALAR_COL}").cast(pl.Float64),
            pl.col(f"lower_iqr_{self._SCALAR_COL}").cast(pl.Float64),
            pl.col(f"upper_iqr_{self._SCALAR_COL}").cast(pl.Float64),
        )
        result = _build_pretty_inputs_summary(summary_df, [self._SCALAR_COL])
        assert result[self._SCALAR_COL][0] == ""
