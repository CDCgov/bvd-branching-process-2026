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
EXPERIMENT_DIR = REPO_ROOT / "experiments" / "bvd_early_phase"


@pytest.fixture(scope="session")
def model_binary() -> Path:
    """The compiled ixa model, or skip the test if it hasn't been built."""
    if not MODEL_BINARY.exists():
        pytest.skip(
            f"Model binary {MODEL_BINARY} not found; run `cargo build --release`."
        )
    return MODEL_BINARY
