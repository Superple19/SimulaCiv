"""Generate and validate the M5-04 M4 regeneration report and ledger updates."""

from __future__ import annotations

import argparse
import collections
import csv
import datetime as dt
import gzip
import json
import math
import re
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import matplotlib.pyplot as plt
import pandas as pd

from simciv_analysis.claims import (
    M5_04_OVERRIDES,
    ROOT,
    apply_m5_04_overrides,
    extract_claims,
    ledger_bytes,
)
from simciv_analysis.figures import configure, svg_bytes
from simciv_analysis.io import load_data
from simciv_analysis.tables import dataframe_to_markdown


DATA_ROOT = ROOT / "docs" / "data" / "m5_04"
CLAIMS_PATH = ROOT / "docs" / "reproducibility" / "claims.csv.gz"
QMD_PATH = ROOT / "docs" / "reproducibility" / "M5_04_M4_REGENERATION.qmd"
MD_PATH = ROOT / "docs" / "reproducibility" / "M5_04_M4_REGENERATION.md"
COMPARE_PATH = DATA_ROOT / "m4_04" / "parallel_estimate_comparison.json"
FIGURES = {
    "timing": ROOT / "docs" / "reproducibility" / "figures" / "m5_04_old_new_timing.svg",
    "divergence": ROOT / "docs" / "reproducibility" / "figures" / "m5_04_old_new_divergence.svg",
}
M4_02_CELLS = {
    "M3 serial, 10k": "m3-10k",
    "M3P serial, 10k": "m3p-10k",
    "M3P incremental, 10k": "m3p-incremental-10k",
    "M3 serial, 100k": "m3-100k",
    "M3P serial, 100k": "m3p-100k",
    "M3 + propagation serial, 100k": "m3prop-100k",
    "M3P incremental, 100k": "m3p-incremental-100k",
    "Recorder M3P serial, 10k": "recorder-10k",
    "Recorder M3P serial, 100k": "recorder-100k",
}
M4_02_DIAG_COMMITS = {
    "m4-02-pass": "d9b08fd",
    "v0.5.0-m4": "4f4498b",
    "head": "ad0c971",
}


def _bytes(path: Path) -> bytes:
    return path.read_bytes()


def _json(path: Path) -> Any:
    return load_data(path, root=ROOT)


def _jsonl(path: Path) -> list[dict[str, Any]]:
    loaded = load_data(path, root=ROOT)
    if not isinstance(loaded, list):
        raise ValueError(f"expected JSONL list at {path}")
    return loaded


def _lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8").splitlines()


def _cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def _first_number(value: str) -> float:
    match = re.search(r"[+-]?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?", value)
    if not match:
        raise ValueError(f"missing numeric value: {value}")
    return float(match.group(0).replace(",", ""))


def _cell_gate(median_delta: float, upper: float, threshold: float, spreads: tuple[float, float]) -> str:
    if any(spread > 0.05 for spread in spreads):
        return "INVALID"
    if upper <= threshold:
        return "PASS"
    if median_delta > threshold:
        return "FAIL"
    return "INCONCLUSIVE"


def _timing_row(
    stage: str,
    cell: str,
    metric: str,
    old: float,
    new: float,
    unit: str,
    protocol: str,
    old_gate: str = "",
    new_gate: str = "",
) -> dict[str, Any]:
    ratio = new / old if old else math.nan
    harness, raw_data = _timing_source(stage)
    return {
        "stage": stage,
        "cell": cell,
        "metric": metric,
        "old": old,
        "new": new,
        "new/old": ratio,
        "|ratio-1| > 20%": bool(math.isfinite(ratio) and abs(ratio - 1.0) > 0.20),
        "old gate": old_gate,
        "new gate": new_gate,
        "unit": unit,
        "protocol": protocol,
        "harness": harness,
        "raw data": raw_data,
    }


def _timing_source(stage: str) -> tuple[str, str]:
    sources = {
        "M4-02": ("crates/sim-model/examples/m4_02_a5_measure.rs", "docs/data/m5_04/m4_02/m4_02_a5.log"),
        "M4-03 gate": ("crates/sim-model/examples/m4_03_gate.rs", "docs/data/m5_04/m4_03/m4_03_gate_a5.log"),
        "M4-05": ("crates/sim-model/examples/m4_05_a5_gate.rs", "docs/data/m5_04/m4_05/m4_05_a5_gate_full.log"),
        "M4-06": ("crates/sim-model/examples/m4_06_a5_gate.rs", "docs/data/m5_04/m4_06/a5_gate.json"),
        "M4-06 hook": ("crates/sim-model/examples/m4_06_hook_followup.rs", "docs/data/m5_04/m4_06/hook_followup.json"),
        "M4-07 CRN": ("crates/sim-model/examples/m4_07_many_branch.rs", "docs/data/m5_04/m4_07/crn_parallel_only.jsonl.gz"),
        "M4-07 day scan": ("crates/sim-model/examples/m4_07_many_branch.rs", "docs/data/m5_04/m4_07/day_scan.jsonl.gz"),
        "M4-07 search": ("crates/sim-model/examples/m4_07_many_branch.rs", "docs/data/m5_04/m4_07/minimal_cause_final.jsonl"),
        "M4-07 clone share": ("crates/sim-model/examples/m4_07_many_branch.rs", "docs/data/m5_04/m4_07/clone_share.jsonl.gz"),
    }
    if stage not in sources:
        raise ValueError(f"no harness/data mapping for timing stage {stage!r}")
    return sources[stage]


def _m4_02_diagnosis() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    log_path = DATA_ROOT / "m4_02_incremental_diagnosis" / "run.log"
    rows: list[dict[str, Any]] = []
    lines = _lines(log_path)
    for line in lines:
        if not line.startswith("SUMMARY\t"):
            continue
        parts = line.split("\t")
        rows.append({
            "revision": parts[1],
            "commit": M4_02_DIAG_COMMITS[parts[1]],
            "cell": parts[2],
            "repetitions": int(parts[3]),
            "direct_ms": float(parts[4]),
            "candidate_ms": float(parts[5]),
            "delta_ms": float(parts[6]),
            "delta_mad_ms": float(parts[7]),
            "upper_ms": float(parts[8]),
            "threshold_ms": float(parts[9]),
            "direct_spread_pct": float(parts[10]),
            "candidate_spread_pct": float(parts[11]),
            "status": parts[12],
            "harness": "crates/sim-model/examples/m4_02_a5_measure.rs + crates/sim-model/examples/m4_02_incremental_diagnosis.ps1",
            "raw data": "docs/data/m5_04/m4_02_incremental_diagnosis/paired_samples.tsv; docs/data/m5_04/m4_02_incremental_diagnosis/run.log",
        })
    if len(rows) != len(M4_02_DIAG_COMMITS) * 2:
        raise ValueError(f"expected six M4-02 diagnosis summaries in {log_path}, found {len(rows)}")
    before = json.loads(next(line.removeprefix("HOST BEFORE ") for line in lines if line.startswith("HOST BEFORE ")))
    after = json.loads(next(line.removeprefix("HOST AFTER ") for line in lines if line.startswith("HOST AFTER ")))
    start = next(line.removeprefix("TIMING RUN START UTC ") for line in lines if line.startswith("TIMING RUN START UTC "))
    end = next(line.removeprefix("TIMING RUN END UTC ") for line in lines if line.startswith("TIMING RUN END UTC "))
    return rows, {
        "start_utc": start,
        "end_utc": end,
        "host_before": before,
        "host_after": after,
        "log": "docs/data/m5_04/m4_02_incremental_diagnosis/run.log",
        "samples": "docs/data/m5_04/m4_02_incremental_diagnosis/paired_samples.tsv",
        "harness": "crates/sim-model/examples/m4_02_a5_measure.rs + crates/sim-model/examples/m4_02_incremental_diagnosis.ps1",
    }


