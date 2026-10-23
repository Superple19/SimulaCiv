# M4-04 recovered data

Report: [M4-04 CRN Efficiency](../../reports/M4_04_CRN_EFFICIENCY.md).

| Report table / section | Recovered data | Harness source |
|---|---|---|
| Food = 0 outcome/horizon table | m4_04_crn_food_0.csv | crates/sim-model/examples/m4_04_crn_experiment.rs restored from session history; not re-run |
| Health = 0.01 outcome/horizon table | m4_04_crn_health_0_01.csv | same |
| BaseMetabolicCost +1% outcome/horizon table | m4_04_crn_base_metabolic_cost_plus_1pct.csv and m4_04_crn_efficiency.csv (the latter is a duplicate-sized captured copy) | same |

The restored source compiles with cargo check -p sim-model --examples; no experiment was rerun.
