import argparse
import datetime as dt
import json
import math
from dataclasses import dataclass
from pathlib import Path

import matplotlib.pyplot as plt
import numpy as _np
import numpy as np
import polars as pl
import seaborn as sns
from scipy import stats as _scipy_stats

from vhf_pipeline import paths
from vhf_pipeline.cli.run_model import _load_json
from vhf_pipeline.pipeline.shared_plotting_data import (
    get_cumulative_cases,
    load_scenario_data,
    save_particle_detection_rate,
)
from vhf_pipeline.utils import (
    get_cumulative_symptomatic_cases,
    load_intervention_date,
    load_simulation_start_date,
)

CATEGORY_ORDER = ["20-30%", "30-40%", "40-50%", "50-60%"]


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Join particle confirmation ratio outputs to per-particle Rt values and "
            "summarize them in a 4x4 inverse-ratio category table."
        )
    )
    parser.add_argument("--output-subdir", required=True)
    parser.add_argument("--calibration-subdir", required=True)
    parser.add_argument("--rt-window-days", type=int, default=15)
    return parser.parse_args()


def _ratio_category(column_name: str) -> pl.Expr:
    return (
        pl.when((pl.col(column_name) >= 0.2) & (pl.col(column_name) < 0.3))
        .then(pl.lit("20-30%"))
        .when((pl.col(column_name) >= 0.3) & (pl.col(column_name) < 0.4))
        .then(pl.lit("30-40%"))
        .when((pl.col(column_name) >= 0.4) & (pl.col(column_name) < 0.5))
        .then(pl.lit("40-50%"))
        .when((pl.col(column_name) >= 0.5) & (pl.col(column_name) <= 0.6))
        .then(pl.lit("50-60%"))
        .otherwise(None)
    )


def _category_grid() -> pl.DataFrame:
    return pl.DataFrame({"earlier_ratio_category": CATEGORY_ORDER}).join(
        pl.DataFrame({"later_ratio_category": CATEGORY_ORDER}), how="cross"
    )


def _resolve_rt_window(
    intervention_start_date: dt.date,
    delay: int = 0,
    duration: int = 15,
) -> tuple[dt.date, dt.date]:
    """Resolve the Rt window start/end dates from the intervention date."""
    start = intervention_start_date + dt.timedelta(days=delay)
    return start, start + dt.timedelta(days=duration - 1)


def _load_particle_rt(
    run_dir: Path,
    window_delay_from_intervention: int = 0,
    window_duration: int = 15,
) -> tuple[pl.DataFrame, dt.date, dt.date]:
    start_date, end_date = _resolve_rt_window(
        load_intervention_date(run_dir / "config.json"),
        delay=window_delay_from_intervention,
        duration=window_duration,
    )
    particle_rt = (
        pl.read_csv(run_dir / "projection" / "all_rt_reports.csv")
        .with_columns(pl.col("infection_date").cast(pl.Date))
        .filter(
            (pl.col("infection_date") >= start_date)
            & (pl.col("infection_date") <= end_date)
        )
        .group_by("particle_id")
        .agg(
            pl.median("avg_onward_infections").alias("particle_rt"),
            pl.mean("avg_onward_infections").alias("particle_rt_mean"),
        )
    )
    return particle_rt, start_date, end_date


def _load_particle_basic_rt(
    run_dir: Path,
    intervention_date: dt.date,
    window_days: int = 14,
) -> pl.DataFrame:
    end_date = intervention_date - dt.timedelta(days=1)
    start_date = end_date - dt.timedelta(days=window_days - 1)
    return (
        pl.read_csv(run_dir / "projection" / "all_rt_reports.csv")
        .with_columns(pl.col("infection_date").cast(pl.Date))
        .filter(
            (pl.col("infection_date") >= start_date)
            & (pl.col("infection_date") <= end_date)
        )
        .group_by("particle_id")
        .agg(pl.median("avg_onward_infections").alias("basic_rt"))
    )


def _build_pretty_summary(summary_df: pl.DataFrame) -> pl.DataFrame:
    rounded = summary_df.with_columns(
        pl.col("median_rt").round(2),
        pl.col("lower_iqr_rt").round(2),
        pl.col("upper_iqr_rt").round(2),
    )
    return rounded.with_columns(
        pl.when(pl.col("particle_count").fill_null(0) == 0)
        .then(pl.lit(""))
        .otherwise(
            pl.concat_str(
                [
                    pl.col("median_rt").cast(pl.String),
                    pl.lit(" ("),
                    pl.col("lower_iqr_rt").cast(pl.String),
                    pl.lit("-"),
                    pl.col("upper_iqr_rt").cast(pl.String),
                    pl.lit("); n="),
                    pl.col("particle_count").cast(pl.Int64).cast(pl.String),
                ],
                separator="",
            )
        )
        .alias("summary")
    )


_SPILLOVER_COL = "initialization.initial_cases.SpilloverEvent.days_since_start"
_OFFSPRING_SCALAR_COL = "offspring_intervention.scalar"
_SHAPE_COL = "testing_config.sample_collection_delay.Gamma.shape"
_RATE_COL = "testing_config.sample_collection_delay.Gamma.rate"
_GI_SHAPE_COL = "generation_interval_distribution.OffsetWeibull.shape"
_GI_OFFSET_COL = "generation_interval_distribution.OffsetWeibull.offset"
_GI_SCALE_COL = "generation_interval_distribution.OffsetWeibull.scale"
_CONFIRMATION_DELAY_ROW = "_confirmation_delay"
_GI_ROW = "_generation_interval_days"


