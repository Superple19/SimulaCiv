"""A-5 comparison of M3P incremental and M3 on matched non-compaction days."""

from __future__ import annotations

import json
import math
import os
import platform
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
CURSOR = 101
DAYS = 3
WARMUPS = 2
SAMPLES = 41
PROBES = 3
TARGET_SAMPLE_SECONDS = 1.5
MIN_SAMPLE_SECONDS = 1.0
MAX_PATH_SPREAD = 0.05


def run(args: list[str], cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, check=True)


def median_mad(values: list[float]) -> tuple[float, float]:
    center = statistics.median(values)
    return center, statistics.median(abs(value - center) for value in values)


def decode_worker_line(process: subprocess.Popen[str], operation: str) -> list[str]:
    assert process.stdout is not None
    line = process.stdout.readline()
    fields = line.rstrip("\r\n").split("|")
    expected = "PROBE" if operation == "probe" else operation
    if fields[0] != expected:
        stderr = process.stderr.read() if process.stderr else ""
        raise RuntimeError(f"worker returned {line!r}, expected {expected}: {stderr}")
    return fields


def worker_command(
    process: subprocess.Popen[str], operation: str, index: int, repetitions: int
) -> dict[str, Any]:
    assert process.stdin is not None
    process.stdin.write(f"{operation}|{index}|{repetitions}\n")
    process.stdin.flush()
    fields = decode_worker_line(process, operation)
    if operation == "probe":
        if len(fields) != 5:
            raise RuntimeError(f"invalid probe response: {fields}")
        return {"sample": int(fields[1]), "numerator_us_per_tick": float(fields[2]),
                "denominator_us_per_tick": float(fields[3]), "pair_wall_seconds": float(fields[4])}
    if len(fields) != 6:
        raise RuntimeError(f"invalid {operation} response: {fields}")
    return {"sample": int(fields[1]), "repetitions": int(fields[2]),
            "numerator_us_per_tick": float(fields[3]), "denominator_us_per_tick": float(fields[4]),
            "pair_wall_seconds": float(fields[5])}


def start_worker(label: str, revision: str, checkout: Path, build_dir: Path) -> tuple[subprocess.Popen[str], dict[str, Any]]:
    if revision != "HEAD":
        run(["git", "worktree", "add", "--detach", str(checkout), revision], ROOT)
        for source in (WORKER, PROCESS_CONTROL, CONFIG):
            destination = checkout / source
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / source, destination)
        commit = run(["git", "rev-parse", revision], cwd=ROOT).stdout.strip()
    else:
        checkout = ROOT
        commit = run(["git", "rev-parse", "HEAD"], cwd=ROOT).stdout.strip()

    config_path = checkout / CONFIG
    build_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update({
        "CARGO_TARGET_DIR": str(build_dir),
        "RUSTUP_TOOLCHAIN": "1.98.1",
        "SIMCIV_M5_05_CONFIG": str(config_path),
        "SIMCIV_M5_05_CHECKPOINT_CURSOR": str(CURSOR),
        "SIMCIV_M5_05_REVISION_ID": label,
        "SIMCIV_M5_05_REVISION_COMMIT": commit,
    })
    built = run(["cargo", "build", "--release", "-p", "sim-model", "--example", "m5_05_m3_09_revision"], checkout, env)
    executable = build_dir / "release" / "examples" / ("m5_05_m3_09_revision.exe" if os.name == "nt" else "m5_05_m3_09_revision")
    if not executable.is_file():
        raise RuntimeError(f"worker build succeeded without executable: {executable}\n{built.stderr}")
    process = subprocess.Popen(
        [str(executable), "--server"], cwd=checkout, env=env, text=True, encoding="utf-8",
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=1,
    )
    fields = decode_worker_line(process, "ready")
    if len(fields) != 13:
        raise RuntimeError(f"invalid worker ready response: {fields}")
    ready = {
        "revision": label,
        "commit": fields[2],
        "config_sha256": fields[3],
        "live_population": int(fields[4]),
        "execution_options": fields[5],
        "m3p_digests": dict(zip(("state", "metrics", "events"), fields[6:9], strict=True)),
        "m3_digests": dict(zip(("state", "metrics", "events"), fields[9:12], strict=True)),
        "process_controls": fields[12],
    }
    if ready["commit"] != commit:
        raise RuntimeError(f"worker commit mismatch: {ready['commit']} != {commit}")
    expected_days = "timed_days=101,102,103"
    if expected_days not in ready["execution_options"]:
        raise RuntimeError(f"worker is not measuring {expected_days}: {ready['execution_options']}")
    if os.name == "nt" and ready["process_controls"] != "priority=high;affinity_mask=0x555":
        raise RuntimeError(f"A-5 process controls were not applied: {ready['process_controls']}")
    return process, ready


