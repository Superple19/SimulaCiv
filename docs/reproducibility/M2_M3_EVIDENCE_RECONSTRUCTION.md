# M2/M3 Evidence Reconstruction Inventory

**Status:** generated inventory updated after the M3-05 pilot; no other reconstruction experiment was run.

## Scope and source method

At initial generation this inventory covered 484 M2/M3 headline/key claims that were still `missing-data` or `missing-source`; the M3-05 pilot outcomes below update that group's current claim statuses. Table claims are grouped by table and prose claims by paragraph/list item. Each CSV row cites source report lines; timed cell counts additionally cite the table/method or benchmark loops used to derive the count.

A cell is one unique configuration/measurement family for A-5. A paired path comparison remains one cell. Summaries and inferences that reuse a measured matrix have zero additional cells and name their shared run ID. Non-timing records (tests, workload counts, schema facts, static analysis) have zero A-5 timing cells and remain linked to their output/configuration cell count where useful.

The A-5 floor is 43 seconds per unique timed cell: two warmups plus 41 timed samples, each at least one second. It excludes compilation, initialization, non-timed probes, and extra repetition factor `r`. This is a lower bound, not a wall-time prediction.

Revision policy: the default reproduction revision is the report's creation commit, answering historical-number reproduction; a separate HEAD run answers only whether the conclusion still holds. The CSV records each report commit and its `rust-toolchain.toml`; pinned toolchain channels found: 1.98.1.

## Cell-count resolution and duplicate audit

All **61** groups that lacked a cell count in the previous inventory were assigned a report/harness-derived count, a zero-cell non-timing/shared designation, or a `needs-decision` status. `needs-decision` groups: **0**.

M3-05 `#first-break-findings` was one section of one report, not two documents. It contains two distinct tables: the trigger-criteria table and the phase-alpha table. Their claim IDs are disjoint, so the inventory merges them into one 100-claim experiment group; a global assertion confirms each of the 484 claim IDs appears exactly once in the CSV.

## Original pre-pilot A-5 estimate by class (historical)

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

## Ten largest groups in the original pre-pilot plan

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

## M3-05 end-to-end pilot outcome

The completed A-5 pilot covers 48 series across 16 cells. Its 100-claim missing-data group records 80 reproduced, 7 reproduced (loose), 11 not reproduced, and 2 inconclusive (validity) claims. Two additional recovered P4 headline claims have pilot status `inconclusive (validity)`. See the [generated claim results](M3_05_PILOT_CLAIMS.csv) and the [pilot raw output instructions](../data/m5_05/m3_05_pilot/README.md).

Recorded sample-timer durations sum to 4,938.605 s. The JSONL has no wall-clock timestamps or elapsed time for calibration and warmups; see the runtime breakdown below for the estimates and unrecorded preparation components.

The frozen report's first-break claims are classified from the completed pilot; invalid paths remain inconclusive and are not silently re-tolerated.
## Criteria and status

The frozen acceptance criteria are in [REPRODUCTION_CRITERIA.md](REPRODUCTION_CRITERIA.md). The M3-05 pilot applies exact checks to metadata and the frozen ratio bands to valid timing paths; invalid paths remain inconclusive. No additional experiment was run for this inventory update.

<!-- BEGIN GENERATED M3-05 RUNTIME UPDATE -->
## M3-05 pilot status and remaining runtime estimate

Generated by [m3_05_pilot_runtime.py](../../analysis/simciv_analysis/m3_05_pilot_runtime.py) from [M3-05 A-5 harness](../../crates/sim-model/examples/m3_05_a5_reconstruction.rs) and `docs/data/m5_05/m3_05_pilot/raw/m3_05_a5.jsonl`. The raw JSONL contains per-sample `sample_seconds` but no wall-clock timestamps, warmup durations, calibration-batch durations, or checkpoint/context-preparation durations. The timed sample sum is recorded; calibration and warmup components below are estimates; preparation fields are partial where stated.

### Elapsed components

| Component | Seconds | Basis |
|---|---:|---|
| initialization + graph + projection | 10.362660 | measured fields in 8 SCALE_SETUP rows |
| checkpoint/context preparation | not recorded | not recorded |
| calibration probes | 268.065310 | estimated from 3 × recorded probe median per series |
| warmups | 240.907548 | estimated as warmup count × timed sample mean per series |
| timed samples | 4938.604737 | sum of recorded sample_seconds |
| known component subtotal | 5457.940255 | setup + estimated calibration/warmups + recorded timed samples |

### Per-N runtime model

| N | series | setup series mean (s) | full-tick series mean (s) | phase-profile series mean (s) | observed known elapsed / series (s) | fitted (s/series) |
|---:|---:|---:|---:|---:|---:|---:|
| 100,000 | 12 | 56.488 | 57.368 | 67.110 | 60.322 | 53.726 |
| 250,000 | 12 | 59.095 | 68.669 | 91.994 | 73.253 | 87.157 |
| 500,000 | 12 | 64.184 | 103.151 | 195.960 | 121.098 | 125.675 |
| 1,000,000 | 12 | 93.200 | 111.010 | 393.663 | 199.291 | 181.214 |

The original 162-cell plan included the completed 48-cell M3-05 pilot, leaving **114 cells** for other experiments. Their A-5 floor is **4,902 s**; the per-N model estimates **8,884.0 s** for timed samples plus estimated calibration and warmups. The model uses the per-series M3-05 aggregate and equal weights across each group's listed N values; groups without parseable N use 1M conservatively (6 groups). It excludes unrecorded checkpoint/context preparation and compilation, so it is not a wall-time prediction.
The fitted observed-series model is `T(N) = 0.123074 × N^0.5280` seconds, where T includes the recorded timed sample duration plus estimated warmups and calibration probes.

### Validity exceptions

4 claim result rows are marked `inconclusive (validity)` because one or more source paths exceeded the 5% MAD/median limit; the two P4 claims are supplemental headline claims already present in the same report section.

| Experiment | Cells remaining | N values | Modeled seconds | A-5 floor seconds |
|---|---:|---|---:|---:|
| `E068` | 13 | 1000000 | 2,355.8 | 559 |
| `E058` | 12 | 1000000 | 2,174.6 | 516 |
| `E049` | 8 | 1000000 | 1,449.7 | 344 |
| `E002` | 4 | 1000000 | 724.9 | 172 |
| `E069` | 18 | 10000;100000 | 626.9 | 774 |
| `E045` | 2 | 1000000 | 362.4 | 86 |
| `E071` | 6 | 10000;100000 | 209.0 | 258 |
| `E056` | 12 | 10000 | 191.1 | 516 |
| `E020` | 1 | 1000000 | 181.2 | 43 |
| `E041` | 6 | 10000;50000 | 159.6 | 258 |

<!-- END GENERATED M3-05 RUNTIME UPDATE -->
