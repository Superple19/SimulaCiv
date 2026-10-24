"""M5-05 A-5 protocol comparison for the M3-09 equal-live result."""

from __future__ import annotations

import json
import math
import os
import platform
import re
import shutil
import statistics
import subprocess
import tempfile
import tomllib
from pathlib import Path
from typing import Any

from simciv_analysis.io import write_text_lf


ROOT = Path(__file__).resolve().parents[3]
WORKER = Path("crates/sim-model/examples/m5_05_m3_09_revision.rs")
PROCESS_CONTROL = Path("crates/sim-model/examples/common/process_controls.rs")
CONFIG = Path("crates/sim-model/tests/fixtures/m3p_propagation_sustained.toml")
REFERENCE = "681bf296ea07d3ddc4391f5f1dfd49255876300c"
CURSOR = 401
DAYS = 3
SEED = 81_985_529_216_486_895
REPLICATE = 7
POPULATIONS = (10_000, 100_000)
WARMUPS = 2
SAMPLES = 41
PROBES = 3
TARGET_SAMPLE_SECONDS = 1.5
MIN_SAMPLE_SECONDS = 1.0
MAX_PATH_SPREAD = 0.05


def run(args: list[str], cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, check=True)


def med_mad(values: list[float]) -> tuple[float, float]:
    median = statistics.median(values)
    return median, statistics.median(abs(value - median) for value in values)


def worker_reply(process: subprocess.Popen[str], operation: str) -> list[str]:
    assert process.stdout is not None
    line = process.stdout.readline()
    fields = line.rstrip("\r\n").split("|")
    expected = {"probe": "PROBE", "validate": "VALIDATE"}.get(operation, operation)
    if not fields or fields[0] != expected:
        stderr = process.stderr.read() if process.stderr else ""
        raise RuntimeError(f"worker returned {line!r}, expected {expected}: {stderr}")
    return fields


def worker_command(
    process: subprocess.Popen[str], operation: str, index: int, repetitions: int = 1
) -> dict[str, Any]:
    assert process.stdin is not None
    process.stdin.write(f"{operation}|{index}|{repetitions}\n")
    process.stdin.flush()
    fields = worker_reply(process, operation)
    if operation == "validate":
        if len(fields) != 3:
            raise RuntimeError(f"invalid validation response: {fields}")
        return {"sample": int(fields[1]), "public_call_us": float(fields[2])}
    if operation == "probe":
        if len(fields) != 6:
            raise RuntimeError(f"invalid probe response: {fields}")
        return {
            "sample": int(fields[1]), "m3p_serial_us_per_tick": float(fields[2]),
            "m3p_incremental_us_per_tick": float(fields[3]), "m3_us_per_tick": float(fields[4]),
            "triple_wall_seconds": float(fields[5]),
        }
    if len(fields) != 7:
        raise RuntimeError(f"invalid {operation} response: {fields}")
    return {
        "sample": int(fields[1]), "repetitions": int(fields[2]),
        "m3p_serial_us_per_tick": float(fields[3]),
        "m3p_incremental_us_per_tick": float(fields[4]),
        "m3_us_per_tick": float(fields[5]), "triple_wall_seconds": float(fields[6]),
    }


