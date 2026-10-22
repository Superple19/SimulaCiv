# M5-01 Reproducibility Inventory

**Status:** inventory complete; no simulation harnesses were run.

## Summary

The inventory covers all **40** files under `docs/performance/`, measured evidence in the M1–M4 contracts, and the measured current-state subsection in the M5 contract. It records **37,612 numeric claim values** in 3,964 source-line records. `claims.csv` is **578,586 bytes**, below the 1 MiB limit.

The largest gap is M3 per-day/parallel measurement evidence: M3-02 alone has 10,089 claim values classified `missing-data`. M4-02 through M4-05 raw records mostly survive, but their restored harnesses are explicitly `restored-not-rerun`. M2/M3 reports that depended on temporary probes remain the main unrecoverable source gaps.

**Inventory table source:** [`claims.csv`](claims.csv). This is a read-only document scan; it has no simulation harness. The claim-level harness and raw-data mappings are recorded in the CSV; uncertain report-level mappings are marked in `notes`.

## CSV encoding and status rules

Each numeric value has its own `claim_id`. To stay below 1 MiB, numeric values from the same Markdown source line share one CSV record: `claim_id` is either one ID or an inclusive ID range, and the `value`, `unit`, `class`, and `status` fields contain aligned semicolon-separated values when the claims differ. A single value in those fields applies to the entire range. Thousands separators are removed from numeric strings without changing their value. An empty `unit` means unitless.

Repeated metadata fields use sparse fill-down: a blank `report`, anchor, claim text, harness, raw-data path, class, status, stage, or note inherits the previous nonblank value. `none` explicitly clears a previous harness, data path, or note. `claim text (short)` begins with the report line number and a shortened source excerpt; use the report and line number to inspect the full claim. All other fields use the exact status labels requested.

`reproducible-now` means the committed source and required deterministic inputs or raw data are present; it does **not** mean they were executed in this task. For timing claims, committed raw samples make the recorded sample recoverable, while any new timing conclusion still needs the frozen A-5 comparison. `restored-not-rerun` means the example was recovered from session history and its result has not been regenerated from that source. `missing-source` means data survives without its producing source. `missing-data` means a usable source exists but the original raw output does not. `unrecoverable` means the source/data needed to establish that historical value is absent or cannot reconstruct the recorded point-in-time value.

Milestone counts are grouped by report family, not by every milestone name mentioned in prose. No report file is named M0, so M0 has zero standalone report-family claims; M0 baseline comparisons inside M2 reports remain grouped under M2. The contract scan includes the M1 test inventory and measured M5-00 current-state facts; contract constants and normative gate inputs are not treated as measurements.

## Claim counts by milestone, class, and status

**Source:** [`claims.csv`](claims.csv). Counts are scalar claim IDs after expanding the packed value lists.

| Milestone | Class | Status | Claims |
|---|---|---|---:|
| M0 | all | — | 0 |
| M1 | derived | unrecoverable | 9 |
| M1 | deterministic | unrecoverable | 20 |
| M2 | derived | missing-data | 692 |
| M2 | derived | unrecoverable | 1 |
| M2 | deterministic | reproducible-now | 3,856 |
| M2 | deterministic | unrecoverable | 23 |
| M2 | timing | missing-data | 974 |
| M2 | timing | unrecoverable | 5 |
| M3 | derived | missing-data | 9,261 |
| M3 | derived | missing-source | 48 |
| M3 | derived | unrecoverable | 51 |
| M3 | deterministic | reproducible-now | 12,500 |
| M3 | deterministic | unrecoverable | 704 |
| M3 | timing | missing-data | 4,524 |
| M3 | timing | missing-source | 12 |
| M3 | timing | unrecoverable | 123 |
| M4 | derived | missing-data | 30 |
| M4 | derived | reproducible-now | 210 |
| M4 | derived | restored-not-rerun | 572 |
| M4 | derived | unrecoverable | 23 |
| M4 | deterministic | reproducible-now | 109 |
| M4 | deterministic | restored-not-rerun | 3,336 |
| M4 | deterministic | unrecoverable | 58 |
| M4 | timing | reproducible-now | 50 |
| M4 | timing | restored-not-rerun | 316 |
| M4 | timing | unrecoverable | 86 |
| M5 | deterministic | reproducible-now | 18 |
| M5 | deterministic | unrecoverable | 1 |
| **Total** |  |  | **37,612** |

## Top 10 evidence gaps

**Source:** [`claims.csv`](claims.csv); the counts below include only non-`reproducible-now` claim values.