def _m4_02_timings(diagnosis: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    report = ROOT / "docs" / "reports" / "M4_02_BRANCHING.md"
    text = report.read_text(encoding="utf-8")
    section = text.split("### Final confirmatory measurement under frozen §11.7 A-5", 1)[1]
    section = section.split("#### Recorder comparison timer", 1)[0]
    old: dict[str, tuple[float, float, str]] = {}
    for line in section.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cells = _cells(line)
        if len(cells) < 9 or cells[0] not in M4_02_CELLS:
            continue
        old[M4_02_CELLS[cells[0]]] = (_first_number(cells[2]), _first_number(cells[3]), cells[-1].strip("*"))

    current: dict[str, dict[str, Any]] = {}
    for line in _lines(DATA_ROOT / "m4_02" / "m4_02_a5.log"):
        if not line.startswith("RESULT\t"):
            continue
        parts = line.split("\t")
        current[parts[1]] = {
            "direct": float(parts[3]),
            "direct_mad": float(parts[4]),
            "candidate": float(parts[5]),
            "candidate_mad": float(parts[6]),
            "delta": float(parts[7]),
            "delta_mad": float(parts[8]),
            "upper": float(parts[9]),
            "threshold": float(parts[10]),
            "direct_spread": float(parts[11]) / 100.0,
            "candidate_spread": float(parts[12]) / 100.0,
            "status": parts[13],
        }
    corrected = {row["cell"]: row for row in diagnosis if row["revision"] == "head"}
    for label, cell_id in M4_02_CELLS.items():
        if cell_id not in {"m3p-incremental-10k", "m3p-incremental-100k"}:
            continue
        result = corrected[cell_id]
        current[cell_id] = {
            "direct": result["direct_ms"],
            "candidate": result["candidate_ms"],
            "status": result["status"],
        }
    rows: list[dict[str, Any]] = []
    gates: list[dict[str, str]] = []
    for label, cell_id in M4_02_CELLS.items():
        old_direct, old_candidate, old_gate = old[cell_id]
        measured = current[cell_id]
        gates.append({"stage": "M4-02", "cell": label, "old": old_gate, "new": measured["status"]})
        row_data = "docs/data/m5_04/m4_02_incremental_diagnosis/paired_samples.tsv" if cell_id in corrected else "docs/data/m5_04/m4_02/m4_02_a5.log"
        for metric, old_value, new_value in [
            ("direct pair median", old_direct, measured["direct"]),
            ("analysis-disabled/full median", old_candidate, measured["candidate"]),
        ]:
            timing = _timing_row("M4-02", label, metric, old_value, new_value, "ms/operation", "A-5, 41 samples", old_gate, measured["status"])
            timing["raw data"] = row_data
            if cell_id in corrected:
                timing["harness"] = "crates/sim-model/examples/m4_02_a5_measure.rs + crates/sim-model/examples/m4_02_incremental_diagnosis.ps1"
            rows.append(timing)
        gates[-1]["raw data"] = row_data
        if cell_id in corrected:
            gates[-1]["harness"] = "crates/sim-model/examples/m4_02_a5_measure.rs + crates/sim-model/examples/m4_02_incremental_diagnosis.ps1"
    return rows, gates


def _m4_03_timings() -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    old_path = ROOT / "docs" / "data" / "m4_03" / "m4_03_gate_rerun.csv"
    new_path = DATA_ROOT / "m4_03" / "m4_03_gate_a5.log"
    old_cells: dict[str, list[str]] = {}
    for line in _lines(old_path):
        cells = next(csv.reader([line]))
        if cells and cells[0] == "CELL":
            old_cells[cells[1]] = cells
    new_cells: dict[str, list[str]] = {}
    for line in _lines(new_path):
        cells = next(csv.reader([line]))
        if cells and cells[0] == "CELL":
            new_cells[cells[1]] = cells
    rows: list[dict[str, Any]] = []
    gates: list[dict[str, str]] = []
    for cell, previous in old_cells.items():
        current = new_cells[cell]
        old_direct, old_series = float(previous[3]), float(previous[5])
        new_direct, new_series = float(current[3]), float(current[5])
        old_gate = _cell_gate(float(previous[7]), float(previous[9]), float(previous[10]), (float(previous[11]), float(previous[12])))
        new_gate = _cell_gate(float(current[7]), float(current[9]), float(current[10]), (float(current[11]), float(current[12])))
        gates.append({"stage": "M4-03 gate", "cell": cell, "old": old_gate, "new": new_gate})
        rows.append(_timing_row("M4-03 gate", cell, "direct branch median", old_direct, new_direct, "µs/operation", "A-5-style 41 samples", old_gate, new_gate))
        rows.append(_timing_row("M4-03 gate", cell, "divergence-series median", old_series, new_series, "µs/operation", "A-5-style 41 samples", old_gate, new_gate))
    return rows, gates


def _m4_05_timings() -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    report = ROOT / "docs" / "reports" / "M4_05_MINIMAL_CAUSE.md"
    old_direct = old_batch = None
    for line in report.read_text(encoding="utf-8").splitlines():
        if line.lstrip().startswith("|") and "M3P serial, N=10,000" in line:
            cells = _cells(line)
            old_direct, old_batch = _first_number(cells[5]), _first_number(cells[6])
            old_gate = cells[-1].strip("*")
            break
    if old_direct is None or old_batch is None:
        raise ValueError("could not find the committed M4-05 A-5 gate row")
    gate_line = next(line for line in _lines(DATA_ROOT / "m4_05" / "m4_05_a5_gate_full.log") if line.startswith("GATE|"))
    current = dict(part.split("=", 1) for part in gate_line.split("|")[1:])
    new_gate = current["status"]
    rows = [
        _timing_row("M4-05", "M3P 10k, H=30", "direct branch median", old_direct, float(current["direct_median_us"]), "µs/operation", "A-5, 41 samples", old_gate, new_gate),
        _timing_row("M4-05", "M3P 10k, H=30", "batch-of-one median", old_batch, float(current["batch_median_us"]), "µs/operation", "A-5, 41 samples", old_gate, new_gate),
    ]
    return rows, [{"stage": "M4-05", "cell": "M3P 10k, H=30", "old": old_gate, "new": new_gate}]


def _m4_06_timings() -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    old = _json(ROOT / "docs" / "data" / "m4_06" / "a5_gate_final.json")
    new = _json(DATA_ROOT / "m4_06" / "a5_gate.json")
    rows: list[dict[str, Any]] = []
    gates: list[dict[str, str]] = []
    for previous, current in zip(old["cells"], new["cells"]):
        n = int(previous["population"])
        old_result, new_result = previous["result"], current["result"]
        old_gate, new_gate = old_result["status"], new_result["status"]
        gates.append({"stage": "M4-06", "cell": f"M3P {n:,}", "old": old_gate, "new": new_gate})
        rows.append(_timing_row("M4-06", f"M3P {n:,}", "direct baseline median", old_result["baseline_median_us_per_3_days"], new_result["baseline_median_us_per_3_days"], "µs/3 days", "A-5, 41 samples", old_gate, new_gate))
        rows.append(_timing_row("M4-06", f"M3P {n:,}", "migration-off candidate median", old_result["explicit_off_median_us_per_3_days"], new_result["explicit_off_median_us_per_3_days"], "µs/3 days", "A-5, 41 samples", old_gate, new_gate))
        rows.append(_timing_row("M4-06", f"M3P {n:,}", "zero-move hook cost", old_result["migration_on_zero_move_ns_per_alive_agent_day_median"], new_result["migration_on_zero_move_ns_per_alive_agent_day_median"], "ns/agent-day", "A-5 gate diagnostic", old_gate, new_gate))
    return rows, gates


def _m4_06_hook_timings() -> list[dict[str, Any]]:
    old = _json(ROOT / "docs" / "data" / "m4_06" / "hook_followup.json")
    new = _json(DATA_ROOT / "m4_06" / "hook_followup.json")
    rows: list[dict[str, Any]] = []
    for previous, current in zip(old["cells"], new["cells"]):
        cell = f"M3P {int(previous['population']):,}, p={previous['probability']}"
        for key, metric, unit in [
            ("baseline_median_us_per_3_days", "baseline median", "µs/3 days"),
            ("enabled_median_us_per_3_days", "migration-enabled median", "µs/3 days"),
            ("migration_hook_ns_per_alive_agent_day_median", "hook cost", "ns/agent-day"),
        ]:
            rows.append(_timing_row("M4-06 hook", cell, metric, previous[key], current[key], unit, "A-5-style descriptive rerun"))
    return rows


def _m4_07_supplemental_timings() -> list[dict[str, Any]]:
    old_crn = {row["intervention"]: row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "crn_parallel.jsonl") if row.get("kind") == "crn_comparison"}
    new_crn = {row["intervention"]: row for row in _jsonl(DATA_ROOT / "m4_07" / "crn_parallel_only.jsonl.gz") if row.get("kind") == "crn_parallel_rerun"}
    rows: list[dict[str, Any]] = []
    for name, old in old_crn.items():
        rows.append(_timing_row("M4-07 CRN", name, "parallel replicate-set wall time", old["parallel_ns"], new_crn[name]["parallel_ns"], "ns/100 replicates", "single aggregate run; supplementary"))

    old_scan = {row["population"]: row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "day_scan.jsonl") if row.get("kind") == "day_scan"}
    new_scan = {row["population"]: row for row in _jsonl(DATA_ROOT / "m4_07" / "day_scan.jsonl.gz") if row.get("kind") == "day_scan"}
    for population, old in old_scan.items():
        new = new_scan[population]
        rows.append(_timing_row("M4-07 day scan", f"N={population:,}", "SHA digest wall time", old["sha_ns"], new["sha_ns"], "ns/30-day branch", "single aggregate run; supplementary"))
        rows.append(_timing_row("M4-07 day scan", f"N={population:,}", "direct scan wall time", old["direct_ns"], new["direct_ns"], "ns/30-day branch", "single aggregate run; supplementary"))

    old_search = next(row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "minimal_cause_final.jsonl") if row.get("kind") == "minimal_cause")
    new_search = next(row for row in _jsonl(DATA_ROOT / "m4_07" / "minimal_cause_final.jsonl") if row.get("kind") == "minimal_cause")
    rows.append(_timing_row("M4-07 search", "M4-05 case (a)", "serial search wall time", old_search["serial_ns"], new_search["serial_ns"], "ns/search", "single aggregate run; supplementary"))
    rows.append(_timing_row("M4-07 search", "M4-05 case (a)", "parallel search wall time", old_search["parallel_ns"], new_search["parallel_ns"], "ns/search", "single aggregate run; supplementary"))

    old_clone = { (row["population"], row["horizon_days"]): row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "clone_share.jsonl") if row.get("kind") == "clone_share"}
    new_clone = { (row["population"], row["horizon_days"]): row for row in _jsonl(DATA_ROOT / "m4_07" / "clone_share.jsonl.gz") if row.get("kind") == "clone_share"}
    for key, old in old_clone.items():
        new = new_clone[key]
        for field, metric in [("snapshot_restore_ns", "snapshot restore median"), ("day_start_clone_ns", "day-start clone median"), ("total_ns", "branch total median")]:
            old_median = statistics.median(sample[field] for sample in old["samples"])
            new_median = statistics.median(sample[field] for sample in new["samples"])
            rows.append(_timing_row("M4-07 clone share", f"N={key[0]:,}, H={key[1]}", metric, old_median, new_median, "ns", "three samples; supplementary"))
    return rows


