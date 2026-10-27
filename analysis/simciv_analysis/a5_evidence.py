"""Write pre-run manifests and ingest completed report-revision A-5 runs."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .conclusions import EVIDENCE, HOSTS, EXPERIMENTS, ROOT, _git, _repo_file, validate_evidence_append_only
from .io import write_text_lf
from .verdicts import _parse_rustc_vv, valid_repetitions_per_sample


def _digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _overlay_bytes(root: Path, overlay: dict[str, Any]) -> bytes | None:
    path, digest, origin = str(overlay["path"]), str(overlay["sha256"]), str(overlay["origin_commit"])
    blob = _git(root, "show", f"{origin}:{path}")
    if blob.returncode == 0 and _digest_bytes(blob.stdout) == digest:
        return blob.stdout
    file = _repo_file(root, path)
    if file is not None and file.is_file() and _digest(file) == digest:
        return file.read_bytes()
    return None


def _runtime_identity(root: Path) -> dict[str, Any]:
    rustc_vv = subprocess.check_output(["rustc", "-Vv"], cwd=root, text=True)
    parsed = _parse_rustc_vv(rustc_vv)
    if parsed is None:
        raise ValueError("rustc -Vv output is invalid")
    if sys.platform == "win32":
        command = (
            "$p=Get-CimInstance Win32_Processor; $s=Get-CimInstance Win32_ComputerSystem; "
            "$o=Get-CimInstance Win32_OperatingSystem; "
            "[pscustomobject]@{cpu=($p | Select-Object -First 1 -ExpandProperty Name); "
            "physical=($p | Measure-Object -Property NumberOfCores -Sum).Sum; "
            "logical=$s.NumberOfLogicalProcessors; os=$o.Caption} | ConvertTo-Json -Compress"
        )
        hardware = json.loads(subprocess.check_output(["powershell", "-NoProfile", "-Command", command], text=True))
        cpu_model, physical, logical = hardware["cpu"].strip(), int(hardware["physical"]), int(hardware["logical"])
        os_family = hardware["os"].removeprefix("Microsoft ")
    else:
        cpu_model = platform.processor() or platform.machine()
        physical = logical = os.cpu_count() or 0
        os_family = platform.system()
        if sys.platform == "darwin":
            cpu_model = subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip()
            physical = int(subprocess.check_output(["sysctl", "-n", "hw.physicalcpu"], text=True))
            logical = int(subprocess.check_output(["sysctl", "-n", "hw.logicalcpu"], text=True))
    toolchain = {
        "rustc": parsed["first_line"],
        "rustc_vv": rustc_vv,
        "rust_target": parsed["host"],
        "cargo_lock_sha256": _digest(root / "Cargo.lock"),
        "analysis_lock_sha256": _digest(root / "analysis" / "uv.lock") if (root / "analysis" / "uv.lock").is_file() else None,
        "python": platform.python_version(),
        "uv": subprocess.check_output(["uv", "--version"], text=True).strip().removeprefix("uv "),
        "os_family": os_family,
        "os_build": platform.version(),
        "cpu_model": cpu_model,
        "physical_cores": physical,
        "logical_cores": logical,
        "external_tools": {
            "cargo": subprocess.check_output(["cargo", "--version"], text=True).split()[1],
            "rustc": parsed["release"],
            "uv": subprocess.check_output(["uv", "--version"], text=True).strip().removeprefix("uv "),
        },
    }
    return {"revision": _git(root, "rev-parse", "HEAD").stdout.decode().strip(), "toolchain": toolchain}


def build_run_manifest(
    protocol: dict[str, Any],
    *,
    run_id: str,
    root: Path = ROOT,
    hosts: dict[str, Any] | None = None,
    identity: dict[str, Any] | None = None,
    started_at: str | None = None,
    commands: list[str] | None = None,
) -> dict[str, Any]:
    host_rows = (hosts or tomllib.loads((root / "contracts/hosts.toml").read_text(encoding="utf-8"))).get("host", [])
    host = next((item for item in host_rows if item.get("id") == protocol.get("host_id")), None)
    if host is None or host.get("reference") is not True:
        raise ValueError("A-5 protocol must name the registered reference host")
    identity = identity or _runtime_identity(root)
    revision = str(protocol.get("report_revision", ""))
    if identity.get("revision") != revision:
        raise ValueError("manifest worktree HEAD must equal the protocol report revision")
    toolchain = identity.get("toolchain")
    rustc = _parse_rustc_vv(toolchain.get("rustc_vv")) if isinstance(toolchain, dict) else None
    if rustc is None or toolchain.get("rustc") != rustc["first_line"]:
        raise ValueError("manifest requires a valid rustc -Vv capture")
    if (
        rustc["release"] != host.get("rustc")
        or rustc["commit_hash"] != host.get("rustc_commit_hash")
        or rustc["commit_date"] != host.get("rustc_commit_date")
        or rustc["host"] != host.get("rustc_host")
    ):
        raise ValueError("manifest rustc -Vv identity differs from the registered reference toolchain")
    for key in ("cpu_model", "physical_cores", "logical_cores", "rust_target", "os_family"):
        if toolchain.get(key) != host.get(key):
            raise ValueError(f"manifest host differs from registered reference host: {key}")
    overlays = []
    for overlay in protocol.get("harness_overlays", []):
        path = str(overlay["path"])
        if _overlay_bytes(root, overlay) is None:
            raise ValueError(f"registered overlay bytes are absent or changed: {path}")
        overlays.append({key: overlay[key] for key in ("path", "sha256", "origin_commit")})
    if overlays != protocol.get("harness_overlays", []):
        raise ValueError("manifest overlay list differs from the protocol")
    started_at = started_at or datetime.now(timezone.utc).isoformat(timespec="seconds")
    return {
        "schema_version": 1,
        "run_id": run_id,
        "started_at": started_at,
        "date": started_at[:10],
        "conclusion_id": protocol["conclusion_id"],
        "protocol_id": protocol["id"],
        "role": "report",
        "revision": revision,
        "worktree_head": identity["revision"],
        "host_id": host["id"],
        "toolchain": {**toolchain, "a5_settings": host["a5"]},
        "a5_settings": host["a5"],
        "harness_overlays": overlays,
        "commands": commands or [],
    }


def build_retrospective_manifest(
    protocol: dict[str, Any],
    *,
    run_id: str,
    summary_path: str | Path,
    raw_path: str | Path,
    harness_commit: str,
    root: Path = ROOT,
    hosts_doc: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Record only what an existing DVC pilot summary and raw file establish."""
    summary_file = _repo_file(root, str(summary_path))
    raw_file = _repo_file(root, str(raw_path))
    if summary_file is None or not summary_file.is_file() or raw_file is None or not raw_file.is_file():
        raise ValueError("retrospective A-5 ingestion needs the committed pilot summary and materialized raw output")
    summary = json.loads(summary_file.read_text(encoding="utf-8"))
    details = summary.get("details", {})
    if details.get("report_revision") != protocol.get("report_revision"):
        raise ValueError("pilot summary report revision differs from the registered protocol")
    if summary.get("stage") != protocol.get("source_stage"):
        raise ValueError("pilot summary stage differs from the registered protocol")
    if not re.fullmatch(r"[0-9a-f]{40}", harness_commit):
        raise ValueError("retrospective source commit must be a full SHA")
    harness_files = [str(path) for path in summary.get("harness_files", [])]
    if not harness_files:
        raise ValueError("pilot summary has no harness source list")
    digest = hashlib.sha256()
    for path in sorted(harness_files, key=lambda value: value.encode("utf-8")):
        blob = _git(root, "show", f"{harness_commit}:{path}")
        if blob.returncode:
            raise ValueError(f"pilot harness source is absent from {harness_commit}: {path}")
        digest.update(path.encode("utf-8"))
        digest.update(b"\0")
        digest.update(_digest_bytes(blob.stdout).encode("ascii"))
        digest.update(b"\n")
    if digest.hexdigest() != summary.get("harness_digest"):
        raise ValueError("pilot harness digest does not match the registered source commit")

    output = next((item for item in summary.get("outputs", []) if item.get("path") in summary.get("raw_outputs", [])), None)
    if not isinstance(output, dict) or not isinstance(output.get("sha256"), str):
        raise ValueError("pilot summary has no hashed DVC raw output")
    output_path = _repo_file(root, str(output["path"]))
    if output_path is None or not output_path.is_dir():
        raise ValueError("pilot DVC output directory is absent")
    from .provenance import digest_files

    output_files = [file for file in output_path.rglob("*") if file.is_file()]
    if digest_files(output_files, root) != output["sha256"]:
        raise ValueError("pilot raw output directory digest differs from its provenance summary")
    if raw_file.resolve().parent != output_path.resolve():
        raise ValueError("raw pilot file is not the registered DVC stage output")

    records = [json.loads(line) for line in raw_file.read_text(encoding="utf-8").splitlines() if line.strip()]
    header = next((item for item in records if item.get("record") == "SCALE_HOST"), None)
    host_rows = (hosts_doc or tomllib.loads((root / "contracts/hosts.toml").read_text(encoding="utf-8"))).get("host", [])
    host = next((item for item in host_rows if item.get("id") == protocol.get("host_id")), None)
    if header is None or host is None or host.get("reference") is not True:
        raise ValueError("pilot raw output or reference host registration is missing")
    if (
        header.get("mode") != "a5"
        or header.get("warmups") != host["a5"]["warmups"]
        or header.get("samples") != host["a5"]["samples"]
        or header.get("target_sample_seconds") != host["a5"]["sample_target_seconds"]
        or header.get("minimum_sample_seconds") != host["a5"]["minimum_sample_seconds"]
        or header.get("process_controls") != f"priority=high;affinity_mask={host['a5']['affinity_mask']}"
    ):
        raise ValueError("pilot raw header does not match the frozen A-5 settings")

    rustc_vv = subprocess.check_output(["rustc", "-Vv"], cwd=root, text=True)
    rustc = _parse_rustc_vv(rustc_vv)
    if rustc is None or f"rustc {rustc['first_line']}" != summary.get("toolchain"):
        raise ValueError("installed rustc -Vv differs from the pilot's recorded rustc version")
    summary_commit = _git(root, "log", "-1", "--format=%H", "--", str(summary_path))
    if summary_commit.returncode:
        raise ValueError("cannot find the committed pilot provenance summary")
    date_result = _git(root, "show", "-s", "--format=%cs", summary_commit.stdout.decode().strip())
    if date_result.returncode:
        raise ValueError("cannot determine the provenance summary commit date")
    pinned_tools = _git(root, "show", f"{harness_commit}:mise.toml")
    uv_version = "not recorded"
    if pinned_tools.returncode == 0:
        uv_match = re.search(r'^"aqua:astral-sh/uv"\s*=\s*"([^"]+)"', pinned_tools.stdout.decode(), re.MULTILINE)
        if uv_match:
            uv_version = uv_match.group(1)
    report_revision = str(protocol["report_revision"])
    cargo_blob = _git(root, "show", f"{report_revision}:Cargo.lock")
    if cargo_blob.returncode:
        raise ValueError("report revision Cargo.lock is unavailable")
    analysis_blob = _git(root, "show", f"{report_revision}:analysis/uv.lock")
    overlays = [{key: item[key] for key in ("path", "sha256", "origin_commit")} for item in protocol.get("harness_overlays", [])]
    for overlay in overlays:
        if overlay["origin_commit"] != harness_commit or _overlay_bytes(root, overlay) is None:
            raise ValueError(f"pilot overlay does not match the source commit: {overlay['path']}")
    return {
        "schema_version": 1,
        "run_id": run_id,
        "started_at": None,
        "date": date_result.stdout.decode().strip(),
        "date_basis": "committed pilot provenance summary; original run start timestamp was not recorded",
        "conclusion_id": protocol["conclusion_id"],
        "protocol_id": protocol["id"],
        "role": "report",
        "revision": report_revision,
        "worktree_head": report_revision,
        "host_id": host["id"],
        "toolchain": {
            "rustc": rustc["first_line"],
            "rustc_vv": rustc_vv,
            "rust_target": rustc["host"],
            "cargo_lock_sha256": _digest_bytes(cargo_blob.stdout),
            "analysis_lock_sha256": _digest_bytes(analysis_blob.stdout) if analysis_blob.returncode == 0 else None,
            "python": "not recorded in pilot provenance",
            "uv": uv_version,
            "os_family": "Windows 11",
            "os_build": "not recorded in pilot provenance",
            "cpu_model": host["cpu_model"],
            "physical_cores": host["physical_cores"],
            "logical_cores": host["logical_cores"],
            "external_tools": {"cargo": "not separately recorded", "rustc": rustc["release"], "uv": uv_version},
            "a5_settings": host["a5"],
        },
        "a5_settings": host["a5"],
        "harness_overlays": overlays,
        "commands": [summary.get("command", "")],
        "raw_artifact": {"path": output["path"], "sha256": output["sha256"]},
        "source_summary": {"path": str(summary_path).replace("\\", "/"), "sha256": _digest(summary_file)},
        "source_harness_commit": harness_commit,
        "source_harness_files": harness_files,
        "source_harness_digest": summary["harness_digest"],
        "notes": [
            "Retrospective manifest derived from the committed DVC summary and complete raw JSONL; no simulation was rerun.",
            "Per-sample timestamps and the original run start time are unavailable; date_basis records the summary commit date.",
            "Python version and Windows build were not captured in the pilot provenance.",
        ],
        "supersedes": protocol.get("evidence_supersedes", []),
    }


