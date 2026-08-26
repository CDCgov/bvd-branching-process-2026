"""End-to-end integration tests for the vhf_pipeline.

These run the real compiled ixa model and the actual pipeline stages, so they
catch breakage that unit tests of individual functions would miss. They use tiny
run sizes (a couple of particles, short epidemics, a single death threshold) so
the whole file stays in the tens-of-seconds range.

Coverage:
- ``test_model_run_writes_outputs``        the raw model runs and writes reports
- ``test_calibration_produces_posterior``  the ABC calibration yields a posterior
- ``test_pipeline_end_to_end_single_threshold``  calibrate_onset -> symptom_onset_figures -> rt_by_ratio_category -> summary_figures
"""

import json
import pickle
from pathlib import Path

import polars as pl
from vhf_pipeline.pipeline import calibrate_onset


def assert_csv_has_columns(path: Path, expected_columns: set[str]) -> pl.DataFrame:
    data = pl.read_csv(path, try_parse_dates=True)
    missing_columns = expected_columns - set(data.columns)
    assert not missing_columns, (
        f"{path.name} missing columns: {sorted(missing_columns)}"
    )
    assert data.height > 0, f"{path.name} should not be empty"
    return data


def test_model_run_writes_outputs(tmp_path, model_binary):
    """The model runs from an ixa config and writes a non-empty prevalence report."""
    from mrp import Environment
    from vhf_pipeline.model import VHFModel

    repo_root = Path(__file__).resolve().parents[1]
    ixa_config = json.loads((repo_root / "input" / "input.json").read_text())
    out = tmp_path / "model_run"
    env = Environment(
        {
            "input": {
                "force_overwrite": True,
                "ixa_config": ixa_config,
                "exe_file": str(model_binary),
                "outputs_to_read": [
                    {"spec": "relative", "name": "prevalence_report"},
                    {"spec": "relative", "name": "symptom_onset_report"},
                ],
            },
            "output": {"spec": "filesystem", "dir": str(out)},
        }
    )
    model = VHFModel(env=env)
    model.run()

    assert (out / "simulation_config.json").exists()
    outputs = model.read_outputs(config=model.input, output_dir=out)
    assert "prevalence_report" in outputs
    assert outputs["prevalence_report"].height > 0


def test_calibration_produces_posterior(tmp_path, tiny_calibration_config):
    """A small ABC calibration runs the model and writes a posterior to disk."""
    from calibrationtools import CalibrationResults
    from vhf_pipeline.model import CalibrationContext

    out = tmp_path / "cal"
    context = CalibrationContext(tiny_calibration_config, out)
    context.run()

    results_file = out / "calibration" / "calibration_results.pkl"
    assert results_file.exists()
    with open(results_file, "rb") as fp:
        results = pickle.load(fp)
    assert isinstance(results, CalibrationResults)
    assert results.ess >= 1
    assert len(results.posterior_particles.particles) >= 1


def test_pipeline_end_to_end(tmp_path, base_config, monkeypatch):
    """calibrate_onset + symptom_onset_figures run end-to-end for a single scenario."""
    import datetime as dt

    from vhf_pipeline.pipeline import symptom_onset_figures

    data_dir = tmp_path / "data"
    data_dir.mkdir()
    (data_dir / "threshold_data_500.csv").write_text(
        "threshold,threshold_lag,threshold_date\n500.0,0,2026-05-24\n"
    )
    # Two qualifying epiweeks (ISO weeks 25-26: 2026-06-15 to 2026-06-28).
    # threshold_date (2026-05-24) + 21 days = 2026-06-14; both weeks qualify.
    # Two epiweeks are required so that save_particle_detection_rate gets
    # exactly the two target checkpoints it expects.
    rows = "\n".join(f"2026-06-{15 + i},1,Confirmed" for i in range(14))
    (data_dir / "daily_confirmation_incidence.csv").write_text(
        f"date,count,case_status\n{rows}\n"
    )
    out_dir = tmp_path / "out"
    monkeypatch.setenv("DATA_INPUT_DIR", str(data_dir))
    monkeypatch.setenv("OUTPUT_DIR", str(out_dir))

    cfg = base_config
    # Shrink to 2 particles and accept all — this test checks pipeline wiring, not fit.
    cfg["calibration"]["generation_particle_count"] = 2
    cfg["calibration"]["tolerance_values"] = [float("inf")]
    # Trigger symptom_onset_report before the projection ends (~2026-05-16 with
    # max_cases=70000) so that all_symptom_onset_reports.csv is written with data.
    cfg["projection"]["default_ixa_overrides"]["symptom_onset_report"] = {
        "write": True,
        "filename": "symptom_onset_report.csv",
        "period": 1.0,
        "trigger": {"Date": {"date": "2026-02-15"}},
    }
    config_path = tmp_path / "base_config.json"
    config_path.write_text(json.dumps(cfg))

    calibrate_onset.main(
        subdir_name="detection_1.00",
        config_file=str(config_path),
        cache=False,
    )
    symptom_onset_figures.main(
        output_subdir="",
        calibration_subdir="detection_1.00",
        projection_date=dt.date(2026, 9, 30),
        plot_incidence=False,
        save_detection_rate=True,
    )

    products = out_dir / "detection_1.00" / "products"
    assert (products / "prevalence_over_time.csv").exists()
    assert_csv_has_columns(
        products / "prevalence_over_time.csv",
        {"particle_id", "date", "cumulative_infections"},
    )
    figures_dir = products / "figures"
    assert (figures_dir / "weekly_rt.png").exists()
