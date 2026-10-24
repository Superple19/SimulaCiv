"""Generate the M5-01 inventory QMD and its Quarto-rendered GFM Markdown."""

from __future__ import annotations

import argparse
import gzip
import json
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

import pandas as pd

from simciv_analysis.claims import OLD_LEDGER, ROOT, expand_old_ledger, match_old_claims
from simciv_analysis.findings import Finding, render_findings
from simciv_analysis.io import load_data, write_text_lf
from simciv_analysis.recovered_claim_audit import SUMMARY_PATH as RECOVERED_AUDIT_SUMMARY_PATH
from simciv_analysis.recovered_claims import MISMATCH_OUTPUT
from simciv_analysis.recovered_mismatch_triage import TRIAGE_OUTPUT, TRIAGE_SUMMARY
from simciv_analysis.tables import dataframe_to_markdown


QMD_PATH = ROOT / "docs" / "reproducibility" / "M5_01_INVENTORY.qmd"
MD_PATH = ROOT / "docs" / "reproducibility" / "M5_01_INVENTORY.md"
LEDGER_PATH = ROOT / "docs" / "reproducibility" / "claims.csv.gz"
RECOVERED_STATUS_PATH = ROOT / "docs" / "reproducibility" / "recovered_claim_statuses.json.gz"
REQUIRED_CLAIM_FIELDS = ["claim_id", "report", "line", "section anchor", "claim text (short)", "context", "value", "unit", "class", "tier", "source harness", "raw data", "status", "M5 stage", "notes"]


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


