import pytest

from simciv_analysis.findings import Finding, render_findings


def test_findings_render_one_check_row_per_tagged_bullet() -> None:
    bullets, checks = render_findings([
        Finding("[exploratory]", "A checked result.", "finding_result_matches", True),
        Finding("[pre-registered: m5_05_gate]", "A gated result.", "finding_gate_passes", True),
    ])

    assert bullets.count("\n- ") == 1
    assert checks.count("| [exploratory] |") == 1
    assert checks.count("| [pre-registered: m5_05_gate] |") == 1
    assert checks.count("`true`") == 2


def test_failed_paired_check_rejects_report_generation() -> None:
    with pytest.raises(ValueError, match="finding_gate_passes"):
        render_findings([Finding("[exploratory]", "A false result.", "finding_gate_passes", False)])


def test_findings_require_supported_tags_and_unique_predicates() -> None:
    with pytest.raises(ValueError, match="invalid finding tag"):
        render_findings([Finding("[unreviewed]", "Text.", "finding_text", True)])
    with pytest.raises(ValueError, match="duplicate finding predicate"):
        render_findings([
            Finding("[exploratory]", "One.", "finding_same", True),
            Finding("[exploratory]", "Two.", "finding_same", True),
        ])
