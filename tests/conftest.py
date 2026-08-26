"""Shared fixtures for the vhf_pipeline integration tests.

These tests drive the real compiled ixa model (``target/release/vhf_model``), so
they need the release binary built. CI builds it before pytest; locally, build
with ``cargo build --release``. A test skips with a clear message if it's missing.
"""

import os

# Matplotlib must not reach for a GUI backend in headless CI. Set this before any
# pyplot import happens in the pipeline modules the tests exercise.
os.environ.setdefault("MPLBACKEND", "Agg")

import json
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
MODEL_BINARY = REPO_ROOT / "target" / "release" / "vhf_model"
EXPERIMENT_DIR = REPO_ROOT / "experiments" / "detection"


@pytest.fixture(scope="session")
def model_binary() -> Path:
    """The compiled ixa model, or skip the test if it hasn't been built."""
    if not MODEL_BINARY.exists():
        pytest.skip(
            f"Model binary {MODEL_BINARY} not found; run `cargo build --release`."
        )
    return MODEL_BINARY


@pytest.fixture
def base_config(model_binary) -> dict:
    """A fresh bvd_early_phase threshold config with absolute data-file paths."""
    cfg = json.loads((EXPERIMENT_DIR / "test_config.json").read_text())
    cfg["default_ixa_file"] = str(EXPERIMENT_DIR / "default_ixa_config.json")
    cfg["priors_file"] = str(EXPERIMENT_DIR / "priors.json")
    return cfg


@pytest.fixture
def tiny_calibration_config(base_config, tmp_path, monkeypatch) -> dict:
    """``base_config`` shrunk to a 2-particle ABC run against a low (2-death)
    threshold nearly every short sim clears, with ``DATA_INPUT_DIR`` pointed at a
    matching one-row CSV. For tests exercising the calibrate machinery, not fit."""
    data_dir = tmp_path / "data"
    data_dir.mkdir()
    (data_dir / "thresh.csv").write_text(
        "threshold,threshold_lag,threshold_date\n2.0,0,2026-05-24\n"
    )
    # One full epiweek (ISO week 25: 2026-06-15 to 2026-06-21).
    # threshold_date (2026-05-24) + 21 days = 2026-06-14, so this week qualifies.
    incidence_rows = "\n".join(f"2026-06-{15 + i},1,Confirmed" for i in range(7))
    (data_dir / "incidence.csv").write_text(
        f"date,count,case_status\n{incidence_rows}\n"
    )
    monkeypatch.setenv("DATA_INPUT_DIR", str(data_dir))

    cfg = base_config
    cfg["target_data_file"]["deaths_threshold"] = "thresh.csv"
    cfg["target_data_file"]["confirmation"] = "incidence.csv"
    cfg["default_ixa_overrides"] = {
        "initialization": {
            "start_date": "2026-03-15",
            "initial_cases": {
                "SpilloverEvent": {"days_since_start": 0.0, "exposures": 20}
            },
        }
    }
    cfg["calibration"]["generation_particle_count"] = 2
    cfg["calibration"]["tolerance_values"] = [float("inf")]  # accept all particles
    cfg["calibration"]["default_ixa_overrides"]["end_run_conditions"] = {
        "max_time": 500.0,
        "max_deaths": 100,
        "max_cases": 1000,
    }
    return cfg
