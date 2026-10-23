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

from simciv_analysis.claims import OLD_LEDGER, ROOT, expand_old_ledger, match_old_claims
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
    current = claims.to_dict(orient="records")
    matches, ambiguous = match_old_claims(old, current, ROOT)
    rename_path = ROOT / "docs" / "reports" / "RENAMES.md"
    renames: dict[str, str] = {}
    if rename_path.exists():
        for line in rename_path.read_text(encoding="utf-8").splitlines():
            match = re.match(r"^\|\s*`([^`]+)`\s*\|\s*`([^`]+)`\s*\|", line)
            if match:
                renames[Path(match.group(1)).name] = Path(match.group(2)).name
    new_exact = Counter((str(row["report"]), int(row["line"]), str(row["value"])) for row in current)
    old_exact = Counter((str(row["report"]), int(row["line"]), str(row["value"])) for row in old)
    previous_exact = sum(count for key, count in old_exact.items() if key in new_exact)
    mapped_old = len({int(row["old_claim_id"]) for row in matches.values()})
    mapped_new = len(matches)
    mapped_old_names = sum(str(row["report"]) in renames for row in old)
    current_reports = {str(row["report"]) for row in current}
    missing_report = sum(renames.get(str(row["report"]), str(row["report"])) not in current_reports for row in old)
    historical_log = sum(
        str(row["status"]) == "unrecoverable"
        and renames.get(str(row["report"]), str(row["report"])) == "M1_CONTRACT_FREEZE.md"
        for row in old
    )
    source_unmatched = len(old) - mapped_old - missing_report

    old_new_rows = pd.DataFrame(
        [
            {"Ledger view": "old packed records", "rows": len(old), "mapped before (exact name/line/value)": previous_exact, "matched after (informational)": mapped_old, "unmatched old rows": len(old) - mapped_old, "reason": "RENAMES.md report mapping; normalized source text and value; line only breaks ties"},
            {"Ledger view": "new tidy records", "rows": len(current), "matched to old ledger (informational)": mapped_new, "unmatched old-ledger comparison": len(current) - mapped_new, "reason": "sources.toml and regeneration results supply provenance; old matches are diagnostics only"},
        ]
    )
    reasons = pd.DataFrame(
        [
            {"old record disposition": "old rows matched (informational)", "records": mapped_old, "rule": "RENAMES.md plus normalized source/value; line breaks ties; does not assign new provenance"},
            {"old record disposition": "old rows unmatched", "records": source_unmatched + missing_report, "rule": "includes excluded inputs, changed/unselected text, and ambiguous old matches"},
            {"old record disposition": "new rows ambiguous", "records": len(ambiguous), "rule": "current claims with conflicting metadata or tied candidates; these are new-row counts"},
            {"old record disposition": "historical-log subset of transferred rows", "records": historical_log, "rule": "M1 aggregate runs are not regeneration gaps"},
            {"old record disposition": "previous exact-name/line/value baseline", "records": previous_exact, "rule": "old rows matched before the correction; diagnostic baseline"},
            {"old record disposition": "old report basenames in RENAMES.md", "records": mapped_old_names, "rule": "diagnostic count; this may overlap matched and unmatched rows"},
        ]
    )
    return old_new_rows, reasons


