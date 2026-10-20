# M4-06 raw data

| File | Source | Report use |
|---|---|---|
| [a5_gate_final.json](a5_gate_final.json) | [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) | Final A-5 paired cells, 41 samples per cell, path spread and migration-hook cost. |
| [a5_host_final.json](a5_host_final.json) | Collected around [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) | CPU and free-memory readings before and after the final gate run. |
| [a5_gate.json](a5_gate.json) | Initial run of [m4_06_a5_gate.rs](../../../crates/sim-model/examples/m4_06_a5_gate.rs) | Superseded history: it compared two configurations through the same direct runner, before the final comparison was changed to direct versus configured dispatch. |
| [a5_host.json](a5_host.json) | Collected around the initial harness run | Host readings for the superseded initial run. |

See the [M4-06 report](../../performance/M4_06_MIGRATION.md) for the final gate interpretation.
