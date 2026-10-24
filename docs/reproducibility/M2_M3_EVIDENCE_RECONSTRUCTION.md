# M2/M3 Evidence Reconstruction Inventory

**Status:** generated planning inventory. No simulation or timing harness was run.

## Scope and source method

This inventory covers the 484 M2/M3 headline/key claims that were still `missing-data` or `missing-source` in the recovered-mapped ledger. Table claims are grouped by table and prose claims by paragraph/list item. Each CSV row cites source report lines; timed cell counts additionally cite the table/method or benchmark loops used to derive the count.

A cell is one unique configuration/measurement family for A-5. A paired path comparison remains one cell. Summaries and inferences that reuse a measured matrix have zero additional cells and name their shared run ID. Non-timing records (tests, workload counts, schema facts, static analysis) have zero A-5 timing cells and remain linked to their output/configuration cell count where useful.

The A-5 floor is 43 seconds per unique timed cell: two warmups plus 41 timed samples, each at least one second. It excludes compilation, initialization, non-timed probes, and extra repetition factor `r`. This is a lower bound, not a wall-time prediction.

Revision policy: the default reproduction revision is the report's creation commit, answering historical-number reproduction; a separate HEAD run answers only whether the conclusion still holds. The CSV records each report commit and its `rust-toolchain.toml`; pinned toolchain channels found: 1.98.1.

## Cell-count resolution and duplicate audit

All **61** groups that lacked a cell count in the previous inventory were assigned a report/harness-derived count, a zero-cell non-timing/shared designation, or a `needs-decision` status. `needs-decision` groups: **0**.

M3-05 `#first-break-findings` was one section of one report, not two documents. It contains two distinct tables: the trigger-criteria table and the phase-alpha table. Their claim IDs are disjoint, so the inventory merges them into one 100-claim experiment group; a global assertion confirms each of the 484 claim IDs appears exactly once in the CSV.

## A-5 estimate by class

| Class | Experiment groups | Claims | Unique A-5 cells | Lower-bound seconds |
|---|---:|---:|---:|---:|
| (a) deterministic replay | 2 | 3 | 0 | 0 |
| (b) committed harness | 68 | 402 | 138 | 5,934 |
| (c) complete recovered source | 5 | 31 | 6 | 258 |
| (d) reconstructed protocol | 2 | 26 | 18 | 774 |
| (e) underspecified | 0 | 0 | 0 | 0 |
| (f) M5-05 conclusion covered | 3 | 22 | 0 | 0 |
| **Total** | **80** | **484** | **162** | **6,966** |

The resulting A-5 floor is **6,966 seconds (1.94 hours)**. This sums unique cells; shared summaries contribute zero. `needs-decision` is currently zero, so no timing cells are hidden from the sum.

## Ten largest groups by A-5 runtime

| # | Experiment group | Class | Cells | Lower-bound seconds |
|---:|---|---|---:|---:|
| 1 | `M3_05_SCALE_LIMIT.md#first-break-findings` (`M3-05-scale-matrix`) | b | 48 | 2,064 |
| 2 | `M3_08_POPULATION_DYNAMICS.md#findings` (`M3-08-population-profile`) | d | 18 | 774 |
| 3 | `M3_07_SCALE_RESIDUAL.md#inferences-by-phase` (`M3-07-residual`) | b | 13 | 559 |
| 4 | `M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#15-alignment-and-gpu-status` (`M2-40-conclusion-cells`) | b | 12 | 516 |
| 5 | `M3_03_SEARCH_REDUCTION.md#executive-summary` (`M3-03-day-grid`) | b | 12 | 516 |
| 6 | `M3_04_SUSTAINED_WORKLOAD.md#comparison-with-m3-02-interim-audit-findings` (`M3-04-sustained-grid`) | b | 12 | 516 |
| 7 | `M2_X_REBASELINE_REPORT.md#4-allocation-scratch-buffer-optimization-ledger-m2-02-to-m2-12` (`M2-X-optimization-ledger`) | b | 9 | 387 |
| 8 | `M2_37_POST_P5_P6B_REPROFILING.md#executive-summary` (`M2-37-reprofile`) | b | 8 | 344 |
| 9 | `M2_34_POST_ALGORITHM_REPROFILING.md#executive-summary` (`M2-34-reprofile`) | b | 6 | 258 |
| 10 | `M3_09_INCREMENTAL_TOPOLOGY.md#findings` (`M3-09-incremental-profile`) | c | 6 | 258 |

## M3-05 end-to-end pilot proposal

Pilot: the merged M3-05 first-break/scale result group. It exercises the A-5 runner, phase profile, setup timing, digest/parity outputs, derived tables, and report path in one end-to-end reconstruction. The source scope is M2-40 with `base_metabolic_cost=0.3129`, M3 `k ∈ {8,16}`, `p_local=0.5`, four populations, and day-0/day-50 checkpoints; see the cited M3-05 method and benchmark source lines in the CSV.

The existing legacy command is `cargo bench -p sim-model --bench m3_scale_limit_bench`; it uses the report's 2 warmups and 7 samples, so it is a protocol reference only and cannot pass the frozen A-5 criteria. The A-5 driver mode is not implemented in the current bench. After that mode is committed, the proposed exact invocation is:

```powershell
cargo bench -p sim-model --bench m3_scale_limit_bench -- --a5-reconstruction --output docs/data/m5_05/m3_05_pilot/m3_05_a5.jsonl
```

Expected records: host/config header; eight setup and memory configurations; 16 paired full-tick configurations; 16 serial phase-profile configurations; per-cell `r`, 2 warmups, 41 samples, medians/MADs, and parity digests; alpha/first-break summaries; completion record. The raw JSONL path is under `docs/data/m5_05/m3_05_pilot/`; register it in the M5-05 DVC stage before the run, with the summary and required manifest metadata in Git.

A-5 floor: **48 timed series × 43 seconds = 2,064 seconds (34 minutes 24 seconds)**. Compilation, graph/world initialization, the non-timed memory probe, host setup, and any `r` above its one-second minimum make actual wall time longer; no wall-time run estimate is claimed.

**Pilot precondition:** the current committed bench recognizes its default mode, `--residual`, and `--diagnose`; it does not recognize `--a5-reconstruction`. Commit the A-5 mode and its result schema before invoking the proposed command. No command was run in this inventory task.

## Criteria and status

Proposed acceptance criteria are in [REPRODUCTION_CRITERIA.md](REPRODUCTION_CRITERIA.md) and remain draft pending approval; no timing is authorized under the draft. They require exact deterministic values and preserve dimensionless timing ratios/rankings; absolute elapsed times are recorded but are not pass criteria.
