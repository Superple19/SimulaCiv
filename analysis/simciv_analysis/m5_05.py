"""Run or validate the frozen M5-05 conclusion-level evidence plan."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

from .provenance import ROOT
from .io import write_text_lf


REGISTRY = Path("docs/reproducibility/m5_05_conclusions.toml")
MEASURE_HARNESS = Path("crates/sim-model/examples/m5_05_conclusion_measure.rs")
MEASURE_HARNESS_SESSION_23 = Path("crates/sim-model/examples/m5_05_sessions23.rs")


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


def _m3_06_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(root / entry["report"], lambda line: "| Run | Serial median ± MAD µs/tick |" in line)
    before = next((row for row in rows if row[0] == "Before"), None)
    if before is None:
        return []
    return [{
        "cell": "M3-indexed-N1000000-K5000-k8-p0.5-day0",
        "indexed_before_us": _number(before[1]),
        "source_report": entry["report"],
        "source_match": "historical before value; executable pre-index source is unrecoverable",
    }]


def _m3_07_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(root / entry["report"], lambda line: "| Phase | N=100k slowdown |" in line)
    row = next((item for item in rows if item and item[0] == "P4 action selection"), None)
    if row is None or len(row) < 4:
        return []
    return [{
        "cell": "P4-selection-N1000000-K5000-k8-p0.5",
        "reversed_over_normal": _number(row[3]),
        "source_report": entry["report"],
        "source_match": "same selected Phase 4 selection cell",
    }]


def _m3_08_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(root / entry["report"], lambda line: "| Initial N | Live agents | M3P median µs/tick |" in line)
    row = next((item for item in rows if item and item[0].replace(",", "") == "100000"), None)
    if row is None:
        return []
    return [{
        "cell": "initial-N100000-live103241-k8-p0.5-day400",
        "old_ratio": _number(row[4]),
        "source_report": entry["report"],
        "source_match": "same equal-live population and three-day shape",
    }]


def _m3_09_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    rows = _table(root / entry["report"], lambda line: "| Initial N | Schedule | M3P serial |" in line)
    row = next((item for item in rows if item and item[0].replace(",", "") == "100000" and item[1] == "Paired"), None)
    if row is None:
        return []
    return [{
        "cell": "initial-N100000-live103241-k8-p0.5-day400",
        "old_ratio": _number(row[5]),
        "source_report": entry["report"],
        "source_match": "same paired equal-live population and three-day shape",
    }]


def _m3_11_values(entry: dict[str, Any], root: Path) -> list[dict[str, Any]]:
    text = (root / entry["report"]).read_text(encoding="utf-8")
    if entry["id"] == "m3_11_phase9c_speedup":
        match = re.search(r"Phase 9c fell from ([0-9.]+) ms serial to ([0-9.]+) ms on pool 6", text)
        if not match:
            return []
        return [{
            "cell": "M3PROP-P9c-N100000-k16-hops16-pool6",
            "serial_median_us": float(match.group(1)) * 1000.0,
            "parallel_median_us": float(match.group(2)) * 1000.0,
            "source_report": entry["report"],
            "source_match": "same fixed-topology N=100k Phase 9c cell",
        }]
    if entry["id"] == "m3_11_whole_tick_speedup":
        match = re.search(r"whole tick improved by ([0-9.]+)× interleaved and ([0-9.]+)× non-interleaved", text)
        return [{
            "cell": "M3PROP-whole-tick-N100000-k16-hops16-pool6",
            "interleaved_speedup": float(match.group(1)) if match else None,
            "non_interleaved_speedup": float(match.group(2)) if match else None,
            "source_report": entry["report"],
            "source_match": "same fixed-topology N=100k whole-tick cell",
        }] if match else []
    match = re.search(r"frontier sizes of ([0-9,]+) sources for pools 2 and 4, and ([0-9,]+) for pool 6", text)
    if not match:
        return []
    return [{
        "cell": "M3PROP-frontier-k8-N10000-hops8-16-transmit0.2",
        "N": 10_000,
        "frontier_pool2_4": int(match.group(1).replace(",", "")),
        "frontier_pool6": int(match.group(2).replace(",", "")),
        "source_report": entry["report"],
        "source_match": "same selected day-zero frontier cells",
    }]


def historical_values(entry: dict[str, Any], root: Path = ROOT) -> list[dict[str, Any]]:
    return {
        "m2_near_linear": _m2_40_values,
        "m2_final_tick": _m2_40_values,
        "m2_phase4_crossover": _m2_38_crossover,
        "m3_01_serial_baseline": _m3_01_values,
        "m3_02_parallel_phase6b": _m3_02_values,
        "m3_03_search_reduction": _m3_03_values,
        "m3_06_settlement_index": _m3_06_values,
        "m3_07_reversed_slots": _m3_07_values,
        "m3_08_full_rebuild_equal_live": _m3_08_values,
        "m3_09_incremental_equal_live": _m3_09_values,
        "m3_11_phase9c_speedup": _m3_11_values,
        "m3_11_whole_tick_speedup": _m3_11_values,
        "m3_11_frontier_crossover": _m3_11_values,
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


def validate_implemented_stages(root: Path = ROOT) -> list[str]:
    entries = validate_registry(root)
    registry_ids = {entry["id"] for entry in entries}
    pipeline = (root / "docs" / "data" / "m5_05" / "dvc.yaml").read_text(encoding="utf-8")
    stage_ids = set(re.findall(r"^\s+m5_05_([a-z0-9_]+):\s*$", pipeline, flags=re.MULTILINE))
    stage_ids.discard("deterministic")
    if stage_ids != registry_ids:
        raise ValueError(
            f"DVC conclusion stages differ from registry: missing={sorted(registry_ids - stage_ids)}, "
            f"extra={sorted(stage_ids - registry_ids)}"
        )

    def declared(path: Path, constant: str) -> set[str]:
        source = (root / path).read_text(encoding="utf-8")
        match = re.search(rf"const {constant}: &\[&str\] = &\[(.*?)\];", source, flags=re.DOTALL)
        if not match:
            raise ValueError(f"measurement harness has no {constant} declaration: {path}")
        return set(re.findall(r'"([a-z0-9_]+)"', match.group(1)))

    implemented = declared(MEASURE_HARNESS, "SESSION_ONE_ENTRIES")
    implemented.update(declared(MEASURE_HARNESS_SESSION_23, "SUPPORTED_ENTRIES"))
    if implemented != registry_ids:
        raise ValueError(
            f"conclusion entries lack a measurement path: missing={sorted(registry_ids - implemented)}, "
            f"extra={sorted(implemented - registry_ids)}"
        )
    return sorted(registry_ids)


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
    write_text_lf(path, json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n")


def run_deterministic(session: int, raw_dir: Path, summary_path: Path, root: Path = ROOT) -> dict[str, Any]:
    entries = validate_registry(root)
    if session != 1:
        raise ValueError("only the explicitly authorized M5-05 Session 1 may run in this execution")
    targets = _test_targets(entries, session)
    log_path = raw_dir / "deterministic-tests.log"
    raw_dir.mkdir(parents=True, exist_ok=True)
    results: list[dict[str, str]] = []
    log_parts = []
    write_text_lf(log_path, "")
    for target in targets:
        test_path = root / "crates" / "sim-model" / "tests" / f"{target}.rs"
        if not test_path.is_file():
            write_text_lf(log_path, "".join(log_parts))
            raise ValueError(f"registry names a missing Rust integration test: {target}")
        command = ["cargo", "test", "-p", "sim-model", "--test", target]
        log_parts.append("COMMAND " + subprocess.list2cmdline(command) + "\n")
        write_text_lf(log_path, "".join(log_parts))
        result = subprocess.run(command, cwd=root, capture_output=True, text=True)
        log_parts.extend((result.stdout, result.stderr))
        write_text_lf(log_path, "".join(log_parts))
        results.append({"target": target, "status": "pass" if result.returncode == 0 else "fail"})
        if result.returncode != 0:
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
    if not _timing_session_authorized(int(entry["session"])):
        raise ValueError(f"Session {entry['session']} timing is outside the currently authorized work")
    raw_dir.mkdir(parents=True, exist_ok=True)
    samples_path = raw_dir / "samples.jsonl"
    log_path = raw_dir / "run.log"
    harness = MEASURE_HARNESS if int(entry["session"]) == 1 else MEASURE_HARNESS_SESSION_23
    command = [
        "cargo",
        "run",
        "--release",
        "-p",
        "sim-model",
        "--example",
        harness.name.removesuffix(".rs"),
        "--",
        "--entry",
        entry_id,
        "--output",
        str(samples_path),
    ]
    print(f"TIMING RUN START entry={entry_id}", flush=True)
    result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    write_text_lf(log_path, result.stdout + result.stderr)
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
    recorded_overrides = list(entry.get("config_overrides", []))
    for cell in cell_records:
        override = cell.get("result", {}).get("config_override")
        if isinstance(override, str) and override not in recorded_overrides:
            recorded_overrides.append(override)
    summary = _summary_template(
        stage=entry_id,
        specs=[str(REGISTRY), config, entry["report"]],
        harnesses=[str(path) for path in [
            str(root / "analysis" / "simciv_analysis" / "m5_05.py"),
            str(root / harness),
            str(root / entry["harness"]),
        ] if Path(path).is_file()],
        raw_output=raw_dir.relative_to(root).as_posix(),
        details={
            "entry": entry_id,
            "cells": entry["cells"],
            "config_overrides": recorded_overrides,
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


def _timing_session_authorized(session: int) -> bool:
    raw = os.environ.get("SIMCIV_M5_05_AUTHORIZED_SESSIONS", "1")
    entries = [value.strip() for value in raw.split(",")]
    if not entries or any(not value.isdecimal() for value in entries):
        raise ValueError("SIMCIV_M5_05_AUTHORIZED_SESSIONS must be a comma-separated list of session numbers")
    return session in {int(value) for value in entries}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true", help="verify that every registered DVC conclusion has a measurement implementation")
    subparsers = parser.add_subparsers(dest="mode")
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
    if args.dry_run:
        for entry_id in validate_implemented_stages():
            print(f"IMPLEMENTED|{entry_id}")
        return 0
    if args.mode is None:
        parser.error("a mode is required unless --dry-run is used")
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