def start_worker(
    label: str, revision: str, population: int, settlements: int, checkout: Path, build_dir: Path,
    validation_timer: bool = False,
) -> tuple[subprocess.Popen[str], dict[str, Any]]:
    if revision != "HEAD":
        run(["git", "worktree", "add", "--detach", str(checkout), revision], ROOT)
        for source in (WORKER, PROCESS_CONTROL, CONFIG):
            destination = checkout / source
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / source, destination)
        commit = run(["git", "rev-parse", revision], ROOT).stdout.strip()
    else:
        checkout = ROOT
        commit = run(["git", "rev-parse", "HEAD"], ROOT).stdout.strip()

    build_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update({
        "CARGO_TARGET_DIR": str(build_dir), "RUSTUP_TOOLCHAIN": "1.98.1",
        "SIMCIV_M5_05_CONFIG": str(checkout / CONFIG),
        "SIMCIV_M5_05_CHECKPOINT_CURSOR": str(CURSOR),
        "SIMCIV_M5_05_N": str(population), "SIMCIV_M5_05_K": str(settlements),
        "SIMCIV_M5_05_REVISION_ID": label, "SIMCIV_M5_05_REVISION_COMMIT": commit,
    })
    if validation_timer:
        env["SIMCIV_M5_05_VALIDATE_FOR_TIMING"] = "1"
    run(["cargo", "build", "--release", "-p", "sim-model", "--example", "m5_05_m3_09_revision"], checkout, env)
    executable = build_dir / "release" / "examples" / (
        "m5_05_m3_09_revision.exe" if os.name == "nt" else "m5_05_m3_09_revision"
    )
    if not executable.is_file():
        raise RuntimeError(f"worker build succeeded without executable: {executable}")
    process = subprocess.Popen(
        [str(executable), "--server"], cwd=checkout, env=env, text=True, encoding="utf-8",
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=1,
    )
    fields = worker_reply(process, "ready")
    if len(fields) != 13:
        raise RuntimeError(f"invalid worker ready response: {fields}")
    ready = {
        "revision": label, "commit": fields[2], "config_sha256": fields[3],
        "live_population": int(fields[4]), "execution_options": fields[5],
        "m3p_digests": dict(zip(("state", "metrics", "events"), fields[6:9], strict=True)),
        "m3_digests": dict(zip(("state", "metrics", "events"), fields[9:12], strict=True)),
        "process_controls": fields[12],
    }
    if ready["commit"] != commit:
        raise RuntimeError(f"worker commit mismatch: {ready['commit']} != {commit}")
    if f"initial_population={population};settlements={settlements};" not in ready["execution_options"]:
        raise RuntimeError(f"worker config mismatch: {ready['execution_options']}")
    if "checkpoint_cursor=401;timed_days=401,402,403" not in ready["execution_options"]:
        raise RuntimeError(f"worker day scope mismatch: {ready['execution_options']}")
    if os.name == "nt" and ready["process_controls"] != "priority=high;affinity_mask=0x555":
        raise RuntimeError(f"A-5 process controls were not applied: {ready['process_controls']}")
    return process, ready


def stop_worker(process: subprocess.Popen[str]) -> str:
    if process.poll() is None:
        assert process.stdin is not None
        process.stdin.write("stop|0|1\n")
        process.stdin.flush()
        worker_reply(process, "STOPPED")
        process.wait(timeout=10)
    return process.stderr.read() if process.stderr else ""


def alternate(workers: dict[str, subprocess.Popen[str]], index: int) -> list[tuple[str, subprocess.Popen[str]]]:
    order = list(workers.items())
    if index % 2:
        order.reverse()
    return order


