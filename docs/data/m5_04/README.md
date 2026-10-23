# M5-04 M4 regeneration data

All files in this stage directory are at most 1 MiB; no release assets or manifest are needed. The generated result tables are in [`M5-04 regeneration`](../../reproducibility/M5_04_M4_REGENERATION.md). [`claim_statuses.json`](claim_statuses.json) carries the generated M5-04 result classification into the reproducibility ledger.

| Stage directory | Main report tables | Harness/data index |
|---|---|---|
| [`m4_02/`](m4_02/README.md) and [`m4_02_incremental_diagnosis/`](m4_02_incremental_diagnosis/README.md) | A-5 gate status, per-cell timing ratios, and interleaved incremental-path diagnosis | `m4_02_a5_measure.rs`; M5-04 A-5 samples and same-session three-revision pairs |
| [`m4_03/`](m4_03/README.md) | deterministic divergence result, A-5 gate and timing ratios, divergence figure | `m4_03_experiment.rs`, `m4_03_gate.rs`; `m4_03_a1_experiments.csv.gz`, `m4_03_gate_a5.log` |
| [`m4_04/`](m4_04/README.md) | field-by-field CRN comparison and serialization mismatch list | `m4_07_many_branch.rs --only=crn-parallel`; `parallel_estimate_comparison.json` |
| [`m4_05/`](m4_05/README.md) | minimal-cause deterministic comparison, A-5 gate and ratios | `m4_05_experiment.rs`, `m4_05_a5_gate.rs`; JSONL reruns and A-5 logs |
| [`m4_06/`](m4_06/README.md) | deterministic experiment parity, hook timing, A-5 gate and ratios | M4-06 committed examples; JSON/JSONL outputs and gate log |
| [`m4_07/`](m4_07/README.md) | parallel CRN, day-scan, search, and clone-share comparisons | `m4_07_many_branch.rs`; four rerun output files |
| [`viewer/`](viewer/README.md) | viewer exporter parity | `m4_branch_trace.rs`; `food_0_h90.jsonl` |

The report generator and its derived comparison/figures use [`report_m5_04.py`](../../../analysis/simciv_analysis/report_m5_04.py). The M4-04 comparison JSON and the generated claims statuses are deterministic report inputs, not simulation outputs.
