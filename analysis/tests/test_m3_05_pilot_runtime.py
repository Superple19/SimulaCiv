import math
from pathlib import Path

from simciv_analysis.io import content_matches
from simciv_analysis.m3_05_pilot_runtime import (
    _component_rows,
    _population_values,
    _power_law_coefficients,
    build_summary_outputs,
)


def test_population_parser_handles_thousands_and_suffixes():
    assert _population_values("N in {10,000, 50k, 1M}") == [10_000, 50_000, 1_000_000]


def test_runtime_fit_recovers_power_law():
    intercept, slope = _power_law_coefficients([(10.0, 20.0), (100.0, 200.0), (1000.0, 2000.0)])
    assert abs(slope - 1.0) < 1e-12
    assert abs(math.exp(intercept) - 2.0) < 1e-12


def test_component_breakdown_marks_unrecorded_checkpoint_time():
    records = [{
        "record": "SCALE_SETUP",
        "initialize_us": 1_000_000,
        "graph_us": 2_000_000,
        "projection_us": 500_000,
    }]
    series = [{
        "calibration_estimate_seconds": "3.0",
        "warmup_estimate_seconds": "2.0",
        "timed_samples_seconds": "10.0",
    }]
    rows = _component_rows(records, series)
    by_name = {row["component"]: row for row in rows}
    assert by_name["initialization + graph + projection"]["seconds"] == "3.500000"
    assert by_name["checkpoint/context preparation"]["seconds"] == ""
    assert by_name["known component subtotal"]["seconds"] == "18.500000"


def test_report_counts_are_generated_from_committed_records():
    root = Path(__file__).resolve().parents[2]
    source = (root / "analysis/simciv_analysis/m3_05_pilot_runtime.py").read_text(encoding="utf-8")
    assert not any(text in source for text in (
        "48 series", "16 cells", "100-claim", "162-cell", "8 SCALE_SETUP", "three calibration probes", "two P4 claims",
    ))
    assert all(content_matches(path, expected) for path, expected in build_summary_outputs(root).items())