def _recovered_evidence(claims: pd.DataFrame) -> dict[str, object]:
    if not RECOVERED_STATUS_PATH.is_file():
        none = pd.DataFrame([{"Report": "none", "Claims": 0}])
        return {
            "moved_table": _markdown(none),
            "unmatched_table": _markdown(none),
            "type_table": _markdown(pd.DataFrame([{"Value type": "none", "Claims": 0}])),
            "cause_table": _markdown(pd.DataFrame([{"Cause": "none", "Claims": 0}])),
            "m3_11_table": _markdown(none),
            "subcategory_table": _markdown(none),
            "genuine_table": _markdown(none),
            "summary": {"outputs_scanned": 0, "recovered": 0, "baseline_mismatch_claims": 0,
                        "baseline_mismatch_records": 0, "current_mismatch_records": 0, "genuine": 0,
            "parser_corrected": 0, "parser_corrected_records": 0, "genuine_other": 0, "config_values_unchanged": 0, "audit_precision": 0.0,
                        "parser_corrected_headline": 0, "parser_corrected_key": 0, "parser_corrected_supporting": 0,
                        "m3_11_claims": 0, "m3_11_records": 0,
                        "audit_yes": 0, "audit_no": 0, "audit_baseline_count": 0},
        }
    with gzip.open(RECOVERED_STATUS_PATH, "rt", encoding="utf-8") as stream:
        mapping = json.load(stream)
    exact = pd.DataFrame(mapping.get("claims", []))
    if exact.empty:
        moved = pd.DataFrame([{"Report": "none", "Tier": "—", "Prior status": "—", "Claims": 0}])
    else:
        recovered_statuses = {
            "recovered-original-output",
            "recovered-original-output (formatted)",
            "recovered-original-output (unit-converted)",
            "recovered-other-run",
            "recovered-derived",
        }
        moved = (
            exact.loc[exact["status_after"].isin(recovered_statuses)]
            .groupby(["report", "tier", "status_before", "status_after"], dropna=False)
            .size().rename("Claims").reset_index()
            .rename(columns={"report": "Report", "tier": "Tier", "status_before": "Prior status", "status_after": "Resolution"})
            .sort_values(["Report", "Tier", "Prior status", "Resolution"], kind="stable")
        )
        if moved.empty:
            moved = pd.DataFrame([{"Report": "none", "Tier": "—", "Prior status": "—", "Claims": 0}])
    unmatched_ids = set(int(value) for value in mapping.get("unmatched_claim_ids", []))
    unmatched = claims.loc[claims["claim_id"].astype(int).isin(unmatched_ids)].copy()
    if unmatched.empty:
        unmatched_table = pd.DataFrame([{"Report": "none", "Tier": "—", "Unmatched claims": 0}])
    else:
        unmatched_table = (
            unmatched.groupby(["report", "tier"], dropna=False).size().rename("Unmatched claims").reset_index()
            .rename(columns={"report": "Report", "tier": "Tier"})
            .sort_values(["Report", "Tier"], kind="stable")
        )
    triage_path = ROOT / TRIAGE_OUTPUT
    triage_summary_path = ROOT / TRIAGE_SUMMARY
    triage = pd.read_csv(triage_path, compression="gzip") if triage_path.is_file() else pd.DataFrame()
    triage_summary = json.loads(triage_summary_path.read_text(encoding="utf-8")) if triage_summary_path.is_file() else {}
    causes = triage_summary.get("causes", {})
    cause_order = ("rounding/formatting", "unit conversion", "different run", "derived", "genuine discrepancy")
    cause_rows = [
        {"Cause": cause, **{("Baseline records" if key == "mismatch_records" else key): value for key, value in causes[cause].items()}}
        for cause in cause_order if cause in causes
    ]
    cause_table = pd.DataFrame(cause_rows) if cause_rows else pd.DataFrame([{"Cause": "none", "Claims": 0}])
    adjustment = triage_summary.get("field_mapping_correction", {})
    subcategories = triage_summary.get("cause_subcategories", {})
    subcategory_rows = [
        {"Cause and classification detail": cause, **{("Baseline records" if key == "mismatch_records" else key): value for key, value in counts.items()}}
        for cause, counts in subcategories.items()
    ]
    subcategory_table = pd.DataFrame(subcategory_rows) if subcategory_rows else pd.DataFrame([{"Cause and classification detail": "none", "Claims": 0}])
    m3_11 = triage_summary.get("m3_11", {})
    m3_11_rows = [
        {"M3_11 mismatch field": field, "Candidate records": count}
        for field, count in m3_11.get("records_by_field", {}).items()
    ]
    m3_11_table = pd.DataFrame(m3_11_rows) if m3_11_rows else pd.DataFrame([{"M3_11 mismatch field": "none", "Candidate records": 0}])
    genuine = (
        triage.loc[(triage["cause"] == "genuine discrepancy") & triage["tier"].isin(["headline", "key"])]
        if not triage.empty else pd.DataFrame()
    )
    genuine_columns = ["claim_id", "report", "tier", "expected", "closest_or_matching_value", "relative_difference", "evidence", "rationale"]
    genuine_table = _markdown(genuine[genuine_columns]) if not genuine.empty else _markdown(pd.DataFrame([{"Genuine discrepancies": "none"}]))
    current_mismatch_records = len(pd.read_csv(MISMATCH_OUTPUT, compression="gzip")) if MISMATCH_OUTPUT.is_file() else 0
    audit = json.loads(RECOVERED_AUDIT_SUMMARY_PATH.read_text(encoding="utf-8")) if RECOVERED_AUDIT_SUMMARY_PATH.is_file() else {}
    value_types = pd.DataFrame(
        [{"Value type": key, "Claims": value} for key, value in audit.get("value_type_counts", {}).items()]
    )
    if value_types.empty:
        value_types = pd.DataFrame([{"Value type": "none", "Claims": 0}])
    statuses = exact["status_after"] if not exact.empty else pd.Series(dtype=str)
    summary = {
        "outputs_scanned": int(mapping.get("outputs_scanned", 0)),
        "recovered": int(statuses.isin(recovered_statuses).sum()),
        "baseline_mismatch_claims": int(triage_summary.get("classified_claims", 0)),
        "baseline_mismatch_records": int(triage_summary.get("mismatch_records", 0)),
        "remaining_mismatch_claims": int(triage_summary.get("classified_claims", 0)) - int(adjustment.get("claims", 0)),
        "remaining_mismatch_records": int(triage_summary.get("mismatch_records", 0)) - int(adjustment.get("mismatch_records", 0)),
        "current_mismatch_records": current_mismatch_records,
        "genuine": int((statuses == "recovered-output-mismatch").sum()),
        "parser_corrected": int((triage["cause"] == "context parser correction").sum()) if not triage.empty else 0,
        "parser_corrected_records": int(triage_summary.get("context_parser_correction_records", 0)),
        "parser_corrected_headline": int(adjustment.get("headline", 0)),
        "parser_corrected_key": int(adjustment.get("key", 0)),
        "parser_corrected_supporting": int(adjustment.get("supporting", 0)),
        "genuine_other": int(triage_summary.get("genuine_other_count", 0)),
        "m3_11_claims": int(m3_11.get("claims", 0)),
        "m3_11_records": int(m3_11.get("mismatch_records", 0)),
        "config_values_unchanged": len(mapping.get("config_value_claim_ids", [])),
        "audit_precision": float(audit.get("precision", 0.0)),
        "audit_yes": int(audit.get("context_corresponds", 0)),
        "audit_no": int(audit.get("context_does_not_correspond", 0)),
        "audit_baseline_count": int(audit.get("baseline_recovered_claims", 0)),
    }
    return {
        "moved_table": _markdown(moved),
        "unmatched_table": _markdown(unmatched_table),
        "type_table": _markdown(value_types),
        "cause_table": _markdown(cause_table),
        "subcategory_table": _markdown(subcategory_table),
        "m3_11_table": _markdown(m3_11_table),
        "genuine_table": genuine_table,
        "summary": summary,
    }


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
            "recovered-original-output",
            "recovered-original-output (formatted)",
            "recovered-original-output (unit-converted)",
            "recovered-other-run",
            "recovered-derived",
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
    m1_is_historical_log = not m1.empty and m1["status"].eq("historical-log").all()
    historical_ids = set(claims.loc[claims["status"].isin(["historical-log", "historical-protocol"]), "claim_id"])
    m2_analysis_only = claims.loc[
        (claims["report"].map(_milestone) == "M2") & (claims["status"] == "analysis-only"), "claim_id"
    ]
    findings_md, findings_checks_md = render_findings([
        Finding("[exploratory]", "The previously unrecoverable M1 aggregate rows carry the `historical-log` status.", "finding_m1_rows_are_historical_log", bool(m1_is_historical_log)),
        Finding("[exploratory]", "Historical-log and historical-protocol rows are excluded from regeneration gap totals.", "finding_historical_rows_excluded_from_gaps", not bool(set(gaps["claim_id"]) & historical_ids)),
        Finding("[exploratory]", "M2 analysis-only rows do not appear in the evidence-gap table.", "finding_m2_analysis_only_excluded_from_gaps", bool(not m2_analysis_only.empty and not (set(gaps["claim_id"]) & set(m2_analysis_only)))),
        Finding("[exploratory]", "Top evidence-gap rows are ordered by claim count descending.", "finding_top_gaps_sorted_by_count", top_gaps.empty or top_gaps["claims"].is_monotonic_decreasing),
    ])
    new_ledger = comparison.loc[comparison["Ledger view"] == "new tidy records"].iloc[0]
    recovered = _recovered_evidence(claims)
    recovered_summary = recovered["summary"]
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
        "recovered_moved_table": str(recovered["moved_table"]),
        "recovered_unmatched_table": str(recovered["unmatched_table"]),
        "recovered_value_types": str(recovered["type_table"]),
        "recovered_mismatch_causes": str(recovered["cause_table"]),
        "recovered_mismatch_subcategories": str(recovered["subcategory_table"]),
        "recovered_m3_11_fields": str(recovered["m3_11_table"]),
        "recovered_genuine_discrepancies": str(recovered["genuine_table"]),
        "findings": findings_md,
        "findings_checks": findings_checks_md,
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
        "recovered_outputs": f"{recovered_summary['outputs_scanned']:,}",
        "recovered_exact": f"{recovered_summary['recovered']:,}",
        "recovered_moved_count": f"{recovered_summary['recovered']:,}",
        "recovered_mismatch_count": f"{recovered_summary['genuine']:,}",
        "recovered_baseline_mismatch_claims": f"{recovered_summary['baseline_mismatch_claims']:,}",
        "recovered_baseline_mismatch_records": f"{recovered_summary['baseline_mismatch_records']:,}",
        "recovered_remaining_mismatch_claims": f"{recovered_summary['remaining_mismatch_claims']:,}",
        "recovered_remaining_mismatch_records": f"{recovered_summary['remaining_mismatch_records']:,}",
        "recovered_current_mismatch_records": f"{recovered_summary['current_mismatch_records']:,}",
        "recovered_parser_corrected": f"{recovered_summary['parser_corrected']:,}",
        "recovered_parser_corrected_records": f"{recovered_summary['parser_corrected_records']:,}",
        "recovered_parser_corrected_headline": f"{recovered_summary['parser_corrected_headline']:,}",
        "recovered_parser_corrected_key": f"{recovered_summary['parser_corrected_key']:,}",
        "recovered_parser_corrected_supporting": f"{recovered_summary['parser_corrected_supporting']:,}",
        "recovered_genuine_other": f"{recovered_summary['genuine_other']:,}",
        "recovered_m3_11_claims": f"{recovered_summary['m3_11_claims']:,}",
        "recovered_m3_11_records": f"{recovered_summary['m3_11_records']:,}",
        "recovered_config_count": f"{recovered_summary['config_values_unchanged']:,}",
        "recovered_audit_yes": f"{recovered_summary['audit_yes']:,}",
        "recovered_audit_no": f"{recovered_summary['audit_no']:,}",
        "recovered_audit_precision": f"{recovered_summary['audit_precision']:.1%}",
        "recovered_audit_baseline_count": f"{recovered_summary['audit_baseline_count']:,}",
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

