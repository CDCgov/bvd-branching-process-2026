"""Named, end-to-end workflows that compose the pipeline stages.

Each workflow module defines a :class:`~vhf_pipeline.workflows.base.Workflow`
subclass and exposes it as a module-level ``WORKFLOW`` instance. ``WORKFLOWS``
maps the CLI name to its module so ``vhf workflow <name>`` can dispatch without
importing every workflow (and its heavy plotting deps) up front.
"""

from .base import Workflow

WORKFLOWS = {"detection": "detection_range"}

__all__ = ["WORKFLOWS", "Workflow"]
