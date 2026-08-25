import json
import shutil
import subprocess
from pathlib import Path
from typing import Any

import polars as pl
from mrp import MRPModel

_MODEL_PARAMETERS_NAME = "evdmodel.Parameters"


def _run_ixa_model(
    output_dir: Path,
    exe_file: str,
    ixa_config: dict[str, Any],
    force_overwrite: bool = False,
    ixa_parameter_key: str = _MODEL_PARAMETERS_NAME,
):
    """
    Run the ixa model with the given configuration and output directory.
    Parameters:
    - output_dir: Path to the directory where model outputs will be saved
    - exe_file: Path to the ixa model executable
    - ixa_config: Dictionary containing the ixa model configuration
    - force_overwrite: If True, will overwrite existing output directory. If False, will raise an error if output directory already exists.
    - ixa_parameter_key: The key in the ixa_config dictionary where the model parameters (including seed) are located
    """
    # Create output directory if it doesn't exist, or if force_overwrite is True
    output_dir.mkdir(parents=True, exist_ok=force_overwrite)

    # Save the ixa config to the output directory with the seed as an integer
    input_file_path = output_dir / "simulation_config.json"
    ixa_config[ixa_parameter_key]["seed"] = int(ixa_config[ixa_parameter_key]["seed"])
    days_since_start = ixa_config[ixa_parameter_key]["initialization"]["initial_cases"][
        "SpilloverEvent"
    ].get("days_since_start", None)
    if days_since_start is not None and days_since_start < 0:
        ixa_config[ixa_parameter_key]["initialization"]["initial_cases"][
            "SpilloverEvent"
        ]["days_since_start"] = 0.0

    with open(input_file_path, "w") as f:
        json.dump(ixa_config, f, indent=4)

    # Run the ixa model with the config file and output directory as arguments
    cmd = [
        exe_file,
        "--config",
        str(input_file_path),
        "--output",
        str(output_dir),
        "--no-stats",
    ]

    if force_overwrite:
        cmd = cmd + ["--force-overwrite"]

    try:
        subprocess.run(cmd, capture_output=True, check=True)
    except subprocess.CalledProcessError as e:
        print("Error running the ixa model:")
        print("Command:", " ".join(cmd))
        print("Return code:", e.returncode)
        print("Standard error:", e.stderr)
        raise e


class VHFModel(MRPModel):
    @property
    def ixa_parameter_key(self) -> str:
        return _MODEL_PARAMETERS_NAME

    def run(self):
        _run_ixa_model(
            output_dir=self.env.output_dir,
            exe_file=self.env.input["exe_file"],
            ixa_config=self.env.input["ixa_config"],
            force_overwrite=self.env.input.get("force_overwrite", False),
            ixa_parameter_key=self.ixa_parameter_key,
        )

    def simulate(self, model_config: dict[str, Any]) -> dict[str, pl.DataFrame]:
        output_dir = self.env.output_dir / model_config.get("output_subdir", "")
        _run_ixa_model(
            output_dir,
            exe_file=model_config["exe_file"],
            ixa_config=model_config["ixa_config"],
            force_overwrite=model_config.get("force_overwrite", False),
            ixa_parameter_key=self.ixa_parameter_key,
        )

        outputs = self.read_outputs(model_config, output_dir)
        if model_config.get("clean", False):
            shutil.rmtree(output_dir)

        return outputs

    def read_outputs(
        self, config: dict[str, Any], output_dir: Path
    ) -> dict[str, pl.DataFrame]:
        outputs = {}
        for output in config["outputs_to_read"]:
            fp = config["ixa_config"][self.ixa_parameter_key][output["name"]][
                "filename"
            ]
            relative_output_file = output_dir / fp
            if output["spec"] == "relative":
                if relative_output_file.exists():
                    try:
                        outputs.update(
                            {output["name"]: pl.read_csv(relative_output_file)}
                        )
                    except pl.exceptions.NoDataError:
                        outputs.update({output["name"]: pl.DataFrame()})
                else:
                    raise FileNotFoundError(
                        f"Expected output file {relative_output_file} not found. Check that path is correct for relative output and not {fp}."
                    )
            elif output["spec"] == "absolute":
                if Path(fp).exists():
                    try:
                        outputs.update({output["name"]: pl.read_csv(Path(fp))})
                    except pl.exceptions.NoDataError:
                        outputs.update({output["name"]: pl.DataFrame()})
                else:
                    raise FileNotFoundError(
                        f"Expected output file {fp} not found. Check that path is correct for absolute output and not {relative_output_file}."
                    )
        return outputs
