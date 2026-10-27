from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from simciv_analysis.conclusions import (
    _protocol_errors,
    _protocol_for_evidence,
    _evidence_errors,
    anchor_section,
    load_evidence,
    validate_anchor_content,
    validate_evidence_append_only,
    validate_artifact_hashes,
    validate_hosts,
)
from simciv_analysis.io import write_text_lf


def test_anchor_section_includes_nested_headings_and_verbatim_is_new():
    report = "# Report\n## Result\nParent line\n### Detail\nExact value 1.25\n## Next\nOther\n"
    parent = "# Report\n## Result\nParent line\n## Next\nOther\n"
    assert "Exact value 1.25" in anchor_section(report, "result")
    assert validate_anchor_content(report, parent, "result", "Exact value 1.25") == []
    assert validate_anchor_content(report, report, "result", "Exact value 1.25") == [
        "verbatim already exists in the report revision's parent"
    ]


def test_anchor_check_rejects_missing_anchor_and_verbatim():
    assert validate_anchor_content("# A\nbody\n", None, "absent", "number") == [
        "anchor not found: absent",
        "verbatim not found in report revision",
    ]


def test_evidence_append_only_rejects_edit_and_removal():
    original = b'{"id":"one"}\n{"id":"two"}\n'
    validate_evidence_append_only(original, original + b'{"id":"three"}\n')
    with pytest.raises(ValueError, match="rewrites or removes"):
        validate_evidence_append_only(original, b'{"id":"one"}\n{"id":"changed"}\n')
    with pytest.raises(ValueError, match="rewrites or removes"):
        validate_evidence_append_only(original, b'{"id":"one"}\n')


def test_evidence_append_only_requires_line_boundary():
    with pytest.raises(ValueError, match="record boundary"):
        validate_evidence_append_only(b'{"id":"one"}', b'{"id":"one"}\n')


def test_superseded_evidence_uses_its_dated_protocol_snapshot():
    experiment = {
        "ordered_cell_ids": ["m3-noop-H3"],
        "harness_paths": ["current.rs"],
        "protocol_history": [{
            "date": "2026-10-27",
            "superseded_evidence_ids": ["old-report-run"],
            "prior_ordered_cell_ids": ["m3-noop-H30"],
            "prior_harness_paths": ["old.rs"],
            "prior_harness_overlays": [{"path": "overlay.rs", "sha256": "a" * 64, "origin_commit": "0" * 40}],
        }],
    }
    old = _protocol_for_evidence(experiment, "old-report-run")
    current = _protocol_for_evidence(experiment, "new-report-run")
    assert old["ordered_cell_ids"] == ["m3-noop-H30"]
    assert old["harness_paths"] == ["old.rs"]
    assert old["harness_overlays"][0]["path"] == "overlay.rs"
    assert current["ordered_cell_ids"] == ["m3-noop-H3"]
    assert current["harness_paths"] == ["current.rs"]


