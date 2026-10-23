"""A-5 bisect runner for the M3-09 M3P/M3 paired timing change."""

from __future__ import annotations

import argparse
import json
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


def command(args: list[str], cwd: Path, check: bool = True, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, check=check)


def med_mad(values: list[float]) -> tuple[float, float]:
    median = statistics.median(values)
    return median, statistics.median(abs(value - median) for value in values)


def changed_from_baseline(numerator: float, denominator: float, ratio: float, plan: dict[str, Any]) -> bool:
    threshold = float(plan["change_screen_percent"]) / 100.0
    values = (
        numerator / float(plan["baseline_numerator_us_per_tick"]) - 1,
        denominator / float(plan["baseline_denominator_us_per_tick"]) - 1,
        ratio / float(plan["baseline_ratio"]) - 1,
    )
    return any(abs(value) > threshold for value in values)


def _self_check() -> None:
    plan = {
        "baseline_numerator_us_per_tick": 100.0,
        "baseline_denominator_us_per_tick": 50.0,
        "baseline_ratio": 2.0,
        "change_screen_percent": 20.0,
    }
    assert not changed_from_baseline(100.0, 50.0, 2.0, plan)
    assert changed_from_baseline(79.0, 50.0, 1.58, plan)


def repo_relative(root: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else root / path


def worker_command(process: subprocess.Popen[str], operation: str, sample: int, repetitions: int) -> dict[str, float | int]:
    assert process.stdin is not None and process.stdout is not None
    process.stdin.write(f"{operation}|{sample}|{repetitions}\n")
    process.stdin.flush()
    line = process.stdout.readline()
    fields = line.rstrip("\r\n").split("|")
    expected = "PROBE" if operation == "probe" else operation
    if not fields or fields[0] != expected:
        stderr = process.stderr.read() if process.stderr else ""
        raise RuntimeError(f"bad worker reply: {line}\n{stderr}")
    return {
        "sample": int(fields[1]),
        "repetitions": int(fields[2]),
        "numerator_us_per_tick": float(fields[3]),
        "denominator_us_per_tick": float(fields[4]),
        "pair_wall_seconds": float(fields[5]),
    }


def measure_candidate(args: argparse.Namespace) -> int:
    root = Path.cwd().resolve()
    plan = tomllib.loads(Path(args.plan).read_text(encoding="utf-8"))
    commit = command(["git", "rev-parse", "HEAD"], root).stdout.strip()
    worker_source = Path(args.worker_source).resolve()
    config = Path(args.config).resolve()
    worker_dest = root / "crates/sim-model/examples/m5_05_m3_09_revision.rs"
    shutil.copy2(worker_source, worker_dest)
    target_dir = Path(args.build_dir).resolve() / commit[:12]
    env = os.environ.copy()
    env.update({
        "CARGO_TARGET_DIR": str(target_dir),
        "SIMCIV_M5_05_CONFIG": str(config),
        "SIMCIV_M5_05_REVISION_ID": f"bisect-{commit[:12]}",
        "SIMCIV_M5_05_REVISION_COMMIT": commit,
    })
    started = time.perf_counter()
    built = command(["cargo", "build", "--release", "-p", "sim-model", "--example", "m5_05_m3_09_revision"], root, check=False, env=env)
    if built.returncode:
        print(json.dumps({"commit": commit, "status": "skip", "reason": "build failed", "stderr": built.stderr[-4000:]}), flush=True)
        return 125
    exe = target_dir / "release/examples/m5_05_m3_09_revision.exe"
    if not exe.is_file():
        print(json.dumps({"commit": commit, "status": "skip", "reason": f"built example not found: {exe}"}), flush=True)
        return 125
    try:
        process = subprocess.Popen(
            [str(exe), "--server"], cwd=root, env=env, text=True, encoding="utf-8",
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=1,
        )
    except OSError as error:
        print(json.dumps({"commit": commit, "status": "skip", "reason": str(error)}), flush=True)
        return 125
    try:
        line = process.stdout.readline() if process.stdout else ""
        fields = line.rstrip("\r\n").split("|")
        if len(fields) != 13 or fields[0] != "READY":
            stderr = process.stderr.read() if process.stderr else ""
            raise RuntimeError(f"invalid worker ready line: {line}\n{stderr}")
        ready = {
            "commit": fields[2],
            "config_sha256": fields[3],
            "live_population": int(fields[4]),
            "execution_options": fields[5],
            "m3p_digests": dict(zip(("state", "metrics", "events"), fields[6:9], strict=True)),
            "m3_digests": dict(zip(("state", "metrics", "events"), fields[9:12], strict=True)),
            "process_controls": fields[12],
        }
        if ready["commit"] != commit:
            raise RuntimeError("worker reported another commit")
        attempts = []
        for attempt in range(2):
            for warmup in range(int(plan["warmups"])):
                result = worker_command(process, "warmup", attempt * 100 + warmup, int(plan["repetitions_per_sample"]))
                if float(result["pair_wall_seconds"]) < float(plan["minimum_pair_wall_seconds"]):
                    raise RuntimeError("warmup pair shorter than A-5 minimum")
            print(f"TIMING RUN START bisect {commit} attempt {attempt + 1}", flush=True)
            samples = []
            for sample in range(int(plan["samples"])):
                result = worker_command(process, "sample", attempt * 100 + sample, int(plan["repetitions_per_sample"]))
                if float(result["pair_wall_seconds"]) < float(plan["minimum_pair_wall_seconds"]):
                    raise RuntimeError("sample pair shorter than A-5 minimum")
                samples.append(result)
            print(f"TIMING RUN END bisect {commit} attempt {attempt + 1}", flush=True)
            numerator, numerator_mad = med_mad([float(row["numerator_us_per_tick"]) for row in samples])
            denominator, denominator_mad = med_mad([float(row["denominator_us_per_tick"]) for row in samples])
            ratio = numerator / denominator
            spreads = {
                "numerator_mad_over_median": numerator_mad / numerator,
                "denominator_mad_over_median": denominator_mad / denominator,
            }
            attempts.append({"attempt": attempt + 1, "samples": samples, "spreads": spreads})
            if max(spreads.values()) <= float(plan["max_path_mad_over_median"]):
                break
        max_spread = max(spreads.values())
        max_spread = max(spreads.values())
        record = {
            "commit": commit,
            "ready": ready,
            "samples": samples,
            "attempts": attempts,
            "numerator_median_us_per_tick": numerator,
            "numerator_mad_us_per_tick": numerator_mad,
            "denominator_median_us_per_tick": denominator,
            "denominator_mad_us_per_tick": denominator_mad,
            "ratio": ratio,
            "spreads": spreads,
            "repetitions_per_sample": int(plan["repetitions_per_sample"]),
            "sample_count": len(samples),
            "wall_seconds": time.perf_counter() - started,
            "status": "measured",
        }
        if max_spread > float(plan["max_path_mad_over_median"]):
            record["status"] = "inconclusive"
            code = 125
        else:
            changes = {
                "numerator": numerator / float(plan["baseline_numerator_us_per_tick"]) - 1,
                "denominator": denominator / float(plan["baseline_denominator_us_per_tick"]) - 1,
                "ratio": ratio / float(plan["baseline_ratio"]) - 1,
            }
            changed = changed_from_baseline(numerator, denominator, ratio, plan)
            record["baseline_relative_changes"] = changes
            record["changed"] = changed
            record["status"] = "changed" if changed else "unchanged"
            code = 1 if changed else 0
        raw_dir = Path(args.output_dir).resolve()
        raw_dir.mkdir(parents=True, exist_ok=True)
        (raw_dir / f"{commit[:12]}.json").write_text(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
        print(json.dumps({key: record[key] for key in ("commit", "status", "numerator_median_us_per_tick", "denominator_median_us_per_tick", "ratio", "spreads", "baseline_relative_changes", "changed") if key in record}, sort_keys=True), flush=True)
        return code
    except Exception as error:  # a failed candidate is a bisect skip, never a false timing classification
        print(json.dumps({"commit": commit, "status": "skip", "reason": str(error)}), flush=True)
        return 125
    finally:
        if process.poll() is None:
            try:
                assert process.stdin is not None
                process.stdin.write("stop|0|1\n")
                process.stdin.flush()
                process.wait(timeout=30)
            except Exception:
                process.kill()


def run_bisect(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.repo).resolve()
    plan = tomllib.loads(repo_relative(root, args.plan).read_text(encoding="utf-8"))
    worker = repo_relative(root, args.worker).resolve()
    config = repo_relative(root, args.config).resolve()
    clone = repo_relative(root, args.clone_dir).resolve()
    build = repo_relative(root, args.build_dir).resolve()
    output = repo_relative(root, args.output_dir).resolve()
    summary_path = repo_relative(root, args.summary).resolve()
    for path in (clone, build, output):
        path.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise FileExistsError(f"refusing to overwrite bisect raw files in {output}")
    if any(clone.iterdir()):
        raise FileExistsError(f"refusing to reuse non-empty bisect clone {clone}")
    command(["git", "clone", "--shared", "--no-checkout", str(root), str(clone)], root)
    command(["git", "checkout", "--detach", plan["changed_revision"]], clone)
    command(["git", "bisect", "start", plan["changed_revision"], plan["good_revision"]], clone)
    run_command = [
        "git", "bisect", "run", sys.executable, str(Path(__file__).resolve()),
        "--candidate", "--worker-source", str(worker), "--config", str(config),
        "--build-dir", str(build), "--output-dir", str(output),
        "--plan", str(repo_relative(root, args.plan)),
    ]
    process = subprocess.Popen(run_command, cwd=clone, text=True, encoding="utf-8", stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=1)
    lines = []
    assert process.stdout is not None
    for line in process.stdout:
        print(line, end="", flush=True)
        lines.append(line.rstrip("\r\n"))
    return_code = process.wait()
    bisect_log = command(["git", "bisect", "log"], clone, check=False).stdout
    bad_line = next((line for line in lines if "is the first bad commit" in line), None)
    first_bad = bad_line.split()[0] if bad_line else None
    candidate_records = []
    for path in sorted(output.glob("*.json")):
        candidate_records.append(json.loads(path.read_text(encoding="utf-8")))
    if not candidate_records or first_bad is None:
        raise RuntimeError("git bisect produced no valid measured change point")
    summary = {
        "plan": {"path": Path(args.plan).as_posix(), "values": plan},
        "first_changed_commit": first_bad,
        "bisect_exit_code": return_code,
        "bisect_log": bisect_log,
        "tested_candidates": candidate_records,
        "candidate_count": len(candidate_records),
        "host": {"system": platform.system(), "release": platform.release(), "machine": platform.machine(), "processor": platform.processor()},
        "protocol": "A-5 bisect diagnostic; 2 warmups; 21 samples; r=3 fixed from Step 2; threshold from same-session M3-09 follow-up median",
    }
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
    command(["git", "bisect", "reset"], clone, check=False)
    if return_code != 0:
        raise RuntimeError(f"git bisect run failed: {return_code}; inspect the saved bisect log")
    return summary


def main() -> int:
    _self_check()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--candidate", action="store_true")
    parser.add_argument("--repo", default=".")
    parser.add_argument("--plan", required=True)
    parser.add_argument("--worker", default="crates/sim-model/examples/m5_05_m3_09_revision.rs")
    parser.add_argument("--worker-source")
    parser.add_argument("--config", required=True)
    parser.add_argument("--clone-dir", default="target/m5_05_m3_09_diagnosis/bisect-clone")
    parser.add_argument("--build-dir", default="target/m5_05_m3_09_diagnosis/bisect-build")
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--summary", default="docs/data/m5_05/m3_09_diagnosis/bisect_summary.json")
    args = parser.parse_args()
    if args.candidate:
        if not args.worker_source:
            raise ValueError("candidate mode requires --worker-source")
        return measure_candidate(args)
    if not args.run:
        parser.error("choose --run or --candidate")
    run_bisect(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
