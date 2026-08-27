"""Posterior pairs plots, and the diagnostic tables behind them.

Everything needed is read from a finished run: the arms are the
subdirectories holding a calibration result, and each arm's ``config.json``
names the priors it was calibrated against.

Two statistics are reported per arm.

``D`` is the Kolmogorov-Smirnov distance between the prior and the
importance-weighted posterior. The particles are weighted, so the comparison
is against the weighted empirical CDF and the significance floor uses the
Kish effective sample size rather than the nominal particle count. A
parameter clears the floor when the calibration moved it by more than
sampling noise.

``rho`` is the Spearman correlation among the accepted particles. The priors
are independent by construction, so any structure there was imposed by the
fit. That is a question about the cloud as drawn, so it uses the particle
count, not the effective sample size.
"""

from __future__ import annotations

import datetime as dt
import json
import pickle
import warnings
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from scipy import stats

from vhf_pipeline.model.calibration_cache import RESULTS_NAME

CALIBRATION_RESULTS = Path("calibration") / RESULTS_NAME
MANIFEST = "manifest.json"
ARM_CONFIG = "config.json"

# Display names and grid order for the parameters this project calibrates.
# Informed parameters first. Anything absent falls back to a derived label.
DEFAULT_LABELS = {
    "SpilloverEvent.days_since_start": "spillover\nday",
    "offspring_distribution.NegativeBinomial.mean": "offspring\nmean",
    "case_fatality_ratio": "CFR",
    "testing_config.sample_collection_delay.Gamma.rate": "collect\nrate",
    "testing_config.sample_collection_delay.Gamma.shape": "collect\nshape",
    "offspring_distribution.NegativeBinomial.concentration": ("offspring\ndispersion"),
    "generation_interval_distribution.OffsetWeibull.offset": "GI\noffset",
    "generation_interval_distribution.OffsetWeibull.scale": "GI\nscale",
    "mortality_delay_distribution.Fixed.delay": "mortality\ndelay",
    "recovery_delay_distribution.Fixed.delay": "recovery\ndelay",
}


def kolmogorov_critical_value(alpha: float = 0.05) -> float:
    """Critical value of the Kolmogorov distribution; 1.358 at alpha=0.05."""
    return float(np.sqrt(-0.5 * np.log(alpha / 2)))


def weighted_ecdf(x: np.ndarray, w: np.ndarray, grid: np.ndarray) -> np.ndarray:
    """Right-continuous step ECDF of the weighted sample, on ``grid``."""
    order = np.argsort(x)
    xs, cw = x[order], np.cumsum(w[order]) / w.sum()
    idx = np.searchsorted(xs, grid, side="right")
    return np.where(idx == 0, 0.0, cw[np.clip(idx - 1, 0, None)])


def weighted_ks(a: np.ndarray, wa: np.ndarray, b: np.ndarray, wb: np.ndarray) -> float:
    """Two-sample KS distance between two weighted empirical CDFs."""
    grid = np.concatenate([a, b])
    gap = weighted_ecdf(a, wa, grid) - weighted_ecdf(b, wb, grid)
    return float(np.max(np.abs(gap)))


def kish_ess(w: np.ndarray) -> float:
    """Kish effective sample size, ``(sum w)^2 / sum w^2``."""
    return float(w.sum() ** 2 / np.square(w).sum())


def discover_arms(run_dir: Path) -> list[str]:
    """Names of the run's arms, in sorted order."""
    arms = sorted(
        d.name for d in run_dir.iterdir() if (d / CALIBRATION_RESULTS).exists()
    )
    if not arms:
        raise SystemExit(f"no arms with a calibration result under {run_dir}")
    return arms


def _run_timestamp(run_dir: Path) -> dt.datetime | None:
    manifest = run_dir / MANIFEST
    if not manifest.exists():
        return None
    created = json.loads(manifest.read_text()).get("created_at")
    return dt.datetime.fromisoformat(created) if created else None


def resolve_priors_path(run_dir: Path, arm: str, repo_root: Path = Path()) -> Path:
    """Priors file the arm was calibrated against, from its own config.

    The run records a path rather than the prior's contents, so a later edit
    to that file changes these plots without any error. Warn when the file is
    newer than the run itself.
    """
    config = json.loads((run_dir / arm / ARM_CONFIG).read_text())
    path = repo_root / config["priors_file"]
    if not path.exists():
        raise SystemExit(f"{arm} was calibrated against {path}, which no longer exists")
    created = _run_timestamp(run_dir)
    if created is not None:
        modified = dt.datetime.fromtimestamp(path.stat().st_mtime, tz=dt.timezone.utc)
        if modified > created:
            warnings.warn(
                f"{path} was modified {modified:%Y-%m-%d %H:%M}, after the run"
                f" of {created:%Y-%m-%d %H:%M}. The run records the priors by"
                " path only, so the prior plotted here may not be the prior"
                " that was calibrated against.",
                stacklevel=2,
            )
    return path


