from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path

import pytest

from simciv_analysis.a5_evidence import (
    append_evidence,
    build_evidence_record,
    build_retrospective_manifest,
    build_run_manifest,
    tonight_plan,
)
import simciv_analysis.a5_evidence as a5_module
from simciv_analysis.conclusions import ROOT
from simciv_analysis.io import write_text_lf
from simciv_analysis.provenance import digest_files


def _identity(revision: str, host: dict):
    return {
        "revision": revision,
        "toolchain": {
            "rustc": "1.98.1 (48a229cea 2026-09-01)",
            "rustc_vv": (
                "rustc 1.98.1 (48a229cea 2026-09-01)\n"
                "commit-hash: 48a229ceaefd4985c50990b14116b6d856af0985\n"
                "commit-date: 2026-09-01\nhost: x86_64-pc-windows-msvc\nrelease: 1.98.1\n"
            ),
            "rust_target": "x86_64-pc-windows-msvc",
            "cargo_lock_sha256": "a" * 64,
            "python": "3.15.1",
            "uv": "0.13.5",
            "analysis_lock_sha256": None,
            "os_family": "Windows",
            "os_build": "10.0.26300",
            "cpu_model": host["cpu_model"],
            "physical_cores": host["physical_cores"],
            "logical_cores": host["logical_cores"],
            "external_tools": {"cargo": "1.98.1", "rustc": "1.98.1", "uv": "0.13.5"},
            "a5_settings": host["a5"],
        },
    }


def _host():
    return {
        "id": "reference",
        "reference": True,
        "cpu_model": "CPU",
        "physical_cores": 6,
        "logical_cores": 12,
        "ram_bytes": 1000,
        "os_family": "Windows",
        "rustc": "1.98.1",
        "rustc_commit_hash": "48a229ceaefd4985c50990b14116b6d856af0985",
        "rustc_commit_date": "2026-09-01",
        "rustc_host": "x86_64-pc-windows-msvc",
        "rust_target": "x86_64-pc-windows-msvc",
        "a5": {
            "warmups": 2,
            "samples": 41,
            "sample_target_seconds": 1.25,
            "minimum_sample_seconds": 1.0,
            "max_path_mad_over_median": 0.05,
            "priority": "High",
            "affinity_mask": "0x555",
            "pair_order": "alternating",
        },
    }


def _protocol(tmp_path: Path):
    overlay_path = tmp_path / "driver.rs"
    write_text_lf(overlay_path, "overlay\n")
    return {
        "id": "experiment",
        "conclusion_id": "C-1",
        "report_revision": "a" * 40,
        "host_id": "reference",
        "ordered_cell_ids": ["cell-1"],
        "harness_overlays": [{
            "path": "driver.rs",
            "sha256": hashlib.sha256(b"overlay\n").hexdigest(),
            "origin_commit": "b" * 40,
        }],
        "sampling": {"reference_host_id": "reference"},
    }


def _completed_result(cell_id="cell-1", *, sample_count=41):
    return {
        "record": "A5_CELL_RESULT",
        "cell_id": cell_id,
        "process_controls": {"priority_set": True, "affinity_set": True},
        "observed": {"ratio": 1.0},
        "validity": {
            "warmups": 2,
            "sample_count": sample_count,
            "repetitions_per_sample": {"lower_endpoint": 5, "upper_endpoint": 9},
            "sample_seconds": [1.3] * sample_count,
            "path_mad_over_median": {"lower_endpoint": 0.01, "upper_endpoint": 0.02},
        },
    }


def test_manifest_pins_revision_toolchain_and_registered_overlay(tmp_path: Path):
    protocol = _protocol(tmp_path)
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="run-1",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    assert manifest["worktree_head"] == protocol["report_revision"]
    assert manifest["toolchain"]["rustc_vv"].startswith("rustc 1.98.1")
    assert manifest["harness_overlays"] == protocol["harness_overlays"]


