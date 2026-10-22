# M4-02–M4-05 artifact inventory

The inventory covers the repository target/ tree, the M4 example binary directories, and the current-user temporary directory. Raw CSV/JSON/JSONL found in those locations is copied under this directory. Compiled executables and PDBs are listed but kept in ignored target/; they are not reproducible source artifacts.

## Report-table coverage

| Report | Table / evidence block | Raw data survives? | Harness source survives? | Location / note |
|---|---|---:|---:|---|
| M4-02 | Historical paired orchestration results | Yes | Partial | ../m4_02/m4_02_raw.csv, ../m4_02/m4_02_final.csv; initial m4_02_measure.rs history was truncated |
| M4-02 | Follow-up paired samples | Yes | No complete source | ../m4_02/m4_02_followup.csv; compiled harness remains |
| M4-02 | Recorder probe | Yes | No complete source | ../m4_02/m4_02_recorder.csv |
| M4-02 | Worked-example day curve | Yes | No complete source | ../m4_02/m4_02_worked.csv |
| M4-02 | A-2 host/load records | Yes | Yes | ../m4_02/m4_02_a2_*; m4_02_a2_measure.rs restored |
| M4-02 | A-3 host/load records | Yes | Yes | ../m4_02/m4_02_a3_official_*; m4_02_a3_measure.rs restored |
| M4-02 | A-4 initial/retry host records | Yes | Yes | ../m4_02/m4_02_a4_*; per-cell samples absent; m4_02_a4_measure.rs restored |
| M4-02 | A-5 final nine-cell gate and recorder comparison | No raw sample file | Yes | m4_02_a5_measure.rs restored; compiled binary survives |
| M4-03 | Original/historical divergence series | Yes | Yes | ../m4_03/m4_03_divergence_experiments.csv; m4_03_experiment.rs restored |
| M4-03 | A-1 corrected daily series, settlement spread, H=365 | Yes | Yes | ../m4_03/m4_03_a1_experiments.csv; same restored source |
| M4-03 | Confirmatory gate | Yes | Yes | ../m4_03/m4_03_gate_rerun.csv; m4_03_gate.rs restored |
| M4-04 | Food=0 result table | Yes | Yes | ../m4_04/m4_04_crn_food_0.csv; m4_04_crn_experiment.rs restored |
| M4-04 | Health=0.01 result table | Yes | Yes | ../m4_04/m4_04_crn_health_0_01.csv; same source |
| M4-04 | BaseMetabolicCost+1% result table | Yes | Yes | ../m4_04/m4_04_crn_base_metabolic_cost_plus_1pct.csv; same source |
| M4-05 | A-5 batch-of-one gate | No raw sample file | Yes | m4_05_a5_gate.rs restored; report retains summary |
| M4-05 | Initial minimal-cause cases (a)/(b)/(c) | Yes | Yes | ../m4_05/m4_05_*.jsonl; m4_05_experiment.rs restored |
| M4-05 | Full-set probe and seed non-uniqueness | Yes | Partial | full probe and seed JSONL survive; follow-up source does not survive intact |
| M4-05 | Golden and incomplete-path probes | Yes | No complete source | copied from %TEMP%; binaries for follow-up helpers survive under target/release/examples |

Restored examples include the required first-line note restored from session history; not re-run. cargo check -p sim-model --examples was run; no restored experiment was executed. Source recovery was limited by the history reader truncating the original M4-02 A1 source and not returning complete M4-02 initial/follow-up or M4-05 follow-up helper source.

## Recovered raw data and original metadata

