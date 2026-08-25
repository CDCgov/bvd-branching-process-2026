import json
import os
import pickle
import shutil
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any, Literal

import numpy as np
import polars as pl
from calibrationtools import (
    ABCSampler,
    AdaptMultivariateNormalVariance,
    CalibrationResults,
    IndependentKernels,
    MultivariateNormalKernel,
    NormalKernel,
    Particle,
    ParticlePopulation,
    ParticleReader,
    ParticleRunner,
    SeedKernel,
    flatten_dict,
)
from mrp import Environment

from .. import paths
from .calibration_cache import CalibrationCache
from .data_handler import NaturalHistoryHandler
from .runner import VHFModel


def disable_unused_reports(
    ixa_params: dict[str, Any], required: tuple[str, ...]
) -> None:
    """Turn off `write` for every enabled report not in `required`, in place.

    Reports are passive observers, so skipping their output does not change the
    simulation's RNG stream or dynamics. During calibration only the objective's
    report is read back, so writing the others is pure per-particle I/O waste.
    """
    keep = set(required)
    for name, param in ixa_params.items():
        if (
            isinstance(param, dict)
            and param.get("write")
            and param.get("filename") is not None
            and name not in keep
        ):
            param["write"] = False


def _resolve_output_dir(output_dir: Path, force_overwrite: bool) -> None:
    if output_dir.exists():
        if force_overwrite:
            shutil.rmtree(str(output_dir))
        else:
            raise FileExistsError(
                f"Output directory {output_dir} already exists and force_overwrite is set to False."
            )
    output_dir.mkdir(parents=True, exist_ok=False)


