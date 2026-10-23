# M4-02 recovered data

Report: [M4-02 Branching and Divergence Detection](../../reports/M4_02_BRANCHING.md).

| Report table / section | Recovered data | Harness source |
|---|---|---|
| Historical paired runs (§11.7 A-1/A-2 context) | m4_02_raw.csv, m4_02_final.csv | original m4_02_measure.rs source is incomplete in session history; compiled binary survives in target/debug/examples and target/release/examples |
| Follow-up orchestration samples | m4_02_followup.csv | m4_02_followup_measure.rs source could not be recovered intact; compiled binary survives |
| Recorder probe | m4_02_recorder.csv | same unrecovered initial harness |
| Worked-example day curve | m4_02_worked.csv | same unrecovered initial harness |
| A-2 host/load records | m4_02_a2_*.json, m4_02_a2_*load.csv | m4_02_a2_measure.rs restored from session history; not re-run |
| A-3 host/load records | m4_02_a3_official_host.json, m4_02_a3_official_load.csv | m4_02_a3_measure.rs restored from session history; not re-run |
| A-4 host snapshots | m4_02_a4_initial_host.json, m4_02_a4_retry_host.json; no per-cell CSV survived | m4_02_a4_measure.rs restored from session history; not re-run |
| A-5 final gate and recorder comparison tables | No raw A-5 sample CSV/JSON survived in searched locations | m4_02_a5_measure.rs restored from session history; not re-run |

The m4_02_a1_measure.rs history entry was truncated by the history reader and was not restored as source. All recovered example files compile with cargo check -p sim-model --examples; they were not executed.