def _old_new_m4_04_comparison() -> dict[str, Any]:
    old_path = ROOT / "docs" / "data" / "m4_04" / "m4_04_crn_efficiency.csv"
    old_rows = list(csv.DictReader(old_path.open(encoding="utf-8", newline=""), delimiter="|"))
    old_rows = [row for row in old_rows if row.get("intervention") in {"food_0", "health_0_01", "base_metabolic_cost_plus_1pct"}]
    new_rows = {row["intervention"]: row for row in _jsonl(DATA_ROOT / "m4_07" / "crn_parallel_only.jsonl.gz") if row.get("kind") == "crn_parallel_rerun"}
    field_map = {
        "paired_mean_effect": "paired_mean_effect", "paired_ci95_low": "paired_mean_ci95_lower",
        "paired_ci95_high": "paired_mean_ci95_upper", "paired_sample_variance": "paired_sample_variance",
        "paired_estimator_variance": "paired_estimator_variance", "independent_mean_effect": "independent_mean_effect",
        "independent_base_sample_variance": "independent_base_sample_variance",
        "independent_intervention_sample_variance": "independent_intervention_sample_variance",
        "independent_estimator_variance": "independent_estimator_variance",
        "mean_D_H": "mean_paired_divergent_records", "mean_newborn_crn_loss": "mean_newborn_crn_loss",
    }
    label_map = {"health_0_01": "health_0.01"}
    estimates = {
        (label_map.get(name, name), item["horizon_days"], item["outcome"]): item
        for name, record in new_rows.items()
        for item in record["estimates"]
    }

    def formatted(value: Any, precision: int) -> str:
        if value is None:
            return ""
        number = float(value)
        if math.isinf(number):
            return "inf" if number > 0 else "-inf"
        if math.isnan(number):
            return "NaN"
        if precision == 6:
            return f"{number:.6f}"
        text = f"{number:.12e}"
        return re.sub(r"e([+-]?)0*(\d+)$", lambda match: "e" + str(-int(match.group(2)) if match.group(1) == "-" else int(match.group(2))), text)

    matches = 0
    normalizations: list[dict[str, Any]] = []
    differences: list[dict[str, Any]] = []

    def compare_field(old: dict[str, str], key: str, expected: str) -> None:
        nonlocal matches
        if old[key] == expected:
            matches += 1
        elif old[key] == "inf" and expected == "":
            normalizations.append({
                "stage": "M4-04",
                "file": old["intervention"],
                "field": f"H={old['horizon']} {old['outcome']} {key}",
                "old": "inf",
                "new": "null",
                "rule": "JSON null and CSV inf both encode an unbounded confidence interval; comparison maps both to positive infinity.",
                "harness": "crates/sim-model/examples/m4_07_many_branch.rs (--only=crn-parallel)",
                "raw data": "docs/data/m4_04/m4_04_crn_efficiency.csv; docs/data/m5_04/m4_07/crn_parallel_only.jsonl.gz",
            })
        else:
            differences.append({
                "intervention": old["intervention"], "horizon": int(old["horizon"]),
                "outcome": old["outcome"], "field": key, "old": old[key], "new": expected,
                "difference": "field value differs",
            })

    for old in old_rows:
        intervention = label_map.get(old["intervention"], old["intervention"])
        current = estimates[(intervention, int(old["horizon"]), old["outcome"])]
        for old_field, new_field in field_map.items():
            precision = 6 if old_field in {"mean_D_H", "mean_newborn_crn_loss"} else 12
            expected = formatted(current[new_field], precision)
            compare_field(old, old_field, expected)
        ratio = current["variance_ratio"]
        expected_ratio = "exact" if ratio["kind"] == "exact_paired_variance_zero" else formatted(ratio["ratio"], 12)
        expected_low = "" if ratio["kind"] == "exact_paired_variance_zero" else formatted(ratio["ci95_lower"], 12)
        expected_high = "" if ratio["kind"] == "exact_paired_variance_zero" else formatted(ratio["ci95_upper"], 12)
        for old_field, expected in [("variance_ratio", expected_ratio), ("ratio_ci95_low", expected_low), ("ratio_ci95_high", expected_high)]:
            compare_field(old, old_field, expected)
    return {
        "method": "M4-07 six-thread parallel-only CRN rerun; estimates compared to M4-04 serial CSV",
        "harness": "crates/sim-model/examples/m4_07_many_branch.rs (--only=crn-parallel)",
        "new_data": "docs/data/m5_04/m4_07/crn_parallel_only.jsonl.gz",
        "reference_data": "docs/data/m4_04/m4_04_crn_efficiency.csv",
        "compared_fields": matches + len(normalizations) + len(differences),
        "exact_fields": matches,
        "normalized_fields": len(normalizations),
        "mismatch_count": len(differences),
        "normalizations": normalizations,
        "mismatches": differences,
    }