@dataclass
class _PParam:
    """Metadata describing one row in the prior/posterior parameter table."""

    col: str
    display: str
    decimals: int = 2
    is_date: bool = False
    prior_key: str | None = None  # overrides col when the priors JSON key differs


_PRIOR_POSTERIOR_PARAMS: list[_PParam] = [
    _PParam(
        "offspring_distribution.NegativeBinomial.mean",
        "Mean of negative binomial offspring distribution",
    ),
    _PParam(
        "offspring_distribution.NegativeBinomial.concentration", "Concentration, k"
    ),
    _PParam(_GI_SCALE_COL, "Generation interval scale"),
    _PParam(_GI_OFFSET_COL, "Generation interval offset"),
    _PParam(_GI_ROW, "Generation interval (days)"),
    _PParam("recovery_delay_distribution.Fixed.delay", "Recovery delay", decimals=1),
    _PParam("mortality_delay_distribution.Fixed.delay", "Mortality delay", decimals=1),
    _PParam(
        _SPILLOVER_COL,
        "Spillover date",
        is_date=True,
        prior_key="SpilloverEvent.days_since_start",
    ),
    _PParam(_OFFSPRING_SCALAR_COL, "Transmission coefficient scalar"),
    _PParam(_RATE_COL, "Confirmation delay distribution rate"),
    _PParam(_SHAPE_COL, "Confirmation delay distribution shape"),
    _PParam(_CONFIRMATION_DELAY_ROW, "Confirmation delay"),
]


def _spillover_ordinals_expr(base_ord: int) -> pl.Expr:
    """Integer date-ordinal per particle for spillover timing, rounded to nearest day.

    Both ``_build_summary_table`` and ``_build_prior_posterior_table`` must call
    this helper so that the per-particle conversion (float days → int ordinal)
    uses identical rounding before any aggregation takes place.
    """
    return (pl.lit(base_ord) + pl.col(_SPILLOVER_COL).floor().cast(pl.Int64)).alias(
        "spillover_ordinal"
    )


def _late_category_grid() -> pl.DataFrame:
    return pl.DataFrame({"later_ratio_category": CATEGORY_ORDER})


def _summarize_by_late_category(joined: pl.DataFrame) -> pl.DataFrame:
    """Rt summary grouped by late-detection bin only."""
    return (
        joined.group_by("later_ratio_category")
        .agg(
            pl.len().alias("particle_count"),
            pl.median("particle_rt").alias("median_rt"),
            pl.quantile("particle_rt", 0.25).alias("lower_iqr_rt"),
            pl.quantile("particle_rt", 0.75).alias("upper_iqr_rt"),
        )
        .join(_late_category_grid(), on="later_ratio_category", how="right")
        .sort("later_ratio_category")
    )


def _load_inputs_df(run_dir: Path) -> pl.DataFrame:
    return pl.read_csv(run_dir / "projection" / "all_simulation_inputs.csv")


def _build_pretty_inputs_summary(
    summary_df: pl.DataFrame,
    cols: list[str],
) -> pl.DataFrame:
    """Format median (IQR) strings for each input column in the summary."""
    format_exprs = [
        pl.when(pl.col(f"median_{col}").is_null())
        .then(pl.lit(""))
        .otherwise(
            pl.concat_str(
                [
                    pl.col(f"median_{col}").round(2).cast(pl.String),
                    pl.lit(" ("),
                    pl.col(f"lower_iqr_{col}").round(2).cast(pl.String),
                    pl.lit("-"),
                    pl.col(f"upper_iqr_{col}").round(2).cast(pl.String),
                    pl.lit(")"),
                ],
                separator="",
            )
        )
        .alias(col)
        for col in cols
    ]
    return summary_df.with_columns(format_exprs).select(["later_ratio_category"] + cols)


def _summarize_inputs_by_late_category(
    joined: pl.DataFrame, inputs_df: pl.DataFrame
) -> pl.DataFrame:
    """Summarize spillover timing and offspring intervention scalar by late-detection bin."""
    cols = [
        c for c in [_SPILLOVER_COL, _OFFSPRING_SCALAR_COL] if c in inputs_df.columns
    ]
    if not cols:
        return _late_category_grid()
    agg_exprs = [
        expr
        for col in cols
        for expr in [
            pl.median(col).alias(f"median_{col}"),
            pl.quantile(col, 0.25).alias(f"lower_iqr_{col}"),
            pl.quantile(col, 0.75).alias(f"upper_iqr_{col}"),
        ]
    ]
    return (
        joined.select("particle_id", "later_ratio_category")
        .join(inputs_df.select(["particle_id"] + cols), on="particle_id", how="left")
        .group_by("later_ratio_category")
        .agg(agg_exprs)
        .join(_late_category_grid(), on="later_ratio_category", how="right")
        .sort("later_ratio_category")
    )


def _load_particle_outbreak_size(
    run_dir: Path, projection_date: dt.date
) -> pl.DataFrame:
    return (
        get_cumulative_symptomatic_cases(
            pl.read_csv(run_dir / "projection" / "all_prevalence_reports.csv"),
            max_date=projection_date,
        )
        .group_by("particle_id")
        .agg(pl.max("count").alias("final_outbreak_size"))
    )


