import gzip
import hashlib
import io
import json

from simciv_analysis.io import write_text_lf
from simciv_analysis.recovered_claims import apply_recorded_statuses, build_mapping


def test_recovered_output_matches_exact_result_column_and_skips_inputs(tmp_path):
    stage = tmp_path / "docs" / "data" / "m5_recovered"
    recovered = stage / "recovered"
    recovered.mkdir(parents=True)
    output = b"N,median_us_tick,seed\n10000,1.250,7\n20000,1.300,7\n10000,1.400,7\n"
    (recovered / "out.csv").write_bytes(output)
    write_text_lf(
        stage / "MANIFEST.json",
        json.dumps(
            {
                "items": [
                    {
                        "path": "recovered/out.csv",
                        "type": "raw-output-copy",
                        "reports_supported": ["sample.md"],
                        "size_bytes": len(output),
                        "sha256": hashlib.sha256(output).hexdigest(),
                    }
                ]
            }
        ),
    )
    claims = [
        {
            "claim_id": 1, "report": "sample.md", "value": "1.25", "unit": "µs", "tier": "headline",
            "status": "missing-data",
            "context": json.dumps({"kind": "table", "column": "median_us_tick", "subfield": "median", "dimensions": {"population_n": "10000", "seed": "7"}}),
        },
        {
            "claim_id": 2, "report": "sample.md", "value": "10000", "unit": "", "tier": "supporting",
            "status": "missing-data",
            "context": json.dumps({"kind": "table", "column": "N", "column_is_parameter": True, "dimensions": {}}),
        },
        {
            "claim_id": 3, "report": "sample.md", "value": "1.25", "unit": "µs", "tier": "supporting",
            "status": "missing-data",
            "context": json.dumps({"kind": "table", "column": "median_us_tick", "subfield": "median", "dimensions": {"population_n": "20000", "seed": "7"}}),
        },
        {
            "claim_id": 4, "report": "sample.md", "value": "1.25", "unit": "µs", "tier": "supporting",
            "status": "missing-data",
            "context": json.dumps({"kind": "table", "column": "other_metric", "subfield": "", "dimensions": {"population_n": "10000", "seed": "7"}}),
        },
        {
            "claim_id": 5, "report": "sample.md", "value": "1.25", "unit": "µs", "tier": "supporting",
            "status": "missing-data",
            "context": json.dumps({"kind": "table", "column": "median_us_tick", "subfield": "median", "dimensions": {"population_n": "10000"}}),
        },
        {
            "claim_id": 6, "report": "sample.md", "value": "1.30", "unit": "µs", "tier": "supporting",
            "status": "missing-data",
            "context": json.dumps({"kind": "table", "column": "median_us_tick", "subfield": "median", "dimensions": {"population_n": "20000", "seed": "7"}}),
        },
    ]

    mapping = build_mapping(claims, tmp_path)

    assert mapping["outputs_scanned"] == 1
    assert [row["claim_id"] for row in mapping["claims"]] == [1, 3, 4, 5, 6]
    assert mapping["claims"][0]["status_after"] == "recovered-other-run"
    assert mapping["claims"][0]["triage_cause"] == "different run"
    assert mapping["claims"][1]["status_after"] == "recovered-output-mismatch"
    assert mapping["claims"][2]["status_after"] == "unverifiable"
    assert mapping["claims"][3]["status_after"] == "unverifiable"
    assert mapping["claims"][4]["status_after"] == "recovered-original-output"
    assert mapping["confirmed_mismatches"][0]["expected"] == "1.25"
    assert mapping["confirmed_mismatches"][0]["actual"] == "1.400"
    assert mapping["unmatched_claim_ids"] == []
    assert mapping["unverifiable_claim_ids"] == [4, 5]
    assert mapping["config_value_claim_ids"] == [2]


def test_recorded_recovered_status_changes_only_context_matched_missing_claims(tmp_path):
    report = tmp_path / "docs" / "reproducibility" / "recovered_claim_statuses.json.gz"
    report.parent.mkdir(parents=True)
    data = json.dumps(
        {
            "claims": [
                {"claim_id": 1, "report": "sample.md", "status_before": "missing-data", "status_after": "recovered-original-output"},
                {"claim_id": 2, "report": "sample.md", "status_before": "bit-exact", "status_after": "bit-exact"},
                {"claim_id": 3, "report": "sample.md", "status_before": "missing-data", "status_after": "recovered-output-mismatch", "mismatches": [{"expected": "1.25", "actual": "1.3", "path": "recovered/out.csv", "line": 2}]},
            ],
            "report_outputs": {"sample.md": ["recovered/out.csv"]},
        }
    ).encode("utf-8")
    compressed = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=compressed, mtime=0) as stream:
        stream.write(data)
    report.write_bytes(compressed.getvalue())
    claims = [
        {"claim_id": 1, "status": "missing-data", "notes": "old source note"},
        {"claim_id": 2, "status": "bit-exact", "notes": "golden test"},
        {"claim_id": 3, "status": "missing-data", "notes": "old source note"},
    ]

    result = apply_recorded_statuses(claims, tmp_path)

    assert result[0]["status"] == "recovered-original-output"
    assert "recovered/out.csv" in result[0]["notes"]
    assert result[1] == claims[1]
    assert result[2]["status"] == "recovered-output-mismatch"
    assert "expected 1.25 but output recorded 1.3" in result[2]["notes"]