def _median(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[len(ordered) // 2]


def _mad_over_median(values: list[float]) -> float:
    center = _median(values)
    return _median([abs(value - center) for value in values]) / center if center > 0 else math.inf


def _normalize_series(row: dict[str, Any]) -> dict[str, Any]:
    series = row.get("series")
    if not isinstance(series, dict) or row.get("complete") is not True or row.get("smoke") is True:
        raise ValueError(f"completed non-smoke A-5 series is required for {row.get('cell_id')}")
    controls = row.get("process_controls")
    if not isinstance(controls, dict) or controls.get("priority_set") is not True or controls.get("affinity_set") is not True:
        raise ValueError(f"A-5 process priority and affinity were not confirmed for {row.get('cell_id')}")
    samples = series.get("sample_records")
    if not isinstance(samples, list) or not samples:
        raise ValueError(f"A-5 series has no sample records: {row.get('cell_id')}")
    serial_key, parallel_key = (
        ("serial_us_per_call", "rayon_us_per_call")
        if "serial_us_per_call" in samples[0]
        else ("serial_ns_per_call", "parallel_ns_per_call")
    )
    serial = [float(sample[serial_key]) for sample in samples]
    parallel = [float(sample[parallel_key]) for sample in samples]
    seconds = [int(sample["timed_pair_elapsed_ns"]) / 1_000_000_000.0 for sample in samples]
    result = {
        "cell_id": row["cell_id"],
        "process_controls": controls,
        "observed_ratio": _median(serial) / _median(parallel),
        "observed": {"serial_median_per_call": _median(serial), "parallel_median_per_call": _median(parallel)},
        "validity": {
            "warmups": int(series["warmups"]),
            "sample_count": int(series["samples"]),
            "repetitions_per_sample": int(series["repetitions_per_sample"]),
            "sample_seconds": seconds,
            "path_mad_over_median": {"serial": _mad_over_median(serial), "parallel": _mad_over_median(parallel)},
        },
    }
    return result


def _pipe_fields(line: str) -> tuple[str, dict[str, str]] | None:
    chunks = line.split("|")
    if len(chunks) < 2 or not chunks[0].startswith("SCALE_"):
        return None
    fields = {}
    for chunk in chunks[1:]:
        key, separator, value = chunk.partition("=")
        if separator:
            fields[key] = value
    return chunks[0], fields


def _numeric_list(value: Any) -> list[float]:
    parts = value.split(",") if isinstance(value, str) else value
    if not isinstance(parts, list):
        raise ValueError("A-5 sample durations must be a list")
    return [float(part) for part in parts]


def _normalize_m3_05(records: list[str] | list[dict[str, Any]], protocol: dict[str, Any], settings: dict[str, Any]) -> dict[str, dict[str, Any]]:
    if records and isinstance(records[0], str):
        parsed = [item for line in records if isinstance(line, str) and (item := _pipe_fields(line)) is not None]
    else:
        parsed = [
            (str(row.get("record", "")), row)
            for row in records
            if isinstance(row, dict) and str(row.get("record", "")).startswith("SCALE_")
        ]
    host = next((fields for kind, fields in parsed if kind == "SCALE_HOST"), None)
    if host is None or host.get("mode") != "a5" or int(host.get("samples", 0)) != settings["samples"]:
        raise ValueError("M3-05 raw output is not a completed A-5 run")
    if host.get("process_controls") != f"priority=high;affinity_mask={settings['affinity_mask']}":
        raise ValueError("M3-05 raw output does not confirm the registered A-5 process controls")
    cell_ids = [str(cell) for cell in protocol.get("ordered_cell_ids", [])]
    if protocol.get("predicate_kind") == "completion":
        completed = any(kind == "SCALE_COMPLETE" and int(fields.get("largest_N", 0)) == 1_000_000 for kind, fields in parsed)
        full_rows = {(int(fields["N"]), int(fields["k"]), int(fields["horizon"]))
                     for kind, fields in parsed if kind == "SCALE_FULL_TICK"}
        result = {}
        for cell_id in cell_ids:
            match = re.fullmatch(r"N1M-k(\d+)-day(\d+)", cell_id)
            key = (1_000_000, int(match.group(1)), int(match.group(2)))
            result[cell_id] = {"cell_id": cell_id, "completed": completed and key in full_rows,
                               "process_controls": {"priority_set": True, "affinity_set": True}}
        return result
    if protocol.get("id") != "m3_05_any_phase_alpha_first_break":
        raise ValueError(f"unsupported M3-05 A-5 protocol: {protocol.get('id')}")
    phase_rows = {
        (int(fields["N"]), int(fields["k"]), int(fields["horizon"]), fields.get("phase", fields.get("stage", ""))): fields
        for kind, fields in parsed if kind in {"SCALE_PHASE", "SCALE_P6B_STAGE"}
    }
    full_rows = {
        (int(fields["N"]), int(fields["k"]), int(fields["horizon"])): fields
        for kind, fields in parsed if kind == "SCALE_FULL_TICK"
    }
    alpha_rows = [fields for kind, fields in parsed if kind == "SCALE_ALPHA"]
    result = {}
    for cell_id in cell_ids:
        match = re.fullmatch(r"k(\d+)-day(\d+)", cell_id)
        degree, day = int(match.group(1)), int(match.group(2))
        candidates = []
        for fields in alpha_rows:
            if int(fields.get("k", -1)) != degree or int(fields.get("horizon", -1)) != day:
                continue
            if int(fields.get("from_N", -1)) != 100_000 or int(fields.get("to_N", -1)) != 250_000:
                continue
            try:
                alpha = float(fields["alpha"])
                if fields["phase"] == "full_tick":
                    low = full_rows[(100_000, degree, day)]
                    high = full_rows[(250_000, degree, day)]
                    low_median, low_mad = float(low["serial_median_us"]), float(low["serial_mad_us"])
                    high_median, high_mad = float(high["serial_median_us"]), float(high["serial_mad_us"])
                else:
                    low = phase_rows[(100_000, degree, day, fields["phase"])]
                    high = phase_rows[(250_000, degree, day, fields["phase"])]
                    low_median, low_mad = float(low["median_us"]), float(low["mad_us"])
                    high_median, high_mad = float(high["median_us"]), float(high["mad_us"])
                low_spread = low_mad / low_median
                high_spread = high_mad / high_median
            except (KeyError, ValueError, ZeroDivisionError):
                continue
            candidates.append((alpha, fields["phase"], low, high, low_spread, high_spread))
        if not candidates:
            raise ValueError(f"M3-05 A-5 raw output lacks alpha endpoints for {cell_id}")
        valid = [item for item in candidates if item[4] <= settings["max_path_mad_over_median"] and item[5] <= settings["max_path_mad_over_median"]]
        alpha, phase, low, high, low_spread, high_spread = max(valid or candidates, key=lambda item: item[0])
        low_seconds = _numeric_list(low["sample_seconds"])
        high_seconds = _numeric_list(high["sample_seconds"])
        if len(low_seconds) != settings["samples"] or len(high_seconds) != settings["samples"]:
            raise ValueError(f"M3-05 A-5 phase series lacks {settings['samples']} samples for {cell_id}")
        low_repetitions = int(low.get("repetitions_per_sample", low.get("r", 0)))
        high_repetitions = int(high.get("repetitions_per_sample", high.get("r", 0)))
        if low_repetitions < 1 or high_repetitions < 1:
            raise ValueError(f"M3-05 A-5 raw output lacks endpoint repetition counts for {cell_id}")
        result[cell_id] = {
            "cell_id": cell_id,
            "value": alpha,
            "observed": {"alpha": alpha, "maximum_phase": phase},
            "process_controls": {"priority_set": True, "affinity_set": True},
            "validity": {
                "warmups": int(host["warmups"]),
                "sample_count": int(host["samples"]),
                "repetitions_per_sample": {"lower_endpoint": low_repetitions, "upper_endpoint": high_repetitions},
                "sample_seconds": [min(left, right) for left, right in zip(low_seconds, high_seconds)],
                "path_mad_over_median": {"lower_endpoint": low_spread, "upper_endpoint": high_spread},
            },
        }
    return result


def build_evidence_record(
    manifest: dict[str, Any],
    raw_path: Path,
    protocol: dict[str, Any],
    *,
    root: Path = ROOT,
    manifest_path: Path | None = None,
    hosts_doc: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if manifest.get("protocol_id") != protocol.get("id") or manifest.get("conclusion_id") != protocol.get("conclusion_id"):
        raise ValueError("run manifest does not match the registered conclusion protocol")
    if manifest.get("role") != "report" or manifest.get("revision") != protocol.get("report_revision"):
        raise ValueError("report evidence must use the protocol's report revision")
    if manifest.get("worktree_head") != manifest.get("revision"):
        raise ValueError("report evidence worktree HEAD differs from its revision")
    if manifest.get("harness_overlays") != protocol.get("harness_overlays", []):
        raise ValueError("run manifest overlays differ from the registered protocol")
    hosts_doc = hosts_doc or tomllib.loads((root / "contracts/hosts.toml").read_text(encoding="utf-8"))
    host_rows = hosts_doc.get("host", [])
    host = next((item for item in host_rows if item.get("id") == manifest.get("host_id")), None)
    if host is None or manifest.get("host_id") != protocol.get("host_id") or manifest.get("a5_settings") != host.get("a5"):
        raise ValueError("run manifest host or A-5 settings differ from the registered reference")
    toolchain = manifest.get("toolchain", {})
    rustc = _parse_rustc_vv(toolchain.get("rustc_vv"))
    if rustc is None or toolchain.get("rustc") != rustc["first_line"]:
        raise ValueError("run manifest lacks an exact rustc -Vv identity")
    for overlay in manifest.get("harness_overlays", []):
        if _overlay_bytes(root, overlay) is None:
            raise ValueError(f"run overlay bytes differ from the manifest: {overlay.get('path')}")
    expected_cells = [str(cell) for cell in protocol.get("ordered_cell_ids", [])]
    raw_lines = raw_path.read_text(encoding="utf-8").splitlines()
    if any(line.startswith("SCALE_HOST|") for line in raw_lines):
        results = _normalize_m3_05(raw_lines, protocol, manifest["a5_settings"])
    else:
        decoded = [json.loads(line) for line in raw_lines if line.strip()]
        if any(row.get("record") == "SCALE_HOST" for row in decoded):
            results = _normalize_m3_05(decoded, protocol, manifest["a5_settings"])
        else:
            cells = {}
            for row in decoded:
                if row.get("record") == "A5_CELL_RESULT" or row.get("kind") == "A5_CELL_RESULT":
                    normalized = row
                elif row.get("kind") == "series":
                    normalized = _normalize_series(row)
                else:
                    continue
                cell_id = normalized["cell_id"]
                if cell_id in cells:
                    raise ValueError(f"duplicate A-5 cell result: {cell_id}")
                cells[cell_id] = normalized
            results = cells
    if list(results) != expected_cells:
        raise ValueError("A-5 results must contain the registered ordered cells exactly once")
    settings = manifest["a5_settings"]
    processed = []
    completion = protocol.get("predicate_kind") == "completion"
    for cell_id in expected_cells:
        item = results[cell_id]
        controls = item.get("process_controls")
        if not isinstance(controls, dict) or controls.get("priority_set") is not True or controls.get("affinity_set") is not True:
            raise ValueError(f"A-5 process priority and affinity were not confirmed for {cell_id}")
        if completion:
            if not isinstance(item.get("completed"), bool):
                raise ValueError(f"completion cell must record a boolean result: {cell_id}")
            processed.append({**item, "cell_id": cell_id})
            continue
        validity = item.get("validity")
        observed = item.get("observed")
        if not isinstance(observed, dict) or not isinstance(validity, dict):
            raise ValueError(f"A-5 cell lacks observed values or validity: {cell_id}")
        samples = validity.get("sample_seconds")
        spreads = validity.get("path_mad_over_median")
        repetitions = validity.get("repetitions_per_sample")
        if validity.get("warmups") != settings["warmups"] or validity.get("sample_count") != settings["samples"]:
            raise ValueError(f"A-5 cell warmups/sample count differ from the registered settings: {cell_id}")
        if not isinstance(samples, list) or len(samples) != settings["samples"] or not all(
            isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) for value in samples
        ):
            raise ValueError(f"A-5 cell must record all {settings['samples']} sample durations: {cell_id}")
        if not valid_repetitions_per_sample(repetitions):
            raise ValueError(f"A-5 cell repetition factor is missing or invalid: {cell_id}")
        if not isinstance(spreads, dict) or len(spreads) != 2 or not all(
            isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0
            for value in spreads.values()
        ):
            raise ValueError(f"A-5 cell must record two finite path MAD/median values: {cell_id}")
        is_valid = all(value >= settings["minimum_sample_seconds"] for value in samples) and all(
            value <= settings["max_path_mad_over_median"] for value in spreads.values()
        )
        processed.append({**item, "cell_id": cell_id, "observed": observed, "validity": validity, "valid": is_valid})
    raw = raw_path.resolve()
    raw_relative = raw.relative_to(root.resolve()).as_posix()
    raw_artifact = manifest.get("raw_artifact")
    artifacts = [
        {"path": raw_artifact["path"], "sha256": raw_artifact["sha256"]}
        if isinstance(raw_artifact, dict)
        else {"path": raw_relative, "sha256": _digest(raw)}
    ]
    if manifest_path is not None:
        manifest_file = manifest_path.resolve()
        artifacts.append({"path": manifest_file.relative_to(root.resolve()).as_posix(), "sha256": _digest(manifest_file)})
    source_summary = manifest.get("source_summary")
    if isinstance(source_summary, dict):
        artifacts.append({"path": source_summary["path"], "sha256": source_summary["sha256"]})
    record = {
        "id": f"{manifest['run_id']}-report",
        "conclusion_id": manifest["conclusion_id"],
        "role": "report",
        "kind": "a5_report_revision",
        "revision": manifest["revision"],
        "toolchain": manifest["toolchain"],
        "protocol_id": manifest["protocol_id"],
        "date": manifest["date"],
        "commands": manifest.get("commands", []),
        "notes": manifest.get("notes", []),
        "artifacts": artifacts,
        "harness_overlays": manifest.get("harness_overlays", []),
        "overlay_build_ok": True,
        "cell_results": processed,
        "raw_outputs": [{"path": raw_relative, "sha256": _digest(raw)}],
    }
    if manifest.get("supersedes"):
        record["supersedes"] = manifest["supersedes"]
    return record


def append_evidence(record: dict[str, Any], path: Path = EVIDENCE) -> None:
    existing = path.read_text(encoding="utf-8") if path.exists() else ""
    for line in existing.splitlines():
        if line and json.loads(line).get("id") == record["id"]:
            raise ValueError(f"evidence id already exists: {record['id']}")
    new_line = json.dumps(record, separators=(",", ":"), sort_keys=True) + "\n"
    validate_evidence_append_only(existing.encode("utf-8"), (existing + new_line).encode("utf-8"))
    write_text_lf(path, new_line, append=True, flush=True)


def _report_text(protocol: dict[str, Any], root: Path) -> str:
    result = _git(root, "show", f"{protocol['report_revision']}:{protocol['report_path_at_revision']}")
    if result.returncode:
        raise ValueError(f"could not read report source for {protocol['id']}")
    return result.stdout.decode("utf-8")


def _table_cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def _median_us(value: str) -> float:
    match = re.match(r"\s*([\d,]+(?:\.\d+)?)", value)
    if match is None:
        raise ValueError(f"report cell is not a numeric timing: {value!r}")
    return float(match.group(1).replace(",", ""))


def _population_value(value: str) -> int:
    normalized = value.replace(",", "").strip().lower()
    if normalized.endswith("k"):
        return int(float(normalized[:-1]) * 1_000)
    if normalized.endswith("m"):
        return int(float(normalized[:-1]) * 1_000_000)
    return int(normalized)


def _setup_build_seconds(root: Path, population: int, degree: int) -> float:
    text = (root / "docs/reports/M3_05_SCALE_LIMIT.md").read_text(encoding="utf-8")
    for line in text.splitlines():
        if not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 7 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[2]) == degree:
            return sum(float(value.replace(",", "")) for value in cells[3:6]) / 1_000_000.0
    raise ValueError(f"M3-05 setup row is missing for N={population}, k={degree}")


def _m3_01_graph_projection_seconds(root: Path, population: int, degree: int) -> float:
    text = (root / "docs/reports/M3_01_SERIAL_BASELINE.md").read_text(encoding="utf-8")
    in_section = False
    for line in text.splitlines():
        if line.startswith("### "):
            in_section = line == "### One-time graph and projection costs"
            continue
        if not in_section or not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 6 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[2]) == degree and float(cells[3]) == 0.5:
            return (_median_us(cells[4]) + _median_us(cells[5])) / 1_000_000.0
    raise ValueError(f"M3-01 graph/projection row is missing for N={population}, k={degree}, p_local=0.5")