def _summarize_outbreak_size_by_late_category(
    joined: pl.DataFrame,
    outbreak_size_df: pl.DataFrame,
) -> pl.DataFrame:
    """Summarize cumulative outbreak size by late-detection bin."""
    return (
        joined.select("particle_id", "later_ratio_category")
        .join(outbreak_size_df, on="particle_id", how="left")
        .group_by("later_ratio_category")
        .agg(
            pl.len().alias("particle_count"),
            pl.median("final_outbreak_size").alias("median_outbreak_size"),
            pl.quantile("final_outbreak_size", 0.25).alias("lower_iqr_outbreak_size"),
            pl.quantile("final_outbreak_size", 0.75).alias("upper_iqr_outbreak_size"),
        )
        .join(_late_category_grid(), on="later_ratio_category", how="right")
        .sort("later_ratio_category")
    )


def _format_median_iqi(
    median: float | None,
    q25: float | None,
    q75: float | None,
    decimals: int = 2,
) -> str:
    """Format a median (Q1-Q3) string, returning empty string for nulls."""
    if median is None or q25 is None or q75 is None:
        return ""
    if decimals <= 0:
        return f"{int(round(median, decimals))} ({int(round(q25, decimals))}-{int(round(q75, decimals))})"
    return f"{round(median, decimals)} ({round(q25, decimals)}-{round(q75, decimals)})"


def _format_date_median_iqi(
    median_ord: float | None,
    q25_ord: float | None,
    q75_ord: float | None,
) -> str:
    if median_ord is None or q25_ord is None or q75_ord is None:
        return ""

    def _from_ordinal(o: float):
        return dt.date.fromordinal(int(round(o))).strftime("%b %d")

    return f"{_from_ordinal(median_ord)} ({_from_ordinal(q25_ord)}-{_from_ordinal(q75_ord)})"


def _make_dist(spec: dict):
    """Return a frozen scipy distribution from a prior spec dict."""
    name, p = spec["distribution"], spec["parameters"]
    if name == "normal":
        return _scipy_stats.norm(p["mean"], p["std_dev"])
    if name == "lognormal":
        return _scipy_stats.lognorm(s=p["std_dev"], scale=math.exp(p["mean"]))
    if name == "uniform":
        return _scipy_stats.uniform(p["min"], p["max"] - p["min"])
    if name == "beta":
        return _scipy_stats.beta(p["alpha"], p["beta"])
    raise ValueError(f"Unknown prior distribution: {name!r}")


def _compute_prior_stats(prior_spec: dict) -> tuple[float, float, float]:
    d = _make_dist(prior_spec)
    return float(d.ppf(0.5)), float(d.ppf(0.25)), float(d.ppf(0.75))


def _sample_distribution(
    prior_spec: dict, n: int = 1000, rng: np.random.Generator | None = None
) -> "_np.ndarray":
    return _make_dist(prior_spec).rvs(size=n, random_state=rng)


def _sample_gamma_delays(
    shape_arr: "_np.ndarray",
    rate_arr: "_np.ndarray",
    sample_size: int = 100,
    rng: np.random.Generator | None = None,
) -> "_np.ndarray":
    """Draw bootstrapped Gamma(shape, scale=1/rate) delay samples from paired arrays."""
    return _scipy_stats.gamma.rvs(
        a=shape_arr,
        scale=1.0 / rate_arr,
        size=(sample_size, len(rate_arr)),
        random_state=rng,
    ).flatten()


def _iqi_of_sampled_gamma_delay(
    shape_arr: "_np.ndarray",
    rate_arr: "_np.ndarray",
    decimals: int = 2,
    sample_size: int = 100,
    rng: np.random.Generator | None = None,
) -> str:
    """Median (IQI) string for bootstrapped Gamma(shape, scale=1/rate) confirmation delays.

    For the prior, pass independently sampled shape/rate arrays; for the posterior,
    pass particle-wise joint samples to account for the full joint distribution.
    """
    s = _sample_gamma_delays(shape_arr, rate_arr, sample_size, rng=rng)
    return _format_median_iqi(
        float(_np.median(s)),
        float(_np.percentile(s, 25)),
        float(_np.percentile(s, 75)),
        decimals,
    )


def _sample_offset_weibull_gi(
    offset_arr: "_np.ndarray",
    scale_arr: "_np.ndarray",
    shape: float,
    sample_size: int = 100,
    rng: np.random.Generator | None = None,
) -> "_np.ndarray":
    """Draw bootstrapped offset + Weibull(c=shape, scale) samples from paired arrays."""
    return (
        _scipy_stats.weibull_min.rvs(
            c=shape,
            scale=scale_arr,
            size=(sample_size, len(scale_arr)),
            random_state=rng,
        )
        + offset_arr
    ).flatten()


