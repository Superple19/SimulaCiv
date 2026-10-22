from simciv_analysis.report_m5_04 import _cell_gate, _timing_source


def test_a5_gate_classification_uses_spread_then_bootstrap_bound():
    assert _cell_gate(-0.1, 0.2, 1.0, (0.01, 0.02)) == "PASS"
    assert _cell_gate(1.1, 1.2, 1.0, (0.01, 0.02)) == "FAIL"
    assert _cell_gate(0.9, 1.1, 1.0, (0.01, 0.02)) == "INCONCLUSIVE"
    assert _cell_gate(0.0, 0.0, 1.0, (0.0501, 0.01)) == "INVALID"


def test_reported_timing_sources_are_committed():
    harness, data = _timing_source("M4-02")
    assert harness.endswith("m4_02_a5_measure.rs")
    assert data.endswith("m4_02/m4_02_a5.log")
