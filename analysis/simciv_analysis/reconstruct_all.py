"""Preflight and orchestrate the remaining M2/M3 reconstruction stages."""

from __future__ import annotations

import argparse
import csv
import subprocess
import sys
import tomllib
from collections import OrderedDict
from pathlib import Path
from typing import Any

from .provenance import ROOT
from .reconstruction import run_m2_06_deterministic, run_protocol_sensitivity


INVENTORY = Path("docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv")
SENSITIVITY_SPEC = Path("docs/reproducibility/M3_05_PROTOCOL_SENSITIVITY.toml")
PILOT_RUNTIME = Path("docs/reproducibility/M3_05_PILOT_RUNTIME.csv")
SENSITIVITY_SMOKE_RAW = Path("target/reconstruction-smoke/m3_05_protocol_sensitivity/records.jsonl")
DETERMINISTIC_SMOKE_RAW = Path("target/reconstruction-smoke/m2_06_deterministic/records.jsonl")
SENSITIVITY_DVC = Path("docs/data/m2_m3_reconstruction/dvc.yaml")
RUN_ORDER = (
    "M2-06-telemetry",
    "M2-09-allocation-summary",
    "M2-16_1-phase10-ablation",
    "M2-18-prior-poc",
    "M2-31-reprofile",
    "M2-34-reprofile",
    "M2-37-reprofile",
    "M2-40-conclusion-cells",
    "M2-X-optimization-ledger",
    "M3-03-candidate-decisions",
    "M3-03-day-grid",
    "M3-04-sustained-grid",
    "M3-07-residual",
    "M3-08-population-profile",
    "M3-09-incremental-profile",
)


def _plan(root: Path = ROOT) -> tuple[list[dict[str, Any]], int, float, float, int, int, list[dict[str, str]]]:
    with (root / INVENTORY).open(encoding="utf-8-sig", newline="") as stream:
        rows = list(csv.DictReader(stream))
    groups: OrderedDict[str, dict[str, Any]] = OrderedDict()
    cells = 0
    floor_seconds = 0.0
    model_seconds = 0.0
    remaining_rows = 0
    zero_cell_rows = 0
    zero_cell_details: list[dict[str, str]] = []
    for row in rows:
        if row.get("classification") not in {"a", "b", "c", "d"}:
            continue
        if row.get("experiment_id") == "M3-05-FIRST-BREAK":
            continue
        remaining_rows += 1
        run_id = row.get("A5_run_id", "")
        cell_count = int(row.get("A5_cells_to_schedule") or 0)
        if cell_count <= 0 or run_id.startswith("derived:"):
            zero_cell_rows += 1
            zero_cell_details.append(
                {
                    "experiment_id": row["experiment_id"],
                    "classification": row["classification"],
                    "report": row["report"],
                    "anchor": row["section_anchor"],
                    "reason": row.get("A5_cell_count_reason", ""),
                }
            )
            continue
        group = groups.setdefault(
            run_id,
            {
                "run_id": run_id,
                "report": row["report"],
                "revision": row["report_written_commit"],
                "toolchain": row.get("report_revision_rust_toolchain", ""),
                "driver_family": _driver_family(row),
                "cells": 0,
                "model_seconds": 0.0,
                "floor_seconds": 0.0,
                "experiments": [],
            },
        )
        group["cells"] += cell_count
        group["model_seconds"] += float(row.get("A5_model_estimate_seconds") or 0.0)
        group["floor_seconds"] += float(row.get("A5_lower_bound_seconds") or 0.0)
        group["experiments"].append(
            {
                "experiment_id": row["experiment_id"],
                "cells": cell_count,
                "N": row.get("N_values", ""),
                "K": row.get("K_values", ""),
                "k": row.get("relationship_k_values", ""),
                "days": row.get("days_as_reported", ""),
            }
        )
        cells += cell_count
        floor_seconds += float(row.get("A5_lower_bound_seconds") or 0.0)
        model_seconds += float(row.get("A5_model_estimate_seconds") or 0.0)
    ordered = [groups[run_id] for run_id in RUN_ORDER if run_id in groups]
    if set(groups) != {group["run_id"] for group in ordered}:
        missing = sorted(set(groups) - {group["run_id"] for group in ordered})
        raise ValueError(f"reconstruction run IDs absent from fixed order: {missing}")
    return ordered, cells, floor_seconds, model_seconds, remaining_rows, zero_cell_rows, zero_cell_details