def stop_worker(process: subprocess.Popen[str]) -> None:
    if process.poll() is None:
        assert process.stdin is not None
        process.stdin.write("stop|0|1\n")
        process.stdin.flush()
        decode_worker_line(process, "STOPPED")
        process.wait(timeout=10)


def summarize_cell(
    revision: str, attempt: int, probe_seconds: list[float], repetitions: int, samples: list[dict[str, Any]]
) -> dict[str, Any]:
    numerator, numerator_mad = median_mad([row["numerator_us_per_tick"] for row in samples])
    denominator, denominator_mad = median_mad([row["denominator_us_per_tick"] for row in samples])
    ratio = numerator / denominator
    spreads = {"numerator_mad_over_median": numerator_mad / numerator,
               "denominator_mad_over_median": denominator_mad / denominator}
    return {
        "revision": revision, "attempt": attempt + 1, "calibration_probe_pair_seconds": probe_seconds,
        "repetitions_per_sample": repetitions, "warmups": WARMUPS, "samples": samples,
        "numerator_median_us_per_tick": numerator, "numerator_mad_us_per_tick": numerator_mad,
        "denominator_median_us_per_tick": denominator, "denominator_mad_us_per_tick": denominator_mad,
        "ratio": ratio, "path_mad_over_median": spreads,
        "valid": max(spreads.values()) <= MAX_PATH_SPREAD,
    }