def _iqi_of_sampled_offset_weibull_gi(
    offset_arr: "_np.ndarray",
    scale_arr: "_np.ndarray",
    shape: float,
    decimals: int = 2,
    sample_size: int = 100,
    rng: np.random.Generator | None = None,
) -> str:
    """Median (IQI) string for bootstrapped offset + Weibull(shape, scale) generation intervals.

    For the prior, pass independently sampled offset/scale arrays; for the posterior,
    pass particle-wise joint samples to account for the full joint distribution.
    """
    s = _sample_offset_weibull_gi(offset_arr, scale_arr, shape, sample_size, rng=rng)
    return _format_median_iqi(
        float(_np.median(s)),
        float(_np.percentile(s, 25)),
        float(_np.percentile(s, 75)),
        decimals,
    )


def _plot_distribution_by_category(
    joined: pl.DataFrame,
    inputs_df: pl.DataFrame,
    figures_dir: Path,
    param_cols: list[str],
    pool_sampler,
    group_sampler,
    pdf_fn,
    title: str,
    filename: str,
    color: str,
) -> None:
    """Plot per-particle distribution PDFs faceted by later ratio category.

    ``pool_sampler(arrays)`` and ``group_sampler(arrays)`` each receive a list of
    numpy arrays (one per ``param_col``) and return a flat sample array.
    ``pdf_fn(x, j, arrays)`` evaluates the PDF at ``x`` for the j-th particle.
    """
    joined_params = joined.select("particle_id", "later_ratio_category").join(
        inputs_df.select(["particle_id"] + param_cols), on="particle_id", how="left"
    )
    pooled = pool_sampler(
        [joined_params.drop_nulls(param_cols)[c].to_numpy() for c in param_cols]
    )
    x = _np.linspace(0.0, float(_np.percentile(pooled, 99)) * 1.1, 500)

    fig, axes = plt.subplots(2, 2, figsize=(13, 10), sharey=False, sharex=True)
    for i, (ax, category) in enumerate(zip(axes.flatten(), CATEGORY_ORDER)):
        subset = joined_params.filter(pl.col("later_ratio_category") == category)
        n = subset.height
        ax.set_title(f"Later ratio: {category} (n={n})")
        ax.set_xlabel("Days")
        ax.set_ylabel("Density")
        if n == 0:
            continue
        arrs = [subset[c].to_numpy() for c in param_cols]
        for j in _np.random.default_rng(42).choice(n, size=min(n, 200), replace=False):
            ax.plot(x, pdf_fn(x, j, arrs), color=color, alpha=0.07, linewidth=0.6)
        samples = group_sampler(arrs)
        q25, q75 = (
            float(_np.percentile(samples, 25)),
            float(_np.percentile(samples, 75)),
        )
        ax.axvspan(
            q25,
            q75,
            alpha=0.30,
            color="orange",
            label="Bootstrapped IQI (Q25\u2013Q75)" if i == 0 else "_nolegend_",
        )
        ax.axvline(
            float(_np.median(samples)),
            color="darkorange",
            linestyle="--",
            linewidth=1.5,
            label="Bootstrapped median" if i == 0 else "_nolegend_",
        )

    handles, labels = axes.flatten()[0].get_legend_handles_labels()
    fig.legend(handles, labels, loc="upper right")
    fig.suptitle(title)
    plt.tight_layout()
    plt.savefig(figures_dir / filename, dpi=300)
    plt.clf()