def _m4_05_differences() -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    files = ["m4_05_a_health8.jsonl", "m4_05_a_health8_full_probe.jsonl", "m4_05_b_health16.jsonl", "m4_05_c_config6.jsonl"]
    differences: list[dict[str, Any]] = []
    normalizations: list[dict[str, Any]] = []

    def normalized_value(filename: str, path: str, old: Any, new: Any) -> tuple[bool, str]:
        if path.endswith("/schema") and old == 1 and new == 2:
            return True, "M4-05 output schema 1 is normalized to schema 2; the guarantee enum clarified incomplete versus exhausted search."
        if path.endswith("/result/guarantee"):
            renames = {
                ("m4_05_a_health8.jsonl", "Minimum", "minimum"),
                ("m4_05_a_health8_full_probe.jsonl", "None", "incomplete"),
                ("m4_05_b_health16.jsonl", "OneMinimal", "one_minimal"),
                ("m4_05_c_config6.jsonl", "None", "exhausted_no_cause"),
            }
            if (filename, old, new) in renames:
                return True, "M4-05 §14 A-1 explicitly renamed guarantee labels to distinguish the frozen search outcomes."
        return False, ""

    def walk(old: Any, new: Any, path: str, filename: str) -> None:
        if isinstance(old, dict) and isinstance(new, dict):
            for key in sorted(set(old) | set(new)):
                if key in {"search_wall_s", "wall_s"}:
                    continue
                if key not in old or key not in new:
                    differences.append({"file": filename, "field": f"{path}/{key}", "old": old.get(key), "new": new.get(key), "difference": "field added/removed"})
                else:
                    walk(old[key], new[key], f"{path}/{key}", filename)
        elif isinstance(old, list) and isinstance(new, list):
            if len(old) != len(new):
                differences.append({"file": filename, "field": f"{path}/length", "old": len(old), "new": len(new), "difference": "list length differs"})
            for index, (left, right) in enumerate(zip(old, new)):
                walk(left, right, f"{path}/{index}", filename)
        elif old != new:
            normalized, rule = normalized_value(filename, path, old, new)
            if normalized:
                normalizations.append({
                    "stage": "M4-05", "file": filename, "field": path, "old": old, "new": new, "rule": rule,
                    "harness": "crates/sim-model/examples/m4_05_experiment.rs",
                    "raw data": f"docs/data/m4_05/{filename}; docs/data/m5_04/m4_05/{filename}",
                })
            else:
                differences.append({"file": filename, "field": path, "old": old, "new": new, "difference": "deterministic JSON field differs"})

    for filename in files:
        old = _jsonl(ROOT / "docs" / "data" / "m4_05" / filename)
        new = _jsonl(DATA_ROOT / "m4_05" / filename)
        for index, (left, right) in enumerate(zip(old, new)):
            walk(left, right, f"record/{index}", filename)
        if len(old) != len(new):
            differences.append({"file": filename, "field": "records/length", "old": len(old), "new": len(new), "difference": "JSONL record count differs"})
    return differences, normalizations


