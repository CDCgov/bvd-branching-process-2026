import datetime as dt
import os
import pickle
from collections.abc import Mapping
from pathlib import Path

import matplotlib.cm as mcm
import matplotlib
import matplotlib.colors as mcolors
import matplotlib.pyplot as plt
import matplotlib.transforms as mtransforms
import numpy as np
import polars as pl
import seaborn as sns
from calibrationtools import CalibrationResults, Particle

from vhf_pipeline import paths
from vhf_pipeline.pipeline.shared_plotting_data import (
    get_cumulative_cases,
    load_scenario_data,
    save_particle_detection_rate,
)
from vhf_pipeline.utils import assign_epiweek


def _plot_posterior_diversity(
    calibration_results: CalibrationResults, figures_dir: Path
) -> None:
    """Plot distribution of posterior particles per seed.

    Args:
        calibration_results: Calibration results object
        figures_dir: Directory for saving figures
    """
    d = {}
    for p in calibration_results.posterior_particles.particles:
        seed = p["seed"]
        d[seed] = d.get(seed, 0) + 1

    barplot_vals = list(d.values())
    sns.histplot(barplot_vals, bins=range(1, max(barplot_vals) + 2), color="blue")
    plt.title("Count of posterior particles per seed")
    plt.xlabel("Number of posterior particles per seed")
    plt.ylabel("Count of seeds")
    plt.tight_layout()
    plt.savefig(figures_dir / "posterior_particle_distribution.png")
    plt.clf()


def _plot_cumulative_cases(
    target_data: pl.DataFrame,
    onset_data: pl.DataFrame,
    prevalence_report_df: pl.DataFrame,
    projected_report_data: pl.DataFrame,
    figures_dir: Path,
    projection_date: dt.date,
) -> None:
    """Plot cumulative true case count vs confirmed case data.

    Args:
        target_data: Target data with observed values
        onset_data: Symptom onset data
        prevalence_report_df: Prevalence report data
        projected_report_data: Simulation output data
        figures_dir: Directory for saving figures
    """
    max_report_date = target_data.select(pl.max("report_date")).item()
    min_date = dt.date(2026, 1, 1)
    particle_ids = projected_report_data["particle_id"].unique().to_list()
    plt.figure(figsize=(6.4, 4.8))

    sns.lineplot(
        data=onset_data.with_columns(
            pl.col("count")
            .cum_sum()
            .over("particle_id", order_by="date")
            .alias("cumulative_count")
        ).filter(
            (pl.col("date") <= max_report_date)
            & (pl.col("date") >= min_date)
            & pl.col("particle_id").is_in(particle_ids)
        ),
        x="date",
        y="cumulative_count",
        estimator=None,
        units="particle_id",
        alpha=0.1,
    )
    cumulative_case_df = get_cumulative_cases(
        prevalence_report_df, max_report_date, projection_date=projection_date
    )

    sns.lineplot(
        data=cumulative_case_df,
        x="date",
        y="cumulative_cases",
        estimator=None,
        units="particle_id",
        color="#666666",
        alpha=0.1,
    )

    sns.scatterplot(
        data=target_data.filter(pl.col("date") <= max_report_date),
        x="date",
        y="cumulative_confirmation_incidence",
        color="red",
        zorder=10,
        label="Target Data",
    )

    plt.title("True case count vs confirmed case data")
    plt.xlabel("Date of symptom onset")
    plt.ylabel("Cumulative true case count")
    plt.ylim(0, 30000)
    plt.xlim(left=min_date)
    plt.xticks(rotation=45)
    plt.legend()
    plt.tight_layout()
    plt.savefig(figures_dir / "cumulative_cases.png")
    plt.clf()