def _driver_family(row: dict[str, str]) -> str:
    harness = row.get("mapped_harnesses", "")
    report = row.get("report", "")
    if "m0_baseline_bench.rs" in harness:
        return "m2_baseline"
    if "m3_02b_search_reduction_bench.rs" in harness:
        return "m3_search_reduction"
    if "m3_sustained_workload_bench.rs" in harness:
        return "m3_sustained"
    if "m3_scale_limit_bench.rs" in harness or report == "M3_07_SCALE_RESIDUAL.md":
        return "m3_scale_profile"
    if report == "M3_08_POPULATION_DYNAMICS.md":
        return "m3p_population"
    if report == "M3_09_INCREMENTAL_TOPOLOGY.md":
        return "m3p_incremental"
    return "unmapped"


def _protocol_cells(root: Path = ROOT) -> set[tuple[int, int, int]]:
    spec = tomllib.loads((root / SENSITIVITY_SPEC).read_text(encoding="utf-8"))
    populations = spec["populations"]
    cells = {
        (population, row["k"], row["day"])
        for row in spec["first_break_change"]
        for population in populations
    }
    for row in spec["not_reproduced_alpha"]:
        cells.add((row["from_N"], row["k"], row["day"]))
        cells.add((row["to_N"], row["k"], row["day"]))
    if len(cells) != spec["unique_phase_population_series"]:
        raise ValueError("pre-registered protocol-sensitivity cells do not match their count")
    return cells


def _protocol_estimate_seconds(root: Path = ROOT) -> float | None:
    pilot_runtime = root / PILOT_RUNTIME
    if not pilot_runtime.is_file():
        return None
    per_cell: dict[tuple[int, int, int], tuple[float, int]] = {}
    with pilot_runtime.open(encoding="utf-8-sig", newline="") as stream:
        for row in csv.DictReader(stream):
            if row.get("series_type") != "phase_profile":
                continue
            key = (int(row["N"]), int(row["k"]), int(row["horizon"]))
            repetitions = int(row["repetitions_per_sample"])
            samples = int(row["samples"])
            timed_seconds = float(row["timed_samples_seconds"])
            if repetitions > 0 and samples > 0:
                per_cell[key] = (timed_seconds / (samples * repetitions), repetitions)
    cells = _protocol_cells(root)
    if not cells.issubset(per_cell):
        return None
    # Use only committed timing summaries so a clean checkout can plan without DVC raw data.
    return sum(9.0 * per_cell[cell][0] for cell in cells)


def dry_run(root: Path = ROOT) -> dict[str, Any]:
    groups, cells, floor_seconds, model_seconds, remaining_rows, zero_cell_rows, zero_cell_details = _plan(root)
    protocol_cells = _protocol_cells(root)
    protocol_estimate = _protocol_estimate_seconds(root)
    total_estimate = None if protocol_estimate is None else model_seconds + protocol_estimate
    return {
        "groups": groups,
        "remaining_group_rows": remaining_rows,
        "zero_cell_group_rows": zero_cell_rows,
        "zero_cell_details": zero_cell_details,
        "a5_cells": cells,
        "a5_floor_seconds": floor_seconds,
        "a5_model_seconds": model_seconds,
        "protocol_cells": sorted(protocol_cells),
        "protocol_target_contrasts": 15,
        "protocol_estimate_seconds": protocol_estimate,
        "timing_cells_total": cells + len(protocol_cells),
        "total_estimated_seconds": total_estimate,
    }


