"""Bindings to the ixa VHF model: the runner, contexts, and output handlers.

This is the single seam between Python and the Rust model. Today ``VHFModel``
shells out to the compiled ``target/release/vhf_model`` binary; a future in-process
pyo3 extension would slot in here without touching the pipeline or workflow code.
"""

from .context import (
    BranchingProcessContext,
    CalibrationContext,
    ProjectionContext,
)
from .data_handler import NaturalHistoryHandler
from .runner import VHFModel

__all__ = [
    "VHFModel",
    "NaturalHistoryHandler",
    "BranchingProcessContext",
    "CalibrationContext",
    "ProjectionContext",
]
