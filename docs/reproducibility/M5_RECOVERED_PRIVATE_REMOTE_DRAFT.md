# Recovered artifact remote policy (draft)

Status: DRAFT — decision required before any DVC upload.

Proposed decision: compiled binaries, debug symbols, session extracts, and user-temporary artifacts listed below should be uploaded only to a private remote or excluded from the public remote. No artifact was changed or moved for this draft.

## Exact manifest paths under review

### Compiled binaries and debug symbols

- `recovered/windows/target/debug/examples/m3p_09_measure.exe`
- `recovered/windows/target/debug/examples/m3p_09_measure.pdb`
- `recovered/windows/target/debug/examples/m4_02_measure.exe`
- `recovered/windows/target/debug/examples/m4_02_measure.pdb`
- `recovered/windows/target/debug/examples/m4_03_experiment_temp.exe`
- `recovered/windows/target/debug/examples/m4_03_experiment_temp.pdb`
- `recovered/windows/target/debug/examples/m4_03_gate_temp.exe`
- `recovered/windows/target/debug/examples/m4_03_gate_temp.pdb`
- `recovered/windows/target/release/examples/m2_37_profile.exe`
- `recovered/windows/target/release/examples/m2_37_profile.pdb`
- `recovered/windows/target/release/examples/m3_generation_timing.exe`
- `recovered/windows/target/release/examples/m3_generation_timing.pdb`
- `recovered/windows/target/release/examples/m3p_09_diagnose.exe`
- `recovered/windows/target/release/examples/m3p_09_diagnose.pdb`
- `recovered/windows/target/release/examples/m3p_09_followup_measure.exe`
- `recovered/windows/target/release/examples/m3p_09_followup_measure.pdb`
- `recovered/windows/target/release/examples/m3p_09_measure.exe`
- `recovered/windows/target/release/examples/m3p_09_measure.pdb`
- `recovered/windows/target/release/examples/m3p_measure.exe`
- `recovered/windows/target/release/examples/m3p_measure.pdb`
- `recovered/windows/target/release/examples/m3p_sustained_search.exe`
- `recovered/windows/target/release/examples/m3p_sustained_search.pdb`
- `recovered/windows/target/release/examples/m4_02_a1_measure.exe`
- `recovered/windows/target/release/examples/m4_02_a1_measure.pdb`
- `recovered/windows/target/release/examples/m4_02_followup_measure.exe`
- `recovered/windows/target/release/examples/m4_02_followup_measure.pdb`
- `recovered/windows/target/release/examples/m4_02_measure.exe`
- `recovered/windows/target/release/examples/m4_02_measure.pdb`
- `recovered/windows/target/release/examples/m4_03_experiment_temp.exe`
- `recovered/windows/target/release/examples/m4_03_experiment_temp.pdb`
- `recovered/windows/target/release/examples/m4_03_gate_temp.exe`
- `recovered/windows/target/release/examples/m4_03_gate_temp.pdb`
- `recovered/windows/target/release/examples/m4_05_followup_measure.exe`
- `recovered/windows/target/release/examples/m4_05_followup_measure.pdb`
- `recovered/windows/target/release/examples/m4_05_subset_verify.exe`
- `recovered/windows/target/release/examples/m4_05_subset_verify.pdb`

### Session extracts

- `recovered/session-extracts.jsonl`

### User-temporary files and captured temporary sources