def _m3_08_checkpoint_seconds(root: Path, population: int, days: int, interval: int) -> float:
    text = (root / "docs/reports/M3_08_POPULATION_DYNAMICS.md").read_text(encoding="utf-8")
    normal = compaction = None
    for line in text.splitlines():
        if not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 8 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[1]) == interval:
            normal = _median_us(cells[4]) / 1_000_000.0
            compaction = _median_us(cells[5]) / 1_000_000.0
            break
    if normal is None or compaction is None:
        raise ValueError(f"M3-08 normal/compaction timing row is missing for N={population}, interval={interval}")
    compaction_days = sum(day % interval == 0 for day in range(days))
    return (days - compaction_days) * normal + compaction_days * compaction


def _series_estimate(
    pair_seconds: float,
    settings: dict[str, Any],
    *,
    probe_count: int,
    setup_per_operation_seconds: float = 0.0,
) -> dict[str, float | int]:
    target = float(settings["sample_target_seconds"])
    operation_seconds = pair_seconds + setup_per_operation_seconds
    repetitions = max(1, math.ceil(target / operation_seconds))
    sample = repetitions * operation_seconds
    calibration = probe_count * operation_seconds
    warmup = int(settings["warmups"]) * sample
    timed = int(settings["samples"]) * sample
    return {
        "pair_seconds": pair_seconds,
        "setup_per_operation_seconds": setup_per_operation_seconds,
        "estimated_operation_wall_seconds": operation_seconds,
        "repetitions_per_sample": repetitions,
        "sample_seconds": sample,
        "calibration_seconds": calibration,
        "warmup_seconds": warmup,
        "timed_seconds": timed,
        "total_seconds": calibration + warmup + timed,
    }


