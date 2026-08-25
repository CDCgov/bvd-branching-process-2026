"""Filesystem paths resolved from CLI args and the project's ``.env``.

Every resolver returns a concrete :class:`~pathlib.Path` (never ``str | None``),
so call sites share one type and one precedence rule instead of each re-reading
``os.getenv`` and tripping over the ``Path(None)`` mismatch.
"""

import json
import os
import urllib
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


def _get_rid(dataset_name: str) -> str:
    """Get the Foundry dataset RID for a given dataset name.

    This is a private helper function that reads the `rid_tags.json` file to
    retrieve the RID associated with the provided dataset name.

    Args:
        dataset_name (str): The name of the dataset for which to retrieve the RID.
    """
    if dataset_name.startswith("ri.foundry.main.dataset."):
        return dataset_name  # Already a RID, no need to look it up
    rid_tags_path = Path("data") / "rid_tags.json"
    if not rid_tags_path.exists():
        raise FileNotFoundError(f"RID tags file not found: {rid_tags_path}")

    with open(rid_tags_path, "r") as f:
        rid_tags = json.load(f)

    if dataset_name not in rid_tags:
        raise KeyError(f"Dataset name '{dataset_name}' not found in RID tags.")

    return rid_tags[dataset_name]


def load_foundry_rid(override: str | None = None) -> tuple[str, str]:
    load_dotenv()

    def _load_rid(rid):
        return urllib.parse.quote(rid, safe="")

    host = os.getenv("FOUNDRY_HOST")
    if override is not None:
        rid = _load_rid(_get_rid(override))
    else:
        dataset = os.getenv("FOUNDRY_DATASET")
        if dataset is None:
            raise RuntimeError(
                "FOUNDRY_DATASET is not set; add it to your .env or environment or supply an in-line override."
            )
        rid = _load_rid(_get_rid(dataset))
    return host, rid


def load_foundry_token() -> str:
    load_dotenv()
    return os.getenv("FOUNDRY_TOKEN")