def _plot_cumulative_confirmed(
    target_data: pl.DataFrame,
    projected_report_data: pl.DataFrame,
    figures_dir: Path,
) -> None:
    """Plot cumulative confirmed cases vs target data.

    Args:
        target_data: Target data with observed values
        projected_report_data: Simulation output data
        figures_dir: Directory for saving figures
    """
    max_report_date = target_data.select(pl.max("report_date")).item()
    min_date = target_data.select(pl.min("date")).item()
    plt.figure(figsize=(6.4, 4.8))
    sns.lineplot(
        data=projected_report_data.with_columns(
            pl.col("confirmation_incidence")
            .cum_sum()
            .over("particle_id", order_by="date")
            .alias("cumulative_confirmed")
        ).filter((pl.col("date") <= max_report_date) & (pl.col("date") >= min_date)),
        x="date",
        y="cumulative_confirmed",
        estimator=None,
        units="particle_id",
        alpha=0.1,
    )

    sns.scatterplot(
        data=target_data.filter(pl.col("date") <= max_report_date),
        x="date",
        y="cumulative_confirmation_incidence",
        color="red",
        zorder=10,
        label="Target Data",
    )

    plt.title("Confirmed Cases (cumulative from 2026-06-01) vs Target Data")
    plt.xlabel("Date of case confirmation")
    plt.ylabel("Confirmed Cases (cumulative from 2026-06-01)")
    plt.xticks(rotation=45)
    plt.legend()
    plt.tight_layout()
    plt.savefig(figures_dir / "cumulative_confirmed.png", dpi=300)
    plt.clf()


def _plot_cumulative_deaths(
    prevalence_report_df: pl.DataFrame,
    figures_dir: Path,
) -> None:
    """Plot true cumulative deaths over time as a spaghetti plot.

    This intentionally uses only ``alive == False`` from prevalence reports and
    does not filter by detection status.
    """
    deaths = (
        prevalence_report_df.with_columns(pl.col("date").cast(pl.Date))
        .filter(~pl.col("alive"))
        .group_by("date", "particle_id")
        .agg(pl.sum("count").alias("cumulative_deaths"))
    )

    if deaths.height == 0:
        print("Skipping cumulative deaths plot because no deaths were recorded.")
        return

    plt.figure(figsize=(6.4, 4.8))
    sns.lineplot(
        data=deaths,
        x="date",
        y="cumulative_deaths",
        estimator=None,
        units="particle_id",
        alpha=0.1,
    )
    plt.title("True Cumulative Deaths Over Time")
    plt.xlabel("Date")
    plt.ylabel("Cumulative deaths")
    plt.xticks(rotation=45)
    plt.tight_layout()
    plt.savefig(figures_dir / "cumulative_deaths_spaghetti.png", dpi=300)
    plt.clf()


def _plot_incidence_comparison(
    target_data: pl.DataFrame,
    projected_report_data: pl.DataFrame,
    figures_dir: Path,
) -> None:
    """Plot projected vs target symptom onset incidence.

    Args:
        target_data: Target data with observed values
        projected_report_data: Simulation output data
        figures_dir: Directory for saving figures
    """
    x_labs = (
        target_data.filter(pl.col("date") <= pl.col("report_date"))
        .sort("date")["date"]
        .to_list()
    )

    sns.barplot(
        data=projected_report_data,
        x="date",
        y="symptom_onset_incidence",
        errorbar=("pi", 50),
        color="orange",
        native_scale=True,
        order=x_labs,
    )

    sns.pointplot(
        data=target_data.filter(pl.col("date") <= pl.col("report_date")),
        x="date",
        y="symptom_onset_incidence",
        color="red",
        zorder=10,
        label="Target Data",
        order=x_labs,
    )

    plt.title("Projected Symptom Onset Report vs Target Data")
    plt.xlabel("Epiweek start date")
    plt.ylabel("Incident Symptom Onset")
    plt.xticks(rotation=45)
    plt.legend()
    plt.tight_layout()
    plt.savefig(figures_dir / "incidence_comparison.png", dpi=300)
    plt.clf()