def _build_prior_posterior_table(
    joined: pl.DataFrame,
    inputs_df: pl.DataFrame,
    priors_path: Path,
    simulation_start_date: dt.date,
    entropy: int,
) -> pl.DataFrame:
    """Compare prior and posterior distributions per later-detection bin.

    Rows are model parameters; columns are a Prior column and one per
    detection-ratio category. Spillover timing is formatted as a calendar date.
    """
    priors_spec = _load_json(priors_path)["priors"]
    base_ord = simulation_start_date.toordinal()

    cat_counts: dict[str, int] = {cat: 0 for cat in CATEGORY_ORDER}
    for cat, n in joined.group_by("later_ratio_category").agg(pl.len()).iter_rows():
        if cat in cat_counts:
            cat_counts[cat] = int(n)
    headers = [f"{cat}, median (IQI) (n = {cat_counts[cat]})" for cat in CATEGORY_ORDER]

    parameter_col: list[str] = []
    prior_col: list[str] = []
    posterior_data: dict[str, list[str]] = {h: [] for h in headers}

    # Single RNG for all reproducible sampling in this table
    rng = np.random.default_rng(entropy)

    for param in _PRIOR_POSTERIOR_PARAMS:
        parameter_col.append(param.display)
        col, decimals, is_date = param.col, param.decimals, param.is_date
        prior_key = param.prior_key or col

        # ── Composite GI row ──────────────────────────────────────────────
        if col == _GI_ROW:
            gi_shape = (
                float(inputs_df[_GI_SHAPE_COL].drop_nulls()[0])
                if _GI_SHAPE_COL in inputs_df.columns
                else None
            )
            if (
                gi_shape is not None
                and _GI_OFFSET_COL in priors_spec
                and _GI_SCALE_COL in priors_spec
            ):
                prior_col.append(
                    _iqi_of_sampled_offset_weibull_gi(
                        _sample_distribution(priors_spec[_GI_OFFSET_COL], rng=rng),
                        _sample_distribution(priors_spec[_GI_SCALE_COL], rng=rng),
                        gi_shape,
                        decimals,
                        rng=rng,
                    )
                )
            else:
                prior_col.append("")
            if (
                gi_shape is not None
                and _GI_OFFSET_COL in inputs_df.columns
                and _GI_SCALE_COL in inputs_df.columns
            ):
                joined_gi = joined.select("particle_id", "later_ratio_category").join(
                    inputs_df.select(["particle_id", _GI_OFFSET_COL, _GI_SCALE_COL]),
                    on="particle_id",
                    how="left",
                )
                for cat, h in zip(CATEGORY_ORDER, headers):
                    subset = joined_gi.filter(pl.col("later_ratio_category") == cat)
                    posterior_data[h].append(
                        _iqi_of_sampled_offset_weibull_gi(
                            subset[_GI_OFFSET_COL].to_numpy(),
                            subset[_GI_SCALE_COL].to_numpy(),
                            gi_shape,
                            decimals,
                            rng=rng,
                        )
                        if subset.height > 0
                        else ""
                    )
            else:
                for h in headers:
                    posterior_data[h].append("")
            continue

        # ── Composite confirmation delay row ──────────────────────────────
        if col == _CONFIRMATION_DELAY_ROW:
            if _SHAPE_COL in priors_spec and _RATE_COL in priors_spec:
                prior_col.append(
                    _iqi_of_sampled_gamma_delay(
                        _sample_distribution(priors_spec[_SHAPE_COL], rng=rng),
                        _sample_distribution(priors_spec[_RATE_COL], rng=rng),
                        decimals,
                        rng=rng,
                    )
                )
            else:
                prior_col.append("")
            if _SHAPE_COL in inputs_df.columns and _RATE_COL in inputs_df.columns:
                joined_delay = joined.select(
                    "particle_id", "later_ratio_category"
                ).join(
                    inputs_df.select(["particle_id", _SHAPE_COL, _RATE_COL]),
                    on="particle_id",
                    how="left",
                )
                for cat, h in zip(CATEGORY_ORDER, headers):
                    subset = joined_delay.filter(pl.col("later_ratio_category") == cat)
                    posterior_data[h].append(
                        _iqi_of_sampled_gamma_delay(
                            subset[_SHAPE_COL].to_numpy(),
                            subset[_RATE_COL].to_numpy(),
                            decimals,
                            rng=rng,
                        )
                        if subset.height > 0
                        else ""
                    )
            else:
                for h in headers:
                    posterior_data[h].append("")
            continue

        # ── Prior ─────────────────────────────────────────────────────────
        if prior_key in priors_spec:
            med, q25, q75 = _compute_prior_stats(priors_spec[prior_key])
            prior_col.append(
                _format_date_median_iqi(base_ord + med, base_ord + q25, base_ord + q75)
                if is_date
                else _format_median_iqi(med, q25, q75, decimals=decimals)
            )
        else:
            prior_col.append("")

        # ── Posterior per category ────────────────────────────────────────
        if col not in inputs_df.columns:
            for h in headers:
                posterior_data[h].append("")
            continue

        joined_col = joined.select("particle_id", "later_ratio_category").join(
            inputs_df.select(["particle_id", col]), on="particle_id", how="left"
        )
        if is_date:
            joined_col = joined_col.with_columns(_spillover_ordinals_expr(base_ord))
            lookup_col = "spillover_ordinal"
        else:
            lookup_col = col

        for cat, h in zip(CATEGORY_ORDER, headers):
            values = joined_col.filter(pl.col("later_ratio_category") == cat)[
                lookup_col
            ]
            if values.drop_nulls().len() == 0:
                posterior_data[h].append("")
            elif is_date:
                posterior_data[h].append(
                    _format_date_median_iqi(
                        values.median(), values.quantile(0.25), values.quantile(0.75)
                    )
                )
            else:
                posterior_data[h].append(
                    _format_median_iqi(
                        values.median(),
                        values.quantile(0.25),
                        values.quantile(0.75),
                        decimals=decimals,
                    )
                )

    return pl.DataFrame(
        {"Parameter": parameter_col, "Prior, median (IQI)": prior_col, **posterior_data}
    )