| Rank | Report / status | Claims | Gap |
|---:|---|---:|---|
| 1 | [M3-02 parallel Phase 6B](../performance/M3_02_PARALLEL_PHASE6B.md) — `missing-data` | 10,089 | The benchmark source exists, but the report's full raw per-day/per-cell measurement output is not committed. The report's smaller “Interaction workload decay” output is not a substitute for the full cell data. |
| 2 | [M3-11 propagation](../performance/M3_11_PROPAGATION.md) — `missing-data` | 1,971 | The bench targets exist; raw timing/frontier records for these table values are absent. |
| 3 | [M4-03 divergence](../performance/M4_03_DIVERGENCE.md) — `restored-not-rerun` | 1,925 | The divergence and gate examples were restored and raw CSVs survive, but the examples were not rerun to verify the report tables. |
| 4 | [M4-04 CRN efficiency](../performance/M4_04_CRN_EFFICIENCY.md) — `restored-not-rerun` | 1,125 | The CRN example and CSVs survive; outputs have not been regenerated from the restored source. |
| 5 | [M4-02 branching](../performance/M4_02_BRANCHING.md) — `restored-not-rerun` | 879 | A2–A5 examples were restored; A-5's final raw sample file is absent. A-1 and some follow-up sources remain incomplete. |
| 6 | [M2-40 final runtime profile](../performance/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md) — `missing-data` | 469 | The committed baseline bench supports full-tick work, but table-level raw samples are absent; temporary profile probes are not available. |
| 7 | [M3-01 serial baseline](../performance/M3_01_SERIAL_BASELINE.md) — `missing-data` | 453 | The full-tick bench exists. Temporary per-phase profile output and raw paired samples are not committed. |
| 8 | [M3-08 population dynamics](../performance/M3_08_POPULATION_DYNAMICS.md) — `unrecoverable` | 442 | The original measurement driver and its raw timing data are not present. A related workload bench is not evidence that it reproduces these historical measurements. |
| 9 | [M3 sustained workload](../performance/M3_SUSTAINED_WORKLOAD.md) — `missing-data` | 431 | The benchmark source exists, but the original cell output is absent. |
| 10 | [M3-09 incremental topology](../performance/M3_09_INCREMENTAL_TOPOLOGY.md) — `unrecoverable` | 325 | The report used a temporary `m3p_09_measure` driver that is not in the repository; source and raw timing output are absent. |

Other notable gaps include M3-07's temporary cache/TLB and access-pattern diagnosis, M2-31/M2-37 temporary profile probes, M3-06 settlement-index raw measurements, and the M4-01 temporary performance probe. See the complete per-value ledger for the exact claim IDs and uncertain mappings.

## Harness commands that exist and should regenerate claims

The commands below were checked against the current source tree and report command text. **None were run.** “Raw data” names the existing committed output when present; `none` marks the gap rather than implying that a new measurement was made.

