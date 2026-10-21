# M3 trace viewer

Generate JSONL traces with the existing M3 storage runners, then open `index.html` in a browser and load the files with the picker or drag-and-drop. The viewer is static and makes no network requests.

```sh
cargo run --release -p sim-model --example m3_trace -- --config m2_40 --n 10000 --settlements 50 --k 8 --p-local 0.5 --days 200 --seed 81985529216486895 --replicate 7 --runner serial --output target/traces/m2_40_serial.jsonl
cargo run --release -p sim-model --example m3_trace -- --config m2_40 --n 10000 --settlements 50 --k 8 --p-local 0.5 --days 200 --seed 81985529216486895 --replicate 7 --runner search_reduction --output target/traces/m2_40_search_reduction.jsonl
cargo run --release -p sim-model --example m3_trace -- --config sustained --n 10000 --settlements 50 --k 8 --p-local 0.5 --days 200 --seed 81985529216486895 --replicate 7 --runner serial --output target/traces/sustained_serial.jsonl
cargo run --release -p sim-model --example m3_trace -- --config sustained --n 10000 --settlements 50 --k 8 --p-local 0.5 --days 200 --seed 81985529216486895 --replicate 7 --runner search_reduction --output target/traces/sustained_search_reduction.jsonl
```

Use `--config <path.toml>` for a custom TOML config; the trace command overrides profile, population, settlement count, relationship degree, locality, seed, and replicate from its arguments. Parallel runs use `--runner parallel:<threads>:<chunk>`.

Count charts use zero-based axes with 1/2/5-step ticks. Undefined measurements appear as line gaps; identical metric traces are marked in the legend and overlaid with a dashed line, while each same-config group reports whether its final digests match.

## M4 branch comparison

Select **Branch comparison** in the viewer, then load or drop an M4 branch JSONL file. A sample is committed at [`docs/data/m4_viewer/food_0_h90.jsonl`](../docs/data/m4_viewer/food_0_h90.jsonl). The branch view shows base/branch daily metrics and alive counts, divergence curves and first-divergence/cascade markers, component flags, and the per-settlement heatmap. All parsing and rendering stays local in the browser.

Generate any of the M4-03 M3P sustained cases (`food_0` or `health_0.01`, horizon 90 or 365) with:

```sh
cargo run --release -p sim-model --example m4_branch_trace -- --case food_0 --horizon 90 --output target/m4_viewer/food_0_h90.jsonl
cargo run --release -p sim-model --example m4_branch_trace -- --case health_0.01 --horizon 365 --output target/m4_viewer/health_0_01_h365.jsonl
```

The exporter checks the M4-03 branch result against a separate M4-02 branch API result before writing the trace.
