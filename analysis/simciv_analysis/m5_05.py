"""Run or validate the frozen M5-05 conclusion-level evidence plan."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

from .provenance import ROOT


REGISTRY = Path("docs/reproducibility/m5_05_conclusions.toml")
MEASURE_HARNESS = Path("crates/sim-model/examples/m5_05_conclusion_measure.rs")


def _table(path: Path, header_predicate) -> list[list[str]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    for index, line in enumerate(lines):
        if not header_predicate(line):
            continue
        rows: list[list[str]] = []
        for candidate in lines[index + 1 :]:
            if not candidate.lstrip().startswith("|"):
                if rows:
                    break
                continue
            cells = [part.strip() for part in candidate.strip().strip("|").split("|")]
            if not cells or all(set(cell) <= {"-", ":", " "} for cell in cells):
                continue
            rows.append(cells)
        return rows
    return []


def _number(cell: str) -> float | None:
    match = re.search(r"(?<![\w.])([0-9][0-9,]*(?:\.[0-9]+)?)", cell)
    return float(match.group(1).replace(",", "")) if match else None


def _m2_40_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    report = root / entry["report"]
    rows = _table(report, lambda line: "| Workload | N / K | Serial µs ± MAD |" in line)
    result: list[dict[str, Any]] = []
    wanted = {50_000, 100_000} if entry["id"] == "m2_near_linear" else {100_000}
    for row in rows:
        if len(row) < 7 or row[0] not in {"A", "B"}:
            continue
        n_match = re.match(r"([0-9,]+)", row[1])
        if not n_match:
            continue
        population = int(n_match.group(1).replace(",", ""))
        if population not in wanted:
            continue
        if entry["id"] == "m2_near_linear" and row[0] != "B":
            continue
        result.append({
            "cell": f"{row[0]}-N{population}",
            "serial_median_us": _number(row[2]),
            "rayon_median_us": _number(row[4]),
            "source_report": entry["report"],
            "source_match": "exact workload/population from the cited M2-40 table",
        })
    return result


def _m2_38_crossover(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(
        root / entry["report"],
        lambda line: "| Workload | N / K | Selection | Indexed Intent | Combined P4 | Full tick; ticks/s |" in line,
    )
    wanted = {1_000, 5_000, 10_000}
    result = []
    for row in rows:
        if len(row) < 6 or row[0] != "B":
            continue
        match = re.match(r"([0-9,]+)(k|M)?", row[1])
        if not match:
            continue
        population = int(match.group(1).replace(",", ""))
        if match.group(2) == "k":
            population *= 1_000
        elif match.group(2) == "M":
            population *= 1_000_000
        if population not in wanted:
            continue
        cell_text = row[5]
        speedup = re.search(r"\(([0-9.]+)×;\s*([0-9]+)/([0-9]+)\)", cell_text)
        result.append({
            "cell": f"B-N{population}",
            "reported_best_serial_us": _number(cell_text.split("→", 1)[0]),
            "reported_best_rayon_us": _number(cell_text.split("→", 1)[1]) if "→" in cell_text else None,
            "reported_best_speedup": float(speedup.group(1)) if speedup else None,
            "reported_best_pool": int(speedup.group(2)) if speedup else None,
            "reported_best_chunk": int(speedup.group(3)) if speedup else None,
            "source_report": entry["report"],
            "source_match": "reported best cell; not necessarily the selected fixed pool/chunk",
        })
    return result


def _m3_01_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(root / entry["report"], lambda line: "| N | K | k | p_local | median ± MAD (µs/tick) |" in line)
    result = []
    for row in rows:
        if len(row) < 5:
            continue
        try:
            n, k = int(row[0].replace(",", "")), int(row[2])
            p_local = float(row[3])
        except ValueError:
            continue
        if n in {10_000, 50_000, 100_000} and k == 8 and p_local == 0.5:
            result.append({
                "cell": f"N{n}-k8-p0.5",
                "storage_median_us": _number(row[4]),
                "source_report": entry["report"],
            })
    return result


def _m3_02_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    lines = (root / entry["report"]).read_text(encoding="utf-8").splitlines()
    result = []
    for index, line in enumerate(lines):
        heading = re.match(r"### Day 0 — N=([0-9,]+) / K=([0-9,]+)", line)
        if not heading:
            continue
        population = int(heading.group(1).replace(",", ""))
        if population not in {10_000, 50_000, 100_000}:
            continue
        for row_line in lines[index + 1 :]:
            if not row_line.lstrip().startswith("|"):
                if result and result[-1].get("N") == population:
                    break
                continue
            cells = [part.strip() for part in row_line.strip().strip("|").split("|")]
            if len(cells) < 6 or cells[0] in {"p_local", "---"}:
                continue
            try:
                p_local, pool, chunk = float(cells[0]), int(cells[1]), int(cells[2])
            except ValueError:
                continue
            if p_local == 0.5 and pool == 6 and chunk == 256:
                result.append({
                    "cell": f"N{population}-p0.5-pool6-chunk256",
                    "N": population,
                    "serial_median_us": _number(cells[3]),
                    "parallel_median_us": _number(cells[4]),
                    "source_report": entry["report"],
                })
                break
    return result


def _m3_03_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(root / entry["report"], lambda line: "| Profile | N | p_local | Serial | Search reduction | Paired saving |" in line)
    result = []
    for row in rows:
        if len(row) < 6 or row[0] != "M2-40 day 0":
            continue
        try:
            population, p_local = int(row[1].replace(",", "")), float(row[2])
        except ValueError:
            continue
        if population in {10_000, 100_000} and p_local == 0.5:
            result.append({
                "cell": f"N{population}-p0.5",
                "serial_median_us": _number(row[3]),
                "search_reduction_median_us": _number(row[4]),
                "paired_saving_us": _number(row[5]),
                "source_report": entry["report"],
            })
    return result


def historical_values(entry: dict[str, Any], root: Path = ROOT) -> list[dict[str, Any]]:
    return {
        "m2_near_linear": _m2_40_values,
        "m2_final_tick": _m2_40_values,
        "m2_phase4_crossover": _m2_38_crossover,
        "m3_01_serial_baseline": _m3_01_values,
        "m3_02_parallel_phase6b": _m3_02_values,
        "m3_03_search_reduction": _m3_03_values,
    }.get(entry["id"], lambda _entry, _root: [])(entry, root)


def _registry(root: Path = ROOT) -> dict[str, Any]:
    return tomllib.loads((root / REGISTRY).read_text(encoding="utf-8"))


def _root_path(path: Path) -> Path:
    return path.resolve() if path.is_absolute() else (ROOT / path).resolve()


def validate_registry(root: Path = ROOT) -> list[dict[str, Any]]:
    entries = _registry(root)["conclusion"]
    ids = [entry["id"] for entry in entries]
    if len(ids) != len(set(ids)) or len(entries) != 13:
        raise ValueError("M5-05 registry must contain 13 uniquely named conclusions")
    if sum(int(entry["estimate_minutes"]) for entry in entries) != 450:
        raise ValueError("M5-05 registry runtime estimate no longer matches the frozen 450-minute plan")
    if sorted({int(entry["session"]) for entry in entries}) != [1, 2, 3]:
        raise ValueError("M5-05 registry must use sessions 1, 2, and 3")
    for entry in entries:
        report = root / entry["report"]
        config = root / entry["config"].split(";", maxsplit=1)[0]
        if not report.is_file() or not config.is_file():
            raise ValueError(f"missing report/config for {entry['id']}")
    return entries


def _test_targets(entries: list[dict[str, Any]], session: int) -> list[str]:
    return sorted({test for entry in entries if int(entry["session"]) == session for test in entry.get("tests", [])})


def _summary_template(
    *,
    stage: str,
    specs: list[str],
    harnesses: list[str],
    raw_output: str,
    details: dict[str, Any],
) -> dict[str, Any]:
    return {
        "stage": stage,
        "spec_files": sorted(set(specs)),
        "harness_files": sorted(set(harnesses)),
        "raw_outputs": [raw_output],
        "command": details.pop("command"),
        "details": details,
    }


def _write_summary(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")


def run_deterministic(session: int, raw_dir: Path, summary_path: Path, root: Path = ROOT) -> dict[str, Any]:
    entries = validate_registry(root)
    if session != 1:
        raise ValueError("only the explicitly authorized M5-05 Session 1 may run in this execution")
    targets = _test_targets(entries, session)
    log_path = raw_dir / "deterministic-tests.log"
    raw_dir.mkdir(parents=True, exist_ok=True)
    results: list[dict[str, str]] = []
    with log_path.open("w", encoding="utf-8", newline="\n") as log:
        for target in targets:
            test_path = root / "crates" / "sim-model" / "tests" / f"{target}.rs"
            if not test_path.is_file():
                raise ValueError(f"registry names a missing Rust integration test: {target}")
            command = ["cargo", "test", "-p", "sim-model", "--test", target]
            log.write("COMMAND " + subprocess.list2cmdline(command) + "\n")
            log.flush()
            result = subprocess.run(command, cwd=root, capture_output=True, text=True)
            log.write(result.stdout)
            log.write(result.stderr)
            results.append({"target": target, "status": "pass" if result.returncode == 0 else "fail"})
            if result.returncode != 0:
                log.flush()
                raise subprocess.CalledProcessError(result.returncode, command)
    configs = [entry["config"].split(";", maxsplit=1)[0] for entry in entries if int(entry["session"]) == session]
    harnesses = [
        str(root / "analysis" / "simciv_analysis" / "m5_05.py"),
        *(str(root / "crates" / "sim-model" / "tests" / f"{target}.rs") for target in targets),
    ]
    summary = _summary_template(
        stage="m5_05_deterministic_session_1",
        specs=[str(REGISTRY), *configs],
        harnesses=harnesses,
        raw_output=raw_dir.relative_to(root).as_posix(),
        details={
            "session": session,
            "results": results,
            "command": "cargo test -p sim-model --test <target> (each Session 1 target separately)",
        },
    )
    _write_summary(summary_path, summary)
    return summary


def run_timing(entry_id: str, raw_dir: Path, summary_path: Path, root: Path = ROOT) -> dict[str, Any]:
    entries = validate_registry(root)
    entry = next((item for item in entries if item["id"] == entry_id), None)
    if entry is None:
        raise ValueError(f"unknown M5-05 entry {entry_id}")
    if int(entry["session"]) != 1:
        raise ValueError(f"Session {entry['session']} timing is outside the currently authorized work")
    raw_dir.mkdir(parents=True, exist_ok=True)
    samples_path = raw_dir / "samples.jsonl"
    log_path = raw_dir / "run.log"
    command = [
        "cargo",
        "run",
        "--release",
        "-p",
        "sim-model",
        "--example",
        MEASURE_HARNESS.name.removesuffix(".rs"),
        "--",
        "--entry",
        entry_id,
        "--output",
        str(samples_path),
    ]
    print(f"TIMING RUN START entry={entry_id}", flush=True)
    result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    log_path.write_text(result.stdout + result.stderr, encoding="utf-8", newline="\n")
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    if result.returncode != 0:
        raise subprocess.CalledProcessError(result.returncode, command)
    print(f"TIMING RUN END entry={entry_id}", flush=True)
    raw_records = [
        json.loads(line)
        for line in samples_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    header = next((record for record in raw_records if record.get("type") == "header"), None)
    cell_records = [record for record in raw_records if record.get("type") == "cell"]
    config = entry["config"].split(";", maxsplit=1)[0]
    baseline = historical_values(entry, root)
    summary = _summary_template(
        stage=entry_id,
        specs=[str(REGISTRY), config, entry["report"]],
        harnesses=[
            str(root / "analysis" / "simciv_analysis" / "m5_05.py"),
            str(root / MEASURE_HARNESS),
            str(root / entry["harness"]),
        ],
        raw_output=raw_dir.relative_to(root).as_posix(),
        details={
            "entry": entry_id,
            "cells": entry["cells"],
            "config_overrides": entry.get("config_overrides", []),
            "bench_args": entry["bench_args"],
            "host": header.get("host") if header else None,
            "process_controls": header.get("process_controls") if header else None,
            "historical_values": baseline,
            "cells_measured": cell_records,
            "timing_start_end_markers": "present in raw run.log",
            "command": subprocess.list2cmdline(command),
        },
    )
    _write_summary(summary_path, summary)
    return summary


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="mode", required=True)
    validate_parser = subparsers.add_parser("validate")
    validate_parser.set_defaults(mode="validate")
    deterministic = subparsers.add_parser("deterministic")
    deterministic.add_argument("--session", type=int, required=True)
    deterministic.add_argument("--raw-dir", type=Path, required=True)
    deterministic.add_argument("--summary", type=Path, required=True)
    timing = subparsers.add_parser("timing")
    timing.add_argument("entry")
    timing.add_argument("--raw-dir", type=Path, required=True)
    timing.add_argument("--summary", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.mode == "validate":
        entries = validate_registry()
        print(f"validated {len(entries)} M5-05 conclusion entries")
    elif args.mode == "deterministic":
        run_deterministic(args.session, _root_path(args.raw_dir), _root_path(args.summary))
    else:
        run_timing(args.entry, _root_path(args.raw_dir), _root_path(args.summary))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
