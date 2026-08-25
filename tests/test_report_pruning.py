"""Unit tests for calibration-time report pruning (the speed win)."""
from vhf_pipeline.model.context import disable_unused_reports
from vhf_pipeline.model.data_processors import NaturalHistoryProcessor

class TestProcessor(NaturalHistoryProcessor):
    required_reports = (
        "prevalence_report",
    )
    def get_target_data():
        pass
    def estimate_error():
        pass
    def process_outputs():
        pass

def test_processors_declare_required_reports():
    assert TestProcessor().required_reports == ("prevalence_report",)

def test_disable_unused_reports_turns_off_only_unneeded_written_reports():
    params = {
        "seed": 1,
        "case_fatality_ratio": 0.5,
        "prevalence_report": {"write": True, "filename": "p.csv", "period": 1.0},
        "symptom_onset_report": {"write": True, "filename": "s.csv", "period": 1.0},
        "confirmation_incidence_report": {"write": False, "filename": "c.csv"},
    }
    disable_unused_reports(params, ("symptom_onset_report",))

    assert params["symptom_onset_report"]["write"] is True  # required, kept
    assert params["prevalence_report"]["write"] is False  # unused, disabled
    assert params["confirmation_incidence_report"]["write"] is False  # already off
    assert params["seed"] == 1  # non-report params untouched
    assert params["case_fatality_ratio"] == 0.5


def test_disable_unused_reports_keeps_all_required():
    params = {
        "prevalence_report": {"write": True, "filename": "p.csv"},
        "symptom_onset_report": {"write": True, "filename": "s.csv"},
    }
    disable_unused_reports(params, ("prevalence_report", "symptom_onset_report"))
    assert params["prevalence_report"]["write"] is True
    assert params["symptom_onset_report"]["write"] is True


def test_disable_unused_reports_ignores_report_without_filename():
    # a write:true entry that isn't a real report (no filename) is left alone
    params = {"weird": {"write": True}}
    disable_unused_reports(params, ())
    assert params["weird"]["write"] is True