| Original path | Bytes | Last write (UTC) | Repository copy |
|---|---:|---|---|
| C:\AI\SimulaCiv\target\m4_02_a2_first_run_host.json | 816 | 2026-10-19T17:21:22.397Z | ../m4_02/m4_02_a2_first_run_host.json |
| C:\AI\SimulaCiv\target\m4_02_a2_first_run_load.csv | 3801 | 2026-10-19T17:21:21.371Z | ../m4_02/m4_02_a2_first_run_load.csv |
| C:\AI\SimulaCiv\target\m4_02_a2_host.json | 816 | 2026-10-19T17:21:22.397Z | ../m4_02/m4_02_a2_host.json |
| C:\AI\SimulaCiv\target\m4_02_a2_load.csv | 3801 | 2026-10-19T17:21:21.371Z | ../m4_02/m4_02_a2_load.csv |
| C:\AI\SimulaCiv\target\m4_02_a2_retry_host.json | 925 | 2026-10-19T17:34:40.739Z | ../m4_02/m4_02_a2_retry_host.json |
| C:\AI\SimulaCiv\target\m4_02_a2_retry_load.csv | 3840 | 2026-10-19T17:34:39.723Z | ../m4_02/m4_02_a2_retry_load.csv |
| C:\AI\SimulaCiv\target\m4_02_a3_official_host.json | 984 | 2026-10-20T00:10:25.019Z | ../m4_02/m4_02_a3_official_host.json |
| C:\AI\SimulaCiv\target\m4_02_a3_official_load.csv | 4767 | 2026-10-20T00:10:24.006Z | ../m4_02/m4_02_a3_official_load.csv |
| C:\AI\SimulaCiv\target\m4_02_a4_initial_host.json | 494 | 2026-10-20T01:25:07.063Z | ../m4_02/m4_02_a4_initial_host.json |
| C:\AI\SimulaCiv\target\m4_02_a4_retry_host.json | 555 | 2026-10-20T01:26:07.885Z | ../m4_02/m4_02_a4_retry_host.json |
| C:\AI\SimulaCiv\target\m4_02_final.csv | 10558 | 2026-10-19T10:02:39.705Z | ../m4_02/m4_02_final.csv |
| C:\AI\SimulaCiv\target\m4_02_followup.csv | 11585 | 2026-10-19T11:33:54.734Z | ../m4_02/m4_02_followup.csv |
| C:\AI\SimulaCiv\target\m4_02_raw.csv | 5836 | 2026-10-19T09:24:25.656Z | ../m4_02/m4_02_raw.csv |
| C:\AI\SimulaCiv\target\m4_02_recorder.csv | 278 | 2026-10-19T09:36:06.347Z | ../m4_02/m4_02_recorder.csv |
| C:\AI\SimulaCiv\target\m4_02_worked.csv | 310 | 2026-10-19T09:41:04.185Z | ../m4_02/m4_02_worked.csv |
| C:\AI\SimulaCiv\target\m4_03_a1_experiments.csv | 1489821 | 2026-10-20T10:11:48.133Z | ../m4_03/m4_03_a1_experiments.csv |
| C:\AI\SimulaCiv\target\m4_03_divergence_experiments.csv | 184607 | 2026-10-20T08:35:33.384Z | ../m4_03/m4_03_divergence_experiments.csv |
| C:\AI\SimulaCiv\target\m4_03_gate_rerun.csv | 6964 | 2026-10-20T10:05:32.831Z | ../m4_03/m4_03_gate_rerun.csv |
| C:\AI\SimulaCiv\target\m4_04_crn_base_metabolic_cost_plus_1pct.csv | 15623 | 2026-10-20T14:05:53.303Z | ../m4_04/m4_04_crn_base_metabolic_cost_plus_1pct.csv |
| C:\AI\SimulaCiv\target\m4_04_crn_efficiency.csv | 15623 | 2026-10-20T14:05:53.304Z | ../m4_04/m4_04_crn_efficiency.csv |
| C:\AI\SimulaCiv\target\m4_04_crn_food_0.csv | 5111 | 2026-10-20T13:11:33.836Z | ../m4_04/m4_04_crn_food_0.csv |
| C:\AI\SimulaCiv\target\m4_04_crn_health_0_01.csv | 10183 | 2026-10-20T13:38:40.074Z | ../m4_04/m4_04_crn_health_0_01.csv |
| C:\AI\SimulaCiv\target\m4_05_a_health8_full_probe.jsonl | 1954 | 2026-10-20T16:48:29.635Z | ../m4_05/m4_05_a_health8_full_probe.jsonl |
| C:\AI\SimulaCiv\target\m4_05_a_health8.jsonl | 27503 | 2026-10-20T16:48:26.955Z | ../m4_05/m4_05_a_health8.jsonl |
| C:\AI\SimulaCiv\target\m4_05_b_health16.jsonl | 6569 | 2026-10-20T16:48:50.562Z | ../m4_05/m4_05_b_health16.jsonl |
| C:\AI\SimulaCiv\target\m4_05_c_config6.jsonl | 19591 | 2026-10-20T16:50:14.261Z | ../m4_05/m4_05_c_config6.jsonl |
| C:\AI\SimulaCiv\target\m4_05_followup\seed-0.jsonl | 82943 | 2026-10-20T18:05:51.234Z | ../m4_05/seed-0.jsonl |
| C:\AI\SimulaCiv\target\m4_05_followup\seed-1.jsonl | 83280 | 2026-10-20T18:10:39.112Z | ../m4_05/seed-1.jsonl |
| C:\AI\SimulaCiv\target\m4_05_followup\seed-3.jsonl | 12500 | 2026-10-20T18:11:27.684Z | ../m4_05/seed-3.jsonl |
| C:\Users\chacj\AppData\Local\Temp\m4-05-golden-21620.jsonl | 1275 | 2026-10-20T16:08:29.509Z | ../m4_05/temp_m4-05-golden-21620.jsonl |
| C:\Users\chacj\AppData\Local\Temp\m4-05-golden-21984.jsonl | 1548 | 2026-10-20T16:15:21.540Z | ../m4_05/temp_m4-05-golden-21984.jsonl |
| C:\Users\chacj\AppData\Local\Temp\m4-05-incomplete-21984.jsonl | 546 | 2026-10-20T16:15:12.347Z | ../m4_05/temp_m4-05-incomplete-21984.jsonl |

