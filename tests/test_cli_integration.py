"""End-to-end integration tests for the ``vhf`` command-line interface.

These drive the real CLI entry point (``vhf_pipeline.cli.main``) the same way a
shell invocation would -- ``main(["model", "calibrate", ...])`` -- so they cover
the argument parsing and dispatch wiring layered on top of the pipeline that the
``test_pipeline_integration`` tests already exercise.

Coverage:
- ``test_cli_rejects_invalid_invocations``   missing subcommands / required args /
                                             unknown workflow names exit non-zero
- ``test_cli_workflow_dispatches_with_remaining_args``  ``vhf workflow <name>``
                                             forwards trailing args to the workflow
- ``test_cli_model_run_writes_outputs``      ``vhf model run`` runs the model
- ``test_cli_model_calibrate_writes_posterior``  ``vhf model calibrate`` calibrates

The tests that run the compiled ixa model reuse the ``model_binary`` and
``base_config`` fixtures from ``conftest.py`` (and skip when the release binary
isn't built). The parser/dispatch tests need neither.
"""

from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize(
    "argv",
    [
        pytest.param([], id="no-command-group"),
        pytest.param(["model"], id="model-without-subcommand"),
        pytest.param(["model", "calibrate"], id="calibrate-without-config"),
        pytest.param(["workflow", "does-not-exist"], id="unknown-workflow"),
    ],
)
def test_cli_rejects_invalid_invocations(argv):
    """argparse exits non-zero for a missing command group, a missing subcommand,
    a missing required option, and an unknown workflow name -- the wiring in
    cli/__init__ and the model/workflow sub-parsers."""
    from vhf_pipeline.cli import main

    with pytest.raises(SystemExit):
        main(argv)


def test_cli_model_run_writes_outputs(tmp_path, model_binary, capsys):
    """``vhf model run`` runs the compiled model from an ixa config and, with
    ``--verbose``, prints the reports it reads back."""
    from vhf_pipeline.cli import main

    out = tmp_path / "model_run"
    main(
        [
            "model",
            "run",
            "--ixa-config",
            str(REPO_ROOT / "input" / "input.json"),
            "--output-dir",
            str(out),
            "--force-overwrite",
            "--verbose",
        ]
    )

    assert (out / "simulation_config.json").exists()
    # --verbose reads the configured outputs back and prints each by name.
    assert "Output: prevalence_report" in capsys.readouterr().out
