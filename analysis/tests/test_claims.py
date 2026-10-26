import csv
import gzip
import json
from pathlib import Path

from simciv_analysis.claims import _tier, _tokens, apply_m5_04_overrides, extract_claims, ledger_bytes, locate_claim, scan_documents
from simciv_analysis.io import content_matches, write_text_lf


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
    assert gzip.decompress(first) == gzip.decompress(second)
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


def test_m5_04_status_overrides_are_deterministic_and_preserve_prior_notes():
    claims = [{"claim_id": 7, "status": "restored-not-rerun", "notes": "source retained"}]
    overrides = {"claims": {"7": {"result": "bit-exact", "note": "compared against committed data"}}}
    result = apply_m5_04_overrides(claims, overrides=overrides)
    assert result[0]["status"] == "bit-exact"
    assert result[0]["notes"] == "source retained; M5-04: compared against committed data"


def test_gate_amendment_labels_are_not_numeric_claims():
    assert list(_tokens("A-5 gate, 20% threshold")) == [("20", "%")]


def test_composite_table_claims_record_their_component_position(tmp_path):
    report = tmp_path / "docs" / "reports" / "sample.md"
    report.parent.mkdir(parents=True)
    report.write_text(
        "# Sample\n\n## Findings\n\n| N | Frontier sizes |\n|---:|---|\n| 10 | [3, 2] |\n",
        encoding="utf-8",
    )

    claims = scan_documents(tmp_path)
    frontier_claims = [claim for claim in claims if claim["value"] in {"3", "2"}]

    assert [claim["value"] for claim in frontier_claims] == ["3", "2"]
    assert [json.loads(claim["context"])["component_index"] for claim in frontier_claims] == [0, 1]


def test_contract_claim_identity_is_stable_when_lines_are_inserted_above_it(tmp_path):
    contract = tmp_path / "docs" / "contracts" / "sample.md"
    contract.parent.mkdir(parents=True)
    original = "# Sample\n\n## Current reproducibility state\n\nObserved value: 42.\n"
    write_text_lf(contract, original)
    old = tmp_path / "old.csv"
    write_text_lf(
        old,
        "claim_id,report,section anchor,claim text (short),value,unit,class,source harness,raw data,status,M5 stage,notes\n",
    )
    before_bytes = ledger_bytes(extract_claims(tmp_path, old))
    committed_ledger = tmp_path / "docs" / "reproducibility" / "claims.csv.gz"
    committed_ledger.parent.mkdir(parents=True)
    committed_ledger.write_bytes(before_bytes)
    before = scan_documents(tmp_path)

    write_text_lf(contract, "A non-claim introduction.\n" + original)
    after_bytes = ledger_bytes(extract_claims(tmp_path, old))
    after = scan_documents(tmp_path)

    assert len(before) == len(after) == 1
    assert before[0]["claim_key"] == after[0]["claim_key"]
    assert after_bytes == committed_ledger.read_bytes() == before_bytes
    assert content_matches(committed_ledger, after_bytes)
    assert {key: value for key, value in before[0].items() if key != "line"} == {key: value for key, value in after[0].items() if key != "line"}
    assert after[0]["line"] == before[0]["line"] + 1


def test_claims_locate_resolves_current_line_from_stable_key(tmp_path):
    contract = tmp_path / "docs" / "contracts" / "sample.md"
    contract.parent.mkdir(parents=True)
    write_text_lf(contract, "# Sample\n\n## Current reproducibility state\n\nObserved value: 42.\n")
    claim = scan_documents(tmp_path)[0]

    located = locate_claim(str(claim["claim_key"]), tmp_path)

    assert located is not None
    assert located["line"] == claim["line"]
    assert located["section anchor"] == claim["section anchor"]


def test_m5_04_result_overrides_recovered_evidence_for_its_claims(tmp_path, monkeypatch):
    import simciv_analysis.claims as claims_module

    report = tmp_path / "docs/reproducibility/M4_sample.md"
    report.parent.mkdir(parents=True)
    write_text_lf(tmp_path / "docs/reproducibility/sources.toml", "")
    overrides = tmp_path / "docs/data/m5_04/claim_statuses.json"
    overrides.parent.mkdir(parents=True)
    write_text_lf(
        overrides,
        json.dumps({"claims": {"1": {"result": "bit-exact", "ledger_status": "bit-exact", "note": "verified"}}}),
    )
    recovered = tmp_path / "docs/reproducibility/recovered_claim_statuses.json"
    write_text_lf(
        recovered,
        json.dumps({"claims": [{
            "claim_id": 1,
            "status_before": "missing-data",
            "status_after": "unverifiable",
            "report": "M4_sample.md",
        }]}),
    )
    monkeypatch.setattr(claims_module, "scan_documents", lambda root: [{
        "report": "M4_sample.md", "section anchor": "result", "class": "derived",
        "claim text (short)": "Recorded result 42", "value": "42", "unit": "",
        "claim_key": "M4_sample.md#result:0",
    }])
    monkeypatch.setattr(claims_module, "load_sources", lambda root: {})
    monkeypatch.setattr(claims_module, "validate_source_paths", lambda sources, root: None)
    monkeypatch.setattr(
        claims_module,
        "source_for_claim",
        lambda *args: {"status": "missing-data", "harness": [], "tests": [], "data": [], "regeneration_stage": "M5-04", "notes": ""},
    )

    result = claims_module.extract_claims(tmp_path)[0]

    assert result["status"] == "bit-exact"


def test_m3_05_pilot_verdict_survives_stale_recovered_status_before(tmp_path):
    from simciv_analysis.claims import M3_05_PILOT_RESULTS, apply_m3_05_pilot_results

    write_text_lf(
        tmp_path / M3_05_PILOT_RESULTS,
        json.dumps({
            "claims": [
                {
                    "claim_key": "M3_report.md#first-break:0",
                    "status_before": "recovered-original-output",
                    "verdict": "inconclusive (validity)",
                    "note": "pilot path did not pass validity",
                },
                {
                    "claim_key": "M3_report.md#first-break:1",
                    "status_before": "recovered-original-output",
                    "verdict": "reproduced",
                    "note": "pilot result",
                },
            ]
        }),
    )
    claims = [
        {"claim_key": "M3_report.md#first-break:0", "status": "missing-data", "notes": ""},
        {"claim_key": "M3_report.md#first-break:1", "status": "conclusion holds", "notes": ""},
    ]

    result = apply_m3_05_pilot_results(claims, tmp_path)

    assert result[0]["status"] == "inconclusive (validity)"
    assert result[1]["status"] == "conclusion holds"


def test_source_line_insert_keeps_committed_ledger_and_report_bytes_unchanged():
    from simciv_analysis.claims import OUTPUT, ROOT, main as claims_main
    from simciv_analysis.report_m5_01 import MD_PATH, QMD_PATH, check_outputs

    contract = ROOT / "docs" / "contracts" / "M1_CONTRACT_FREEZE.md"
    original_contract = contract.read_bytes()
    original_ledger = OUTPUT.read_bytes()
    original_qmd = QMD_PATH.read_bytes()
    original_report = MD_PATH.read_bytes()
    try:
        contract.write_bytes(b"<!-- temporary line stability test -->\n" + original_contract)
        assert claims_main(["--check"]) == 0
        assert OUTPUT.read_bytes() == original_ledger
        assert check_outputs()
        assert QMD_PATH.read_bytes() == original_qmd
        assert MD_PATH.read_bytes() == original_report
    finally:
        contract.write_bytes(original_contract)
    assert contract.read_bytes() == original_contract