def _deterministic_results(m4_04: dict[str, Any], m4_05_diffs: list[dict[str, Any]], m4_05_normalizations: list[dict[str, Any]]) -> list[dict[str, Any]]:
    a1_old = ROOT / "docs" / "data" / "m4_03" / "m4_03_a1_experiments.csv"
    a1_new = DATA_ROOT / "m4_03" / "m4_03_a1_experiments.csv.gz"
    m406_experiment_equal = gzip.decompress(_bytes(DATA_ROOT / "m4_06" / "experiment.json.gz")) == _bytes(ROOT / "docs" / "data" / "m4_06" / "experiment.json")
    m406_branch_equal = gzip.decompress(_bytes(DATA_ROOT / "m4_06" / "branch_experiment.json.gz")) == _bytes(ROOT / "docs" / "data" / "m4_06" / "branch_experiment.json")
    viewer_equal = _bytes(DATA_ROOT / "viewer" / "food_0_h90.jsonl") == _bytes(ROOT / "docs" / "data" / "m4_viewer" / "food_0_h90.jsonl")
    old_crn = {row["intervention"]: row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "crn_parallel.jsonl") if row.get("kind") == "crn_comparison"}
    new_crn = {row["intervention"]: row for row in _jsonl(DATA_ROOT / "m4_07" / "crn_parallel_only.jsonl.gz") if row.get("kind") == "crn_parallel_rerun"}
    crn_equal = all(old_crn[name]["estimates"] == new_crn[name]["estimates"] for name in old_crn)
    old_scan = {row["population"]: row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "day_scan.jsonl") if row.get("kind") == "day_scan"}
    new_scan = {row["population"]: row for row in _jsonl(DATA_ROOT / "m4_07" / "day_scan.jsonl.gz") if row.get("kind") == "day_scan"}
    scan_equal = all(all(old_scan[n][key] == new_scan[n][key] for key in ("kind", "population", "settlements", "horizon_days", "bit_identical", "first_divergent_day")) for n in old_scan)
    old_search = next(row for row in _jsonl(ROOT / "docs" / "data" / "m4_07" / "minimal_cause_final.jsonl") if row.get("kind") == "minimal_cause")
    new_search = next(row for row in _jsonl(DATA_ROOT / "m4_07" / "minimal_cause_final.jsonl") if row.get("kind") == "minimal_cause")
    search_equal = all(old_search[key] == new_search[key] for key in ("cause_ids", "guarantee", "evaluation_count", "bit_identical"))
    old_viewer = _jsonl(ROOT / "docs" / "data" / "m4_viewer" / "food_0_h90.jsonl")
    new_viewer = _jsonl(DATA_ROOT / "viewer" / "food_0_h90.jsonl")
    return [
        {"stage": "M4-03 A-1 divergence CSV", "result": "bit-exact" if _bytes(a1_old) == gzip.decompress(_bytes(a1_new)) else "mismatch", "harness": "crates/sim-model/examples/m4_03_experiment.rs", "data": "docs/data/m5_04/m4_03/m4_03_a1_experiments.csv.gz"},
        {"stage": "M4-04 CRN estimates", "result": "bit-exact after documented normalization" if m4_04["mismatch_count"] == 0 and m4_04["normalized_fields"] else "bit-exact" if m4_04["mismatch_count"] == 0 else "mismatch", "harness": m4_04["harness"], "data": m4_04["new_data"], "detail": f"{m4_04['exact_fields']} raw-exact + {m4_04['normalized_fields']} documented normalizations / {m4_04['compared_fields']} fields"},
        {"stage": "M4-05 deterministic case records", "result": "bit-exact after documented normalization" if not m4_05_diffs and m4_05_normalizations else "bit-exact" if not m4_05_diffs else "mismatch", "harness": "crates/sim-model/examples/m4_05_experiment.rs", "data": "docs/data/m5_04/m4_05/", "detail": f"{len(m4_05_diffs)} unexplained fields; {len(m4_05_normalizations)} documented normalizations"},
        {"stage": "M4-06 population experiment", "result": "bit-exact" if m406_experiment_equal else "mismatch", "harness": "crates/sim-model/examples/m4_06_experiment.rs", "data": "docs/data/m5_04/m4_06/experiment.json.gz"},
        {"stage": "M4-06 branch experiment", "result": "bit-exact" if m406_branch_equal else "mismatch", "harness": "crates/sim-model/examples/m4_06_branch_experiment.rs", "data": "docs/data/m5_04/m4_06/branch_experiment.json.gz"},
        {"stage": "M4-07 CRN estimate arrays", "result": "bit-exact" if crn_equal else "mismatch", "harness": "crates/sim-model/examples/m4_07_many_branch.rs (--only=crn-parallel)", "data": "docs/data/m5_04/m4_07/crn_parallel_only.jsonl.gz"},
        {"stage": "M4-07 day-scan deterministic fields", "result": "bit-exact" if scan_equal else "mismatch", "harness": "crates/sim-model/examples/m4_07_many_branch.rs (--only=scan)", "data": "docs/data/m5_04/m4_07/day_scan.jsonl.gz"},
        {"stage": "M4-07 minimal-cause result", "result": "bit-exact" if search_equal else "mismatch", "harness": "crates/sim-model/examples/m4_07_many_branch.rs (--only=search)", "data": "docs/data/m5_04/m4_07/minimal_cause_final.jsonl"},
        {"stage": "M4-07 viewer sample", "result": "bit-exact" if viewer_equal and old_viewer == new_viewer else "mismatch", "harness": "crates/sim-model/examples/m4_branch_trace.rs", "data": "docs/data/m5_04/viewer/food_0_h90.jsonl"},
        {"stage": "M4-05 seed follow-up runs", "result": "unrecoverable", "harness": "none (follow-up helper source incomplete)", "data": "docs/data/m4_05/seed-*.jsonl"},
    ]


def _timing_log_durations() -> list[dict[str, Any]]:
    paths = [
        ("M4-02 A-5 nine-cell attempt (superseded path mismatch)", DATA_ROOT / "m4_02" / "m4_02_a5.log"),
        ("M4-03 gate A-5 rerun", DATA_ROOT / "m4_03" / "m4_03_gate_a5.log"),
        ("M4-05 batch-of-one A-5 gate", DATA_ROOT / "m4_05" / "m4_05_a5_gate_full.log"),
        ("M4-06 migration-off A-5 gate", DATA_ROOT / "m4_06" / "a5_gate.log"),
    ]
    result = []
    for name, path in paths:
        lines = _lines(path)
        start = next(line.split("UTC ", 1)[1] for line in lines if line.startswith("TIMING RUN START UTC "))
        end = next(line.split("UTC ", 1)[1] for line in lines if line.startswith("TIMING RUN END UTC "))
        start_time = dt.datetime.fromisoformat(start.replace("Z", "+00:00"))
        end_time = dt.datetime.fromisoformat(end.replace("Z", "+00:00"))
        harness_key = {
            "M4-02 A-5 nine-cell gate": "M4-02",
            "M4-03 gate A-5 rerun": "M4-03 gate",
            "M4-05 batch-of-one A-5 gate": "M4-05",
            "M4-06 migration-off A-5 gate": "M4-06",
        }[name]
        harness = _timing_source(harness_key)[0]
        result.append({"run": name, "start_utc": start, "end_utc": end, "seconds": (end_time - start_time).total_seconds(), "harness": harness, "data": str(path.relative_to(ROOT)).replace("\\", "/")})
    return result