class BranchingProcessContext(ABC):
    def __init__(
        self,
        config: dict[str, Any],
        output_dir: Path,
        mode: Literal["calibration", "projection"],
        verbose: bool = True,
    ):
        # General configuration loading and model environment setup - this should be handled automatically to some extent by calibrationtools
        # -----------------------------------
        self.config = config
        self.output_dir = output_dir
        self.mode = mode
        self.verbose = verbose
        self.force_overwrite = self.config.get("force_overwrite", False)
        self._validate_config()

        if self.config.get("target_data_file") is not None:
            data_input_path = paths.data_input_dir()
            if isinstance(self.config["target_data_file"], dict):
                self.target_data_file = {
                    key: data_input_path / value
                    for key, value in self.config["target_data_file"].items()
                }
            else:
                self.target_data_file = (
                    data_input_path / self.config["target_data_file"]
                )
        else:
            self.target_data_file = None
        if self.config.get("priors_file") is not None:
            with open(self.config["priors_file"], "r") as fp:
                self.priors = json.load(fp)
        else:
            self.priors = None

        # ----------------------------------------------
        # Set up model environment and runner

        env = Environment(
            {
                "output": {
                    "spec": "filesystem",
                    "dir": self.output_dir / self.mode / "simulations",
                }
            }
        )
        self.model = VHFModel(env=env)
        self._handler = NaturalHistoryHandler(self.config["strategy"])
        self.set_defaults()

        # ----------------------------------------------

    def _validate_config(self):
        # Validate config file and raise errors if invalid
        required_keys = set(
            [
                "strategy",
                "default_ixa_file",
                "exe_file",
                self.mode,
            ]
        )
        if self.mode == "calibration":
            required_keys.update(
                [
                    "priors_file",
                    "target_data_file",
                ]
            )
        missing_keys = required_keys - self.config.keys()
        if missing_keys:
            raise ValueError(f"Missing required config keys: {missing_keys}")

    def set_defaults(self):
        # --------------------------------------
        # Model specific updating of vaccine deployment file and storing outputs to read
        with open(self.config["default_ixa_file"], "r") as fp:
            default_ixa_config = json.load(fp)

        # check for experiment-wide ixa overrides
        if self.config.get("default_ixa_overrides") is not None:
            for param_name, param_value in self.config["default_ixa_overrides"].items():
                default_ixa_config[self.model.ixa_parameter_key][param_name] = (
                    param_value
                )

        # check for mode-specific ixa overrides
        if self.config[self.mode].get("default_ixa_overrides") is not None:
            for param_name, param_value in self.config[self.mode][
                "default_ixa_overrides"
            ].items():
                default_ixa_config[self.model.ixa_parameter_key][param_name] = (
                    param_value
                )

        # During calibration only the objective's report is read back, so skip
        # writing the rest (bit-for-bit identical results, less per-particle I/O).
        if self.mode == "calibration":
            disable_unused_reports(
                default_ixa_config[self.model.ixa_parameter_key],
                self._handler.required_reports,
            )

        # Set up defaults to be called with the particle reader
        self.mrp_defaults = {
            "force_overwrite": self.force_overwrite,
            "clean": self.config[self.mode]["clean"],
            "output_subdir": "",
            "ixa_config": default_ixa_config,
            "exe_file": self.config["exe_file"],
            "outputs_to_read": self._get_ixa_outputs_to_read(default_ixa_config),
        }

    def update_ixa_default(self, param_name: str, param_value: Any):
        self.mrp_defaults["ixa_config"][self.model.ixa_parameter_key][param_name] = (
            param_value
        )

    def _get_ixa_outputs_to_read(
        self, ixa_config: dict[str, Any]
    ) -> list[dict[str, str]]:
        outputs_to_read = []
        for param_name, param_config in ixa_config[
            self.model.ixa_parameter_key
        ].items():
            if isinstance(param_config, dict):
                if (
                    param_config.get("write", False)
                    and param_config.get("filename") is not None
                ):
                    outputs_to_read.append(
                        {
                            "name": param_name,
                            "spec": "relative"
                            if not Path(param_config["filename"]).is_absolute()
                            else "absolute",
                        }
                    )
        return outputs_to_read

    def set_reader(self, particle_param_names: list[str]):
        self.reader = ParticleReader(
            particle_param_names=particle_param_names,
            default_params=self.mrp_defaults,
        )

        def particles_to_params(
            particle: Particle, reader: ParticleReader = self.reader
        ):
            particle_params = reader.read_particle(particle=particle)
            particle_params["output_subdir"] = str(np.random.SeedSequence().entropy)
            return particle_params

        self.particles_to_params = particles_to_params

    def get_target_data(self) -> pl.DataFrame | None:
        if self.target_data_file is not None:
            return self._handler.get_target_data(self.target_data_file)
        else:
            return None

    def process_outputs(self, outputs: dict[str, pl.DataFrame]) -> pl.DataFrame:
        return self._handler.process_outputs(outputs)

    def estimate_error(
        self, outputs: dict[str, pl.DataFrame], target_df: pl.DataFrame
    ) -> float:
        return self._handler.estimate_error(outputs, target_df)

    @abstractmethod
    def run(self):
        pass

    @abstractmethod
    def save(self):
        pass


