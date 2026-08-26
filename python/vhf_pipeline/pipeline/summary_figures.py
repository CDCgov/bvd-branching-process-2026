import datetime as dt
import os
from pathlib import Path
from typing import Literal

import matplotlib.pyplot as plt
import polars as pl
import polars.selectors as cs
import seaborn as sns

from vhf_pipeline import paths
from vhf_pipeline.utils import (
    categorize_by_breaks,
    get_cumulative_symptomatic_cases,
    load_intervention_date,
    read_griddle,
)


def categorize_cumulative_cases(
    df: pl.DataFrame,
    max_date: dt.date,
    bin_breaks: list[int] = [15000, 30000],
    detected_only: bool = False,
) -> pl.DataFrame:
    return categorize_by_breaks(
        get_cumulative_symptomatic_cases(
            df.filter(pl.col("date").cast(pl.Date) <= max_date),
            max_date=max_date,
            detected_only=detected_only,
        ),
        col="count",
        breaks=bin_breaks,
    )


def group_sizes(
    prevalence_report: pl.DataFrame,
    max_date: dt.date,
    scenario_name: str,
    bin_breaks: list[int] = [15000, 30000],
    detected_only: bool = False,
) -> pl.DataFrame:
    return (
        categorize_cumulative_cases(
            prevalence_report,
            max_date=max_date,
            bin_breaks=bin_breaks,
            detected_only=detected_only,
        )
        .filter(pl.col("date") == max_date)
        .group_by("count_category")
        .agg(pl.max("date"), pl.len().alias("trajectories"))
        .with_columns(
            pl.lit(scenario_name).alias("scenario"),
            pl.sum("trajectories").alias("total_trajectories"),
        )
        .with_columns(
            (pl.col("trajectories") / pl.col("total_trajectories")).alias("proportion")
        )
    )


def make_name(scenario: dict) -> str:
    if "etu_transmission_probability" in scenario:
        return f"detection_{scenario['passive_detection_probability']:.2f}_isolation_{1.0 - scenario['etu_transmission_probability']:.2f}"
    else:
        return f"detection_{scenario['passive_detection_probability']:.2f}"


def _plot_cumulative_stacked_bars(
    output_dir: Path,
    scenarios: pl.DataFrame,
    strategy: Literal["outbreak_size", "detected"],
    max_date: dt.date,
) -> None:
    if strategy == "outbreak_size":
        title_str = f"Outbreak size by {max_date}"
    elif strategy == "detected":
        title_str = f"Cases detected by {max_date}"

    summaries = []
    figures_path = output_dir / "products" / "figures"
    os.makedirs(figures_path, exist_ok=True)

    for parameters in scenarios.iter_rows(named=True):
        scenario_name = make_name(parameters)

        prevalence_report_file_path = (
            output_dir / scenario_name / "projection" / "all_prevalence_reports.csv"
        )
        prevalence_report = pl.read_csv(prevalence_report_file_path)

        detected_only = strategy == "detected"
        scenario_sizes = group_sizes(
            prevalence_report,
            max_date=max_date,
            scenario_name=scenario_name,
            detected_only=detected_only,
        )

        summaries.append(scenario_sizes)

    all_scenario_sizes = pl.concat(summaries).with_columns(
        pl.when(pl.col("count_category") == "<15k")
        .then(0)
        .otherwise(
            pl.when(pl.col("count_category") == "15k-30k").then(15).otherwise(30)
        )
        .alias("count_category_lower_bound")
    )

    all_scenario_sizes = all_scenario_sizes.with_columns(
        pl.cum_sum("proportion")
        .over("scenario", order_by="count_category_lower_bound")
        .alias("cumulative_proportion")
    )

    custom_colors = ["#B51612", "#750C75", "#B55BB5"]
    sns.set_palette(sns.color_palette(custom_colors))

    ax = sns.barplot(
        data=all_scenario_sizes,
        x="scenario",
        y="cumulative_proportion",
        hue="count_category",
        dodge=False,
        hue_order=[">30k", "15k-30k", "<15k"],
    )

    plt.ylabel("Proportion of simulations by category")
    plt.xlabel("Ascertainment x Isolation scenario")
    plt.title(title_str)
    plt.legend(title="Final count category", loc="upper right")
    sns.move_legend(ax, "upper left", bbox_to_anchor=(1, 1))
    plt.savefig(figures_path / f"cumulative_stacked_{strategy}.png")
    plt.clf()


