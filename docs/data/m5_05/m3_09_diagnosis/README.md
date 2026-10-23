# M3-09 ratio diagnosis data

The generated [M5-05 report diagnosis section](../../../reproducibility/M5_05_M0_M3_REGENERATION.md#m3-09-ratio-diagnosis) uses the committed summaries and DVC-managed raw samples below.

| Report table | Measurement harness | Git summary | DVC raw data |
|---|---|---|---|
| Historical numerator/denominator decomposition | [M3-09 report parser](../../../../analysis/simciv_analysis/m5_05_diagnosis.py) | [M3-09 M5-05 summary](../m3_09_incremental_equal_live/summary.json) | — |
| Five-revision A-5 table and adjacent changes | [m5_05_m3_09_revision.rs](../../../../crates/sim-model/examples/m5_05_m3_09_revision.rs) | [diagnosis summary](summary.json) | [samples.jsonl](raw/samples.jsonl) |
| Bisect candidates and first changed commit | [m5_05_m3_09_bisect.py](../../../../crates/sim-model/examples/m5_05_m3_09_bisect.py) | [bisect summary](bisect_summary.json) | [candidate raw sample](bisect_raw/78181e21aad0.json); all candidates are in `bisect_raw/` |

The execution plans are [m3_09_diagnosis.toml](../../../reproducibility/m5_05_configs/m3_09_diagnosis.toml) and [m3_09_diagnosis_bisect.toml](../../../reproducibility/m5_05_configs/m3_09_diagnosis_bisect.toml). Reproduce the recorded data only by explicitly running the corresponding DVC stage; raw outputs are not committed to Git.
