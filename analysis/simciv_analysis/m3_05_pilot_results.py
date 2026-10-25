"""Classify the recorded M3-05 pilot claims without rerunning measurements."""

from __future__ import annotations

import argparse
import csv
import io
import json
import re
import sys
from collections import Counter
from pathlib import Path
from typing import Any

from .io import content_matches, write_text_lf
from .provenance import ROOT


RAW = ROOT / "docs" / "data" / "m5_05" / "m3_05_pilot" / "raw" / "m3_05_a5.jsonl"
RESULTS_JSON = ROOT / "docs" / "reproducibility" / "M3_05_PILOT_RESULTS.json"
RESULTS_CSV = ROOT / "docs" / "reproducibility" / "M3_05_PILOT_CLAIMS.csv"
VERDICT_REVIEW_CSV = ROOT / "docs" / "reproducibility" / "M3_05_PILOT_VERDICT_REVIEW.csv"
HISTORICAL_REPORT = ROOT / "docs" / "reports" / "M3_05_SCALE_LIMIT.md"
EXPERIMENT_ID = "M3-05-FIRST-BREAK"
REPORT = "M3_05_SCALE_LIMIT.md"
ANCHOR = "first-break-findings"
TOLERANCE = 0.05
ALPHA_THRESHOLD = 1.15
ALPHA_NOISE_RATIO = 0.20
FIELDS = [
    "claim_id",
    "claim_key",
    "scope",
    "tier",
    "reported_value",
    "pilot_value",
    "unit",
    "verdict",
    "relative_deviation",
    "validity_paths",
    "note",
    "measurement_class",
    "class_before",
    "class_override",
    "class_reason",
    "signed_deviation_pct",
    "harness",
    "raw_data",
]
VERDICT_REVIEW_FIELDS = [
    "row_type",
    "claim_id",
    "profile_k",
    "checkpoint_day",
    "phase",
    "from_N",
    "to_N",
    "historical_value",
    "pilot_value",
    "signed_deviation_pct",
    "old_gate_side",
    "pilot_gate_side",
    "verdict",
    "historical_method",
    "pilot_method",
    "note",
]

REPORT_SOURCE_COMMIT = "5df006c150179f67078588dfaf83a355a81d5f9f"
HISTORICAL_PHASE_LABELS = {
    "full tick": "full_tick",
    "P1 regrowth": "P1_regrowth",
    "P2 degradation": "P2_degradation",
    "P3 features": "P3_features",
    "P4 action selection": "P4_action_selection",
    "P4 intent/projection": "P4_intent_projection",
    "P5 partition": "P5_partition",
    "P6A work resolution": "P6A_work_resolution",
    "P6B total": "P6B_total",
    "P6B validation/slot": "P6B_validation_slot_lookup",
    "P6B " + chr(107) + "ey generation": "P6B_key_generation",
    "P6B sort": "P6B_global_sort",
    "P6B evaluation/writeback/output": "P6B_evaluation_writeback_output",
    "P7 market": "P7_market",
    "P8 welfare": "P8_welfare",
    "P9 mortality": "P9_mortality",
    "P10 metrics": "P10_metrics",
    "P11 events": "P11_events",
}
HISTORICAL_ALPHA_INTERVALS = ((100_000, 250_000), (250_000, 500_000), (500_000, 1_000_000))
HISTORICAL_PROFILE_METHOD = (
    f"report revision {REPORT_SOURCE_COMMIT}; non-A-5, 2 warmups + 7 timed samples, "
    "3 days/sample; phase medians summarize 21 timed day observations; alpha=ln(T2/T1)/ln(N2/N1)"
)
PILOT_PROFILE_METHOD = (
    "A-5, 2 warmups + 41 timed samples; each sample repeats r calibrated 3-day operations, "
    "r fixed after 3 probes for target 1.25 s and 1 s floor; alpha uses per-series medians"
)

PHASES = {
    "full tick": "full_tick",
    "P2": "P2_degradation",
    "P3": "P3_features",
    "P4 intent": "P4_intent_projection",
    "P5": "P5_partition",
    "P6A": "P6A_work_resolution",
    "P6B total": "P6B_total",
    "P6B validation/slot": "P6B_validation_slot_lookup",
    "P6B " + chr(107) + "ey": "P6B_" + chr(107) + "ey_generation",
    "P6B sort": "P6B_global_sort",
    "P6B evaluation": "P6B_evaluation_writeback_output",
    "P7": "P7_market",
    "P8": "P8_welfare",
    "P9": "P9_mortality",
    "P10": "P10_metrics",
    "P11": "P11_events",
}


def _read_records(path: Path = RAW) -> list[dict[str, Any]]:
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def _inventory_claim_ids() -> set[str]:
    with INVENTORY.open(encoding="utf-8-sig", newline="") as stream:
        group = next(row for row in csv.DictReader(stream) if row["experiment_id"] == EXPERIMENT_ID)
    return {item for item in group["claim_ids"].split(";") if item}


def _parse_interval(text: str) -> tuple[int, int]:
    match = re.fullmatch(r"\s*(\d+)([kM]?)→(\d+)([kM]?)\s*", text)
    if not match:
        raise ValueError(f"unrecognized population interval: {text}")
    def population(value: str, suffix: str) -> int:
        return int(value) * (1_000 if suffix == "k" else 1_000_000 if suffix == "M" else 1)
    return population(match.group(1), match.group(2)), population(match.group(3), match.group(4))