def _collect_current_and_future_outbreak_size(
    prevalence_report: pl.DataFrame,
    current_date: dt.date,
    projection_date: dt.date,
    scenario_name: str,
    bin_breaks=[10000, 20000],
) -> pl.DataFrame:
    returned_dfs = []
    for eval_date in [current_date, projection_date]:
        cases = group_sizes(
            prevalence_report,
            max_date=eval_date,
            scenario_name=scenario_name,
            bin_breaks=bin_breaks,
        )
        confirmations = group_sizes(
            prevalence_report,
            max_date=eval_date,
            scenario_name=scenario_name,
            bin_breaks=bin_breaks,
            detected_only=True,
        )
        combined = cases.join(
            confirmations,
            on=["scenario", "count_category", "date"],
            how="full",
            suffix="_confirmed",
            coalesce=True,
        ).with_columns(cs.numeric().fill_null(0))
        returned_dfs.append(combined)
    return pl.concat(returned_dfs)


def _collect_rt_estimates(
    rt_report: pl.DataFrame,
    date_range: pl.Expr,
) -> pl.DataFrame:
    return (
        rt_report.with_columns(pl.col("infection_date").cast(pl.Date).alias("date"))
        .filter(pl.col("date").is_in(date_range))
        .select(["particle_id", "scenario", "avg_onward_infections"])
        .group_by("scenario")
        .agg(
            pl.mean("avg_onward_infections").alias("mean_rt"),
            pl.median("avg_onward_infections").alias("median_rt"),
            pl.std("avg_onward_infections").alias("std_rt"),
            pl.quantile("avg_onward_infections", 0.025).alias("lower_95ci_rt"),
            pl.quantile("avg_onward_infections", 0.975).alias("upper_95ci_rt"),
            pl.quantile("avg_onward_infections", 0.25).alias("lower_iqi_rt"),
            pl.quantile("avg_onward_infections", 0.75).alias("upper_iqi_rt"),
        )
    )


def _collect_known_case_estimates(
    confirmations_df: pl.DataFrame,
    date_range: pl.Expr,
) -> pl.DataFrame:
    return (
        confirmations_df.with_columns(pl.col("date").cast(pl.Date))
        .filter(
            (pl.col("case_status") == "Confirmed") & (pl.col("date").is_in(date_range))
        )
        .with_columns(
            pl.when(pl.col("count") > 0)
            .then(pl.col("with_confirmed_index") / pl.col("count"))
            .otherwise(None)
            .alias("proportion_with_confirmed_index")
        )
        .group_by("scenario")
        .agg(
            pl.mean("count").alias("mean_confirmation"),
            (pl.sum("with_confirmed_index") / pl.sum("count")).alias(
                "mean_proportion_with_confirmed_index"
            ),
            pl.quantile("proportion_with_confirmed_index", 0.025).alias(
                "lower_95ci_proportion_with_confirmed_index"
            ),
            pl.quantile("proportion_with_confirmed_index", 0.975).alias(
                "upper_95ci_proportion_with_confirmed_index"
            ),
            pl.quantile("proportion_with_confirmed_index", 0.25).alias(
                "lower_iqi_proportion_with_confirmed_index"
            ),
            pl.quantile("proportion_with_confirmed_index", 0.75).alias(
                "upper_iqi_proportion_with_confirmed_index"
            ),
        )
    )


def _collect_current_size(
    prevalence_report: pl.DataFrame,
    current_date: dt.date,
) -> pl.DataFrame:
    return get_cumulative_symptomatic_cases(
        prevalence_report, max_date=current_date
    ).filter(pl.col("date") == current_date)


