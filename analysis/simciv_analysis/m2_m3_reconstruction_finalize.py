from __future__ import annotations

import csv
import gzip
import io
import json
import re
import subprocess
import tomllib
from collections import Counter, defaultdict
from pathlib import Path

from .claims import scan_documents
from .m2_m3_evidence_inventory import a5_cell_floor_seconds
from .io import write_text_lf
from .provenance import ROOT

CSV_PATH = ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv"
MD_PATH = ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.md"
LEDGER_PATH = ROOT / "docs/reproducibility/claims.csv.gz"
A5_PLAN_SPEC = ROOT / "docs/reproducibility/m5_05_configs/m3_05_a5_pilot.toml"

# tuple: represented config cells, new A-5 cells, run key, status, rationale, extra evidence
PLAN: dict[str, tuple[int, int, str, str, str, str]] = {
    "E002": (4, 4, "M2-06-telemetry", "timed", "Four telemetry-mode rows are four paired A/B rebaseline cells; one cell per row.", "docs/reports/M2_06_BASELINE_REPORT.md#L172-L179"),
    "E003": (4, 0, "M2-06-telemetry", "shared", "The throughput-penalty sentence is derived from the same four telemetry rows already counted in E002.", "docs/reports/M2_06_BASELINE_REPORT.md#L172-L183"),
    "E014": (2, 2, "M2-09-allocation-summary", "timed", "The allocation table has two population columns (N=10 and N=1,000); its four vector rows are outputs within those two configurations, not four independent configurations.", "docs/reports/M2_09_PHASE34_SCRATCH_BUFFER_ANALYSIS.md#L102-L107"),
    "E020": (1, 1, "M2-16_1-phase10-ablation", "timed", "One canonical N=10, 500-day comparison is described; the three variants share that single input configuration.", "docs/reports/M2_16_1_SOA_ABLATION_ANALYSIS.md#L16; docs/reports/M2_16_1_SOA_ABLATION_ANALYSIS.md#L49-L53"),
    "E025": (1, 1, "M2-18-prior-poc", "timed", "The cited M2-17 comparison names one N=1,000 configuration and one conversion/compute comparison.", "docs/reports/M2_18_SEGMENTED_SOA_STORAGE_POC.md#L7-L21"),
    "E032": (4, 4, "M2-31-reprofile", "timed", "Workloads A/B at N=1,000 and N=10,000 are the four endpoints used by the headline alpha/build comparison.", "docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L46-L48; docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L67-L75"),
    "E033": (0, 0, "M2-31-reprofile", "not-timed", "This is a static operation-count description of the Phase 3 settlement lookup path, not an elapsed-time result.", "docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L17"),
    "E034": (0, 0, "M2-31-reprofile", "not-timed", "This is a static operation-count description of the Phase 8 group scan, not an elapsed-time result.", "docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L18"),
    "E035": (2, 1, "M2-31-reprofile", "shared", "Workload B compares N=10,000 and N=20,000; the N=10,000 endpoint is already counted in E032, so only N=20,000 is additional.", "docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L130-L138"),
    "E036": (2, 0, "M2-31-reprofile", "shared", "The N=10,000 hot-path list covers workloads A/B already represented by E032.", "docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L24; docs/reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md#L102-L117"),
    "E037": (2, 0, "M2-34-reprofile", "shared", "The two P8 workload shares are the N=10,000 A/B cells included in E041.", "docs/reports/M2_34_POST_ALGORITHM_REPROFILING.md#L70-L89"),
    "E038": (0, 0, "M2-34-validation", "not-timed", "The row reports an integration-test count, not a timing observation.", "docs/reports/M2_34_POST_ALGORITHM_REPROFILING.md#L239"),
    "E039": (0, 0, "M2-34-validation", "not-timed", "The row reports an integration-test count, not a timing observation.", "docs/reports/M2_34_POST_ALGORITHM_REPROFILING.md#L240"),
    "E040": (0, 0, "M2-34-validation", "not-timed", "The numeric token is part of the baseline commit hash; the sentence reports repository state, not a timing result.", "docs/reports/M2_34_POST_ALGORITHM_REPROFILING.md#L247"),
    "E041": (6, 6, "M2-34-reprofile", "timed", "N=10,000/20,000/50,000 across workloads A/B gives six cells for the near-linear full-tick conclusion.", "docs/reports/M2_34_POST_ALGORITHM_REPROFILING.md#L14; docs/reports/M2_34_POST_ALGORITHM_REPROFILING.md#L40-L74"),
    "E042": (8, 8, "M2-37-reprofile", "timed", "Two workload families at N=1,000/10,000/50,000/100,000 give eight cells for the two headline scaling intervals and phase shares.", "docs/reports/M2_37_POST_P5_P6B_REPROFILING.md#L14-L18; docs/reports/M2_37_POST_P5_P6B_REPROFILING.md#L32-L58"),
    "E043": (4, 0, "M2-37-reprofile", "shared", "The N=10,000 and N=50,000 A/B phase shares are subsets of the eight cells in E042.", "docs/reports/M2_37_POST_P5_P6B_REPROFILING.md#L16; docs/reports/M2_37_POST_P5_P6B_REPROFILING.md#L66-L110"),
    "E044": (0, 0, "M2-40-conclusion-cells", "not-timed", "Alignment status is explicitly a debt statement and the report says no alignment counters were collected.", "docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L410-L414"),
    "E045": (4, 2, "M2-40-conclusion-cells", "shared", "N=100,000/200,000 in workloads A/B gives four represented cells; N=100,000 is shared with E049, so the two 200,000 cells are additional.", "docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L15; docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L43-L66"),
    "E046": (0, 0, "M2-40-conclusion-cells", "not-timed", "The numeric values are objective identifiers/table fields; this audit row adds no timing observation.", "docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L443"),
    "E047": (4, 2, "M2-40-conclusion-cells", "shared", "The 1,000/100,000 A/B boundary cells support the graduation alpha criterion; the 100,000 cells are shared with E049, leaving two additional 1,000 cells.", "docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L20; docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L43-L66"),
    "E048": (4, 0, "M2-40-conclusion-cells", "shared", "The 100,000/200,000 A/B extension cells are already covered by E045; this paragraph derives alpha from those measurements.", "docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L15; docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L43-L66"),
    "E049": (8, 8, "M2-40-conclusion-cells", "timed", "N=10,000/20,000/50,000/100,000 across A/B gives eight paired Serial/Rayon configurations for the 10k–100k speedup and phase-share findings.", "docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L17; docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L33-L35; docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L43-L66"),
    "E050": (9, 0, "M2-X-optimization-ledger", "shared", "The executive summary reuses the nine phase/milestone rows enumerated in E051; it adds no configuration cell.", "docs/reports/M2_X_REBASELINE_REPORT.md#L17; docs/reports/M2_X_REBASELINE_REPORT.md#L34-L54"),
    "E051": (9, 9, "M2-X-optimization-ledger", "timed", "The optimization ledger has nine distinct milestone/phase rows, one before/after comparison per row.", "docs/reports/M2_X_REBASELINE_REPORT.md#L34-L54"),
    "E054": (6, 0, "M3-02-workload-counts", "not-timed", "The table contains six workload-count outputs (N=10k/100k by three p_local values); they are untimed outcome counts, not A-5 timing cells.", "docs/reports/M3_02_PARALLEL_PHASE6B.md#L1241-L1246"),
    "E055": (5, 5, "M3-03-candidate-decisions", "timed", "The candidate-decision table has five candidate rows; each row is one candidate profile evaluation, with its supporting timings in the same M3-03 source run.", "docs/reports/M3_03_SEARCH_REDUCTION.md#L40-L58"),
    "E056": (12, 12, "M3-03-day-grid", "timed", "The frozen grid is N={10k,100k} × p_local={0,0.5,1} × horizons={day 0, day 150}, twelve paired configurations.", "docs/reports/M3_03_SEARCH_REDUCTION.md#L28-L38; docs/reports/M3_03_SEARCH_REDUCTION.md#L44-L57; docs/reports/M3_03_SEARCH_REDUCTION.md#L65-L78"),
    "E057": (12, 0, "M3-03-day-grid", "shared", "The whole-tick conclusion is derived from the same twelve paired configurations already counted in E056.", "docs/reports/M3_03_SEARCH_REDUCTION.md#L15; docs/reports/M3_03_SEARCH_REDUCTION.md#L29-L32; docs/reports/M3_03_SEARCH_REDUCTION.md#L44-L57"),
    "E058": (12, 12, "M3-04-sustained-grid", "timed", "M3-04 records N={10k,100k} × p_local={0,0.5,1} at day 0 and day 150: twelve configurations; a paired comparison remains one cell per configuration.", "docs/reports/M3_04_SUSTAINED_WORKLOAD.md#L62-L73"),
    "E059": (6, 0, "M3-04-sustained-grid", "shared", "The six population/locality stability outcomes are workload records from the same M3-04 matrix; no separate A-5 timer cell is added.", "docs/reports/M3_04_SUSTAINED_WORKLOAD.md#L10-L20; docs/reports/M3_04_SUSTAINED_WORKLOAD.md#L62-L73"),
    "E060": (3, 0, "M3-04-sustained-grid", "shared", "The three day-150 locality observations are outputs of cells already counted in E058.", "docs/reports/M3_04_SUSTAINED_WORKLOAD.md#L62-L73; docs/reports/M3_04_SUSTAINED_WORKLOAD.md#L353-L355"),
    "E061": (16, 0, "M3-05-scale-matrix", "shared", "The report declares 16 N×k×checkpoint full-tick configurations; this summary reuses the full pilot cells counted in the merged first-break entry.", "docs/reports/M3_05_SCALE_LIMIT.md#L22-L26; crates/sim-model/benches/m3_scale_limit_bench.rs#L115-L123; crates/sim-model/benches/m3_scale_limit_bench.rs#L2304-L2317"),
    "E062": (16, 0, "M3-05-scale-matrix", "shared", "Digest parity was checked on the same 16 paired configurations and adds no timed cell.", "docs/reports/M3_05_SCALE_LIMIT.md#L11; docs/reports/M3_05_SCALE_LIMIT.md#L234-L256"),
    "E063": (16, 0, "M3-05-scale-matrix", "shared", "First-break criteria are derived from the same scale-grid and setup records; no new cell is needed.", "docs/reports/M3_05_SCALE_LIMIT.md#L207-L232"),
    "E064": (8, 0, "M3-05-scale-memory", "not-timed", "The report has eight N×k memory/setup rows; the OS working-set probe is explicitly outside timed samples, so these add no A-5 timing cells.", "docs/reports/M3_05_SCALE_LIMIT.md#L25; docs/reports/M3_05_SCALE_LIMIT.md#L44-L55"),
    "E067": (0, 0, "M3-07-residual", "not-timed", "The byte-traffic values are analytical field-size estimates, not measured timing cells.", "docs/reports/M3_07_SCALE_RESIDUAL.md#L133-L136"),
    "E068": (13, 13, "M3-07-residual", "timed", "The residual driver has 10 fine-sweep populations, one alpha anchor, and two slot-layout cells; 13 total profile cells.", "crates/sim-model/benches/m3_scale_limit_bench.rs#L124-L127; crates/sim-model/benches/m3_scale_limit_bench.rs#L2139-L2194; docs/reports/M3_07_SCALE_RESIDUAL.md#L140-L174"),
    "E069": (18, 18, "M3-08-population-profile", "timed", "The findings summarize 12 full-tick cells (N=2 × compaction interval=3 × normal/compaction=2), 4 interval-50 Phase 9b profile cells (N=2 × day class=2), and 2 equal-live cells (N=2).", "docs/reports/M3_08_POPULATION_DYNAMICS.md#L59-L70; docs/reports/M3_08_POPULATION_DYNAMICS.md#L90-L130; docs/reports/M3_08_POPULATION_DYNAMICS.md#L135-L155"),
    "E070": (18, 0, "M3-08-population-profile", "shared", "This inference is based on the M3-08 measurements counted in E069; it does not add timing cells.", "docs/reports/M3_08_POPULATION_DYNAMICS.md#L155-L165"),
    "E071": (6, 6, "M3-09-incremental-profile", "timed", "The findings use two full-tick population cells plus four interval-50 Phase 9b cells (N=2 × normal/compaction); the equal-live ratio is reported separately and was rechecked in M5-05.", "docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L16-L24; docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L26-L73; docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L91-L103"),
    "E072": (6, 0, "M3-09-incremental-profile", "shared", "This inference summarizes the same incremental Phase 9b cells already counted in E071.", "docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L102-L104"),
    "E073": (26, 0, "M3-08-M3-09-and-M5-05-shared", "shared", "This summary combines M3-08 baseline timing already counted in E069, M3-09 incremental timing already counted in E071, and two equal-live cells covered by the protocol-matched M5-05 scope check; it adds no cells.", "docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L12-L24; docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L91-L103; docs/reproducibility/M5_05_M0_M3_REGENERATION_RESULTS.json#/m3_09_scope_check"),
    "E079": (2, 0, "M3-01-and-M2-40-source-cells", "shared", "The audit repeats the M3-01 and M2-40 100k medians; those source cells are already scheduled in their report groups.", "docs/reports/M3_AUDIT_INTERIM_AUDIT.md#L11; docs/reports/M3_01_SERIAL_BASELINE.md#L20-L60; docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#L43-L66"),
    "E074": (36, 0, "M3-09-parity-gates", "not-timed", "The 36-cell matrix and 1,000-day gate are deterministic parity/stability checks; they are not A-5 timing cells.", "docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L14-L15; docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md#L278-L290"),
}


