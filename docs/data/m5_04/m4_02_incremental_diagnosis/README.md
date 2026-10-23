# M4-02 incremental M3P diagnosis

| File | Producer | Use |
|---|---|---|
| [`paired_samples.tsv`](paired_samples.tsv) | [`m4_02_a5_measure.rs`](../../../../crates/sim-model/examples/m4_02_a5_measure.rs) streamed by [`m4_02_incremental_diagnosis.ps1`](../../../../crates/sim-model/examples/m4_02_incremental_diagnosis.ps1) | Interleaved 41-sample A-5 raw pairs for the M4-02 PASS commit, `v0.5.0-m4`, and the pre-follow-up HEAD. Both M3P incremental cells use `BranchExecutionOption::IncrementalTopology` on direct and branch paths. |
| [`run.log`](run.log) | Same PowerShell coordinator | Timing markers, CPU/RAM samples, process priority/affinity, and per-build gate summaries. |
| [`superseded/serial_branch_request/`](superseded/serial_branch_request/) | Same harness before the measurement-path correction | Preserved diagnostic run where the direct path used incremental topology but the branch request used serial execution. It is not a valid apples-to-apples A-5 result. |

The corrected interleaved run gives the same PASS verdict for each revision and cell. This identifies the earlier M5-04 FAIL as a measurement-harness path mismatch, not a production regression or host/session change. The generated [M5-04 report](../../../reproducibility/M5_04_M4_REGENERATION.md#m4-02-incremental-diagnosis) computes the table directly from `paired_samples.tsv`.