def _collect_outbreak_size_for_boxplot(
    prevalence_report: pl.DataFrame,
    current_date: dt.date,
    projection_date: dt.date,
    scenario_name: str,
) -> pl.DataFrame:
    """Extract outbreak sizes for both current and projection dates."""
    returned_dfs = []
    for eval_date, date_label in [
        (current_date, "Report Date"),
        (projection_date, "Projection Date"),
    ]:
        sizes = get_cumulative_symptomatic_cases(
            prevalence_report, max_date=eval_date
        ).filter(pl.col("date") == eval_date)
        sizes = sizes.with_columns(
            pl.lit(scenario_name).alias("scenario"),
            pl.lit(date_label).alias("date_type"),
            pl.lit(eval_date).alias("eval_date"),
        )
        returned_dfs.append(sizes)
    return pl.concat(returned_dfs, how="vertical_relaxed")


def _plot_current_rt_estimates_boxplots(
    output_dir: Path,
    rt_estimates: pl.DataFrame,
    current_date: dt.date,
) -> None:
    """Create boxplots comparing Rt estimates across scenarios."""
    figures_path = output_dir / "products" / "figures"
    os.makedirs(figures_path, exist_ok=True)

    plt.figure(figsize=(12, 6))
    sns.boxplot(
        data=rt_estimates.filter(
            pl.col("infection_date").cast(pl.Date) == current_date
        ),
        x="scenario",
        y="avg_onward_infections",
    )

    plt.ylabel("Estimated Rt")
    plt.xlabel("Ascertainment scenario")
    plt.title(f"Estimated Rt by Scenario (Current Date {current_date})")
    plt.xticks(rotation=45, ha="right")
    plt.tight_layout()
    plt.savefig(figures_path / "rt_estimates_boxplots.png", dpi=300)
    plt.axhline(y=1.0, color="black", linestyle="--", label="Rt=1")
    plt.clf()


def _plot_outbreak_size_boxplots(
    output_dir: Path,
    scenarios: pl.DataFrame,
    current_date: dt.date,
    projection_date: dt.date,
) -> None:
    """Create boxplots comparing outbreak sizes at report date vs projection date."""
    figures_path = output_dir / "products" / "figures"
    os.makedirs(figures_path, exist_ok=True)

    size_dfs = []
    for parameters in scenarios.iter_rows(named=True):
        scenario_name = make_name(parameters)
        projection_dir = output_dir / scenario_name / "projection"

        prevalence_report = pl.read_csv(projection_dir / "all_prevalence_reports.csv")

        sizes = _collect_outbreak_size_for_boxplot(
            prevalence_report,
            current_date=current_date,
            projection_date=projection_date,
            scenario_name=scenario_name,
        )
        size_dfs.append(sizes)

    all_sizes = pl.concat(size_dfs, how="vertical_relaxed")

    # Create boxplot
    plt.figure(figsize=(12, 6))
    sns.boxplot(
        data=all_sizes.filter(pl.col("date_type") == "Report Date"),
        x="scenario",
        y="count",
    )

    plt.ylabel("Outbreak Size (total symptomatic cases)")
    plt.xlabel("Ascertainment scenario")
    plt.title(f"Outbreak Size by Scenario (Report Date {current_date})")
    # plt.legend(title="Date Type", loc="upper left")
    plt.xticks(rotation=45, ha="right")
    plt.tight_layout()
    plt.axhline(y=10000, color="orange", linestyle="--", label="10k cases")
    plt.axhline(y=20000, color="red", linestyle="--", label="20k cases")
    plt.savefig(figures_path / "outbreak_size_boxplots.png", dpi=300)
    plt.clf()

    # Create projection boxplot
    plt.figure(figsize=(12, 6))
    sns.boxplot(
        data=all_sizes.filter(pl.col("date_type") == "Projection Date"),
        x="scenario",
        y="count",
    )
    plt.ylabel("Outbreak Size (total symptomatic cases)")
    plt.xlabel("Ascertainment scenario")
    plt.title(f"Outbreak Size by Scenario (Projection Date {projection_date})")
    # plt.legend(title="Date Type", loc="upper left")
    plt.xticks(rotation=45, ha="right")
    plt.tight_layout()
    plt.axhline(y=10000, color="orange", linestyle="--", label="10k cases")
    plt.axhline(y=20000, color="red", linestyle="--", label="20k cases")
    plt.savefig(figures_path / "outbreak_size_boxplots_projection.png", dpi=300)
    plt.clf()