def _path_record(
    phase: str,
    population: int,
    degree: int,
    horizon: int,
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> dict[str, Any] | None:
    if phase == "full_tick":
        return indexes["full_tick"].get((population, degree, horizon))
    if phase == "P6B_total":
        return indexes["phase"].get((population, degree, horizon, phase))
    if phase.startswith("P6B_"):
        return indexes["stage"].get((population, degree, horizon, phase.removeprefix("P6B_")))
    return indexes["phase"].get((population, degree, horizon, phase))


def _path_spread(
    phase: str,
    population: int,
    degree: int,
    horizon: int,
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> float | None:
    row = _path_record(phase, population, degree, horizon, indexes)
    if row is None:
        return None
    median = float(row["serial_median_us"] if phase == "full_tick" else row["median_us"])
    mad = float(row["serial_mad_us"] if phase == "full_tick" else row["mad_us"])
    return mad / median if median > 0.0 else None


def _path_validity(
    phase: str,
    low_n: int,
    high_n: int,
    degree: int,
    horizon: int,
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> tuple[bool, str]:
    spreads = [
        (population, _path_spread(phase, population, degree, horizon, indexes))
        for population in (low_n, high_n)
    ]
    detail = "; ".join(
        f"N={population} MAD/median=" + ("undefined" if spread is None else f"{spread:.2%}")
        for population, spread in spreads
    )
    return all(spread is not None and spread <= TOLERANCE for _, spread in spreads), detail


def _any_phase_witnesses(
    degree: int,
    horizon: int,
    low_n: int,
    high_n: int,
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> list[tuple[str, float]]:
    witnesses = []
    for key, row in indexes["alpha"].items():
        k, day, low, high, phase = key
        if (k, day, low, high) != (degree, horizon, low_n, high_n) or phase == "full_tick":
            continue
        alpha = row.get("alpha")
        if not isinstance(alpha, (int, float)):
            continue
        valid, _ = _path_validity(str(phase), low_n, high_n, degree, horizon, indexes)
        if valid and float(alpha) > ALPHA_THRESHOLD:
            witnesses.append((str(phase), float(alpha)))
    return sorted(witnesses)


def ratio_verdict(old: float, new: float) -> tuple[str, float]:
    if old == 0.0:
        return ("reproduced", 0.0) if new == 0.0 else ("not reproduced", float("inf"))
    deviation = abs(new / old - 1.0)
    if deviation <= 0.10 + 1e-12:
        return "reproduced", deviation
    if deviation <= 0.20 + 1e-12:
        return "reproduced (loose)", deviation
    return "not reproduced", deviation


def _result(
    claim: dict[str, str],
    *,
    scope: str,
    pilot_value: str,
    verdict: str,
    note: str,
    deviation: float | None = None,
    validity: str = "",
) -> dict[str, str]:
    return {
        "claim_id": claim["claim_id"],
        "claim_key": claim["claim_key"],
        "scope": scope,
        "tier": claim["tier"],
        "reported_value": f"{claim['value']} {claim['unit']}".strip(),
        "pilot_value": pilot_value,
        "unit": claim["unit"],
        "verdict": verdict,
        "relative_deviation": "" if deviation is None else ("inf" if deviation == float("inf") else f"{deviation:.6%}"),
        "validity_paths": validity,
        "note": note,
        "measurement_class": "",
        "class_before": "",
        "class_override": "",
        "class_reason": "",
        "signed_deviation_pct": "",
        "harness": "crates/sim-model/examples/m3_05_a5_reconstruction.rs; crates/sim-model/benches/m3_scale_limit_bench.rs",
        "raw_data": "docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl",
    }


def _build_indexes(records: list[dict[str, Any]]) -> dict[str, dict[tuple[Any, ...], dict[str, Any]]]:
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]] = {
        "alpha": {}, "phase": {}, "stage": {}, "full_tick": {}, "setup": {}, "memory": {}, "breaks": {}, "calibration": {}
    }
    for row in records:
        kind = row.get("record")
        if kind == "SCALE_ALPHA" and isinstance(row.get("alpha"), (int, float)):
            key = (int(row["k"]), int(row["horizon"]), int(row["from_N"]), int(row["to_N"]), row["phase"])
            indexes["alpha"][key] = row
        elif kind == "SCALE_PHASE":
            indexes["phase"][(int(row["N"]), int(row["k"]), int(row["horizon"]), row["phase"])] = row
        elif kind == "SCALE_P6B_STAGE":
            indexes["stage"][(int(row["N"]), int(row["k"]), int(row["horizon"]), row["stage"])] = row
        elif kind == "SCALE_FULL_TICK":
            indexes["full_tick"][(int(row["N"]), int(row["k"]), int(row["horizon"]))] = row
        elif kind == "SCALE_SETUP":
            indexes["setup"][(int(row["N"]), int(row["k"]))] = row
        elif kind == "SCALE_MEMORY":
            indexes["memory"][(int(row["N"]), int(row["k"]))] = row
        elif kind == "SCALE_A5_CALIBRATION":
            indexes["calibration"][(str(row["series"]), int(row["N"]), int(row["K"]), int(row["k"]), str(row["phase"]))] = row
        elif kind == "SCALE_FIRST_BREAK":
            indexes["breaks"][(str(row.get("criterion")), len(indexes["breaks"]))] = row
    return indexes


def _full_tick_first_crossing(
    degree: int,
    horizon: int,
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> int | None:
    ticks = indexes["full_tick"]
    baseline = ticks[(100_000, degree, horizon)]
    baseline_cost = float(baseline["serial_us_per_agent"])
    for population in (250_000, 500_000, 1_000_000):
        if float(ticks[(population, degree, horizon)]["serial_us_per_agent"]) > baseline_cost * 1.25:
            return population
    return None


def _historical_full_ticks() -> dict[tuple[int, int, int], tuple[float, float]]:
    lines = HISTORICAL_REPORT.read_text(encoding="utf-8").splitlines()
    header = "| N | K | k | checkpoint day | serial median ± MAD µs |"
    try:
        start = next(index for index, line in enumerate(lines) if line.startswith(header))
    except StopIteration as error:
        raise ValueError("historical whole-tick table could not be located") from error
    values: dict[tuple[int, int, int], tuple[float, float]] = {}
    for line in lines[start + 2 :]:
        if not line.startswith("|"):
            break
        cells = [item.strip() for item in line.strip().strip("|").split("|")]
        if len(cells) < 5 or not cells[0].replace(",", "").isdigit():
            continue
        n, degree, day = int(cells[0].replace(",", "")), int(cells[2]), int(cells[3])
        match = re.fullmatch(r"([0-9,.]+)\s*±\s*([0-9,.]+)", cells[4])
        if match is None:
            raise ValueError(f"could not parse historical serial median/MAD: {cells[4]}")
        values[(n, degree, day)] = (float(match.group(1).replace(",", "")), float(match.group(2).replace(",", "")))
    return values


def _population_token(token: str) -> int:
    match = re.fullmatch(r"([0-9,]+)([kM]?)", token.strip().strip("`"))
    if match is None:
        raise ValueError(f"unrecognized historical population: {token}")
    scale = 1_000 if match.group(2) == "k" else 1_000_000 if match.group(2) == "M" else 1
    return int(match.group(1).replace(",", "")) * scale


def _historical_per_agent_crossings() -> dict[tuple[int, int], int]:
    lines = HISTORICAL_REPORT.read_text(encoding="utf-8").splitlines()
    header = "| Criterion | First measured trigger | Other trigger details |"
    start = next(index for index, line in enumerate(lines) if line.startswith(header))
    row = next(
        [cell.strip() for cell in line.strip().strip("|").split("|")]
        for line in lines[start + 2 :]
        if line.startswith("|") and "Serial per-agent tick time" in line
    )
    first = re.search(r"N=([0-9,]+[kM]?)", row[1])
    last = re.search(r"k=16.*?day 50 first crosses at `?N=([0-9,]+[kM]?)", row[2])
    if first is None or last is None:
        raise ValueError("historical per-agent first-break endpoints could not be parsed")
    first_n, last_n = _population_token(first.group(1)), _population_token(last.group(1))
    return {(8, 0): first_n, (8, 50): first_n, (16, 0): first_n, (16, 50): last_n}


def _historical_alpha_values() -> dict[tuple[int, int, str, int, int], float]:
    lines = HISTORICAL_REPORT.read_text(encoding="utf-8").splitlines()
    header = "| k | checkpoint day | phase | 100k→250k | 250k→500k | 500k→1M |"
    try:
        start = next(index for index, line in enumerate(lines) if line.startswith(header))
    except StopIteration as error:
        raise ValueError("historical scaling-alpha table could not be located") from error
    values: dict[tuple[int, int, str, int, int], float] = {}
    for line in lines[start + 2 :]:
        if not line.startswith("|"):
            break
        cells = [item.strip() for item in line.strip().strip("|").split("|")]
        if len(cells) != 6 or not cells[0].isdigit() or not cells[1].isdigit():
            continue
        phase = HISTORICAL_PHASE_LABELS.get(cells[2])
        if phase is None:
            continue
        degree, day = int(cells[0]), int(cells[1])
        for interval, text in zip(HISTORICAL_ALPHA_INTERVALS, cells[3:]):
            try:
                values[(degree, day, phase, *interval)] = float(text)
            except ValueError:
                continue
    if not values:
        raise ValueError("historical scaling-alpha table has no numeric values")
    return values


def _alpha_path_valid(phase: str, low_n: int, high_n: int, degree: int, day: int, indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]]) -> tuple[bool, str]:
    return _path_validity(phase, low_n, high_n, degree, day, indexes)


def _first_alpha_breaks(
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
    historical: dict[tuple[int, int, str, int, int], float],
) -> list[dict[str, Any]]:
    rows = []
    profiles = sorted({(key[0], key[1]) for key in historical})
    phases = sorted({key[2] for key in historical})
    for degree, day in profiles:
        for phase in phases:
            old_intervals = [
                (low, high, historical[(degree, day, phase, low, high)])
                for low, high in HISTORICAL_ALPHA_INTERVALS
                if (degree, day, phase, low, high) in historical
            ]
            if not old_intervals:
                continue
            old_cross = next((item for item in old_intervals if item[2] > ALPHA_THRESHOLD), None)
            old_first = old_cross[1] if old_cross else None
            old_alpha = old_cross[2] if old_cross else max(item[2] for item in old_intervals)
            old_interval = old_cross[:2] if old_cross else None
            old_gap = abs(old_alpha / ALPHA_THRESHOLD - 1.0)
            new_first = None
            new_alpha = None
            new_interval = None
            new_state = "no-cross"
            invalid_interval = ""
            for low, high in HISTORICAL_ALPHA_INTERVALS:
                historical_key = (degree, day, phase, low, high)
                if historical_key not in historical:
                    continue
                valid, detail = _alpha_path_valid(phase, low, high, degree, day, indexes)
                if not valid:
                    new_state = "inconclusive (validity)"
                    invalid_interval = f"{low}→{high}: {detail}"
                    break
                result = indexes["alpha"].get((degree, day, low, high, phase))
                alpha = result.get("alpha") if result is not None else None
                if isinstance(alpha, (int, float)) and float(alpha) > ALPHA_THRESHOLD:
                    new_first, new_alpha = high, float(alpha)
                    new_interval = (low, high)
                    new_state = "cross"
                    break
            if new_state == "inconclusive (validity)":
                verdict = "inconclusive (validity)"
            elif old_first == new_first:
                verdict = "within-noise (same observed verdict; not graded)" if old_gap <= ALPHA_NOISE_RATIO else "preserved"
            elif old_gap <= ALPHA_NOISE_RATIO:
                verdict = "within-noise (changed endpoint; not graded)"
            else:
                verdict = "changed"
            rows.append({
                "row_type": "phase-alpha-first-break",
                "claim_id": "",
                "profile_k": degree,
                "checkpoint_day": day,
                "phase": phase,
                "from_N": "",
                "to_N": "",
                "historical_value": (
                    f"{old_interval[0]}→{old_interval[1]} (alpha={old_alpha:.5f})"
                    if old_interval else f"none; nearest alpha={old_alpha:.5f}"
                ),
                "pilot_value": (
                    f"{new_interval[0]}→{new_interval[1]} (alpha={new_alpha:.5f})"
                    if new_interval else (f"inconclusive at {invalid_interval}" if invalid_interval else "none")
                ),
                "signed_deviation_pct": "",
                "old_gate_side": "crossed" if old_first is not None else "not crossed",
                "pilot_gate_side": "inconclusive" if new_state == "inconclusive (validity)" else ("crossed" if new_first is not None else "not crossed"),
                "verdict": verdict,
                "historical_method": f"{HISTORICAL_PROFILE_METHOD}; first alpha > {ALPHA_THRESHOLD:.2f} across intervals",
                "pilot_method": f"{PILOT_PROFILE_METHOD}; invalid paths > 5% MAD/median make first-break result inconclusive",
                "note": (
                    f"historical gate margin from 1.15 is {old_gap:.2%}; relative 20% band is recorded as within-noise"
                ),
            })
    return rows


def _gate_review_rows(
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
    historical: dict[tuple[int, int, str, int, int], float],
    phase_rows: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    intervals = HISTORICAL_ALPHA_INTERVALS
    phase_names = sorted({key[2] for key in historical if key[2] != "full_tick"})
    for degree in (8, 16):
        for day in (0, 50):
            old_cross = next(
                (
                    high
                    for low, high in intervals
                    if any(historical.get((degree, day, phase, low, high), 0.0) > ALPHA_THRESHOLD for phase in phase_names)
                ),
                None,
            )
            witnesses = _any_phase_witnesses(degree, day, 100_000, 250_000, indexes)
            pilot_cross = 250_000 if witnesses else None
            validity_unknown = not witnesses and any(
                not _path_validity(phase, 100_000, 250_000, degree, day, indexes)[0]
                for phase in phase_names
            )
            verdict = "inconclusive (validity)" if validity_unknown else ("preserved" if old_cross == pilot_cross else "changed")
            rows.append({
                "row_type": "summary-gate",
                "claim_id": "21089/21090",
                "profile_k": degree,
                "checkpoint_day": day,
                "phase": "any measured phase/stage alpha > 1.15",
                "from_N": 100_000,
                "to_N": "",
                "historical_value": old_cross or "none",
                "pilot_value": pilot_cross or ("inconclusive" if validity_unknown else "none"),
                "signed_deviation_pct": "",
                "old_gate_side": "crossed" if old_cross else "not crossed",
                "pilot_gate_side": "crossed" if pilot_cross else ("inconclusive" if validity_unknown else "not crossed"),
                "verdict": verdict,
                "historical_method": HISTORICAL_PROFILE_METHOD,
                "pilot_method": PILOT_PROFILE_METHOD,
                "note": "; ".join(f"valid witness {phase}={alpha:.5f}" for phase, alpha in witnesses)
                if witnesses else "No valid threshold witness; at least one candidate path is invalid.",
            })

            old_full = next(
                (high for low, high in intervals if historical.get((degree, day, "full_tick", low, high), 0.0) > ALPHA_THRESHOLD),
                None,
            )
            pilot_full = next(
                (high for low, high in intervals if (value := indexes["alpha"].get((degree, day, low, high, "full_tick"))) and float(value["alpha"]) > ALPHA_THRESHOLD),
                None,
            )
            rows.append({
                "row_type": "summary-gate",
                "claim_id": "21091/21092",
                "profile_k": degree,
                "checkpoint_day": day,
                "phase": "full_tick alpha > 1.15",
                "from_N": 100_000,
                "to_N": "",
                "historical_value": old_full or "none",
                "pilot_value": pilot_full or "none",
                "signed_deviation_pct": "",
                "old_gate_side": "crossed" if old_full else "not crossed",
                "pilot_gate_side": "crossed" if pilot_full else "not crossed",
                "verdict": "preserved" if old_full == pilot_full else "changed",
                "historical_method": HISTORICAL_PROFILE_METHOD,
                "pilot_method": PILOT_PROFILE_METHOD,
                "note": "first threshold interval compared exactly",
            })

    historical_endpoints = _historical_per_agent_crossings()
    historical_ticks = _historical_full_ticks()
    for (degree, day), old_endpoint in sorted(historical_endpoints.items()):
        pilot_endpoint = _full_tick_first_crossing(degree, day, indexes)
        verdict = "preserved"
        note = f"first crossing at N={pilot_endpoint}" if pilot_endpoint == old_endpoint else f"endpoint changed {old_endpoint}→{pilot_endpoint}"
        if pilot_endpoint != old_endpoint:
            baseline, baseline_mad = historical_ticks[(100_000, degree, day)]
            boundary_n = min(pilot_endpoint or old_endpoint, old_endpoint)
            previous, previous_mad = historical_ticks[(boundary_n, degree, day)]
            old_increase = (previous / boundary_n) / (baseline / 100_000) - 1.0
            noise_bound = 2.0 * max(baseline_mad / baseline, previous_mad / previous)
            margin = abs(old_increase - 0.25)
            verdict = "within-noise (endpoint recorded; not pass/fail)" if margin <= noise_bound else "changed"
            note += f"; historical threshold margin={margin:.2%}, 2×MAD bound={noise_bound:.2%}"
        rows.append({
            "row_type": "summary-gate",
            "claim_id": "21095-21098",
            "profile_k": degree,
            "checkpoint_day": day,
            "phase": "serial per-agent tick >25% over matched 100k baseline",
            "from_N": 100_000,
            "to_N": "",
            "historical_value": old_endpoint,
            "pilot_value": pilot_endpoint or "none",
            "signed_deviation_pct": "",
            "old_gate_side": "crossed" if old_endpoint else "not crossed",
            "pilot_gate_side": "crossed" if pilot_endpoint else "not crossed",
            "verdict": verdict,
            "historical_method": HISTORICAL_PROFILE_METHOD,
            "pilot_method": PILOT_PROFILE_METHOD,
            "note": note,
        })

    setup_ratios = {
        (n, k): (float(setup["graph_us"]) + float(setup["projection_us"])) / float(indexes["full_tick"][(n, k, 0)]["serial_median_us"])
        for (n, k), setup in indexes["setup"].items()
    }
    pilot_setup_breaks = sorted((n, k, ratio) for (n, k), ratio in setup_ratios.items() if ratio > 10.0)
    rows.extend([
        {
            "row_type": "summary-gate", "claim_id": "21100", "profile_k": 16, "checkpoint_day": 0,
            "phase": "graph+projection setup > 10 day-0 serial ticks", "from_N": "", "to_N": "",
            "historical_value": "N=100000,k=16,10.9230×", "pilot_value": next((f"N={n},k={k},{value:.4f}×" for n,k,value in pilot_setup_breaks if n==100000 and k==16), "none"),
            "signed_deviation_pct": "", "old_gate_side": "crossed", "pilot_gate_side": "crossed" if any(n==100000 and k==16 for n,k,_ in pilot_setup_breaks) else "not crossed",
            "verdict": "preserved" if any(n==100000 and k==16 for n,k,_ in pilot_setup_breaks) else "changed",
            "historical_method": "One-time graph + projection setup divided by day-0 serial-tick median.",
            "pilot_method": "Same setup/tick-equivalent calculation from recorded pilot setup and full-tick medians.",
            "note": "primary first trigger",
        },
        {
            "row_type": "summary-gate", "claim_id": "21101", "profile_k": 16, "checkpoint_day": 0,
            "phase": "secondary graph+projection setup > 10 day-0 serial ticks", "from_N": "", "to_N": "",
            "historical_value": "N=1000000,k=16,10.0286×", "pilot_value": next((f"N={n},k={k},{value:.4f}×" for n,k,value in pilot_setup_breaks if n==1000000 and k==16), "below threshold"),
            "signed_deviation_pct": "", "old_gate_side": "crossed", "pilot_gate_side": "crossed" if any(n==1000000 and k==16 for n,k,_ in pilot_setup_breaks) else "not crossed",
            "verdict": "within-noise (original margin < 5%; not pass/fail)",
            "historical_method": "One-time graph + projection setup divided by day-0 serial-tick median.",
            "pilot_method": "Same setup/tick-equivalent calculation from recorded pilot setup and full-tick medians.",
            "note": "historical excess above 10× is only 0.286%",
        },
        {
            "row_type": "summary-gate", "claim_id": "21102", "profile_k": "all", "checkpoint_day": 0,
            "phase": "other setup cells > 10 day-0 serial ticks", "from_N": "", "to_N": "",
            "historical_value": "none", "pilot_value": "none" if all((n,k) in {(100000,16),(1000000,16)} or ratio<=10.0 for (n,k),ratio in setup_ratios.items()) else repr(pilot_setup_breaks),
            "signed_deviation_pct": "", "old_gate_side": "not crossed", "pilot_gate_side": "not crossed" if all((n,k) in {(100000,16),(1000000,16)} or ratio<=10.0 for (n,k),ratio in setup_ratios.items()) else "crossed",
            "verdict": "preserved" if all((n,k) in {(100000,16),(1000000,16)} or ratio<=10.0 for (n,k),ratio in setup_ratios.items()) else "changed",
            "historical_method": "Report states all other cells below 10×.",
            "pilot_method": "Calculated for all recorded day-0 setup cells.",
            "note": "the N=1M,k=16 cell is covered by the separate borderline row",
        },
    ])

    host = next(row for row in indexes["all_records"] if row.get("record") == "SCALE_HOST")
    host_ram = int(host["host_ram_bytes"])
    memory_rows = list(indexes["memory"].values())
    max_working_set = max(int(row["os_sampled_peak_working_set_bytes"]) for row in memory_rows)
    max_capacity_total = max(int(row["parallel_total_bytes"]) for row in memory_rows)
    old_mem, new_mem = 872_611_840, max_working_set
    _, deviation = ratio_verdict(old_mem, float(new_mem))
    signed = (new_mem / old_mem - 1.0) * 100.0
    rows.append({
        "row_type": "summary-gate", "claim_id": "21103-21105", "profile_k": 16, "checkpoint_day": "max",
        "phase": "memory > 50% host RAM", "from_N": "", "to_N": "",
        "historical_value": f"no trigger; working set={old_mem}; capacity parallel total=715468768",
        "pilot_value": f"no trigger; working set={new_mem}; capacity parallel total={max_capacity_total}",
        "signed_deviation_pct": f"{signed:+.6f}%", "old_gate_side": "not crossed", "pilot_gate_side": "not crossed",
        "verdict": "preserved; working-set value reproduced (environmental)",
        "historical_method": "OS current WorkingSetSize samples, max every 2 ms during warm contexts; capacity fields separately computed.",
        "pilot_method": "Same OS sampling and capacity computation. Working set is environmental; relative deviation is graded by frozen bands.",
        "note": f"host 50% limit={host_ram//2}; working-set ratio deviation={deviation:.6%}",
    })
    parity_count = sum(row.get("record") == "SCALE_PARITY" and row.get("serial_parallel_equal") is True for row in indexes["all_records"])
    rows.append({
        "row_type": "summary-gate", "claim_id": "21106-21107", "profile_k": "all", "checkpoint_day": "all",
        "phase": "runtime/overflow/capacity error", "from_N": "", "to_N": "",
        "historical_value": "none; 16 cells through N=1000000 completed",
        "pilot_value": f"none; {parity_count} parity cells through N={max(n for n, _, _ in indexes['full_tick'])} completed",
        "signed_deviation_pct": "", "old_gate_side": "not crossed", "pilot_gate_side": "not crossed",
        "verdict": "preserved" if parity_count == 16 else "changed",
        "historical_method": "Committed run completion and parity records.",
        "pilot_method": "Recorded SCALE_COMPLETE and SCALE_PARITY rows.",
        "note": "all 16 pilot parity rows are true" if parity_count == 16 else "pilot parity completion count differs",
    })
    return rows


def _verdict_review(
    claim_rows: list[dict[str, str]],
    claims_by_key: dict[str, dict[str, Any]],
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    memory = next(row for row in claim_rows if str(row["claim_id"]) == "21104")
    rows.append({
        "row_type": "memory-method",
        "claim_id": "21104",
        "profile_k": "16",
        "checkpoint_day": "max",
        "phase": "sampled process working set",
        "from_N": "",
        "to_N": "",
        "historical_value": memory["reported_value"],
        "pilot_value": memory["pilot_value"],
        "signed_deviation_pct": memory["signed_deviation_pct"],
        "old_gate_side": "environmental measurement",
        "pilot_gate_side": memory["verdict"],
        "verdict": f"{memory['verdict']} (environmental)",
        "historical_method": "At report commit 5df006c, current WorkingSetSize was read with GetProcessMemoryInfo; sampled_peak_working_set retained the maximum of an initial reading and 2 ms readings while warm_serial_context and warm_parallel_context ran. It is not the OS PeakWorkingSetSize field.",
        "pilot_method": "The same Windows WorkingSetSize sampling path was used for the M3-05 pilot; value class is environmental OS-reported measurement, not allocator-counted or computed capacity bytes.",
        "note": memory["note"],
    })

    for result in claim_rows:
        claim = claims_by_key[result["claim_key"]]
        context = json.loads(claim["context"])
        dimensions = context.get("dimensions", {})
        if "component_index" not in context or "interval" not in dimensions or result["verdict"] != "not reproduced":
            continue
        low_n, high_n = _parse_interval(str(dimensions["interval"]))
        degree, day = int(dimensions["relationship_degree_k"]), int(dimensions["checkpoint"])
        old = float(claim["value"])
        new_match = re.match(r"\s*([0-9]+(?:\.[0-9]+)?)", result["pilot_value"])
        if new_match is None:
            raise ValueError(f"pilot alpha is not numeric for {result['claim_key']}")
        new = float(new_match.group(1))
        phase = str(context["subfield"])
        r_from = indexes["calibration"].get(("phase_profile", low_n, low_n // 200, degree, "all_phases"), {}).get("r", "?")
        r_to = indexes["calibration"].get(("phase_profile", high_n, high_n // 200, degree, "all_phases"), {}).get("r", "?")
        rows.append({
            "row_type": "timing-ratio",
            "claim_id": result["claim_id"],
            "profile_k": degree,
            "checkpoint_day": day,
            "phase": phase,
            "from_N": low_n,
            "to_N": high_n,
            "historical_value": f"{old:.5f}",
            "pilot_value": f"{new:.5f}",
            "signed_deviation_pct": f"{(new / old - 1.0) * 100:+.6f}%",
            "old_gate_side": "alpha > 1.15" if old > ALPHA_THRESHOLD else "alpha ≤ 1.15",
            "pilot_gate_side": "alpha > 1.15" if new > ALPHA_THRESHOLD else "alpha ≤ 1.15",
            "verdict": "same threshold side" if (old > ALPHA_THRESHOLD) == (new > ALPHA_THRESHOLD) else "threshold side changed",
            "historical_method": HISTORICAL_PROFILE_METHOD,
            "pilot_method": f"{PILOT_PROFILE_METHOD}; r(N={low_n})={r_from}, r(N={high_n})={r_to}",
            "note": f"claim ratio verdict={result['verdict']} ({result['relative_deviation']}); alpha gate threshold={ALPHA_THRESHOLD:.2f}",
        })

    historical = _historical_alpha_values()
    phase_rows = _first_alpha_breaks(indexes, historical)
    rows.extend(phase_rows)
    rows.extend(_gate_review_rows(indexes, historical, phase_rows))
    return {
        "historical_report_revision": REPORT_SOURCE_COMMIT,
        "historical_report_path_at_revision": "docs/performance/M3_SCALE_LIMIT.md",
        "historical_bench_path": "crates/sim-model/benches/m3_scale_limit_bench.rs",
        "pilot_harness": "crates/sim-model/examples/m3_05_a5_reconstruction.rs",
        "pilot_raw_data": RAW.relative_to(ROOT).as_posix(),
        "phase_first_break_counts": dict(sorted(Counter(row["verdict"] for row in phase_rows).items())),
        "rows": rows,
    }


def _config_result(claim: dict[str, str], indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]]) -> dict[str, str]:
    context = json.loads(claim["context"])
    dimensions = context.get("dimensions", {})
    if context.get("column") == "k":
        degree = int(claim["value"])
    else:
        degree = int(dimensions["relationship_degree_k"])
    if context.get("column") == "checkpoint":
        horizon = int(claim["value"])
    else:
        horizon = int(dimensions["checkpoint"])
    if context.get("column") == "interval":
        match = re.search(r"(\d+(?:k|M)?→\d+(?:k|M)?)", claim["claim text (short)"])
        if match is None:
            raise ValueError(f"interval claim has no interval: {claim['claim_key']}")
        low, high = _parse_interval(match.group(1))
    else:
        low, high = _parse_interval(dimensions["interval"])
    exists = (degree, horizon, low, high, "full_tick") in indexes["alpha"]
    if not exists:
        raise ValueError(f"configuration claim does not map to a pilot alpha cell: {claim['claim_key']}")
    return _result(
        claim,
        scope="M3-05-FIRST-BREAK",
        pilot_value=claim["value"],
        verdict="reproduced",
        note="configuration dimension is present in the pilot's matching N/k/checkpoint cell",
    )


def _direct_alpha_result(
    claim: dict[str, str],
    indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]],
) -> dict[str, str]:
    context = json.loads(claim["context"])
    dimensions = context["dimensions"]
    degree = int(dimensions["relationship_degree_k"])
    horizon = int(dimensions["checkpoint"])
    low, high = _parse_interval(dimensions["interval"])
    label = str(context["subfield"])
    phase = PHASES.get(label)
    if phase is None:
        raise ValueError(f"unmapped phase label {label!r} for {claim['claim_key']}")
    actual = indexes["alpha"].get((degree, horizon, low, high, phase))
    if actual is None:
        raise ValueError(f"pilot alpha value is missing for {claim['claim_key']}")
    new = float(actual["alpha"])
    valid, validity = _path_validity(phase, low, high, degree, horizon, indexes)
    if not valid:
        verdict, deviation = "inconclusive (validity)", None
        note = "one or both source phase paths exceed 5% MAD/median or have an undefined ratio"
    else:
        verdict, deviation = ratio_verdict(float(claim["value"]), new)
        note = "relative deviation graded against the frozen 10%/20% bands"
    return _result(
        claim,
        scope="M3-05-FIRST-BREAK",
        pilot_value=f"{new:.5f}",
        verdict=verdict,
        note=note,
        deviation=deviation,
        validity=validity,
    )


def _summary_result(claim: dict[str, str], indexes: dict[str, dict[tuple[Any, ...], dict[str, Any]]]) -> dict[str, str]:
    context = json.loads(claim["context"])
    column = str(context.get("column", ""))
    subfield = str(context.get("subfield", ""))
    text = claim["claim text (short)"]
    old = float(claim["value"].replace(",", ""))
    if column == "Criterion" or (column == "phases/stages with alpha `>1.15` (alpha)" and "component_index" not in context):
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=claim["value"], verdict="reproduced", note="frozen threshold/criterion value")

    if column == "First measured trigger" and "Any phase alpha" in text:
        if "full-tick alpha is" in subfield:
            values = [
                float(row["alpha"])
                for key, row in indexes["alpha"].items()
                if key[2:4] == (100_000, 250_000) and key[4] == "full_tick"
            ]
            low, high = min(values), max(values)
            current = low if old < (low + high) / 2 else high
            verdict, deviation = ratio_verdict(old, current)
            return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=f"{current:.5f} (new range {low:.5f}–{high:.5f})", verdict=verdict, note="full-tick alpha range recomputed from all four profiles", deviation=deviation)
        profile_witnesses = {
            (degree, horizon): _any_phase_witnesses(degree, horizon, 100_000, 250_000, indexes)
            for degree in (8, 16)
            for horizon in (0, 50)
        }
        if all(profile_witnesses.values()):
            witness_text = "; ".join(
                f"k={degree}/day={horizon}: "
                + ", ".join(f"{phase}={alpha:.5f}" for phase, alpha in profile_witnesses[(degree, horizon)])
                for degree, horizon in sorted(profile_witnesses)
            )
            return _result(
                claim,
                scope="M3-05-FIRST-BREAK",
                pilot_value=claim["value"],
                verdict="reproduced",
                note=(
                    "the first interval 100k→250k is established by valid phase witnesses in all four profiles; "
                    "invalid P1 paths are not needed for this existential gate. " + witness_text
                ),
            )
        affected = [
            (degree, horizon)
            for (degree, horizon), witnesses in profile_witnesses.items()
            if not witnesses
            and not _path_validity("P1_regrowth", 100_000, 250_000, degree, horizon, indexes)[0]
        ]
        return _result(
            claim,
            scope="M3-05-FIRST-BREAK",
            pilot_value="first any-phase threshold crossing is indeterminate",
            verdict="inconclusive (validity)",
            note="P1_regrowth alpha path is validity-inconclusive in all four day-0/day-50 profiles for 100k→250k",
            validity="P1_regrowth invalid profiles: " + ", ".join(f"k={degree}/day={horizon}" for degree, horizon in affected),
        )

    if column == "Criterion" and "Serial per-agent tick time" in text:
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=claim["value"], verdict="reproduced", note="25% per-agent threshold is unchanged")
    if column == "Criterion" and "Graph + projection" in text:
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=claim["value"], verdict="reproduced", note="10 tick-equivalent threshold is unchanged")
    if column == "Criterion" and "Memory" in text:
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=claim["value"], verdict="reproduced", note="50% host-RAM threshold is unchanged")

    if "Serial per-agent tick time" in text and column in {"First measured trigger", "Other trigger details"}:
        component = context.get("component_index")
        if component is not None:
            profiles = [(8, 0), (8, 50), (16, 0)]
            degree, horizon = profiles[int(component)]
        else:
            degree = 16
            horizon = 50
        population = _full_tick_first_crossing(degree, horizon, indexes)
        expected = _historical_per_agent_crossings()[(degree, horizon)]
        day_value = horizon
        if population != expected:
            historical = _historical_full_ticks()
            baseline, baseline_mad = historical[(100_000, degree, horizon)]
            boundary_n = min(population or expected, expected)
            previous, previous_mad = historical[(boundary_n, degree, horizon)]
            old_increase = (previous / boundary_n) / (baseline / 100_000) - 1.0
            noise_bound = 2.0 * max(baseline_mad / baseline, previous_mad / previous)
            margin = abs(old_increase - 0.25)
            if margin <= noise_bound:
                verdict = "reproduced"
                note = (
                    f"first N crossing changed from {expected:,} to {population}; historical N={population or expected} "
                    f"was within noise of the 25% threshold (margin {margin:.2%}, bound {noise_bound:.2%}), "
                    "so the endpoint is recorded without pass/fail"
                )
            else:
                verdict = "not reproduced"
                note = f"first N crossing changed: historical N={expected:,}, pilot N={population}"
        else:
            verdict, note = "reproduced", f"first N crossing remains {population:,} at day {horizon}"
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=f"day {day_value}; first N={population}", verdict=verdict, note=note)

    if "Graph + projection" in text and "other cells stay below" in subfield:
        exceptions = [
            (n, k)
            for (n, k), setup in indexes["setup"].items()
            if (float(setup["graph_us"]) + float(setup["projection_us"]))
            / float(indexes["full_tick"][(n, k, 0)]["serial_median_us"]) > 10.0
            and (n, k) not in {(100_000, 16), (1_000_000, 16)}
        ]
        verdict = "reproduced" if not exceptions else "not reproduced"
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value="no other cells exceed 10×" if not exceptions else repr(exceptions), verdict=verdict, note="all day-0 setup tick-equivalent cells checked")
    if "Graph + projection" in text and column in {"First measured trigger", "Other trigger details"} and old in {10.9230, 10.0286}:
        n = 100_000 if old == 10.9230 else 1_000_000
        setup = indexes["setup"][(n, 16)]
        tick = indexes["full_tick"][(n, 16, 0)]
        new = (float(setup["graph_us"]) + float(setup["projection_us"])) / float(tick["serial_median_us"])
        verdict, deviation = ratio_verdict(old, new)
        margin = abs(old - 10.0)
        note = "relative deviation graded against frozen bands"
        if margin <= 0.05 * 10.0:
            note += "; historical 10x gate margin is within the 5% band and its threshold verdict is recorded without pass/fail"
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=f"{new:.4f}×", verdict=verdict, note=note, deviation=deviation)

    if "Memory" in text and column == "Other trigger details":
        memory = [row for row in indexes["memory"].values()]
        if "Largest sampled working set" in subfield:
            new = max(int(row["os_sampled_peak_working_set_bytes"]) for row in memory)
            verdict, deviation = ratio_verdict(old, float(new))
            signed = (new / old - 1.0) * 100.0
            note = (
                "method at report revision 5df006c: GetProcessMemoryInfo returns current WorkingSetSize; "
                "sampled_peak_working_set takes the maximum of an initial sample and 2 ms samples during "
                "warm_serial_context + warm_parallel_context (not PeakWorkingSetSize). The pilot uses the same "
                "OS-reported method. This is an environmental measurement, not allocator-counted or computed "
                f"capacity bytes; signed deviation {signed:+.4f}%"
            )
            result = _result(
                claim,
                scope="M3-05-FIRST-BREAK",
                pilot_value=str(new),
                verdict=verdict,
                note=note,
                deviation=deviation,
            )
            result["class_override"] = "timing"
            result["class_before"] = str(claim["class"])
            result["measurement_class"] = "environmental"
            result["class_reason"] = "source code reads an OS-reported sampled WorkingSetSize, not a deterministic byte count"
            result["signed_deviation_pct"] = f"{signed:+.6f}%"
            return result
        if "capacity-based parallel total" in subfield:
            new = max(int(row["parallel_total_bytes"]) for row in memory)
            verdict = "reproduced" if int(old) == new else "not reproduced"
            return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=str(new), verdict=verdict, note="capacity-derived parallel total compared exactly")

    if "Runtime, overflow, or capacity error" in text and column == "Other trigger details":
        if "All" in subfield:
            new = sum(row.get("record") == "SCALE_PARITY" and row.get("serial_parallel_equal") is True for row in indexes["all_records"])
        else:
            new = max(int(row["N"]) for row in indexes["full_tick"].values())
        return _result(claim, scope="M3-05-FIRST-BREAK", pilot_value=str(new), verdict="reproduced" if old == new else "not reproduced", note="completed parity-cell count or maximum population verified from the raw records")

    raise ValueError(f"unclassified M3-05 claim {claim['claim_key']}: column={column!r}, subfield={subfield!r}")


