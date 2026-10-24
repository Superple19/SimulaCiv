"""Generate the M5-05 conclusion-level regeneration report from Git summaries."""

from __future__ import annotations

import argparse
import json
import math
import re
import statistics
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import Any

import pandas as pd

from .figures import configure, svg_bytes
from .findings import Finding, render_findings
from .m5_05 import _registry
from .provenance import ROOT
from .io import content_matches, write_text_lf
from .tables import dataframe_to_markdown

configure()

import matplotlib.pyplot as plt


RESULTS_PATH = ROOT / "docs" / "reproducibility" / "M5_05_M0_M3_REGENERATION_RESULTS.json"
QMD_PATH = ROOT / "docs" / "reproducibility" / "M5_05_M0_M3_REGENERATION.qmd"
MD_PATH = ROOT / "docs" / "reproducibility" / "M5_05_M0_M3_REGENERATION.md"
FIGURE_PATH = ROOT / "docs" / "reproducibility" / "figures" / "m5_05_old_new_timing.svg"
DETERMINISTIC_SUMMARY = ROOT / "docs" / "data" / "m5_05" / "deterministic" / "summary.json"
DIAGNOSIS_SUMMARY = ROOT / "docs" / "data" / "m5_05" / "m3_09_diagnosis" / "summary.json"
DIAGNOSIS_BISECT_SUMMARY = ROOT / "docs" / "data" / "m5_05" / "m3_09_diagnosis" / "bisect_summary.json"
M3_09_SCOPE_SUMMARY = ROOT / "docs" / "data" / "m5_05" / "m3_09_scope_check" / "summary.json"
M3_09_SCOPE_VALIDATION_SUMMARY = ROOT / "docs" / "data" / "m5_05" / "m3_09_scope_check" / "validate_for" / "summary.json"
M3_09_SCOPE_PLAN = ROOT / "docs" / "reproducibility" / "m5_05_configs" / "m3_09_scope_check.toml"
M3_09_REPORT = ROOT / "docs" / "reports" / "M3_09_INCREMENTAL_TOPOLOGY.md"
M3_AUDIT_REPORT = ROOT / "docs" / "reports" / "M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md"


def _summary(entry_id: str) -> dict[str, Any] | None:
    path = ROOT / "docs" / "data" / "m5_05" / entry_id / "summary.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else None


def _diagnosis() -> dict[str, Any] | None:
    if not DIAGNOSIS_SUMMARY.is_file():
        return None
    details = json.loads(DIAGNOSIS_SUMMARY.read_text(encoding="utf-8")).get("details", {})
    if DIAGNOSIS_BISECT_SUMMARY.is_file():
        bisect = json.loads(DIAGNOSIS_BISECT_SUMMARY.read_text(encoding="utf-8"))
        commit = bisect.get("first_changed_commit")
        details["bisect_summary"] = bisect
        details["bisect"] = commit
        details["phase_attribution"] = bisect.get("phase_attribution")
        details["bisect_patch_has_phase9b_append"] = bisect.get("bisect_patch_has_phase9b_append", False)
    return details


def _scope_check() -> dict[str, Any] | None:
    if not M3_09_SCOPE_SUMMARY.is_file() or not M3_09_SCOPE_VALIDATION_SUMMARY.is_file():
        return None
    summary = json.loads(M3_09_SCOPE_SUMMARY.read_text(encoding="utf-8"))
    validation = json.loads(M3_09_SCOPE_VALIDATION_SUMMARY.read_text(encoding="utf-8"))
    return {"summary": summary, "validation": validation, "plan": tomllib.loads(M3_09_SCOPE_PLAN.read_text(encoding="utf-8"))}


def _diagnosis_change_screen(details: dict[str, Any]) -> bool:
    stats = details["revisions"]
    order = [item["id"] for item in details["plan"]["values"]["revisions"]]
    threshold = float(details["plan"]["values"]["change_screen_percent"]) / 100.0
    scanned = []
    for before_id, after_id in zip(order, order[1:]):
        before, after = stats[before_id], stats[after_id]
        changes = [
            after["numerator_median_us_per_tick"] / before["numerator_median_us_per_tick"] - 1,
            after["denominator_median_us_per_tick"] / before["denominator_median_us_per_tick"] - 1,
            after["ratio"] / before["ratio"] - 1,
        ]
        valid = max(
            before["numerator_mad_over_median"], before["denominator_mad_over_median"],
            after["numerator_mad_over_median"], after["denominator_mad_over_median"],
        ) <= 0.05
        scanned.append({"before": before_id, "after": after_id, "step_change": valid and any(abs(value) > threshold for value in changes)})
    expected = next((item for item in scanned if item["step_change"]), None)
    recorded = details["first_step_change"]
    recorded_pair = None if recorded is None else (recorded["before"], recorded["after"])
    expected_pair = None if expected is None else (expected["before"], expected["after"])
    return recorded_pair == expected_pair


def _decomposition_matches_paths(details: dict[str, Any]) -> bool:
    rows = details["historical_decomposition"]["rows"]
    baseline, current = rows[1], rows[-1]
    numerator_delta = current["numerator_m3p_incremental_us"] / baseline["numerator_m3p_incremental_us"] - 1
    denominator_delta = current["denominator_m3_us"] / baseline["denominator_m3_us"] - 1
    expected = (
        "both, dominated by numerator"
        if numerator_delta > 0 and denominator_delta < 0 and abs(numerator_delta) > abs(denominator_delta)
        else "numerator" if numerator_delta > 0 else "denominator" if denominator_delta < 0 else "neither"
    )
    return expected == details["historical_decomposition"]["dominant_component"]


def _bisect_evidence_matches(details: dict[str, Any]) -> bool:
    bisect = details.get("bisect_summary")
    if not bisect or not details.get("bisect_patch_has_phase9b_append") or not _diagnosis_change_screen(details):
        return False
    commit = bisect.get("first_changed_commit")
    reproduced_candidate = bool(commit) and any(
        row.get("commit") == commit and row.get("status") == "changed"
        for row in bisect.get("tested_candidates", [])
    )
    old_ratio = details["historical_decomposition"]["rows"][1]["ratio"]
    same_revision_ratio = details["revisions"]["m3_09_followup"]["ratio"]
    return reproduced_candidate and abs(same_revision_ratio / old_ratio - 1) > 0.20