def _plot_weekly_confirmation_indicator(
    confirmation_df: pl.DataFrame,
    figures_dir: str,
    date_range: tuple[dt.date, dt.date] = (dt.date(2026, 4, 1), dt.date(2026, 9, 1)),
) -> None:
    weekly_confirmed_cases = (
        confirmation_df.pipe(assign_epiweek, date_column_name="date")
        .filter(pl.col("case_status") == "Confirmed")
        .group_by("epiweek_startdate", "particle_id")
        .agg(
            pl.sum("count"),
            pl.sum("with_confirmed_index"),
        )
        .with_columns(
            (pl.col("with_confirmed_index") / pl.col("count")).alias(
                "proportion_from_confirmed_index"
            )
        )
    )
    plt.figure(figsize=(6.4, 4.8))

    sns.lineplot(
        data=weekly_confirmed_cases.filter(
            (pl.col("epiweek_startdate") >= date_range[0])
            & (pl.col("epiweek_startdate") <= date_range[1])
        ),
        x="epiweek_startdate",
        y="proportion_from_confirmed_index",
        units="particle_id",
        estimator=None,
        alpha=0.1,
    )
    plt.xticks(rotation=45)
    # set vertical line at 2026-05-24
    plt.axvline(x=dt.date(2026, 5, 24), color="black", linestyle="--")
    plt.title("Case confirmation by epiweek")
    plt.xlabel("Epiweek confirmed")
    plt.ylabel("Proportion of confirmed cases with confirmed infector")
    plt.tight_layout()
    plt.savefig(figures_dir / "proportion_from_confirmed_cases.png", dpi=300)
    plt.clf()


def _plot_weekly_rt(
    rt_reports: pl.DataFrame,
    figures_dir: Path,
    date_range: tuple[dt.date, dt.date] = (dt.date(2026, 4, 1), dt.date(2026, 8, 1)),
) -> None:
    weekly_rt = (
        rt_reports.pipe(assign_epiweek, date_column_name="infection_date")
        .group_by("epiweek_startdate", "particle_id")
        .agg(pl.sum("infection_count"), pl.sum("total_offspring"))
        .with_columns(
            (pl.col("total_offspring") / pl.col("infection_count")).alias("weekly_rt")
        )
    )
    plt.figure(figsize=(6.4, 4.8))

    sns.lineplot(
        data=weekly_rt.filter(
            (pl.col("epiweek_startdate") >= date_range[0])
            & (pl.col("epiweek_startdate") <= date_range[1])
        ),
        x="epiweek_startdate",
        y="weekly_rt",
        units="particle_id",
        estimator=None,
        alpha=0.1,
    )
    plt.xticks(rotation=45)
    # set vertical line at 2026-05-24
    plt.axvline(x=dt.date(2026, 5, 24), color="black", linestyle="--")
    plt.xlabel("Epiweek infection")
    plt.ylabel("Weekly Rt")
    plt.ylim(0, 2.5)
    plt.axhline(y=1, color="red")
    plt.tight_layout()
    plt.savefig(figures_dir / "weekly_rt.png", dpi=300)
    plt.clf()


def _plot_final_cases_against_detection_probability(
    prevalence_report_df: pl.DataFrame,
    inputs_df: pl.DataFrame,
    target_data: pl.DataFrame,
    figures_dir: Path,
    projection_date: dt.date,
) -> None:
    """
    Plot final cumulative cases against passive detection probability.

    """

    max_report_date = target_data.select(pl.max("report_date")).item()
    passive_detection_prob_key = inputs_df.select(
        "passive_detection_probability", "particle_id"
    )

    cumulative = get_cumulative_cases(
        prevalence_report_df=prevalence_report_df,
        max_report_date=max_report_date,
        projection_date=projection_date,
    )
    cumulative = (
        cumulative.group_by("particle_id")
        .agg(
            pl.max("cumulative_cases").alias("final_cumulative_cases"),
            pl.max("date").alias("final_date"),
        )
        .join(
            passive_detection_prob_key,
            on="particle_id",
        )
    )

    end_date = cumulative.select(pl.max("final_date")).item()

    sns.scatterplot(
        data=cumulative,
        x="passive_detection_probability",
        y="final_cumulative_cases",
        alpha=0.5,
    )

    plt.title(f"Cumulative Cases on {end_date}")
    plt.xlabel("Passive Detection Probability")
    plt.ylabel("Cumulative Cases")
    plt.tight_layout()
    plt.savefig(figures_dir / "final_cases_vs_detection_probability.png", dpi=300)
    plt.clf()