def alternate_workers(
    workers: dict[str, subprocess.Popen[str]], index: int
) -> list[tuple[str, subprocess.Popen[str]]]:
    order = list(workers.items())
    if index % 2:
        order.reverse()
    return order


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    args = parser.parse_args()
    plan_path = (ROOT / args.plan).resolve() if not args.plan.is_absolute() else args.plan.resolve()
    plan = tomllib.loads(plan_path.read_text(encoding="utf-8"))
    expected_plan = {
        "reference_commit": REFERENCE,
        "initial_population": 100000,
        "settlement_count": 500,
        "relationship_degree_k": 8,
        "p_local": 0.5,
        "master_seed": 81985529216486895,
        "replicate_id": 7,
        "checkpoint_cursor": CURSOR,
        "timed_internal_days": [101, 102, 103],
        "expected_compaction_days": [],
        "historical_ratio": 1.49,
        "historical_ratio_tolerance": 0.20,
        "days_per_operation": DAYS,
        "compaction_interval_days": 50,
        "warmups": WARMUPS,
        "samples": SAMPLES,
        "calibration_probes": PROBES,
        "target_paired_sample_seconds": TARGET_SAMPLE_SECONDS,
        "minimum_paired_sample_seconds": MIN_SAMPLE_SECONDS,
        "maximum_path_mad_over_median": MAX_PATH_SPREAD,
        "invalid_cell_retries": 1,
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
    with tempfile.TemporaryDirectory(prefix="worktree-", dir=target) as temporary:
        ref_checkout = Path(temporary) / "checkout"
        ref_build = Path(temporary) / "build"
        head_build = target / "head-build"
        head_process, head_ready = start_worker("HEAD", "HEAD", ROOT, head_build)
        ref_process: subprocess.Popen[str] | None = None
        try:
            ref_process, ref_ready = start_worker("m3_09_followup", REFERENCE, ref_checkout, ref_build)
            if head_ready["live_population"] != ref_ready["live_population"]:
                raise RuntimeError("checkpoint live populations differ across revisions")
            if head_ready["execution_options"].replace("checkpoint_cursor=101", "") != ref_ready["execution_options"].replace("checkpoint_cursor=101", ""):
                raise RuntimeError("execution options differ across revisions")
            if head_ready["m3p_digests"] != ref_ready["m3p_digests"] or head_ready["m3_digests"] != ref_ready["m3_digests"]:
                raise RuntimeError("State/Metrics/Events digests differ across revisions")

            workers = {"HEAD": head_process, "m3_09_followup": ref_process}
            ready_records = {"HEAD": head_ready, "m3_09_followup": ref_ready}
            results: dict[str, dict[str, Any]] = {}
            for attempt in range(2):
                probe_seconds = {revision: [] for revision in workers}
                for probe in range(PROBES):
                    for revision, process in alternate_workers(workers, probe):
                        response = worker_command(process, "probe", attempt * 100 + probe, 1)
                        probe_seconds[revision].append(float(response["pair_wall_seconds"]))
                repetitions = {
                    revision: max(1, math.ceil(TARGET_SAMPLE_SECONDS / min(values)))
                    for revision, values in probe_seconds.items()
                }
                sample_rows: dict[str, list[dict[str, Any]]] = {revision: [] for revision in workers}
                for warmup in range(WARMUPS):
                    for revision, process in alternate_workers(workers, warmup + attempt):
                        response = worker_command(
                            process, "warmup", attempt * 100 + warmup, repetitions[revision]
                        )
                        if response["pair_wall_seconds"] < MIN_SAMPLE_SECONDS:
                            raise RuntimeError(f"A-5 warmup under one second for {revision}: {response}")

                for revision in workers:
                    marker = f"TIMING RUN START {revision} attempt={attempt + 1}"
                    print(marker, flush=True)
                    log.append(marker)
                for sample in range(SAMPLES):
                    for revision, process in alternate_workers(workers, sample):
                        response = worker_command(
                            process, "sample", attempt * 1000 + sample, repetitions[revision]
                        )
                        if response["pair_wall_seconds"] < MIN_SAMPLE_SECONDS:
                            raise RuntimeError(f"A-5 sample under one second for {revision}: {response}")
                        sample_rows[revision].append(response)
                for revision in workers:
                    marker = f"TIMING RUN END {revision} attempt={attempt + 1}"
                    print(marker, flush=True)
                    log.append(marker)

                results = {
                    revision: summarize_cell(
                        revision, attempt, probe_seconds[revision], repetitions[revision], sample_rows[revision]
                    )
                    for revision in workers
                }
                if all(row["valid"] for row in results.values()):
                    break
            if not all(row["valid"] for row in results.values()):
                raise RuntimeError(f"A-5 path spread remained above 5% after one retry: {results}")

            historical_ratio = float(plan["historical_ratio"])
            for result in results.values():
                result["within_20_percent_of_historical_1_49"] = (
                    abs(result["ratio"] / historical_ratio - 1.0)
                    <= float(plan["historical_ratio_tolerance"])
                )
            previous_summary = json.loads(
                (ROOT / "docs/data/m5_05/m3_09_incremental_equal_live/summary.json").read_text(encoding="utf-8")
            )
            all_days_ratio = float(
                previous_summary["details"]["cells_measured"][0]["result"]["m3p_over_m3"]
            )
            previous_options = previous_summary["details"]["cells_measured"][0]["result"]["execution_options"]
            all_days_cursor = int(previous_options["equal_live_checkpoint_day"])
            all_days_count = int(previous_options["days"])
            previous_config = tomllib.loads((ROOT / CONFIG).read_text(encoding="utf-8"))
            compaction_interval = int(previous_config["population"]["compaction_interval_days"])
            all_days_timed = list(range(all_days_cursor, all_days_cursor + all_days_count))
            all_days_compaction = [day for day in all_days_timed if day % compaction_interval == 0]
            if all_days_timed != [400, 401, 402] or all_days_compaction != [400]:
                raise RuntimeError(
                    f"M5-05 checkpoint schedule changed: days={all_days_timed}, compaction={all_days_compaction}"
                )
            scope_days = list(plan["timed_internal_days"])
            scope_compaction = [day for day in scope_days if day % compaction_interval == 0]
            if scope_compaction != plan["expected_compaction_days"]:
                raise RuntimeError(
                    f"scope-day compaction mismatch: {scope_days} has {scope_compaction}"
                )
            conclusion_registry = tomllib.loads(
                (ROOT / "docs/reproducibility/m5_05_conclusions.toml").read_text(encoding="utf-8")
            )
            conclusion = next(
                item for item in conclusion_registry["conclusion"]
                if item["id"] == "m3_09_incremental_equal_live"
            )
            if conclusion["scope_check_reference_days"] != scope_days:
                raise RuntimeError("scope days differ from the pre-registered conclusion")
            if conclusion["scope_check_current_cursor_days"] != all_days_timed:
                raise RuntimeError("M5-05 checkpoint days differ from the pre-registered conclusion")
            if conclusion["scope_check_historical_ratio"] != historical_ratio:
                raise RuntimeError("historical comparison ratio differs from the pre-registered conclusion")
            if conclusion["scope_check_tolerance"] != plan["historical_ratio_tolerance"]:
                raise RuntimeError("historical ratio tolerance differs from the pre-registered conclusion")
            hypothesis_holds = all(row["within_20_percent_of_historical_1_49"] for row in results.values())
            header = {
                "type": "header", "protocol": "M5-00 A-5",
                "host": {"platform": platform.platform(), "processor": platform.processor()},
                "scope_days": scope_days, "compaction_days": scope_compaction, "compaction_interval_days": compaction_interval,
                "m5_05_timed_days": all_days_timed, "m5_05_compaction_days": all_days_compaction,
                "reference_commit": ref_ready["commit"], "head_commit": head_ready["commit"],
                "execution_options": head_ready["execution_options"], "equal_live_population": head_ready["live_population"],
                "metrics_events_on_snapshots_off": True, "process_controls": head_ready["process_controls"],
                "original_historical_ratio": historical_ratio, "m5_05_checkpoint_all_days_ratio": all_days_ratio,
                "hypothesis_holds": hypothesis_holds,
            }
            write_text_lf(
                output_dir / "samples.jsonl",
                "".join(
                    json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n"
                    for row in [
                        header,
                        *[
                            dict(type="cell", **row, ready=ready_records[key])
                            for key, row in results.items()
                        ],
                    ]
                ),
            )
            summary = {
                "stage": "m5_05_m3_09_scope_check",
                "spec_files": ["docs/reproducibility/m5_05_conclusions.toml", "docs/reproducibility/m5_05_configs/m3_09_scope_check.toml"],
                "harness_files": ["crates/sim-model/examples/m5_05_m3_09_revision.rs", "crates/sim-model/examples/m5_05_m3_09_scope_check.py"],
                "raw_outputs": [output_dir.relative_to(ROOT).as_posix()],
                "command": "dvc repro docs/data/m5_05/dvc.yaml:m3_09_scope_check",
                "details": {
                    "scope": {"checkpoint_cursor": CURSOR, "timed_internal_days": scope_days, "compaction_days": scope_compaction, "compaction_interval_days": compaction_interval},
                    "m5_05_checkpoint_cell": {"checkpoint_cursor": all_days_cursor, "timed_internal_days": all_days_timed, "compaction_days": all_days_compaction, "ratio": all_days_ratio},
                    "historical_ratio": historical_ratio, "hypothesis_holds": hypothesis_holds,
                    "results": results,
                },
            }
            write_text_lf(summary_path, json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n")
        finally:
            stop_worker(head_process)
            if ref_process is not None:
                stop_worker(ref_process)
            if ref_checkout.exists():
                run(["git", "worktree", "remove", "--force", str(ref_checkout)], ROOT)
    write_text_lf(output_dir / "run.log", "\n".join(log) + "\n")


if __name__ == "__main__":
    main()