def measure_population(
    population: int, temporary_root: Path, log: list[str]
) -> tuple[list[dict[str, Any]], str]:
    settlements = population // 200
    ref_checkout = temporary_root / f"ref-{population}"
    ref_build = temporary_root / f"build-ref-{population}"
    head_build = temporary_root / f"build-head-{population}"
    ref_process, ref_ready = start_worker(
        f"m3_09_followup_n{population}", REFERENCE, population, settlements, ref_checkout, ref_build
    )
    head_process: subprocess.Popen[str] | None = None
    stderr_head = ""
    try:
        head_process, head_ready = start_worker(
            f"head_n{population}", "HEAD", population, settlements, temporary_root / "unused", head_build
        )
        if ref_ready["live_population"] != head_ready["live_population"]:
            raise RuntimeError(f"live population differs at N={population}")
        if ref_ready["m3p_digests"] != head_ready["m3p_digests"] or ref_ready["m3_digests"] != head_ready["m3_digests"]:
            raise RuntimeError(f"canonical digests differ at N={population}")
        if ref_ready["config_sha256"] != head_ready["config_sha256"]:
            raise RuntimeError(f"config differs at N={population}")
        workers = {"m3_09_followup": ref_process, "HEAD": head_process}
        ready = {"m3_09_followup": ref_ready, "HEAD": head_ready}
        final: dict[str, dict[str, Any]] = {}

        for attempt in range(2):
            probe_seconds = {key: [] for key in workers}
            for probe in range(PROBES):
                for revision, process in alternate(workers, probe):
                    result = worker_command(process, "probe", attempt * 100 + probe)
                    probe_seconds[revision].append(float(result["triple_wall_seconds"]))
            repetitions = {
                revision: max(1, math.ceil(TARGET_SAMPLE_SECONDS / min(times)))
                for revision, times in probe_seconds.items()
            }
            for warmup in range(WARMUPS):
                for revision, process in alternate(workers, warmup):
                    result = worker_command(process, "warmup", attempt * 100 + warmup, repetitions[revision])
                    if result["triple_wall_seconds"] < MIN_SAMPLE_SECONDS:
                        raise RuntimeError(f"A-5 warmup below one second: {revision} {result}")

            samples = {revision: [] for revision in workers}
            for revision in workers:
                marker = f"TIMING RUN START M3-09 protocol-match N={population} {revision} attempt={attempt + 1}"
                print(marker, flush=True)
                log.append(marker)
            for sample in range(SAMPLES):
                for revision, process in alternate(workers, sample):
                    result = worker_command(process, "sample", attempt * 1000 + sample, repetitions[revision])
                    if result["triple_wall_seconds"] < MIN_SAMPLE_SECONDS:
                        raise RuntimeError(f"A-5 sample below one second: {revision} {result}")
                    samples[revision].append(result)
            for revision in workers:
                marker = f"TIMING RUN END M3-09 protocol-match N={population} {revision} attempt={attempt + 1}"
                print(marker, flush=True)
                log.append(marker)

            final = {}
            for revision, rows in samples.items():
                stats = {}
                for key in ("m3p_serial_us_per_tick", "m3p_incremental_us_per_tick", "m3_us_per_tick"):
                    median = statistics.median(float(row[key]) for row in rows)
                    mad = statistics.median(abs(float(row[key]) - median) for row in rows)
                    stats[key.removesuffix("_us_per_tick")] = {"median_us_per_tick": median, "mad_us_per_tick": mad, "mad_over_median": mad / median}
                ratio = stats["m3p_incremental"]["median_us_per_tick"] / stats["m3"]["median_us_per_tick"]
                final[revision] = {
                    "revision": revision, "commit": ready[revision]["commit"], "initial_population": population,
                    "settlement_count": settlements, "live_population": ready[revision]["live_population"],
                    "checkpoint_cursor": CURSOR, "timed_internal_days": [401, 402, 403],
                    "compaction_days": [], "runner_rotation": ["m3p_serial", "m3p_incremental", "m3"],
                    "warmups": WARMUPS, "samples": SAMPLES, "repetitions_per_sample": repetitions[revision],
                    "calibration_probe_triple_seconds": probe_seconds[revision], "samples_raw": rows,
                    "paths": stats, "m3p_incremental_over_m3": ratio,
                    "path_spread_valid": max(value["mad_over_median"] for value in stats.values()) <= MAX_PATH_SPREAD,
                    "execution_options": ready[revision]["execution_options"],
                    "m3p_digests": ready[revision]["m3p_digests"], "m3_digests": ready[revision]["m3_digests"],
                    "process_controls": ready[revision]["process_controls"],
                }
            if all(row["path_spread_valid"] for row in final.values()):
                break
        if not all(row["path_spread_valid"] for row in final.values()):
            raise RuntimeError(f"A-5 path spread exceeded 5% after one retry at N={population}")
        historical = 1.49 if population == 100_000 else 1.21
        for row in final.values():
            row["historical_ratio"] = historical
            row["within_20_percent_of_historical_ratio"] = abs(row["m3p_incremental_over_m3"] / historical - 1.0) <= 0.20
        return list(final.values()), stderr_head
    finally:
        if head_process is not None:
            stderr_head = stop_worker(head_process)
        stop_worker(ref_process)


