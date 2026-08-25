"""The base class shared by the named workflows.

Kept import-light (only stdlib) so the CLI can look up and dispatch a workflow
without pulling in any workflow's heavy plotting deps. Each workflow owns its own
argument parsing: it defines a typed args object, a parser for it, and a
``run()`` that takes that typed object.

Every run also writes a provenance manifest (see :mod:`vhf_pipeline.provenance`)
before it executes, recording the code, environment, and inputs it ran with. A
workflow opts in by overriding :meth:`Workflow.manifest_dir`.
"""

import argparse
import dataclasses
from abc import ABC, abstractmethod
from collections.abc import Sequence
from pathlib import Path
from typing import Generic, TypeVar

ArgsT = TypeVar("ArgsT")


def _params(args: object) -> dict:
    """The workflow's typed args as a plain dict for the manifest."""
    if dataclasses.is_dataclass(args) and not isinstance(args, type):
        return dataclasses.asdict(args)
    return dict(vars(args))


class Workflow(ABC, Generic[ArgsT]):
    """A named, end-to-end workflow invoked as ``vhf workflow <name>``.

    Each workflow module defines a subclass and exposes a single instance as a
    module-level ``WORKFLOW`` attribute, which the CLI dispatches to via
    :meth:`main`.
    """

    name: str

    def parser(self) -> argparse.ArgumentParser:
        """A parser pre-named for this workflow; subclasses add their arguments."""
        return argparse.ArgumentParser(prog=f"vhf workflow {self.name}")

    @abstractmethod
    def parse_args(self, argv: list[str]) -> ArgsT:
        """Parse this workflow's CLI arguments into its typed args object."""

    @abstractmethod
    def run(self, args: ArgsT) -> None:
        """Execute the workflow with its typed arguments."""

    def manifest_dir(self, args: ArgsT) -> Path | None:
        """Directory the run's provenance manifest is written to.

        Return the run's output directory to record provenance, or ``None`` (the
        default) to skip it.
        """
        return None

    def manifest_inputs(self, args: ArgsT) -> Sequence[tuple[str, Path]]:
        """Input files to hash into the manifest, as ``(label, path)`` pairs."""
        return ()

    def main(self, argv: list[str]) -> None:
        """Parse ``argv``, record provenance, then run — the CLI entry point."""
        args = self.parse_args(argv)
        self._write_manifest(args)
        self.run(args)

    def _write_manifest(self, args: ArgsT) -> None:
        manifest_dir = self.manifest_dir(args)
        if manifest_dir is None:
            return
        from .. import provenance

        provenance.write_run_manifest(
            manifest_dir,
            command=f"workflow {self.name}",
            params=_params(args),
            input_files=self.manifest_inputs(args),
        )