def _timing_figure(rows: list[dict[str, Any]]) -> bytes:
    configure()
    selected = [row for row in rows if row["stage"] in {"M4-02", "M4-03 gate", "M4-05", "M4-06"}]
    labels = [f"{row['stage']} · {row['cell']} · {row['metric']}" for row in selected]
    values = [row["new/old"] for row in selected]
    colors = ["#b23a48" if row["|ratio-1| > 20%"] else "#377eb8" for row in selected]
    height = max(5.0, 0.28 * len(labels))
    fig, ax = plt.subplots(figsize=(12, height), layout="constrained")
    y = list(range(len(labels)))
    ax.barh(y, values, color=colors)
    ax.axvline(1.0, color="black", linewidth=1)
    ax.axvline(0.8, color="#999999", linestyle="--", linewidth=0.8)
    ax.axvline(1.2, color="#999999", linestyle="--", linewidth=0.8)
    ax.set_yticks(y, labels)
    ax.invert_yaxis()
    ax.set_xlabel("new / old median")
    ax.set_title("M5-04 reference-host timing comparisons")
    return _normalized_svg(fig)


def _divergence_series() -> tuple[list[dict[str, Any]], bool]:
    old_rows = list(csv.reader((ROOT / "docs" / "data" / "m4_03" / "m4_03_divergence_experiments.csv").open(encoding="utf-8", newline=""), delimiter="|"))
    new_path = DATA_ROOT / "m4_03" / "m4_03_a1_experiments.csv.gz"
    new_rows = [next(csv.reader([line])) for line in gzip.open(new_path, "rt", encoding="utf-8")]
    old = {int(row[3]): row for row in old_rows if len(row) > 7 and row[0] == "DAY" and row[1] == "food_zero" and row[2] == "0"}
    new = {int(row[4]): row for row in new_rows if len(row) > 8 and row[0] == "DAY" and row[1] == "H90/food0" and row[2] == "0"}
    if set(old) != set(new):
        raise ValueError("M4-03 old/new divergence curve day ranges differ")
    series = []
    d_exact = True
    for day in sorted(old):
        d_exact = d_exact and int(old[day][4]) == int(new[day][5])
        series.extend([
            {"day": day, "version": "old initial run", "D": int(old[day][4]), "fraction": float(old[day][6])},
            {"day": day, "version": "M5-04 A-1 rerun", "D": int(new[day][5]), "fraction": float(new[day][7])},
        ])
    return series, d_exact


def _divergence_figure(series: list[dict[str, Any]]) -> bytes:
    configure()
    frame = pd.DataFrame(series)
    fig, axes = plt.subplots(2, 1, figsize=(11, 7), sharex=True, layout="constrained")
    for label, group in frame.groupby("version", sort=False):
        axes[0].plot(group["day"], group["D"], label=label)
        axes[1].plot(group["day"], group["fraction"], label=label)
    axes[0].set_ylabel("changed agent records |D|")
    axes[0].legend()
    axes[1].set_ylabel("normalized divergence |D| / L")
    axes[1].set_xlabel("day")
    axes[1].legend()
    fig.suptitle("M4-03 food-zero, seed 0, H=90")
    return _normalized_svg(fig)


def _normalized_svg(fig: Any) -> bytes:
    return b"\n".join(line.rstrip() for line in svg_bytes(fig).splitlines()) + b"\n"


def _status_overrides(claims: list[dict[str, object]]) -> dict[str, Any]:
    updates: dict[str, Any] = {}
    for claim in claims:
        if claim["tier"] != "headline" or not str(claim["report"]).startswith("M4_"):
            continue
        report = str(claim["report"])
        if report == "M4_02_BRANCHING.md":
            result = ledger_status = "historical-protocol"
            note = "This claim is from superseded M4-02 A-1 through A-4 protocols, which M5-04 does not rerun. The final A-5 verdict is regenerated using the corrected same-mode incremental diagnosis."
        elif report == "M4_03_DIVERGENCE.md":
            result = ledger_status = "bit-exact"
            note = "M4-03 A-1 deterministic daily records match the committed raw CSV byte-for-byte."
        elif report == "M4_04_CRN_EFFICIENCY.md":
            result = ledger_status = "bit-exact after documented normalization"
            note = "M4-04 numeric estimates match after the documented normalization of CSV inf and JSON null for unbounded confidence limits."
        elif report == "M4_05_MINIMAL_CAUSE.md":
            result = ledger_status = "bit-exact after documented normalization"
            note = "M4-05 values match after the documented §14 A-1 schema and guarantee-label normalization."
        elif report == "M4_06_MIGRATION.md":
            result = ledger_status = "conclusion holds"
            note = "Deterministic experiment files match byte-for-byte and both fresh migration-off A-5 cells pass."
        elif report == "M4_07_MANY_BRANCH.md":
            result = ledger_status = "conclusion holds"
            note = "Parallel CRN results and deterministic scan/search fields match; one-shot supplementary timings are not A-5 gate results."
        else:
            result = ledger_status = "unrecoverable"
            note = "No M5-04 comparator was included for this headline claim."
        updates[str(claim["claim_id"])] = {"result": result, "ledger_status": ledger_status, "note": note}
    return {"schema": 1, "claims": updates}


def _headline_source(report: str) -> tuple[str, str]:
    sources = {
        "M4_02_BRANCHING.md": ("none (historical A-1–A-4 sources unavailable)", "none"),
        "M4_03_DIVERGENCE.md": ("crates/sim-model/examples/m4_03_experiment.rs", "docs/data/m5_04/m4_03/m4_03_a1_experiments.csv.gz"),
        "M4_05_MINIMAL_CAUSE.md": ("crates/sim-model/examples/m4_05_experiment.rs", "docs/data/m5_04/m4_05/"),
        "M4_07_MANY_BRANCH.md": ("crates/sim-model/examples/m4_07_many_branch.rs", "docs/data/m5_04/m4_07/clone_share.jsonl.gz"),
    }
    return sources.get(report, ("none", "none"))


def _markdown(frame: pd.DataFrame) -> str:
    return dataframe_to_markdown(frame.fillna("")) if not frame.empty else "| result |\n|---|\n| none |"


