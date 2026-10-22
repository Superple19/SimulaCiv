# M5-04 M4-03 regenerated data

| File | Harness and command | Comparison |
|---|---|---|
| [`m4_03_a1_experiments.csv.gz`](m4_03_a1_experiments.csv.gz) | [`m4_03_experiment.rs`](../../../../crates/sim-model/examples/m4_03_experiment.rs); `cargo run --release -p sim-model --example m4_03_experiment` | Byte-exact with `docs/data/m4_03/m4_03_a1_experiments.csv`; SHA-256 of committed gzip: `7239412fcd72dee423df8ba728fbd9dcdacad91166a3185be158d5c30a760489`. |
| [`m4_03_gate_a5.log`](m4_03_gate_a5.log) | [`m4_03_gate.rs`](../../../../crates/sim-model/examples/m4_03_gate.rs); `cargo build --release -p sim-model --example m4_03_gate`, then run its executable at High priority and affinity `0x555` | Fresh three-cell A-5 samples with timing markers; compare conclusions with `docs/data/m4_03/m4_03_gate_rerun.csv`. SHA-256: `0c0bd1e1c573a3ef3aad1eb272401217cf6808e423a63d8eae85ea59f8de1a57`. |
