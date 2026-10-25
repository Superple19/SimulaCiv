import json

import pytest

from simciv_analysis.claims import apply_m3_05_pilot_results
from simciv_analysis.m3_05_pilot_results import ratio_verdict


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