def _entry_cells(entry: dict[str, Any], summary: dict[str, Any] | None) -> list[dict[str, Any]]:
    if summary is None:
        return []
    rows = summary.get("details", {}).get("cells_measured", [])
    old_rows = summary.get("details", {}).get("historical_values", [])
    old_by_n: dict[int, dict[str, Any]] = {}
    old_by_cell: dict[str, dict[str, Any]] = {}
    for old in old_rows:
        old_by_cell[str(old.get("cell", ""))] = old
        if "N" in old:
            old_by_n[int(old["N"])] = old
        else:
            match = re.search(r"N(\d+)", str(old.get("cell", "")))
            if match:
                old_by_n[int(match.group(1))] = old
    output = []
    for row in rows:
        result = row.get("result", {})
        cell_id = str(row.get("cell", ""))
        n = int(row.get("N", 0))
        old = old_by_cell.get(cell_id)
        if old is None and (entry["id"] != "m3_11_frontier_crossover" or "pool2" in cell_id):
            old = old_by_n.get(n)
        exact_old = old is not None and "not necessarily" not in str(old.get("source_match", ""))
        measurements: list[dict[str, Any]] = []
        if old and exact_old:
            if "serial_median_us" in old and "rayon_median_us" in old:
                measurements.extend([
                    {"path": "serial", "old_us": old["serial_median_us"], "new_us": result.get("left_median_us")},
                    {"path": "rayon", "old_us": old["rayon_median_us"], "new_us": result.get("right_median_us")},
                ])
            elif "storage_median_us" in old:
                measurements.append({"path": "M3 storage", "old_us": old["storage_median_us"], "new_us": result.get("right_median_us")})
            elif "parallel_median_us" in old:
                measurements.extend([
                    {"path": "serial", "old_us": old["serial_median_us"], "new_us": result.get("left_median_us")},
                    {"path": "parallel", "old_us": old["parallel_median_us"], "new_us": result.get("right_median_us")},
                ])
            elif "search_reduction_median_us" in old:
                measurements.extend([
                    {"path": "serial", "old_us": old["serial_median_us"], "new_us": result.get("left_median_us")},
                    {"path": "search reduction", "old_us": old["search_reduction_median_us"], "new_us": result.get("right_median_us")},
                ])
            elif "indexed_before_us" in old:
                single = result.get("single_path", {})
                measurements.append({"path": "indexed M3 vs recorded pre-index baseline", "old_us": old["indexed_before_us"], "new_us": single.get("median_us") if isinstance(single, dict) else result.get("right_median_us")})
            elif "reversed_over_normal" in old:
                measurements.append({"path": "reversed/normal P4 selection slowdown", "old_us": old["reversed_over_normal"], "new_us": result.get("reverse_over_normal"), "unit": "×"})
            elif "old_ratio" in old:
                measurements.append({"path": "M3P/M3 equal-live ratio", "old_us": old["old_ratio"], "new_us": result.get("m3p_over_m3"), "unit": "×"})
            elif entry["id"] == "m3_11_phase9c_speedup" and "serial_median_us" in old:
                measurements.extend([
                    {"path": "Phase 9c serial", "old_us": old["serial_median_us"], "new_us": result.get("left_median_us")},
                    {"path": "Phase 9c pool 6", "old_us": old["parallel_median_us"], "new_us": result.get("right_median_us")},
                ])
            elif entry["id"] == "m3_11_whole_tick_speedup":
                for schedule, old_key in (("interleaved", "interleaved_speedup"), ("non-interleaved", "non_interleaved_speedup")):
                    new_result = result.get(schedule.replace("-", "_"), {})
                    measurements.append({"path": f"whole-tick {schedule} speedup", "old_us": old.get(old_key), "new_us": new_result.get("speedup_left_over_right")})
        for measurement in measurements:
            measurement.setdefault("unit", "µs")
            old_value = measurement.get("old_us")
            new_value = measurement.get("new_us")
            if isinstance(old_value, (int, float)) and isinstance(new_value, (int, float)) and old_value != 0:
                ratio = float(new_value) / float(old_value)
                measurement["ratio"] = ratio
                measurement["flag_over_20_percent"] = abs(ratio - 1.0) > 0.20
            else:
                measurement["ratio"] = None
                measurement["flag_over_20_percent"] = False
        output.append({
            "cell": cell_id,
            "N": n or None,
            "K": row.get("K"),
            "k": row.get("k"),
            "p_local": row.get("p_local"),
            "threads": row.get("threads"),
            "max_hops": row.get("max_hops"),
            "left_mode": row.get("left_mode"),
            "right_mode": row.get("right_mode"),
            "result": result,
            "historical": old,
            "historical_exact_match": exact_old,
            "measurements": measurements,
        })
    return output


def _scope_result() -> dict[str, Any] | None:
    scope = _scope_check()
    if scope is None:
        return None
    details = scope["summary"]["details"]
    plan = scope["plan"]
    cells = []
    for item in details["cells"]:
        historical = float(item["historical_ratio"])
        ratio = float(item["m3p_incremental_over_m3"])
        within = abs(ratio / historical - 1.0) <= float(plan["historical_ratio_tolerance"])
        if within != bool(item["within_historical_tolerance"]):
            raise ValueError(f"M3-09 scope classification differs from its measured ratio: {item}")
        paths = item["paths"]
        cells.append({
            "N": int(item["initial_population"]), "revision": item["revision"],
            "commit": item["commit"], "live": int(item["live_population"]),
            "cursor": int(item["checkpoint_cursor"]),
            "timed_internal_days": item["timed_internal_days"],
            "compaction_days": item["compaction_days"],
            "serial": paths["m3p_serial"], "incremental": paths["m3p_incremental"],
            "m3": paths["m3"], "ratio": ratio, "historical_ratio": historical,
            "within_20": within, "repetitions": int(item["repetitions_per_sample"]),
            "valid": bool(item["path_spread_valid"]),
            "state_digests": item["m3p_digests"],
        })
    primary = [row for row in cells if row["N"] == 100_000]
    if len(primary) != 2 or not all(row["valid"] for row in primary):
        raise ValueError("M3-09 protocol recheck requires both 100k revisions")
    if len({row["state_digests"]["state"] for row in primary}) != 1:
        raise ValueError("M3-09 protocol recheck 100k M3P state digests differ across revisions")
    details_scope = details["scope"]
    old_m5_summary = _summary("m3_09_incremental_equal_live")
    if old_m5_summary is None:
        raise ValueError("the committed M5-05 all-days summary is missing")
    old_m5_result = old_m5_summary["details"]["cells_measured"][0]["result"]
    return {
        "cells": cells, "details": details, "plan": plan,
        "primary_within_20": all(row["within_20"] for row in primary),
        "validation": details["validate_for"],
        "old_m5_result": old_m5_result,
        "old_m5_ratio": float(old_m5_result["m3p_over_m3"]),
        "scope_days": details_scope["reference_days"],
        "scope_compaction_days": details_scope["reference_compaction"],
        "reference_compaction_days": details_scope["reference_compaction"],
        "m5_days": details_scope["m5_05_days"],
        "m5_compaction_days": details_scope["m5_05_compaction"],
    }


def _m3_09_first_improvement(diagnosis: dict[str, Any] | None) -> dict[str, Any] | None:
    if diagnosis is None:
        return None
    bisect = diagnosis.get("bisect_summary")
    commit = diagnosis.get("bisect")
    if bisect is None or not commit:
        return None
    candidate = next((item for item in bisect.get("tested_candidates", []) if item.get("commit") == commit), None)
    baseline = diagnosis.get("revisions", {}).get("m3_09_followup")
    if candidate is None or baseline is None:
        return None
    old = float(baseline["numerator_median_us_per_tick"])
    new = float(candidate["numerator_median_us_per_tick"])
    return {"commit": commit, "baseline_us": old, "candidate_us": new, "relative_change": new / old - 1.0}


