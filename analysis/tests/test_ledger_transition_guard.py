import pytest

from simciv_analysis.ledger_transition_guard import validate_transitions


def _claim(status, evidence="none"):
    return {"claim_key": "M3_report.md#results:0", "status": status, "recovered_evidence": evidence}


def test_unallowlisted_evidence_downgrade_fails():
    with pytest.raises(ValueError, match="evidence weakened"):
        validate_transitions(
            [_claim("recovered-original-output")],
            [_claim("missing-data", "unverifiable")],
            [],
        )


def test_reviewed_unstructured_match_downgrade_is_allowlisted():
    transitions = validate_transitions(
        [_claim("recovered-original-output (formatted)")],
        [_claim("missing-source", "unverifiable")],
        [{
            "from_status": "recovered-original-output (formatted)",
            "to_status": "missing-source",
            "recovered_evidence": "unverifiable",
            "reason": "Unstructured value-only text is not measurement evidence.",
        }],
    )

    assert transitions == [
        ("recovered-original-output (formatted)", "missing-source", "unverifiable")
    ]


def test_primary_verdict_cannot_be_downgraded_even_with_unverifiable_evidence():
    with pytest.raises(ValueError, match="evidence weakened"):
        validate_transitions(
            [_claim("bit-exact")],
            [_claim("missing-data", "unverifiable")],
            [{
                "from_status": "bit-exact",
                "to_status": "missing-data",
                "recovered_evidence": "unverifiable",
                "reason": "This must not override a golden comparison.",
            }],
        )


def test_unclassified_status_transition_fails_closed():
    with pytest.raises(ValueError, match="unclassified status transition"):
        validate_transitions([_claim("new-status-a")], [_claim("new-status-b")], [])
