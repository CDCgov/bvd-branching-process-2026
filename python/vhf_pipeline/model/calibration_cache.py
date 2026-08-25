"""Content-hash cache for the deaths-threshold calibration posterior.

The stage-1 (deaths-threshold) calibration is the expensive, deterministic part
of the sitrep workflows: ``etr-sitrep`` runs it only to seed a spillover prior
for the later symptom-onset calibration. Caching its posterior lets repeat runs
skip the ABC entirely when nothing that determines the posterior changed.

The key hashes what determines the posterior: ``strategy``, the ``calibration``
block, the experiment-wide top-level ``default_ixa_overrides`` (``set_defaults``
merges it into the calibration model), and the *contents* of the priors file,
target-data file, and default-ixa file. It excludes the ``projection`` block,
``force_overwrite``, and output paths. The key also carries a cache-format
version and the installed ``calibrationtools`` version, so a change to either
(the pickle is a ``calibrationtools.CalibrationResults``) is a miss rather than
an unpicklable hit.

The **model binary is deliberately out of the key** and recorded in the entry's
``meta.json`` instead, so a rebuilt binary still *finds* the same entry. The
caller compares the stored binary hash to the current one at hit time and, by
default, treats a mismatch as a miss (safe: a model change can change the
posterior). Passing ``reuse_across_binary`` downgrades a mismatch to a warning
and reuses the posterior anyway, for rebuilds the caller knows are
calibration-neutral.
"""

import hashlib
import json
import os
import shutil
import tempfile
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any

from .. import paths

CACHE_DIRNAME = ".calibration_cache"
RESULTS_NAME = "calibration_results.pkl"
CACHE_VERSION = 1  # bump when the cached pickle's format assumptions change
_MISSING_MARKER = b"<missing>"


def _hash_file(path: Path) -> str:
    """SHA-256 of a file's contents; a stable marker if it does not exist.

    A missing input never crashes key computation: the subsequent calibration
    run (or its absence) surfaces the real error instead.
    """
    h = hashlib.sha256()
    try:
        with open(path, "rb") as fp:
            for chunk in iter(lambda: fp.read(1 << 20), b""):
                h.update(chunk)
    except (FileNotFoundError, IsADirectoryError):
        h.update(_MISSING_MARKER)
    return h.hexdigest()


def _calibrationtools_version() -> str:
    try:
        return version("calibrationtools")
    except PackageNotFoundError:
        return "unknown"


def _resolve_target(target_data_file: str | dict[str, Path]) -> set[Path]:
    """Resolve the target CSV the way :class:`BranchingProcessContext` does.

    A relative name is joined onto ``$DATA_INPUT_DIR``; an absolute path is used
    as-is (which also keeps the cache testable without the env var set).
    """

    if isinstance(target_data_file, dict):
        return {paths.data_input_dir() / Path(v) for v in target_data_file.values()}
    p = Path(target_data_file)
    return {p} if p.is_absolute() else {paths.data_input_dir() / p}


class CalibrationCache:
    """Content-addressed store of ``calibration_results.pkl`` keyed on inputs."""

    def __init__(self, root: str | Path | None = None):
        self.root = (
            Path(root) if root is not None else paths.output_dir() / CACHE_DIRNAME
        )

    def key_for_config(self, config: dict[str, Any]) -> str:
        """Hash of the inputs that determine the deaths-threshold posterior.

        The model binary is intentionally excluded (see :meth:`binary_hash`); it
        lives in the entry meta and is checked at hit time.
        """
        target = config.get("target_data_file")
        payload = {
            "cache_version": CACHE_VERSION,
            "calibrationtools": _calibrationtools_version(),
            "strategy": config.get("strategy"),
            "calibration": config.get("calibration"),
            "default_ixa_overrides": config.get("default_ixa_overrides"),
            "priors": _hash_file(Path(config["priors_file"]))
            if config.get("priors_file")
            else None,
            "target_data": {str(p): _hash_file(p) for p in _resolve_target(target)}
            if target
            else None,
            "default_ixa": _hash_file(Path(config["default_ixa_file"]))
            if config.get("default_ixa_file")
            else None,
        }
        blob = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
        return hashlib.sha256(blob).hexdigest()

    def binary_hash(self, config: dict[str, Any]) -> str | None:
        """SHA-256 of the model binary (``exe_file``), or ``None`` if unset."""
        exe = config.get("exe_file")
        return _hash_file(Path(exe)) if exe else None

    def _entry_dir(self, key: str) -> Path:
        return self.root / key

    def load(self, key: str) -> Path | None:
        """Path to the cached posterior pickle, or ``None`` on a miss."""
        pkl = self._entry_dir(key) / RESULTS_NAME
        return pkl if pkl.exists() else None

    def load_meta(self, key: str) -> dict[str, Any]:
        """The entry's ``meta.json`` contents, or ``{}`` if absent/unreadable."""
        meta_path = self._entry_dir(key) / "meta.json"
        try:
            with open(meta_path, "r") as fp:
                return json.load(fp)
        except (FileNotFoundError, json.JSONDecodeError):
            return {}

    def store(
        self, key: str, results_pkl: Path, meta: dict[str, Any] | None = None
    ) -> Path:
        """Copy ``results_pkl`` into the cache under ``key`` and return its path.

        The pickle lands via a temp-file-then-rename so an interrupted or
        disk-full store never leaves a truncated file that a later run would
        treat as a valid hit.
        """
        entry = self._entry_dir(key)
        entry.mkdir(parents=True, exist_ok=True)
        dest = entry / RESULTS_NAME
        fd, tmp = tempfile.mkstemp(dir=entry, suffix=".pkl.tmp")
        os.close(fd)
        try:
            shutil.copyfile(results_pkl, tmp)
            os.replace(tmp, dest)
        finally:
            if os.path.exists(tmp):
                os.remove(tmp)
        with open(entry / "meta.json", "w") as fp:
            json.dump(meta or {}, fp, indent=2, default=str)
        return dest

    def restore(self, key: str, dest_pkl: Path) -> bool:
        """Copy a cached posterior to ``dest_pkl``; ``False`` if not cached."""
        cached = self.load(key)
        if cached is None:
            return False
        dest_pkl.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(cached, dest_pkl)
        return True
