# M4-03 recovered data

Report: [M4-03 Divergence Measurement](../../reports/M4_03_DIVERGENCE.md).

| Report table / section | Recovered data | Harness source |
|---|---|---|
| Historical divergence experiment series | m4_03_divergence_experiments.csv | m4_03_experiment.rs restored from session history; not re-run |
| A-1 corrected runs, daily curves, settlement spread, H=365 extension | m4_03_a1_experiments.csv | m4_03_experiment.rs restored from session history; not re-run |
| Confirmatory performance gate | m4_03_gate_rerun.csv | m4_03_gate.rs restored from session history; not re-run |

The restored sources compile with cargo check -p sim-model --examples; no experiments were rerun.