def _m3_05_setup_seconds(root: Path, population: int, degree: int) -> float:
    text = (root / "docs/reports/M3_05_SCALE_LIMIT.md").read_text(encoding="utf-8")
    for line in text.splitlines():
        if not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 7 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[2]) == degree:
            return sum(float(value.replace(",", "")) for value in cells[3:6]) / 1_000_000.0
    raise ValueError(f"M3-05 setup row is missing for N={population}, k={degree}")


def _m3_01_graph_projection_seconds(root: Path, population: int, degree: int) -> float:
    text = (root / "docs/reports/M3_01_SERIAL_BASELINE.md").read_text(encoding="utf-8")
    in_section = False
    for line in text.splitlines():
        if line.startswith("### "):
            in_section = line == "### One-time graph and projection costs"
            continue
        if not in_section or not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 6 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[2]) == degree and float(cells[3]) == 0.5:
            return (_median_us(cells[4]) + _median_us(cells[5])) / 1_000_000.0
    raise ValueError(f"M3-01 graph/projection row is missing for N={population}, k={degree}, p_local=0.5")


def _m3_01_tick_seconds(root: Path, population: int, degree: int) -> float:
    text = (root / "docs/reports/M3_01_SERIAL_BASELINE.md").read_text(encoding="utf-8")
    in_section = False
    for line in text.splitlines():
        if line.startswith("### "):
            in_section = line == "### Full-tick samples"
            continue
        if not in_section or not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 5 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[2]) == degree and float(cells[3]) == 0.5:
            return _median_us(cells[4]) / 1_000_000.0
    raise ValueError(f"M3-01 full-tick row is missing for N={population}, k={degree}, p_local=0.5")