def test_windows_manifest_trims_wmi_cpu_model_padding(monkeypatch):
    rustc_vv = (
        "rustc 1.98.1 (48a229cea 2026-09-01)\n"
        "commit-hash: 48a229ceaefd4985c50990b14116b6d856af0985\n"
        "commit-date: 2026-09-01\nhost: x86_64-pc-windows-msvc\nrelease: 1.98.1\n"
    )

    def check_output(args, **_kwargs):
        if args[0] == "rustc":
            return rustc_vv
        if args[0] == "powershell":
            return json.dumps({"cpu": "Reference CPU   ", "physical": 6, "logical": 12, "os": "Microsoft Windows 11 Pro"})
        if args[0] == "cargo":
            return "cargo test"
        if args[0] == "uv":
            return "uv test"
        raise AssertionError(args)

    monkeypatch.setattr(a5_module.sys, "platform", "win32")
    monkeypatch.setattr(a5_module.subprocess, "check_output", check_output)
    identity = a5_module._runtime_identity(ROOT)
    assert identity["toolchain"]["cpu_model"] == "Reference CPU"


def test_manifest_rejects_a_different_worktree_revision(tmp_path: Path):
    protocol = _protocol(tmp_path)
    with pytest.raises(ValueError, match="worktree HEAD"):
        build_run_manifest(
            protocol,
            run_id="run-1",
            root=tmp_path,
            hosts={"host": [_host()]},
            identity=_identity("c" * 40, _host()),
            started_at="2026-10-27T00:00:00Z",
        )


def test_manifest_rejects_overlay_bytes_that_changed_after_registration(tmp_path: Path):
    protocol = _protocol(tmp_path)
    write_text_lf(tmp_path / "driver.rs", "changed\n")
    with pytest.raises(ValueError, match="registered overlay bytes"):
        build_run_manifest(
            protocol,
            run_id="run-1",
            root=tmp_path,
            hosts={"host": [_host()]},
            identity=_identity(protocol["report_revision"], _host()),
            started_at="2026-10-27T00:00:00Z",
        )


def test_manifest_uses_registered_git_blob_when_the_worktree_copy_differs(tmp_path: Path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=tmp_path, check=True)
    source = b"historical overlay\n"
    write_text_lf(tmp_path / "driver.rs", source.decode())
    subprocess.run(["git", "add", "driver.rs"], cwd=tmp_path, check=True)
    write_text_lf(tmp_path / "message.txt", "test: add overlay\n\n- historical source\n")
    subprocess.run(["git", "commit", "--quiet", "-F", "message.txt"], cwd=tmp_path, check=True)
    origin = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()
    protocol = _protocol(tmp_path)
    protocol["harness_overlays"] = [{
        "path": "driver.rs",
        "sha256": hashlib.sha256(source).hexdigest(),
        "origin_commit": origin,
    }]
    write_text_lf(tmp_path / "driver.rs", "later worktree copy\n")

    manifest = build_run_manifest(
        protocol,
        run_id="historical-overlay",
        root=tmp_path,
        hosts={"host": [_host()]},
        identity=_identity(protocol["report_revision"], _host()),
        started_at="2026-10-27T00:00:00Z",
    )

    assert manifest["harness_overlays"] == protocol["harness_overlays"]