class CalibrationContext(BranchingProcessContext):
    def __init__(self, config: dict, output_dir: Path, verbose: bool = True):
        super().__init__(config, output_dir, mode="calibration", verbose=verbose)

    def _initialize_cache(self, cache: bool, cache_dir: str) -> CalibrationCache | None:
        """Initialize cache object if caching is enabled."""
        return CalibrationCache(cache_dir) if cache else None

    def _check_binary_compatibility(
        self,
        calibration_cache: CalibrationCache,
        cache_key: str,
        reuse_across_binary: bool,
    ) -> bool:
        """Check if cached binary matches current binary. Returns True if compatible."""
        stored_bin = calibration_cache.load_meta(cache_key).get("binary_sha")
        current_bin = calibration_cache.binary_hash(self.config)
        if stored_bin != current_bin and not reuse_across_binary:
            if self.verbose:
                print(
                    "binary changed since the cached "
                    f"posterior for {cache_key}; re-running "
                    "(include reuse_across_binary=True to reuse it anyway)"
                )
            return False
        return True

    def _load_cache(
        self,
        cache: bool,
        cache_dir: str,
        refresh_cache: bool = False,
        reuse_across_binary: bool = False,
    ) -> tuple[CalibrationCache, str, Path] | tuple[None, None, None]:
        calibration_cache = self._initialize_cache(cache, cache_dir)
        if calibration_cache is None:
            return (None, None, None)

        cache_key = calibration_cache.key_for_config(self.config)
        cached_pkl = calibration_cache.load(cache_key) if not refresh_cache else None
        if cached_pkl is not None:
            if self._check_binary_compatibility(
                calibration_cache, cache_key, reuse_across_binary
            ):
                return (calibration_cache, cache_key, cached_pkl)
            else:
                return (None, None, None)
        else:
            return (calibration_cache, cache_key, None)

    def run(
        self,
        cache: bool = True,
        cache_dir: str | None = None,
        refresh_cache: bool = False,
        reuse_across_binary: bool = False,
    ):
        calibration_cache, cache_key, cached_pkl = self._load_cache(
            cache=cache,
            cache_dir=cache_dir,
            refresh_cache=refresh_cache,
            reuse_across_binary=reuse_across_binary,
        )

        calibration_results_file = (
            self.output_dir / self.mode / "calibration_results.pkl"
        )

        if cached_pkl is not None:
            if (
                not self.config.get("force_overwrite", False)
                and calibration_results_file.exists()
            ):
                raise FileExistsError(
                    f"{calibration_results_file} already exists and "
                    "force_overwrite is False."
                )

            calibration_results_file.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(cached_pkl, calibration_results_file)
            with open(calibration_results_file, "rb") as fp:
                self.results: CalibrationResults = pickle.load(fp)
            if self.verbose:
                print("Restoring calibration fit from previous run")
                print(self.results)
        else:
            _resolve_output_dir(self.model.env.output_dir, self.force_overwrite)
            self.set_reader(list(self.priors["priors"].keys()) + ["seed"])
            if len(self.priors["priors"]) == 0:
                perturbations = SeedKernel(
                    "seed",
                    prob_keep=self.config["calibration"].get("prob_keep_seed", 0.0),
                )
            elif len(self.priors["priors"]) == 1:
                perturbations = IndependentKernels(
                    [
                        NormalKernel(
                            list(self.priors["priors"].keys())[0], std_dev=1.0
                        ),
                        SeedKernel(
                            "seed",
                            prob_keep=self.config["calibration"].get(
                                "prob_keep_seed", 0.0
                            ),
                        ),
                    ]
                )
            else:
                perturbations = IndependentKernels(
                    [
                        MultivariateNormalKernel(list(self.priors["priors"].keys())),
                        SeedKernel(
                            "seed",
                            prob_keep=self.config["calibration"].get(
                                "prob_keep_seed", 0.0
                            ),
                        ),
                    ]
                )

            runner = ABCSampler(
                generation_particle_count=self.config["calibration"][
                    "generation_particle_count"
                ],
                tolerance_values=self.config["calibration"]["tolerance_values"],
                priors=self.priors,
                particles_to_params=self.particles_to_params,
                perturbation_kernel=perturbations,
                variance_adapter=AdaptMultivariateNormalVariance(),
                outputs_to_distance=self.estimate_error,
                target_data=self.get_target_data(),
                model_runner=self.model,
                keep_previous_population_data=self.config["calibration"].get(
                    "keep_previous_population_data", False
                ),
                entropy=self.config["calibration"]["entropy"],
            )

            self.results = runner.run()
            self.save(
                calibration_results_file=calibration_results_file,
                cache_key=cache_key,
                calibration_cache=calibration_cache,
            )

    def save(
        self,
        calibration_results_file: Path | None = None,
        cache_key: str | None = None,
        calibration_cache: CalibrationCache | None = None,
    ):
        if self.verbose:
            print(self.results)
        if calibration_results_file is None:
            calibration_results_file = (
                self.output_dir / self.mode / "calibration_results.pkl"
            )
        with open(calibration_results_file, "wb") as f:
            pickle.dump(self.results, f)

        if calibration_cache is not None:
            calibration_cache.store(
                cache_key,
                calibration_results_file,
                meta={
                    "parent_dir": self.output_dir,
                    "binary_sha": calibration_cache.binary_hash(self.config),
                },
            )


