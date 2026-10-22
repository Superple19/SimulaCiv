# M5-04 viewer exporter replay

| File | Harness and command | Comparison |
|---|---|---|
| [`food_0_h90.jsonl`](food_0_h90.jsonl) | [`m4_branch_trace.rs`](../../../../crates/sim-model/examples/m4_branch_trace.rs); `cargo run --release -p sim-model --example m4_branch_trace -- --case food_0 --horizon 90 --output target/m5_04/viewer/food_0_h90.jsonl` | Byte-exact with `docs/data/m4_viewer/food_0_h90.jsonl`; SHA-256: `a6c844a37bfcfb1c155a877e6cd4efbcfaa9d80446c5ef0a176653add70fe918`. |
