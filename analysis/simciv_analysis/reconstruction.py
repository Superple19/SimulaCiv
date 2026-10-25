"""Historical M3-05 protocol-sensitivity driver launcher."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path
from typing import Any

from .io import write_text_lf
from .provenance import ROOT


REVISION = "5df006c150179f67078588dfaf83a355a81d5f9f"
REPORT = "docs/reports/M3_05_SCALE_LIMIT.md"
SPEC = Path("docs/reproducibility/M3_05_PROTOCOL_SENSITIVITY.toml")
DRIVER = Path("crates/sim-model/examples/m3_05_protocol_sensitivity.rs")
BENCH = Path("crates/sim-model/benches/m3_scale_limit_bench.rs")
STAGE = "m3_05_protocol_sensitivity"
DETERMINISTIC_STAGE = "m2_06_determinism"
DETERMINISTIC_REVISION = "d130322e79eaa9f3a8e6791ba77161696c47055d"
DETERMINISTIC_TESTS = (
    "test_22_500_day_graduation_state_hash_matches_manifest",
    "test_23_500_day_graduation_metrics_hash_matches_manifest",
    "test_24_500_day_graduation_event_hash_matches_manifest",
)


def _relative_path(value: str | Path, label: str) -> tuple[str, Path]:
    candidate = Path(value)
    resolved = (candidate if candidate.is_absolute() else ROOT / candidate).resolve()
    try:
        relative = resolved.relative_to(ROOT.resolve()).as_posix()
    except ValueError as error:
        raise ValueError(f"{label} must stay inside the repository: {value}") from error
    return relative, resolved


def _verify_report_toolchain(worktree: Path, expected: str) -> None:
    toolchain = tomllib.loads((worktree / "rust-toolchain.toml").read_text(encoding="utf-8"))
    recorded = str(toolchain.get("toolchain", {}).get("channel", ""))
    if recorded != expected:
        raise ValueError(
            f"report revision toolchain mismatch: expected {expected}, found {recorded or 'unset'}"
        )
    actual = subprocess.run(
        ["rustc", "-V"], cwd=worktree, capture_output=True, text=True, check=True
    ).stdout.split()
    if len(actual) < 2 or actual[1] != expected:
        raise ValueError(
            f"active Rust toolchain mismatch at report revision: expected {expected}, "
            f"found {' '.join(actual) or 'unavailable'}"
        )


def _parse_driver_line(line: str) -> dict[str, Any]:
    fields = line.rstrip("\r\n").split("|")
    if not fields or not fields[0].startswith("RECON_"):
        raise ValueError(f"unexpected historical driver output: {line!r}")
    record_name = fields[0].removeprefix("RECON_").lower()
    record_name = {
        "header": "header",
        "series_start": "series_start",
        "checkpoint": "checkpoint_preparation",
        "calibration": "calibration",
        "sample": "sample_timing",
        "parity": "parity",
        "phase": "phase_profile_phase",
        "stage": "phase_profile_stage",
        "series_complete": "series_complete",
        "run_complete": "run_complete",
    }[record_name]
    record: dict[str, Any] = {
        "record": record_name,
        "observed_at_unix_ms": time.time_ns() // 1_000_000,
    }
    for field in fields[1:]:
        key, value = field.split("=", maxsplit=1)
        if key.endswith("_unix_ms"):
            parsed: Any = int(value)
        elif key in {"equal"}:
            parsed = value == "true"
        else:
            try:
                parsed = int(value)
            except ValueError:
                try:
                    parsed = float(value)
                except ValueError:
                    parsed = value
        record[key] = parsed
    return record


def _read_resume(raw_file: Path, spec_hash: str) -> tuple[list[dict[str, Any]], dict[str, tuple[str, float]]]:
    if not raw_file.exists():
        return [], {}
    content = raw_file.read_bytes()
    valid_end = content.rfind(b"\n") + 1
    content = content[:valid_end]
    records = [json.loads(line) for line in content.decode("utf-8").splitlines() if line.strip()]
    if records and records[0].get("record") != "header":
        raise ValueError("historical sensitivity JSONL is missing its header")
    if records and records[0].get("spec_hash") != spec_hash:
        raise ValueError("historical sensitivity raw output was produced from a different spec")

    active_start: int | None = None
    completed: dict[str, tuple[str, float]] = {}
    byte_offset = 0
    for line in content.decode("utf-8").splitlines(keepends=True):
        record = json.loads(line)
        if record.get("record") == "series_start":
            active_start = byte_offset
        elif record.get("record") == "series_complete":
            if active_start is None:
                raise ValueError("series completion has no matching start marker")
            series_id = str(record["series_id"])
            completed[series_id] = (str(record["config_hash"]), float(record["elapsed_seconds"]))
            active_start = None
        byte_offset += len(line.encode("utf-8"))
    if active_start is not None:
        raw_file.write_bytes(content[:active_start])
        records = [
            json.loads(line)
            for line in content[:active_start].decode("utf-8").splitlines()
            if line.strip()
        ]
    return records, completed


def _write_records(raw_file: Path, records: list[dict[str, Any]]) -> None:
    text = "".join(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n" for record in records)
    write_text_lf(raw_file, text, append=True, flush=True)


def _m2_06_config_hashes() -> dict[str, str]:
    return {
        name: "sha256:" + hashlib.sha256(
            f"{DETERMINISTIC_REVISION}|{name}|--exact".encode("utf-8")
        ).hexdigest()
        for name in DETERMINISTIC_TESTS
    }


def _validate_m2_06_records(records: list[dict[str, Any]]) -> None:
    hashes = _m2_06_config_hashes()
    tests = [row for row in records if row.get("record") == "deterministic_test"]
    if len(tests) != len(DETERMINISTIC_TESTS):
        raise ValueError(f"expected {len(DETERMINISTIC_TESTS)} deterministic records, found {len(tests)}")
    if {row.get("test") for row in tests} != set(DETERMINISTIC_TESTS):
        raise ValueError("deterministic test record set does not match the registered gates")
    for row in tests:
        name = str(row["test"])
        if (
            row.get("report_revision") != DETERMINISTIC_REVISION
            or row.get("config_hash") != hashes[name]
            or row.get("passed") is not True
            or row.get("exit_code") != 0
            or not isinstance(row.get("started_at_unix_ms"), int)
            or not isinstance(row.get("completed_at_unix_ms"), int)
        ):
            raise ValueError(f"deterministic record is incomplete or failed: {name}")


def _execute(raw_file: Path, smoke: bool) -> tuple[list[dict[str, Any]], str]:
    raw_file.parent.mkdir(parents=True, exist_ok=True)
    spec_bytes = (ROOT / SPEC).read_bytes()
    spec_hash = f"sha256:{hashlib.sha256(spec_bytes).hexdigest()}"
    if smoke:
        raw_file.unlink(missing_ok=True)
    records, completed = _read_resume(raw_file, spec_hash)
    target_root = ROOT / "target"
    target_root.mkdir(exist_ok=True)
    started_run = time.monotonic()
    command: list[str] = []
    active_series: str | None = None
    active_block: list[dict[str, Any]] = []
    process: subprocess.Popen[str] | None = None
    with tempfile.TemporaryDirectory(prefix="m3-05-protocol-", dir=target_root) as temporary:
        temp_root = Path(temporary).resolve()
        temp_root.relative_to(target_root.resolve())
        worktree = temp_root / "worktree"
        subprocess.run(["git", "worktree", "add", "--detach", str(worktree), REVISION], cwd=ROOT, check=True)
        try:
            _verify_report_toolchain(worktree, "1.98.1")
            historical_driver = worktree / DRIVER
            historical_driver.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / DRIVER, historical_driver)
            historical_spec = worktree / SPEC
            historical_spec.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / SPEC, historical_spec)
            environment = os.environ.copy()
            environment["CARGO_TARGET_DIR"] = str(ROOT / "target" / f"reconstruction-{REVISION[:8]}")
            command = [
                "cargo", "run", "--release", "--locked", "-p", "sim-model",
                "--example", "m3_05_protocol_sensitivity", "--",
                "--spec", str(historical_spec),
                "--skip-series", ",".join(sorted(completed)),
            ]
            if smoke:
                command.append("--smoke")
            process = subprocess.Popen(
                command,
                cwd=worktree,
                env=environment,
                text=True,
                encoding="utf-8",
                bufsize=1,
                stdout=subprocess.PIPE,
            )
            for line in process.stdout or ():
                record = _parse_driver_line(line)
                name = record["record"]
                if name == "header":
                    if not records:
                        _write_records(raw_file, [record])
                        records.append(record)
                    elif record.get("report_revision") != records[0].get("report_revision"):
                        process.kill()
                        raise ValueError("report revision changed while resuming raw output")
                    continue
                if name == "series_start":
                    if active_series is not None:
                        process.kill()
                        raise ValueError("nested historical series marker")
                    active_series = str(record["series_id"])
                    active_block = [record]
                    continue
                if name == "series_skipped":
                    series_id = str(record["series_id"])
                    expected = completed.get(series_id)
                    if expected is None or expected[0] != record.get("config_hash"):
                        process.kill()
                        raise ValueError(f"driver skipped unknown or mismatched series {series_id}")
                    continue
                if name == "series_complete":
                    if active_series != str(record["series_id"]):
                        process.kill()
                        raise ValueError("historical series completion marker does not match its start")
                    active_block.append(record)
                    _write_records(raw_file, active_block)
                    records.extend(active_block)
                    completed[active_series] = (str(record["config_hash"]), float(record["elapsed_seconds"]))
                    active_series = None
                    active_block = []
                    elapsed = time.monotonic() - started_run
                    average = elapsed / max(len(completed), 1)
                    planned = int(records[0].get("planned_series", 16))
                    remaining = max(planned - len(completed), 0)
                    print(
                        f"M3-05 protocol series {len(completed)}/{planned} elapsed={elapsed:.1f}s "
                        f"estimated_remaining={average * remaining:.1f}s: {record['series_id']}",
                        file=sys.stderr,
                        flush=True,
                    )
                    continue
                if name == "run_complete":
                    if active_series is not None:
                        process.kill()
                        raise ValueError("driver ended while a series was incomplete")
                    if not records or records[-1].get("record") != "run_complete":
                        _write_records(raw_file, [record])
                        records.append(record)
                    continue
                if active_series is not None:
                    active_block.append(record)
            returncode = process.wait()
            if returncode != 0:
                raise RuntimeError(f"historical protocol driver exited {returncode}")
            if active_series is not None:
                raise RuntimeError(f"incomplete driver series: {active_series}")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(worktree)],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )
    return records, subprocess.list2cmdline(command)


def _validate_records(records: list[dict[str, Any]], smoke: bool) -> None:
    header = next((record for record in records if record.get("record") == "header"), None)
    if not header or header.get("report_revision") != REVISION or "no_process_controls" not in header.get("protocol", ""):
        raise ValueError("historical protocol JSONL header is incomplete")
    completed = [record for record in records if record.get("record") == "series_complete"]
    expected = 1 if smoke else 16
    if header.get("planned_series") != expected:
        raise ValueError(f"header planned {header.get('planned_series')} cells, expected {expected}")
    if len(completed) != expected:
        raise ValueError(f"expected {expected} completed profile series, found {len(completed)}")
    parity = [record for record in records if record.get("record") == "parity"]
    if len(parity) != expected or any(record.get("equal") is not True for record in parity):
        raise ValueError("historical protocol parity records are missing or unequal")
    for series in completed:
        series_id = series["series_id"]
        events = [record for record in records if record.get("series_id") == series_id]
        if not any(record.get("record") == "checkpoint_preparation" for record in events):
            raise ValueError(f"checkpoint timing is missing for {series_id}")
        if not any(record.get("record") == "calibration" for record in events):
            raise ValueError(f"calibration status is missing for {series_id}")
        samples = [record for record in events if record.get("record") == "sample_timing"]
        expected_samples = 2 if smoke else 9
        if len(samples) != expected_samples or not all(
            isinstance(record.get("started_at_unix_ms"), int)
            and isinstance(record.get("completed_at_unix_ms"), int)
            for record in samples
        ):
            raise ValueError(f"timestamped warmup/sample records are invalid for {series_id}")
        if not any(record.get("record") == "phase_profile_phase" for record in events):
            raise ValueError(f"phase records are missing for {series_id}")


def run_protocol_sensitivity(
    raw_dir: str | Path,
    summary_path: str | Path | None = None,
    smoke: bool = False,
) -> dict[str, Any] | None:
    raw_relative, raw_root = _relative_path(raw_dir, "raw output directory")
    raw_root.mkdir(parents=True, exist_ok=True)
    raw_file = raw_root / "records.jsonl"
    if smoke:
        raw_file.unlink(missing_ok=True)
    records, command = _execute(raw_file, smoke)
    _validate_records(records, smoke)
    if smoke:
        return None
    if summary_path is None:
        raise ValueError("full run requires --summary")
    summary_relative, summary_file = _relative_path(summary_path, "summary file")
    summary_file.parent.mkdir(parents=True, exist_ok=True)
    summary = {
        "stage": STAGE,
        "spec_files": [SPEC.as_posix()],
        "harness_files": [
            "analysis/simciv_analysis/reconstruction.py",
            DRIVER.as_posix(),
            BENCH.as_posix(),
        ],
        "raw_outputs": [raw_relative],
        "command": command,
        "details": {
            "report_revision": REVISION,
            "report_rust_toolchain": "1.98.1",
            "protocol": "historical 2 warmups + 7 samples, 3 days/sample, no priority/affinity",
            "unique_profile_population_series": sum(
                record.get("record") == "series_complete" for record in records
            ),
            "target_contrasts": 15,
            "parity_records": sum(record.get("equal") is True for record in records),
            "timing_run": True,
        },
    }
    write_text_lf(summary_file, json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n")
    print(f"wrote summary skeleton: {summary_relative}; finalize after DVC updates its lock")
    return summary


def run_m2_06_deterministic(
    raw_dir: str | Path,
    summary_path: str | Path | None = None,
) -> dict[str, Any] | None:
    raw_relative, raw_root = _relative_path(raw_dir, "raw output directory")
    raw_root.mkdir(parents=True, exist_ok=True)
    raw_file = raw_root / "records.jsonl"
    existing: list[dict[str, Any]] = []
    if raw_file.is_file():
        existing = [
            json.loads(line)
            for line in raw_file.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
    expected_hashes = _m2_06_config_hashes()
    completed: set[str] = set()
    durations = {
        str(row["test"]): max(
            (int(row["completed_at_unix_ms"]) - int(row["started_at_unix_ms"])) / 1000.0,
            0.0,
        )
        for row in existing
        if row.get("record") == "deterministic_test"
        and isinstance(row.get("started_at_unix_ms"), int)
        and isinstance(row.get("completed_at_unix_ms"), int)
    }
    for row in existing:
        if row.get("record") != "deterministic_test":
            raise ValueError("deterministic resume output contains an unknown record")
        name = str(row.get("test", ""))
        if row.get("report_revision") != DETERMINISTIC_REVISION:
            raise ValueError("deterministic resume output belongs to a different report revision")
        if name not in expected_hashes or row.get("config_hash") != expected_hashes[name]:
            raise ValueError(f"deterministic resume config hash mismatch for {name or 'unknown test'}")
        if row.get("passed") is True:
            completed.add(name)
    target_root = ROOT / "target"
    target_root.mkdir(exist_ok=True)
    run_started = time.monotonic()
    commands: list[str] = []
    with tempfile.TemporaryDirectory(prefix="m2-06-deterministic-", dir=target_root) as temporary:
        temp_root = Path(temporary).resolve()
        worktree = temp_root / "worktree"
        subprocess.run(
            ["git", "worktree", "add", "--detach", str(worktree), DETERMINISTIC_REVISION],
            cwd=ROOT,
            check=True,
        )
        try:
            _verify_report_toolchain(worktree, "1.98.1")
            environment = os.environ.copy()
            environment["CARGO_TARGET_DIR"] = str(ROOT / "target" / f"reconstruction-{DETERMINISTIC_REVISION[:8]}")
            for test_name in DETERMINISTIC_TESTS:
                command = [
                    "cargo", "test", "--locked", "-p", "sim-model",
                    "--test", "m1_contract_manifest_tests", test_name, "--", "--exact",
                ]
                commands.append(subprocess.list2cmdline(command))
                if test_name in completed:
                    average = sum(durations.values()) / len(durations) if durations else 0.0
                    remaining = max(len(DETERMINISTIC_TESTS) - len(completed), 0)
                    print(
                        f"M2-06 deterministic {len(completed)}/{len(DETERMINISTIC_TESTS)} "
                        f"elapsed={time.monotonic() - run_started:.1f}s "
                        f"estimated_remaining={average * remaining:.1f}s already complete: {test_name}",
                        file=sys.stderr,
                        flush=True,
                    )
                    continue
                started_at = time.time_ns() // 1_000_000
                result = subprocess.run(
                    command,
                    cwd=worktree,
                    env=environment,
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                    check=False,
                )
                completed_at = time.time_ns() // 1_000_000
                record = {
                    "record": "deterministic_test",
                    "test": test_name,
                    "config_hash": expected_hashes[test_name],
                    "report_revision": DETERMINISTIC_REVISION,
                    "rust_toolchain": "1.98.1",
                    "command": subprocess.list2cmdline(command),
                    "started_at_unix_ms": started_at,
                    "completed_at_unix_ms": completed_at,
                    "exit_code": result.returncode,
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                    "passed": result.returncode == 0 and "1 passed" in result.stdout,
                }
                _write_records(raw_file, [record])
                if not record["passed"]:
                    raise RuntimeError(f"historical deterministic gate failed: {test_name}")
                elapsed = (completed_at - started_at) / 1000.0
                completed.add(test_name)
                durations[test_name] = elapsed
                average = sum(durations.values()) / len(durations)
                remaining = max(len(DETERMINISTIC_TESTS) - len(completed), 0)
                print(
                    f"M2-06 deterministic {len(completed)}/{len(DETERMINISTIC_TESTS)} "
                    f"elapsed={time.monotonic() - run_started:.1f}s "
                    f"estimated_remaining={average * remaining:.1f}s: {test_name}",
                    file=sys.stderr,
                    flush=True,
                )
        finally:
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(worktree)],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )
    _validate_m2_06_records(
        [
            json.loads(line)
            for line in raw_file.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
    )
    if summary_path is None:
        return None
    summary_relative, summary_file = _relative_path(summary_path, "summary file")
    summary_file.parent.mkdir(parents=True, exist_ok=True)
    summary = {
        "stage": DETERMINISTIC_STAGE,
        "spec_files": ["docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv"],
        "harness_files": ["analysis/simciv_analysis/reconstruction.py"],
        "raw_outputs": [raw_relative],
        "command": commands,
        "details": {
            "report_revision": DETERMINISTIC_REVISION,
            "rust_toolchain": "1.98.1",
            "tests": list(DETERMINISTIC_TESTS),
            "config_hashes": expected_hashes,
            "passed": True,
        },
    }
    write_text_lf(summary_file, json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n")
    print(f"wrote summary skeleton: {summary_relative}; finalize after DVC updates its lock")
    return summary


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    protocol = subparsers.add_parser("protocol-sensitivity")
    protocol.add_argument("--raw-dir", type=Path, required=True)
    protocol.add_argument("--summary", type=Path)
    protocol.add_argument("--smoke", action="store_true")
    deterministic = subparsers.add_parser("m2-06-deterministic")
    deterministic.add_argument("--raw-dir", type=Path, required=True)
    deterministic.add_argument("--summary", type=Path)
    args = parser.parse_args(argv)
    if args.command == "protocol-sensitivity":
        run_protocol_sensitivity(args.raw_dir, args.summary, args.smoke)
    elif args.command == "m2-06-deterministic":
        run_m2_06_deterministic(args.raw_dir, args.summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
