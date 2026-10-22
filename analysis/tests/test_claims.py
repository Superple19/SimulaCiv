import csv
import gzip
from pathlib import Path

from simciv_analysis.claims import _tier, extract_claims, ledger_bytes, scan_documents


def test_claims_exclude_inputs_match_exact_metadata_and_pack_one_row_each(tmp_path):
    report = tmp_path / "docs" / "reports" / "sample.md"
    report.parent.mkdir(parents=True)
    report.write_text(
        "# Sample\n\n## Findings\n\n"
        "| Workload | N | Median (µs) |\n|---|---:|---:|\n| baseline | 10,000 | 1,234.5 |\n",
        encoding="utf-8",
    )
    old = tmp_path / "old.csv"
    with old.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow([
            "claim_id", "report", "section anchor", "claim text (short)", "value", "unit", "class",
            "source harness", "raw data", "status", "M5 stage", "notes",
        ])
        writer.writerow(["1", "sample.md", "#findings", "L7: | baseline | 10,000 | 1,234.5 |", "1234.5", "µs", "timing", "bench.rs", "data.csv", "reproducible-now", "M5-04", "exact"])

    claims = extract_claims(tmp_path, old)
    assert len(claims) == 1
    assert claims[0]["value"] == "1234.5"
    assert claims[0]["source harness"] == "bench.rs"
    assert claims[0]["tier"] == "headline"
    first = ledger_bytes(claims)
    second = ledger_bytes(claims)
    assert first == second
    assert gzip.decompress(first).count(b"\n") == 2


def test_unmatched_provenance_is_unmapped(tmp_path):
    report = tmp_path / "docs" / "reports" / "sample.md"
    report.parent.mkdir(parents=True)
    report.write_text("# Sample\n\n## Findings\n\n- A committed result is 42.\n", encoding="utf-8")
    old = tmp_path / "old.csv"
    old.write_text(
        "claim_id,report,section anchor,claim text (short),value,unit,class,source harness,raw data,status,M5 stage,notes\n",
        encoding="utf-8",
    )
    claims = extract_claims(tmp_path, old)
    assert len(claims) == 1
    assert claims[0]["status"] == "unmapped"
    assert claims[0]["source harness"] == "unmapped"
    assert "not inferred" in claims[0]["notes"]


def test_conflicting_old_metadata_is_unmapped(tmp_path):
    report = tmp_path / "docs" / "reports" / "sample.md"
    report.parent.mkdir(parents=True)
    report.write_text("# Sample\n\n## Findings\n\n- Result is 42.\n", encoding="utf-8")
    old = tmp_path / "old.csv"
    with old.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow([
            "claim_id", "report", "section anchor", "claim text (short)", "value", "unit", "class",
            "source harness", "raw data", "status", "M5 stage", "notes",
        ])
        writer.writerow(["1", "sample.md", "#findings", "L5: - Result is 42", "42", "", "derived", "none", "none", "reproducible-now", "M5-04", "first"])
        writer.writerow(["2", "sample.md", "#findings", "L5: - Result is 42", "42", "", "derived", "none", "none", "missing-data", "M5-04", "second"])

    claim = extract_claims(tmp_path, old)[0]
    assert claim["status"] == "unmapped"
    assert "ambiguous" in claim["notes"]


def test_headline_tier_excludes_stage_scope_and_full_gate_tables():
    assert _tier("M5 stage plan", "M5-03 implementation COMPLETE") == "supporting"
    assert _tier("Validation record", "8 analysis tests passed") == "key"
    assert _tier("Audit appendix", "population count 42") == "supporting"
    assert _tier("Audit appendix", "| final | 42 |", table_row=True) == "headline"
    assert _tier("Gate matrix", "| case | PASS |", table_row=True) == "key"
    assert _tier("Summary", "The speedup was 2.1x.") == "headline"
    assert _tier("document", "Status: complete after 42 runs") == "headline"
    assert _tier("Summary", "The metric was 42.", contract=True) == "supporting"
    assert _tier("Results", "gate verdict: PASS") == "key"
    assert _tier("Status", "Status: complete (42)") == "headline"


def test_mapping_uses_renamed_report_and_source_when_line_moves(tmp_path):
    report = tmp_path / "docs" / "reports" / "renamed.md"
    report.parent.mkdir(parents=True)
    report.write_text(
        "# Report\n\nStatus: current\n\n## Findings\n\n| Workload | N | Median (µs) |\n|---|---:|---:|\n| baseline | 10,000 | 1,234.5 |\n",
        encoding="utf-8",
    )
    (report.parent / "RENAMES.md").write_text(
        "| Old path | New path |\n|---|---|\n| `docs/performance/old.md` | `docs/reports/renamed.md` |\n",
        encoding="utf-8",
    )
    old = tmp_path / "old.csv"
    with old.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow([
            "claim_id", "report", "section anchor", "claim text (short)", "value", "unit", "class",
            "source harness", "raw data", "status", "M5 stage", "notes",
        ])
        writer.writerow(["1", "old.md", "#findings", "L7: | baseline | 10,000 | 1,234.5 |", "1234.5", "µs", "timing", "bench.rs", "data.csv", "reproducible-now", "M5-04", "exact"])

    claim = next(item for item in extract_claims(tmp_path, old) if item["value"] == "1234.5")
    assert claim["source harness"] == "bench.rs"
    assert claim["status"] == "reproducible-now"
    assert "line used only as a tie-breaker" in claim["notes"]


def test_m1_aggregate_run_is_historical_log_not_gap(tmp_path):
    report = tmp_path / "docs" / "reports" / "M1_CONTRACT_FREEZE.md"
    report.parent.mkdir(parents=True)
    report.write_text("# M1\n\n## Findings\n\n- Historical run recorded 42 passing tests.\n", encoding="utf-8")
    old = tmp_path / "old.csv"
    old.write_text(
        "claim_id,report,section anchor,claim text (short),value,unit,class,source harness,raw data,status,M5 stage,notes\n"
        '"1","M1_CONTRACT_FREEZE.md","#findings","L5: - Historical run recorded 42 passing tests.","42","tests","deterministic","none","none","unrecoverable","none","historic"\n',
        encoding="utf-8",
    )
    claim = extract_claims(tmp_path, old)[0]
    assert claim["status"] == "historical-log"
    assert "not a regeneration target" in claim["notes"]


def test_contract_tables_outside_measured_sections_are_not_claims(tmp_path):
    contract = tmp_path / "docs" / "contracts" / "sample.md"
    contract.parent.mkdir(parents=True)
    contract.write_text(
        "# Contract\n\n## Decision register\n\n| ID | Rule |\n|---|---|\n| D-1 | schema version 2 |\n\n"
        "## Current reproducibility state\n\n| Artifact | Bytes |\n|---|---:|\n| sample | 42 |\n",
        encoding="utf-8",
    )
    old = tmp_path / "old.csv"
    old.write_text(
        "claim_id,report,section anchor,claim text (short),value,unit,class,source harness,raw data,status,M5 stage,notes\n",
        encoding="utf-8",
    )
    claims = extract_claims(tmp_path, old)
    assert [claim["value"] for claim in claims] == ["42"]
    assert claims[0]["section anchor"] == "current-reproducibility-state"


def test_contract_validation_status_is_measured_evidence(tmp_path):
    contract = tmp_path / "docs" / "contracts" / "sample.md"
    contract.parent.mkdir(parents=True)
    contract.write_text("# Contract\n\nValidation Status: Complete (42 tests passed).\n", encoding="utf-8")
    claims = scan_documents(tmp_path)
    assert [claim["value"] for claim in claims] == ["42"]