def load_prior_draws(
    priors_path: Path, n_draws: int, seed: int
) -> tuple[dict[str, np.ndarray], list[str]]:
    """Sample the priors, returning draws per parameter and the grid order."""
    from calibrationtools.load_priors import independent_priors_from_dict

    spec = json.loads(priors_path.read_text())
    priors = independent_priors_from_dict(spec, incl_seed_parameter=False)
    draws = priors.sample(n_draws, np.random.SeedSequence(seed))
    known = [p for p in DEFAULT_LABELS if p in spec["priors"]]
    rest = [p for p in spec["priors"] if p not in DEFAULT_LABELS]
    params = known + rest
    return {p: np.array([d[p] for d in draws]) for p in params}, params


def label_for(param: str) -> str:
    """Short display name for a prior key."""
    if param in DEFAULT_LABELS:
        return DEFAULT_LABELS[param]
    return "\n".join(param.split(".")[-2:])


@dataclass
class ArmDiagnostics:
    """Per-arm posterior summary; ``ks`` and ``rho`` key off prior names."""

    arm: str
    ess: float
    floor: float
    n_particles: int
    weights: np.ndarray
    posterior: dict[str, np.ndarray]
    ks: dict[str, float]
    rho: dict[tuple[str, str], stats._stats_py.SignificanceResult]


def arm_diagnostics(
    run_dir: Path,
    arm: str,
    prior: dict[str, np.ndarray],
    params: list[str],
    ks_alpha: float = 0.05,
) -> ArmDiagnostics:
    """Weighted KS against the prior, and Spearman structure, for one arm."""
    with open(run_dir / arm / CALIBRATION_RESULTS, "rb") as fh:
        population = pickle.load(fh).posterior_particles

    weights = np.asarray(population.weights, dtype=float)
    posterior = {p: np.array([q[p] for q in population.particles]) for p in params}
    ess = kish_ess(weights)
    prior_weights = np.ones(len(next(iter(prior.values()))))
    return ArmDiagnostics(
        arm=arm,
        ess=ess,
        floor=kolmogorov_critical_value(ks_alpha) / np.sqrt(ess),
        n_particles=len(population.particles),
        weights=weights,
        posterior=posterior,
        ks={
            p: weighted_ks(prior[p], prior_weights, posterior[p], weights)
            for p in params
        },
        rho={
            (a, b): stats.spearmanr(posterior[a], posterior[b])
            for i, a in enumerate(params)
            for b in params[i + 1 :]
        },
    )


