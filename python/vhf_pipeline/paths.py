"""Filesystem paths resolved from CLI args and the project's ``.env``.

Every resolver returns a concrete :class:`~pathlib.Path` (never ``str | None``),
so call sites share one type and one precedence rule instead of each re-reading
``os.getenv`` and tripping over the ``Path(None)`` mismatch.
"""

import os
from pathlib import Path

from dotenv import load_dotenv

DEFAULT_OUTPUT_DIR = "output"


def output_dir(
    subdir: str | os.PathLike[str] = "", *, override: str | None = None
) -> Path:
    """Resolve the output directory, with ``subdir`` appended.

    Precedence for the base: ``override`` arg, then ``$OUTPUT_DIR`` (from the
    environment or ``.env``), then :data:`DEFAULT_OUTPUT_DIR`.
    """
    load_dotenv()
    base = override or os.getenv("OUTPUT_DIR") or DEFAULT_OUTPUT_DIR
    return Path(base).expanduser() / subdir


def data_input_dir(*, override: str | None = None) -> Path:
    """Resolve the data input directory: ``override`` arg, then ``$DATA_INPUT_DIR``.

    Raises :class:`RuntimeError` if neither is set, rather than failing later
    with a confusing ``TypeError`` from ``Path(None)``.
    """
    load_dotenv()
    value = override or os.getenv("DATA_INPUT_DIR")
    if value is None:
        raise RuntimeError(
            "DATA_INPUT_DIR is not set; add it to your .env or environment."
        )
    return Path(value).expanduser()
