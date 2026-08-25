"""The ``vhf plot`` command group: figures from a finished run.

``vhf plot <command> <run_dir>`` reads everything it needs from the run --
the arms are discovered from the directory, and each arm's ``config.json``
names the priors it was calibrated against.
"""

import argparse
from pathlib import Path


def _run_pairs(args: argparse.Namespace) -> None:
    from ..plots.posterior_pairs import run

    run(
        run_dir=args.run_dir,
        out=args.out,
        priors=args.priors,
        arms=args.arms,
        prior_draws=args.prior_draws,
        seed=args.seed,
        ks_alpha=args.ks_alpha,
        rho_alpha=args.rho_alpha,
        dpi=args.dpi,
    )


def add_plot_parser(subparsers: argparse._SubParsersAction) -> None:
    """Attach the ``plot`` command group to the top-level ``vhf`` parser."""
    parser = subparsers.add_parser("plot", help="Plot diagnostics from a finished run")
    sub = parser.add_subparsers(dest="plot", required=True)

    pairs = sub.add_parser(
        "pairs",
        help="Posterior pairs grid per arm, with weighted KS and Spearman",
    )
    pairs.add_argument("run_dir", type=Path, help="A finished run directory")
    pairs.add_argument(
        "--out",
        type=Path,
        default=None,
        help="Where to write figures (default: <run_dir>/figures)",
    )
    pairs.add_argument(
        "--priors",
        type=Path,
        default=None,
        help="Override the priors recorded in each arm's config.json",
    )
    pairs.add_argument(
        "--arms",
        nargs="+",
        default=None,
        help="Restrict to these arms (default: every arm in the run)",
    )
    pairs.add_argument("--prior-draws", type=int, default=20_000)
    pairs.add_argument("--seed", type=int, default=1)
    pairs.add_argument(
        "--ks-alpha",
        type=float,
        default=0.05,
        help="Level for the KS noise floor (default: 0.05)",
    )
    pairs.add_argument(
        "--rho-alpha",
        type=float,
        default=0.01,
        help="Level for marking Spearman correlations (default: 0.01)",
    )
    pairs.add_argument("--dpi", type=int, default=110)
    pairs.set_defaults(func=_run_pairs)
