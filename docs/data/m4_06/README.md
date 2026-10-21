# M4-06 raw data

| File | Source | Report use |
|---|---|---|
| [a5_gate_final.json](a5_gate_final.json) | [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) | Final A-5 paired cells, 41 samples per cell, path spread and migration-hook cost. |
| [a5_host_final.json](a5_host_final.json) | Collected around [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) | CPU and free-memory readings before and after the final gate run. |
| [a5_gate_configured_cost_superseded.json](a5_gate_configured_cost_superseded.json) | Intermediate [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) run | Preserved A-5 samples whose migration-on cost included configured-dispatch overhead. |
| [a5_host_configured_cost_superseded.json](a5_host_configured_cost_superseded.json) | Collected around that intermediate run | Host readings for the superseded migration-on attribution. |
| [a5_gate_concurrent_suite.json](a5_gate_concurrent_suite.json) | Intermediate [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) run | Preserved A-5 samples collected while the workspace suite was also using the host. |
| [a5_host_concurrent_suite.json](a5_host_concurrent_suite.json) | Collected around that intermediate run | Host readings for the concurrent run. |
| [a5_gate.json](a5_gate.json) | Initial run of [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) | Superseded history: it compared two configurations through the same direct runner, before the final comparison was changed to direct versus configured dispatch. |
| [a5_host.json](a5_host.json) | Collected around the initial harness run | Host readings for the superseded initial run. |
| [experiment.json](experiment.json) | [m4_06_experiment.rs](../../../crates/sim-model/examples/m4_06_experiment.rs) | Final five-seed population and food-zero comparison; daily records include settlement distribution, lineage divergence, moves, cleanup wealth, and migration transfer checks. |
| [experiment_superseded_seed_set.json](experiment_superseded_seed_set.json) | Earlier run of [m4_06_experiment.rs](../../../crates/sim-model/examples/m4_06_experiment.rs) | Superseded history: used seed 42 instead of the M4-03 seed 7 and did not yet account for wealth removed at compaction. |
| [hook_followup.json](hook_followup.json) | [m4_06_hook_followup.rs](../../../crates/sim-model/examples/m4_06_hook_followup.rs) | Final descriptive A-5-style hook-cost samples at migration probabilities 0 and 0.05, with migration-off parity assertions at probability 0. |
| [hook_followup_pre_state_check.json](hook_followup_pre_state_check.json) | Earlier run of [m4_06_hook_followup.rs](../../../crates/sim-model/examples/m4_06_hook_followup.rs) | Preserved timing output before the final p=0 state parity assertions were added. |
| [branch_experiment.json](branch_experiment.json) | [m4_06_branch_experiment.rs](../../../crates/sim-model/examples/m4_06_branch_experiment.rs) | Food-zero migration-off/on comparison through the M4 branch API at probability 0.02 and threshold 0.2, including daily moves and touched-settlement series. |

See the [M4-06 report](../../performance/M4_06_MIGRATION.md) for the final gate interpretation.