def _scope_later_update(scope: dict[str, Any], *, audit: bool) -> str:
    rows = {(row["N"], row["revision"]): row for row in scope["cells"]}
    followup = rows[(100_000, "m3_09_followup")]["ratio"]
    head = rows[(100_000, "HEAD")]["ratio"]
    all_days = scope["old_m5_ratio"]
    within = "within the pre-registered 20% band" if scope["primary_within_20"] else "outside the pre-registered 20% band"
    link = "[M5-05 protocol-scope table](../reproducibility/M5_05_M0_M3_REGENERATION.md#m3-09-protocol-scope-recheck)"
    if audit:
        return (
            "Later update (2026-10-24): the M3-09 follow-up's 1.49× value came from the equal-live rerun, "
            "not the separate Phase 9b profile on days 101–103. The protocol-matched 100k check measured "
            f"{followup:.2f}× at the follow-up revision and {head:.2f}× at HEAD, {within}; the M5-05 cursor-400 "
            f"sample included compaction day 400 and measured {all_days:.2f}×. Both protocols use one "
            f"three-day public call per path. See the generated {link}."
        )
    return (
        "Later update (2026-10-24): the days 101–103 values above are the Phase 9b diagnostic, not the "
        "1.49× equal-live ratio. The follow-up used a checkpoint after executed day 400, two warm-ups, "
        "11 samples, and a rotating M3P serial/incremental/M3 three-day-call schedule. Recovered helper "
        "source `docs/data/m5_recovered/recovered/session-history/C/AI/SimulaCiv/crates/sim-model/examples/"
        "m3p_09_followup_measure.rs:16,309-478` confirms the checkpoint call through executed day 400, the "
        "three-day public calls, two discarded warmups, 11 measured samples, and both schedules. Recovered "
        "command output `docs/data/m5_recovered/recovered/session-extracts.jsonl:148-151` contains the "
        "runner medians but no explicit ratio field; the ratio rows in the report are calculated from those "
        "medians. The protocol-matched "
        f"100k ratios were {followup:.2f}× at the follow-up revision and {head:.2f}× at HEAD, {within}. "
        f"The separate M5-05 cursor-400 sample covered days 400–402, included compaction day 400, and measured "
        f"{all_days:.2f}×. See the generated {link}."
    )


def _later_update_document(path: Path, note: str) -> bytes:
    start = "<!-- BEGIN GENERATED M5-05 M3-09 PROTOCOL UPDATE -->"
    end = "<!-- END GENERATED M5-05 M3-09 PROTOCOL UPDATE -->"
    text = path.read_text(encoding="utf-8")
    block = f"{start}\n\n{note}\n\n{end}"
    if start in text or end in text:
        if text.count(start) != 1 or text.count(end) != 1:
            raise ValueError(f"expected one M5-05 update block in {path}")
        text, count = re.subn(re.escape(start) + r"\n.*?\n" + re.escape(end), block, text, count=1, flags=re.DOTALL)
        if count != 1:
            raise ValueError(f"could not update M5-05 note in {path}")
    else:
        text = text.rstrip() + "\n\n" + block + "\n"
    return text.encode("utf-8")


def _status(entry_id: str, cells: list[dict[str, Any]]) -> tuple[str, str]:
    if not cells:
        return "inconclusive", "No recorded timing summary for this conclusion."
    exact = [m for cell in cells for m in cell["measurements"] if m.get("ratio") is not None]
    if entry_id == "m2_final_tick":
        new_values = [m["new_us"] for cell in cells for m in cell["measurements"] if m.get("new_us") is not None]
        holds = bool(new_values) and all(17_000 <= value <= 22_000 for value in new_values)
        return ("holds" if holds else "does not hold"), "Selected 100k serial and explicit-Rayon medians compared with the recorded 17–22 ms range."
    if entry_id == "m2_phase4_crossover":
        ratios = {str(cell["cell"]): float(cell["result"]["speedup_left_over_right"]) for cell in cells}
        low = next((value for cell, value in ratios.items() if "N1000" in cell), None)
        high = next((value for cell, value in ratios.items() if "N5000" in cell), None)
        one_thread = next((value for cell, value in ratios.items() if "N10000" in cell), None)
        if None not in {low, high, one_thread}:
            note = (
                f"Full-tick ratios (serial/Rayon) were 1k={low:.3f}, 5k={high:.3f}, "
                f"10k one-thread={one_thread:.3f}; combined-P4 timing was not separately measured."
            )
        else:
            note = "The selected full-tick cells are incomplete."
        return "inconclusive", note
    if entry_id == "m3_02_parallel_phase6b":
        ratios = [float(cell["result"]["speedup_left_over_right"]) for cell in cells]
        if len(ratios) == 3 and all(ratio <= 1.0 for ratio in ratios):
            return "holds", "All selected full-tick cells show no parallel gain; the historical 180-cell aggregate is not recomputed."
        return "inconclusive", "Selected cells do not establish the historical 180-cell aggregate."
    if entry_id == "m3_03_search_reduction":
        deltas = [cell["result"].get("paired_left_minus_right_median_us") for cell in cells]
        mads = [cell["result"].get("paired_left_minus_right_mad_us") for cell in cells]
        if all(delta is not None and mad is not None and delta <= mad for delta, mad in zip(deltas, mads, strict=True)):
            return "holds", "The selected paired savings do not exceed their paired MADs."
        return "does not hold", "At least one selected paired saving exceeded its paired MAD."
    if entry_id == "m3_06_settlement_index":
        return "inconclusive", "The current indexed path was measured, but the executable pre-index implementation is absent; the recorded before value is historical context only."
    if entry_id == "m3_07_reversed_slots":
        ratio = cells[0]["result"].get("reverse_over_normal") if cells else None
        if isinstance(ratio, (int, float)):
            holds = abs(float(ratio) / 2.3734 - 1.0) <= 0.20
            return ("holds" if holds else "does not hold"), f"The reversed/normal Phase 4 selection slowdown was {float(ratio):.3f}×; canonical digest parity was asserted."
        return "inconclusive", "The selected Phase 4 selection ratio was not recorded."
    if entry_id in {"m3_08_full_rebuild_equal_live", "m3_09_incremental_equal_live"}:
        measurement = next((item for item in cells[0]["measurements"] if item["path"] == "M3P/M3 equal-live ratio"), None) if cells else None
        if measurement and isinstance(measurement.get("old_us"), (int, float)) and isinstance(measurement.get("new_us"), (int, float)):
            holds = abs(float(measurement["new_us"]) / float(measurement["old_us"]) - 1.0) <= 0.20
            return ("holds" if holds else "does not hold"), f"The equal-live ratio was {float(measurement['new_us']):.3f}× versus the recorded {float(measurement['old_us']):.3f}×; the sustained non-propagation workload was used."
        return "inconclusive", "The selected equal-live result lacks a matched historical ratio."
    if entry_id == "m3_11_phase9c_speedup":
        ratio = cells[0]["result"].get("speedup_left_over_right") if cells else None
        if isinstance(ratio, (int, float)):
            return ("holds" if ratio > 1.0 else "does not hold"), f"The selected Phase 9c serial/pool-6 speedup was {float(ratio):.3f}×."
        return "inconclusive", "The selected Phase 9c cell is missing its speedup."
    if entry_id == "m3_11_whole_tick_speedup":
        result = cells[0]["result"] if cells else {}
        interleaved = result.get("interleaved", {}).get("speedup_left_over_right")
        non_interleaved = result.get("non_interleaved", {}).get("speedup_left_over_right")
        if isinstance(interleaved, (int, float)) and isinstance(non_interleaved, (int, float)):
            holds = interleaved > 1.0 and non_interleaved > 1.0
            return ("holds" if holds else "does not hold"), f"Interleaved/non-interleaved speedups were {float(interleaved):.3f}×/{float(non_interleaved):.3f}×."
        return "inconclusive", "The paired whole-tick schedules are incomplete."
    if entry_id == "m3_11_frontier_crossover":
        selected = [cell for cell in cells if cell["result"].get("paired_left_minus_right_median_us") is not None]
        if len(selected) == 3:
            expected_frontier = {2: 728, 4: 728, 6: 852}
            frontiers_match = all(
                cell["result"].get("max_frontier_size") == expected_frontier.get(int(cell.get("threads", 0)))
                for cell in selected
            )
            clears = frontiers_match and all(
                cell["result"]["paired_left_minus_right_median_us"]
                > cell["result"]["left_mad_us"] + cell["result"]["right_mad_us"]
                for cell in selected
            )
            return ("holds" if clears else "does not hold"), "Pool 2/4 at frontier 728 and pool 6 at frontier 852 were checked against the paired MAD sum and the recorded frontier size."
        return "inconclusive", "The selected frontier/pool cells are incomplete."
    if exact:
        return "inconclusive", "Representative cells were remeasured; the headline covers a broader population/grid than this session."
    return "inconclusive", "The selected timing values do not have an exact historical workload/mode match."


