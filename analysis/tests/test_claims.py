import csv
import gzip
from pathlib import Path

from simciv_analysis.claims import extract_claims, ledger_bytes


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
    assert claims[0]["tier"] == "key"
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
