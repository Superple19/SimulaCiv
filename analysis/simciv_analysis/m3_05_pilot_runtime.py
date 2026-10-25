"""Generate the M3-05 pilot runtime breakdown and remaining-plan estimate."""

from __future__ import annotations

import argparse
import csv
import io
import json
import math
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

from .io import content_matches, write_text_lf
from .provenance import ROOT


INVENTORY = ROOT / "docs" / "reproducibility" / "M2_M3_EVIDENCE_RECONSTRUCTION.csv"
INVENTORY_REPORT = ROOT / "docs" / "reproducibility" / "M2_M3_EVIDENCE_RECONSTRUCTION.md"
RAW = ROOT / "docs" / "data" / "m5_05" / "m3_05_pilot" / "raw" / "m3_05_a5.jsonl"
SERIES_CSV = ROOT / "docs" / "reproducibility" / "M3_05_PILOT_RUNTIME.csv"
PLAN_CSV = ROOT / "docs" / "reproducibility" / "M2_M3_REMAINING_RUNTIME.csv"
COMPONENTS_CSV = ROOT / "docs" / "reproducibility" / "M3_05_PILOT_COMPONENTS.csv"
EXPERIMENT_ID = "M3-05-FIRST-BREAK"
BEGIN = "<!-- BEGIN GENERATED M3-05 RUNTIME UPDATE -->"
END = "<!-- END GENERATED M3-05 RUNTIME UPDATE -->"
REPORT_LINK = "[M3-05 A-5 harness](../../crates/sim-model/examples/m3_05_a5_reconstruction.rs)"
DATA_LINK = "`docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl`"

SERIES_FIELDS = [
    "series_id",
    "N",
    "K",
    "k",
    "horizon",
    "series_type",
    "series_label",
    "config_hash",
    "prepare_init_graph_projection_shared_seconds",
    "checkpoint_context_prepare_seconds",
    "calibration_estimate_seconds",
    "warmup_estimate_seconds",
    "timed_samples_seconds",
    "known_elapsed_estimate_seconds",
    "warmups",
    "samples",
    "repetitions_per_sample",
    "recalibrations",
    "harness",
    "raw_data",
    "notes",
]
PLAN_FIELDS = [
    "experiment_id",
    "report",
    "section_anchor",
    "classification",
    "cells_remaining",
    "N_values",
    "model_seconds",
    "A5_floor_seconds",
    "estimate_basis",
    "pilot_harness",
    "pilot_raw_data",
    "harness",
    "raw_data",
]
COMPONENT_FIELDS = ["component", "seconds", "basis", "notes", "raw_data"]


def _records(path: Path = RAW) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def _csv_bytes(rows: list[dict[str, Any]], fields: list[str]) -> bytes:
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    writer.writerows({field: row.get(field, "") for field in fields} for row in rows)
    return stream.getvalue().encode("utf-8")