def _plot_posterior_vs_prior(
    calibration_results: CalibrationResults,
    true_values: Mapping[str, float] | None = None,
) -> tuple[plt.Figure, pl.DataFrame]:
    """Create stepped posterior vs prior distributions plot.

    Args:
        calibration_results: Calibration results object
        true_values: Dictionary of true parameter values for reference lines

    Returns:
        Tuple of (figure, None for compatibility)
    """
    particle_history = calibration_results.population_archive
    steps = max(calibration_results.distance_history.keys())
    distributions = [particle_history[key] for key in range(steps)] + [
        calibration_results.posterior_particles
    ]

    # Create a grid of subplots: rows for parameters, columns for steps
    num_params = len(calibration_results.fitted_params)
    num_steps = len(distributions)
    fig, axes = plt.subplots(
        num_steps,
        num_params,
        figsize=(6 * num_params, 5 * num_steps),
        constrained_layout=True,
    )

    for col_idx, param in enumerate(calibration_results.fitted_params):
        for row_idx, step in enumerate(distributions):
            # Extract parameter values for the current step
            vals = [p[param] for p in step.particles]
            min_val = max([min(vals) - np.var(vals), 0])
            max_val = max(vals) + np.var(vals)

            if num_params > 1 and num_steps > 1:
                ax = axes[row_idx, col_idx]
            elif num_params > 1:
                ax = axes[col_idx]
            else:
                ax = axes[row_idx]

            # Plot posterior distribution
            sns.histplot(
                x=vals,
                stat="density",
                kde=True,
                color="orange",
                edgecolor="black",
                ax=ax,
                weights=step.weights,
                bins=30,
            )

            # Evaluate prior density
            eval_points = np.arange(min_val, max_val, 0.01)
            param_prior = None
            for prior in calibration_results.priors.priors:
                if prior.param == param:
                    param_prior = prior
                    break
            if not param_prior:
                raise ValueError(f"Could not find prior {param}")

            density_vals = [
                param_prior.probability_density(Particle({param: v}))
                for v in eval_points
            ]

            # Plot prior density
            sns.lineplot(
                x=eval_points,
                y=density_vals,
                ax=ax,
                color="blue",
                label="Prior",
            )

            # Add true value line if provided
            if true_values and param in true_values:
                ax.axvline(
                    x=true_values[param],
                    color="red",
                    linestyle="--",
                    linewidth=1.5,
                    label="True value" if row_idx == 0 else None,
                )

            # Set titles and labels
            if row_idx == 0:
                ax.set_title(" ".join(param.split(".")))
            if col_idx == 0:
                ax.set_ylabel(f"Step {row_idx + 1}")

    # Add a legend to the figure
    handles, labels = (
        axes[0, 0].get_legend_handles_labels()
        if num_params > 1 and num_steps > 1
        else axes[0].get_legend_handles_labels()
    )
    fig.legend(handles, labels, loc="upper right", bbox_to_anchor=(1.1, 1.0))

    return fig, axes


def _save_parameter_figures(
    fig: plt.Figure,
    axes,
    calibration_results: CalibrationResults,
    figures_dir: Path,
) -> None:
    """Save parameter figures as both combined and per-parameter images.

    Args:
        fig: The figure object
        axes: The axes array
        calibration_results: Calibration results object
        figures_dir: Directory for saving figures
    """
    num_params = len(calibration_results.fitted_params)
    num_steps = len(calibration_results.tolerance_values)

    # Save the overall figure
    fig.savefig(figures_dir / "stepped_posterior_vs_prior.png")

    # Save each parameter as its own figure
    for col in range(num_params):
        ax_col = axes[:, col] if num_params > 1 and num_steps > 1 else axes
        param_name = calibration_results.fitted_params[col]

        fig.canvas.draw()
        bbox = mtransforms.Bbox.union(
            [ax.get_tightbbox(fig.canvas.renderer) for ax in ax_col]
        )
        bbox_inches = bbox.transformed(fig.dpi_scale_trans.inverted())

        fig.savefig(
            figures_dir / "parameters" / f"{param_name}_posterior.png",
            bbox_inches=bbox_inches,
        )

    plt.clf()


