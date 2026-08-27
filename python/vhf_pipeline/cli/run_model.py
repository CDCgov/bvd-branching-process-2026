"""The ``vhf model`` command group: run the ixa model or a calibration.

- ``vhf model run``                 run the ixa model once from an ixa config
- ``vhf model calibrate``           run one calibration from a config file
"""

import argparse
import json
from pathlib import Path

from .. import paths

DEFAULT_IXA_CONFIG = "input/input.json"


def _load_json(path: str | Path) -> dict:
    with open(path, "r", encoding="utf-8") as fp:
        return json.load(fp)


def _run(args: argparse.Namespace) -> None:
    from mrp import Environment

    from ..model.runner import VHFModel

    ixa_config = _load_json(args.ixa_config)
    output_dir = paths.output_dir(override=args.output_dir)
    env = Environment(
        {
            "input": {
                "force_overwrite": args.force_overwrite,
                "ixa_config": ixa_config,
                "exe_file": "./target/release/vhf_model",
                "outputs_to_read": [
                    {"spec": "relative", "name": "prevalence_report"},
                    {"spec": "relative", "name": "symptom_onset_report"},
                ],
            },
            "output": {"spec": "filesystem", "dir": str(output_dir)},
        }
    )
    model = VHFModel(env=env)
    model.run()

    if args.verbose:
        outputs = model.read_outputs(config=model.input, output_dir=output_dir)
        for name, output in outputs.items():
            print(f"Output: {name}")
            print(output)


def _calibrate(args: argparse.Namespace) -> None:
    from ..model.context import CalibrationContext

    output_dir = paths.output_dir(args.output_subdir)
    calibration_config = _load_json(args.config_file)
    CalibrationContext(calibration_config, output_dir).run()


def add_model_parser(subparsers: argparse._SubParsersAction) -> None:
    """Attach the ``model`` command group to the top-level ``vhf`` parser."""
    parser = subparsers.add_parser("model", help="Run the ixa model or a calibration")
    sub = parser.add_subparsers(dest="model_command", required=True)

    p_run = sub.add_parser("run", help="Run the ixa model once from an ixa config")
    p_run.add_argument(
        "-o",
        "--output-dir",
        default=None,
        help=(
            "Output directory. Defaults to the OUTPUT_DIR env var, then "
            f"'{paths.DEFAULT_OUTPUT_DIR}'."
        ),
    )
    p_run.add_argument("-c", "--ixa-config", default=DEFAULT_IXA_CONFIG)
    p_run.add_argument("-f", "--force-overwrite", action="store_true")
    p_run.add_argument("-v", "--verbose", action="store_true")
    p_run.set_defaults(func=_run)

    p_cal = sub.add_parser("calibrate", help="Run one calibration from a config file")
    p_cal.add_argument("-c", "--config-file", required=True)
    p_cal.add_argument(
        "-o",
        "--output-subdir",
        default="",
        help="Subdirectory under the OUTPUT_DIR base to write results to.",
    )
    p_cal.set_defaults(func=_calibrate)