def _series(records: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    host = next(row for row in records if row.get("record") == "SCALE_HOST")
    calibrations = {
        (str(row["series"]), int(row["N"]), int(row["K"]), int(row["k"]), str(row["phase"])): row
        for row in records if row.get("record") == "SCALE_A5_CALIBRATION"
    }
    setup = {
        (int(row["N"]), int(row["k"])): row
        for row in records if row.get("record") == "SCALE_SETUP"
    }
    a5_setup = [row for row in records if row.get("record") == "SCALE_SETUP_A5"]
    full = [row for row in records if row.get("record") == "SCALE_FULL_TICK"]
    phase_records = [row for row in records if row.get("record") == "SCALE_PHASE"]
    phase_series: dict[tuple[int, int, int, int], dict[str, Any]] = {}
    for row in phase_records:
        phase_series[(int(row["N"]), int(row["K"]), int(row["k"]), int(row["horizon"]))] = row

    raw_series: list[tuple[str, dict[str, Any], str, str, int | None]] = []
    for row in a5_setup:
        raw_series.append(("setup", row, str(row["phase"]), str(row["phase"]), None))
    for row in full:
        raw_series.append(("full_tick", row, f"day{row['horizon']}", f"horizon_{row['horizon']}", int(row["horizon"])))
    for key, row in phase_series.items():
        n, kcount, degree, horizon = key
        raw_series.append(("phase_profile", row, f"day{horizon}", "all_phases", horizon))

    output: list[dict[str, Any]] = []
    for kind, row, label, calibration_phase, horizon in raw_series:
        n, kcount, degree = int(row["N"]), int(row["K"]), int(row["k"])
        if kind == "setup":
            calibration_key = ("setup", n, kcount, degree, calibration_phase)
            prep = float(setup[(n, degree)]["initialize_us"]) / 1_000_000.0
            checkpoint_note = "not applicable to setup series"
            # The series reports the selected graph/projection timer. The other construction work
            # in measure_setup is outside that timer and has no per-sample wall timestamps.
        else:
            calibration_key = (kind, n, kcount, degree, calibration_phase)
            config_setup = setup[(n, degree)]
            prep = sum(float(config_setup[key]) for key in ("initialize_us", "graph_us", "projection_us")) / 1_000_000.0
            checkpoint_note = "checkpoint/context preparation was outside the timer and not recorded"
        calibration = calibrations[calibration_key]
        sample_seconds = row["sample_seconds"]
        if isinstance(sample_seconds, str):
            sample_seconds = [float(value) for value in sample_seconds.split(",") if value]
        timed = sum(float(value) for value in sample_seconds)
        mean_sample = timed / len(sample_seconds)
        warmups = int(row.get("warmups", host["warmups"]))
        warmup_estimate = mean_sample * warmups
        calibration_r = int(calibration["r"])
        calibration_estimate = 3.0 * float(calibration["estimated_sample_seconds"]) / calibration_r
        notes = (
            "timed sample seconds are recorded; warmups and three calibration probes are estimated from the recorded timed mean and probe median"
            if kind != "setup"
            else "the selected graph/projection timer is recorded; the other setup operation may run outside that timer"
        )
        output.append({
            "series_id": f"{kind}:N{n}:K{kcount}:k{degree}:{label}",
            "N": n,
            "K": kcount,
            "k": degree,
            "horizon": "" if horizon is None else horizon,
            "series_type": kind,
            "series_label": label,
            "config_hash": "not present in this pre-hardening run",
            "prepare_init_graph_projection_shared_seconds": f"{prep:.6f}",
            "checkpoint_context_prepare_seconds": checkpoint_note,
            "calibration_estimate_seconds": f"{calibration_estimate:.6f}",
            "warmup_estimate_seconds": f"{warmup_estimate:.6f}",
            "timed_samples_seconds": f"{timed:.6f}",
            "known_elapsed_estimate_seconds": f"{timed + warmup_estimate + calibration_estimate:.6f}",
            "warmups": warmups,
            "samples": int(row.get("samples", host["samples"])),
            "repetitions_per_sample": int(row.get("r", row.get("repetitions_per_sample", calibration["r"]))),
            "recalibrations": int(row.get("recalibrations", calibration["recalibrations"])),
            "harness": "crates/sim-model/examples/m3_05_a5_reconstruction.rs; crates/sim-model/benches/m3_scale_limit_bench.rs",
            "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl",
            "notes": notes,
        })
    if len(output) != 48:
        raise ValueError(f"expected 48 recorded A-5 series, found {len(output)}")

    by_n_type: dict[tuple[int, str], list[float]] = defaultdict(list)
    for row in output:
        by_n_type[(int(row["N"]), str(row["series_type"]))].append(float(row["known_elapsed_estimate_seconds"]))
    per_n: list[dict[str, Any]] = []
    for n in sorted({int(row["N"]) for row in output}):
        means = {kind: sum(by_n_type[(n, kind)]) / len(by_n_type[(n, kind)]) for kind in ("setup", "full_tick", "phase_profile")}
        total = [float(row["known_elapsed_estimate_seconds"]) for row in output if int(row["N"]) == n]
        per_n.append({"N": n, "mean_per_series_seconds": sum(total) / len(total), "by_type": means, "series_count": len(total)})
    model = _power_law([(float(row["N"]), float(row["mean_per_series_seconds"])) for row in per_n])
    for row in per_n:
        row["fitted_seconds"] = model(float(row["N"]))
    return output, [{"N": row["N"], **row["by_type"], "mean_per_series_seconds": f"{row['mean_per_series_seconds']:.6f}", "fitted_seconds": f"{row['fitted_seconds']:.6f}", "series_count": row["series_count"]} for row in per_n]


def _power_law(points: list[tuple[float, float]]):
    intercept, slope = _power_law_coefficients(points)
    return lambda n: math.exp(intercept + slope * math.log(n))


def _component_rows(records: list[dict[str, Any]], series_rows: list[dict[str, Any]]) -> list[dict[str, str]]:
    setup = [row for row in records if row.get("record") == "SCALE_SETUP"]
    setup_seconds = sum(
        sum(float(row[field]) for field in ("initialize_us", "graph_us", "projection_us"))
        for row in setup
    ) / 1_000_000.0
    calibration = sum(float(row["calibration_estimate_seconds"]) for row in series_rows)
    warmups = sum(float(row["warmup_estimate_seconds"]) for row in series_rows)
    samples = sum(float(row["timed_samples_seconds"]) for row in series_rows)
    return [
        {"component": "initialization + graph + projection", "seconds": f"{setup_seconds:.6f}", "basis": "measured fields in 8 SCALE_SETUP rows", "notes": "setup component only; checkpoint/context preparation is separate and unrecorded", "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl"},
        {"component": "checkpoint/context preparation", "seconds": "", "basis": "not recorded", "notes": "raw JSONL has no timestamps for this work", "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl"},
        {"component": "calibration probes", "seconds": f"{calibration:.6f}", "basis": "estimated from 3 × recorded probe median per series", "notes": "not an observed wall duration", "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl"},
        {"component": "warmups", "seconds": f"{warmups:.6f}", "basis": "estimated as warmup count × timed sample mean per series", "notes": "not an observed wall duration", "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl"},
        {"component": "timed samples", "seconds": f"{samples:.6f}", "basis": "sum of recorded sample_seconds", "notes": "observed timer durations", "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl"},
        {"component": "known component subtotal", "seconds": f"{setup_seconds + calibration + warmups + samples:.6f}", "basis": "setup + estimated calibration/warmups + recorded timed samples", "notes": "not total wall time; checkpoint/context preparation and other process overhead are excluded", "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl"},
    ]


def _power_law_coefficients(points: list[tuple[float, float]]) -> tuple[float, float]:
    xs = [math.log(x) for x, _ in points]
    ys = [math.log(y) for _, y in points]
    mean_x, mean_y = sum(xs) / len(xs), sum(ys) / len(ys)
    slope = sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys)) / sum((x - mean_x) ** 2 for x in xs)
    intercept = mean_y - slope * mean_x
    return intercept, slope


