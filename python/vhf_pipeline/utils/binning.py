import polars as pl


def _format_k(n: int) -> str:
    """Format a number as 'Xk' if divisible by 1000, otherwise as a plain integer string."""
    return f"{n // 1000}k" if n % 1000 == 0 else str(n)


def categorize_by_breaks(
    df: pl.DataFrame,
    col: str,
    breaks: list[int],
    alias: str = "count_category",
) -> pl.DataFrame:
    """Bin a numeric column into categories defined by explicit break points.

    For breaks=[15000, 30000] produces three categories:
      <15k   (count < 15000)
      15k-30k  (15000 <= count <= 30000)
      >30k   (count > 30000)

    Numbers that are exact multiples of 1000 are formatted with a 'k' suffix.

    Args:
        df: Input DataFrame.
        col: Name of the column to categorize.
        breaks: Sorted list of break-point values defining category boundaries.
        alias: Name for the resulting category column.

    Returns:
        DataFrame with an additional column named alias.
    """
    expr = pl.when(pl.col(col) < breaks[0]).then(pl.lit(f"<{_format_k(breaks[0])}"))
    for i in range(1, len(breaks)):
        expr = expr.when(pl.col(col) <= breaks[i]).then(
            pl.lit(f"{_format_k(breaks[i - 1])}-{_format_k(breaks[i])}")
        )
    expr = expr.otherwise(pl.lit(f">{_format_k(breaks[-1])}")).alias(alias)
    return df.with_columns(expr)


def bin_by_width(
    df: pl.DataFrame,
    col: str,
    binning_width: int,
    max_size: int,
    alias: str = "final_size_category",
) -> pl.DataFrame:
    """Bin a numeric column into equal-width bins with an overflow category.

    Bins span [i*binning_width, (i+1)*binning_width) up to max_size, labelled
    'low-high' (e.g. '0-2499'). Values >= max_size receive the label '>=max_size'.

    Args:
        df: Input DataFrame.
        col: Name of the column to bin.
        binning_width: Width of each bin.
        max_size: Values at or above this threshold go into the overflow bin.
        alias: Name for the resulting bin-label column.

    Returns:
        DataFrame with an additional column named alias.
    """
    expr = (
        pl.when(pl.col(col) >= max_size)
        .then(pl.lit(f">={max_size}"))
        .otherwise(
            (pl.col(col) // binning_width * binning_width).cast(pl.Utf8)
            + pl.lit("-")
            + (pl.col(col) // binning_width * binning_width + binning_width - 1).cast(
                pl.Utf8
            )
        )
        .alias(alias)
    )
    return df.with_columns(expr)


def make_bin_labels(binning_width: int, max_size: int) -> list[str]:
    """Return the ordered list of labels produced by bin_by_width.

    Useful for specifying category order in plots and cross-joining expected
    categories to ensure zero counts are preserved.

    Args:
        binning_width: Width of each bin (same value passed to bin_by_width).
        max_size: Overflow threshold (same value passed to bin_by_width).

    Returns:
        List of label strings in ascending order, with the overflow label last.
    """
    return [
        f"{i * binning_width}-{i * binning_width + binning_width - 1}"
        for i in range(max_size // binning_width)
    ] + [f">={max_size}"]
