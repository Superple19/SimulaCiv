"""Prepare, run, summarize, and ingest the registered report-revision A-5 cells."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .a5_evidence import (
    append_evidence,
    build_evidence_record,
    build_run_manifest,
    tonight_plan,
)
from .conclusions import EXPERIMENTS, ROOT
from .io import write_text_lf


PIPELINE = "docs/data/m5_08/dvc.yaml"
DATA = ROOT / "docs" / "data" / "m5_08"
SCRATCH = ROOT / "target" / "a5_tonight"
BUILD = SCRATCH / "cargo-target"
GROUPS: dict[str, dict[str, Any]] = {
    "m2": {
        "stage": "a5_m2_explicit_rayon",
        "protocols": ["m2_explicit_rayon_speedup"],
        "raw": {"m2_explicit_rayon_speedup": "m2_explicit_rayon/raw/records.jsonl"},
        "temp": "m2_explicit_rayon.jsonl",
        "example": "m5_06_m2_rayon_a5",
    },
    "m3_09": {
        "stage": "a5_m3_09_equal_live",
        "protocols": ["m3_09_incremental_equal_live"],
        "raw": {"m3_09_incremental_equal_live": "m3_09_equal_live/raw/records.jsonl"},
        "temp": "m3_09_equal_live.jsonl",
        "example": "m5_05_m3_09_revision",
    },
    "m3_11": {
        "stage": "a5_m3_11_propagation",
        "protocols": ["m3_11_phase9c_speedup", "m3_11_whole_tick_speedup"],
        "raw": {
            "m3_11_phase9c_speedup": "m3_11/raw/m3_11_phase9c_speedup.jsonl",
            "m3_11_whole_tick_speedup": "m3_11/raw/m3_11_whole_tick_speedup.jsonl",
        },
        "temp": "m3_11_all.jsonl",
        "example": "m3_11_propagation_bench",
    },
    "m4": {
        "stage": "a5_m4_search_parallel",
        "protocols": ["m4_07_case_a_search_parallel_speedup"],
        "raw": {"m4_07_case_a_search_parallel_speedup": "m4_search_parallel/raw/records.jsonl"},
        "temp": "m4_search_parallel.jsonl",
        "example": "m5_06_m4_parallel_a5",
    },
}
ORDER = ("m2", "m3_09", "m3_11", "m4")


def _protocols(root: Path = ROOT) -> dict[str, dict[str, Any]]:
    document = tomllib.loads((root / "contracts" / "experiments.toml").read_text(encoding="utf-8"))
    all_protocols = {item["id"]: item for item in document["experiment"]}
    needed = {protocol for group in GROUPS.values() for protocol in group["protocols"]}
    return {key: all_protocols[key] for key in needed}


def _manifest_path(protocol_id: str, root: Path = ROOT) -> Path:
    return root / "docs" / "data" / "m5_08" / "manifests" / f"{protocol_id}.json"


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _exceeds_invalid_limit(invalid_count: int, expected_count: int) -> bool:
    return expected_count > 0 and invalid_count / expected_count > 0.10


def _available_protocol_ids(preflight: dict[str, Any]) -> set[str]:
    return {
        protocol_id
        for group_id in preflight.get("prepared_groups", [])
        for protocol_id in GROUPS[group_id]["protocols"]
    }


def _overlay_failure_record(
    manifest: dict[str, Any], protocol: dict[str, Any], artifact_path: str, artifact_sha256: str
) -> dict[str, Any]:
    return {
        "id": f"{manifest['run_id']}-overlay-build-failed",
        "conclusion_id": protocol["conclusion_id"],
        "role": "report",
        "kind": "a5_report_revision",
        "revision": protocol["report_revision"],
        "toolchain": manifest["toolchain"],
        "protocol_id": protocol["id"],
        "date": manifest["date"],
        "commands": manifest["commands"],
        "notes": ["Report-revision overlay failed to build before any A-5 samples; timing was not run."],
        "artifacts": [{"path": artifact_path, "sha256": artifact_sha256}],
        "harness_overlays": manifest["harness_overlays"],
        "overlay_build_ok": False,
        "cell_results": [
            {"cell_id": cell_id, "overlay_build_ok": False}
            for cell_id in protocol["ordered_cell_ids"]
        ],
    }


def _git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def _worktree(report_revision: str) -> Path:
    path = SCRATCH / "worktrees" / report_revision
    if path.exists():
        if _git(path, "rev-parse", "HEAD") != report_revision:
            raise ValueError(f"unexpected report worktree revision at {path}")
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "worktree", "add", "--detach", str(path), report_revision], cwd=ROOT, check=True)
    return path


def _apply_overlays(worktree: Path, protocol: dict[str, Any]) -> list[dict[str, str]]:
    from .a5_evidence import _overlay_bytes

    overlays = protocol.get("harness_overlays", [])
    for overlay in overlays:
        path = worktree.joinpath(*Path(overlay["path"]).parts)
        content = _overlay_bytes(ROOT, overlay)
        if content is None or _sha256(content) != overlay["sha256"]:
            raise ValueError(f"registered overlay bytes are missing or changed: {overlay['path']}")
        current = path.read_bytes() if path.is_file() else None
        if current != content:
            report_blob = subprocess.run(
                ["git", "show", f"{protocol['report_revision']}:{overlay['path']}"],
                cwd=ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if path.exists() and (report_blob.returncode != 0 or current != report_blob.stdout):
                raise ValueError(f"refusing to overwrite unregistered worktree content: {overlay['path']}")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        if _sha256(path.read_bytes()) != overlay["sha256"]:
            raise ValueError(f"overlay verification failed in report worktree: {overlay['path']}")
    return overlays


def _cargo_env(report_revision: str) -> dict[str, str]:
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(BUILD / report_revision)
    return environment


def _smoke_worktree(group_id: str, worktree: Path) -> list[str]:
    spec = GROUPS[group_id]
    temp = SCRATCH / "smoke"
    temp.mkdir(parents=True, exist_ok=True)
    example = spec["example"]
    report_revision = _protocols()[spec["protocols"][0]]["report_revision"]
    env = _cargo_env(report_revision)
    if group_id == "m3_11":
        cells = [
            "N100000-k16-hops16-transmit0.2-pool6-day0",
            "large-frontier-interleaved-pool6",
            "large-frontier-noninterleaved-pool6",
        ]
        for cell in cells:
            subprocess.run(
                ["cargo", "+1.98.1", "bench", "--locked", "-p", "sim-model", "--bench", example, "--", "--a5-smoke", cell],
                cwd=worktree,
                env=env,
                check=True,
            )
        return cells
    if group_id == "m3_09":
        cell = "N10k-paired"
        subprocess.run(
            ["cargo", "+1.98.1", "run", "--release", "--locked", "-p", "sim-model", "--example", example, "--", "--a5-smoke", cell],
            cwd=worktree,
            env=env,
            check=True,
        )
        return [cell]
    output = temp / f"{group_id}.jsonl"
    args = ["cargo", "+1.98.1", "run", "--release", "--locked", "-p", "sim-model", "--example", example]
    if group_id == "m2":
        args += ["--", "--smoke", f"--output={output}"]
        cells = ["smoke-A-N100"]
    else:
        args += ["--", "--smoke", f"--output={output}"]
        cells = ["smoke-crn-food-zero", "smoke-minimal-cause"]
    subprocess.run(args, cwd=worktree, env=env, check=True)
    return cells


def _commands(group_id: str, worktree: Path, temp_raw: Path) -> list[str]:
    spec = GROUPS[group_id]
    example = spec["example"]
    if group_id == "m2":
        command = f"cargo +1.98.1 run --release --locked -p sim-model --example {example} -- --resume --output={temp_raw}"
    elif group_id == "m3_09":
        command = f"cargo +1.98.1 run --release --locked -p sim-model --example {example} -- --a5-all {temp_raw}"
    elif group_id == "m3_11":
        command = f"cargo +1.98.1 bench --locked -p sim-model --bench {example} -- --a5 {temp_raw}"
    else:
        command = f"cargo +1.98.1 run --release --locked -p sim-model --example {example} -- --cell=case-a-exhaustive-R10-H90 --resume --output={temp_raw}"
    return [command]


def prepare() -> None:
    dirty = _git(ROOT, "status", "--porcelain")
    if dirty:
        raise ValueError("prepare requires a clean Git worktree; commit generated code before creating manifests")
    protocols = _protocols()
    hosts = tomllib.loads((ROOT / "contracts" / "hosts.toml").read_text(encoding="utf-8"))
    worktrees: dict[str, Path] = {}
    for group_id in ORDER:
        for protocol_id in GROUPS[group_id]["protocols"]:
            protocol = protocols[protocol_id]
            revision = protocol["report_revision"]
            worktree = worktrees.setdefault(revision, _worktree(revision))
            _apply_overlays(worktree, protocol)
    preflight: dict[str, Any] = {"prepared_groups": [], "blocked_groups": {}}
    for group_id in ORDER:
        revision = protocols[GROUPS[group_id]["protocols"][0]]["report_revision"]
        worktree = worktrees[revision]
        try:
            smoke_cells = _smoke_worktree(group_id, worktree)
        except (OSError, subprocess.CalledProcessError) as error:
            preflight["blocked_groups"][group_id] = {
                "report_revision": revision,
                "reason": str(error),
            }
            print(f"report-SHA smoke blocked: {group_id}: {error}")
            continue
        preflight["prepared_groups"].append(group_id)
        print(f"report-SHA smoke passed: {group_id}; cells={','.join(smoke_cells)}")

    observation = _host_observation()
    for group_id in ORDER:
        if group_id not in preflight["prepared_groups"]:
            continue
        spec = GROUPS[group_id]
        temp_raw = SCRATCH / "raw" / spec["temp"]
        for protocol_id in spec["protocols"]:
            protocol = protocols[protocol_id]
            worktree = worktrees[protocol["report_revision"]]
            manifest = build_run_manifest(
                protocol,
                run_id=f"m5-08-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}-{protocol_id}",
                root=worktree,
                hosts=hosts,
                commands=_commands(group_id, worktree, temp_raw),
            )
            manifest["host_observation_before"] = observation
            output = _manifest_path(protocol_id)
            write_text_lf(output, json.dumps(manifest, separators=(",", ":"), sort_keys=True) + "\n")
            print(f"manifest ready: {output.relative_to(ROOT).as_posix()} ({protocol['report_revision']})")
    preflight["host_observation_before"] = observation
    write_text_lf(SCRATCH / "preflight.json", json.dumps(preflight, separators=(",", ":"), sort_keys=True) + "\n")


def prepare_group(group_id: str) -> None:
    if group_id not in GROUPS:
        raise ValueError(f"unknown A-5 group: {group_id}")
    if _git(ROOT, "status", "--porcelain"):
        raise ValueError("prepare-group requires a clean Git worktree")
    protocols = _protocols()
    spec = GROUPS[group_id]
    revisions = {protocols[protocol_id]["report_revision"] for protocol_id in spec["protocols"]}
    if len(revisions) != 1:
        raise ValueError(f"all protocols in {group_id} must share one report revision")
    revision = revisions.pop()
    worktree = _worktree(revision)
    for protocol_id in spec["protocols"]:
        _apply_overlays(worktree, protocols[protocol_id])
    preflight_path = SCRATCH / "preflight.json"
    preflight = json.loads(preflight_path.read_text(encoding="utf-8")) if preflight_path.is_file() else {"prepared_groups": [], "blocked_groups": {}}
    try:
        smoke_cells = _smoke_worktree(group_id, worktree)
    except (OSError, subprocess.CalledProcessError) as error:
        preflight["prepared_groups"] = [item for item in preflight.get("prepared_groups", []) if item != group_id]
        preflight.setdefault("blocked_groups", {})[group_id] = {"report_revision": revision, "reason": str(error)}
        write_text_lf(preflight_path, json.dumps(preflight, separators=(",", ":"), sort_keys=True) + "\n")
        print(f"report-SHA smoke blocked: {group_id}: {error}")
        return
    host_observation = _host_observation()
    for protocol_id in spec["protocols"]:
        protocol = protocols[protocol_id]
        manifest = build_run_manifest(
            protocol,
            run_id=f"m5-08-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}-{protocol_id}",
            root=worktree,
            hosts=tomllib.loads((ROOT / "contracts" / "hosts.toml").read_text(encoding="utf-8")),
            commands=_commands(group_id, worktree, SCRATCH / "raw" / spec["temp"]),
        )
        manifest["host_observation_before"] = host_observation
        output = _manifest_path(protocol_id)
        write_text_lf(output, json.dumps(manifest, separators=(",", ":"), sort_keys=True) + "\n")
        print(f"manifest ready: {output.relative_to(ROOT).as_posix()} ({revision})")
    preflight["prepared_groups"] = list(dict.fromkeys([*preflight.get("prepared_groups", []), group_id]))
    preflight.setdefault("blocked_groups", {}).pop(group_id, None)
    preflight["host_observation_before"] = host_observation
    write_text_lf(preflight_path, json.dumps(preflight, separators=(",", ":"), sort_keys=True) + "\n")
    print(f"report-SHA smoke passed: {group_id}; cells={','.join(smoke_cells)}")


def print_dry_run() -> None:
    rows = tonight_plan()
    preflight_path = SCRATCH / "preflight.json"
    blocked: dict[str, Any] = {}
    if preflight_path.is_file():
        preflight = json.loads(preflight_path.read_text(encoding="utf-8"))
        ready = _available_protocol_ids(preflight)
        rows = [row for row in rows if row["protocol_id"] in ready]
        blocked = preflight.get("blocked_groups", {})
    print(f"A-5 report-revision cells available after overlay gate: {len(rows)}")
    for row in rows:
        series = row["series"]
        details = "; ".join(
            f"pair={float(item['pair_seconds'])*1000:.3f}ms + setup/op={float(item['setup_per_operation_seconds'])*1000:.3f}ms "
            f"r={item['repetitions_per_sample']} sample={float(item['sample_seconds']):.3f}s x41 "
            f"+ warmup {float(item['warmup_seconds']):.3f}s + calibration {float(item['calibration_seconds']):.3f}s"
            for item in series
        )
        print(f"{row['protocol_id']} / {row['cell_id']}: {details}; setup {row['setup_seconds']:.3f}s -> {row['estimated_seconds']:.1f}s")
    if blocked:
        for group_id, details in blocked.items():
            print(f"BLOCKED {group_id} at {details['report_revision']}: {details['reason']}")
    total = sum(row["estimated_seconds"] for row in rows)
    print(f"Total estimated time: {total:.1f}s ({total / 3600:.2f}h); excludes recalibration retries and clean-worktree compilation.")


def _host_observation() -> dict[str, Any]:
    if sys.platform == "win32":
        script = (
            "$p=Get-CimInstance Win32_Processor; $o=Get-CimInstance Win32_OperatingSystem; "
            "$s=Get-CimInstance Win32_ComputerSystem; "
            "[pscustomobject]@{cpu_load=($p | Measure-Object -Property LoadPercentage -Average).Average; "
            "free_ram_kib=$o.FreePhysicalMemory; cpu=$p[0].Name; physical=($p | Measure-Object NumberOfCores -Sum).Sum; "
            "logical=$s.NumberOfLogicalProcessors; os=$o.Caption; build=$o.BuildNumber} | ConvertTo-Json -Compress"
        )
        raw = subprocess.check_output(["powershell", "-NoProfile", "-Command", script], text=True)
        return json.loads(raw)
    return {"cpu_load": None, "free_ram_bytes": None, "platform": sys.platform}


def _split_by_cell(data: bytes, expected: dict[str, list[str]]) -> dict[str, bytes]:
    outputs = {cell: bytearray() for cells in expected.values() for cell in cells}
    for line in data.splitlines(keepends=True):
        if not line.strip():
            continue
        record = json.loads(line)
        cell_id = record.get("cell_id")
        if cell_id not in outputs:
            raise ValueError(f"unexpected M3-11 raw cell record: {cell_id!r}")
        outputs[cell_id].extend(line)
    result: dict[str, bytes] = {}
    for protocol_id, cells in expected.items():
        selected = {cell: bytes(outputs[cell]) for cell in cells if outputs[cell]}
        result[protocol_id] = b"".join(selected[cell] for cell in cells if cell in selected)
    return result


def _stage_command(group_id: str) -> int:
    spec = GROUPS[group_id]
    protocols = _protocols()
    for protocol_id in spec["protocols"]:
        if not _manifest_path(protocol_id).is_file():
            raise ValueError(f"blocked by report-SHA preflight; no manifest for {protocol_id}")
    report_revision = protocols[spec["protocols"][0]]["report_revision"]
    worktree = SCRATCH / "worktrees" / report_revision
    if not worktree.is_dir() or _git(worktree, "rev-parse", "HEAD") != report_revision:
        raise ValueError(f"report-SHA worktree is unavailable for {group_id}")
    temp_raw = SCRATCH / "raw" / spec["temp"]
    temp_raw.parent.mkdir(parents=True, exist_ok=True)
    command = _commands(group_id, worktree, temp_raw)[0]
    started = datetime.now(timezone.utc).isoformat(timespec="seconds")
    start = time.perf_counter()
    try:
        args = command.split()
        result = subprocess.run(args, cwd=worktree, env=_cargo_env(report_revision), check=False)
        complete = result.returncode == 0
        error = None if complete else f"harness exited with status {result.returncode}"
    except OSError as failure:
        complete = False
        error = f"could not execute harness: {failure}"
    elapsed = time.perf_counter() - start

    output_root = ROOT / "docs" / "data" / "m5_08"
    status: dict[str, Any] = {
        "schema_version": 1,
        "group": group_id,
        "complete": complete,
        "started_at": started,
        "finished_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "wall_seconds": elapsed,
        "command": command,
        "error": error,
        "completed_cells": [],
        "host_observation_after": _host_observation(),
    }
    if group_id == "m3_11":
        expected = {protocol_id: protocols[protocol_id]["ordered_cell_ids"] for protocol_id in spec["protocols"]}
        split = _split_by_cell(temp_raw.read_bytes(), expected) if temp_raw.exists() else {key: b"" for key in expected}
        all_cells = []
        for protocol_id, payload in split.items():
            if payload:
                relative = spec["raw"][protocol_id]
                path = output_root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(payload)
                all_cells.extend(json.loads(line)["cell_id"] for line in payload.splitlines() if line.strip())
        status["completed_cells"] = all_cells
    elif temp_raw.exists():
        raw_relative = spec["raw"][spec["protocols"][0]]
        out = output_root / raw_relative
        out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(temp_raw, out)
        rows = [json.loads(line) for line in temp_raw.read_text(encoding="utf-8").splitlines() if line.strip()]
        status["completed_cells"] = [str(row["cell_id"]) for row in rows if isinstance(row, dict) and row.get("cell_id")]

    # A partial run is still captured as immutable DVC evidence; the summary prevents ingestion.
    stage_out = output_root / {
        "m2": "m2_explicit_rayon/raw",
        "m3_09": "m3_09_equal_live/raw",
        "m3_11": "m3_11/raw",
        "m4": "m4_search_parallel/raw",
    }[group_id]
    write_text_lf(stage_out / "stage_status.json", json.dumps(status, separators=(",", ":"), sort_keys=True) + "\n")
    print(f"stage group={group_id} complete={complete} wall={elapsed:.1f}s cells={len(status['completed_cells'])}")
    return 0


def _freeze_stage(stage_name: str) -> None:
    pipeline = ROOT / PIPELINE
    text = pipeline.read_text(encoding="utf-8")
    lines = text.splitlines()
    start = next((i for i, line in enumerate(lines) if line == f"  {stage_name}:"), None)
    if start is None:
        raise ValueError(f"stage is missing from {PIPELINE}: {stage_name}")
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("  ") and not lines[i].startswith("    ")), len(lines))
    if any(line.strip() == "frozen: true" for line in lines[start:end]):
        return
    insert_at = next((i for i in range(start + 1, end) if lines[i].strip() == "meta:"), end)
    lines.insert(insert_at, "    frozen: true")
    write_text_lf(pipeline, "\n".join(lines) + "\n")


def _summary_path(protocol_id: str) -> Path:
    return DATA / protocol_id / "summary.json"


def _read_status(group_id: str) -> dict[str, Any]:
    path = DATA / {
        "m2": "m2_explicit_rayon/raw/stage_status.json",
        "m3_09": "m3_09_equal_live/raw/stage_status.json",
        "m3_11": "m3_11/raw/stage_status.json",
        "m4": "m4_search_parallel/raw/stage_status.json",
    }[group_id]
    return json.loads(path.read_text(encoding="utf-8"))


def _build_protocol_summary(group_id: str, protocol_id: str, status: dict[str, Any]) -> dict[str, Any]:
    protocol = _protocols()[protocol_id]
    spec = GROUPS[group_id]
    raw_path = DATA / spec["raw"][protocol_id]
    manifest_path = _manifest_path(protocol_id)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    raw_exists = raw_path.is_file()
    raw_digest = _sha256(raw_path.read_bytes()) if raw_exists else None
    expected = protocol["ordered_cell_ids"]
    completed = status.get("complete") is True and all(cell in status.get("completed_cells", []) for cell in expected)
    cells: list[dict[str, Any]] = []
    if completed:
        record = build_evidence_record(manifest, raw_path, protocol, root=ROOT, manifest_path=manifest_path)
        raw_rows = [json.loads(line) for line in raw_path.read_text(encoding="utf-8").splitlines() if line.strip()]
        recalibration_by_cell = {
            row.get("cell_id"): row.get("series", {}).get("recalibrations", [])
            for row in raw_rows
            if isinstance(row, dict) and row.get("kind") == "series"
        }
        cells = [
            {
                "cell_id": row["cell_id"],
                "valid": row.get("valid"),
                "observed": row.get("observed"),
                "validity": row.get("validity"),
                "recalibrations": recalibration_by_cell.get(row["cell_id"], []),
            }
            for row in record["cell_results"]
        ]
    invalid_cells = [row["cell_id"] for row in cells if row["valid"] is False]
    validity_limit = _exceeds_invalid_limit(len(invalid_cells), len(expected))
    return {
        "schema_version": 1,
        "protocol_id": protocol_id,
        "conclusion_id": protocol["conclusion_id"],
        "report_revision": protocol["report_revision"],
        "run_id": manifest["run_id"],
        "status": "validity_limit" if completed and validity_limit else "complete" if completed else "incomplete",
        "results_complete": completed,
        "stage_error": status.get("error"),
        "group_wall_seconds": status["wall_seconds"],
        "manifest_path": manifest_path.relative_to(ROOT).as_posix(),
        "manifest_sha256": _sha256(manifest_path.read_bytes()),
        "raw_path": raw_path.relative_to(ROOT).as_posix(),
        "raw_sha256": raw_digest,
        "expected_cells": expected,
        "completed_cells": status.get("completed_cells", []),
        "validity_failures": invalid_cells,
        "recalibrations": {row["cell_id"]: row["recalibrations"] for row in cells},
        "cells": cells,
    }


def _run_group(group_id: str) -> None:
    if group_id not in GROUPS:
        raise ValueError(f"unknown A-5 group: {group_id}")
    stage_name = GROUPS[group_id]["stage"]
    missing = [protocol_id for protocol_id in GROUPS[group_id]["protocols"] if not _manifest_path(protocol_id).is_file()]
    if missing:
        raise ValueError(f"report-SHA preflight blocked {group_id}: {', '.join(missing)}")
    start = time.perf_counter()
    subprocess.run(["dvc", "repro", f"{PIPELINE}:{stage_name}"], cwd=ROOT, check=True)
    elapsed = time.perf_counter() - start
    _freeze_stage(stage_name)
    status = _read_status(group_id)
    status["orchestrator_wall_seconds"] = elapsed
    for protocol_id in GROUPS[group_id]["protocols"]:
        summary = _build_protocol_summary(group_id, protocol_id, status)
        write_text_lf(_summary_path(protocol_id), json.dumps(summary, separators=(",", ":"), sort_keys=True) + "\n")
        state = summary["status"]
        invalid = len(summary["validity_failures"])
        print(f"summary protocol={protocol_id} status={state} invalid_cells={invalid}/{len(summary['expected_cells'])}")
    if not status.get("complete"):
        print(f"preserved incomplete stage raw output for {group_id}; later groups may proceed")


def _ingest(group_id: str) -> None:
    protocols = _protocols()
    for protocol_id in GROUPS[group_id]["protocols"]:
        summary_path = _summary_path(protocol_id)
        if not summary_path.is_file():
            print(f"skip ingestion; no summary for {protocol_id}")
            continue
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        if not summary.get("results_complete"):
            print(f"skip ingestion; protocol incomplete: {protocol_id}")
            continue
        manifest_path = _manifest_path(protocol_id)
        raw_path = ROOT / summary["raw_path"]
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        record = build_evidence_record(
            manifest,
            raw_path,
            protocols[protocol_id],
            root=ROOT,
            manifest_path=manifest_path,
        )
        append_evidence(record)
        print(f"ingested {protocol_id}: {len(record['cell_results'])} cells")


def check_summaries(root: Path = ROOT) -> list[str]:
    errors: list[str] = []
    protocols = _protocols(root)
    for protocol_id, protocol in protocols.items():
        manifest_path = _manifest_path(protocol_id, root)
        summary_path = _summary_path(protocol_id) if root == ROOT else root / "docs" / "data" / "m5_08" / protocol_id / "summary.json"
        if manifest_path.exists():
            try:
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                if manifest.get("protocol_id") != protocol_id or manifest.get("revision") != protocol.get("report_revision"):
                    errors.append(f"manifest/protocol mismatch: {manifest_path.relative_to(root).as_posix()}")
            except (OSError, json.JSONDecodeError) as error:
                errors.append(f"invalid manifest {manifest_path}: {error}")
        if summary_path.exists():
            try:
                summary = json.loads(summary_path.read_text(encoding="utf-8"))
                if summary.get("protocol_id") != protocol_id:
                    errors.append(f"summary/protocol mismatch: {summary_path.relative_to(root).as_posix()}")
                if summary.get("manifest_sha256") and manifest_path.is_file():
                    if summary["manifest_sha256"] != _sha256(manifest_path.read_bytes()):
                        errors.append(f"summary manifest digest differs: {summary_path.relative_to(root).as_posix()}")
                if summary.get("results_complete") and summary.get("completed_cells") != protocol.get("ordered_cell_ids"):
                    errors.append(f"complete summary has wrong cells: {summary_path.relative_to(root).as_posix()}")
            except (OSError, json.JSONDecodeError) as error:
                errors.append(f"invalid summary {summary_path}: {error}")
    blocked_path = root / "docs" / "data" / "m5_08" / "m3_11_report_build_failure.json"
    if blocked_path.is_file():
        try:
            blocked = json.loads(blocked_path.read_text(encoding="utf-8"))
            expected = [
                cell
                for protocol_id in GROUPS["m3_11"]["protocols"]
                for cell in protocols[protocol_id]["ordered_cell_ids"]
            ]
            if blocked.get("status") != "blocked" or blocked.get("cell_ids") != expected:
                errors.append("M3-11 report-SHA build diagnostic does not cover the blocked protocol cells")
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"invalid M3-11 report-SHA build diagnostic: {error}")
    return errors


def record_blocked_m3_11() -> None:
    preflight_path = SCRATCH / "preflight.json"
    preflight = json.loads(preflight_path.read_text(encoding="utf-8"))
    blocked = preflight.get("blocked_groups", {}).get("m3_11")
    if not blocked:
        raise ValueError("M3-11 is not recorded as blocked by report-SHA preflight")
    protocols = _protocols()
    first_protocol = protocols[GROUPS["m3_11"]["protocols"][0]]
    worktree = SCRATCH / "worktrees" / first_protocol["report_revision"]
    command = [
        "cargo", "+1.98.1", "bench", "--locked", "--no-run", "-p", "sim-model",
        "--bench", "m3_11_propagation_bench",
    ]
    result = subprocess.run(command, cwd=worktree, env=_cargo_env(first_protocol["report_revision"]), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if result.returncode == 0:
        raise ValueError("M3-11 report-SHA build now passes; do not record a stale block")
    compiler_errors = [
        line.strip()
        for line in result.stdout.splitlines()
        if line.lstrip().startswith("error[") or "could not find" in line or "no `" in line
    ]
    protocols_doc = _protocols()
    host_doc = tomllib.loads((ROOT / "contracts" / "hosts.toml").read_text(encoding="utf-8"))
    cell_ids = [
        cell
        for protocol_id in GROUPS["m3_11"]["protocols"]
        for cell in protocols_doc[protocol_id]["ordered_cell_ids"]
    ]
    diagnostic = {
        "schema_version": 1,
        "status": "blocked",
        "group": "m3_11",
        "report_revision": first_protocol["report_revision"],
        "report_worktree_head": _git(worktree, "rev-parse", "HEAD"),
        "exit_status": result.returncode,
        "command": " ".join(command),
        "preflight_reason": blocked["reason"],
        "compiler_errors": compiler_errors,
        "cell_ids": cell_ids,
        "overlay_build_ok": False,
        "a5_samples_run": False,
    }
    diagnostic_path = ROOT / "docs" / "data" / "m5_08" / "m3_11_report_build_failure.json"
    write_text_lf(diagnostic_path, json.dumps(diagnostic, separators=(",", ":"), sort_keys=True) + "\n")
    relative = diagnostic_path.relative_to(ROOT).as_posix()
    digest = _sha256(diagnostic_path.read_bytes())
    for protocol_id in GROUPS["m3_11"]["protocols"]:
        protocol = protocols_doc[protocol_id]
        manifest = build_run_manifest(
            protocol,
            run_id=f"m5-08-report-build-failed-{protocol_id}",
            root=worktree,
            hosts=host_doc,
            commands=[" ".join(command)],
        )
        append_evidence(_overlay_failure_record(manifest, protocol, relative, digest))
    print(f"recorded report-SHA build failure for {len(GROUPS['m3_11']['protocols'])} M3-11 protocols; A-5 was not run")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    sub.add_parser("prepare")
    prepare_group_parser = sub.add_parser("prepare-group")
    prepare_group_parser.add_argument("group", choices=ORDER)
    sub.add_parser("dry-run")
    sub.add_parser("check-summaries")
    stage_parser = sub.add_parser("stage")
    stage_parser.add_argument("group", choices=ORDER)
    run_parser = sub.add_parser("run")
    run_parser.add_argument("group", choices=ORDER)
    ingest_parser = sub.add_parser("ingest")
    ingest_parser.add_argument("group", choices=ORDER)
    sub.add_parser("record-blocked-m3-11")
    args = parser.parse_args(argv)
    try:
        if args.mode == "prepare":
            prepare()
        elif args.mode == "prepare-group":
            prepare_group(args.group)
        elif args.mode == "dry-run":
            print_dry_run()
        elif args.mode == "check-summaries":
            errors = check_summaries()
            if errors:
                print("\n".join(errors), file=sys.stderr)
                return 1
            print("A-5 tonight manifests and summaries are valid")
        elif args.mode == "stage":
            return _stage_command(args.group)
        elif args.mode == "run":
            _run_group(args.group)
        elif args.mode == "record-blocked-m3-11":
            record_blocked_m3_11()
        else:
            _ingest(args.group)
    except (OSError, KeyError, ValueError, subprocess.CalledProcessError) as error:
        print(f"A-5 tonight error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