def _population_values(value: str) -> list[int]:
    found = []
    for match in re.finditer(r"(?<![A-Za-z])([0-9][0-9,]*)([kM]?)\b", value):
        number = int(match.group(1).replace(",", ""))
        number *= 1_000 if match.group(2) == "k" else 1_000_000 if match.group(2) == "M" else 1
        if number >= 1_000:
            found.append(number)
    return sorted(set(found))


def _inventory_outputs(root: Path, per_n_rows: list[dict[str, Any]]) -> tuple[bytes, bytes, bytes, dict[str, Any]]:
    with (root / INVENTORY.relative_to(ROOT)).open(encoding="utf-8-sig", newline="") as stream:
        reader = csv.DictReader(stream)
        fields = list(reader.fieldnames or [])
        inventory = list(reader)
    model = _power_law([(float(row["N"]), float(row["mean_per_series_seconds"])) for row in per_n_rows])
    model_rows: list[dict[str, Any]] = []
    remaining_cells = 0
    total_estimate = 0.0
    fallback_groups = 0
    for row in inventory:
        cells = int(row.get("A5_cells_to_schedule", "0") or 0)
        if row["experiment_id"] == EXPERIMENT_ID:
            cells = 0
            row["A5_cells_to_schedule"] = "0"
            row["A5_lower_bound_seconds"] = "0"
            row["A5_status"] = "recorded-with-validity-inconclusive-paths"
            row["A5_estimate_basis"] = "all 48 cells completed; path-level validity exceptions are recorded in M3_05_PILOT_CLAIMS.csv"
            row["A5_cell_count_reason"] = "completed M3-05 A-5 pilot; 15 profile series have at least one path above the 5% MAD/median validity limit"
        n_values = _population_values(str(row.get("N_values", "")))
        if cells and not n_values:
            n_values = [1_000_000]
            fallback_groups += 1
            basis = "N not parseable from inventory; conservatively model at the largest measured N"
        elif cells:
            basis = "listed N values weighted equally; per-series model extrapolated by a log-log power fit"
        else:
            basis = "no remaining A-5 cells"
        estimated_per_cell = sum(model(float(n)) for n in n_values) / len(n_values) if cells else 0.0
        estimate = cells * estimated_per_cell
        remaining_cells += cells
        total_estimate += estimate
        model_row = {
            "experiment_id": row["experiment_id"],
            "report": row["report"],
            "section_anchor": row["section_anchor"],
            "classification": row["classification"],
            "cells_remaining": cells,
            "N_values": ";".join(map(str, n_values)),
            "model_seconds": f"{estimate:.3f}",
            "A5_floor_seconds": cells * 43,
            "estimate_basis": basis,
            "pilot_harness": "analysis/simciv_analysis/m3_05_pilot_runtime.py; crates/sim-model/examples/m3_05_a5_reconstruction.rs",
            "pilot_raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl",
            "harness": str(row.get("mapped_harnesses", "none")),
            "raw_data": str(row.get("mapped_data_paths", "none")),
        }
        model_rows.append(model_row)
        row["A5_model_estimate_seconds"] = f"{estimate:.3f}"
        row["A5_runtime_estimate_basis"] = basis

    for field in ("A5_model_estimate_seconds", "A5_runtime_estimate_basis"):
        if field not in fields:
            fields.append(field)
    csv_text = io.StringIO(newline="")
    writer = csv.DictWriter(csv_text, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    writer.writerows(row for row in inventory)
    inventory_csv = csv_text.getvalue().encode("utf-8")

    plan_rows = sorted(model_rows, key=lambda row: (-float(row["model_seconds"]), str(row["experiment_id"])))
    plan_csv = _csv_bytes(plan_rows, PLAN_FIELDS)
    summary = {
        "remaining_cells": remaining_cells,
        "remaining_floor_seconds": remaining_cells * 43,
        "remaining_model_seconds": round(total_estimate, 3),
        "unparsed_N_groups_fallback_to_1M": fallback_groups,
        "model": "log-log power fit to M3-05 per-series known elapsed estimate (timed samples + estimated warmups + estimated calibration probes)",
        "excluded": "checkpoint/context preparation and compilation because the existing JSONL did not record wall timestamps for them",
    }
    return inventory_csv, plan_csv, summary, {"rows": plan_rows, "summary": summary}


def _markdown_section(per_n_rows: list[dict[str, Any]], plan: dict[str, Any], pilot_rows: list[dict[str, Any]], component_rows: list[dict[str, str]]) -> str:
    summary = plan["summary"]
    intercept, slope = _power_law_coefficients([
        (float(row["N"]), float(row["mean_per_series_seconds"])) for row in per_n_rows
    ])
    invalid = [row for row in pilot_rows if row.get("verdict") == "inconclusive (validity)"]
    lines = [
        BEGIN,
        "## M3-05 pilot status and remaining runtime estimate",
        "",
        f"Generated by [m3_05_pilot_runtime.py](../../analysis/simciv_analysis/m3_05_pilot_runtime.py) from {REPORT_LINK} and {DATA_LINK}. The raw JSONL contains per-sample `sample_seconds` but no wall-clock timestamps, warmup durations, calibration-batch durations, or checkpoint/context-preparation durations. The timed sample sum is recorded; calibration and warmup components below are estimates; preparation fields are partial where stated.",
        "",
        "### Elapsed components",
        "",
        "| Component | Seconds | Basis |",
        "|---|---:|---|",
    ]
    for row in component_rows:
        lines.append(f"| {row['component']} | {row['seconds'] or 'not recorded'} | {row['basis']} |")
    lines.extend([
        "",
        "### Per-N runtime model",
        "",
        "| N | series | setup series mean (s) | full-tick series mean (s) | phase-profile series mean (s) | observed known elapsed / series (s) | fitted (s/series) |",
        "|---:|---:|---:|---:|---:|---:|---:|",
    ])
    for row in per_n_rows:
        lines.append(
            f"| {row['N']:,} | {row['series_count']} | {row['setup']:.3f} | {row['full_tick']:.3f} | {row['phase_profile']:.3f} | {float(row['mean_per_series_seconds']):.3f} | {float(row['fitted_seconds']):.3f} |"
        )
    lines.extend([
        "",
        f"The original 162-cell plan included the completed 48-cell M3-05 pilot, leaving **{summary['remaining_cells']} cells** for other experiments. Their A-5 floor is **{summary['remaining_floor_seconds']:,} s**; the per-N model estimates **{summary['remaining_model_seconds']:,.1f} s** for timed samples plus estimated calibration and warmups. The model uses the per-series M3-05 aggregate and equal weights across each group's listed N values; groups without parseable N use 1M conservatively ({summary['unparsed_N_groups_fallback_to_1M']} groups). It excludes unrecorded checkpoint/context preparation and compilation, so it is not a wall-time prediction.",
        f"The fitted observed-series model is `T(N) = {math.exp(intercept):.6g} × N^{slope:.4f}` seconds, where T includes the recorded timed sample duration plus estimated warmups and calibration probes.",
        "",
        "### Validity exceptions",
        "",
        f"{len(invalid)} claim result rows are marked `inconclusive (validity)` because one or more source paths exceeded the 5% MAD/median limit; the two P4 claims are supplemental headline claims already present in the same report section.",
        "",
        "| Experiment | Cells remaining | N values | Modeled seconds | A-5 floor seconds |",
        "|---|---:|---|---:|---:|",
    ])
    for row in plan["rows"][:10]:
        if int(row["cells_remaining"]) == 0:
            continue
        lines.append(
            f"| `{row['experiment_id']}` | {row['cells_remaining']} | {row['N_values'] or 'not specified'} | {float(row['model_seconds']):,.1f} | {int(row['A5_floor_seconds']):,} |"
        )
    lines.extend(["", END, ""])
    return "\n".join(lines)


def _pilot_outcome_section(pilot_rows: list[dict[str, Any]], series_rows: list[dict[str, Any]]) -> str:
    verdicts: dict[str, int] = defaultdict(int)
    for row in pilot_rows:
        verdicts[str(row["verdict"])] += 1
    timed_seconds = sum(float(row["timed_samples_seconds"]) for row in series_rows)
    return "\n".join([
        "## M3-05 end-to-end pilot outcome",
        "",
        f"The completed A-5 pilot covers 48 series across 16 cells. Its 100-claim missing-data group records {verdicts['reproduced']} reproduced, {verdicts['reproduced (loose)']} reproduced (loose), {verdicts['not reproduced']} not reproduced, and {verdicts['inconclusive (validity)'] - 2} inconclusive (validity) claims. Two additional recovered P4 headline claims have pilot status `inconclusive (validity)`. See the [generated claim results](M3_05_PILOT_CLAIMS.csv) and the [pilot raw output instructions](../data/m5_05/m3_05_pilot/README.md).",
        "",
        f"Recorded sample-timer durations sum to {timed_seconds:,.3f} s. The JSONL has no wall-clock timestamps or elapsed time for calibration and warmups; see the runtime breakdown below for the estimates and unrecorded preparation components.",
        "",
        "The frozen report's first-break claims are classified from the completed pilot; invalid paths remain inconclusive and are not silently re-tolerated.",
        "",
    ])


def _existing_with_generated_section(old: str, section: str) -> str:
    if BEGIN in old and END in old:
        start = old.index(BEGIN)
        finish = old.index(END, start) + len(END)
        return old[:start].rstrip() + "\n\n" + section.rstrip() + "\n"
    return old.rstrip() + "\n\n" + section


def build_outputs(root: Path = ROOT) -> dict[Path, bytes]:
    records = _records(root / RAW.relative_to(ROOT))
    if not any(row.get("record") == "SCALE_COMPLETE" for row in records):
        raise ValueError("pilot runtime output lacks SCALE_COMPLETE")
    series_rows, per_n_rows = _series(records)
    component_rows = _component_rows(records, series_rows)
    inventory_csv, plan_csv, summary, plan = _inventory_outputs(root, per_n_rows)
    pilot_results = json.loads((root / "docs" / "reproducibility" / "M3_05_PILOT_RESULTS.json").read_text(encoding="utf-8"))
    report_path = root / INVENTORY_REPORT.relative_to(ROOT)
    report = report_path.read_text(encoding="utf-8")
    report = report.replace(
        "**Status:** generated planning inventory. No simulation or timing harness was run.",
        "**Status:** generated inventory updated after the M3-05 pilot; no other reconstruction experiment was run.",
    )
    report = report.replace(
        "This inventory covers the 484 M2/M3 headline/key claims that were still `missing-data` or `missing-source` in the recovered-mapped ledger.",
        "At initial generation this inventory covered 484 M2/M3 headline/key claims that were still `missing-data` or `missing-source`; the M3-05 pilot outcomes below update that group's current claim statuses.",
    )
    report = report.replace("## A-5 estimate by class", "## Original pre-pilot A-5 estimate by class (historical)")
    report = report.replace("## Ten largest groups by A-5 runtime", "## Ten largest groups in the original pre-pilot plan")
    report = report.replace(
        "Proposed acceptance criteria are in [REPRODUCTION_CRITERIA.md](REPRODUCTION_CRITERIA.md) and remain draft pending approval; no timing is authorized under the draft. They require exact deterministic values and preserve dimensionless timing ratios/rankings; absolute elapsed times are recorded but are not pass criteria.",
        "The frozen acceptance criteria are in [REPRODUCTION_CRITERIA.md](REPRODUCTION_CRITERIA.md). The M3-05 pilot applies exact checks to metadata and the frozen ratio bands to valid timing paths; invalid paths remain inconclusive. No additional experiment was run for this inventory update.",
    )
    pilot_start = report.find("## M3-05 end-to-end pilot proposal")
    if pilot_start < 0:
        pilot_start = report.find("## M3-05 end-to-end pilot outcome")
    criteria_start = report.find("## Criteria and status", pilot_start)
    if pilot_start < 0 or criteria_start < 0:
        raise ValueError("inventory pilot proposal section markers were not found")
    report = report[:pilot_start] + _pilot_outcome_section(pilot_results["claims"], series_rows) + report[criteria_start:]
    section = _markdown_section(per_n_rows, plan, pilot_results["claims"], component_rows)
    return {
        root / SERIES_CSV.relative_to(ROOT): _csv_bytes(series_rows, SERIES_FIELDS),
        root / PLAN_CSV.relative_to(ROOT): plan_csv,
        root / COMPONENTS_CSV.relative_to(ROOT): _csv_bytes(component_rows, COMPONENT_FIELDS),
        root / INVENTORY.relative_to(ROOT): inventory_csv,
        report_path: _existing_with_generated_section(report, section).encode("utf-8"),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    try:
        outputs = build_outputs()
    except (OSError, KeyError, ValueError, json.JSONDecodeError) as error:
        print(f"M3-05 runtime model failed: {error}", file=sys.stderr)
        return 1
    if args.check:
        failures = [str(path.relative_to(ROOT)) for path, content in outputs.items() if not content_matches(path, content)]
        if failures:
            print(f"M3-05 runtime outputs differ: {', '.join(failures)}", file=sys.stderr)
            return 1
        print("M3-05 runtime tables and inventory match the recorded pilot data")
        return 0
    for path, content in outputs.items():
        write_text_lf(path, content.decode("utf-8"))
    print(f"generated M3-05 runtime breakdown, remaining plan, and inventory update: {len(outputs)} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
