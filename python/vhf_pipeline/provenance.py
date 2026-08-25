"""Run provenance: a small JSON manifest describing how a run was produced.

A workflow writes one ``manifest.json`` at the root of its output directory so a
result can later be traced back to the code, environment, and inputs that made
it. Deliberately stdlib-only (``git`` via subprocess, versions via
``importlib.metadata``) so the import stays cheap and the manifest never depends
on the heavy modelling/plotting stack it describes.
"""

import hashlib
import importlib.metadata
import json
import platform
import subprocess
from collections.abc import Mapping, Sequence
from datetime import datetime, timezone
from pathlib import Path

SCHEMA_VERSION = 1

MANIFEST_FILENAME = "manifest.json"

# Distributions whose versions pin a run. Names are distribution names (as in
# pyproject), not import names -- e.g. ``cfa-mrp`` is imported as ``mrp``.
_TRACKED_DISTRIBUTIONS = (
    "vhf_pipeline",
    "cfa-mrp",
    "calibrationtools",
    "griddler",
    "numpy",
    "scipy",
    "polars",
    "matplotlib",
)

# Run git against the package's own checkout, not the process cwd, so the
# captured commit is the code that ran regardless of where ``vhf`` was invoked.
_REPO_ANCHOR = Path(__file__).resolve().parent


def build_manifest(
    *,
    command: str,
    params: Mapping[str, object],
    input_files: Sequence[tuple[str, Path]] = (),
) -> dict:
    """Assemble the provenance manifest for a run.

    Args:
        command: How the run was invoked, e.g. ``"workflow mmwr"``.
        params: The resolved arguments the run was given.
        input_files: ``(label, path)`` pairs of input files to hash, e.g. the
            config and griddle, so the manifest pins their exact contents.

    Returns:
        A JSON-serialisable manifest dict.
    """
    return {
        "schema_version": SCHEMA_VERSION,
        "command": command,
        "created_at": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "params": dict(params),
        "git": _git_info(),
        "versions": _versions(),
        "inputs": {label: _file_digest(path) for label, path in input_files},
    }


def write_run_manifest(
    output_dir: Path,
    *,
    command: str,
    params: Mapping[str, object],
    input_files: Sequence[tuple[str, Path]] = (),
) -> Path:
    """Write the manifest to ``output_dir/manifest.json`` and return its path.

    Creates ``output_dir`` if needed. See :func:`build_manifest` for the args.
    """
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest = build_manifest(command=command, params=params, input_files=input_files)
    path = output_dir / MANIFEST_FILENAME
    with open(path, "w", encoding="utf-8") as fp:
        json.dump(manifest, fp, indent=2, default=str)
        fp.write("\n")
    return path


def _git(*args: str) -> str | None:
    """Run ``git`` in the package checkout; ``None`` if git or the repo is absent."""
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=_REPO_ANCHOR,
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return result.stdout.strip()


def _git_info() -> dict:
    status = _git("status", "--porcelain")
    return {
        "commit": _git("rev-parse", "HEAD"),
        "branch": _git("rev-parse", "--abbrev-ref", "HEAD"),
        # An uncommitted working tree means the commit alone doesn't pin the code.
        "dirty": None if status is None else bool(status),
    }


def _versions() -> dict:
    versions: dict[str, str | None] = {"python": platform.python_version()}
    for dist in _TRACKED_DISTRIBUTIONS:
        try:
            versions[dist] = importlib.metadata.version(dist)
        except importlib.metadata.PackageNotFoundError:
            versions[dist] = None
    return versions


def _file_digest(path: Path) -> dict:
    path = Path(path)
    info: dict[str, object] = {
        "path": str(path),
        "exists": path.is_file(),
        "sha256": None,
    }
    if info["exists"]:
        with open(path, "rb") as fp:
            info["sha256"] = hashlib.file_digest(fp, "sha256").hexdigest()
    return info