def _report_parts() -> dict[str, str]:
    claims = load_data("docs/reproducibility/claims.csv.gz")
    if list(claims.columns) != REQUIRED_CLAIM_FIELDS:
        raise ValueError(f"unexpected claim columns: {list(claims.columns)}")
    counts = _grouped_counts(claims)
    comparison, comparison_reasons = _comparison(claims)
    headline_counts = (
        claims.loc[claims["tier"] == "headline"]
        .assign(milestone=lambda frame: frame["report"].map(_milestone))
        .groupby("milestone", dropna=False)
        .size()
        .rename("headline claims")
        .reset_index()
    )
    old_claims = expand_old_ledger(OLD_LEDGER)
    old_matches, _ = match_old_claims(old_claims, claims.to_dict(orient="records"), ROOT)
    m1_indices = [
        index for index, old in old_matches.items()
        if str(old["report"]) == "M1_CONTRACT_FREEZE.md" and str(old["status"]) == "unrecoverable"
    ]
    m1 = claims.iloc[m1_indices].copy()
    m1 = m1.sort_values(["line", "claim_id"], kind="stable")
    m1 = m1[["claim_id", "line", "value", "class", "status", "notes"]]
    gaps = claims.loc[
        ~claims["status"].isin([
            "reproducible-now",
            "analysis-only",
            "historical-log",
            "historical-protocol",
            "bit-exact",
            "bit-exact after documented normalization",
            "conclusion holds",
        ])
    ].copy()
    gaps["milestone"] = gaps["report"].map(_milestone)
    top_gaps = (
        gaps.groupby(["milestone", "report", "status"], dropna=False)
        .size()
        .rename("claims")
        .reset_index()
        .sort_values(["claims", "report", "status"], ascending=[False, True, True], kind="stable")
        .head(10)
    )
    new_ledger = comparison.loc[comparison["Ledger view"] == "new tidy records"].iloc[0]
    input_rule = (
        "Excluded inputs are table columns or labeled assignments for population/workload sizes, group and neighbor counts, seeds, "
        "replicates, horizons/days, thread/chunk settings, and simulation parameter values. Free-text numbers are only extracted from "
        "tables, report Summary/Findings/Status sections, selected audit/graduation tables, and measured contract sections. "
        "The machine-readable contracts/m1_contract.toml "
        "is a configuration/contract input and is not scanned as a measured report."
    )
    return {
        "counts": _markdown(counts),
        "headline_counts": _markdown(headline_counts),
        "comparison": _markdown(comparison),
        "comparison_reasons": _markdown(comparison_reasons),
        "m1_recheck": _markdown(m1),
        "gaps": _markdown(top_gaps),
        "input_rule": input_rule,
        "total": f"{len(claims):,}",
        "m1_count": f"{len(m1):,}",
        "m1_historical_count": f"{int(((claims['report'] == 'M1_CONTRACT_FREEZE.md') & (claims['status'] == 'historical-log')).sum()):,}",
        "m1_extra_count": f"{int(((claims['report'] == 'M1_CONTRACT_FREEZE.md') & (claims['status'] == 'historical-log')).sum()) - len(m1):,}",
        "headline_count": f"{int((claims['tier'] == 'headline').sum()):,}",
        "key_count": f"{int((claims['tier'] == 'key').sum()):,}",
        "supporting_count": f"{int((claims['tier'] == 'supporting').sum()):,}",
        "mapped_count": f"{int(new_ledger['matched to old ledger (informational)']):,}",
        "unmapped_count": f"{int(new_ledger['unmatched old-ledger comparison']):,}",
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

The committed extractor [claims.py](../../analysis/simciv_analysis/claims.py) reads the report/contract source tree and assigns provenance from [`sources.toml`](sources.toml). Its tidy output [claims.csv.gz](claims.csv.gz) contains **{parts['total']}** rows; the prior packed ledger [superseded/claims.csv](superseded/claims.csv) contains **{parts['old_count']}** expanded scalar values. M5-04 statuses are applied only to source-map entries assigned to that stage. {parts['input_rule']}

## Results

Every table below is generated from [claims.csv.gz](claims.csv.gz) by the report generator [report_m5_01.py](../../analysis/simciv_analysis/report_m5_01.py). Old-ledger matching is diagnostic only and does not populate source paths, statuses, or M5 stages.

The tidy ledger contains **{parts['headline_count']}** headline claims, **{parts['key_count']}** key claims, and **{parts['supporting_count']}** supporting claims.

### Headline counts by milestone

{parts['headline_counts']}

Headline rows come from report Summary, Findings, and Status sections plus audit/graduation tables; full gate tables are excluded. Other gate verdicts remain in the `key` tier.

### Counts by milestone, class, status, and tier

{parts['counts']}

### Old and tidy ledger comparison

{parts['comparison']}

#### Match and unmapped reasons

{parts['comparison_reasons']}

The old ledger text-matches **{parts['mapped_count']}** new claims for comparison; **{parts['unmapped_count']}** do not match. These matches do not transfer status or provenance. The extractor assigns harness, data, status, and M5 stage from [`sources.toml`](sources.toml) and the committed regeneration results.

### M1 unrecoverable-claim recheck

The **{parts['m1_count']}** previously unrecoverable M1 aggregate claims were rechecked against current tests, goldens, and the M1 manifest. All remain `historical-log`: current tests and canonical artifacts revalidate behavior, not the original aggregate test-run totals. The current extractor also selects **{parts['m1_extra_count']}** additional numeric values from those historical M1 sections, for **{parts['m1_historical_count']}** historical-log values total. Evidence includes [golden PRNG vectors](../../crates/sim-core/tests/golden_prng_tests.rs), [determinism oracles](../../crates/sim-model/tests/determinism_oracle_tests.rs), [M0 graduation gate](../../crates/sim-model/tests/m0_determinism_gate_tests.rs), [snapshot tests](../../crates/sim-model/tests/phase11_snapshot_tests.rs), and the [M1 manifest](../../contracts/m1_contract.toml).

{parts['m1_recheck']}

### Top evidence gaps

{parts['gaps']}

## Findings

- The tidy ledger uses `sources.toml` for provenance; old-ledger matching is informational only; see the ledger comparison tables above.
- The M1 aggregate run records are historical logs and are excluded from regeneration gap totals.
- M2 design-analysis reports without measurement outputs are marked analysis-only and excluded from regeneration gap totals.
- The largest gaps are grouped directly from the current status values in the tidy ledger; see Top evidence gaps.

## Limits

- A `reproducible-now` status means a source and data mapping is present; this inventory did not run the harness.
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