def _build_summary_table(
    joined: pl.DataFrame,
    inputs_df: pl.DataFrame,
    aug9_outbreak_size_df: pl.DataFrame,
    basic_rt_df: pl.DataFrame,
    simulation_start_date: dt.date,
    outbreak_size_date: dt.date,
) -> pl.DataFrame:
    """Build a wide-format summary table grouped by later_ratio_category."""
    input_cols_present = [
        c for c in [_SPILLOVER_COL, _OFFSPRING_SCALAR_COL] if c in inputs_df.columns
    ]
    has_reduction = _OFFSPRING_SCALAR_COL in input_cols_present
    has_spillover = _SPILLOVER_COL in input_cols_present
    base_ord = simulation_start_date.toordinal()

    metric_df = (
        joined.select(
            "particle_id",
            "later_ratio_category",
            "particle_rt",
            "observed_detection_rate_target_early",
        )
        .join(basic_rt_df, on="particle_id", how="left")
        .join(
            inputs_df.select(["particle_id"] + input_cols_present),
            on="particle_id",
            how="left",
        )
        .join(aug9_outbreak_size_df, on="particle_id", how="left")
    )

    transform_exprs = [
        (pl.col("observed_detection_rate_target_early") * 100).alias("detection_pct")
    ]
    if has_spillover:
        transform_exprs.append(_spillover_ordinals_expr(base_ord))
    if has_reduction:
        transform_exprs.append(
            ((1.0 - pl.col(_OFFSPRING_SCALAR_COL)) * 100).alias("reduction_pct")
        )
    metric_df = metric_df.with_columns(transform_exprs)

    agg_exprs: list[pl.Expr] = [
        pl.len().alias("n"),
        pl.median("final_outbreak_size").alias("median_outbreak"),
        pl.quantile("final_outbreak_size", 0.25).alias("q25_outbreak"),
        pl.quantile("final_outbreak_size", 0.75).alias("q75_outbreak"),
        pl.median("particle_rt").alias("median_eff_rt"),
        pl.quantile("particle_rt", 0.25).alias("q25_eff_rt"),
        pl.quantile("particle_rt", 0.75).alias("q75_eff_rt"),
        pl.median("basic_rt").alias("median_basic_rt"),
        pl.quantile("basic_rt", 0.25).alias("q25_basic_rt"),
        pl.quantile("basic_rt", 0.75).alias("q75_basic_rt"),
        pl.median("detection_pct").alias("median_detection"),
        pl.quantile("detection_pct", 0.25).alias("q25_detection"),
        pl.quantile("detection_pct", 0.75).alias("q75_detection"),
    ]
    if has_reduction:
        agg_exprs += [
            pl.median("reduction_pct").alias("median_reduction"),
            pl.quantile("reduction_pct", 0.25).alias("q25_reduction"),
            pl.quantile("reduction_pct", 0.75).alias("q75_reduction"),
        ]
    if has_spillover:
        agg_exprs += [
            pl.median("spillover_ordinal").alias("median_spillover_ord"),
            pl.quantile("spillover_ordinal", 0.25).alias("q25_spillover_ord"),
            pl.quantile("spillover_ordinal", 0.75).alias("q75_spillover_ord"),
        ]

    stats = (
        metric_df.group_by("later_ratio_category")
        .agg(agg_exprs)
        .join(_late_category_grid(), on="later_ratio_category", how="right")
        .sort("later_ratio_category")
    )

    row_names = [
        f"Cumulative symptomatic BVD illnesses {outbreak_size_date.strftime('%B %-d')}",
        "Effective reproductive number (>= May 24)",
        "Basic reproductive number",
        "Reduction in transmission (%)",
        "Spillover date",
        "Detection proportion (%) (June 1 - June 21)",
    ]
    table: dict[str, list[str]] = {"Modeled value": row_names}

    for cat in CATEGORY_ORDER:
        cat_rows = stats.filter(pl.col("later_ratio_category") == cat)
        if cat_rows.height == 0:
            table[f"{cat}, median (IQI) (n = 0)"] = [""] * len(row_names)
            continue
        r = cat_rows.row(0, named=True)
        n = int(r["n"]) if r["n"] is not None else 0
        table[f"{cat}, median (IQI) (n = {n})"] = [
            _format_median_iqi(
                r.get("median_outbreak"),
                r.get("q25_outbreak"),
                r.get("q75_outbreak"),
                decimals=-2,
            ),
            _format_median_iqi(
                r.get("median_eff_rt"), r.get("q25_eff_rt"), r.get("q75_eff_rt")
            ),
            _format_median_iqi(
                r.get("median_basic_rt"), r.get("q25_basic_rt"), r.get("q75_basic_rt")
            ),
            _format_median_iqi(
                r.get("median_reduction"),
                r.get("q25_reduction"),
                r.get("q75_reduction"),
                decimals=1,
            )
            if has_reduction
            else "",
            _format_date_median_iqi(
                r.get("median_spillover_ord"),
                r.get("q25_spillover_ord"),
                r.get("q75_spillover_ord"),
            )
            if has_spillover
            else "",
            _format_median_iqi(
                r.get("median_detection"),
                r.get("q25_detection"),
                r.get("q75_detection"),
                decimals=0,
            ),
        ]

    return pl.DataFrame(table)


