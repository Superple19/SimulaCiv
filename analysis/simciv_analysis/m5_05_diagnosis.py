"""Run the registered M3-09 numerator/denominator revision diagnosis."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Any

from .provenance import ROOT


PLAN = Path("docs/reproducibility/m5_05_configs/m3_09_diagnosis.toml")
WORKER = Path("crates/sim-model/examples/m5_05_m3_09_revision.rs")
OLD_REPORT = Path("docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md")
CURRENT_SUMMARY = Path("docs/data/m5_05/m3_09_incremental_equal_live/summary.json")
RAW_REL = Path("docs/data/m5_05/m3_09_diagnosis/raw")
SUMMARY_REL = Path("docs/data/m5_05/m3_09_diagnosis/summary.json")


def _read_plan(root: Path = ROOT) -> dict[str, Any]:
    return tomllib.loads((root / PLAN).read_text(encoding="utf-8"))


def _table_after(text: str, heading: str) -> list[list[str]]:
    lines = text.splitlines()
    start = next((i for i, line in enumerate(lines) if line.strip() == heading), None)
    if start is None:
        raise ValueError(f"missing report heading: {heading}")
    for index in range(start + 1, len(lines)):
        if not lines[index].lstrip().startswith("|"):
            continue
        rows = []
        for line in lines[index:]:
            if not line.lstrip().startswith("|"):
                break
            row = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if row and not all(set(cell) <= {"-", ":", " "} for cell in row):
                rows.append(row)
        return rows
    raise ValueError(f"missing table after report heading: {heading}")


def _number(value: str) -> tuple[float, float | None]:
    median, separator, mad = value.replace(",", "").replace("µs", "").strip().partition("±")
    return float(median.strip()), float(mad.strip()) if separator else None


def historical_decomposition(root: Path = ROOT) -> dict[str, Any]:
    """Extract absolute M3P numerator and M3 denominator from the old report."""
    text = (root / OLD_REPORT).read_text(encoding="utf-8")
    initial_rows = _table_after(text, "## Equal-live M3P/M3 comparison")
    initial = next(row for row in initial_rows[1:] if row[0] == "100,000")
    rerun_rows = _table_after(text, "### Equal-live M3P/M3 rerun and interleaving")
    reruns = {
        row[1].lower(): row
        for row in rerun_rows[1:]
        if row[0] == "100,000"
    }
    current = json.loads((root / CURRENT_SUMMARY).read_text(encoding="utf-8"))
    result = current["details"]["cells_measured"][0]["result"]
    current_num = float(result["left_median_us"])
    current_den = float(result["right_median_us"])
    current_ratio = current_num / current_den

    rows = [
        {
            "source": "M3-09 original 100k equal-live row",
            "numerator_m3p_incremental_us": _number(initial[6])[0],
            "numerator_mad_us": None,
            "denominator_m3_us": _number(initial[7])[0],
            "denominator_mad_us": None,
            "ratio": float(initial[8].split("×")[0]),
        }
    ]
    for schedule in ("paired", "non-interleaved"):
        row = reruns[schedule]
        numerator, numerator_mad = _number(row[3])
        denominator, denominator_mad = _number(row[4])
        rows.append({
            "source": f"M3-09 follow-up {schedule}",
            "numerator_m3p_incremental_us": numerator,
            "numerator_mad_us": numerator_mad,
            "denominator_m3_us": denominator,
            "denominator_mad_us": denominator_mad,
            "ratio": float(row[5].split("×")[0]),
        })
    rows.append({
        "source": "M5-05 current M3-09 data",
        "numerator_m3p_incremental_us": current_num,
        "numerator_mad_us": float(result["left_mad_us"]),
        "denominator_m3_us": current_den,
        "denominator_mad_us": float(result["right_mad_us"]),
        "ratio": current_ratio,
    })
    reference = rows[1]
    return {
        "rows": rows,
        "comparison_reference": reference["source"],
        "numerator_change_percent": 100 * (current_num / reference["numerator_m3p_incremental_us"] - 1),
        "denominator_change_percent": 100 * (current_den / reference["denominator_m3_us"] - 1),
        "ratio_change_percent": 100 * (current_ratio / reference["ratio"] - 1),
        "dominant_component": (
            "both, dominated by numerator"
            if current_num > reference["numerator_m3p_incremental_us"]
            and current_den < reference["denominator_m3_us"]
            and abs(current_num / reference["numerator_m3p_incremental_us"] - 1)
            > abs(current_den / reference["denominator_m3_us"] - 1)
            else "numerator"
            if current_num > reference["numerator_m3p_incremental_us"]
            else "denominator"
            if current_den < reference["denominator_m3_us"]
            else "neither"
        ),
    }


def median_mad(values: list[float]) -> tuple[float, float]:
    median = statistics.median(values)
    return median, statistics.median(abs(value - median) for value in values)


def repetitions_for_target(pair_walls: list[float], target_seconds: float) -> int:
    if not pair_walls or min(pair_walls) <= 0:
        raise ValueError("calibration requires positive paired durations")
    return max(1, math.ceil(target_seconds / min(pair_walls)))


class Worker:
    def __init__(self, revision: dict[str, str], process: subprocess.Popen[str]):
        self.revision = revision
        self.process = process
        line = process.stdout.readline() if process.stdout else ""
        if not line.startswith("READY|"):
            stderr = process.stderr.read() if process.stderr else ""
            raise RuntimeError(f"worker {revision['id']} failed to start: {line}\n{stderr}")
        values = line.rstrip("\r\n").split("|")
        self.ready = {
            "revision_id": values[1],
            "commit": values[2],
            "config_sha256": values[3],
            "live_population": int(values[4]),
            "execution_options": values[5],
            "m3p_digests": dict(zip(("state", "metrics", "events"), values[6:9], strict=True)),
            "m3_digests": dict(zip(("state", "metrics", "events"), values[9:12], strict=True)),
            "process_controls": values[12],
        }
        if self.ready["revision_id"] != revision["id"] or self.ready["commit"] != revision["commit"]:
            raise RuntimeError(f"worker identity mismatch: {self.ready}")

    def command(self, operation: str, sample: int, repetitions: int = 1) -> dict[str, Any]:
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("worker protocol streams closed")
        self.process.stdin.write(f"{operation}|{sample}|{repetitions}\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        fields = line.rstrip("\r\n").split("|")
        expected_operation = "PROBE" if operation == "probe" else operation
        if not fields or fields[0] != expected_operation:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"bad worker reply from {self.revision['id']}: {line}\n{stderr}")
        if operation == "probe":
            return {"sample": int(fields[1]), "numerator_us": float(fields[2]), "denominator_us": float(fields[3]), "pair_wall_seconds": float(fields[4])}
        return {"sample": int(fields[1]), "repetitions": int(fields[2]), "numerator_us_per_tick": float(fields[3]), "denominator_us_per_tick": float(fields[4]), "pair_wall_seconds": float(fields[5])}

    def close(self) -> None:
        if self.process.poll() is None:
            if self.process.stdin:
                self.process.stdin.write("stop|0|1\n")
                self.process.stdin.flush()
            self.process.wait(timeout=30)


def _git(root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", *args], cwd=root, text=True, capture_output=True, check=check)


def _ensure_worktree(root: Path, revision: dict[str, str], worktree_root: Path) -> Path:
    path = worktree_root / revision["id"]
    if path.exists():
        current = _git(path, "rev-parse", "HEAD").stdout.strip()
        if current != revision["commit"]:
            raise RuntimeError(f"worktree path has unexpected revision: {path} {current}")
    else:
        worktree_root.mkdir(parents=True, exist_ok=True)
        _git(root, "worktree", "add", "--detach", str(path), revision["commit"])
    source = root / WORKER
    target = path / WORKER
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    return path


def _build_start_worker(root: Path, revision: dict[str, str], worktree_root: Path, build_root: Path, config_path: Path, raw: Path) -> tuple[Worker, float]:
    worktree = _ensure_worktree(root, revision, worktree_root)
    target_dir = build_root / revision["id"]
    env = os.environ.copy()
    env.update({
        "CARGO_TARGET_DIR": str(target_dir),
        "SIMCIV_M5_05_CONFIG": str(config_path),
        "SIMCIV_M5_05_REVISION_ID": revision["id"],
        "SIMCIV_M5_05_REVISION_COMMIT": revision["commit"],
    })
    log_dir = raw / "build_logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    command = ["cargo", "build", "--release", "-p", "sim-model", "--example", WORKER.stem]
    build = subprocess.run(command, cwd=worktree, env=env, text=True, capture_output=True)
    (log_dir / f"{revision['id']}.stdout.txt").write_text(build.stdout, encoding="utf-8")
    (log_dir / f"{revision['id']}.stderr.txt").write_text(build.stderr, encoding="utf-8")
    if build.returncode:
        raise RuntimeError(f"build failed at {revision['id']}: {build.stderr[-8000:]}")
    exe = target_dir / "release" / "examples" / f"{WORKER.stem}.exe"
    if not exe.is_file():
        raise FileNotFoundError(exe)
    process = subprocess.Popen(
        [str(exe), "--server"], cwd=worktree, env=env, text=True,
        encoding="utf-8", stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, bufsize=1,
    )
    worker = Worker(revision, process)
    elapsed = time.perf_counter() - started
    return worker, elapsed


def _revision_order(workers: list[Worker], round_index: int) -> list[Worker]:
    offset = round_index % len(workers)
    ordered = workers[offset:] + workers[:offset]
    return list(reversed(ordered)) if round_index % 2 else ordered


def run_diagnosis(output_dir: Path, summary_path: Path, root: Path = ROOT) -> dict[str, Any]:
    plan = _read_plan(root)
    output_dir.mkdir(parents=True, exist_ok=True)
    sample_path = output_dir / "samples.jsonl"
    if sample_path.exists():
        raise FileExistsError(f"refusing to overwrite previous raw measurements: {sample_path}")
    raw_records: list[dict[str, Any]] = []
    workers: list[Worker] = []
    worktree_root = root / "target" / "m5_05_m3_09_diagnosis" / "worktrees"
    build_root = root / "target" / "m5_05_m3_09_diagnosis" / "builds"
    config_path = root / "crates/sim-model/tests/fixtures/m3p_propagation_sustained.toml"
    start = time.perf_counter()
    build_seconds: dict[str, float] = {}
    try:
        for revision in plan["revisions"]:
            worker, build_wall = _build_start_worker(root, revision, worktree_root, build_root, config_path, output_dir)
            workers.append(worker)
            build_seconds[revision["id"]] = build_wall
            raw_records.append({"type": "ready", **worker.ready})

        first = workers[0].ready
        for worker in workers[1:]:
            if worker.ready["execution_options"] != first["execution_options"]:
                raise RuntimeError("execution options differ across revisions")
            if worker.ready["config_sha256"] != first["config_sha256"]:
                raise RuntimeError("frozen config bytes differ across revisions")
            if worker.ready["live_population"] != first["live_population"]:
                raise RuntimeError("checkpoint live population differs across revisions")
            if worker.ready["m3p_digests"] != first["m3p_digests"] or worker.ready["m3_digests"] != first["m3_digests"]:
                raise RuntimeError("canonical checkpoint/result digests differ across revisions")

        calibration: dict[str, list[dict[str, Any]]] = {worker.revision["id"]: [] for worker in workers}
        for probe in range(int(plan["calibration_probes"])):
            for worker in _revision_order(workers, probe):
                record = worker.command("probe", probe)
                record.update({"type": "calibration", "revision_id": worker.revision["id"]})
                calibration[worker.revision["id"]].append(record)
                raw_records.append(record)
        repetitions = repetitions_for_target(
            [record["pair_wall_seconds"] for records in calibration.values() for record in records],
            float(plan["target_pair_wall_seconds"]),
        )

        warmups = int(plan["warmups"])
        for warmup in range(warmups):
            for worker in _revision_order(workers, warmup):
                record = worker.command("warmup", warmup, repetitions)
                record.update({"type": "warmup", "revision_id": worker.revision["id"]})
                raw_records.append(record)

        print("TIMING RUN START M5-05 M3-09 cross-revision diagnosis", flush=True)
        timing_started = time.perf_counter()
        samples: dict[str, list[dict[str, Any]]] = {worker.revision["id"]: [] for worker in workers}
        for sample in range(int(plan["samples"])):
            for worker in _revision_order(workers, sample + warmups):
                record = worker.command("sample", sample, repetitions)
                record.update({"type": "sample", "revision_id": worker.revision["id"], "round": sample})
                samples[worker.revision["id"]].append(record)
                raw_records.append(record)

        retry_ids = []
        stats = {}
        for worker in workers:
            items = samples[worker.revision["id"]]
            num_med, num_mad = median_mad([item["numerator_us_per_tick"] for item in items])
            den_med, den_mad = median_mad([item["denominator_us_per_tick"] for item in items])
            if max(num_mad / num_med, den_mad / den_med) > 0.05:
                retry_ids.append(worker.revision["id"])
            stats[worker.revision["id"]] = {
                "numerator_median_us_per_tick": num_med,
                "numerator_mad_us_per_tick": num_mad,
                "numerator_mad_over_median": num_mad / num_med,
                "denominator_median_us_per_tick": den_med,
                "denominator_mad_us_per_tick": den_mad,
                "denominator_mad_over_median": den_mad / den_med,
                "ratio": num_med / den_med,
                "repetitions_per_sample": repetitions,
                "sample_count": len(items),
                "retry_samples": [],
            }
        for retry_round, revision_id in enumerate(retry_ids):
            worker = next(worker for worker in workers if worker.revision["id"] == revision_id)
            for warmup in range(warmups):
                raw_records.append({"type": "retry_warmup", "revision_id": revision_id, **worker.command("warmup", 100 + warmup, repetitions)})
            retry = []
            for sample in range(int(plan["samples"])):
                record = worker.command("sample", 100 + sample, repetitions)
                record.update({"type": "retry_sample", "revision_id": revision_id, "round": sample})
                retry.append(record)
                raw_records.append(record)
            stats[revision_id]["retry_samples"] = retry
            final = retry
            num_med, num_mad = median_mad([item["numerator_us_per_tick"] for item in final])
            den_med, den_mad = median_mad([item["denominator_us_per_tick"] for item in final])
            stats[revision_id].update({
                "numerator_median_us_per_tick": num_med,
                "numerator_mad_us_per_tick": num_mad,
                "numerator_mad_over_median": num_mad / num_med,
                "denominator_median_us_per_tick": den_med,
                "denominator_mad_us_per_tick": den_mad,
                "denominator_mad_over_median": den_mad / den_med,
                "ratio": num_med / den_med,
                "sample_count": len(final),
                "retried_once": True,
            })
        timing_wall = time.perf_counter() - timing_started
        print("TIMING RUN END M5-05 M3-09 cross-revision diagnosis", flush=True)

        order = [item["id"] for item in plan["revisions"]]
        changes = []
        threshold = float(plan["change_screen_percent"]) / 100.0
        for before_id, after_id in zip(order, order[1:]):
            before, after = stats[before_id], stats[after_id]
            deltas = {
                "numerator": after["numerator_median_us_per_tick"] / before["numerator_median_us_per_tick"] - 1,
                "denominator": after["denominator_median_us_per_tick"] / before["denominator_median_us_per_tick"] - 1,
                "ratio": after["ratio"] / before["ratio"] - 1,
            }
            changes.append({
                "before": before_id,
                "after": after_id,
                "relative_changes": deltas,
                "step_change": any(abs(value) > threshold for value in deltas.values()),
                "valid": not any(
                    max(stats[key]["numerator_mad_over_median"], stats[key]["denominator_mad_over_median"]) > 0.05
                    for key in (before_id, after_id)
                ),
            })
        step = next((item for item in changes if item["step_change"] and item["valid"]), None)
        details = {
            "plan": {"path": PLAN.as_posix(), "values": plan},
            "historical_decomposition": historical_decomposition(root),
            "execution_options_identical": True,
            "checkpoint_live_population": first["live_population"],
            "checkpoint_digest_parity": True,
            "repetitions_per_sample": repetitions,
            "calibration": calibration,
            "workers": [worker.ready | {"build_wall_seconds": build_seconds[worker.revision["id"]]} for worker in workers],
            "revisions": stats,
            "adjacent_changes": changes,
            "first_step_change": step,
            "validity": {key: max(value["numerator_mad_over_median"], value["denominator_mad_over_median"]) <= 0.05 for key, value in stats.items()},
            "retry_revision_ids": retry_ids,
            "timing_wall_seconds": timing_wall,
            "total_wall_seconds": time.perf_counter() - start,
            "host": {"system": platform.system(), "release": platform.release(), "machine": platform.machine(), "processor": platform.processor()},
            "protocol": "A-5; 2 warmups; 41 samples; fixed r calibrated once across revisions; revision order rotates/reverses each round; within-pair order alternates; Windows workers high priority with affinity 0x555",
            "bisect": None,
            "phase_attribution": None,
        }
        raw_records.insert(0, {"type": "header", "plan": plan, "protocol": details["protocol"], "host": details["host"]})
        with sample_path.open("w", encoding="utf-8", newline="\n") as stream:
            for item in raw_records:
                stream.write(json.dumps(item, sort_keys=True, separators=(",", ":")) + "\n")
        summary_path.parent.mkdir(parents=True, exist_ok=True)
        summary = {
            "command": "dvc repro docs/data/m5_05/m3_09_diagnosis/dvc.yaml:m5_05_m3_09_diagnosis",
            "details": details,
            "spec_files": [PLAN.as_posix(), OLD_REPORT.as_posix(), CURRENT_SUMMARY.as_posix()],
            "harness_files": [WORKER.as_posix(), "analysis/simciv_analysis/m5_05_diagnosis.py"],
            "raw_outputs": [RAW_REL.as_posix()],
            "stage": "m3_09_diagnosis",
            "type": "diagnostic-timing",
        }
        summary_path.write_text(json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
        if summary_path.stat().st_size > 1024 * 1024:
            raise ValueError("diagnosis summary exceeds the M5-00 1 MiB committed-file limit")
        return summary
    finally:
        for worker in reversed(workers):
            worker.close()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / RAW_REL)
    parser.add_argument("--summary", type=Path, default=ROOT / SUMMARY_REL)
    parser.add_argument("--plan-only", action="store_true", help="validate the registered revisions and print their exact commits")
    args = parser.parse_args(argv)
    plan = _read_plan()
    if args.plan_only:
        for item in plan["revisions"]:
            resolved = _git(ROOT, "rev-parse", f"{item['commit']}^{{commit}}").stdout.strip()
            if resolved != item["commit"]:
                raise ValueError(f"revision does not resolve exactly: {item['id']}")
            print(f"{item['id']} {resolved} — {item['role']}")
        return 0
    output_dir = args.output_dir if args.output_dir.is_absolute() else ROOT / args.output_dir
    summary_path = args.summary if args.summary.is_absolute() else ROOT / args.summary
    run_diagnosis(output_dir, summary_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
