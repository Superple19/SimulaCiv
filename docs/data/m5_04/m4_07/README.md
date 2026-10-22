# M5-04 M4-07 regenerated data

| File | Harness and command | Comparison |
|---|---|---|
| [`crn_parallel_only.jsonl.gz`](crn_parallel_only.jsonl.gz) | [`m4_07_many_branch.rs`](../../../../crates/sim-model/examples/m4_07_many_branch.rs), `--only=crn-parallel` | All three M4-04 estimate arrays match the committed serial/parallel records in `docs/data/m4_07/crn_parallel.jsonl`; M5-04 executed the parallel mode only. Gzip SHA-256: `f3fc5360215a9952bf78d26e829b758bd3793b3ad18878f64d42c1bef877f05e`. |
| [`day_scan.jsonl.gz`](day_scan.jsonl.gz) | [`m4_07_many_branch.rs`](../../../../crates/sim-model/examples/m4_07_many_branch.rs), `--only=scan` | Deterministic scan fields for N=10k and 100k match `docs/data/m4_07/day_scan.jsonl`; timing fields are one-sample observations, not A-5 measurements. Gzip SHA-256: `8117d5fdcd5bbfa593112c3490484c4bc30f78f89f1bf7b44f6918daab189f09`. |