def test_retrospective_manifest_is_source_backed_and_does_not_invent_start_time(tmp_path: Path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=tmp_path, check=True)
    (tmp_path / "analysis").mkdir()
    (tmp_path / "docs/data/pilot/raw").mkdir(parents=True)
    write_text_lf(tmp_path / "Cargo.lock", "lock\n")
    write_text_lf(tmp_path / "analysis/uv.lock", "uv lock\n")
    write_text_lf(tmp_path / "mise.toml", '"aqua:astral-sh/uv" = "0.13.5"\n')
    write_text_lf(tmp_path / "rust-toolchain.toml", (Path(__file__).resolve().parents[2] / "rust-toolchain.toml").read_text(encoding="utf-8"))
    write_text_lf(tmp_path / "driver.rs", "driver\n")
    write_text_lf(tmp_path / "message.txt", "test: add pilot source\n\n- fixture source commit\n")
    subprocess.run(["git", "add", "Cargo.lock", "analysis/uv.lock", "mise.toml", "rust-toolchain.toml", "driver.rs"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-F", "message.txt"], cwd=tmp_path, check=True)
    source_commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()
    rustc_version = subprocess.check_output(["rustc", "-V"], cwd=tmp_path, text=True).strip()
    host = _host()
    protocol = {
        "id": "pilot-stage",
        "source_stage": "pilot-stage",
        "conclusion_id": "C-1",
        "report_revision": source_commit,
        "host_id": "reference",
        "harness_overlays": [{
            "path": "driver.rs",
            "sha256": hashlib.sha256(b"driver\n").hexdigest(),
            "origin_commit": source_commit,
        }],
    }
    header = {
        "record": "SCALE_HOST", "mode": "a5", "warmups": 2, "samples": 41,
        "target_sample_seconds": 1.25, "minimum_sample_seconds": 1.0,
        "process_controls": "priority=high;affinity_mask=0x555",
    }
    raw = tmp_path / "docs/data/pilot/raw/run.jsonl"
    write_text_lf(raw, json.dumps(header) + "\n")
    summary = {
        "stage": "pilot-stage",
        "details": {"report_revision": source_commit},
        "toolchain": rustc_version,
        "harness_files": ["driver.rs"],
        "harness_digest": digest_files(["driver.rs"], tmp_path),
        "outputs": [{
            "path": "docs/data/pilot/raw",
            "sha256": digest_files(["docs/data/pilot/raw/run.jsonl"], tmp_path),
        }],
        "raw_outputs": ["docs/data/pilot/raw"],
        "command": "cargo run --release --example pilot",
    }
    summary_path = tmp_path / "docs/data/pilot/summary.json"
    write_text_lf(summary_path, json.dumps(summary) + "\n")
    write_text_lf(tmp_path / "message.txt", "data: record source summary\n\n- preserve pilot provenance\n")
    subprocess.run(["git", "add", "docs/data/pilot"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-F", "message.txt"], cwd=tmp_path, check=True)

    manifest = build_retrospective_manifest(
        protocol,
        run_id="pilot-run",
        summary_path="docs/data/pilot/summary.json",
        raw_path="docs/data/pilot/raw/run.jsonl",
        harness_commit=source_commit,
        root=tmp_path,
        hosts_doc={"host": [host]},
    )

    assert manifest["started_at"] is None
    assert manifest["harness_overlays"] == protocol["harness_overlays"]
    assert manifest["raw_artifact"] == summary["outputs"][0]
    assert "original run start timestamp was not recorded" in manifest["date_basis"]


def test_ingestion_builds_hashed_report_evidence_from_complete_cells(tmp_path: Path):
    protocol = _protocol(tmp_path)
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="run-1",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    manifest_path = tmp_path / "manifest.json"
    raw_path = tmp_path / "raw.jsonl"
    write_text_lf(manifest_path, json.dumps(manifest) + "\n")
    write_text_lf(raw_path, json.dumps(_completed_result()) + "\n")

    record = build_evidence_record(manifest, raw_path, protocol, root=tmp_path, manifest_path=manifest_path, hosts_doc={"host": [host]})

    assert record["kind"] == "a5_report_revision"
    assert record["role"] == "report"
    assert record["cell_results"][0]["valid"] is True
    assert record["cell_results"][0]["validity"]["sample_seconds"] == [1.3] * 41
    assert {item["path"] for item in record["artifacts"]} == {"manifest.json", "raw.jsonl"}
    assert record["harness_overlays"] == protocol["harness_overlays"]


@pytest.mark.parametrize(
    ("serial_key", "parallel_key", "serial", "parallel"),
    [
        ("serial_us_per_call", "rayon_us_per_call", 100.0, 50.0),
        ("serial_ns_per_call", "parallel_ns_per_call", 100_000.0, 50_000.0),
    ],
)
def test_ingestion_normalizes_committed_m2_and_m4_adapter_series(
    tmp_path: Path, serial_key: str, parallel_key: str, serial: float, parallel: float
):
    protocol = _protocol(tmp_path)
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="adapter-run",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    sample_records = [
        {serial_key: serial + 2 * index, parallel_key: parallel + index, "timed_pair_elapsed_ns": 1_300_000_000}
        for index in range(41)
    ]
    row = {
        "kind": "series", "cell_id": "cell-1", "complete": True,
        "process_controls": {"priority_set": True, "affinity_set": True},
        "series": {"warmups": 2, "samples": 41, "repetitions_per_sample": 3, "sample_records": sample_records},
    }
    raw_path = tmp_path / "raw.jsonl"
    write_text_lf(raw_path, json.dumps(row) + "\n")

    record = build_evidence_record(manifest, raw_path, protocol, root=tmp_path, hosts_doc={"host": [host]})

    assert record["cell_results"][0]["observed_ratio"] == 2.0
    assert record["cell_results"][0]["validity"]["repetitions_per_sample"] == 3
    assert record["cell_results"][0]["validity"]["path_mad_over_median"]["serial"] > 0


def test_ingestion_normalizes_m3_05_pipe_records_and_excludes_invalid_phases(tmp_path: Path):
    protocol = _protocol(tmp_path)
    protocol.update({"id": "m3_05_any_phase_alpha_first_break", "ordered_cell_ids": ["k8-day0"], "harness_overlays": []})
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="m3-run",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    durations = ",".join(["1.3"] * 41)
    lines = ["SCALE_HOST|mode=a5|warmups=2|samples=41|process_controls=priority=high;affinity_mask=0x555"]
    for phase, high_mad, alpha in (("P1", 0.02, 1.2), ("P2", 0.10, 1.4)):
        for population, mad in ((100000, 0.01), (250000, high_mad)):
            lines.append(
                f"SCALE_PHASE|N={population}|k=8|horizon=0|phase={phase}|median_us=100|mad_us={mad * 100}"
                f"|repetitions_per_sample=5|sample_seconds={durations}"
            )
        lines.append(f"SCALE_ALPHA|k=8|horizon=0|phase={phase}|from_N=100000|to_N=250000|alpha={alpha}")
    for population in (100000, 250000):
        lines.append(
            f"SCALE_FULL_TICK|N={population}|K={population // 200}|k=8|horizon=0|serial_median_us=100|serial_mad_us=2"
            f"|repetitions_per_sample=5|sample_seconds={durations}"
        )
    lines.append("SCALE_ALPHA|k=8|horizon=0|phase=full_tick|from_N=100000|to_N=250000|alpha=1.3")
    raw_path = tmp_path / "m3_raw.jsonl"
    write_text_lf(raw_path, "\n".join(lines) + "\n")

    record = build_evidence_record(manifest, raw_path, protocol, root=tmp_path, hosts_doc={"host": [host]})
    cell = record["cell_results"][0]

    assert cell["observed"]["maximum_phase"] == "full_tick"
    assert cell["value"] == 1.3
    assert cell["valid"] is True
    assert cell["validity"]["repetitions_per_sample"] == {"lower_endpoint": 5, "upper_endpoint": 5}


def test_ingestion_normalizes_m3_05_json_records_from_the_committed_pilot_format(tmp_path: Path):
    protocol = _protocol(tmp_path)
    protocol.update({"id": "m3_05_any_phase_alpha_first_break", "ordered_cell_ids": ["k8-day0"], "harness_overlays": []})
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="m3-json-run",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    seconds = [1.3] * 41
    records = [{
        "record": "SCALE_HOST", "mode": "a5", "warmups": 2, "samples": 41,
        "process_controls": "priority=high;affinity_mask=0x555",
    }]
    for population, median in ((100_000, 100.0), (250_000, 250.0)):
        records.append({
            "record": "SCALE_PHASE", "N": population, "k": 8, "horizon": 0,
            "phase": "P1_regrowth", "median_us": median, "mad_us": 1.0,
            "repetitions_per_sample": 2, "sample_seconds": seconds,
        })
        records.append({
            "record": "SCALE_FULL_TICK", "N": population, "k": 8, "horizon": 0,
            "serial_median_us": median, "serial_mad_us": 1.0, "r": 2,
            "sample_seconds": seconds,
        })
    records.extend([
        {"record": "SCALE_ALPHA", "k": 8, "horizon": 0, "phase": "P1_regrowth", "from_N": 100_000, "to_N": 250_000, "alpha": 1.2},
        {"record": "SCALE_ALPHA", "k": 8, "horizon": 0, "phase": "full_tick", "from_N": 100_000, "to_N": 250_000, "alpha": 1.3},
    ])
    raw_path = tmp_path / "m3_json_raw.jsonl"
    write_text_lf(raw_path, "".join(json.dumps(row) + "\n" for row in records))

    record = build_evidence_record(manifest, raw_path, protocol, root=tmp_path, hosts_doc={"host": [host]})
    cell = record["cell_results"][0]

    assert cell["observed"] == {"alpha": 1.3, "maximum_phase": "full_tick"}
    assert cell["valid"] is True
    assert cell["process_controls"] == {"priority_set": True, "affinity_set": True}


def test_ingestion_normalizes_m3_05_completion_cells_from_run_records(tmp_path: Path):
    protocol = _protocol(tmp_path)
    protocol.update({
        "id": "m3_05_static_scale_completion",
        "conclusion_id": "C-COMPLETION",
        "ordered_cell_ids": ["N1M-k8-day0"],
        "harness_overlays": [],
        "predicate_kind": "completion",
    })
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="m3-completion",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    raw_path = tmp_path / "completion.jsonl"
    rows = [
        {"record": "SCALE_HOST", "mode": "a5", "warmups": 2, "samples": 41, "process_controls": "priority=high;affinity_mask=0x555"},
        {"record": "SCALE_FULL_TICK", "N": 1_000_000, "K": 5_000, "k": 8, "horizon": 0, "serial_median_us": 10},
        {"record": "SCALE_COMPLETE", "largest_N": 1_000_000, "cells": 16, "parity": "all_serial_parallel_final_digests_equal"},
    ]
    write_text_lf(raw_path, "".join(json.dumps(row) + "\n" for row in rows))

    record = build_evidence_record(manifest, raw_path, protocol, root=tmp_path, hosts_doc={"host": [host]})

    assert record["cell_results"] == [{
        "cell_id": "N1M-k8-day0",
        "completed": True,
        "process_controls": {"priority_set": True, "affinity_set": True},
    }]


def test_ingestion_rejects_missing_cell_and_non_a5_sample_count(tmp_path: Path):
    protocol = _protocol(tmp_path)
    host = _host()
    manifest = build_run_manifest(
        protocol,
        run_id="run-1",
        root=tmp_path,
        hosts={"host": [host]},
        identity=_identity(protocol["report_revision"], host),
        started_at="2026-10-27T00:00:00Z",
    )
    raw_path = tmp_path / "raw.jsonl"
    write_text_lf(raw_path, json.dumps(_completed_result(sample_count=40)) + "\n")
    with pytest.raises(ValueError, match="sample count differ"):
        build_evidence_record(manifest, raw_path, protocol, root=tmp_path, hosts_doc={"host": [host]})

    write_text_lf(raw_path, "")
    with pytest.raises(ValueError, match="ordered cells"):
        build_evidence_record(manifest, raw_path, protocol, root=tmp_path, hosts_doc={"host": [host]})


def test_evidence_append_keeps_old_bytes_and_rejects_duplicate_id(tmp_path: Path):
    path = tmp_path / "evidence.jsonl"
    original = '{"id":"old"}\n'
    write_text_lf(path, original)
    append_evidence({"id": "new", "cell_results": []}, path)
    assert path.read_text(encoding="utf-8") == original + '{"cell_results":[],"id":"new"}\n'
    with pytest.raises(ValueError, match="already exists"):
        append_evidence({"id": "new"}, path)


def test_tonight_dry_run_lists_passed_gate_cells_and_models_setup():
    from simciv_analysis.provenance import ROOT

    rows = tonight_plan(ROOT)
    assert len(rows) == 16
    assert all(row["estimated_seconds"] >= row["setup_seconds"] > 0 for row in rows)
    by_protocol = {}
    for row in rows:
        by_protocol[row["protocol_id"]] = by_protocol.get(row["protocol_id"], 0) + 1
    assert by_protocol == {
        "m2_explicit_rayon_speedup": 8,
        "m3_11_phase9c_speedup": 1,
        "m3_11_whole_tick_speedup": 2,
        "m3_09_incremental_equal_live": 4,
        "m4_07_case_a_search_parallel_speedup": 1,
    }
    assert not any(row["protocol_id"].startswith("m3_05_") for row in rows)
    m3_09_100k = next(row for row in rows if row["cell_id"] == "N100k-paired")
    assert m3_09_100k["setup_seconds"] > 70
    assert m3_09_100k["series"][0]["setup_per_operation_seconds"] > 0
    m3_11_phase = next(row for row in rows if row["protocol_id"] == "m3_11_phase9c_speedup")
    assert m3_11_phase["series"][0]["pair_seconds"] == pytest.approx(0.2739754, rel=1e-6)
    m4 = next(row for row in rows if row["protocol_id"] == "m4_07_case_a_search_parallel_speedup")
    assert m4["series"][0]["pair_seconds"] > 1.25
    assert m4["series"][0]["repetitions_per_sample"] == 1
    assert m4["series"][0]["calibration_seconds"] == m4["series"][0]["pair_seconds"]
    assert m4["estimated_seconds"] > 41 * 1.25