### Recovered original-output matches

The recovered-output matcher [recovered_claims.py](../../analysis/simciv_analysis/recovered_claims.py) scanned **{parts['recovered_outputs']}** files. A match requires the same structured output row, measurement field, and all recorded configuration dimensions. It records **{parts['recovered_exact']}** claims as recovered evidence across exact, formatted, unit-converted, other-run, and derived statuses; **{parts['recovered_config_count']}** parameter values were excluded from measurement matching.

The prior 144af62 rule matched value tokens without row, label, column, or unit context. Among **{parts['recovered_audit_baseline_count']}** claims it had moved, the recorded type split was:

{parts['recovered_value_types']}

A fixed-seed sample (20261024; 50 headline/key and 100 other measured claims) found matching context for **{parts['recovered_audit_yes']}** of 150 value-only matches (**{parts['recovered_audit_precision']}**). The full review is in [recovered_claim_precision_audit.csv](recovered_claim_precision_audit.csv).

#### Claims moved from missing status

{parts['recovered_moved_table']}

#### Claims without an exact context match

These claims retain their previous status. A value found only in a different row, configuration, or measure column—or only in unstructured output—does not qualify as recovered evidence.

{parts['recovered_unmatched_table']}

#### Recovered-output mismatch triage

The baseline mismatch file recorded at commit `fddd9a9` contained **{parts['recovered_baseline_mismatch_claims']}** claims and **{parts['recovered_baseline_mismatch_records']}** output records. After correcting graph/projection metric-field assignment, **{parts['recovered_parser_corrected']}** claims (**{parts['recovered_parser_corrected_records']}** records) matched the intended output field exactly; these are a matcher correction, not data discrepancies. Tier counts for that correction: headline **{parts['recovered_parser_corrected_headline']}**, key **{parts['recovered_parser_corrected_key']}**, supporting **{parts['recovered_parser_corrected_supporting']}**.