- `recovered/session-history/C/Users/chacj/AppData/Local/Temp/simulaciv_m2_31_probe.rs`
- `recovered/session-history/C/Users/chacj/AppData/Local/Temp/simulaciv_m2_31_profile.rs`
- `recovered/windows/user-temp/AppData/Local/Temp/build_m3_02_followup_report.py`
- `recovered/windows/user-temp/AppData/Local/Temp/m3_02_followup_workspace_tests.txt`
- `recovered/windows/user-temp/AppData/Local/Temp/m3_02_interaction_followup_raw.txt`
- `recovered/windows/user-temp/AppData/Local/Temp/m3_02_parallel_phase6b_bench.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4_01_step_a_m3p_noise.err.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4_01_step_a_m3p_noise.out.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4_01_step_a_other_cells.err.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4_01_step_a_other_cells.out.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4_01_step_e_noise_gate.err.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4_01_step_e_noise_gate.out.log`
- `recovered/windows/user-temp/AppData/Local/Temp/m4-05-golden-21620.jsonl`
- `recovered/windows/user-temp/AppData/Local/Temp/m4-05-golden-21984.jsonl`
- `recovered/windows/user-temp/AppData/Local/Temp/m4-05-incomplete-21984.jsonl`
- `recovered/windows/user-temp/AppData/Local/Temp/simciv-cargo-test-after.log`
- `recovered/windows/user-temp/AppData/Local/Temp/simciv-cargo-test-before.log`
- `recovered/windows/user-temp/AppData/Local/Temp/simulaciv-m4-07-parallel-32712.jsonl`
- `recovered/windows/user-temp/AppData/Local/Temp/simulaciv-m4-07-serial-32712.jsonl`

### Additional paths found by the read-only DVC-output scan

These directory prefixes include the additional recovered items whose bytes contain the absolute checkout path or whose manifest provenance points to the maintainer's Codex history. They are included as subtrees so newly discovered files under those roots remain covered.

- `recovered/windows/target/`
- `recovered/dvc-cache/materialized-paths/`
- `recovered/session-history/C/AI/SimulaCiv/crates/sim-model/examples/`
- `recovered/source-snapshots/`
- `recovered/session-history/C/Users/chacj/AppData/Local/Temp/`
- `recovered/dvc-cache/unreferenced-objects/files/md5/48/3011236336943a15456ac74de276cf`
- `recovered/dvc-cache/unreferenced-objects/files/md5/7c/9997044a521e678295127d24eead27`
- `recovered/dvc-cache/unreferenced-objects/files/md5/a0/ef891b7466fe535aeb6d4b394c92dd`

The scan also found these separately DVC-tracked raw logs with the absolute checkout path in their contents:

- `docs/data/m5_05/m2_final_tick/raw/run.log`
- `docs/data/m5_05/m2_near_linear/raw/run.log`
- `docs/data/m5_05/m2_phase4_crossover/raw/run.log`
- `docs/data/m5_05/m3_01_serial_baseline/raw/run.log`
- `docs/data/m5_05/m3_02_parallel_phase6b/raw/run.log`
- `docs/data/m5_05/m3_03_search_reduction/raw/run.log`
- `docs/data/m5_05/m3_06_settlement_index/raw/run.log`
- `docs/data/m5_05/m3_07_reversed_slots/raw/run.log`
- `docs/data/m5_05/m3_08_full_rebuild_equal_live/raw/run.log`
- `docs/data/m5_05/m3_09_diagnosis/raw/build_logs/m3_09_followup.stderr.txt`
- `docs/data/m5_05/m3_09_diagnosis/raw/build_logs/m3_release.stderr.txt`
- `docs/data/m5_05/m3_09_diagnosis/raw/build_logs/m4_01_registry.stderr.txt`
- `docs/data/m5_05/m3_09_diagnosis/raw/build_logs/m4_release.stderr.txt`
- `docs/data/m5_05/m3_09_diagnosis/raw/build_logs/task_start_head.stderr.txt`
- `docs/data/m5_05/m3_09_incremental_equal_live/raw/run.log`
- `docs/data/m5_05/m3_11_frontier_crossover/raw/run.log`
- `docs/data/m5_05/m3_11_phase9c_speedup/raw/run.log`
- `docs/data/m5_05/m3_11_whole_tick_speedup/raw/run.log`

The Git-tracked [stage manifest](../data/m5_recovered/MANIFEST.json) also contains `items[].source_location` provenance referring to the maintainer's Codex history database. The manifest bytes remain unchanged; this field is a separate privacy-review item outside the DVC output tree.
