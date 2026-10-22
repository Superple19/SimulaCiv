# M5-04 M4-05 regenerated data

| File | Harness and command | Comparison |
|---|---|---|
| [`m4_05_a_health8.jsonl`](m4_05_a_health8.jsonl) | [`m4_05_experiment.rs`](../../../../crates/sim-model/examples/m4_05_experiment.rs); `cargo run --release -p sim-model --example m4_05_experiment` | Cause IDs, evaluation counts, and effects match the committed run; schema version and guarantee enum casing differ. SHA-256: `25b52b8af6733025effaffffdc2cc02106178970179f12b53c416ec9e3b85c62`. |
| [`m4_05_a_health8_full_probe.jsonl`](m4_05_a_health8_full_probe.jsonl) | same harness/command | Deterministic result fields match except `guarantee` was renamed `None` → `incomplete`; raw run retained. SHA-256: `7ef8a90ba246fd18cf50bc7006aa42b3e24acd32b4e98e7899baff9ac4cb89d9`. |
| [`m4_05_b_health16.jsonl`](m4_05_b_health16.jsonl) | same harness/command | Cause IDs, evaluation counts, and effects match; schema version and `OneMinimal` → `one_minimal` differ. SHA-256: `921fe90d2c0cefb03034f33062a52188c1313efc2dff48eb546b1933cb185c2d`. |
| [`m4_05_c_config6.jsonl`](m4_05_c_config6.jsonl) | same harness/command | Exhaustive result fields match except `guarantee` was renamed `None` → `exhausted_no_cause`; raw run retained. SHA-256: `34f5ca18578eede43b940c5c46071bead02808d158876b9c28c98119a9972aba`. |
| [`m4_05_a5_summary_only.log`](m4_05_a5_summary_only.log) | [`m4_05_a5_gate.rs`](../../../../crates/sim-model/examples/m4_05_a5_gate.rs); controlled M5-04 A-5 invocation | Completed A-5 aggregate output; superseded by the next log, which also retains all sample pairs. SHA-256: `f8ff48955d16fb647e6f537d2ad665fd032d660c87e02c6cdbec399637152bc2`. |

Seed-specific follow-up files under `docs/data/m4_05/seed-*.jsonl` were not regenerated because their dedicated helper source is incomplete; they remain historical and are classified separately in the M5-04 report.