def _save_error_history_plot(
    calibration_results: CalibrationResults,
    figures_dir: Path,
) -> None:
    """Save a histogram plot of the distance history over calibration steps.

    Args:
        calibration_results: Calibration results object
        figures_dir: Directory for saving figures
    """
    plt.figure(figsize=(6.4, 4.8))
    distance_history = calibration_results.flatten_distance_history()

    # Make data frame grouped by step with distance values
    distance_df = pl.DataFrame(
        {
            "step": [
                step for step, distances in distance_history.items() for _ in distances
            ],
            "distance": [
                distance
                for step, distances in distance_history.items()
                for distance in distances
            ],
        }
    )

    plt.figure(figsize=(6.4, 4.8))
    G = sns.catplot(
        data=distance_df,
        x="distance",
        hue="step",
        row="step",
        kind="swarm",
        sharex=False,
        sharey=False,
    )
    for i, ax in enumerate(G.axes.flatten()):
        tolerance = calibration_results.tolerance_values[i]
        ax.set_title(f"Step {i}")
        ax.set_xlabel("Distance")
        ax.axvline(
            x=tolerance, linestyle="--", color="black", label=f"Tolerance {tolerance}"
        )

    plt.tight_layout()
    plt.savefig(figures_dir / "distance_history.png")
    plt.clf()


def _plot_simulations_by_distance(
    target_data: pl.DataFrame,
    projected_report_data: pl.DataFrame,
    figures_dir: Path,
) -> None:
    """Plot cumulative confirmed cases per particle, colored by distance to target.

    Uses ``cumulative_confirmation_incidence`` from the simulation data directly,
    matching the alignment used by DetectionRangeProcessor (epiweek start dates joined
    against the same column in the target).  Lines are colored by ``total_error``
    so that better-fitting particles stand out visually.

    Args:
        target_data: Target data with ``date`` (epiweek start), ``report_date``,
            and ``cumulative_confirmation_incidence`` columns.
        projected_report_data: Per-particle simulation data with ``date``
            (epiweek start), ``cumulative_confirmation_incidence``,
            ``particle_id``, and ``total_error`` columns – i.e. the
            all_simulations.csv format.
        figures_dir: Directory for saving the figure.
    """
    max_report_date = target_data.select(pl.max("report_date")).item()
    min_date = target_data.select(pl.min("date")).item()

    # Use the pre-computed cumulative column – same quantity joined by
    # detection_band processor when computing distance.
    plot_df = projected_report_data.filter(
        (pl.col("date") <= max_report_date) & (pl.col("date") >= min_date)
    ).with_columns(pl.col("date").cast(pl.Date))

    # One total_error value per particle (constant across rows for that particle).
    particle_errors = (
        plot_df.select(["particle_id", "total_error"]).unique().sort("total_error")
    )

    error_min = particle_errors["total_error"].min()
    error_max = particle_errors["total_error"].max()
    norm = mcolors.Normalize(vmin=error_min, vmax=error_max)
    # Low error (good fit) → green; high error (poor fit) → red.
    cmap = matplotlib.colormaps["RdYlGn_r"]

    fig, ax = plt.subplots(figsize=(8, 5))

    for row in particle_errors.iter_rows(named=True):
        pid = row["particle_id"]
        err = row["total_error"]
        pdata = plot_df.filter(pl.col("particle_id") == pid).sort("date")
        ax.plot(
            pdata["date"].to_list(),
            pdata["cumulative_confirmation_incidence"].to_list(),
            color=cmap(norm(err)),
            alpha=0.5,
            linewidth=0.8,
        )

    # Target data: plot only the epiweeks that fall within the report window.
    target_filtered = target_data.filter(pl.col("date") <= max_report_date)
    ax.scatter(
        target_filtered["date"].to_list(),
        target_filtered["cumulative_confirmation_incidence"].to_list(),
        color="black",
        zorder=10,
        s=60,
        label="Target data",
    )

    sm = mcm.ScalarMappable(cmap=cmap, norm=norm)
    sm.set_array([])
    fig.colorbar(sm, ax=ax, label="Distance to target (total_error)")

    ax.set_title("Cumulative Confirmed Cases by Particle Distance")
    ax.set_xlabel("Epiweek start date")
    ax.set_ylabel("Cumulative confirmed cases")
    ax.legend()
    plt.xticks(rotation=45)
    plt.tight_layout()
    plt.savefig(figures_dir / "simulations_by_distance.png", dpi=300)
    plt.clf()


