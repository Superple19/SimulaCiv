# M4-05 recovered data

Report: [M4-05 Minimal-Cause Search](../../reports/M4_05_MINIMAL_CAUSE.md).

| Report table / section | Recovered data | Harness source |
|---|---|---|
| A-5 batch-of-one gate | Gate values are in the report; no corresponding raw sample file survived | m4_05_a5_gate.rs restored from session history; not re-run |
| Initial cases (a), (b), and (c) search logs | m4_05_a_health8.jsonl, m4_05_b_health16.jsonl, m4_05_c_config6.jsonl | m4_05_experiment.rs restored from session history; not re-run |
| Case (a) full-set probe | m4_05_a_health8_full_probe.jsonl | same |
| A-1 seed repeats | seed-0.jsonl, seed-1.jsonl, seed-3.jsonl | source for the follow-up measurement was not recovered intact |
| Golden/incomplete process probes | temp_m4-05-golden-21620.jsonl, temp_m4-05-golden-21984.jsonl, temp_m4-05-incomplete-21984.jsonl | temporary verification harness source not recovered intact |

The A-5 and initial experiment sources compile with cargo check -p sim-model --examples; none were executed.