class ProjectionContext(BranchingProcessContext):
    def __init__(self, config: dict, output_dir: Path, verbose: bool = True):
        super().__init__(config, output_dir, mode="projection", verbose=verbose)

    def run(
        self,
        particles: ParticlePopulation | list[Particle] | pl.DataFrame,
    ):
        _resolve_output_dir(self.model.env.output_dir, self.force_overwrite)
        if isinstance(particles, pl.DataFrame):
            particle_list = []
            for row in particles.iter_rows(named=True):
                particle_dict = {k: row[k] for k in particles.columns}
                particle_list.append(Particle(particle_dict))
            particles = particle_list
        elif isinstance(particles, ParticlePopulation):
            particles = particles.particles
        elif not isinstance(particles, list):
            raise ValueError(
                f"Invalid type for particles: {type(particles)}. Must be list of Particle or polars DataFrame."
            )
        param_names = list(particles[0].keys())
        self.set_reader(param_names)

        runner = ParticleRunner(
            model=self.model,
            particles_to_params=self.particles_to_params,
        )

        runner.run(particles)
        self.save()

    def save(self, process_outputs: bool = True):
        all_simulations = []
        raw_simulations = {}
        ixa_inputs = []
        i = 0
        for root, dir, _ in os.walk(self.model.env.output_dir):
            for subdir in dir:
                simulation_dir = Path(root) / subdir

                # Read the ixa input config JSON for parameters used
                input_file = simulation_dir / "simulation_config.json"
                if input_file.exists():
                    with open(input_file, "r") as f:
                        simulation_config = json.load(f)
                        config_df = pl.DataFrame(
                            flatten_dict(
                                simulation_config[self.model.ixa_parameter_key]
                            )
                        ).with_columns(pl.lit(i).alias("particle_id"))
                        ixa_inputs.append(config_df)
                else:
                    raise FileNotFoundError(
                        f"Expected input file {input_file} not found. Skipping directory {subdir}."
                    )

                # Read in the ouptuts generated by the model for this particle
                raw_outputs = self.model.read_outputs(self.mrp_defaults, simulation_dir)
                target_data = self.get_target_data()
                if target_data is not None:
                    err = self.estimate_error(raw_outputs, target_data)
                else:
                    err = None

                if process_outputs:
                    # Process the raw outputs into a dataframe using config-specified function strategy
                    sim_data = self.process_outputs(raw_outputs).with_columns(
                        pl.lit(i).alias("particle_id"),
                        pl.lit(err).alias("total_error"),
                    )

                    all_simulations.append(sim_data)
                else:
                    for output_name, output_df in raw_outputs.items():
                        output_df = output_df.with_columns(
                            pl.lit(i).alias("particle_id"),
                            pl.lit(err).alias("total_error"),
                        )
                        raw_simulations.setdefault(output_name, []).append(output_df)
                i += 1

        if process_outputs:
            simulations_df: pl.DataFrame = pl.concat(
                all_simulations, how="vertical_relaxed"
            )
            simulations_df.write_csv(
                self.output_dir / self.mode / "all_simulations.csv"
            )
        else:
            for output_name, output_dfs in raw_simulations.items():
                output_df = pl.concat(output_dfs, how="diagonal_relaxed")
                output_df.write_csv(
                    self.output_dir / self.mode / f"all_{output_name}s.csv"
                )
        simulation_inputs_df = pl.concat(ixa_inputs)

        simulation_inputs_df.write_csv(
            self.output_dir / self.mode / "all_simulation_inputs.csv"
        )