def _plot_cumulative_cases_by_ratio_category(
    target_data: pl.DataFrame,
    onset_data: pl.DataFrame,
    prevalence_report_df: pl.DataFrame,
    joined: pl.DataFrame,
    figures_dir: Path,
    projection_date: dt.date,
) -> None:
    """Plot cumulative true case count vs confirmed case data, faceted by later ratio category."""
    max_report_date = target_data.select(pl.max("report_date")).item()
    min_date = dt.date(2026, 1, 1)

    category_particles = joined.select("particle_id", "later_ratio_category")
    cumulative_case_df = get_cumulative_cases(
        prevalence_report_df, max_report_date, projection_date
    )

    fig, axes = plt.subplots(2, 2, figsize=(13, 10), sharey=True, sharex=True)
    axes_flat = axes.flatten()

    for i, category in enumerate(CATEGORY_ORDER):
        ax = axes_flat[i]
        particle_ids = category_particles.filter(
            pl.col("later_ratio_category") == category
        )["particle_id"].to_list()
        n = len(particle_ids)

        if not particle_ids:
            ax.set_title(f"Later ratio: {category} (n=0)")
            continue

        onset_filtered = onset_data.with_columns(
            pl.col("count")
            .cum_sum()
            .over("particle_id", order_by="date")
            .alias("cumulative_count")
        ).filter(
            (pl.col("date") <= max_report_date)
            & (pl.col("date") >= min_date)
            & pl.col("particle_id").is_in(particle_ids)
        )
        if onset_filtered.height > 0:
            sns.lineplot(
                data=onset_filtered,
                x="date",
                y="cumulative_count",
                estimator=None,
                units="particle_id",
                alpha=0.1,
                ax=ax,
            )

        cum_cases_filtered = cumulative_case_df.filter(
            pl.col("particle_id").is_in(particle_ids)
        )
        if cum_cases_filtered.height > 0:
            sns.lineplot(
                data=cum_cases_filtered,
                x="date",
                y="cumulative_cases",
                estimator=None,
                units="particle_id",
                color="#666666",
                alpha=0.1,
                ax=ax,
            )

        sns.scatterplot(
            data=target_data.filter(pl.col("date") <= max_report_date),
            x="date",
            y="cumulative_confirmation_incidence",
            color="red",
            zorder=10,
            label="Target Data" if i == 0 else "_nolegend_",
            ax=ax,
        )

        ax.set_title(f"Later ratio: {category} (n={n})")
        ax.set_xlabel("Date of symptom onset")
        ax.set_ylabel("Cumulative true case count")
        ax.set_ylim(0, 30000)
        ax.set_xlim(left=min_date)
        ax.tick_params(axis="x", rotation=45)

    handles, labels = axes_flat[0].get_legend_handles_labels()
    fig.legend(handles, labels, loc="upper right")
    fig.suptitle(
        "True case count vs confirmed case data\nfaceted by later-period detection ratio category"
    )
    plt.tight_layout()
    plt.savefig(figures_dir / "cumulative_cases_by_ratio_category.png", dpi=300)
    plt.clf()


