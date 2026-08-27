"""Unit + integration tests for the detection-band posterior cache.

CalibrationCache unit tests verify the cache API directly.
Integration tests verify that CalibrationContext.run() correctly uses the cache
to skip expensive calibration runs when inputs are unchanged.
"""

import json
import pickle
import types
from pathlib import Path

from vhf_pipeline import CalibrationContext
from vhf_pipeline.model.calibration_cache import CalibrationCache
from vhf_pipeline.pipeline import calibrate_onset as ct


def _make_config(tmp_path: Path, **overrides) -> dict:
    priors = tmp_path / "priors.json"
    priors.write_text('{"priors": {}}')
    target = tmp_path / "target.csv"
    target.write_text("threshold,count\n50,1\n")
    default_ixa = tmp_path / "default_params.json"
    default_ixa.write_text('{"seed": 0}')
    exe = tmp_path / "vhf_model"
    exe.write_bytes(b"BINARY-v1")

    config = {
        "strategy": "detection_band",
        "priors_file": str(priors),
        "target_data_file": str(target),  # absolute -> no DATA_INPUT_DIR needed
        "default_ixa_file": str(default_ixa),
        "exe_file": str(exe),
        "force_overwrite": True,
        "calibration": {"generation_particle_count": 2, "entropy": 123},
        "projection": {"clean": False},
    }
    config.update(overrides)
    return config