def _m3_08_checkpoint_seconds(root: Path, population: int, days: int, interval: int) -> float:
    text = (root / "docs/reports/M3_08_POPULATION_DYNAMICS.md").read_text(encoding="utf-8")
    for line in text.splitlines():
        if not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 8 and cells[0].replace(",", "").isdigit() and int(cells[0].replace(",", "")) == population and int(cells[1]) == interval:
            normal = _median_us(cells[4]) / 1_000_000.0
            compaction = _median_us(cells[5]) / 1_000_000.0
            compaction_days = sum(day % interval == 0 for day in range(days))
            return (days - compaction_days) * normal + compaction_days * compaction
    raise ValueError(f"M3-08 normal/compaction timing row is missing for N={population}, interval={interval}")


def _report_section(text: str, heading: str) -> str:
    start = text.find(heading)
    if start < 0:
        raise ValueError(f"report section is missing: {heading}")
    tail = text[start + len(heading):]
    end = tail.find("\n### ")
    return tail if end < 0 else tail[:end]


def tonight_plan(root: Path = ROOT) -> list[dict[str, Any]]:
    """Estimate tonight's 16 passed-gate cells from report data and setup costs."""
    experiments = tomllib.loads((root / "contracts/experiments.toml").read_text(encoding="utf-8"))["experiment"]
    by_id = {item["id"]: item for item in experiments}
    hosts = tomllib.loads((root / "contracts/hosts.toml").read_text(encoding="utf-8"))["host"]
    host = next(item for item in hosts if item.get("reference") is True)
    settings = host["a5"]
    rows: list[dict[str, Any]] = []

    m2 = by_id["m2_explicit_rayon_speedup"]
    m2_text = _report_text(m2, root)
    m2_pairs: dict[str, float] = {}
    for line in m2_text.splitlines():
        if not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) != 7 or cells[0] not in {"A", "B"} or "/" not in cells[1]:
            continue
        n = _population_value(cells[1].split("/", 1)[0])
        m2_pairs[f"{cells[0]}-N{n}"] = (_median_us(cells[2]) + _median_us(cells[4])) / 1_000_000.0
    for cell_id in m2["ordered_cell_ids"]:
        pair = 3.0 * m2_pairs[cell_id]
        series = _series_estimate(pair, settings, probe_count=3)
        setup = 1.0
        rows.append({"cell_id": cell_id, "protocol_id": m2["id"], "series": [series], "setup_seconds": setup,
                     "setup_basis": "1-second process/config setup reserve; three-day API calls repeat from prepared states.",
                     "estimated_seconds": float(series["total_seconds"]) + setup})

    m3_11_phase = by_id["m3_11_phase9c_speedup"]
    m3_11_tick = by_id["m3_11_whole_tick_speedup"]
    m3_11_text = _report_text(m3_11_phase, root)
    phase_section = _report_section(m3_11_text, "### Phase 9c — paired interleaved")
    phase_cells = next(
        _table_cells(line)
        for line in phase_section.splitlines()
        if line.startswith("|")
        and len(_table_cells(line)) == 12
        and _table_cells(line)[:5] == ["100000", "500", "16", "16", "0.20"]
    )
    phase_pair = (_median_us(phase_cells[7]) + _median_us(phase_cells[11])) / 1_000_000.0
    setup_graph = _m3_05_setup_seconds(root, 100_000, 16)
    setup_base_tick = _m3_01_tick_seconds(root, 100_000, 16)
    phase_series = _series_estimate(
        phase_pair,
        settings,
        probe_count=3,
        setup_per_operation_seconds=setup_base_tick,
    )
    phase_setup = setup_graph + setup_base_tick + 1.0
    rows.append({
        "cell_id": m3_11_phase["ordered_cell_ids"][0],
        "protocol_id": m3_11_phase["id"],
        "series": [phase_series],
        "setup_seconds": phase_setup,
        "setup_basis": "M3-05 measured N=100k,k=16 initialization+graph+projection; one M3 base-day setup; 1-second pool/config reserve. The M3-01 tick is a proxy for repeated propagation-state reset, whose cost is not separately reported.",
        "estimated_seconds": float(phase_series["total_seconds"]) + phase_setup,
    })

    tick_text = _report_text(m3_11_tick, root)
    for cell_id, schedule, label in [
        (m3_11_tick["ordered_cell_ids"][0], "interleaved", "### Whole tick — paired interleaved"),
        (m3_11_tick["ordered_cell_ids"][1], "noninterleaved", "### Whole tick — paired noninterleaved"),
    ]:
        section = _report_section(tick_text, label)
        tick_cells = next(
            _table_cells(line)
            for line in section.splitlines()
            if line.startswith("|")
            and len(_table_cells(line)) == 9
            and _table_cells(line)[:6] == ["100000", "500", "16", "16", "0.20", "6"]
        )
        pair = 3.0 * (_median_us(tick_cells[6]) + _median_us(tick_cells[7])) / 1_000_000.0
        tick_series = _series_estimate(
            pair,
            settings,
            probe_count=6 if schedule == "noninterleaved" else 3,
            setup_per_operation_seconds=2.0 * setup_graph,
        )
        setup = 1.0
        rows.append({
            "cell_id": cell_id,
            "protocol_id": m3_11_tick["id"],
            "series": [tick_series],
            "setup_seconds": setup,
            "setup_basis": "1-second pool/config reserve; repeated fresh-world graph/projection setup is estimated per operation from M3-05 N=100k,k=16 measurements.",
            "estimated_seconds": float(tick_series["total_seconds"]) + setup,
        })

    m3_09 = by_id["m3_09_incremental_equal_live"]
    m3_09_text = _report_text(m3_09, root)
    equal_live = _report_section(m3_09_text, "### Equal-live M3P/M3 rerun and interleaving")
    equal_rows = {}
    for line in equal_live.splitlines():
        if not line.startswith("|"):
            continue
        cells = _table_cells(line)
        if len(cells) == 6 and cells[0].replace(",", "").isdigit() and cells[1] in {"Paired", "Non-interleaved"}:
            n = int(cells[0].replace(",", ""))
            schedule = "paired" if cells[1] == "Paired" else "noninterleaved"
            equal_rows[(n, schedule)] = [_median_us(value) for value in cells[2:5]]
    for cell_id in m3_09["ordered_cell_ids"]:
        population = 10_000 if cell_id.startswith("N10k-") else 100_000
        schedule = "paired" if cell_id.endswith("-paired") else "noninterleaved"
        values = equal_rows[(population, schedule)]
        pair = 3.0 * sum(values) / 1_000_000.0
        context_setup = 3.0 * _m3_01_graph_projection_seconds(root, population, 8)
        series = _series_estimate(
            pair,
            settings,
            probe_count=3 if schedule == "paired" else 9,
            setup_per_operation_seconds=context_setup,
        )
        checkpoint = _m3_08_checkpoint_seconds(root, population, 400, 50)
        parity_call_setup = pair + context_setup
        setup = checkpoint + parity_call_setup + 1.0
        rows.append({
            "cell_id": cell_id,
            "protocol_id": m3_09["id"],
            "series": [series],
            "setup_seconds": setup,
            "setup_basis": f"400-day M3-08 M3P checkpoint at N={population} (normal/compaction report medians), one three-day parity pass, plus 1-second process/config reserve.",
            "estimated_seconds": float(series["total_seconds"]) + setup,
        })

    m4 = by_id["m4_07_case_a_search_parallel_speedup"]
    line = next((line for line in _report_text(m4, root).splitlines() if "Exhaustive case (a), R=10, H=90" in line and line.startswith("|")), None)
    if line is None:
        raise ValueError("M4-07 report-revision case-a timing row is missing")
    cells = _table_cells(line)
    pair = float(cells[1].split("s", 1)[0].replace(",", "")) + float(cells[2].split("s", 1)[0].replace(",", ""))
    series = _series_estimate(pair, settings, probe_count=1)
    setup = 1.0
    rows.append({"cell_id": m4["ordered_cell_ids"][0], "protocol_id": m4["id"], "series": [series], "setup_seconds": setup,
                 "setup_basis": "1-second planning reserve; the report does not time branch-cache setup separately.",
                 "estimated_seconds": float(series["total_seconds"]) + setup})

    if len(rows) != 16:
        raise ValueError(f"expected 16 passed-gate report-revision cells for tonight; found {len(rows)}")
    evidence_path = root / "contracts/evidence.jsonl"
    if evidence_path.is_file():
        for line in evidence_path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            if record.get("role") != "report" or record.get("overlay_build_ok") is not False:
                continue
            failures = {
                str(result.get("cell_id")): str(result.get("reason", "overlay failed"))
                for result in record.get("cell_results", [])
                if isinstance(result, dict) and result.get("overlay_build_ok") is False
            }
            for row in rows:
                if row["protocol_id"] == record.get("protocol_id") and row["cell_id"] in failures:
                    row["blocked_reason"] = failures[row["cell_id"]]
    return rows