def _derived_summary(entry_id: str, cells: list[dict[str, Any]]) -> str:
    by_n = {int(cell["N"]): cell["result"] for cell in cells if cell.get("N") is not None}

    def alpha(low_n: int, high_n: int, field: str) -> float | None:
        low = by_n.get(low_n, {}).get(field)
        high = by_n.get(high_n, {}).get(field)
        if not isinstance(low, (int, float)) or not isinstance(high, (int, float)) or low <= 0 or high <= 0:
            return None
        return math.log(float(high) / float(low)) / math.log(high_n / low_n)

    if entry_id == "m2_near_linear":
        serial = alpha(50_000, 100_000, "left_median_us")
        rayon = alpha(50_000, 100_000, "right_median_us")
        if serial is not None and rayon is not None:
            return f"50k→100k α: serial {serial:.3f}; explicit Rayon {rayon:.3f}"
    if entry_id == "m3_01_serial_baseline":
        first = alpha(10_000, 50_000, "right_median_us")
        second = alpha(50_000, 100_000, "right_median_us")
        if first is not None and second is not None:
            return f"serial storage α: 10k→50k {first:.3f}; 50k→100k {second:.3f}"
    return "—"


def _new_values_text(cells: list[dict[str, Any]]) -> str:
    values: list[str] = []
    for cell in cells:
        result = cell.get("result", {})
        if isinstance(result.get("interleaved"), dict):
            interleaved = result["interleaved"]
            non_interleaved = result.get("non_interleaved", {})
            values.append(
                f"{cell['cell']} interleaved {interleaved.get('left_median_us', float('nan')):.3f}/{interleaved.get('right_median_us', float('nan')):.3f} µs/tick; "
                f"non-interleaved {non_interleaved.get('left_median_us', float('nan')):.3f}/{non_interleaved.get('right_median_us', float('nan')):.3f} µs/tick"
            )
            continue
        left, right = result.get("left_median_us"), result.get("right_median_us")
        if isinstance(left, (int, float)) and isinstance(right, (int, float)):
            unit = result.get("time_unit")
            suffix = f" {unit.replace('us', 'µs')}" if isinstance(unit, str) else " µs"
            frontier = f"; max frontier {result['max_frontier_size']}" if isinstance(result.get("max_frontier_size"), int) else ""
            values.append(f"{cell['cell']} {cell['left_mode']} {left:.3f}{suffix}; {cell['right_mode']} {right:.3f}{suffix}{frontier}")
        elif isinstance(result.get("single_path"), dict):
            single = result["single_path"]
            values.append(f"{cell['cell']} {cell['right_mode']} {single['median_us']:.3f} µs/tick")
    return "; ".join(values) or "not run"