def test_artifact_hash_check_detects_mismatch_and_unhashed_record(tmp_path: Path):
    artifact = tmp_path / "output.json"
    artifact.write_bytes(b'{"value":1}\n')
    record = {
        "id": "ev-1",
        "artifacts": [{"path": "output.json", "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest()}],
    }
    assert validate_artifact_hashes(record, tmp_path) == []
    artifact.write_bytes(b'{"value":2}\n')
    assert "SHA-256 mismatch" in validate_artifact_hashes(record, tmp_path)[0]
    assert "needs at least one hashed artifact" in validate_artifact_hashes({"id": "no-hash"}, tmp_path)[0]


def test_artifact_hash_check_accepts_unmaterialized_dvc_output_only_with_blob_manifest(tmp_path: Path, monkeypatch):
    import simciv_analysis.conclusions as conclusions

    digest = hashlib.sha256(b"raw output\n").hexdigest()
    artifact_path = "docs/data/stage/raw.csv"
    manifest = tmp_path / "docs/data/SHA256SUMS"
    manifest.parent.mkdir(parents=True)
    write_text_lf(manifest, f"{digest}  {artifact_path}\n")
    monkeypatch.setattr(conclusions, "dvc_output_paths", lambda root: {"docs/data/stage/raw.csv"})
    record = {"id": "dvc-artifact", "artifacts": [{"path": artifact_path, "sha256": digest}]}

    assert validate_artifact_hashes(record, tmp_path) == []
    write_text_lf(manifest, f"{'0' * 64}  {artifact_path}\n")
    assert "DVC SHA256SUMS mismatch" in validate_artifact_hashes(record, tmp_path)[0]


def test_hosts_registry_requires_full_rustc_identity():
    valid_host = {
        "id": "ref", "reference": True, "cpu_model": "CPU", "physical_cores": 4,
        "logical_cores": 8, "ram_bytes": 1024, "os_family": "OS", "rustc": "1.98.1",
        "rustc_commit_hash": "a" * 40, "rustc_commit_date": "2026-09-01", "rustc_host": "target",
        "rust_target": "target", "a5": {"samples": 41},
    }
    assert validate_hosts({"host": [valid_host]}) == []
    invalid_host = {**valid_host, "rustc_commit_hash": "not-a-full-hash"}
    assert any("rustc_commit_hash" in error for error in validate_hosts({"host": [invalid_host]}))


def test_complete_protocol_requires_exact_predicate_parameter_keys():
    conclusion = {"id": "C-1", "tier": 1, "type": "timing"}
    protocol = {
        "id": "E-1",
        "status": "complete",
        "ordered_cell_ids": ["c1"],
        "predicate_kind": "ratio_band",
        "predicate_parameters": {
            "historical_values": [1.0],
            "ratio_direction": "a/b",
            "bands_pct": [0.1, 0.2],
        },
        "host_id": "host",
        "scope": "one cell",
        "harness_paths": ["h.rs"],
    }
    assert any("exactly" in error for error in _protocol_errors(conclusion, protocol, Path(".")))


def test_evidence_reader_rejects_blank_non_jsonl_records(tmp_path: Path):
    path = tmp_path / "evidence.jsonl"
    write_text_lf(path, json.dumps({"id": "one"}) + "\n\n")
    records, errors = load_evidence(path)
    assert records == [{"id": "one"}]
    assert any("blank" in error for error in errors)


def test_hosts_registry_requires_one_well_formed_reference():
    valid = {
        "host": [{
            "id": "ref", "reference": True, "cpu_model": "CPU", "physical_cores": 4,
            "logical_cores": 8, "ram_bytes": 1024, "os_family": "OS", "rustc": "1.98.1",
            "rustc_commit_hash": "a" * 40, "rustc_commit_date": "2026-09-01", "rustc_host": "target",
            "rust_target": "target", "a5": {"samples": 41},
        }]
    }
    assert validate_hosts(valid) == []
    non_reference = {**valid["host"][0], "reference": False}
    assert any("exactly one reference" in error for error in validate_hosts({"host": [non_reference]}))


def test_evidence_checker_rejects_missing_supersession_target():
    from simciv_analysis.conclusions import ROOT, _evidence_errors

    lock_path = ROOT / "Cargo.lock"
    record = {
        "id": "ev-1",
        "conclusion_id": "C-1",
        "role": "report",
        "kind": "deterministic_exact",
        "revision": __import__("subprocess").check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "toolchain": {
            "rustc": "1.98.1", "rust_target": "target", "cargo_lock_sha256": "a" * 64,
            "python": None, "uv": None, "analysis_lock_sha256": None, "os_family": None,
            "os_build": None, "cpu_model": None, "physical_cores": None, "logical_cores": None,
            "external_tools": {}, "a5_settings": None,
        },
        "protocol_id": "E-1",
        "date": "2026-10-27",
        "artifacts": [{"path": "Cargo.lock", "sha256": hashlib.sha256(lock_path.read_bytes()).hexdigest()}],
        "cell_results": [],
        "supersedes": ["missing-evidence"],
    }
    errors = _evidence_errors(
        [record],
        {"C-1": {"experiment": "E-1", "report_revision": record["revision"]}},
        {"E-1": {"ordered_cell_ids": []}},
        {"host": []},
        ROOT,
    )
    assert any("supersedes target does not exist" in error for error in errors)


def test_evidence_checker_rejects_incompatible_kind_type_role():
    from simciv_analysis.conclusions import ROOT, _evidence_errors
    import subprocess

    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    lock_path = ROOT / "Cargo.lock"
    record = {
        "id": "ev-kind", "conclusion_id": "C-1", "role": "report", "kind": "deterministic_exact",
        "revision": revision,
        "toolchain": {
            "rustc": "1.98.1", "rust_target": "target", "cargo_lock_sha256": "a" * 64,
            "python": None, "uv": None, "analysis_lock_sha256": None, "os_family": None,
            "os_build": None, "cpu_model": None, "physical_cores": None, "logical_cores": None,
            "external_tools": {}, "a5_settings": None,
        },
        "protocol_id": "E-1", "date": "2026-10-27",
        "artifacts": [{"path": "Cargo.lock", "sha256": hashlib.sha256(lock_path.read_bytes()).hexdigest()}],
        "cell_results": [],
    }
    errors = _evidence_errors(
        [record],
        {"C-1": {"type": "timing", "experiment": "E-1", "report_revision": revision}},
        {"E-1": {"ordered_cell_ids": []}},
        {"host": []},
        ROOT,
    )
    assert any("combination is not accepted" in error for error in errors)


def test_protocol_checker_rejects_missing_harness_and_nonclosed_parameter_keys():
    conclusion = {"id": "C-1", "tier": 1, "type": "timing"}
    protocol = {
        "id": "E-1", "status": "complete", "ordered_cell_ids": ["c1"],
        "predicate_kind": "ratio_band",
        "predicate_parameters": {"historical_values": [1.0], "ratio_direction": "a/b", "deviation_bands_pct": [10, 20], "extra": True},
        "host_id": "host", "scope": "one cell", "harness_paths": ["missing-harness.rs"],
    }
    errors = _protocol_errors(conclusion, protocol, Path("."))
    assert any("exactly" in error for error in errors)
    assert any("harness path is missing" in error for error in errors)