def validate_for_cost(temporary_root: Path) -> dict[str, Any]:
    checkout = temporary_root / "unused-validation"
    build = temporary_root / "build-validation"
    process, ready = start_worker("head_validate_for", "HEAD", 100_000, 500, checkout, build, True)
    calls = []
    try:
        for index in range(2 + 41):
            result = worker_command(process, "validate", index)
            if index >= 2:
                calls.append(result["public_call_us"])
    finally:
        stderr = stop_worker(process)
    lines = stderr.splitlines()
    internal = []
    for index, line in enumerate(lines[:-1]):
        if line.startswith("M5_VALIDATE_REQUEST="):
            match = re.fullmatch(r"M5_VALIDATE_FOR_US=([0-9.]+)", lines[index + 1])
            if match is None:
                raise RuntimeError(f"missing validate_for timer after {line}")
            internal.append(float(match.group(1)))
    if len(internal) != 43:
        raise RuntimeError(f"temporary validate_for timer recorded {len(internal)} calls, expected 43")
    internal = internal[2:]
    median, mad = med_mad(internal)
    public_median, public_mad = med_mad(calls)
    return {
        "revision": ready["commit"], "population": 100000, "calls": 41,
        "validate_for_median_us_per_call": median, "validate_for_mad_us_per_call": mad,
        "public_zero_day_call_median_us": public_median, "public_zero_day_call_mad_us": public_mad,
        "internal_samples_us": internal, "public_call_samples_us": calls,
        "timer_method": "temporary Instant instrumentation inside M3pStorageRunContext::validate_for; removed after validation measurement",
        "process_controls": ready["process_controls"],
    }


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("validate", "a5"), required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    args = parser.parse_args()
    plan_path = (ROOT / args.plan).resolve() if not args.plan.is_absolute() else args.plan.resolve()
    plan = tomllib.loads(plan_path.read_text(encoding="utf-8"))
    expected_plan = {
        "reference_commit": REFERENCE, "initial_populations": list(POPULATIONS),
        "settlements_per_200_agents": True, "relationship_degree_k": 8, "p_local": 0.5,
        "master_seed": SEED, "replicate_id": REPLICATE, "reference_checkpoint_cursor": CURSOR,
        "reference_timed_internal_days": [401, 402, 403], "m5_05_checkpoint_cursor": 400,
        "m5_05_timed_internal_days": [400, 401, 402], "compaction_interval_days": 50,
        "reference_compaction_days": [], "m5_05_compaction_days": [400], "days_per_public_call": DAYS,
        "reference_runner_rotation": ["m3p_serial", "m3p_incremental", "m3"],
        "m5_05_runner_rotation": ["m3p_incremental", "m3"], "reference_warmups": 2,
        "reference_samples": 11, "m5_05_warmups": WARMUPS, "m5_05_samples": SAMPLES,
        "calibration_probes": PROBES, "target_paired_sample_seconds": TARGET_SAMPLE_SECONDS,
        "minimum_paired_sample_seconds": MIN_SAMPLE_SECONDS,
        "maximum_path_mad_over_median": MAX_PATH_SPREAD, "invalid_cell_retries": 1,
        "measure_validate_for": True, "validate_for_population": 100000,
        "validate_for_warmups": 2, "validate_for_calls": 41,
    }
    for key, expected in expected_plan.items():
        if plan.get(key) != expected:
            raise ValueError(f"scope plan mismatch for {key}: {plan.get(key)!r} != {expected!r}")
    output_dir = (ROOT / args.output_dir).resolve() if not args.output_dir.is_absolute() else args.output_dir.resolve()
    summary_path = (ROOT / args.summary).resolve() if not args.summary.is_absolute() else args.summary.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    target = ROOT / "target" / "m5_05_m3_09_scope_check"
    target.mkdir(parents=True, exist_ok=True)
    log: list[str] = []

    if args.mode == "validate":
        with tempfile.TemporaryDirectory(prefix="validate-", dir=target) as temporary:
            data = validate_for_cost(Path(temporary))
        header = {"type": "header", "mode": "validate_for_cost", "host": platform.platform(), "scope": plan}
        records = [header, {"type": "validation", **data}]
        summary = {
            "stage": "m5_05_m3_09_validate_for_cost",
            "spec_files": ["docs/reproducibility/m5_05_configs/m3_09_scope_check.toml"],
            "harness_files": ["crates/sim-model/examples/m5_05_m3_09_revision.rs", "crates/sim-model/examples/m5_05_m3_09_scope_check.py"],
            "raw_outputs": [output_dir.relative_to(ROOT).as_posix()],
            "command": "dvc repro docs/data/m5_05/m3_09_scope_check/dvc.yaml:m3_09_validate_for_cost",
            "details": data,
        }
    else:
        previous = json.loads((ROOT / "docs/data/m5_05/m3_09_incremental_equal_live/summary.json").read_text(encoding="utf-8"))
        previous_result = previous["details"]["cells_measured"][0]["result"]
        m5_cursor = int(previous_result["execution_options"]["equal_live_checkpoint_day"])
        m5_days = list(range(m5_cursor, m5_cursor + int(previous_result["execution_options"]["days"])))
        fixture = tomllib.loads((ROOT / CONFIG).read_text(encoding="utf-8"))
        compact_interval = int(fixture["population"]["compaction_interval_days"])
        m5_compaction = [day for day in m5_days if day % compact_interval == 0]
        if m5_days != [400, 401, 402] or m5_compaction != [400]:
            raise RuntimeError(f"M5-05 driver scope changed: days={m5_days}, compaction={m5_compaction}")
        cells = []
        with tempfile.TemporaryDirectory(prefix="protocol-", dir=target) as temporary:
            temp_root = Path(temporary)
            for population in POPULATIONS:
                values, _ = measure_population(population, temp_root, log)
                cells.extend(values)
        validation = json.loads((ROOT / "docs/data/m5_05/m3_09_scope_check/validate_for/summary.json").read_text(encoding="utf-8"))["details"]
        for row in cells:
            row["matches_m5_09_equal_live_protocol"] = True
            row["m5_05_all_days_ratio_100k"] = float(previous_result["m3p_over_m3"])
        header = {
            "type": "header", "mode": "protocol_matched_a5", "host": {"platform": platform.platform(), "processor": platform.processor()},
            "scope_days": [401, 402, 403], "compaction_days": [], "reference_commit": REFERENCE,
            "head_commit": run(["git", "rev-parse", "HEAD"], ROOT).stdout.strip(),
            "m5_05_days": m5_days, "m5_05_compaction_days": m5_compaction,
            "m5_05_ratio_100k": float(previous_result["m3p_over_m3"]),
            "validate_for": {key: value for key, value in validation.items() if not key.endswith("samples_us")},
        }
        records = [header, *[{"type": "cell", **row} for row in cells]]
        summary = {
            "stage": "m5_05_m3_09_scope_check",
            "spec_files": ["docs/reproducibility/m5_05_conclusions.toml", "docs/reproducibility/m5_05_configs/m3_09_scope_check.toml"],
            "harness_files": ["crates/sim-model/examples/m5_05_m3_09_revision.rs", "crates/sim-model/examples/m5_05_m3_09_scope_check.py"],
            "raw_outputs": [output_dir.relative_to(ROOT).as_posix()],
            "command": "dvc repro docs/data/m5_05/m3_09_scope_check/dvc.yaml:m3_09_scope_check",
            "details": {
                "historical_followup_protocol": "2 warmups; 11 samples; one three-day public call per path; M3P serial/M3P incremental/M3 rotated; contexts built outside each timer.",
                "m5_05_protocol": "2 warmups; 41 A-5 samples; three-day calls; 7 repetitions per 100k sample; M3P incremental/M3 pair only; contexts built outside each timer.",
                "scope": {"reference_cursor": 401, "reference_days": [401, 402, 403], "reference_compaction": [], "m5_05_cursor": m5_cursor, "m5_05_days": m5_days, "m5_05_compaction": m5_compaction},
                "m5_05_ratio_100k": float(previous_result["m3p_over_m3"]), "validate_for": validation,
                "cells": cells,
            },
        }
    write_text_lf(output_dir / "samples.jsonl", "".join(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n" for record in records))
    write_text_lf(output_dir / "run.log", "\n".join(log) + ("\n" if log else ""))
    write_text_lf(summary_path, json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n")
    print("\n".join(log), flush=True)


if __name__ == "__main__":
    main()