def main(
    output_subdir,
    projection_date: dt.date,
    calibration_subdir: str = "symptom_onset",
    true_values: Mapping[str, float] | None = None,
    plot_incidence: bool = True,
    save_detection_rate: bool = True,
) -> None:
    """Generate figures for symptom onset calibration analysis.

    Args:
        output_subdir: Subdirectory for output (relative to output dir)
        calibration_subdir: Subdirectory for calibration results
        true_values: Dictionary of true parameter values for reference lines
    """
    # Set up the environment and directories
    output_dir = paths.output_dir(output_subdir)
    products_dir = output_dir / calibration_subdir / "products"
    products_dir.mkdir(parents=True, exist_ok=True)
    figures_dir = products_dir / "figures"
    figures_dir.mkdir(parents=True, exist_ok=True)

    # Load data
    data = load_scenario_data(output_dir, calibration_subdir, products_dir)
    target_data = data["target_data"]
    onset_data = data["onset_data"]
    projection_dir = data["projection_dir"]

    # Load calibration results
    calibration_results_file = (
        output_dir / calibration_subdir / "calibration" / "calibration_results.pkl"
    )
    with open(calibration_results_file, "rb") as fp:
        calibration_results: CalibrationResults = pickle.load(fp)

    # Load simulations
    prevalence_report_df = pl.read_csv(projection_dir / "all_prevalence_reports.csv")
    _all_sims_df = pl.read_csv(projection_dir / "all_simulations.csv")
    if "epiweek_startdate" in _all_sims_df.columns:
        projected_report_data = _all_sims_df.with_columns(
            pl.col("epiweek_startdate").cast(pl.Date)
        ).rename({"epiweek_startdate": "date"})
    else:
        projected_report_data = _all_sims_df.with_columns(pl.col("date").cast(pl.Date))

    # Generate figures
    print("Generating figures...")
    os.makedirs(figures_dir / "parameters", exist_ok=True)
    if save_detection_rate:
        save_particle_detection_rate(
            target_data,
            projected_report_data,
            products_dir,
        )

    # Posterior trajectories fit analysis
    _plot_posterior_diversity(calibration_results, figures_dir)
    if onset_data.height > 0:
        _plot_cumulative_cases(
            target_data,
            onset_data,
            prevalence_report_df,
            projected_report_data,
            figures_dir,
            projection_date,
        )
    else:
        print("Skipping cumulative case plot because onset data is empty.")
    _plot_cumulative_confirmed(target_data, projected_report_data, figures_dir)
    _plot_simulations_by_distance(target_data, projected_report_data, figures_dir)
    _plot_cumulative_deaths(prevalence_report_df, figures_dir)
    if plot_incidence:
        _plot_incidence_comparison(target_data, projected_report_data, figures_dir)
    _plot_final_cases_against_detection_probability(
        prevalence_report_df,
        data["inputs_df"],
        target_data,
        figures_dir,
        projection_date,
    )

    # Indicators
    rt_reports_file = (
        output_dir / calibration_subdir / "projection" / "all_rt_reports.csv"
    )
    rt_reports = pl.read_csv(rt_reports_file)
    _plot_weekly_rt(rt_reports, figures_dir)

    confirmed_cases_file = (
        output_dir
        / calibration_subdir
        / "projection"
        / "all_confirmation_incidence_reports.csv"
    )
    confirmed_cases = pl.read_csv(confirmed_cases_file)
    _plot_weekly_confirmation_indicator(confirmed_cases, figures_dir)

    # Distance history
    _save_error_history_plot(calibration_results, figures_dir)

    # Posterior vs prior distributions
    print(f"ESS posterior samples: {calibration_results.posterior_particles.ess:.2f}")
    fig, axes = _plot_posterior_vs_prior(calibration_results, true_values)
    _save_parameter_figures(fig, axes, calibration_results, figures_dir)

    print(f"Figures saved to {figures_dir}")