def main(
    output_subdir: str,
    scenarios: pl.DataFrame,
    config_file: str,
    current_date: dt.date,
    max_date: dt.date,
) -> None:
    intervention_date = load_intervention_date(config_file)
    indicator_date_range = pl.date_range(
        start=intervention_date + dt.timedelta(days=14),
        end=intervention_date + dt.timedelta(days=28),
        interval="1d",
    )

    output_dir = paths.output_dir(output_subdir)
    study_products_dir = output_dir / "products"
    os.makedirs(study_products_dir, exist_ok=True)

    grouped_prevalence_reports = []
    grouped_rt_reports = []
    grouped_confirmation_reports = []
    current_size_dfs = []
    for parameters in scenarios.iter_rows(named=True):
        scenario_name = make_name(parameters)
        projection_dir = output_dir / scenario_name / "projection"

        rt_report = pl.read_csv(projection_dir / "all_rt_reports.csv")
        grouped_rt_reports.append(
            rt_report.with_columns(pl.lit(scenario_name).alias("scenario"))
        )
        confirmation_report = pl.read_csv(
            projection_dir / "all_confirmation_incidence_reports.csv"
        )
        grouped_confirmation_reports.append(
            confirmation_report.with_columns(pl.lit(scenario_name).alias("scenario"))
        )

        prevalence_report_file_path = projection_dir / "all_prevalence_reports.csv"
        prevalence_report = pl.read_csv(prevalence_report_file_path)

        all_size_report = _collect_current_size(
            prevalence_report=prevalence_report, current_date=current_date
        )
        current_size_dfs.append(
            all_size_report.with_columns(pl.lit(scenario_name).alias("scenario"))
        )

        grouped_prevalence_report = prevalence_report.pipe(
            _collect_current_and_future_outbreak_size,
            current_date=current_date,
            projection_date=max_date,
            scenario_name=scenario_name,
            bin_breaks=[10000, 20000],
        )

        grouped_prevalence_reports.append(grouped_prevalence_report)

    prevalence_df = pl.concat(grouped_prevalence_reports, how="vertical_relaxed")
    current_size_df = pl.concat(current_size_dfs, how="vertical_relaxed")
    current_size_df.write_csv(study_products_dir / "current_outbreak_sizes.csv")
    prevalence_df.sort("scenario", "date", "count_category").write_csv(
        study_products_dir / "cumulative_outbreak_sizes.csv"
    )

    rt_df = pl.concat(grouped_rt_reports, how="vertical_relaxed")
    confirmations_df = pl.concat(grouped_confirmation_reports, how="vertical_relaxed")

    for strategy in ["outbreak_size", "detected"]:
        _plot_cumulative_stacked_bars(
            output_dir=output_dir,
            scenarios=scenarios,
            strategy=strategy,
            max_date=max_date,
        )

    _plot_outbreak_size_boxplots(
        output_dir=output_dir,
        scenarios=scenarios,
        current_date=current_date,
        projection_date=max_date,
    )

    _plot_current_rt_estimates_boxplots(
        output_dir=output_dir,
        rt_estimates=rt_df,
        current_date=current_date,
    )

    confirmations_df.pipe(
        _collect_known_case_estimates,
        date_range=indicator_date_range,
    ).write_csv(study_products_dir / "known_case_estimates.csv")

    rt_df.pipe(
        _collect_rt_estimates,
        date_range=indicator_date_range,
    ).write_csv(study_products_dir / "rt_estimates.csv")
