"""Generate the M5-05 conclusion-level regeneration report from Git summaries."""

from __future__ import annotations

import argparse
import json
import math
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import pandas as pd

from .figures import configure, svg_bytes
from .m5_05 import _registry
from .provenance import ROOT
from .tables import dataframe_to_markdown

configure()

import matplotlib.pyplot as plt


RESULTS_PATH = ROOT / "docs" / "data" / "m5_05" / "results.json"
QMD_PATH = ROOT / "docs" / "reproducibility" / "M5_05_M0_M3_REGENERATION.qmd"
MD_PATH = ROOT / "docs" / "reproducibility" / "M5_05_M0_M3_REGENERATION.md"
FIGURE_PATH = ROOT / "docs" / "reproducibility" / "figures" / "m5_05_old_new_timing.svg"
DETERMINISTIC_SUMMARY = ROOT / "docs" / "data" / "m5_05" / "deterministic" / "summary.json"


def _summary(entry_id: str) -> dict[str, Any] | None:
    path = ROOT / "docs" / "data" / "m5_05" / entry_id / "summary.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else None


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
        old = old_by_cell.get(cell_id) or old_by_n.get(n)
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
        for measurement in measurements:
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
                "carrying capacity": row.get("carrying_capacity"),
                "initial settlement resource": row.get("initial_settlement_resource"),
            "left_mode": row.get("left_mode"),
            "right_mode": row.get("right_mode"),
            "result": result,
            "historical": old,
            "historical_exact_match": exact_old,
            "measurements": measurements,
        })
    return output


def _status(entry_id: str, cells: list[dict[str, Any]]) -> tuple[str, str]:
    if not cells:
        return "inconclusive", "No Session 1 summary; later session not run."
    exact = [m for cell in cells for m in cell["measurements"] if m.get("ratio") is not None]
    if entry_id == "m2_final_tick":
        new_values = [m["new_us"] for cell in cells for m in cell["measurements"] if m.get("new_us") is not None]
        holds = bool(new_values) and all(17_000 <= value <= 22_000 for value in new_values)
        return ("holds" if holds else "does not hold"), "Selected 100k whole-tick values compared with the recorded 17–22 ms range."
    if entry_id == "m3_03_search_reduction":
        deltas = [cell["result"].get("paired_left_minus_right_median_us") for cell in cells]
        mads = [cell["result"].get("paired_left_minus_right_mad_us") for cell in cells]
        if all(delta is not None and mad is not None and delta <= mad for delta, mad in zip(deltas, mads, strict=True)):
            return "holds", "The selected paired savings do not exceed their paired MADs."
        return "does not hold", "At least one selected paired saving exceeded its paired MAD."
    if exact:
        return "inconclusive", "Representative cells were remeasured; the headline covers a broader population/grid than this session."
    return "inconclusive", "The selected timing values do not have an exact historical workload/mode match."


