"""Tests for run provenance: the manifest module and the workflow wiring.

None of these run the compiled ixa model -- they exercise the provenance module
directly and the :class:`~vhf_pipeline.workflows.base.Workflow` hook, stubbing the
heavy pipeline stages so ``detection`` writes its manifest without doing a real run.
"""

import hashlib
import json
from dataclasses import dataclass

from vhf_pipeline import provenance
from vhf_pipeline.workflows.base import Workflow

# --- the provenance module -------------------------------------------------


def test_build_manifest_has_core_fields():
    manifest = provenance.build_manifest(command="workflow test", params={"a": 1})

    assert manifest["schema_version"] == provenance.SCHEMA_VERSION
    assert manifest["command"] == "workflow test"
    assert manifest["params"] == {"a": 1}
    assert manifest["created_at"].endswith("+00:00")
    # versions always carry python + the package itself
    assert manifest["versions"]["python"]
    assert "vhf_pipeline" in manifest["versions"]
    assert set(manifest["git"]) == {"commit", "branch", "dirty"}


def test_git_info_resolves_inside_the_repo():
    # The test suite runs inside this project's git checkout, so git resolves.
    git = provenance.build_manifest(command="x", params={})["git"]

    assert git["commit"] and len(git["commit"]) == 40
    assert git["branch"]
    assert isinstance(git["dirty"], bool)


def test_input_files_are_hashed(tmp_path):
    present = tmp_path / "config.json"
    present.write_bytes(b'{"k": 1}')
    missing = tmp_path / "nope.json"

    manifest = provenance.build_manifest(
        command="x",
        params={},
        input_files=[("config_file", present), ("griddle", missing)],
    )

    assert manifest["inputs"]["config_file"] == {
        "path": str(present),
        "exists": True,
        "sha256": hashlib.sha256(b'{"k": 1}').hexdigest(),
    }
    assert manifest["inputs"]["griddle"]["exists"] is False
    assert manifest["inputs"]["griddle"]["sha256"] is None


def test_write_run_manifest_creates_dir_and_round_trips(tmp_path):
    out = tmp_path / "run"  # does not exist yet
    path = provenance.write_run_manifest(
        out, command="workflow detection", params={"threshold_upper_bound": 100}
    )

    assert path == out / "manifest.json"
    loaded = json.loads(path.read_text())
    assert loaded["command"] == "workflow detection"
    assert loaded["params"]["threshold_upper_bound"] == 100


# --- the Workflow.main provenance hook -------------------------------------


@dataclass(frozen=True)
class _FakeArgs:
    output_subdir: str = ""
    n: int = 3


class _ManifestWorkflow(Workflow[_FakeArgs]):
    name = "fake"

    def __init__(self, run_dir):
        self._run_dir = run_dir
        self.ran_with = None

    def parse_args(self, argv):
        return _FakeArgs()

    def run(self, args):
        self.ran_with = args

    def manifest_dir(self, args):
        return self._run_dir


class _NoManifestWorkflow(Workflow[_FakeArgs]):
    name = "plain"

    def parse_args(self, argv):
        return _FakeArgs()

    def run(self, args):
        self.ran = True


def test_main_writes_manifest_then_runs(tmp_path):
    wf = _ManifestWorkflow(tmp_path / "run")

    wf.main([])

    assert wf.ran_with == _FakeArgs()
    manifest = json.loads((tmp_path / "run" / "manifest.json").read_text())
    assert manifest["command"] == "workflow fake"
    assert manifest["params"] == {"output_subdir": "", "n": 3}


def test_main_skips_manifest_when_dir_is_none(tmp_path):
    wf = _NoManifestWorkflow()

    wf.main([])

    assert wf.ran is True
    assert wf.manifest_dir(_FakeArgs()) is None
    assert list(tmp_path.iterdir()) == []


# --- the real detection workflow, with the heavy stages stubbed out -------------


def test_detection_workflow_writes_manifest(tmp_path, monkeypatch):
    import vhf_pipeline.workflows.detection_range as dr

    # Stub run() directly so we only test the manifest-writing hook; the
    # real run() needs a valid config file with all required keys.
    monkeypatch.setattr(dr.WORKFLOW, "run", lambda args: None)

    monkeypatch.setenv("OUTPUT_DIR", str(tmp_path / "out"))
    config = tmp_path / "config.json"
    config.write_text("{}")
    scenarios = tmp_path / "all_confirmed_scenario.json"
    scenarios.write_text("[]")

    dr.WORKFLOW.main(
        [
            "-o",
            "sub",
            "-c",
            str(config),
            "--scenarios",
            str(scenarios),
        ]
    )

    manifest = json.loads((tmp_path / "out" / "sub" / "manifest.json").read_text())
    assert manifest["command"] == "workflow detection"
    assert manifest["params"]["main_config"] == str(config)
    assert (
        manifest["inputs"]["config_file"]["sha256"] == hashlib.sha256(b"{}").hexdigest()
    )
    assert manifest["inputs"]["scenarios"]["exists"] is True