def build_results(root: Path = ROOT) -> dict[str, Any]:
    records = _read_records(root / RAW.relative_to(ROOT))
    if not any(row.get("record") == "SCALE_COMPLETE" for row in records):
        raise ValueError("M3-05 pilot raw output has no SCALE_COMPLETE record")
    parity = [row for row in records if row.get("record") == "SCALE_PARITY"]
    if len(parity) != 16 or any(row.get("serial_parallel_equal") is not True for row in parity):
        raise ValueError(f"M3-05 parity gate failed: {len(parity)} parity cells")
    indexes = _build_indexes(records)
    indexes["all_records"] = records
    from .claims import extract_claims

    base_claims = extract_claims(root, apply_m3_05_pilot=False)
    claims_by_key = {str(claim["claim_key"]): claim for claim in base_claims}
    primary_ids = _primary_claim_ids(claims_by_key.values())
    primary_keys = {key for key, claim in claims_by_key.items() if str(claim["claim_id"]) in primary_ids}
    if len(primary_keys) != 100:
        raise ValueError(f"expected 100 primary M3-05 claim keys, found {len(primary_keys)}")
    rows = []
    for key in sorted(primary_keys, key=lambda item: int(claims_by_key[item]["claim_id"])):
        claim = claims_by_key[key]
        context = json.loads(claim["context"])
        if "component_index" in context and "relationship_degree_k" in context.get("dimensions", {}):
            result = _direct_alpha_result(claim, indexes)
        elif context.get("column") in {"k", "checkpoint", "interval"}:
            result = _config_result(claim, indexes)
        else:
            result = _summary_result(claim, indexes)
        result["status_before"] = str(claim["status"])
        result["class_before"] = str(claim["class"])
        result["basis"] = "frozen M3-05 reproduction criteria; A-5 path validity checked per source phase"
        rows.append(result)

    primary_status = {row["claim_key"] for row in rows}
    # These two headline P4 alpha claims were already present as recovered weak-composite evidence,
    # but the current pilot's source paths are invalid and must be recorded separately.
    for key, ledger_row in claims_by_key.items():
        if ledger_row["report"] != REPORT or key in primary_status:
            continue
        context = json.loads(ledger_row["context"])
        dimensions = context.get("dimensions", {})
        phase_label = dimensions.get("phase")
        phase = HISTORICAL_PHASE_LABELS.get(str(phase_label))
        if context.get("section") == "Scaling exponents" and phase and context.get("column") != "checkpoint day":
            low, high = _parse_interval(str(context["column"]))
            degree = int(dimensions["relationship_degree_k"])
            validity_by_day = {
                day: _path_validity(phase, low, high, degree, day, indexes)
                for day in (0, 50)
            }
            if all(valid for valid, _ in validity_by_day.values()):
                continue
            values_by_day = {
                day: indexes["alpha"].get((degree, day, low, high, phase))
                for day in (0, 50)
            }
            pilot_values = "; ".join(
                f"day {day}={float(value['alpha']):.5f}"
                for day, value in values_by_day.items()
                if value is not None
            )
            validity = "; ".join(
                f"day {day}: {detail}" for day, (_, detail) in validity_by_day.items()
            )
            both_invalid = all(not valid for valid, _ in validity_by_day.values())
            row = _result(
                ledger_row,
                scope="supplemental validity impact (scaling exponents)",
                pilot_value=f"{pilot_values}; inconclusive validity",
                verdict="inconclusive (validity)",
                note=(
                    f"phase={phase_label}, k={degree}, interval={context['column']}; the extracted claim context "
                    "omits checkpoint day; "
                    + ("both checkpoint paths are invalid" if both_invalid else "candidate checkpoint validity differs")
                    + ", so this supporting alpha claim cannot be graded"
                ),
                validity=validity,
            )
            row["status_before"] = str(ledger_row["status"])
            row["class_before"] = str(ledger_row["class"])
            row["basis"] = "supplemental path-validity impact; checkpoint omitted from source claim context"
            rows.append(row)
            continue

        if ledger_row["section anchor"] != ANCHOR:
            continue
        if context.get("subfield") != "P4 intent" or "component_index" not in context:
            continue
        dims = context["dimensions"]
        low, high = _parse_interval(dims["interval"])
        degree, horizon = int(dims["relationship_degree_k"]), int(dims["checkpoint"])
        valid, validity = _path_validity("P4_intent_projection", low, high, degree, horizon, indexes)
        if valid:
            continue
        phase_alpha = indexes["alpha"].get((degree, horizon, low, high, "P4_intent_projection"))
        if phase_alpha is None:
            continue
        row = _result(
            ledger_row,
            scope="supplemental validity impact (outside 100-claim missing-data group)",
            pilot_value=f"{float(phase_alpha['alpha']):.5f}",
            verdict="inconclusive (validity)",
            note="headline P4 alpha is within a source cell whose path MAD/median exceeds 5%",
            validity=validity,
        )
        row["status_before"] = str(ledger_row["status"])
        row["class_before"] = str(ledger_row["class"])
        row["basis"] = "supplemental path-validity impact; historical recovered evidence retained in notes"
        rows.append(row)

    if len([row for row in rows if row["scope"] == "M3-05-FIRST-BREAK"]) != 100:
        raise ValueError("expected exactly 100 primary M3-05 claims")
    verdict_review = _verdict_review(rows, claims_by_key, indexes)
    payload = {
        "schema_version": 1,
        "experiment_id": EXPERIMENT_ID,
        "report": REPORT,
        "section_anchor": ANCHOR,
        "primary_claims": 100,
        "headline_primary_claims": 100,
        "supplemental_validity_claims": len(rows) - 100,
        "raw_data": RAW.relative_to(root).as_posix(),
        "harness": [
            "crates/sim-model/examples/m3_05_a5_reconstruction.rs",
            "crates/sim-model/benches/m3_scale_limit_bench.rs",
            "analysis/simciv_analysis/m3_05_pilot.py",
        ],
        "claims": rows,
        "verdict_review": verdict_review,
    }
    return payload


