"""Underdetection range workflow: the full BVD case detection calibration and scenario analysis
"""

import datetime as dt
import json
from dataclasses import dataclass
from pathlib import Path

from vhf_pipeline.cli.run_model import _load_json
from vhf_pipeline.pipeline.load_1cdp_confirmation import (
    ISO3C_VALUES,
    load_1cdp_confirmation,
)
from vhf_pipeline.utils.read_griddle import read_griddle

from .. import paths
from ..pipeline import (
    build,
    calibrate_onset,
    rt_by_ratio_category,
    summary_figures,
    symptom_onset_figures,
)
from ..pipeline.summary_figures import make_name
from ..pipeline.write_threshold_data import write_threshold_data
from ..plots import posterior_pairs
from ..utils import load_intervention_date
from .base import Workflow

# Default arguments declaration -------------------
DEFAULT_CONFIG = "experiments/detection/config.json"
DEFAULT_SCENARIOS = "experiments/detection/all_confirmed_scenario.json"
THRESHOLD_UPPER_BOUND = 500
PROJECTION_DATE = dt.date(2026, 9, 30)  # The date to project to for the figures.
MAX_DATA_DATE = dt.date(2026, 8, 10)  # The last date of the data used for calibration.
INTERVENTION_START_DATE = dt.date(2026, 5, 24)  # The date of the interventions
OUTBREAK_ISO3C_EXTENT = ["COD"]

@dataclass(frozen=True)
class DetectionRangeArgs:
    output_subdir: str = ""
    main_config: str = DEFAULT_CONFIG
    threshold_upper_bound: int = THRESHOLD_UPPER_BOUND
    scenarios: str = DEFAULT_SCENARIOS
    cache: bool = True
    refresh_cache: bool = False
    reuse_across_binary: bool = False
    max_data_date: dt.date = MAX_DATA_DATE
    intervention_start_date: dt.date = INTERVENTION_START_DATE
    projection_date: dt.date = PROJECTION_DATE
    iso3c_codes: list[ISO3C_VALUES] = (OUTBREAK_ISO3C_EXTENT,)

class CacheRefresh:
    def __init__(self, flag: bool = False) -> None:
        self.flag = flag

    def update(self, new_flag: bool) -> None:
        # don't replace a previously "true" refresh flag
        if not self.flag:
            self.flag = new_flag

