import polars as pl
from vhf_pipeline.utils import bin_by_width, categorize_by_breaks, make_bin_labels

# ---------------------------------------------------------------------------
# categorize_by_breaks
# ---------------------------------------------------------------------------


class TestCategorizeByBreaks:
    def test_two_breaks_all_three_categories(self):
        df = pl.DataFrame({"val": [0, 14999, 15000, 22500, 30000, 30001, 99999]})
        result = categorize_by_breaks(df, col="val", breaks=[15000, 30000])
        expected = ["<15k", "<15k", "15k-30k", "15k-30k", "15k-30k", ">30k", ">30k"]
        assert result["count_category"].to_list() == expected

    def test_lower_break_boundary_belongs_to_middle_category(self):
        # Exactly at breaks[0]: NOT < breaks[0], so falls into the middle bucket.
        df = pl.DataFrame({"val": [15000]})
        result = categorize_by_breaks(df, col="val", breaks=[15000, 30000])
        assert result["count_category"][0] == "15k-30k"

    def test_upper_break_boundary_belongs_to_middle_category(self):
        df = pl.DataFrame({"val": [30000]})
        result = categorize_by_breaks(df, col="val", breaks=[15000, 30000])
        assert result["count_category"][0] == "15k-30k"

    def test_three_breaks_four_categories(self):
        df = pl.DataFrame({"val": [500, 10000, 15000, 25000, 30001]})
        result = categorize_by_breaks(df, col="val", breaks=[10000, 20000, 30000])
        expected = ["<10k", "10k-20k", "10k-20k", "20k-30k", ">30k"]
        assert result["count_category"].to_list() == expected

    def test_single_break_two_categories(self):
        # With one break, values below get <label and values at/above get >label.
        df = pl.DataFrame({"val": [0, 999, 1000, 5000]})
        result = categorize_by_breaks(df, col="val", breaks=[1000])
        expected = ["<1k", "<1k", ">1k", ">1k"]
        assert result["count_category"].to_list() == expected

    def test_non_k_numbers_use_plain_integer_labels(self):
        # 1500 is not divisible by 1000, so the label is "1500" not "1k".
        # val=500 < 1500 → "<1500"; val=1500 is at the boundary → "1500-3k"; val=5000 → ">3k"
        df = pl.DataFrame({"val": [0, 500, 1500, 2000, 5000]})
        result = categorize_by_breaks(df, col="val", breaks=[1500, 3000])
        expected = ["<1500", "<1500", "1500-3k", "1500-3k", ">3k"]
        assert result["count_category"].to_list() == expected

    def test_custom_alias(self):
        df = pl.DataFrame({"n": [100]})
        result = categorize_by_breaks(df, col="n", breaks=[500], alias="size_bin")
        assert "size_bin" in result.columns
        assert "count_category" not in result.columns

    def test_original_columns_preserved(self):
        df = pl.DataFrame({"val": [100], "extra": ["x"]})
        result = categorize_by_breaks(df, col="val", breaks=[500])
        assert "extra" in result.columns
        assert result["extra"][0] == "x"

    def test_default_alias_is_count_category(self):
        df = pl.DataFrame({"val": [100]})
        result = categorize_by_breaks(df, col="val", breaks=[500])
        assert "count_category" in result.columns


# ---------------------------------------------------------------------------
# bin_by_width
# ---------------------------------------------------------------------------


class TestBinByWidth:
    def test_basic_binning(self):
        # width=2500, max=20000 → bins: 0-2499, 2500-4999, …, 17500-19999, >=20000
        df = pl.DataFrame({"n": [0, 1000, 2500, 5000, 17500, 19999]})
        result = bin_by_width(df, col="n", binning_width=2500, max_size=20000)
        expected = [
            "0-2499",
            "0-2499",
            "2500-4999",
            "5000-7499",
            "17500-19999",
            "17500-19999",
        ]
        assert result["final_size_category"].to_list() == expected

    def test_overflow_at_max_size(self):
        df = pl.DataFrame({"n": [20000, 25000, 999999]})
        result = bin_by_width(df, col="n", binning_width=2500, max_size=20000)
        assert (result["final_size_category"] == ">=20000").all()

    def test_value_just_below_max_size(self):
        df = pl.DataFrame({"n": [19999]})
        result = bin_by_width(df, col="n", binning_width=2500, max_size=20000)
        assert result["final_size_category"][0] == "17500-19999"

    def test_zero_goes_to_first_bin(self):
        df = pl.DataFrame({"n": [0]})
        result = bin_by_width(df, col="n", binning_width=500, max_size=4000)
        assert result["final_size_category"][0] == "0-499"

    def test_custom_alias(self):
        df = pl.DataFrame({"deaths": [100]})
        result = bin_by_width(
            df, col="deaths", binning_width=500, max_size=4000, alias="death_bin"
        )
        assert "death_bin" in result.columns
        assert "final_size_category" not in result.columns

    def test_default_alias_is_final_size_category(self):
        df = pl.DataFrame({"n": [0]})
        result = bin_by_width(df, col="n", binning_width=100, max_size=1000)
        assert "final_size_category" in result.columns

    def test_original_columns_preserved(self):
        df = pl.DataFrame({"n": [0], "tag": ["a"]})
        result = bin_by_width(df, col="n", binning_width=100, max_size=1000)
        assert "tag" in result.columns


# ---------------------------------------------------------------------------
# make_bin_labels
# ---------------------------------------------------------------------------


class TestMakeBinLabels:
    def test_returns_correct_labels(self):
        labels = make_bin_labels(binning_width=2500, max_size=10000)
        expected = ["0-2499", "2500-4999", "5000-7499", "7500-9999", ">=10000"]
        assert labels == expected

    def test_overflow_label_last(self):
        labels = make_bin_labels(binning_width=500, max_size=2000)
        assert labels[-1] == ">=2000"

    def test_label_count(self):
        # max_size / binning_width bins + 1 overflow
        labels = make_bin_labels(binning_width=1000, max_size=5000)
        assert len(labels) == 6  # 5 bins + overflow

    def test_consistent_with_bin_by_width(self):
        """Every label produced by bin_by_width must appear in make_bin_labels."""
        binning_width = 2500
        max_size = 10000
        values = list(range(0, max_size + 1, 250)) + [max_size + 1, 99999]
        df = pl.DataFrame({"n": values})
        result = bin_by_width(
            df, col="n", binning_width=binning_width, max_size=max_size
        )
        observed_labels = set(result["final_size_category"].to_list())
        expected_labels = set(make_bin_labels(binning_width, max_size))
        assert observed_labels == expected_labels