def pairs_figure(
    diag: ArmDiagnostics,
    prior: dict[str, np.ndarray],
    params: list[str],
    rho_alpha: float = 0.01,
):
    """Lower-triangle pairs grid: prior vs posterior, and accepted particles."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    n = len(params)
    post, w = diag.posterior, diag.weights
    fig, axes = plt.subplots(n, n, figsize=(2.05 * n, 2.05 * n))
    for i in range(n):
        for j in range(n):
            ax = axes[i, j]
            if j > i:
                ax.axis("off")
                continue

            if i == j:
                p = params[i]
                lo, hi = np.percentile(np.concatenate([prior[p], post[p]]), [0.5, 99.5])
                grid = np.linspace(lo, hi, 200)
                ax.fill_between(
                    grid,
                    stats.gaussian_kde(prior[p])(grid),
                    color="0.75",
                    label="prior",
                )
                ax.hist(
                    post[p],
                    bins=30,
                    range=(lo, hi),
                    weights=w,
                    density=True,
                    color="#c1121f",
                    alpha=0.8,
                    label="posterior",
                )
                ax.set_yticks([])
                ks = diag.ks[p]
                ax.set_title(
                    f"KS {ks:.2f}",
                    fontsize=8,
                    pad=2,
                    fontweight="bold" if ks > diag.floor else "normal",
                )
                if i == 0:
                    ax.legend(fontsize=7, frameon=False, loc="upper right")
            else:
                x, y = post[params[j]], post[params[i]]
                result = diag.rho[(params[j], params[i])]
                rho, significant = result.statistic, result.pvalue < rho_alpha
                if significant:
                    ax.set_facecolor(
                        plt.cm.RdBu_r(0.5 + 0.5 * np.clip(rho, -1, 1) * 0.8)
                    )
                    ax.patch.set_alpha(0.35)
                ax.scatter(x, y, s=4, color="#22223b", alpha=0.35, linewidths=0)
                ax.text(
                    0.05,
                    0.93,
                    f"{rho:+.2f}" + ("*" if significant else ""),
                    transform=ax.transAxes,
                    fontsize=8,
                    va="top",
                    fontweight="bold" if significant else "normal",
                )

            ax.tick_params(labelsize=6)
            if i == n - 1:
                ax.set_xlabel(label_for(params[j]), fontsize=8)
            else:
                ax.set_xticklabels([])
            if j == 0:
                ax.set_ylabel(label_for(params[i]), fontsize=8)
            else:
                ax.set_yticklabels([])

    fig.suptitle(
        f"{diag.arm}  \u2014  ESS {diag.ess:.1f} of {diag.n_particles} particles\n"
        "diagonal: prior (grey) vs weight-adjusted posterior (red), "
        f"weighted KS D (bold if > {diag.floor:.2f}).  "
        f"lower: accepted particles, Spearman rho, * = p<{rho_alpha}",
        fontsize=13,
    )
    fig.tight_layout(rect=(0, 0, 1, 0.965))
    return fig


def _flat(param: str) -> str:
    return label_for(param).replace("\n", " ")


def _legend(diagnostics: list[ArmDiagnostics]) -> str:
    """Arm names are too long to head a column, so number them instead."""
    return "\n".join(f"  [{i}] {d.arm}" for i, d in enumerate(diagnostics, start=1))


def _row(name: str, cells: list[str], width: int) -> str:
    return name.ljust(width) + "".join(c.rjust(9) for c in cells)


def format_ks_table(diagnostics: list[ArmDiagnostics], params: list[str]) -> str:
    """Weighted KS per parameter and arm; ``*`` marks values over the floor."""
    width = max(len(_flat(p)) for p in params) + 2
    lines = ["weighted KS D, prior vs posterior", _legend(diagnostics), ""]
    headers = [f"[{i}]" for i in range(1, len(diagnostics) + 1)]
    lines.append(_row("", headers, width))
    lines.append(_row("ESS", [f"{d.ess:.1f}" for d in diagnostics], width))
    lines.append(_row("floor", [f"{d.floor:.2f}" for d in diagnostics], width))
    for p in params:
        cells = [
            f"{d.ks[p]:.3f}" + ("*" if d.ks[p] > d.floor else "") for d in diagnostics
        ]
        lines.append(_row(_flat(p), cells, width))
    return "\n".join(lines)


def format_rho_table(diagnostics: list[ArmDiagnostics], alpha: float = 0.01) -> str:
    """Spearman pairs reaching ``alpha`` in at least one arm."""
    pairs = [
        pair
        for pair in sorted(diagnostics[0].rho)
        if any(d.rho[pair].pvalue < alpha for d in diagnostics)
    ]
    if not pairs:
        return f"No Spearman correlation reaches p<{alpha} in any arm."

    names = {pair: " x ".join(_flat(q) for q in pair) for pair in pairs}
    width = max(len(n) for n in names.values()) + 2
    lines = [
        f"Spearman rho among accepted particles, * = p<{alpha}",
        _legend(diagnostics),
        "",
        _row("", [f"[{i}]" for i in range(1, len(diagnostics) + 1)], width),
    ]
    for pair in pairs:
        cells = [
            f"{d.rho[pair].statistic:+.2f}"
            + ("*" if d.rho[pair].pvalue < alpha else "")
            for d in diagnostics
        ]
        lines.append(_row(names[pair], cells, width))
    return "\n".join(lines)


def run(
    run_dir: Path,
    out: Path | None = None,
    priors: Path | None = None,
    arms: list[str] | None = None,
    prior_draws: int = 20_000,
    seed: int = 1,
    ks_alpha: float = 0.05,
    rho_alpha: float = 0.01,
    dpi: int = 110,
    repo_root: Path = Path(),
) -> None:
    """Write one pairs figure per arm and print the diagnostic tables."""
    import matplotlib.pyplot as plt

    run_dir = Path(run_dir)
    out = Path(out) if out is not None else run_dir / "figures"
    out.mkdir(parents=True, exist_ok=True)
    arms = arms or discover_arms(run_dir)

    diagnostics = []
    for arm in arms:
        priors_path = priors or resolve_priors_path(run_dir, arm, repo_root)
        prior, params = load_prior_draws(Path(priors_path), prior_draws, seed)
        diag = arm_diagnostics(run_dir, arm, prior, params, ks_alpha)
        figure = pairs_figure(diag, prior, params, rho_alpha)
        path = out / f"pairs_{arm}.png"
        figure.savefig(path, dpi=dpi)
        plt.close(figure)
        print(f"wrote {path}")
        diagnostics.append(diag)

    print()
    print(format_ks_table(diagnostics, params))
    print()
    print(format_rho_table(diagnostics, rho_alpha))
