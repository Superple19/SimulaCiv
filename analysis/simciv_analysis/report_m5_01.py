"""Generate the M5-01 inventory QMD and its Quarto-rendered GFM Markdown."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

import pandas as pd

from simciv_analysis.claims import OLD_LEDGER, ROOT, _old_metadata, expand_old_ledger
from simciv_analysis.io import load_data
from simciv_analysis.tables import dataframe_to_markdown


QMD_PATH = ROOT / "docs" / "reproducibility" / "M5_01_INVENTORY.qmd"
MD_PATH = ROOT / "docs" / "reproducibility" / "M5_01_INVENTORY.md"
LEDGER_PATH = ROOT / "docs" / "reproducibility" / "claims.csv.gz"
REQUIRED_CLAIM_FIELDS = ["claim_id", "report", "line", "section anchor", "claim text (short)", "value", "unit", "class", "tier", "source harness", "raw data", "status", "M5 stage", "notes"]


def _milestone(report: str) -> str:
    match = re.match(r"M(\d+)", report.upper())
    return f"M{match.group(1)}" if match else "M0"


def _markdown(frame: pd.DataFrame) -> str:
    if frame.empty:
        frame = pd.DataFrame([{"result": "none"}])
    return dataframe_to_markdown(frame.fillna(""))


def _grouped_counts(claims: pd.DataFrame) -> pd.DataFrame:
    data = claims.copy()
    data["milestone"] = data["report"].map(_milestone)
    counts = (
        data.groupby(["milestone", "class", "status", "tier"], dropna=False)
        .size()
        .rename("claims")
        .reset_index()
        .sort_values(["milestone", "class", "status", "tier"], kind="stable")
    )
    if "M0" not in set(counts["milestone"]):
        counts = pd.concat(
            [counts, pd.DataFrame([{"milestone": "M0", "class": "—", "status": "—", "tier": "—", "claims": 0}])],
            ignore_index=True,
        )
    return counts.sort_values(["milestone", "class", "status", "tier"], kind="stable").reset_index(drop=True)


def _comparison(claims: pd.DataFrame) -> tuple[pd.DataFrame, pd.DataFrame]:
    old = expand_old_ledger(OLD_LEDGER)
    new_keys = Counter((str(row.report), int(row.line), str(row.value)) for row in claims.itertuples(index=False))
    old_keys = Counter((str(row["report"]), int(row["line"]), str(row["value"])) for row in old)
    old_metadata = _old_metadata(old)
    exact_old_records = sum(count for key, count in old_keys.items() if key in new_keys)
    exact_new_records = sum(int(row.status != "unmapped") for row in claims.itertuples(index=False))
    ambiguous_new_records = sum("ambiguous duplicate" in str(note) for note in claims["notes"])
    no_key_new_records = len(claims) - exact_new_records - ambiguous_new_records
    ambiguous_old_records = sum(count for key, count in old_keys.items() if key in new_keys and old_metadata.get(key) is None)

    documents = sorted((ROOT / "docs" / "reports").glob("*.md")) + sorted((ROOT / "docs" / "contracts").glob("*.md"))
    by_name: dict[str, list[Path]] = {}
    for path in documents:
        by_name.setdefault(path.name, []).append(path)
    old_unmatched = [row for row in old if (str(row["report"]), int(row["line"]), str(row["value"])) not in new_keys]
    source_missing = source_changed = still_present_excluded = 0
    for row in old_unmatched:
        candidates = by_name.get(str(row["report"]), [])
        matching_lines = []
        for path in candidates:
            lines = path.read_text(encoding="utf-8").splitlines()
            line_number = int(row["line"])
            if 1 <= line_number <= len(lines):
                matching_lines.append(lines[line_number - 1])
        if not matching_lines:
            source_missing += 1
        elif any(_contains_value(line, str(row["value"])) for line in matching_lines):
            still_present_excluded += 1
        else:
            source_changed += 1

    old_new_rows = pd.DataFrame(
        [
            {"Ledger view": "old packed records", "rows": len(old), "exact tuple records": exact_old_records, "unmapped/excluded": len(old) - exact_old_records, "reason": "old one-row-or-range records expanded to scalar values"},
            {"Ledger view": "new tidy records", "rows": len(claims), "exact tuple records": exact_new_records, "unmapped/excluded": no_key_new_records + ambiguous_new_records, "reason": "one row per selected numeric claim; no exact provenance is inferred"},
            {"Ledger view": "new rows with conflicting old metadata", "rows": ambiguous_new_records, "exact tuple records": 0, "unmapped/excluded": ambiguous_new_records, "reason": "duplicate old report/line/value keys disagree, so provenance remains unmapped"},
        ]
    )
    reasons = pd.DataFrame(
        [
            {"old record disposition": "exact tuple present", "records": exact_old_records - ambiguous_old_records, "rule": "report, source line, and value match; old metadata is unambiguous"},
            {"old record disposition": "exact tuple has conflicting metadata", "records": ambiguous_old_records, "rule": "same report/line/value maps to conflicting old metadata; no transfer"},
            {"old record disposition": "value remains on source line but is not selected", "records": still_present_excluded, "rule": "input/config value or outside the new claim-position rule"},
            {"old record disposition": "source, line, or value no longer matches", "records": source_missing + source_changed, "rule": "no exact current source evidence; left unmapped"},
        ]
    )
    return old_new_rows, reasons


def _contains_value(line: str, value: str) -> bool:
    if value.lower() in line.lower():
        return True
    value = value.replace(",", "")
    number = re.compile(r"(?<![\w.])[+-]?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?")
    return any(match.group(0).replace(",", "") == value for match in number.finditer(line))


def _report_parts() -> dict[str, str]:
    claims = load_data("docs/reproducibility/claims.csv.gz")
    if list(claims.columns) != REQUIRED_CLAIM_FIELDS:
        raise ValueError(f"unexpected claim columns: {list(claims.columns)}")
    counts = _grouped_counts(claims)
    comparison, comparison_reasons = _comparison(claims)
    key_claims = claims.loc[claims["tier"] == "key"].copy()
    key_claims["milestone"] = key_claims["report"].map(_milestone)
    key_claims = key_claims.sort_values(["milestone", "report", "line", "claim_id"], kind="stable")
    key_claims = key_claims[["milestone", "report", "line", "section anchor", "value", "unit", "class", "status"]]
    m1 = claims.loc[(claims["report"] == "M1_CONTRACT_FREEZE.md") & (claims["status"] == "unrecoverable")].copy()
    m1 = m1.sort_values(["line", "claim_id"], kind="stable")
    m1 = m1[["claim_id", "line", "value", "class", "status", "notes"]]
    gaps = claims.loc[claims["status"] != "reproducible-now"].copy()
    gaps["milestone"] = gaps["report"].map(_milestone)
    top_gaps = (
        gaps.groupby(["milestone", "report", "status"], dropna=False)
        .size()
        .rename("claims")
        .reset_index()
        .sort_values(["claims", "report", "status"], ascending=[False, True, True], kind="stable")
        .head(10)
    )
    mapped = claims["status"] != "unmapped"
    input_rule = (
        "Excluded inputs are table columns or labeled assignments for population/workload sizes, group and neighbor counts, seeds, "
        "replicates, horizons/days, thread/chunk settings, and simulation parameter values. Free-text numbers are only extracted from "
        "tables, status/headline lines, findings bullets, and measured contract sections. The machine-readable contracts/m1_contract.toml "
        "is a configuration/contract input and is not scanned as a measured report."
    )
    return {
        "counts": _markdown(counts),
        "comparison": _markdown(comparison),
        "comparison_reasons": _markdown(comparison_reasons),
        "key_claims": _markdown(key_claims),
        "m1_recheck": _markdown(m1),
        "gaps": _markdown(top_gaps),
        "input_rule": input_rule,
        "total": f"{len(claims):,}",
        "mapped": f"{int(mapped.sum()):,}",
        "unmapped": f"{int((~mapped).sum()):,}",
        "m1_count": f"{len(m1):,}",
        "key_count": f"{len(key_claims):,}",
        "supporting_count": f"{len(claims) - len(key_claims):,}",
        "old_count": f"{len(expand_old_ledger(OLD_LEDGER)):,}",
    }


def render_qmd() -> str:
    parts = _report_parts()
    return f'''---
title: "M5-01 Reproducibility Inventory"
format: gfm
---

Status: current

## Question

Which numeric statements in the existing M0–M4 performance reports and contract evidence have a reproducible source, and which evidence gaps remain?

## Method

The committed extractor [claims.py](../../analysis/simciv_analysis/claims.py) reads the report/contract source tree. Its tidy output [claims.csv.gz](claims.csv.gz) contains **{parts['total']}** rows; the prior packed ledger [superseded/claims.csv](superseded/claims.csv) contains **{parts['old_count']}** expanded scalar values. {parts['input_rule']}

## Results

Every table below is generated from [claims.csv.gz](claims.csv.gz) by the report generator [report_m5_01.py](../../analysis/simciv_analysis/report_m5_01.py). Old-ledger comparisons additionally read the retained packed ledger.

The tidy ledger contains **{parts['key_count']}** key claims and **{parts['supporting_count']}** supporting claims.

### Counts by milestone, class, status, and tier

{parts['counts']}

### Old and tidy ledger comparison

{parts['comparison']}

#### Exact-match and unmapped reasons

{parts['comparison_reasons']}

The tidy ledger has **{parts['mapped']}** rows with transferred prior status/provenance and **{parts['unmapped']}** rows marked unmapped. Exact transfers require an unambiguous (report, line, value) key; all others remain unguessed.

### Key claims

The following key-tier rows are selected by the frozen tier rule (summary, findings, gate verdict, and audit/graduation tables). Supporting claims remain in the ledger.

{parts['key_claims']}

### M1 unrecoverable-claim recheck

The **{parts['m1_count']}** M1 rows remain classified as unrecoverable for the historical aggregate test-run record. The inventory source is the [M1 contract freeze](../contracts/M1_CONTRACT_FREEZE.md); current source evidence includes [golden PRNG vectors](../../crates/sim-core/tests/golden_prng_tests.rs), [determinism oracles](../../crates/sim-model/tests/determinism_oracle_tests.rs), [M0 graduation gate](../../crates/sim-model/tests/m0_determinism_gate_tests.rs), [snapshot tests](../../crates/sim-model/tests/phase11_snapshot_tests.rs), and the [M1 manifest](../../contracts/m1_contract.toml). Those sources cover semantics and canonical values, not the aggregate test-run output. Each row carries this reason in its notes field.

{parts['m1_recheck']}

### Top evidence gaps

{parts['gaps']}

## Findings

- The tidy ledger preserves one row per selected claim and leaves ambiguous or unmatched provenance unmapped; see the ledger comparison tables above.
- The M1 recheck keeps historical test-count claims unrecoverable while identifying the present semantic tests and canonical goldens that cover their contract behavior.
- The largest gaps are grouped directly from the current status values in the tidy ledger; see Top evidence gaps.

## Limits

- A carried reproducible-now status means inputs/source are present; it does not claim that any harness was run in this inventory task.
- Values excluded as input/configuration or outside the selected claim contexts are not assigned new provenance.
- This report compares source evidence only and does not remeasure timings or run simulations.
'''


def _render_quarto(qmd: Path, output_dir: Path) -> Path:
    result = subprocess.run(
        ["quarto", "render", str(qmd), "--to", "gfm", "--output-dir", str(output_dir)],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if result.returncode:
        raise RuntimeError(f"Quarto render failed:\n{result.stdout}\n{result.stderr}")
    return output_dir / "M5_01_INVENTORY.md"


def check_outputs() -> bool:
    qmd_text = render_qmd()
    if not QMD_PATH.exists() or QMD_PATH.read_text(encoding="utf-8") != qmd_text or not MD_PATH.exists():
        print(f"generated QMD/Markdown differs: {QMD_PATH} / {MD_PATH}", file=sys.stderr)
        return False
    with tempfile.TemporaryDirectory(prefix="simciv-m5-01-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        qmd.write_text(qmd_text, encoding="utf-8", newline="\n")
        rendered = _render_quarto(qmd, temp_dir)
        if not rendered.exists() or rendered.read_bytes() != MD_PATH.read_bytes():
            print(f"Quarto output differs: {MD_PATH}", file=sys.stderr)
            return False
    print("M5-01 QMD and Quarto GFM output match committed files")
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="render in a temporary directory and compare without writing")
    args = parser.parse_args(argv)
    if args.check:
        return 0 if check_outputs() else 1
    qmd_text = render_qmd()
    QMD_PATH.write_text(qmd_text, encoding="utf-8", newline="\n")
    rendered = _render_quarto(QMD_PATH, QMD_PATH.parent)
    if rendered != MD_PATH:
        rendered.replace(MD_PATH)
    print(f"generated {QMD_PATH.relative_to(ROOT)} and {MD_PATH.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