## Compiled M4 example binaries and symbols

These are build outputs, not source; paths and timestamps are from the local target/ tree.

| Original path | Bytes | Last write (UTC) |
|---|---:|---|
| C:\AI\SimulaCiv\target\debug\examples\m4_02_measure.exe | 5851136 | 2026-10-19T10:02:56.429Z |
| C:\AI\SimulaCiv\target\debug\examples\m4_02_measure.pdb | 42438656 | 2026-10-19T10:02:56.432Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a1_measure.exe | 2029056 | 2026-10-19T12:21:19.497Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a1_measure.pdb | 2191360 | 2026-10-19T12:21:19.498Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a2_measure.exe | 1960448 | 2026-10-19T17:08:45.429Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a2_measure.pdb | 2199552 | 2026-10-19T17:08:45.431Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a3_measure.exe | 1977344 | 2026-10-19T23:57:03.254Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a3_measure.pdb | 2199552 | 2026-10-19T23:57:03.258Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a4_measure.exe | 1969152 | 2026-10-20T01:00:34.116Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a4_measure.pdb | 2183168 | 2026-10-20T01:00:34.119Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a5_measure.exe | 1972736 | 2026-10-20T02:34:08.515Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_a5_measure.pdb | 2199552 | 2026-10-20T02:34:08.518Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_followup_measure.exe | 1972224 | 2026-10-19T11:17:51.519Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_followup_measure.pdb | 2199552 | 2026-10-19T11:17:51.521Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_measure.exe | 1980928 | 2026-10-19T09:40:54.796Z |
| C:\AI\SimulaCiv\target\release\examples\m4_02_measure.pdb | 2150400 | 2026-10-19T09:40:54.797Z |
| C:\AI\SimulaCiv\target\release\examples\m4_03_experiment_temp.exe | 1971200 | 2026-10-20T10:08:12.311Z |
| C:\AI\SimulaCiv\target\release\examples\m4_03_experiment_temp.pdb | 2199552 | 2026-10-20T10:08:12.314Z |
| C:\AI\SimulaCiv\target\release\examples\m4_03_gate_temp.exe | 1979392 | 2026-10-20T09:42:31.639Z |
| C:\AI\SimulaCiv\target\release\examples\m4_03_gate_temp.pdb | 2224128 | 2026-10-20T09:42:31.641Z |
| C:\AI\SimulaCiv\target\release\examples\m4_04_crn_experiment.exe | 1994240 | 2026-10-20T12:44:32.616Z |
| C:\AI\SimulaCiv\target\release\examples\m4_04_crn_experiment.pdb | 2093056 | 2026-10-20T12:44:32.618Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_a5_gate.exe | 2027520 | 2026-10-20T16:39:44.932Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_a5_gate.pdb | 2093056 | 2026-10-20T16:39:44.933Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_experiment.exe | 2004480 | 2026-10-20T16:46:24.212Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_experiment.pdb | 2166784 | 2026-10-20T16:46:24.213Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_followup_measure.exe | 2326528 | 2026-10-20T17:58:29.011Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_followup_measure.pdb | 2224128 | 2026-10-20T17:58:29.014Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_subset_verify.exe | 2017792 | 2026-10-20T18:13:36.450Z |
| C:\AI\SimulaCiv\target\release\examples\m4_05_subset_verify.pdb | 2076672 | 2026-10-20T18:13:36.452Z |