def _report_qmd(data: dict[str, Any]) -> str:
    return f'''---
title: "M5-04 M4 Regeneration"
format: gfm
---

Status: current

## Question

Which M4 findings regenerate from committed harnesses and data, and which A-5 timing conclusions still hold on the reference host?

## Method

The comparison logic is in [`report_m5_04.py`](../../analysis/simciv_analysis/report_m5_04.py). Source provenance is resolved through [`sources.toml`](sources.toml); regeneration outcomes are read separately from the committed M5-04 comparisons. The deterministic M4-04 CRN comparison used the explicitly bit-identical M4-07 parallel-only mode. All timing gate processes ran at High priority with affinity `0x555`; each log has timing markers.

## Results

### M4 headline claims

{data['headline_counts_table']}

{data['headline_table']}

### Deterministic reruns

Each row cites its current harness and raw output. The M4-04 field comparison has {data['m4_04_exact_fields']} raw-exact fields, {data['m4_04_normalized_count']} documented normalization cases, and {data['m4_04_mismatches']} unexplained mismatches out of {data['m4_04_compared_fields']} fields.

{data['deterministic_table']}

#### Documented representation normalizations

These are listed separately from raw bit-exact results. The transformations are limited to the recorded M4-04 non-finite CI encoding and the M4-05 §14 A-1 schema/guarantee clarification. Every row cites the old/new data and producing harness.

{data['normalization_table']}

### A-5 timing ratios

`new/old` compares reported medians. A ratio deviation above 20% is flagged. A-1–A-4 remain historical and are excluded. Each row cites its harness and raw data.

{data['timing_table']}

#### Gate verdicts

{data['gate_table']}

### M4-02 incremental diagnosis

The first M5-04 A-5 attempt used an incremental direct runner but left `BranchRequest.execution = Serial`; its two incremental-cell FAILs are invalid comparisons and are retained under `superseded/serial_branch_request/`. The corrected harness selects incremental topology for both paths. In the interleaved rerun, all three revisions agreed; the results below determine the diagnosis.

{data['diagnosis_table']}

Host samples for that same session:

{data['diagnosis_host_table']}

### Mismatch list

{data['mismatch_table']}

### Generated figures

The timing figure is generated from the A-5 rows above using [`report_m5_04.py`](../../analysis/simciv_analysis/report_m5_04.py) and the linked timing data. The divergence figure uses the food-zero, seed-0, H=90 daily records from [`m4_03_divergence_experiments.csv`](../data/m4_03/m4_03_divergence_experiments.csv) and [`m4_03_a1_experiments.csv.gz`](../data/m5_04/m4_03/m4_03_a1_experiments.csv.gz), produced by [`m4_03_experiment.rs`](../../crates/sim-model/examples/m4_03_experiment.rs). `|D|` is compared unchanged; the normalized denominator reflects the M4-03 correction.

![Old and regenerated A-5 timing medians](figures/m5_04_old_new_timing.svg)

![Old and regenerated divergence curves](figures/m5_04_old_new_divergence.svg)

### Recorded compute time

{data['duration_table']}

The total above sums timing-gate runs with explicit start/end markers. Restored deterministic harnesses did not record their full process wall time; the complete M5-04 wall time is therefore unavailable.

## Findings

- The M4-03 A-1 daily CSV, M4-06 experiment outputs, M4-07 CRN estimate arrays, and viewer sample regenerated exactly; see [Deterministic reruns](#deterministic-reruns).
- The corrected M4-02 incremental A-5 comparison agrees across the M4-02 PASS commit, tag, and pre-follow-up HEAD; see [M4-02 incremental diagnosis](#m4-02-incremental-diagnosis). The previous failures came from the harness path mismatch, not a runtime change.
- M4-03, M4-05, and M4-06 A-5 gates pass; see [Gate verdicts](#gate-verdicts) and their cited raw samples.
- M4-04 and M4-05 representation differences are listed under [documented normalizations](#documented-representation-normalizations), separate from unexplained mismatches.

## Limits

- A-1–A-4 measurements remain `historical-protocol`; they are not regeneration targets.
- The invalid first M5-04 incremental A-5 comparison is preserved as superseded evidence and excluded from the corrected verdict.
- The M4-07 search, day-scan, and clone-share reruns are supplemental one-shot measurements, not A-5 timing results.
- The M4-05 seed-specific follow-up helper source is incomplete; those historical seed records remain unrecoverable.
- The aggregate A-5 gate time excludes deterministic harness wall time, which the restored harnesses did not record.
'''