def print_tonight_plan(root: Path = ROOT) -> None:
    rows = tonight_plan(root)
    total = sum(row["estimated_seconds"] for row in rows)
    print("A-5 passed-gate report-revision cells only: M2 explicit Rayon (8), M4 search parallel (1), M3-11 (3), M3-09 (4). M3-05 pilot is already complete.")
    for row in rows:
        series = row["series"]
        details = "; ".join(
            f"pair={float(item['pair_seconds'])*1000:.3f}ms + setup/op={float(item['setup_per_operation_seconds'])*1000:.3f}ms "
            f"r={item['repetitions_per_sample']} "
            f"sample={float(item['sample_seconds']):.3f}s x41 + warmup {float(item['warmup_seconds']):.3f}s + calibration {float(item['calibration_seconds']):.3f}s"
            for item in series
        )
        blocked = f"; BLOCKED: {row['blocked_reason']}" if row.get("blocked_reason") else ""
        print(f"{row['protocol_id']} / {row['cell_id']}: {details}; setup {row['setup_seconds']:.3f}s ({row['setup_basis']}) -> {row['estimated_seconds']:.1f}s{blocked}")
    print(f"Total: {total:.1f}s ({total / 3600:.2f}h) across {len(rows)} cells; setup includes the reported checkpoint/graph costs and setup proxies above, but excludes recalibration retries and clean-worktree compilation.")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    modes.add_parser("dry-run", help="estimate 16 passed-gate report-revision cells without sampling")
    manifest_parser = modes.add_parser("manifest")
    manifest_parser.add_argument("protocol_id")
    manifest_parser.add_argument("run_id")
    manifest_parser.add_argument("output", type=Path)
    manifest_parser.add_argument("--command", action="append", default=[])
    ingest_parser = modes.add_parser("ingest")
    ingest_parser.add_argument("manifest", type=Path)
    ingest_parser.add_argument("raw", type=Path)
    retro_parser = modes.add_parser("retrospective-manifest")
    retro_parser.add_argument("protocol_id")
    retro_parser.add_argument("run_id")
    retro_parser.add_argument("summary", type=Path)
    retro_parser.add_argument("raw", type=Path)
    retro_parser.add_argument("harness_commit")
    retro_parser.add_argument("output", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.mode == "dry-run":
            print_tonight_plan()
            return 0
        experiments = tomllib.loads(EXPERIMENTS.read_text(encoding="utf-8"))
        protocols = {item["id"]: item for item in experiments.get("experiment", [])}
        if args.mode == "manifest":
            protocol = protocols[args.protocol_id]
            manifest = build_run_manifest(protocol, run_id=args.run_id, commands=args.command)
            output = _repo_file(ROOT, args.output.as_posix())
            if output is None:
                raise ValueError("manifest output must stay inside the repository")
            write_text_lf(output, json.dumps(manifest, separators=(",", ":"), sort_keys=True) + "\n")
            print(f"wrote A-5 run manifest: {output.relative_to(ROOT).as_posix()}")
        elif args.mode == "retrospective-manifest":
            protocol = protocols[args.protocol_id]
            manifest = build_retrospective_manifest(
                protocol,
                run_id=args.run_id,
                summary_path=args.summary,
                raw_path=args.raw,
                harness_commit=args.harness_commit,
            )
            output = _repo_file(ROOT, args.output.as_posix())
            if output is None:
                raise ValueError("manifest output must stay inside the repository")
            write_text_lf(output, json.dumps(manifest, separators=(",", ":"), sort_keys=True) + "\n")
            print(f"wrote retrospective A-5 manifest: {output.relative_to(ROOT).as_posix()}")
        else:
            manifest_path = _repo_file(ROOT, args.manifest.as_posix())
            raw_path = _repo_file(ROOT, args.raw.as_posix())
            if manifest_path is None or raw_path is None:
                raise ValueError("manifest and raw output paths must stay inside the repository")
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            protocol = protocols[manifest["protocol_id"]]
            record = build_evidence_record(manifest, raw_path, protocol, manifest_path=manifest_path)
            append_evidence(record)
            print(f"appended A-5 report evidence: {record['id']}")
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"A-5 evidence error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