def _build() -> dict[str, Any]:
    registry = _registry()
    deterministic = json.loads(DETERMINISTIC_SUMMARY.read_text(encoding="utf-8")) if DETERMINISTIC_SUMMARY.exists() else None
    scope_result = _scope_result()
    rows = []
    measurement_rows = []
    ratio_measurements = []
    measurement_harness_by_session = {
        1: "crates/sim-model/examples/m5_05_conclusion_measure.rs",
        2: "crates/sim-model/examples/m5_05_sessions23.rs",
        3: "crates/sim-model/examples/m5_05_sessions23.rs",
    }
    for entry in registry["conclusion"]:
        summary = _summary(entry["id"])
        config_overrides = summary.get("details", {}).get("config_overrides", []) if summary else []
        config_override_text = "; ".join(str(item) for item in config_overrides) or "—"
        cells = _entry_cells(entry, summary)
        ratio_measurements.extend(
            measurement
            for cell in cells
            for measurement in cell["measurements"]
            if measurement.get("ratio") is not None
        )
        status, note = _status(entry["id"], cells)
        if entry["id"] == "m3_09_incremental_equal_live" and scope_result is not None:
            status = "holds" if scope_result["primary_within_20"] else "does not hold"
            matched = [cell for cell in scope_result["cells"] if cell["N"] == 100_000]
            matched_text = ", ".join(f"{cell['revision']} {cell['ratio']:.3f}×" for cell in matched)
            note = (
                f"Protocol-matched cursor-401 ratios: {matched_text}; the separate M5-05 cursor-400 cell was "
                f"{scope_result['old_m5_ratio']:.3f}× and includes a compaction day."
            )
        raw = f"../data/m5_05/{entry['id']}/summary.json" if summary else "not recorded"
        harness_path = str(entry.get("harness", ""))
        measurement_harness = measurement_harness_by_session[int(entry["session"])]
        harness_link = (
            f"[{Path(harness_path).name}](../../{harness_path})"
            if harness_path and (ROOT / harness_path).is_file()
            else f"source unavailable: `{harness_path}`" if harness_path else "unmapped"
        )
        old_text = "; ".join(
            f"{item['cell']} {key} {value:.3f} {'agents' if 'frontier' in key else '×' if key.endswith('ratio') or key.endswith('_over_normal') or 'speedup' in key or 'slowdown' in key else 'µs'}"
            for cell in cells if (item := cell.get("historical"))
            for key, value in item.items()
            if (key.endswith("_us") or key.endswith("ratio") or "slowdown" in key or "speedup" in key or key.endswith("_over_normal") or key.startswith("frontier_")) and isinstance(value, (int, float))
        ) or "not exactly mapped"
        rows.append({
            "milestone": entry["milestone"],
            "conclusion": entry["id"],
            "report": Path(entry["report"]).name,
            "section_anchor": entry["section"],
            "old values": old_text,
            "new values": _new_values_text(cells),
            "new/old ratios": "; ".join(
                f"{cell['cell']} {measurement['path']} {measurement['ratio']:.3f}×"
                for cell in cells for measurement in cell["measurements"]
                if measurement.get("ratio") is not None
            ) or "—",
            "derived": _derived_summary(entry["id"], cells),
            ">20% flag": any(
                measurement.get("flag_over_20_percent")
                for cell in cells for measurement in cell["measurements"]
            ),
            "status": status,
            "notes": note,
            "harness": harness_link,
            "measurement harness": f"[{Path(measurement_harness).name}](../../{measurement_harness})",
            "summary": f"[summary]({raw})" if summary else raw,
            "source report": f"[{Path(entry['report']).name}](../../{entry['report']})",
        })
        for cell in cells:
            result = cell["result"]
            measurement_rows.append({
                "conclusion": entry["id"],
                "cell": cell["cell"],
                "N": cell["N"],
                "K": cell["K"],
                "k": cell["k"],
                "p_local": cell["p_local"],
                "config overrides": config_override_text,
                "threads": cell["threads"],
                "max hops": cell["max_hops"],
                "time unit": result.get("time_unit", "µs/tick"),
                "max frontier": result.get("max_frontier_size"),
                "levels": result.get("phase9c_levels"),
                "left median": result.get("left_median_us"),
                "left MAD": result.get("left_mad_us"),
                "right median": result.get("right_median_us"),
                "right MAD": result.get("right_mad_us"),
                "speedup": result.get("speedup_left_over_right"),
                "paired delta": result.get("paired_left_minus_right_median_us"),
                "paired delta MAD": result.get("paired_left_minus_right_mad_us"),
                "r": result.get("repetitions_per_sample"),
                "minimum sample s": result.get("minimum_sample_seconds"),
                "harness": f"[{Path(measurement_harness).name}](../../{measurement_harness})",
                "summary": f"[summary](../data/m5_05/{entry['id']}/summary.json)",
            })
    deterministic_results = deterministic.get("details", {}).get("results", []) if deterministic else []
    planned = [
            {
                "conclusion": entry["id"],
                "session": int(entry["session"]),
                "estimate_minutes": int(entry["estimate_minutes"]),
                "status": "recorded" if _summary(entry["id"]) is not None else "not run",
            }
            for entry in registry["conclusion"]
        ]
    sessions_not_run = sorted({row["session"] for row in planned if row["status"] == "not run"})
    sessions_complete = sorted({row["session"] for row in planned if row["status"] == "recorded"})
    results = {
        "session": max(sessions_complete, default=0),
        "sessions_complete": sessions_complete,
        "planned": planned,
        "deterministic": deterministic_results,
        "conclusions": rows,
        "cells": measurement_rows,
        "deterministic_summary": (
            "[deterministic summary](../data/m5_05/deterministic/summary.json)"
            if deterministic else "not recorded"
        ),
        "sessions_not_run": sessions_not_run,
        "m3_09_diagnosis": _diagnosis(),
        "m3_09_scope_check": scope_result,
    }
    conclusion_by_id = {row["conclusion"]: row for row in rows}
    unrun_ids = {row["conclusion"] for row in results["planned"] if row["status"] == "not run"}
    findings = [
        Finding(
            "[exploratory]",
            "Selected path ratios are flagged when the absolute deviation exceeds {{< meta ratio_flag_threshold_percent >}}%; the flag does not determine the conclusion status.",
            "finding_ratio_flags_match_threshold",
            bool(ratio_measurements)
            and all(
                measurement["flag_over_20_percent"] == (abs(float(measurement["ratio"]) - 1.0) > 0.20)
                for measurement in ratio_measurements
            ),
        ),
        Finding(
            "[exploratory]",
            "The deterministic test-result table records pass for every target; see [Deterministic confirmations](#deterministic-confirmations).",
            "finding_deterministic_targets_passed",
            bool(deterministic_results) and all(item["status"] == "pass" for item in deterministic_results),
        ),
    ]
    if unrun_ids:
        findings.append(Finding(
            "[exploratory]",
            "Unrun session conclusions remain inconclusive and are not counted as reproduced.",
            "finding_unrun_sessions_remain_inconclusive",
            all(conclusion_by_id.get(identifier, {}).get("status") == "inconclusive" for identifier in unrun_ids)
            and all(row["status"] == "not run" for row in results["planned"] if row["conclusion"] in unrun_ids),
        ))
    else:
        findings.append(Finding(
            "[exploratory]",
            "Every pre-registered timing stage has a recorded summary.",
            "finding_all_timing_stages_recorded",
            all(row["status"] == "recorded" for row in results["planned"]),
        ))
    diagnosis = results["m3_09_diagnosis"]
    if diagnosis is not None:
        findings.append(Finding(
            "[exploratory]",
            f"The first pre-registered source change was `{diagnosis.get('bisect')}` in the Phase 9B incremental append path; see the cross-revision table.",
            "finding_m3_09_bisect_matches_candidate_and_phase_diff",
            _bisect_evidence_matches(diagnosis),
        ))
    if scope_result is not None:
        scope_tag = "[pre-registered: M5-05-M3-09-PROTOCOL-RECHECK]"
        historical_100k = float(scope_result["plan"]["historical_ratio_100k"])
        all_days_differs = abs(scope_result["old_m5_ratio"] / historical_100k - 1.0) > float(scope_result["plan"]["historical_ratio_tolerance"])
        findings.append(Finding(
            scope_tag,
            "The M5-05 cursor-400 all-days ratio differs by more than the registered 20% band from the historical 100k equal-live ratio; its protocol includes a compaction day.",
            "finding_m3_09_all_days_ratio_outside_historical_band",
            all_days_differs and scope_result["m5_compaction_days"] == [400],
        ))
        if scope_result["primary_within_20"]:
            findings.append(Finding(
                scope_tag,
                "The protocol-matched 100k ratios fall within the pre-registered band around the M3-09 follow-up value; see the scope table.",
                "finding_m3_09_protocol_ratio_within_band",
                scope_result["primary_within_20"],
            ))
        else:
            findings.append(Finding(
                scope_tag,
                "The protocol-matched 100k ratios fall outside the pre-registered band around the M3-09 follow-up value; see the scope table.",
                "finding_m3_09_protocol_ratio_outside_band",
                not scope_result["primary_within_20"],
            ))
        findings.append(Finding(
            scope_tag,
            "The committed M5-05 cursor sample includes a compaction day; see the scope table.",
            "finding_m3_09_m5_cursor_includes_compaction",
            scope_result["m5_days"] == [400, 401, 402] and scope_result["m5_compaction_days"] == [400],
        ))
        validation = scope_result["validation"]
        internal = validation["internal_samples_us"]
        internal_median = statistics.median(internal)
        internal_mad = statistics.median(abs(value - internal_median) for value in internal)
        public_samples = validation["public_call_samples_us"]
        public_median = statistics.median(public_samples)
        findings.append(Finding(
            "[exploratory]",
            "The validation method and zero-day public-call timings are reported separately from A-5 full-tick samples.",
            "finding_m3_09_validate_for_samples_match_summary",
            len(internal) == int(validation["calls"]) == 41
            and abs(internal_median - float(validation["validate_for_median_us_per_call"])) < 1e-6
            and abs(internal_mad - float(validation["validate_for_mad_us_per_call"])) < 1e-6
            and abs(public_median - float(validation["public_zero_day_call_median_us"])) < 1e-6,
        ))
    findings_md, findings_checks_md = render_findings(findings)
    results["findings"] = findings_md
    results["findings_checks"] = findings_checks_md
    figure = _timing_figure(measurement_rows)
    qmd = _render_qmd(results)
    return {"results": results, "results_bytes": (json.dumps(results, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8"), "figure": figure, "qmd": qmd.encode("utf-8")}


def _timing_figure(rows: list[dict[str, Any]]) -> bytes:
    matched = []
    for row in rows:
        for entry in _registry()["conclusion"]:
            if entry["id"] != row["conclusion"]:
                continue
            summary = _summary(entry["id"])
            if not summary:
                continue
            cells = _entry_cells(entry, summary)
            for cell in cells:
                if cell["cell"] != row["cell"]:
                    continue
                for measure in cell["measurements"]:
                    if measure.get("unit", "µs") != "µs":
                        continue
                    if measure.get("ratio") is not None:
                        matched.append((f"{entry['id']}\n{cell['cell']}\n{measure['path']}", measure["old_us"], measure["new_us"]))
    fig, ax = plt.subplots(figsize=(9, max(2.5, 0.35 * len(matched))))
    if matched:
        positions = list(range(len(matched)))
        ax.scatter([item[1] for item in matched], positions, label="recorded", marker="o")
        ax.scatter([item[2] for item in matched], positions, label="M5-05 current measurements", marker="x")
        ax.set_yticks(positions, [item[0] for item in matched])
        ax.legend(loc="best")
    else:
        ax.text(0.5, 0.5, "No exact old/new timing pairs recorded yet", ha="center", va="center")
        ax.set_axis_off()
    ax.set_xlabel("µs per operation (timing comparisons only)")
    ax.set_title("Recorded versus M5-05 selected timings")
    fig.tight_layout()
    rendered = b"\n".join(line.rstrip(b" \t") for line in svg_bytes(fig).splitlines()) + b"\n"
    plt.close(fig)
    return rendered


def _table(rows: list[dict[str, Any]], columns: list[str]) -> str:
    frame = pd.DataFrame(rows, columns=columns)
    if frame.empty:
        frame = pd.DataFrame([{column: "—" for column in columns}], columns=columns)
    return dataframe_to_markdown(frame.fillna(""))


def _render_qmd(data: dict[str, Any]) -> str:
    conclusions = data["conclusions"]
    cells = data["cells"]
    deterministic = data["deterministic"]
    deterministic_rows = [
        {
            "target": item["target"],
            "status": item["status"],
            "harness": f"[{item['target']}.rs](../../crates/sim-model/tests/{item['target']}.rs)",
            "summary": data["deterministic_summary"],
        }
        for item in deterministic
    ]
    deterministic_table = _table(deterministic_rows, ["target", "status", "harness", "summary"])
    completed = ", ".join(str(session) for session in data["sessions_complete"]) or "none"
    pending = ", ".join(str(session) for session in data["sessions_not_run"]) or "none"
    scope_result = data.get("m3_09_scope_check")
    scope_status = "not recorded" if scope_result is None else (
        "within the protocol-matched range" if scope_result["primary_within_20"] else "outside the protocol-matched range"
    )
    status = f"Status: sessions complete {completed}; sessions pending {pending}; M3-09 protocol recheck {scope_status}"
    if scope_result is None:
        scope_section = "## M3-09 protocol scope recheck\n\nNo protocol-matched recheck has been recorded."
    else:
        plan = scope_result["plan"]
        validation = scope_result["validation"]
        old_result = scope_result["old_m5_result"]
        followup_days = ", ".join(str(day) for day in plan["reference_timed_internal_days"])
        current_days = ", ".join(str(day) for day in scope_result["m5_days"])
        matched_rows = [
            {
                "N": cell["N"], "revision": cell["revision"], "commit": cell["commit"],
                "live": cell["live"], "cursor": cell["cursor"],
                "days": ", ".join(str(day) for day in cell["timed_internal_days"]),
                "compaction": ", ".join(str(day) for day in cell["compaction_days"]) or "none",
                "M3P serial median ± MAD µs/tick": f"{cell['serial']['median_us_per_tick']:.3f} ± {cell['serial']['mad_us_per_tick']:.3f}",
                "M3P incremental median ± MAD µs/tick": f"{cell['incremental']['median_us_per_tick']:.3f} ± {cell['incremental']['mad_us_per_tick']:.3f}",
                "M3 median ± MAD µs/tick": f"{cell['m3']['median_us_per_tick']:.3f} ± {cell['m3']['mad_us_per_tick']:.3f}",
                "incremental / M3": cell["ratio"], "follow-up ratio": cell["historical_ratio"],
                "within 20%": cell["within_20"], "r": cell["repetitions"], "valid": cell["valid"],
            }
            for cell in scope_result["cells"]
        ]
        protocol_rows = [
            {
                "measurement": "M3-09 follow-up as documented",
                "checkpoint / timed days": "executed days 0–400; cursor 401; timed days 401–403",
                "compaction": "none",
                "warmups / samples": f"{plan['reference_warmups']} / {plan['reference_samples']}",
                "public calls / sample": f"one call per runner; {plan['days_per_public_call']} days each",
                "timed runners": "M3P serial → M3P incremental → M3 (rotating)",
                "context timing": "contexts built outside sample timer",
            },
            {
                "measurement": "M5-05 selected cell",
                "checkpoint / timed days": f"cursor {scope_result['details']['scope']['m5_05_cursor']}; {current_days}",
                "compaction": ", ".join(str(day) for day in scope_result["m5_compaction_days"]) or "none",
                "warmups / samples": f"{plan['m5_05_warmups']} / {plan['m5_05_samples']}",
                "public calls / sample": f"{old_result['repetitions_per_sample']} repeated calls per runner; {old_result['days_per_operation']} days each",
                "timed runners": "M3P incremental ↔ M3; M3P serial used only for parity",
                "context timing": "contexts built before each call timer",
            },
            {
                "measurement": "Protocol-matched A-5 recheck",
                "checkpoint / timed days": f"cursor {plan['reference_checkpoint_cursor']}; {followup_days}",
                "compaction": ", ".join(str(day) for day in scope_result["reference_compaction_days"]) or "none",
                "warmups / samples": f"{plan['m5_05_warmups']} / {plan['m5_05_samples']}",
                "public calls / sample": f"r calls per runner; {plan['days_per_public_call']} days each",
                "timed runners": "M3P serial → M3P incremental → M3 (rotating)",
                "context timing": "contexts built outside each call timer",
            },
        ]
        validation_rows = [{
            "measurement": "M3pStorageRunContext::validate_for",
            "N": validation["population"], "calls": validation["calls"],
            "median µs/call": validation["validate_for_median_us_per_call"],
            "MAD µs/call": validation["validate_for_mad_us_per_call"],
            "public zero-day median µs": validation["public_zero_day_call_median_us"],
            "public zero-day MAD µs": validation["public_zero_day_call_mad_us"],
        }]
        scope_section = f'''## M3-09 protocol scope recheck

The recovered helper source at `docs/data/m5_recovered/recovered/session-history/C/AI/SimulaCiv/crates/sim-model/examples/m3p_09_followup_measure.rs:16,309-478` confirms the checkpoint run executes 401 days from the initial state through day 400, then restores each runner from that checkpoint. Its sample function makes one public API call for three consecutive days (lines 343–428). The paired schedule runs 13 iterations, discards the first two as warmups, retains 11 samples, and rotates runner order (lines 431–456); the non-interleaved schedule runs runners sequentially (lines 459–478). Recovered command output at `docs/data/m5_recovered/recovered/session-extracts.jsonl:148-151` records paired and non-interleaved runner medians. The recovered output does not print a ratio field; the report ratios are calculated from those medians.

The committed M5-05 driver constructs its checkpoint with `m3p_checkpoint` in [m5_05_sessions23.rs](../../crates/sim-model/examples/m5_05_sessions23.rs). Its cursor is 400, so its three-day call covers days {current_days} and includes compaction day {', '.join(str(day) for day in scope_result['m5_compaction_days'])}. For the 100k cell, the M5-05 timing protocol was: “2 warmups; 41 A-5 samples; three-day calls; {old_result['repetitions_per_sample']} repetitions per sample; M3P incremental/M3 pair only; contexts built outside each timer.” The 10k selected cell used {scope_result['cells'][0]['repetitions']} repetitions per sample. Both methods call each public runner once per three simulated days; M5-05 repeats that operation to meet A-5 sample duration, so validation cadence per simulated day is equal. Other differences are the checkpoint/day boundary and compaction, three-runner rotation versus a two-path pair, 11 versus 41 measured samples, and one operation versus repeated operations per sample. The historical report does not record process priority or affinity, so those controls cannot be compared. The configs, seed, replicate, metrics/events setting, snapshots-off setting, and serial execution mode match.
{_table(protocol_rows, ["measurement", "checkpoint / timed days", "compaction", "warmups / samples", "public calls / sample", "timed runners", "context timing"])}

The original equal-live definition is preserved: M3 starts from the same live M3P agent records and settlement state, while its graph is profile-specific. The matched data uses the same config, seed, replicate, action settings, metrics/events, and serial execution on both revisions. Harness: [m5_05_m3_09_scope_check.py](../../crates/sim-model/examples/m5_05_m3_09_scope_check.py) and [m5_05_m3_09_revision.rs](../../crates/sim-model/examples/m5_05_m3_09_revision.rs); plan: [m3_09_scope_check.toml](m5_05_configs/m3_09_scope_check.toml); Git summary: [protocol-matched summary](../data/m5_05/m3_09_scope_check/summary.json); raw samples are DVC-managed at `docs/data/m5_05/m3_09_scope_check/raw/samples.jsonl`.

{_table(matched_rows, ["N", "revision", "commit", "live", "cursor", "days", "compaction", "M3P serial median ± MAD µs/tick", "M3P incremental median ± MAD µs/tick", "M3 median ± MAD µs/tick", "incremental / M3", "follow-up ratio", "within 20%", "r", "valid"])}

### Validation cost per public call

The probe uses a temporary `Instant` timer inside `M3pStorageRunContext::validate_for`, enabled only during zero-day validation calls. That instrumentation was removed before the A-5 runner comparison. The public zero-day call also includes the empty registered-loop setup, shown as a separate upper-bound measurement.

Harness: [scope-check worker](../../crates/sim-model/examples/m5_05_m3_09_revision.rs); summary: [validation probe](../data/m5_05/m3_09_scope_check/validate_for/summary.json); raw timing output is DVC-managed.

{_table(validation_rows, ["measurement", "N", "calls", "median µs/call", "MAD µs/call", "public zero-day median µs", "public zero-day MAD µs"])}
'''
    diagnosis = data.get("m3_09_diagnosis")
    if diagnosis is None:
        diagnosis_section = """## M3-09 ratio diagnosis

No cross-revision diagnosis has been recorded. The pre-registered plan is [m3_09_diagnosis.toml](m5_05_configs/m3_09_diagnosis.toml); its committed worker is [m5_05_m3_09_revision.rs](../../crates/sim-model/examples/m5_05_m3_09_revision.rs).
"""
    else:
        historical = diagnosis["historical_decomposition"]
        historical_table = _table(historical["rows"], [
            "source", "numerator_m3p_incremental_us", "numerator_mad_us",
            "denominator_m3_us", "denominator_mad_us", "ratio",
        ])
        revision_rows = []
        for revision in diagnosis["plan"]["values"]["revisions"]:
            stats = diagnosis["revisions"][revision["id"]]
            worker = next(item for item in diagnosis["workers"] if item["revision_id"] == revision["id"])
            revision_rows.append({
                "revision": revision["id"], "commit": revision["commit"],
                "live population": worker["live_population"],
                "numerator median µs/tick": stats["numerator_median_us_per_tick"],
                "numerator MAD µs/tick": stats["numerator_mad_us_per_tick"],
                "numerator MAD/median": stats["numerator_mad_over_median"],
                "denominator median µs/tick": stats["denominator_median_us_per_tick"],
                "denominator MAD µs/tick": stats["denominator_mad_us_per_tick"],
                "denominator MAD/median": stats["denominator_mad_over_median"],
                "M3P/M3": stats["ratio"], "r": stats["repetitions_per_sample"],
                "samples": stats["sample_count"],
                "valid": diagnosis["validity"][revision["id"]],
            })
        change_rows = [
            {"before": row["before"], "after": row["after"],
             "numerator change": row["relative_changes"]["numerator"],
             "denominator change": row["relative_changes"]["denominator"],
             "ratio change": row["relative_changes"]["ratio"],
             "step change": row["step_change"], "valid": row["valid"]}
            for row in diagnosis["adjacent_changes"]
        ]
        step = diagnosis["first_step_change"]
        step_text = "No valid adjacent transition exceeded the pre-registered change screen." if step is None else (
            f"First screen-positive adjacent transition: `{step['before']}` → `{step['after']}`."
        )
        bisect_summary = diagnosis.get("bisect_summary")
        bisect_rows = []
        if bisect_summary is not None:
            for candidate in bisect_summary["tested_candidates"]:
                candidate_commit = candidate["commit"]
                bisect_rows.append({
                    "commit": candidate_commit,
                    "numerator median µs/tick": candidate.get("numerator_median_us_per_tick"),
                    "numerator MAD µs/tick": candidate.get("numerator_mad_us_per_tick"),
                    "denominator median µs/tick": candidate.get("denominator_median_us_per_tick"),
                    "denominator MAD µs/tick": candidate.get("denominator_mad_us_per_tick"),
                    "ratio": candidate.get("ratio"),
                    "numerator MAD/median": candidate.get("spreads", {}).get("numerator_mad_over_median"),
                    "denominator MAD/median": candidate.get("spreads", {}).get("denominator_mad_over_median"),
                    "status": candidate.get("status"),
                    "raw data": f"`docs/data/m5_05/m3_09_diagnosis/bisect_raw/{candidate_commit[:12]}.json`",
                })
        bisect_text = "Bisect not run: no valid step change was detected." if step is None else (
            f"A-5 `git bisect run` identified `{diagnosis.get('bisect')}` as the first changed commit."
        )
        phase = diagnosis.get("phase_attribution")
        phase_text = "Phase attribution: not run." if phase is None else (
            f"The example timer brackets the public three-day M3P incremental runner call; the changed source diff maps the first change to {phase}."
        )
        improvement = _m3_09_first_improvement(diagnosis)
        improvement_text = "" if improvement is None else (
            f"The first changed commit was `{improvement['commit']}`; its Phase 9b incremental append path reduced the measured numerator by "
            f"{-100.0 * improvement['relative_change']:.1f}% relative to the preceding candidate in that bisect."
        )
        diagnosis_section = f"""## M3-09 ratio diagnosis

### Absolute numerator and denominator decomposition

Values are µs/tick. Numerator is the M3P incremental-topology runner; denominator is M3 at the same live population. The historical and current absolute paths are extracted from [M3-09 report](../../docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md) and [M5-05 summary](../data/m5_05/m3_09_incremental_equal_live/summary.json). New runner timings come from [m5_05_m3_09_revision.rs](../../crates/sim-model/examples/m5_05_m3_09_revision.rs).

{historical_table}

Against `{historical['comparison_reference']}`, numerator changed {historical['numerator_change_percent']:.1f}%, denominator changed {historical['denominator_change_percent']:.1f}%, and the ratio changed {historical['ratio_change_percent']:.1f}%. The decomposition attributes the ratio change to **{historical['dominant_component']}**.

### Cross-revision A-5 results

The paired execution options are `{diagnosis['workers'][0]['execution_options']}` for every revision. Checkpoint live population: {diagnosis['checkpoint_live_population']}. M3P and M3 digest parity across revisions: {diagnosis['checkpoint_digest_parity']}. The worker is [m5_05_m3_09_revision.rs](../../crates/sim-model/examples/m5_05_m3_09_revision.rs); raw rounds and calibration probes are `docs/data/m5_05/m3_09_diagnosis/raw/samples.jsonl`, DVC-managed.

{_table(revision_rows, ["revision", "commit", "live population", "numerator median µs/tick", "numerator MAD µs/tick", "numerator MAD/median", "denominator median µs/tick", "denominator MAD µs/tick", "denominator MAD/median", "M3P/M3", "r", "samples", "valid"])}

The screen was fixed at {diagnosis['plan']['values']['change_screen_percent']:.0f}% before timing. A transition is screen-positive if the numerator, denominator, or ratio changes by more than that amount from the previous valid revision.

{_table(change_rows, ["before", "after", "numerator change", "denominator change", "ratio change", "step change", "valid"])}

{step_text} {bisect_text} {phase_text} {improvement_text}

The bisect harness is [m5_05_m3_09_bisect.py](../../crates/sim-model/examples/m5_05_m3_09_bisect.py); per-candidate raw samples are DVC-managed. The example timer includes the full public runner call, so the phase attribution below comes from the first changed commit's source diff, not an internal Phase 9B subphase stopwatch.

{_table(bisect_rows, ["commit", "numerator median µs/tick", "numerator MAD µs/tick", "denominator median µs/tick", "denominator MAD µs/tick", "ratio", "numerator MAD/median", "denominator MAD/median", "status", "raw data"])}

Build/checkpoint time is outside the timed samples. Repetitions per sample were fixed at {diagnosis['repetitions_per_sample']}; total timing window {diagnosis['timing_wall_seconds']:.1f} s; complete run {diagnosis['total_wall_seconds']:.1f} s. Protocol and revision order are in [m3_09_diagnosis.toml](m5_05_configs/m3_09_diagnosis.toml).
"""
    return f'''---
title: "M5-05 M0–M3 Conclusion Regeneration"
format: gfm
ratio_flag_threshold_percent: 20
---

{status}

## Question

Do the selected conclusion-bearing M2/M3 timing results and mapped deterministic results still support the claims in the M0–M3 reports?

## Method

The frozen conclusion set is [`m5_05_conclusions.toml`](m5_05_conclusions.toml). Each timing stage runs its committed measurement example with A-5 sampling; every timing DVC output has a Git summary and a raw output directory. This report reads only Git summaries, not DVC raw paths. Historical values are extracted from the named reports by [m5_05.py](../../analysis/simciv_analysis/m5_05.py). Session cell data and timing are collected in the summaries under `docs/data/m5_05/`.

## Results

### Conclusion status

{_table(conclusions, ["milestone", "conclusion", "old values", "new values", "new/old ratios", "derived", ">20% flag", "status", "notes", "harness", "measurement harness", "summary", "source report"])}

### Planned conclusion set

{_table(data["planned"], ["conclusion", "session", "estimate_minutes", "status"])}

### Selected cells

{_table(cells, ["conclusion", "cell", "N", "K", "k", "p_local", "config overrides", "threads", "max hops", "time unit", "max frontier", "levels", "left median", "left MAD", "right median", "right MAD", "speedup", "paired delta", "paired delta MAD", "r", "minimum sample s"])}

{diagnosis_section}

{scope_section}

### Deterministic confirmations

{deterministic_table}

### Timing comparison figure

![Recorded and M5-05 selected timings](figures/m5_05_old_new_timing.svg)

### Sessions not run

{f"Sessions {pending} remain not run; their conclusion rows remain inconclusive." if data["sessions_not_run"] else "All registered sessions have a recorded summary."} This report does not substitute selected cells for broader historical grids or aggregate statistics.

## Findings

{data['findings']}

## Limits

- Session 1 uses representative cells; M2-02 and other historical grid aggregates remain inconclusive from samples.
- Timing is specific to the recorded reference host and A-5 sampling protocol.
- No raw DVC object is needed to run `just check`; summaries are the only report inputs.

## Appendix: paired findings checks

{data['findings_checks']}
'''


def _render_quarto(qmd: Path, output_dir: Path) -> Path:
    result = subprocess.run(
        ["quarto", "render", str(qmd), "--to", "gfm", "--output-dir", str(output_dir)],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Quarto render failed: {result.stdout}\n{result.stderr}")
    rendered = output_dir / qmd.with_suffix(".md").name
    if not rendered.is_file():
        raise FileNotFoundError(rendered)
    return rendered


def _outputs() -> dict[Path, bytes]:
    data = _build()
    outputs = {
        RESULTS_PATH: data["results_bytes"],
        FIGURE_PATH: data["figure"],
        QMD_PATH: data["qmd"],
    }
    scope = data["results"]["m3_09_scope_check"]
    if scope is not None:
        outputs[M3_09_REPORT] = _later_update_document(M3_09_REPORT, _scope_later_update(scope, audit=False))
        outputs[M3_AUDIT_REPORT] = _later_update_document(M3_AUDIT_REPORT, _scope_later_update(scope, audit=True))
    return outputs


def render_outputs() -> None:
    outputs = _outputs()
    for path, contents in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        write_text_lf(path, contents.decode("utf-8"))
    with tempfile.TemporaryDirectory(prefix="simciv-m5-05-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        write_text_lf(qmd, outputs[QMD_PATH].decode("utf-8"))
        rendered = _render_quarto(qmd, temp_dir)
        write_text_lf(MD_PATH, rendered.read_text(encoding="utf-8"))
    print(f"generated {QMD_PATH.relative_to(ROOT)} and {MD_PATH.relative_to(ROOT)}")


def check_outputs() -> bool:
    outputs = _outputs()
    missing = [str(path) for path in [*outputs, MD_PATH] if not path.exists()]
    if missing:
        print(f"missing M5-05 outputs: {', '.join(missing)}", file=sys.stderr)
        return False
    mismatched = [
        str(path) for path, expected in outputs.items() if not content_matches(path, expected)
    ]
    if mismatched:
        print(f"M5-05 generated files differ: {', '.join(mismatched)}", file=sys.stderr)
        return False
    with tempfile.TemporaryDirectory(prefix="simciv-m5-05-check-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        write_text_lf(qmd, outputs[QMD_PATH].decode("utf-8"))
        rendered = _render_quarto(qmd, temp_dir)
        normalized = temp_dir / "normalized.md"
        write_text_lf(normalized, rendered.read_text(encoding="utf-8"))
        if normalized.read_bytes() != MD_PATH.read_bytes():
            print(f"M5-05 Quarto output differs: {MD_PATH}", file=sys.stderr)
            return False
    print("M5-05 summaries, result table, figure, QMD, and Quarto GFM output match")
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    if args.check:
        return 0 if check_outputs() else 1
    render_outputs()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