def test_key_is_stable(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    config = _make_config(tmp_path)
    assert cache.key_for_config(config) == cache.key_for_config(config)


def test_key_changes_with_calibration_block(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    a = _make_config(tmp_path)
    b = _make_config(tmp_path)
    b["calibration"]["entropy"] = 999
    assert cache.key_for_config(a) != cache.key_for_config(b)


def test_key_insensitive_to_projection_and_output(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    a = _make_config(tmp_path)
    b = _make_config(tmp_path)
    b["projection"] = {"clean": True, "default_ixa_overrides": {"anything": 1}}
    b["force_overwrite"] = False
    assert cache.key_for_config(a) == cache.key_for_config(b)


def test_key_changes_with_top_level_default_ixa_overrides(tmp_path):
    # set_defaults applies the experiment-wide default_ixa_overrides to the
    # calibration model, so it must be part of the key.
    cache = CalibrationCache(root=tmp_path / "cache")
    none = _make_config(tmp_path)
    a = _make_config(tmp_path, default_ixa_overrides={"case_fatality_ratio": 0.1})
    b = _make_config(tmp_path, default_ixa_overrides={"case_fatality_ratio": 0.9})
    assert cache.key_for_config(a) != cache.key_for_config(b)
    assert cache.key_for_config(a) != cache.key_for_config(none)


def test_key_sensitive_to_data_files_but_not_binary(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    config = _make_config(tmp_path)
    before = cache.key_for_config(config)
    # the binary is out of the key (checked at hit time via meta), so a recompile
    # keeps the key stable and the entry findable
    Path(config["exe_file"]).write_bytes(b"BINARY-v2")
    assert cache.key_for_config(config) == before
    # priors/target/default-params contents ARE determinants
    Path(config["priors_file"]).write_text('{"priors": {"x": 1}}')
    assert cache.key_for_config(config) != before


def test_binary_hash_and_meta_roundtrip(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    config = _make_config(tmp_path)
    h1 = cache.binary_hash(config)
    Path(config["exe_file"]).write_bytes(b"BINARY-v2")
    assert cache.binary_hash(config) != h1

    src = tmp_path / "r.pkl"
    with open(src, "wb") as fp:
        pickle.dump({"a": 1}, fp)
    cache.store("k", src, meta={"binary_sha": "abc"})
    assert cache.load_meta("k")["binary_sha"] == "abc"
    assert cache.load_meta("missing") == {}


def test_missing_file_does_not_crash_key(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    config = _make_config(tmp_path, exe_file=str(tmp_path / "nope"))
    # Stable across calls even though the binary is absent.
    assert cache.key_for_config(config) == cache.key_for_config(config)


def test_store_load_restore_roundtrip(tmp_path):
    cache = CalibrationCache(root=tmp_path / "cache")
    src = tmp_path / "calibration_results.pkl"
    with open(src, "wb") as fp:
        pickle.dump({"posterior": [1, 2, 3]}, fp)

    assert cache.load("k") is None
    dest = cache.store("k", src, meta={"threshold": 50})
    assert cache.load("k") == dest
    with open(dest, "rb") as fp:
        assert pickle.load(fp) == {"posterior": [1, 2, 3]}
    assert json.loads((dest.parent / "meta.json").read_text())["threshold"] == 50

    restored = tmp_path / "out" / "calibration" / "calibration_results.pkl"
    assert cache.restore("k", restored) is True
    with open(restored, "rb") as fp:
        assert pickle.load(fp) == {"posterior": [1, 2, 3]}
    assert cache.restore("missing", restored) is False


class _MockProjectionContext:
    """No-op ProjectionContext used by the cache integration tests.

    calibrate_onset.main() calls ProjectionContext after calibration; this stub
    lets the tests exercise only the caching layer without needing a real binary.
    """

    def __init__(self, config, output_dir):
        pass

    def get_target_data(self):
        return None

    def update_ixa_default(self, param_name, param_value):
        pass

    def run(self, particles):
        pass

    def save(self, process_outputs=True):
        pass


class _MockCalibrationContext(CalibrationContext):
    """Mock CalibrationContext that simulates caching behavior.

    This mock reproduces the caching logic from CalibrationContext.run()
    without requiring full initialization of BranchingProcessContext.
    Tracks how many times actual calibration runs occur.
    """

    runs = 0  # Class variable to track total calibration runs

    def __init__(self, config, output_dir):
        self.config = config
        self.output_dir = Path(output_dir)
        self.mode = "calibration"
        self.results = None
        # Required attributes used by parent methods
        self.verbose = True
        self.force_overwrite = config.get("force_overwrite", False)
        self.target_data_file = None
        self.priors = None

    def run(
        self,
        cache: bool = True,
        cache_dir: str | None = None,
        refresh_cache: bool = False,
        reuse_across_binary: bool = False,
    ):
        """Simulate CalibrationContext.run() with caching logic."""
        calibration_cache, cache_key, cached_pkl = self._load_cache(
            cache=cache,
            cache_dir=cache_dir,
            refresh_cache=refresh_cache,
            reuse_across_binary=reuse_across_binary,
        )

        if cached_pkl is not None:
            # Cache hit: restore without running
            calibration_results_file = (
                self.output_dir / self.mode / "calibration_results.pkl"
            )
            calibration_results_file.parent.mkdir(parents=True, exist_ok=True)
            import shutil

            shutil.copyfile(cached_pkl, calibration_results_file)
            with open(calibration_results_file, "rb") as fp:
                self.results = pickle.load(fp)
        else:
            self._run_calibration(calibration_cache, cache_key)

    def _run_calibration(self, calibration_cache=None, cache_key=None):
        """Run calibration and save result (with caching if enabled)."""
        type(self).runs += 1

        # Write mock calibration results
        calibration_results_file = (
            self.output_dir / self.mode / "calibration_results.pkl"
        )
        calibration_results_file.parent.mkdir(parents=True, exist_ok=True)

        # Create mock results — include posterior_particles so calibrate_onset.main()
        # can access it when running the (mocked) projection step.
        self.results = types.SimpleNamespace(
            posterior_particles=None,
            run_number=type(self).runs,
        )

        with open(calibration_results_file, "wb") as fp:
            pickle.dump(self.results, fp)

        # Store in cache if caching is enabled
        if calibration_cache is not None:
            calibration_cache.store(
                cache_key,
                calibration_results_file,
                meta={
                    "parent_dir": self.output_dir,
                    "binary_sha": calibration_cache.binary_hash(self.config),
                },
            )


def test_calibrate_cache_hit_skips_run(tmp_path, monkeypatch):
    """Test that cache hit in CalibrationContext.run() skips calibration run."""
    _MockCalibrationContext.runs = 0
    monkeypatch.setattr(ct, "CalibrationContext", _MockCalibrationContext)
    monkeypatch.setattr(ct, "ProjectionContext", _MockProjectionContext)
    monkeypatch.setenv("OUTPUT_DIR", str(tmp_path / "out"))
    monkeypatch.setenv("DATA_INPUT_DIR", str(tmp_path / "data"))
    (tmp_path / "data").mkdir()
    (tmp_path / "data" / "threshold_data_500.csv").write_text(
        "threshold,count\n500,1\n"
    )

    config = _make_config(tmp_path)
    config_file = tmp_path / "config.json"
    config_file.write_text(json.dumps(config))

    # First run: cache miss -> calibration runs and populates the cache.
    ct.main(
        output_dir="run1",
        config_file=str(config_file),
        cache=True,
    )
    assert _MockCalibrationContext.runs == 1

    # Second run in a fresh output dir: cache hit -> no new calibration, posterior restored.
    ct.main(
        output_dir="run2",
        config_file=str(config_file),
        cache=True,
    )
    assert _MockCalibrationContext.runs == 1
    restored = tmp_path / "out" / "run2" / "calibration" / "calibration_results.pkl"
    assert restored.exists()

    # refresh_cache forces a re-run even on a hit.
    ct.main(
        output_dir="run3",
        config_file=str(config_file),
        cache=True,
        refresh_cache=True,
    )
    assert _MockCalibrationContext.runs == 2


def test_calibrate_no_cache_always_runs(tmp_path, monkeypatch):
    """Test that without caching, calibration always runs."""
    _MockCalibrationContext.runs = 0
    monkeypatch.setattr(ct, "CalibrationContext", _MockCalibrationContext)
    monkeypatch.setattr(ct, "ProjectionContext", _MockProjectionContext)
    monkeypatch.setenv("OUTPUT_DIR", str(tmp_path / "out"))
    monkeypatch.setenv("DATA_INPUT_DIR", str(tmp_path / "data"))
    (tmp_path / "data").mkdir()
    (tmp_path / "data" / "threshold_data_50.csv").write_text("threshold,count\n50,1\n")

    config_file = tmp_path / "config.json"
    config_file.write_text(json.dumps(_make_config(tmp_path)))

    for i in range(2):
        ct.main(
            output_dir=f"run{i}",
            config_file=str(config_file),
            cache=False,
        )
    assert _MockCalibrationContext.runs == 2


def test_cache_binary_change_misses_unless_reuse_flag(tmp_path, monkeypatch):
    """A cached entry whose binary hash differs is a miss by default (re-run),
    but reused with reuse_across_binary=True."""
    _MockCalibrationContext.runs = 0
    monkeypatch.setattr(ct, "CalibrationContext", _MockCalibrationContext)
    monkeypatch.setattr(ct, "ProjectionContext", _MockProjectionContext)
    monkeypatch.setenv("OUTPUT_DIR", str(tmp_path / "out"))
    monkeypatch.setenv("DATA_INPUT_DIR", str(tmp_path / "data"))
    (tmp_path / "data").mkdir()
    (tmp_path / "data" / "threshold_data_50.csv").write_text("threshold,count\n50,1\n")

    config = _make_config(tmp_path)
    config_file = tmp_path / "config.json"
    config_file.write_text(json.dumps(config))
    exe = Path(config["exe_file"])

    # Populate the cache (records the binary hash in meta)
    ct.main(
        output_dir="r1",
        config_file=str(config_file),
        cache=True,
    )
    assert _MockCalibrationContext.runs == 1

    # Recompile -> binary differs -> default is a miss (re-run)
    exe.write_bytes(b"BINARY-v2")
    ct.main(
        output_dir="r2",
        config_file=str(config_file),
        cache=True,
    )
    assert _MockCalibrationContext.runs == 2

    # Recompile again -> reuse flag reuses the entry despite the mismatch
    exe.write_bytes(b"BINARY-v3")
    ct.main(
        output_dir="r3",
        config_file=str(config_file),
        cache=True,
        reuse_across_binary=True,
    )
    assert _MockCalibrationContext.runs == 2  # no new run