def _print_dry_run(plan: dict[str, Any]) -> None:
    print("Remaining historical A-5 groups (fixed execution order):")
    for index, group in enumerate(plan["groups"], start=1):
        print(
            f"{index:02d}. {group['run_id']}: {group['cells']} cells; "
            f"source family {group['driver_family']}; model={group['model_seconds']:.1f}s; "
            f"report {group['revision'][:10]} / {group['toolchain']}"
        )
        for experiment in group["experiments"]:
            dims = ", ".join(
                f"{name}={experiment[name].strip()}"
                for name in ("N", "K", "k", "days")
                if experiment[name]
                and "not stated" not in experiment[name].lower()
                and "not captured" not in experiment[name].lower()
                and not experiment[name].strip().startswith("L")
            ) or "dimensions not captured in inventory"
            print(f"    {experiment['experiment_id']}: {experiment['cells']} cells ({dims})")
    print(
        f"A-5 total: {plan['a5_cells']} cells; floor={plan['a5_floor_seconds']:.0f}s; "
        f"modeled={plan['a5_model_seconds']:.1f}s"
    )
    print(
        f"Inventory coverage: {plan['remaining_group_rows']} classes (a-d) group rows; "
        f"{plan['zero_cell_group_rows']} rows require no new timed cell (deterministic, derived, or shared)."
    )
    print("Rows with no separate timed cell:")
    for row in plan["zero_cell_details"]:
        reason = row["reason"] or "inventory has no scheduled cell"
        print(f"    {row['experiment_id']} [{row['classification']}] {row['report']}#{row['anchor']}: {reason}")
    protocol_estimate = plan["protocol_estimate_seconds"]
    estimate_text = "unavailable" if protocol_estimate is None else f"{protocol_estimate:.1f}s (from committed M3-05 per-series timing totals)"
    print(
        f"M3-05 protocol-sensitivity add-on: {len(plan['protocol_cells'])} unique profile/N cells, "
        f"15 target contrasts; estimated={estimate_text}; historical 2/7/3 protocol, no process controls"
    )
    print("Protocol-sensitivity cells: " + "; ".join(f"N={n}/k={k}/day={day}" for n, k, day in plan["protocol_cells"]))
    if protocol_estimate := plan["protocol_estimate_seconds"]:
        total = plan["total_estimated_seconds"]
        print(
            f"All planned timing entries: {plan['timing_cells_total']} cells; "
            f"modeled total={total:.1f}s ({total / 3600:.2f}h)"
        )
    pending = [group["run_id"] for group in plan["groups"]]
    print("No A-5 driver yet: " + ", ".join(pending))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--smoke", action="store_true")
    args = parser.parse_args(argv)
    plan = dry_run()
    _print_dry_run(plan)
    if args.dry_run:
        return 0
    if args.smoke:
        (ROOT / DETERMINISTIC_SMOKE_RAW).unlink(missing_ok=True)
        (ROOT / SENSITIVITY_SMOKE_RAW).unlink(missing_ok=True)
        run_m2_06_deterministic(DETERMINISTIC_SMOKE_RAW.parent)
        run_protocol_sensitivity(SENSITIVITY_SMOKE_RAW.parent, smoke=True)
        print("Smoke-tested the two prepared drivers: M2-06 deterministic and M3-05 protocol sensitivity.")
        if plan["groups"]:
            print("Historical A-5 group drivers still pending: " + ", ".join(group["run_id"] for group in plan["groups"]))
        return 0
    if plan["groups"]:
        raise SystemExit(
            "refusing to start an incomplete overnight run; build the listed A-5 group drivers first"
        )
    result = subprocess.run(
        ["dvc", "repro", "--file", str(SENSITIVITY_DVC)],
        cwd=ROOT,
        check=False,
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