The remaining **{parts['recovered_remaining_mismatch_claims']}** claims (**{parts['recovered_remaining_mismatch_records']}** records) are classified in precedence order: printed formatting/rounding, unit conversion, a matching other run or aggregate, a derived value, then a genuine discrepancy. Statuses for the first four are distinct; only the last retains `recovered-output-mismatch`. The generated cause table uses **claims** and **baseline output records**, split by headline/key/supporting tier:

{parts['recovered_mismatch_causes']}

The detailed audit separates the earlier field-mapping correction from the cause categories, then splits composite-cell and printed-precision formatting:

{parts['recovered_mismatch_subcategories']}

Classification details are listed in [recovered_claim_mismatch_triage.csv.gz](recovered_claim_mismatch_triage.csv.gz), with its generated summary in [recovered_claim_mismatch_triage.json](recovered_claim_mismatch_triage.json).

M3_11 accounts for **{parts['recovered_m3_11_claims']}** baseline mismatch claims and **{parts['recovered_m3_11_records']}** records. The recovered output contains composite `frontiers` arrays and `levels_max` pairs; scalar claim extraction split their values into sibling claims, which the earlier matching pass counted as mismatch records. These are composite-cell formatting matches, not separate run disagreements:

{parts['recovered_m3_11_fields']}

#### Genuine discrepancies retained

Current contextual matching leaves **{parts['recovered_mismatch_count']}** genuine mismatch claims. The table lists all headline/key cases; **{parts['recovered_genuine_other']}** supporting cases remain. An empty table means no headline/key discrepancies were found.

{parts['recovered_genuine_discrepancies']}

The contextual-difference evidence file is [recovered_claim_mismatches.csv.gz](recovered_claim_mismatches.csv.gz); it contains **{parts['recovered_current_mismatch_records']}** candidate records before triage. These candidates are not current mismatch statuses unless they remain after the ordered checks above.

## Findings

{parts['findings']}

## Limits

- A `reproducible-now` status means a source and data mapping is present; this inventory did not run the harness.
- Values excluded as input/configuration or outside the selected claim contexts are not assigned new provenance.
- This report compares source evidence only and does not remeasure timings or run simulations.

## Appendix: paired findings checks

{parts['findings_checks']}
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
        write_text_lf(qmd, qmd_text)
        rendered = _render_quarto(qmd, temp_dir)
        normalized = temp_dir / "normalized.md"
        write_text_lf(normalized, rendered.read_text(encoding="utf-8"))
        if not rendered.exists() or normalized.read_bytes() != MD_PATH.read_bytes():
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
    write_text_lf(QMD_PATH, qmd_text)
    with tempfile.TemporaryDirectory(prefix="simciv-m5-01-") as temp:
        temp_dir = Path(temp)
        qmd = temp_dir / QMD_PATH.name
        write_text_lf(qmd, qmd_text)
        rendered = _render_quarto(qmd, temp_dir)
        write_text_lf(MD_PATH, rendered.read_text(encoding="utf-8"))
    print(f"generated {QMD_PATH.relative_to(ROOT)} and {MD_PATH.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
