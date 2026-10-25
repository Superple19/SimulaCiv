from simciv_analysis.report_m5_04 import (
    _cell_gate,
    _old_new_m4_04_comparison,
    _m4_05_differences,
    _timing_source,
)
from simciv_analysis.claims import apply_m5_04_overrides
from simciv_analysis.claims import extract_claims, ledger_bytes
from simciv_analysis.report_m5_04 import CLAIMS_PATH, _outputs


def test_a5_gate_classification_uses_spread_then_bootstrap_bound():
    assert _cell_gate(-0.1, 0.2, 1.0, (0.01, 0.02)) == "PASS"
    assert _cell_gate(1.1, 1.2, 1.0, (0.01, 0.02)) == "FAIL"
    assert _cell_gate(0.9, 1.1, 1.0, (0.01, 0.02)) == "INCONCLUSIVE"
    assert _cell_gate(0.0, 0.0, 1.0, (0.0501, 0.01)) == "INVALID"


def test_reported_timing_sources_are_committed():
    harness, data = _timing_source("M4-02")
    assert harness.endswith("m4_02_a5_measure.rs")
    assert data.endswith("m4_02/m4_02_a5.log")


def test_representation_only_m4_differences_are_normalized_and_listed_separately():
    m4_04 = _old_new_m4_04_comparison()
    m4_05_differences, m4_05_normalizations = _m4_05_differences()
    assert m4_04["mismatch_count"] == 0
    assert m4_04["exact_fields"] == 836
    assert m4_04["normalized_fields"] == 4
    assert m4_04["compared_fields"] == 840
    assert len(m4_04["normalizations"]) == 4
    assert all(row["harness"] and row["raw data"] for row in m4_04["normalizations"])
    assert m4_05_differences == []
    assert len(m4_05_normalizations) == 8
    assert all(row["harness"] and row["raw data"] for row in m4_05_normalizations)


def test_m5_04_ledger_override_is_idempotent():
    claims = [{"claim_id": 7, "status": "unverifiable", "notes": "source note; M5-04: old"}]
    overrides = {"claims": {"7": {"result": "bit-exact", "ledger_status": "bit-exact", "note": "exact replay"}}}

    once = apply_m5_04_overrides([dict(claims[0])], overrides=overrides)
    twice = apply_m5_04_overrides([dict(once[0])], overrides=overrides)

    assert once == twice


def test_m5_04_ledger_output_matches_the_canonical_claim_generator():
    assert _outputs()[CLAIMS_PATH] == ledger_bytes(extract_claims())
