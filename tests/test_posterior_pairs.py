"""Unit tests for the posterior pairs diagnostics.

These exercise the statistics and the run-discovery logic only; they build no
model and load no calibration results.
"""

import json
import warnings

import numpy as np
import pytest
from scipy import stats
from vhf_pipeline.plots.posterior_pairs import (
    CALIBRATION_RESULTS,
    discover_arms,
    kish_ess,
    kolmogorov_critical_value,
    label_for,
    resolve_priors_path,
    weighted_ecdf,
    weighted_ks,
)


def test_kolmogorov_critical_value_matches_the_familiar_constant():
    assert kolmogorov_critical_value(0.05) == pytest.approx(1.3581, abs=1e-4)
    assert kolmogorov_critical_value(0.01) > kolmogorov_critical_value(0.05)


def test_weighted_ecdf_spans_zero_to_one():
    x = np.array([1.0, 2.0, 3.0])
    w = np.ones(3)
    assert weighted_ecdf(x, w, np.array([0.5])) == pytest.approx(0.0)
    assert weighted_ecdf(x, w, np.array([3.0])) == pytest.approx(1.0)
    assert weighted_ecdf(x, w, np.array([2.0])) == pytest.approx(2 / 3)


def test_weighted_ecdf_respects_weights():
    x = np.array([1.0, 2.0])
    heavy_on_first = np.array([9.0, 1.0])
    assert weighted_ecdf(x, heavy_on_first, np.array([1.0])) == pytest.approx(0.9)


def test_weighted_ks_reduces_to_the_unweighted_statistic():
    rng = np.random.default_rng(0)
    a, b = rng.normal(size=200), rng.normal(loc=0.6, size=150)
    expected = stats.ks_2samp(a, b).statistic
    got = weighted_ks(a, np.ones(a.size), b, np.ones(b.size))
    assert got == pytest.approx(expected)


def test_weighted_ks_sees_a_shift_that_the_unweighted_statistic_misses():
    """Weighting is the whole point: identical samples, different weights."""
    x = np.linspace(0.0, 1.0, 200)
    flat = np.ones(x.size)
    concentrated = np.where(x < 0.1, 1.0, 1e-6)
    assert weighted_ks(x, flat, x, flat) == pytest.approx(0.0)
    assert weighted_ks(x, flat, x, concentrated) > 0.5


def test_kish_ess_is_the_count_when_weights_are_equal():
    assert kish_ess(np.ones(300)) == pytest.approx(300.0)


def test_kish_ess_collapses_when_one_weight_dominates():
    w = np.concatenate([[1.0], np.full(299, 1e-9)])
    assert kish_ess(w) == pytest.approx(1.0, abs=1e-3)


def test_label_falls_back_to_the_tail_of_an_unknown_prior_key():
    assert label_for("case_fatality_ratio") == "CFR"
    assert label_for("some_new_distribution.Gamma.rate") == "Gamma\nrate"


def _make_run(tmp_path, arms, created_at="2999-01-01T00:00:00+00:00"):
    (tmp_path / "manifest.json").write_text(json.dumps({"created_at": created_at}))
    for arm in arms:
        result = tmp_path / arm / CALIBRATION_RESULTS
        result.parent.mkdir(parents=True)
        result.touch()
    return tmp_path


def test_discover_arms_finds_only_directories_holding_a_calibration(tmp_path):
    run = _make_run(tmp_path, ["detection_0.40", "detection_0.20"])
    (run / "products").mkdir()
    assert discover_arms(run) == ["detection_0.20", "detection_0.40"]


def test_discover_arms_refuses_a_directory_with_no_calibration(tmp_path):
    with pytest.raises(SystemExit):
        discover_arms(tmp_path)


def _write_arm_config(run, arm, priors_path):
    (run / arm / "config.json").write_text(
        json.dumps({"priors_file": str(priors_path)})
    )


def test_resolve_priors_path_reads_the_path_the_arm_recorded(tmp_path):
    run = _make_run(tmp_path, ["arm"])
    priors = tmp_path / "priors.json"
    priors.write_text("{}")
    _write_arm_config(run, "arm", "priors.json")
    assert resolve_priors_path(run, "arm", repo_root=tmp_path) == priors


def test_resolve_priors_path_fails_when_the_priors_are_gone(tmp_path):
    run = _make_run(tmp_path, ["arm"])
    _write_arm_config(run, "arm", "vanished.json")
    with pytest.raises(SystemExit, match="no longer exists"):
        resolve_priors_path(run, "arm", repo_root=tmp_path)


def test_resolve_priors_path_warns_when_the_priors_postdate_the_run(tmp_path):
    """The run records the priors by path, so drift is otherwise silent."""
    run = _make_run(tmp_path, ["arm"], created_at="2020-01-01T00:00:00+00:00")
    priors = tmp_path / "priors.json"
    priors.write_text("{}")
    _write_arm_config(run, "arm", "priors.json")
    with pytest.warns(UserWarning, match="may not be the prior"):
        resolve_priors_path(run, "arm", repo_root=tmp_path)


def test_resolve_priors_path_is_quiet_when_the_priors_predate_the_run(tmp_path):
    run = _make_run(tmp_path, ["arm"])
    priors = tmp_path / "priors.json"
    priors.write_text("{}")
    _write_arm_config(run, "arm", "priors.json")
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        resolve_priors_path(run, "arm", repo_root=tmp_path)
