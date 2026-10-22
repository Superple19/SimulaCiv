# M5-04 M4-06 regenerated data

| File | Harness and command | Comparison |
|---|---|---|
| [`experiment.json.gz`](experiment.json.gz) | [`m4_06_experiment.rs`](../../../../crates/sim-model/examples/m4_06_experiment.rs); `cargo run --release -p sim-model --example m4_06_experiment -- target/m5_04/m4_06/experiment.json` | Byte-exact with `docs/data/m4_06/experiment.json`; SHA-256 of committed gzip: `1dee1446fd98ea4c1b2d96a42467eccd2287b31f27b09b9638de1bfc3a3bb926` |
| [`branch_experiment.json.gz`](branch_experiment.json.gz) | [`m4_06_branch_experiment.rs`](../../../../crates/sim-model/examples/m4_06_branch_experiment.rs); `cargo run --release -p sim-model --example m4_06_branch_experiment -- target/m5_04/m4_06/branch_experiment.json` | Byte-exact with `docs/data/m4_06/branch_experiment.json`; SHA-256 of committed gzip: `2a7c53a7c061705cf7b8abe9ee162e846deb5e89446bca0993b38250d7a537a5` |
| [`hook_followup.json`](hook_followup.json) | [`m4_06_hook_followup.rs`](../../../../crates/sim-model/examples/m4_06_hook_followup.rs); `cargo run --release -p sim-model --example m4_06_hook_followup -- target/m5_04/m4_06/hook_followup.json` | Fresh 2-warmup/41-sample descriptive A-5-style hook-cost run with high priority and affinity `0x555`; time distributions differ from the committed prior run. SHA-256: `3fa90ca94c0379e8c1c404f12f12b0031f469b6ccf4145ef5b7586fecaf18457`. |
