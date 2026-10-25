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