| Command | Source harness | Existing raw data | Claims to revisit |
|---|---|---|---|
| `cargo bench --bench m0_baseline_bench` | [`m0_baseline_bench.rs`](../../crates/sim-model/benches/m0_baseline_bench.rs) | none | M2 full-tick and workload-family tables; cannot regenerate removed profile hooks by itself. |
| `python scripts/profile_m0_baseline.py` | [`profile_m0_baseline.py`](../../scripts/profile_m0_baseline.py) | none | M2 baseline profile values, subject to the script's current inputs and the report's original measurement protocol. |
| `cargo bench --bench m3_01_serial_baseline_bench` | [`m3_01_serial_baseline_bench.rs`](../../crates/sim-model/benches/m3_01_serial_baseline_bench.rs) | none | M3-01 full-tick, one-time-cost, volume, and release-differential sections. The temporary per-phase mode is not in the current harness. |
| `cargo bench --bench m3_02_parallel_phase6b_bench` | [`m3_02_parallel_phase6b_bench.rs`](../../crates/sim-model/benches/m3_02_parallel_phase6b_bench.rs) | none | M3-02 full matrix and interaction follow-up; the report's per-day raw records are missing. |
| `cargo bench -p sim-model --bench m3_02b_search_reduction_bench` | [`m3_02b_search_reduction_bench.rs`](../../crates/sim-model/benches/m3_02b_search_reduction_bench.rs) | none | M3-03 search-reduction measurements. |
| `cargo bench -p sim-model --bench m3_equilibrium_bench` | [`m3_equilibrium_bench.rs`](../../crates/sim-model/benches/m3_equilibrium_bench.rs) | none | M3-04 workload search. |
| `cargo bench -p sim-model --bench m3_scale_limit_bench` (plus report-specific `--diagnose`, `--residual`, or `--parallel-only`) | [`m3_scale_limit_bench.rs`](../../crates/sim-model/benches/m3_scale_limit_bench.rs) | none | M3-05/M3-06/M3-07 scale and settlement-index claims. |
| `cargo bench -p sim-model --bench m3_sustained_workload_bench` | [`m3_sustained_workload_bench.rs`](../../crates/sim-model/benches/m3_sustained_workload_bench.rs) | none | M3 sustained workload, not the missing M3-08 measurement driver. |
| `cargo bench -p sim-model --bench m3_11_propagation_bench` and `cargo bench -p sim-model --bench m3_11_m3p_sustained_bench` | [`m3_11_propagation_bench.rs`](../../crates/sim-model/benches/m3_11_propagation_bench.rs), [`m3_11_m3p_sustained_bench.rs`](../../crates/sim-model/benches/m3_11_m3p_sustained_bench.rs) | none | M3-11 propagation and M3P sustained propagation tables. |
| `cargo run --release -p sim-model --example m4_02_a2_measure -- all` (likewise A3 and A4); `cargo run --release -p sim-model --example m4_02_a5_measure` | Restored examples under [`crates/sim-model/examples/`](../../crates/sim-model/examples/) | [`docs/data/m4/m4_02/`](../data/m4/m4_02/) historical files; final A-5 sample file absent | M4-02 restored gate sources; they were not rerun. M4-02 A-1 source is incomplete. |
| `cargo run --release -p sim-model --example m4_03_experiment_temp` and `cargo run --release -p sim-model --example m4_03_gate_temp` | Restored examples under [`crates/sim-model/examples/`](../../crates/sim-model/examples/) | [`docs/data/m4/m4_03/`](../data/m4/m4_03/) | M4-03 divergence and confirmatory gate records. |
| `cargo run --release -p sim-model --example m4_04_crn_experiment` | [`m4_04_crn_experiment.rs`](../../crates/sim-model/examples/m4_04_crn_experiment.rs) (restored) | [`docs/data/m4/m4_04/`](../data/m4/m4_04/) | M4-04 paired/independent CRN tables. |
| `cargo run --release -p sim-model --example m4_05_experiment` and `cargo run --release -p sim-model --example m4_05_a5_gate` | Restored examples under [`crates/sim-model/examples/`](../../crates/sim-model/examples/) | [`docs/data/m4/m4_05/`](../data/m4/m4_05/) | M4-05 search results and gate. The follow-up helper source is incomplete. |
| `cargo run --release -p sim-model --example m4_06_a5_gate`, `m4_06_experiment`, `m4_06_branch_experiment`, and `m4_06_hook_followup` | [`crates/sim-model/examples/`](../../crates/sim-model/examples/) | [`docs/data/m4_06/`](../data/m4_06/) | M4-06 gate and experiment; committed harness/data pairs exist. |
| `cargo run --release -p sim-model --example m4_07_many_branch -- --only=crn` (also `scan`, `search`, and `cow`) | [`m4_07_many_branch.rs`](../../crates/sim-model/examples/m4_07_many_branch.rs) | [`docs/data/m4_07/`](../data/m4_07/) | M4-07 parallel branch, day-scan, minimal-cause, and clone-share results. |

## Proposed M5-03 / M5-04 work order

1. **M5-03, lowest cost:** reproduce M4-06 and M4-07 first because both have committed harness/data pairs; compare generated values to their JSON outputs and keep the existing large files unchanged.
2. **M5-03:** run the restored M4-03/M4-04/M4-05 sources against their committed CSV/JSONL records. Record exact matches and differences; do not treat the restored status as verification. Then rerun M4-02 A2–A5, preserving old protocol runs and capturing new A-5 samples.
3. **M5-04, deterministic checks first:** trace M0/M1 State/Metrics/Events and snapshot claims to their committed tests/goldens; rerun only through the standard suite when that stage is authorized. Resolve the historical M1 test-count claims separately from canonical output claims.
4. **M5-04, timing tables with live harnesses:** rerun M2's current baseline benchmark and the committed M3-01/02/03/04/scale/11 benches. Capture raw outputs under `docs/data/m5_04/`, apply A-5 on the reference host for timing conclusions, and compare each absolute timing with its old value using the frozen new/old ratio rule.
5. **M5-04, missing-probe reconstruction last:** only rebuild the removed drivers needed for M2-31/37/40 and M3-01/08/09 claims that remain decision-relevant. Keep each new driver committed; label old probe values unrecoverable when the original source/data cannot be recovered rather than substituting a related benchmark.

This order prioritizes exact M4 replays and deterministic M0–M3 checks before reconstructing costly temporary profiling work. Multi-seed and multi-workload robustness remains M5-05 and is outside this inventory task.