def compress_lines(values: list[int]) -> str:
    numbers = sorted(set(values))
    if not numbers:
        return ""
    spans = []
    start = previous = numbers[0]
    for value in numbers[1:]:
        if value == previous + 1:
            previous = value
            continue
        spans.append(f"{start}-{previous}" if start != previous else str(start))
        start = previous = value
    spans.append(f"{start}-{previous}" if start != previous else str(start))
    return ",".join(spans)


def main() -> None:
    spec = tomllib.loads(A5_PLAN_SPEC.read_text(encoding="utf-8"))
    warmups = int(spec["warmups"])
    samples = int(spec["samples"])
    sample_floor = float(spec["sample_floor_seconds"])
    cell_floor = a5_cell_floor_seconds()
    with CSV_PATH.open(encoding="utf-8", newline="") as f:
        old_rows = list(csv.DictReader(f))
    generated_columns = {
        "A5_cells_in_group", "A5_cells_to_schedule", "A5_run_id", "A5_status",
        "A5_cell_count_evidence", "A5_cell_count_reason", "report_revision_rust_toolchain",
    }
    for row in old_rows:
        for column in generated_columns:
            row.pop(column, None)
    with gzip.open(LEDGER_PATH, "rt", encoding="utf-8", newline="") as f:
        ledger = list(csv.DictReader(f))
    claims_by_id = {row["claim_id"]: row for row in ledger}
    scan_by_key = {str(row["claim_key"]): row for row in scan_documents(ROOT)}
    by_id = {row["experiment_id"]: row for row in old_rows}
    if "M3-05-FIRST-BREAK" in by_id:
        rows = list(old_rows)
        merged = by_id["M3-05-FIRST-BREAK"]
    elif (
        "E065" in by_id
        and "E066" in by_id
        and by_id["E065"]["report"] == "M3_05_SCALE_LIMIT.md"
        and by_id["E066"]["report"] == "M3_05_SCALE_LIMIT.md"
        and by_id["E065"]["section_anchor"] == "first-break-findings"
        and by_id["E066"]["section_anchor"] == "first-break-findings"
    ):
        first, second = by_id["E065"], by_id["E066"]
        first_ids = first["claim_ids"].split(";")
        second_ids = second["claim_ids"].split(";")
        if set(first_ids) & set(second_ids):
            raise RuntimeError("M3-05 first-break groups contain overlapping claim IDs")
        class_counts = Counter(claims_by_id[claim_id]["class"] for claim_id in first_ids + second_ids)
        merged = dict(first)
        merged.update({
            "experiment_id": "M3-05-FIRST-BREAK",
            "claim_source_group": "two-tables-one-section-and-protocol",
            "table_header": f"{first['table_header']} || {second['table_header']}",
            "claim_count": str(int(first["claim_count"]) + int(second["claim_count"])),
            "headline_count": str(int(first["headline_count"]) + int(second["headline_count"])),
            "key_count": str(int(first["key_count"]) + int(second["key_count"])),
            "class_mix": ";".join(f"{name}={count}" for name, count in sorted(class_counts.items())),
            "claim_ids": ";".join(first_ids + second_ids),
            "claim_values": f"{first['claim_values']}; {second['claim_values']}",
        })
        merged["classification_basis"] = "Two separate result tables in one report section summarize the same M3-05 scale run; merged as one experiment. Claim IDs are disjoint."
        rows = [row for row in old_rows if row["experiment_id"] not in {"E065", "E066"}]
        rows.append(merged)
    else:
        rows = list(old_rows)
        merged = None
    rows.sort(key=lambda row: (row["report"], row["section_anchor"], row["experiment_id"]))

    # Existing estimated groups plus the formerly unestimated groups. The second value is the
    # additional unique cell count; shared summaries intentionally contribute zero to totals.
    plan: dict[str, tuple[int, int, str, str, str]] = {}
    for experiment, represented, additional, run_id, status, reason, _ in (
        (k, *v) for k, v in PLAN.items()
    ):
        plan[experiment] = (represented, additional, run_id, status, reason)
    # Retain the exact previously estimated cells, correcting matrix sizes where report tables
    # identify the full configuration set. M3-05 E065/E066 were merged above.
    plan.update({
        "E001": (0, 0, "M2-06-determinism", "not-timed", "The 500-day oracle claim is deterministic; the three named M1 manifest tests are its verification path, not an A-5 timing cell."),
        "E025": (1, 1, "M2-18-prior-poc", "timed", "One N=1,000 AoS/ephemeral-SoA comparison configuration is named in the reported finding."),
        "E042": (8, 8, "M2-37-reprofile", "timed", "Two workloads × N=1k/10k/50k/100k gives eight headline profile configurations."),
        "E047": (4, 2, "M2-40-conclusion-cells", "shared", "Two workloads × N=1k/100k; N=100k overlaps E049, so only the two N=1k cells are additional."),
        "E051": (9, 9, "M2-X-optimization-ledger", "timed", "Nine distinct before/after optimization rows are the nine source comparison cells."),
        "E055": (5, 5, "M3-03-candidate-decisions", "timed", "Five candidate rows, each one candidate evaluation configuration."),
        "E056": (12, 12, "M3-03-day-grid", "timed", "N=2 × p_local=3 × horizon=2 forms twelve paired full-tick/profile configurations."),
        "E057": (12, 0, "M3-03-day-grid", "shared", "The no-whole-tick-gain summary is computed from the same twelve cells counted in E056."),
        "E058": (12, 12, "M3-04-sustained-grid", "timed", "N=2 × p_local=3 × checkpoint=2 forms twelve configurations."),
        "E068": (13, 13, "M3-07-residual", "timed", "Ten fine-sweep populations + one alpha anchor + two slot-layout cells = thirteen profile cells."),
        "E069": (18, 18, "M3-08-population-profile", "timed", "Twelve N×interval×day-class full-tick cells + four Phase 9b cells + two equal-live cells."),
        "E070": (18, 0, "M3-08-population-profile", "shared", "The inference is derived from the M3-08 timing set counted in E069."),
        "E071": (6, 6, "M3-09-incremental-profile", "timed", "Two population full-tick cells plus four interval-50 normal/compaction Phase 9b cells."),
        "E072": (6, 0, "M3-09-incremental-profile", "shared", "This is an inference from the same incremental-profile cells counted in E071."),
        "E073": (26, 0, "M3-08-M3-09-and-M5-05-shared", "shared", "This summary repeats M3-08/M3-09 measurements already counted in their source runs; M5-05 also rechecked the two equal-live cells."),
        "E079": (2, 0, "M3-01-and-M2-40-source-cells", "shared", "The audit copies 100k medians already represented by M3-01 and M2-40 source cells."),
    })

    if merged is not None and merged["experiment_id"] in plan:
        raise RuntimeError("merged M3-05 group unexpectedly collides with a plan row")
    if merged is not None:
        raise RuntimeError("M3-05 pilot claims are no longer missing; do not schedule them as missing evidence")

    # Source line citations for every row, including zero-cell static/derived groups.
    toolchain_cache: dict[str, str] = {}
    for row in rows:
        line_numbers: dict[str, set[int]] = defaultdict(set)
        for claim_id in row["claim_ids"].split(";"):
            ledger_row = claims_by_id.get(claim_id)
            if not ledger_row:
                raise RuntimeError(f"claim ID disappeared from ledger: {claim_id}")
            scanned = scan_by_key.get(ledger_row["claim_key"])
            if scanned:
                line_numbers[row["report"]].add(int(scanned["line"]))
        citations = []
        for report, lines in sorted(line_numbers.items()):
            citations.append(f"docs/reports/{report}#L{compress_lines(list(lines))}")
        represented, additional, run_id, status, rationale = plan.get(
            row["experiment_id"],
            (0, 0, f"derived:{row['report']}#{row['section_anchor']}", "not-timed", "Claim is structural, validation-only, workload-count, or a derived summary; it adds no A-5 timing cell."),
        )
        if represented < additional:
            raise RuntimeError(f"additional A-5 cells exceed represented cells for {row['experiment_id']}")
        evidence = "; ".join(citations)
        supplemental = PLAN.get(row["experiment_id"])
        if supplemental:
            evidence += "; " + supplemental[5]
        if row["experiment_id"] in {"E061", "E062", "E063", "E064", "M3-05-FIRST-BREAK"}:
            evidence += "; docs/reports/M3_05_SCALE_LIMIT.md#L22-L26; crates/sim-model/benches/m3_scale_limit_bench.rs#L115-L123; crates/sim-model/benches/m3_scale_limit_bench.rs#L1236-L1266; crates/sim-model/benches/m3_scale_limit_bench.rs#L2304-L2317"
        if row["experiment_id"] == "E073":
            evidence += "; docs/reproducibility/M5_05_M0_M3_REGENERATION_RESULTS.json#/m3_09_scope_check"
        row["A5_cells_in_group"] = str(represented)
        row["A5_cells_to_schedule"] = str(additional)
        row["A5_run_id"] = run_id
        row["A5_status"] = status
        row["A5_cell_count_evidence"] = evidence
        row["A5_cell_count_reason"] = rationale
        row["A5_lower_bound_seconds"] = str(additional * cell_floor)
        row["A5_estimate_basis"] = f"{cell_floor} s/cell floor ({warmups} warmups + {samples} samples, each >= {sample_floor:g} s); excludes compile/setup/retries and r above the minimum."
        commit = row["report_written_commit"]
        if commit not in toolchain_cache:
            result = subprocess.run(
                ["git", "-C", str(ROOT), "show", f"{commit}:rust-toolchain.toml"],
                capture_output=True, text=True,
            ) if commit and commit != "not found" else None
            channel = next((line.split("=", 1)[1].strip().strip('"') for line in (result.stdout.splitlines() if result and result.returncode == 0 else []) if line.strip().startswith("channel")), None)
            toolchain_cache[commit] = f"{channel} (rust-toolchain.toml at report commit {commit})" if channel else "no pinned rust-toolchain.toml at report commit; use compiler recorded in report or mark needs-decision"
        row["report_revision_rust_toolchain"] = toolchain_cache[commit]

    # Each ledger claim must occur in exactly one inventory row.
    all_ids = [claim_id for row in rows for claim_id in row["claim_ids"].split(";") if claim_id]
    if len(set(all_ids)) != len(all_ids):
        raise RuntimeError(f"claim uniqueness failed: total={len(all_ids)}, unique={len(set(all_ids))}")
    if any(not row["A5_cell_count_evidence"] for row in rows):
        raise RuntimeError("every experiment row must cite cell-count evidence")

    # Preserve the original CSV columns and add the plan/estimate evidence columns.
    columns = list(rows[0])
    out = io.StringIO(newline="")
    writer = csv.DictWriter(out, fieldnames=columns, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    write_text_lf(CSV_PATH, out.getvalue())

    class_groups = Counter(row["classification"] for row in rows)
    class_claims = Counter()
    class_cells = Counter()
    for row in rows:
        cls = row["classification"]
        class_claims[cls] += int(row["claim_count"])
        class_cells[cls] += int(row["A5_cells_to_schedule"])
    class_seconds = {key: value * cell_floor for key, value in class_cells.items()}
    total_cells = sum(class_cells.values())
    total_seconds = total_cells * cell_floor
    run_cells: dict[str, int] = defaultdict(int)
    for row in rows:
        run_cells[row["A5_run_id"]] += int(row["A5_cells_to_schedule"])
    runtime_runs: dict[str, dict[str, object]] = {}
    for row in rows:
        add_cells = int(row["A5_cells_to_schedule"])
        if not add_cells:
            continue
        run_id = row["A5_run_id"]
        grouped = runtime_runs.setdefault(run_id, {
            "run_id": run_id, "cells": 0, "seconds": 0, "report": row["report"],
            "anchor": row["section_anchor"], "classification": row["classification"],
            "claim_count": 0,
        })
        grouped["cells"] = int(grouped["cells"]) + add_cells
        grouped["seconds"] = int(grouped["seconds"]) + int(row["A5_lower_bound_seconds"])
        grouped["claim_count"] = int(grouped["claim_count"]) + int(row["claim_count"])
    largest = sorted(runtime_runs.values(), key=lambda run: (-int(run["seconds"]), str(run["run_id"])))[:10]
    previously_unestimated_count = sum(1 for row in rows if row["experiment_id"] not in plan)
    needs_decision = [row for row in rows if row["A5_status"] == "needs-decision"]
    toolchain_channels = sorted({row["report_revision_rust_toolchain"].split(" ", 1)[0] for row in rows})

    report = [
        "# M2/M3 Evidence Reconstruction Inventory",
        "",
        "**Status:** generated from committed source inputs and pilot records; no new simulation or timing run occurred during generation.",
        "",
        "## Scope and source method",
        "",
        f"This inventory covers the {len(all_ids)} M2/M3 headline/key claims that currently have status `missing-data` or `missing-source` in the recovered-mapped ledger. Table claims are grouped by table and prose claims by paragraph/list item. Each CSV row cites source report lines; timed cell counts additionally cite the table/method or benchmark loops used to derive the count.",
        "",
        "A cell is one unique configuration/measurement family for A-5. A paired path comparison remains one cell. Summaries and inferences that reuse a measured matrix have zero additional cells and name their shared run ID. Non-timing records (tests, workload counts, schema facts, static analysis) have zero A-5 timing cells and remain linked to their output/configuration cell count where useful.",
        "",
        f"The A-5 floor is {cell_floor} seconds per unique timed cell from the committed pilot protocol: {warmups} warmups plus {samples} samples with a {sample_floor:g}-second minimum. It excludes compilation, initialization, non-timed probes, and extra repetition factor `r`. This is a lower bound, not a wall-time prediction.",
        "",
        f"Revision policy: the default reproduction revision is the report's creation commit, answering historical-number reproduction; a separate HEAD run answers only whether the conclusion still holds. The CSV records each report commit and its `rust-toolchain.toml`; pinned toolchain channels found: {', '.join(toolchain_channels)}.",
        "",
        "## Cell-count resolution and duplicate audit",
        "",
        f"All **{previously_unestimated_count}** groups that lacked a cell count in the previous inventory were assigned a report/harness-derived count, a zero-cell non-timing/shared designation, or a `needs-decision` status. `needs-decision` groups: **{len(needs_decision)}**.",
        "",
        "The input ledger is the source of the selected claim set; rows are rejected if a claim ID is duplicated.",
        "",
        "## A-5 estimate by class",
        "",
        "| Class | Experiment groups | Claims | Unique A-5 cells | Lower-bound seconds |",
        "|---|---:|---:|---:|---:|",
    ]
    class_labels = {"a": "(a) deterministic replay", "b": "(b) committed harness", "c": "(c) complete recovered source", "d": "(d) reconstructed protocol", "e": "(e) underspecified", "f": "(f) M5-05 conclusion covered"}
    for cls in "abcdef":
        report.append(f"| {class_labels[cls]} | {class_groups[cls]} | {class_claims[cls]} | {class_cells[cls]} | {class_seconds.get(cls, 0):,} |")
    report.extend([
        f"| **Total** | **{len(rows)}** | **{sum(int(row['claim_count']) for row in rows)}** | **{total_cells}** | **{total_seconds:,}** |",
        "",
        f"The resulting A-5 floor is **{total_seconds:,} seconds ({total_seconds/3600:.2f} hours)**. This sums unique cells; shared summaries contribute zero. `needs-decision` is currently zero, so no timing cells are hidden from the sum.",
        "",
        "## Ten largest groups by A-5 runtime",
        "",
        "| # | Experiment group | Class | Cells | Lower-bound seconds |",
        "|---:|---|---|---:|---:|",
    ])
    for index, row in enumerate(largest, 1):
        report.append(f"| {index} | `{row['report']}#{row['anchor']}` (`{row['run_id']}`) | {row['classification']} | {row['cells']} | {int(row['seconds']):,} |")
    report.extend([
        "",
        "## M3-05 end-to-end pilot proposal",
        "",
        "The completed pilot settings and protocol are maintained in [m3_05_a5_pilot.toml](m5_05_configs/m3_05_a5_pilot.toml). The committed result records generate the outcome section.",
        "",
        "The existing legacy benchmark is a protocol reference only; its sample procedure is not the frozen A-5 procedure. The committed M3-05 driver and its specification are the invocation source:",
        "",
        "```powershell\ncargo bench -p sim-model --bench m3_scale_limit_bench -- --a5-reconstruction --output docs/data/m5_05/m3_05_pilot/m3_05_a5.jsonl\n```",
        "",
        "Record families and configurations are defined in the committed pilot specification and raw output schema. The raw JSONL lives under `docs/data/m5_05/m3_05_pilot/`; its summary and manifest metadata are maintained in Git.",
        "",
        f"A-5 floor: **{total_cells} unique cells × {cell_floor} seconds = {total_seconds:,} seconds ({total_seconds / 3600:.2f} hours)**. Compilation, graph/world initialization, non-timed probes, host setup, and extra repetitions make actual wall time longer; no wall-time run estimate is claimed.",
        "",
        "**Pilot precondition:** run only the separately committed driver named by the pilot specification; do not use the legacy bench as an A-5 substitute. No command was run while regenerating this inventory.",
        "",
        "## Criteria and status",
        "",
        "The frozen acceptance criteria are in [REPRODUCTION_CRITERIA.md](REPRODUCTION_CRITERIA.md); this inventory records remaining evidence work without changing those criteria.",
        "",
    ])
    write_text_lf(MD_PATH, "\n".join(report))

    print(f"groups={len(rows)} claims={len(all_ids)} unique_claim_ids={len(set(all_ids))}")
    print(f"previously_unestimated_groups={previously_unestimated_count} needs_decision={len(needs_decision)}")
    print(f"A5_cells={total_cells} lower_bound_seconds={total_seconds} lower_bound_hours={total_seconds/3600:.4f}")
    print("A5_cells_by_class=" + json.dumps(dict(class_cells), sort_keys=True))
    print("top10=" + json.dumps(largest, ensure_ascii=False))


if __name__ == "__main__":
    main()
