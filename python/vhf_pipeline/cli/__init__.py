"""Single entry point for the VHF model and pipeline: ``vhf <group> <command>``.

Command groups:
- ``model``      run the ixa model (``run``) or a calibration (``calibrate``)
- ``workflow``   run a named end-to-end workflow (e.g. ``mmwr``)
- ``plot``       diagnostics from a finished run (e.g. ``pairs``)
"""

import argparse

from .plot import add_plot_parser
from .run_model import add_model_parser
from .run_workflow import add_workflow_parser


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="vhf", description="CFA EVD 2025 intervention model and pipeline"
    )
    sub = parser.add_subparsers(dest="group", required=True)
    add_model_parser(sub)
    add_workflow_parser(sub)
    add_plot_parser(sub)
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main()