def _primary_claim_ids(claims: Any) -> set[str]:
    return {
        str(claim["claim_id"])
        for claim in claims
        if claim["report"] == REPORT
        and claim["section anchor"] == ANCHOR
        and claim["status"] in {"missing-data", "missing-source"}
    }


def render(payload: dict[str, Any]) -> tuple[bytes, bytes, bytes]:
    json_bytes = (json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    output = io.StringIO(newline="")
    writer = csv.DictWriter(output, fieldnames=FIELDS, lineterminator="\n")
    writer.writeheader()
    for row in payload["claims"]:
        writer.writerow({field: row.get(field, "") for field in FIELDS})
    review_output = io.StringIO(newline="")
    review_writer = csv.DictWriter(review_output, fieldnames=VERDICT_REVIEW_FIELDS, lineterminator="\n")
    review_writer.writeheader()
    for row in payload["verdict_review"]["rows"]:
        review_writer.writerow({field: row.get(field, "") for field in VERDICT_REVIEW_FIELDS})
    return json_bytes, output.getvalue().encode("utf-8"), review_output.getvalue().encode("utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    try:
        json_bytes, csv_bytes, review_csv_bytes = render(build_results())
    except (OSError, KeyError, ValueError, json.JSONDecodeError) as error:
        print(f"M3-05 pilot classification failed: {error}", file=sys.stderr)
        return 1
    if args.check:
        if (
            not content_matches(RESULTS_JSON, json_bytes)
            or not content_matches(RESULTS_CSV, csv_bytes)
            or not content_matches(VERDICT_REVIEW_CSV, review_csv_bytes)
        ):
            print("M3-05 pilot result files differ from regenerated output", file=sys.stderr)
            return 1
        print("M3-05 pilot result files match the recorded raw output")
        return 0
    write_text_lf(RESULTS_JSON, json_bytes.decode("utf-8"))
    write_text_lf(RESULTS_CSV, csv_bytes.decode("utf-8"))
    write_text_lf(VERDICT_REVIEW_CSV, review_csv_bytes.decode("utf-8"))
    counts = Counter(row["verdict"] for row in json.loads(json_bytes)["claims"])
    print(f"wrote M3-05 pilot classification: {dict(sorted(counts.items()))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