def _build() -> dict[str, Any]:
    registry = _registry()
    deterministic = json.loads(DETERMINISTIC_SUMMARY.read_text(encoding="utf-8")) if DETERMINISTIC_SUMMARY.exists() else None
    rows = []
    measurement_rows = []
    for entry in registry["conclusion"]:
        summary = _summary(entry["id"])
        cells = _entry_cells(entry, summary)
        status, note = _status(entry["id"], cells)
        raw = f"../../data/m5_05/{entry['id']}/summary.json" if summary else "not recorded"
        harness_path = str(entry.get("harness", ""))
        harness_link = f"[{Path(harness_path).name}](../../{harness_path})" if harness_path else "unmapped"
        old_text = "; ".join(
            f"{item['cell']} {key} {value:.3f} µs"
            for cell in cells if (item := cell.get("historical"))
            for key, value in item.items()
            if key.endswith("_us") and isinstance(value, (int, float))
        ) or "not exactly mapped"
        rows.append({
            "milestone": entry["milestone"],
            "conclusion": entry["id"],
            "report": Path(entry["report"]).name,
            "section_anchor": entry["section"],
            "old values": old_text,
            "new values": "; ".join(
                f"{cell['cell']} {measurement['path']} {measurement['new_us']:.3f} µs"
                for cell in cells for measurement in cell["measurements"]
                if isinstance(measurement.get("new_us"), (int, float))
            ) or "not run",
            "new/old ratios": "; ".join(
                f"{cell['cell']} {measurement['path']} {measurement['ratio']:.3f}×"
                for cell in cells for measurement in cell["measurements"]
                if measurement.get("ratio") is not None
            ) or "—",
            ">20% flag": any(
                measurement.get("flag_over_20_percent")
                for cell in cells for measurement in cell["measurements"]
            ),
            "status": status,
            "notes": note,
            "harness": harness_link,
            "measurement harness": "[m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs)",
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
                "left µs/tick": result.get("left_median_us"),
                "left MAD": result.get("left_mad_us"),
                "right µs/tick": result.get("right_median_us"),
                "right MAD": result.get("right_mad_us"),
                "speedup": result.get("speedup_left_over_right"),
                "paired delta µs": result.get("paired_left_minus_right_median_us"),
                "paired delta MAD": result.get("paired_left_minus_right_mad_us"),
                "r": result.get("repetitions_per_sample"),
                "minimum sample s": result.get("minimum_sample_seconds"),
                "harness": f"[m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs)",
                "summary": f"[summary](../../data/m5_05/{entry['id']}/summary.json)",
            })
    deterministic_results = deterministic.get("details", {}).get("results", []) if deterministic else []
    results = {
        "session": 1,
        "deterministic": deterministic_results,
        "conclusions": rows,
        "cells": measurement_rows,
        "deterministic_summary": (
            "[deterministic summary](../../data/m5_05/deterministic/summary.json)"
            if deterministic else "not recorded"
        ),
        "sessions_not_run": [2, 3],
    }
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
                    if measure.get("ratio") is not None:
                        matched.append((f"{entry['id']}\n{cell['cell']}\n{measure['path']}", measure["old_us"], measure["new_us"]))
    fig, ax = plt.subplots(figsize=(9, max(2.5, 0.35 * len(matched))))
    if matched:
        positions = list(range(len(matched)))
        ax.scatter([item[1] for item in matched], positions, label="recorded", marker="o")
        ax.scatter([item[2] for item in matched], positions, label="M5-05 Session 1", marker="x")
        ax.set_yticks(positions, [item[0] for item in matched])
        ax.legend(loc="best")
    else:
        ax.text(0.5, 0.5, "No exact old/new timing pairs recorded yet", ha="center", va="center")
        ax.set_axis_off()
    ax.set_xlabel("µs per tick")
    ax.set_title("Recorded versus M5-05 selected timings")
    fig.tight_layout()
    rendered = svg_bytes(fig)
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
    return f'''---
title: "M5-05 M0–M3 Conclusion Regeneration"
format: gfm
---

Status: in progress — Session 1 complete; Sessions 2–3 not run

## Question

Do the selected conclusion-bearing M2/M3 timing results and mapped deterministic results still support the claims in the M0–M3 reports?

## Method

The frozen conclusion set is [`m5_05_conclusions.toml`](m5_05_conclusions.toml). Each timing stage runs the committed [measurement example](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) with A-5 sampling; every timing DVC output has a Git summary and a raw output directory. This report reads only Git summaries, not DVC raw paths. Historical values are extracted from the named reports by [m5_05.py](../../analysis/simciv_analysis/m5_05.py). Session 1 cell data and timing are collected in the summaries under `docs/data/m5_05/`.

## Results

### Conclusion status

{_table(conclusions, ["milestone", "conclusion", "old values", "new values", "new/old ratios", ">20% flag", "status", "notes", "harness", "measurement harness", "summary", "source report"])}

### Session 1 selected cells

{_table(cells, ["conclusion", "cell", "N", "K", "k", "p_local", "carrying capacity", "initial settlement resource", "left µs/tick", "left MAD", "right µs/tick", "right MAD", "speedup", "paired delta µs", "paired delta MAD", "r", "minimum sample s"])}

### Deterministic confirmations

{deterministic_table}

### Timing comparison figure

![Recorded and M5-05 selected timings](figures/m5_05_old_new_timing.svg)

### Sessions not run

Session 2 and Session 3 are **not run** in this execution. Their conclusion rows remain inconclusive. This report does not substitute selected cells for the broader historical grid or its aggregate statistics.

## Findings

- Each conclusion status distinguishes direct representative-cell evidence from unsupported full-grid aggregates; see the generated status table.
- Exact selected-cell path ratios are flagged when `|ratio - 1| > 20%`; the flag does not itself decide whether the conclusion holds.
- Deterministic test targets ran without changing existing expected values; see the test result table.
- Unrun sessions remain inconclusive and are not counted as reproduced.

## Limits

- Session 1 uses representative cells; M2-02 and other historical grid aggregates remain inconclusive from samples.
- Timing is specific to the recorded reference host and A-5 sampling protocol.
- No raw DVC object is needed to run `just check`; summaries are the only report inputs.
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
    return {
        RESULTS_PATH: data["results_bytes"],
        FIGURE_PATH: data["figure"],
        QMD_PATH: data["qmd"],
    }


def render_outputs() -> None:
    outputs = _outputs()
    for path, contents in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents)
    with tempfile.TemporaryDirectory(prefix="simciv-m5-05-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        qmd.write_bytes(outputs[QMD_PATH])
        MD_PATH.write_bytes(_render_quarto(qmd, temp_dir).read_bytes())
    print(f"generated {QMD_PATH.relative_to(ROOT)} and {MD_PATH.relative_to(ROOT)}")


def check_outputs() -> bool:
    outputs = _outputs()
    missing = [str(path) for path in [*outputs, MD_PATH] if not path.exists()]
    if missing:
        print(f"missing M5-05 outputs: {', '.join(missing)}", file=sys.stderr)
        return False
    mismatched = [str(path) for path, expected in outputs.items() if path.read_bytes() != expected]
    if mismatched:
        print(f"M5-05 generated files differ: {', '.join(mismatched)}", file=sys.stderr)
        return False
    with tempfile.TemporaryDirectory(prefix="simciv-m5-05-check-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        qmd.write_bytes(outputs[QMD_PATH])
        if _render_quarto(qmd, temp_dir).read_bytes() != MD_PATH.read_bytes():
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