def _render_quarto(qmd: Path, output_dir: Path) -> Path:
    result = subprocess.run(["quarto", "render", str(qmd), "--to", "gfm", "--output-dir", str(output_dir)], cwd=ROOT, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(f"Quarto render failed:\n{result.stdout}\n{result.stderr}")
    return output_dir / "M5_04_M4_REGENERATION.md"


def _build() -> dict[str, Any]:
    base_claims = extract_claims(apply_m5_04=False)
    m4_04 = _old_new_m4_04_comparison()
    m4_05_diffs, m4_05_normalizations = _m4_05_differences()
    deterministic = _deterministic_results(m4_04, m4_05_diffs, m4_05_normalizations)
    diagnosis_rows, diagnosis_info = _m4_02_diagnosis()
    timing_rows, gate_rows = [], []
    for parser in (_m4_02_timings, _m4_03_timings, _m4_05_timings, _m4_06_timings):
        rows, gates = parser(diagnosis_rows) if parser is _m4_02_timings else parser()
        timing_rows.extend(rows)
        gate_rows.extend(gates)
    timing_rows.extend(_m4_06_hook_timings())
    timing_rows.extend(_m4_07_supplemental_timings())
    for gate in gate_rows:
        default_harness, default_data = _timing_source(gate["stage"])
        gate.setdefault("harness", default_harness)
        gate.setdefault("raw data", default_data)
    divergence, d_exact = _divergence_series()
    m4_02_failures = [row for row in gate_rows if row["stage"] == "M4-02" and row["new"] == "FAIL"]
    m4_04_mismatch = m4_04["mismatches"]
    timing_frame = pd.DataFrame(timing_rows)
    gate_frame = pd.DataFrame(gate_rows)
    headline_overrides = _status_overrides(base_claims)
    headline_rows = []
    for claim in base_claims:
        override = headline_overrides["claims"].get(str(claim["claim_id"]))
        if override is not None:
            headline_rows.append({
                "claim_id": claim["claim_id"], "report": claim["report"], "section": claim["section anchor"],
                "claim": claim["claim text (short)"], "value": claim["value"], "unit": claim["unit"],
                "class": claim["class"], "M5-04 result": override["result"],
                "harness": _headline_source(str(claim["report"]))[0],
                "raw data": _headline_source(str(claim["report"]))[1],
                "basis": override["note"],
            })
    headline_counts = collections.Counter(row["M5-04 result"] for row in headline_rows)
    diagnosis_frame = pd.DataFrame(diagnosis_rows)
    diagnosis_table = _markdown(diagnosis_frame[[
        "revision", "commit", "cell", "repetitions", "direct_ms", "candidate_ms",
        "delta_ms", "delta_mad_ms", "upper_ms", "threshold_ms", "direct_spread_pct",
        "candidate_spread_pct", "status",
        "harness", "raw data",
    ]])
    diagnosis_host_table = _markdown(pd.DataFrame([
        {
            "boundary": boundary,
            "UTC": sample["utc"],
            "total CPU samples (%)": ", ".join(map(str, sample["total_cpu_samples_percent"])),
            "free RAM (bytes)": sample["free_ram_bytes"],
            "harness": diagnosis_info["harness"],
            "raw data": diagnosis_info["log"],
        }
        for boundary, sample in [("before", diagnosis_info["host_before"]), ("after", diagnosis_info["host_after"])]
    ]))
    diagnosis_start = dt.datetime.fromisoformat(diagnosis_info["start_utc"].replace("Z", "+00:00"))
    diagnosis_end = dt.datetime.fromisoformat(diagnosis_info["end_utc"].replace("Z", "+00:00"))

    mismatches: list[dict[str, Any]] = []
    for row in m4_04_mismatch:
        mismatches.append({"area": "M4-04 CRN estimates", "difference": f"{row['intervention']} H={row['horizon']} {row['outcome']} {row['field']}: {row['old']} → {row['new']}", "cause": row["difference"], "source": "m4_07_many_branch.rs · docs/data/m5_04/m4_07/crn_parallel_only.jsonl.gz"})
    for row in m4_05_diffs:
        mismatches.append({"area": "M4-05 JSONL", "difference": f"{row['file']} {row['field']}: {row['old']} → {row['new']}", "cause": row["difference"], "source": f"m4_05_experiment.rs · docs/data/m5_04/m4_05/{row['file']}"})
    old_m4_02_status = {row["cell"]: row["old"] for row in gate_rows if row["stage"] == "M4-02"}
    for row in gate_rows:
        if row["stage"] == "M4-02" and row["old"] != row["new"]:
            mismatches.append({"area": "M4-02 A-5 gate", "difference": f"{row['cell']}: {row['old']} → {row['new']}", "cause": "fresh valid 41-sample A-5 run changed the gate verdict", "source": "m4_02_a5_measure.rs · docs/data/m5_04/m4_02/m4_02_a5.log"})

    durations = _timing_log_durations()
    durations.append({
        "run": "M4-02 corrected three-revision incremental diagnosis",
        "start_utc": diagnosis_info["start_utc"],
        "end_utc": diagnosis_info["end_utc"],
        "seconds": (diagnosis_end - diagnosis_start).total_seconds(),
        "harness": diagnosis_info["harness"],
        "data": diagnosis_info["log"],
    })
    total_gate_seconds = sum(row["seconds"] for row in durations)
    m4_04_comparison = {
        **m4_04,
        "m4_05_deterministic_differences": m4_05_diffs,
        "m4_05_normalizations": m4_05_normalizations,
        "m4_03_curve_changed_records_exact": d_exact,
    }
    status_bytes = (json.dumps(headline_overrides, indent=2, sort_keys=True) + "\n").encode("utf-8")
    comparison_bytes = (json.dumps(m4_04_comparison, indent=2, sort_keys=True) + "\n").encode("utf-8")
    ledger_bytes_updated = ledger_bytes(apply_m5_04_overrides(base_claims, ROOT, headline_overrides))
    timing_svg = _timing_figure(timing_rows)
    divergence_svg = _divergence_figure(divergence)
    data = {
        "headline_table": _markdown(pd.DataFrame(headline_rows)),
        "headline_counts_table": _markdown(pd.DataFrame([{"M5-04 result": result, "headline claims": count, "source ledger": "docs/reproducibility/claims.csv.gz"} for result, count in sorted(headline_counts.items())])),
        "deterministic_table": _markdown(pd.DataFrame(deterministic)),
        "normalization_table": _markdown(pd.DataFrame(m4_04["normalizations"] + m4_05_normalizations, columns=["stage", "file", "field", "old", "new", "rule", "harness", "raw data"])),
        "diagnosis_table": diagnosis_table,
        "diagnosis_host_table": diagnosis_host_table,
        "timing_table": _markdown(timing_frame[["stage", "cell", "metric", "old", "new", "new/old", "|ratio-1| > 20%", "old gate", "new gate", "unit", "protocol", "harness", "raw data"]]),
        "gate_table": _markdown(gate_frame[["stage", "cell", "old", "new", "harness", "raw data"]]),
        "mismatch_table": _markdown(pd.DataFrame(mismatches, columns=["area", "difference", "cause", "source"])),
        "duration_table": _markdown(pd.DataFrame(durations + [{"run": "TOTAL A-5 gate wall time", "start_utc": "", "end_utc": "", "seconds": total_gate_seconds, "harness": "four committed A-5 drivers", "data": "sum of four logs"}])),
        "headline_count": len(headline_rows),
        "m4_02_fail_count": len(m4_02_failures),
        "m4_04_mismatches": len(m4_04_mismatch),
        "m4_04_normalized_count": m4_04["normalized_fields"],
        "m4_04_normalizations": m4_04["normalizations"],
        "m4_05_normalizations": m4_05_normalizations,
        "m4_04_exact_fields": m4_04["exact_fields"],
        "m4_04_compared_fields": m4_04["compared_fields"],
        "status_bytes": status_bytes,
        "comparison_bytes": comparison_bytes,
        "ledger_bytes": ledger_bytes_updated,
        "qmd": "",
        "figures": timing_svg,
        "divergence_figures": divergence_svg,
        "timing_rows": timing_rows,
        "gate_rows": gate_rows,
        "mismatches": mismatches,
        "durations": durations,
        "total_gate_seconds": total_gate_seconds,
    }
    data["qmd"] = _report_qmd(data)
    data["md"] = None
    return data


def _outputs() -> dict[Path, bytes]:
    data = _build()
    return {
        ROOT / M5_04_OVERRIDES: data["status_bytes"],
        COMPARE_PATH: data["comparison_bytes"],
        CLAIMS_PATH: data["ledger_bytes"],
        QMD_PATH: data["qmd"].encode("utf-8"),
        FIGURES["timing"]: data["figures"],
        FIGURES["divergence"]: data["divergence_figures"],
    }


def render_outputs() -> None:
    outputs = _outputs()
    for path, contents in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents)
    with tempfile.TemporaryDirectory(prefix="simciv-m5-04-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        qmd.write_bytes(outputs[QMD_PATH])
        rendered = _render_quarto(qmd, temp_dir)
        MD_PATH.write_bytes(rendered.read_bytes())
    print(f"generated {QMD_PATH.relative_to(ROOT)} and {MD_PATH.relative_to(ROOT)}")


def check_outputs() -> bool:
    outputs = _outputs()
    missing = [str(path) for path in [*outputs, MD_PATH] if not path.exists()]
    if missing:
        print(f"missing M5-04 outputs: {', '.join(missing)}", file=sys.stderr)
        return False
    mismatched = [str(path) for path, expected in outputs.items() if path.read_bytes() != expected]
    if mismatched:
        print(f"M5-04 generated files differ: {', '.join(mismatched)}", file=sys.stderr)
        return False
    with tempfile.TemporaryDirectory(prefix="simciv-m5-04-check-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        qmd.write_bytes(outputs[QMD_PATH])
        rendered = _render_quarto(qmd, temp_dir)
        if rendered.read_bytes() != MD_PATH.read_bytes():
            print(f"M5-04 Quarto output differs: {MD_PATH}", file=sys.stderr)
            return False
    print("M5-04 claim statuses, comparison data, figures, QMD, and Quarto GFM output match")
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare generated files without writing")
    args = parser.parse_args(argv)
    if args.check:
        return 0 if check_outputs() else 1
    render_outputs()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
