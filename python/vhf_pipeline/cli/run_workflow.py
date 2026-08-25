"""The ``vhf workflow`` command group: run a named end-to-end workflow.

``vhf workflow <name> [workflow args...]`` selects a workflow by name, then
hands the remaining arguments to that workflow to parse and run. Only the
selected workflow is imported, so its (heavy) plotting deps stay lazy, and each
workflow owns its own typed arguments — see ``vhf workflow <name> -h``.
"""

import argparse
import importlib


def _run_workflow(args: argparse.Namespace) -> None:
    from ..workflows import WORKFLOWS

    module = importlib.import_module(f"vhf_pipeline.workflows.{WORKFLOWS[args.name]}")
    module.WORKFLOW.main(args.workflow_args)


def add_workflow_parser(subparsers: argparse._SubParsersAction) -> None:
    """Attach the ``workflow`` command group to the top-level ``vhf`` parser."""
    from ..workflows import WORKFLOWS

    parser = subparsers.add_parser("workflow", help="Run a named end-to-end workflow")
    parser.add_argument("name", choices=sorted(WORKFLOWS), help="Which workflow to run")
    parser.add_argument(
        "workflow_args",
        nargs=argparse.REMAINDER,
        help="Arguments for the workflow (see 'vhf workflow <name> -h')",
    )
    parser.set_defaults(func=_run_workflow)