def main(
    output_subdir: str,
    calibration_subdir: str,
    projection_date: dt.date,
    rt_window_days: int = 15,
) -> None:
    output_dir = paths.output_dir(output_subdir)
    run_dir = output_dir / calibration_subdir
    products_dir = run_dir / "products"
    products_dir.mkdir(parents=True, exist_ok=True)
    figures_dir = products_dir / "figures"
    figures_dir.mkdir(parents=True, exist_ok=True)

    data = load_scenario_data(output_dir, calibration_subdir, products_dir)
    # Load simulations
    _all_sims_df = pl.read_csv(run_dir / "projection" / "all_simulations.csv")
    if "epiweek_startdate" in _all_sims_df.columns:
        projected_report_data = _all_sims_df.with_columns(
            pl.col("epiweek_startdate").cast(pl.Date)
        ).rename({"epiweek_startdate": "date"})
    else:
        projected_report_data = _all_sims_df.with_columns(pl.col("date").cast(pl.Date))
    if not (products_dir / "particle_confirmation_errors.csv").exists():
        save_particle_detection_rate(
            target_data=data["target_data"],
            projected_report_data=projected_report_data,
            products_dir=products_dir,
        )

    ratio_df = pl.read_csv(
        products_dir / "particle_observed_detection.csv", try_parse_dates=True
    )
    particle_rt, rt_start_date, rt_end_date = _load_particle_rt(
        run_dir=run_dir,
        window_delay_from_intervention=1,
        window_duration=rt_window_days,
    )
    joined = (
        ratio_df.join(particle_rt, on="particle_id", how="inner")
        .with_columns(
            _ratio_category("observed_detection_rate_target_early").alias(
                "earlier_ratio_category"
            ),
            _ratio_category("observed_detection_rate_target_late").alias(
                "later_ratio_category"
            ),
        )
        .drop_nulls(["earlier_ratio_category", "later_ratio_category"])
        .sort("particle_id")
    )
    joined.write_csv(products_dir / "particle_confirmation_errors_with_rt.csv")
    pl.DataFrame(
        {
            "rt_window_start": [rt_start_date],
            "rt_window_end": [rt_end_date],
            "rt_window_days": [rt_window_days],
        }
    ).write_csv(products_dir / "rt_by_ratio_category_window.csv")

    # ── 4×4 Rt table ──────────────────────────────────────────────────────────
    summary = (
        joined.group_by("earlier_ratio_category", "later_ratio_category")
        .agg(
            pl.len().alias("particle_count"),
            pl.median("particle_rt").alias("median_rt"),
            pl.quantile("particle_rt", 0.25).alias("lower_iqr_rt"),
            pl.quantile("particle_rt", 0.75).alias("upper_iqr_rt"),
        )
        .join(
            _category_grid(),
            on=["earlier_ratio_category", "later_ratio_category"],
            how="right",
        )
        .select(
            "earlier_ratio_category",
            "later_ratio_category",
            "particle_count",
            "median_rt",
            "lower_iqr_rt",
            "upper_iqr_rt",
        )
        .sort("earlier_ratio_category", "later_ratio_category")
    )
    summary.write_csv(products_dir / "rt_by_ratio_category_long.csv")
    _build_pretty_summary(summary).select(
        "earlier_ratio_category", "later_ratio_category", "summary"
    ).pivot(
        on="later_ratio_category",
        index="earlier_ratio_category",
        values="summary",
        sort_columns=False,
    ).write_csv(products_dir / "rt_by_ratio_category_table.csv")
    summary.select(
        "earlier_ratio_category", "later_ratio_category", "particle_count"
    ).pivot(
        on="later_ratio_category",
        index="earlier_ratio_category",
        values="particle_count",
        sort_columns=False,
    ).write_csv(products_dir / "rt_by_ratio_category_counts.csv")

    # ── Late-detection Rt table ───────────────────────────────────────────────
    late_rt_summary = _summarize_by_late_category(joined)
    late_rt_summary.write_csv(products_dir / "rt_by_late_category_long.csv")
    _build_pretty_summary(late_rt_summary).select(
        "later_ratio_category", "summary"
    ).write_csv(products_dir / "rt_by_late_category_table.csv")

    # ── Inputs summary by late bin ────────────────────────────────────────────
    inputs_df = _load_inputs_df(run_dir)
    inputs_cols_present = [
        c for c in [_SPILLOVER_COL, _OFFSPRING_SCALAR_COL] if c in inputs_df.columns
    ]
    inputs_summary = _summarize_inputs_by_late_category(joined, inputs_df)
    inputs_summary.write_csv(products_dir / "inputs_by_late_category_long.csv")
    if inputs_cols_present:
        _build_pretty_inputs_summary(inputs_summary, inputs_cols_present).write_csv(
            products_dir / "inputs_by_late_category_table.csv"
        )

    # ── Outbreak size by late bin ─────────────────────────────────────────────
    outbreak_size_df = _load_particle_outbreak_size(
        run_dir, projection_date=projection_date
    )
    _summarize_outbreak_size_by_late_category(joined, outbreak_size_df).write_csv(
        products_dir / "outbreak_size_by_late_category.csv"
    )

    # ── Figures ───────────────────────────────────────────────────────────────
    prevalence_report_df = pl.read_csv(
        run_dir / "projection" / "all_prevalence_reports.csv"
    )
    _plot_cumulative_cases_by_ratio_category(
        target_data=data["target_data"],
        onset_data=data["onset_data"],
        prevalence_report_df=prevalence_report_df,
        joined=joined,
        figures_dir=figures_dir,
        projection_date=projection_date,
    )

    if _SHAPE_COL in inputs_df.columns and _RATE_COL in inputs_df.columns:
        _plot_distribution_by_category(
            joined,
            inputs_df,
            figures_dir,
            param_cols=[_SHAPE_COL, _RATE_COL],
            pool_sampler=lambda a: _sample_gamma_delays(a[0], a[1], sample_size=5),
            group_sampler=lambda a: _sample_gamma_delays(a[0], a[1]),
            pdf_fn=lambda x, j, a: _scipy_stats.gamma.pdf(
                x, a=a[0][j], scale=1.0 / a[1][j]
            ),
            title="Confirmation delay distribution by later-period detection ratio category",
            filename="confirmation_delay_by_ratio_category.png",
            color="#1f77b4",
        )

    gi_params = [_GI_OFFSET_COL, _GI_SCALE_COL]
    if all(c in inputs_df.columns for c in gi_params + [_GI_SHAPE_COL]):
        gi_shape = float(inputs_df[_GI_SHAPE_COL].drop_nulls()[0])
        _plot_distribution_by_category(
            joined,
            inputs_df,
            figures_dir,
            param_cols=gi_params,
            pool_sampler=lambda a: _sample_offset_weibull_gi(
                a[0], a[1], gi_shape, sample_size=5
            ),
            group_sampler=lambda a: _sample_offset_weibull_gi(a[0], a[1], gi_shape),
            pdf_fn=lambda x, j, a: _scipy_stats.weibull_min.pdf(
                x, c=gi_shape, scale=a[1][j], loc=a[0][j]
            ),
            title="Generation interval distribution by later-period detection ratio category",
            filename="generation_interval_by_ratio_category.png",
            color="#2ca02c",
        )

    # ── Summary and prior/posterior tables ────────────────────────────────────
    config_file = run_dir / "config.json"
    simulation_start_date = load_simulation_start_date(config_file)
    intervention_date = load_intervention_date(config_file)
    basic_rt_df = _load_particle_basic_rt(
        run_dir=run_dir, intervention_date=intervention_date
    )
    outbreak_size_date = data["target_data"].select(
        pl.max("date")
    ).item() + dt.timedelta(days=6)
    aug9_outbreak_size_df = (
        get_cumulative_symptomatic_cases(
            prevalence_report_df, max_date=outbreak_size_date
        )
        .filter(pl.col("date") == outbreak_size_date)
        .rename({"count": "final_outbreak_size"})
    )
    _build_summary_table(
        joined=joined,
        inputs_df=inputs_df,
        aug9_outbreak_size_df=aug9_outbreak_size_df,
        basic_rt_df=basic_rt_df,
        simulation_start_date=simulation_start_date,
        outbreak_size_date=outbreak_size_date,
    ).write_csv(products_dir / "summary_table.csv")

    config_json = json.loads(config_file.read_text())
    _build_prior_posterior_table(
        joined=joined,
        inputs_df=inputs_df,
        priors_path=Path(config_json["priors_file"]),
        simulation_start_date=simulation_start_date,
        entropy=config_json["calibration"]["entropy"],
    ).write_csv(products_dir / "prior_posterior_parameter_table.csv")


if __name__ == "__main__":
    args = _parse_args()
    main(
        args.output_subdir,
        args.calibration_subdir,
        projection_date=dt.date(2026, 9, 30),
        rt_window_days=args.rt_window_days,
    )
