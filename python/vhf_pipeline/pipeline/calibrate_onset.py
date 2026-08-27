import json

from vhf_pipeline import (
    BranchingProcessContext,
    CalibrationContext,
    ProjectionContext,
    paths,
)


def change_default_report_date(context: BranchingProcessContext) -> None:
    """Change the default report date in the ixa config to match the target data.

    If target data is unavailable or lacks a ``report_date`` column (e.g. for the
    ``deaths_threshold`` strategy), this is a no-op.
    """
    target_data = context.get_target_data()
    if target_data is None or "report_date" not in target_data.columns:
        return
    report_date = str(target_data["report_date"][0])
    context.update_ixa_default(
        "symptom_onset_report",
        {
            "write": True,
            "filename": "symptom_onset_report.csv",
            "period": 1.0,
            "trigger": {"Date": {"date": report_date}},
        },
    )


def update_overrides(config: dict, default_ixa_overrides: dict | None = None) -> None:
    """Set up the context for symptom onset calibration."""
    if default_ixa_overrides is not None:
        if "default_ixa_overrides" not in config:
            config.update({"default_ixa_overrides": default_ixa_overrides})
        else:
            config["default_ixa_overrides"].update(default_ixa_overrides)


def main(
    output_dir: str = "",
    subdir_name: str = "",
    config_file: str = "",
    ixa_scenario_overrides: dict | None = None,
    cache: bool = True,
    refresh_cache: bool = False,
    reuse_across_binary: bool = False,
) -> None:
    """Calibrate then project the detection scenarios for each scenario."""
    output_dir = paths.output_dir(output_dir)
    with open(config_file, "r") as fp:
        base_config = json.load(fp)
    update_overrides(base_config, ixa_scenario_overrides)

    calibration = CalibrationContext(base_config, output_dir / subdir_name)
    change_default_report_date(calibration)
    calibration.run(
        cache=cache,
        refresh_cache=refresh_cache,
        reuse_across_binary=reuse_across_binary,
    )

    with open(output_dir / subdir_name / "config.json", "w") as fp:
        json.dump(base_config, fp, indent=4)

    posterior = calibration.results.posterior_particles

    projection = ProjectionContext(base_config, output_dir / subdir_name)
    change_default_report_date(projection)
    projection.run(particles=posterior)
    projection.save(process_outputs=False)