class DetectionRangeWorkflow(Workflow[DetectionRangeArgs]):
    """The full BVD detection band workstream"""

    name = "detection"

    def parse_args(self, argv: list[str]) -> DetectionRangeArgs:
        parser = self.parser()
        parser.add_argument("-o", "--output-subdir", default="")
        parser.add_argument("-c", "--main-config", default=DEFAULT_CONFIG)

        # Parse input parameters
        parser.add_argument("--scenarios", default=DEFAULT_SCENARIOS)
        parser.add_argument(
            "--threshold-upper-bound",
            type=int,
            default=THRESHOLD_UPPER_BOUND,
        )
        parser.add_argument(
            "--iso3c-codes",
            nargs="+",
            default=OUTBREAK_ISO3C_EXTENT,
            help="The ISO3C codes to include in the calibration.",
        )

        # Parse date input parameters
        parser.add_argument(
            "--max-data-date",
            type=dt.date.fromisoformat,
            default=MAX_DATA_DATE,
            help="The last date of the data used for calibration.",
        )
        parser.add_argument(
            "--intervention-start-date",
            type=dt.date.fromisoformat,
            default=INTERVENTION_START_DATE,
            help="The first date of the surveillance and intervention program.",
        )
        parser.add_argument(
            "--projection-date",
            type=dt.date.fromisoformat,
            default=PROJECTION_DATE,
            help="The last date to project until.",
        )

        # Parse caching parameters
        parser.add_argument(
            "--no-cache",
            dest="cache",
            action="store_false",
            default=True,
            help="Always re-run the setup calibration instead of using the cache.",
        )
        parser.add_argument(
            "--refresh-cache",
            action="store_true",
            default=False,
            help="Ignore any cached posterior, re-run, and overwrite the cache.",
        )
        parser.add_argument(
            "--reuse-across-binary",
            action="store_true",
            default=False,
            help="Reuse a cached posterior even if the model binary changed (warns).",
        )

        ns = parser.parse_args(argv)
        return DetectionRangeArgs(
            output_subdir=ns.output_subdir,
            main_config=ns.main_config,
            threshold_upper_bound=ns.threshold_upper_bound,
            cache=ns.cache,
            scenarios=ns.scenarios,
            refresh_cache=ns.refresh_cache,
            reuse_across_binary=ns.reuse_across_binary,
            max_data_date=ns.max_data_date,
            intervention_start_date=ns.intervention_start_date,
            projection_date=ns.projection_date,
            iso3c_codes=ns.iso3c_codes,
        )
    
    def manifest_dir(self, args: DetectionRangeArgs) -> Path:
        return paths.output_dir(args.output_subdir)

    def manifest_inputs(self, args: DetectionRangeArgs) -> list[tuple[str, Path]]:
        return [
            ("config_file", Path(args.main_config)),
            ("scenarios", Path(args.scenarios)),
        ]

    def load_upper_threshold(self, args: DetectionRangeArgs) -> bool:
        """Load the upper threshold value from the config file.
        Return True if the value was updated, False otherwise.
        """
        with open(args.main_config, "r") as f:
            config = json.load(f)
        filename = write_threshold_data(
            directory=paths.data_input_dir(),
            threshold=args.threshold_upper_bound,
            date=args.intervention_start_date,
        )
        if config["target_data_file"]["deaths_threshold"] != filename:
            config["target_data_file"]["deaths_threshold"] = filename
            with open(args.main_config, "w") as f:
                json.dump(config, f, indent=4)
            return True
        return False

    def run(self, args: DetectionRangeArgs) -> None:
        build.main()

        cache_refresh = CacheRefresh(flag=args.refresh_cache)
        cache_refresh.update(self.load_upper_threshold(args))
        config = _load_json(args.main_config)

        cache_refresh.update(
            load_1cdp_confirmation(
                filename=config["target_data_file"]["confirmation"],
                max_data_date=args.max_data_date,
                iso3c=args.iso3c_codes,
            )
        )

        ascertainment_scenarios = read_griddle(args.scenarios)
        assert load_intervention_date(args.main_config) == args.intervention_start_date, (
            f"Intervention start date in config file ({load_intervention_date(args.main_config)}) does not match the expected date ({args.intervention_start_date})."
        )

        for scenario in ascertainment_scenarios.iter_rows(named=True):
            scenario_name = make_name(scenario)
            print(f"Running scenario: {scenario_name}")
            # Main calibration to symptom onset data
            calibrate_onset.main(
                output_dir=args.output_subdir,
                subdir_name=scenario_name,
                config_file=args.main_config,
                ixa_scenario_overrides=scenario,
                cache=args.cache,
                refresh_cache=cache_refresh.flag,
                reuse_across_binary=args.reuse_across_binary,
            )

            # Generate figures for symptom onset calibration of each scenario
            symptom_onset_figures.main(
                output_subdir=args.output_subdir,
                projection_date=args.projection_date,
                calibration_subdir=scenario_name,
                plot_incidence=False,
                save_detection_rate=True,
            )

            rt_by_ratio_category.main(
                output_subdir=args.output_subdir,
                calibration_subdir=scenario_name,
                projection_date=args.projection_date,
                rt_window_days=15,
            )

        posterior_pairs.run(run_dir=paths.output_dir(args.output_subdir))

        # Generate the summary figures across scenarios
        summary_figures.main(
            output_subdir=args.output_subdir,
            scenarios=ascertainment_scenarios,
            config_file=args.main_config,
            current_date=args.max_data_date,
            max_date=args.projection_date,
        )


WORKFLOW = DetectionRangeWorkflow()
