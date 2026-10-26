import json

import pytest

from simciv_analysis.claims import apply_m3_05_pilot_results
from simciv_analysis.m3_05_pilot_results import (
    _any_phase_witnesses,
    _first_alpha_breaks,
    _historical_per_agent_crossings,
    _primary_claim_ids,
    RESULTS_JSON,
    ratio_verdict,
)


def test_frozen_ratio_verdict_bands_are_inclusive_at_ten_and_twenty_percent():
    assert ratio_verdict(1.0, 1.1) == ("reproduced", pytest.approx(0.1))
    assert ratio_verdict(1.0, 1.2) == ("reproduced (loose)", pytest.approx(0.2))
    assert ratio_verdict(1.0, 1.2001)[0] == "not reproduced"


def test_pilot_results_apply_by_stable_claim_key(tmp_path):
    path = tmp_path / "docs" / "reproducibility" / "M3_05_PILOT_RESULTS.json"
    path.parent.mkdir(parents=True)
    path.write_text(
        json.dumps(
            {
                "claims": [
                    {
                        "claim_key": "report.md#section:1",
                        "status_before": "missing-data",
                        "verdict": "inconclusive (validity)",
                        "note": "source phase path exceeded the validity limit",
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    claims = [
        {"claim_key": "report.md#section:1", "status": "missing-data", "notes": "source map"},
        {"claim_key": "report.md#section:2", "status": "missing-data", "notes": "source map"},
    ]

    updated = apply_m3_05_pilot_results(claims, tmp_path)

    assert updated[0]["status"] == "inconclusive (validity)"
    assert "M3-05 pilot: source phase path exceeded the validity limit" in updated[0]["notes"]
    assert updated[1]["status"] == "missing-data"


def test_pilot_result_map_does_not_overwrite_a_changed_base_status(tmp_path):
    path = tmp_path / "docs" / "reproducibility" / "M3_05_PILOT_RESULTS.json"
    path.parent.mkdir(parents=True)
    path.write_text(
        json.dumps(
            {
                "claims": [
                    {
                        "claim_key": "report.md#section:1",
                        "status_before": "missing-data",
                        "verdict": "reproduced",
                        "note": "pilot completed",
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    claim = {"claim_key": "report.md#section:1", "status": "recovered-original-output", "notes": "historical evidence"}
    assert apply_m3_05_pilot_results([claim], tmp_path)[0]["status"] == "recovered-original-output"


def test_os_working_set_method_can_override_derived_class(tmp_path):
    path = tmp_path / "docs" / "reproducibility" / "M3_05_PILOT_RESULTS.json"
    path.parent.mkdir(parents=True)
    path.write_text(
        json.dumps({"claims": [{
            "claim_key": "report.md#section:1",
            "status_before": "missing-data",
            "verdict": "reproduced",
            "class_override": "timing",
            "class_reason": "OS-reported environmental WorkingSetSize",
            "note": "within the timing-style band",
        }]}),
        encoding="utf-8",
    )
    claim = {"claim_key": "report.md#section:1", "status": "missing-data", "class": "derived", "notes": "source map"}

    updated = apply_m3_05_pilot_results([claim], tmp_path)[0]

    assert updated["status"] == "reproduced"
    assert updated["class"] == "timing"
    assert "class derived→timing" in updated["notes"]


def test_any_phase_gate_uses_a_valid_witness_when_another_phase_is_invalid():
    indexes = {
        "alpha": {
            (8, 0, 100_000, 250_000, "P1_regrowth"): {"alpha": 0.7},
            (8, 0, 100_000, 250_000, "P3_features"): {"alpha": 1.2},
        },
        "phase": {
            (100_000, 8, 0, "P1_regrowth"): {"median_us": 100.0, "mad_us": 10.0},
            (250_000, 8, 0, "P1_regrowth"): {"median_us": 100.0, "mad_us": 10.0},
            (100_000, 8, 0, "P3_features"): {"median_us": 100.0, "mad_us": 1.0},
            (250_000, 8, 0, "P3_features"): {"median_us": 200.0, "mad_us": 1.0},
        },
        "stage": {},
        "full_tick": {},
    }

    assert _any_phase_witnesses(8, 0, 100_000, 250_000, indexes) == [("P3_features", 1.2)]


def test_historical_per_agent_gate_keeps_secondary_endpoint_from_detail_cell():
    assert _historical_per_agent_crossings()[(8, 0)] == 250_000
    assert _historical_per_agent_crossings()[(16, 50)] == 500_000


def test_first_alpha_break_reads_the_current_alpha_index_key_order():
    historical = {(8, 0, "P3_features", 100_000, 250_000): 1.2}
    indexes = {
        "alpha": {(8, 0, 100_000, 250_000, "P3_features"): {"alpha": 1.3}},
        "phase": {
            (100_000, 8, 0, "P3_features"): {"median_us": 100.0, "mad_us": 1.0},
            (250_000, 8, 0, "P3_features"): {"median_us": 200.0, "mad_us": 1.0},
        },
        "stage": {},
        "full_tick": {},
    }

    row = _first_alpha_breaks(indexes, historical)[0]

    assert row["historical_value"].startswith("100000→250000")
    assert row["pilot_value"].startswith("100000→250000")
    assert row["old_gate_side"] == row["pilot_gate_side"] == "crossed"


def test_invalid_scaling_claims_without_checkpoint_context_are_supplemental_validity():
    payload = json.loads(RESULTS_JSON.read_text(encoding="utf-8"))

    rows = [
        row
        for row in payload["claims"]
        if row["scope"] == "supplemental validity impact (scaling exponents)"
    ]

    assert {row["claim_id"] for row in rows} == {
        20805, 20806, 20807, 20877, 20878, 20879,
        20949, 20950, 20951, 21021, 21022, 21023,
        20830, 20831, 20902, 20903, 20973, 20974, 21045, 21046,
        20854, 20855, 20926, 20927, 20985, 21057,
    }
    assert all(row["tier"] == "supporting" and row["verdict"] == "inconclusive (validity)" for row in rows)


def test_primary_claim_selection_does_not_depend_on_reconstruction_inventory():
    claims = [
        {"claim_id": 1, "report": "M3_05_SCALE_LIMIT.md", "section anchor": "first-break-findings", "status": "missing-data"},
        {"claim_id": 2, "report": "M3_05_SCALE_LIMIT.md", "section anchor": "first-break-findings", "status": "recovered-original-output"},
        {"claim_id": 3, "report": "M3_04_SUSTAINED_WORKLOAD.md", "section anchor": "first-break-findings", "status": "missing-data"},
        {"claim_id": 4, "report": "M3_05_SCALE_LIMIT.md", "section anchor": "other-section", "status": "missing-source"},
        {"claim_id": 5, "report": "M3_05_SCALE_LIMIT.md", "section anchor": "first-break-findings", "status": "missing-source"},
    ]

    assert _primary_claim_ids(claims) == {"1", "5"}


def test_frozen_primary_claim_keys_survive_recovered_status_changes():
    claims = [
        {
            "claim_id": 1,
            "claim_key": "M3_05_SCALE_LIMIT.md#first-break-findings:0",
            "report": "M3_05_SCALE_LIMIT.md",
            "section anchor": "first-break-findings",
            "status": "recovered-original-output",
        },
        {
            "claim_id": 2,
            "claim_key": "M3_05_SCALE_LIMIT.md#first-break-findings:1",
            "report": "M3_05_SCALE_LIMIT.md",
            "section anchor": "first-break-findings",
            "status": "missing-data",
        },
    ]

    assert _primary_claim_ids(claims, {"M3_05_SCALE_LIMIT.md#first-break-findings:0"}) == {"1"}
