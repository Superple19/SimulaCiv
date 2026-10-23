# M5-04 M4-02 A-5 rerun data

See the [incremental M3P same-session diagnosis](../m4_02_incremental_diagnosis/README.md) for the follow-up that corrected the branch execution mode and retested the two incremental cells across the M4-02 PASS commit, release tag, and pre-follow-up HEAD.

| File | Harness and command | Result |
|---|---|---|
| [`m4_02_a5_samples.tsv`](m4_02_a5_samples.tsv) | [`m4_02_a5_measure.rs`](../../../../crates/sim-model/examples/m4_02_a5_measure.rs); build with `cargo build --release -p sim-model --example m4_02_a5_measure`, then run the executable with High priority and affinity `0x555` | Nine cells, 41 samples per cell. SHA-256: `3f5a5edc5845bb2145b0212943904fd7002e335a536e6f5c4a47f74f1c176c86`. |
| [`m4_02_a5.log`](m4_02_a5.log) | Same executable and controls | Contains `TIMING RUN START/END`, controls, repetitions-per-sample, gate statistics, and verdicts. SHA-256: `064a8191a43493e970b8b056ddd786b9306b124e43e626c850999f933237aa76`. |
