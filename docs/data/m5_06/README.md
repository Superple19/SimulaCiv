# M5-06 Phase 3 deterministic rerun data

Raw outputs from deterministic report-revision reruns. No A-5 timing series were run. Files are DVC-tracked; until upload, the raw bytes are available only in the maintainer's local DVC cache. After upload, restore with `dvc pull docs/data/m5_06/phase3_raw.dvc`.

| Raw file(s) | Report revision | Harness and protocol |
|---|---|---|
| `m4_03_a1_experiments.csv` | `bfe162baee95130a268707fef6e1489ef96cc414` | `crates/sim-model/examples/m4_03_experiment.rs`, registered SHA-256 overlay `ab2b02a54607d01c9b2d0459cc83e96d3ab37e18aabd8556b6f085c51aba66f1`; H=90 and H=365 deterministic divergence rows. |
| `m4_05_*.jsonl` | `6b925b91d565d98706fe6cac3cdf14164abc7a32` | `crates/sim-model/examples/m4_05_experiment.rs`, registered SHA-256 overlay `baa21e7e4b9e88dadf37edf4e41146f0f55933b18102a66842955cebb1f2be78`; case (a), case (b), case (c), and the direct full-set probe. |
| `experiment.json` | `392f6064b6a5ac4132722af0dc8941e8d3908d9d` | `crates/sim-model/examples/m4_06_experiment.rs`, registered SHA-256 overlay `267ca9ea90ee3d89d183453652f7e0d12e26e427fac1ee1a67f3c4998688f1be`. |
| `m4_04_739/*.csv` | `739004cf6eee9c03d6f462f1852dcf10ffe43293` | `crates/sim-model/examples/m4_04_crn_experiment.rs`, registered SHA-256 overlay `9e552d89704611a7223f61cf2eda9e638930b2e7a06d19a7ff3ab6c7cbb773fc`; all 3 interventions, 4 horizons, 5 outcomes. |
| `m4_04_477/*.csv` | `477bbc9bb716a34a6942b4bd7032b2d8ad46a791` | Same registered M4-04 harness overlay and full configuration. Every CSV is byte-identical to the matching `m4_04_739/` file. |

The repository-side evidence logs and per-cell exact-match results are in `docs/reproducibility/evidence/m5_06_phase3/` and `contracts/evidence.jsonl`.

## Phase 4 deterministic M4-05 enumeration

| Output | Report revision | Harness and command |
|---|---|---|
| `phase4_raw/m4_05_case_a_size4.jsonl` | `6b925b91d565d98706fe6cac3cdf14164abc7a32` | `crates/sim-model/examples/m4_05_experiment.rs` overlay from `4218d39`; `cargo +1.98.1 run --locked --release -p sim-model --example m4_05_experiment -- --enumerate-size4`. Raw output is under `phase4_raw/m4_05_case_a_size4.jsonl.dvc`; it is local-cache-only until DVC upload.
