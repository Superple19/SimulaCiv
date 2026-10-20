# M4-06 Settlement Migration

**Status:** implementation and parity passed; A-5 migration-off gate passed; migration experiment pending.

## A-5 migration-off performance gate

The gate paired the existing direct `run_m3p_storage_days` path with the existing configured M3P dispatch path using explicit `migration = false`. Both paths start each sample from the same initialized state; each paired operation runs three consecutive days with metrics and events enabled and snapshots off. The migration-off digest and per-day outcomes matched exactly. The measured gate uses 2 warmups, 41 timed samples, alternating path order, and one fixed repetition count per cell chosen during warmup to make each paired sample at least one second. Migration-on hook cost is a separate descriptive measurement with migration enabled and `migration_probability = 0`, so it contains no movement commits.

| N | K | r | direct median ± MAD (µs / 3 days) | configured-off median ± MAD (µs / 3 days) | delta median / MAD (µs) | U (µs) | T (µs) | path MAD/median | status | migration hook (ns / alive-agent-day) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|
| 10,000 | 50 | 19 | 29,703.09 ± 54.15 | 29,717.58 ± 61.86 | 6.77 / 38.90 | 33.44 | 1,485.16 | 0.18% / 0.21% | PASS | 833.39 |
| 100,000 | 500 | 2 | 480,763.05 ± 1,314.00 | 481,075.10 ± 1,389.40 | -20.85 / 1,818.80 | 706.75 | 24,038.15 | 0.27% / 0.29% | PASS | 1,058.58 |

The table reports medians from the committed [A-5 harness](../../crates/sim-model/examples/m4_06_a5_gate.rs) and its 41 per-cell sample arrays in [a5_gate_final.json](../data/m4_06/a5_gate_final.json). A-5 validity required each path MAD/median ≤ 5%; PASS required the fixed-seed bootstrap upper bound `U` ≤ `T = 5% × direct median`. Both cells were valid PASS with no retry. The migration-on figures are the active hook with zero moves, reported only as descriptive cost.

| Host record | Before | After | Applied controls | Source |
|---|---:|---:|---|---|
| Total CPU load | 7.98% | 8.79% | informational only | [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs), [a5_host_final.json](../data/m4_06/a5_host_final.json) |
| Free physical memory | 76,611,360 KiB | 76,643,904 KiB | informational only | [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs), [a5_host_final.json](../data/m4_06/a5_host_final.json) |
| Benchmark process | Windows x86_64, 12 logical processors | High priority, affinity `0x555` (reported mask `0x555`) | applied and verified by the harness | [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs), [a5_gate_final.json](../data/m4_06/a5_gate_final.json) |

The corrected gate ran from 04:48:27 to 04:52:58 local time. The first run is retained in [a5_gate.json](../data/m4_06/a5_gate.json) as superseded history; it compared implicit and explicit off configurations through the direct runner, so it is not used for this gate.

## Implementation and forward gate

Migration is an opt-in registered system. The reference and storage paths, including both propagation combinations, passed per-day outcome and digest parity, v4 snapshot resume, shuffled-slot checks, and the migration failure-atomicity tests. Migration-off execution remains the M3/M3P runner path. The harness and validation edits do not add migration-specific public runner entry points.

The M4-01 forward gate passed: there are zero new runner functions and zero edits to the generic loop bodies `run_registered_world_day_loop_impl` and `run_registered_storage_day_loop`. The migration descriptor is added as a separate static descriptor and is included by system-set validation only when the migration switch is enabled. Propagation composition passed without generic-loop changes.

Touched files in the implementation commit:

- `crates/sim-model/src/config.rs`
- `crates/sim-model/src/hashing.rs`
- `crates/sim-model/src/lib.rs`
- `crates/sim-model/src/m3p.rs`
- `crates/sim-model/src/m3pprop.rs`
- `crates/sim-model/src/m3prop.rs`
- `crates/sim-model/src/migration.rs`
- `crates/sim-model/src/relationship_graph.rs`
- `crates/sim-model/src/runner.rs`
- `crates/sim-model/src/storage.rs`
- `crates/sim-model/src/system_registry.rs`
- `crates/sim-model/tests/m4_06_migration_tests.rs` (new)
- `crates/sim-model/benches/m3_scale_limit_bench.rs`
- `crates/sim-model/examples/m4_02_a2_measure.rs`
- `crates/sim-model/examples/m4_02_a3_measure.rs`
- `crates/sim-model/examples/m4_02_a4_measure.rs`
- `crates/sim-model/examples/m4_02_a5_measure.rs`
- `crates/sim-model/examples/m4_03_experiment_temp.rs`
- `crates/sim-model/examples/m4_03_gate_temp.rs`
- `crates/sim-model/examples/m4_04_crn_experiment.rs`
- `crates/sim-model/examples/m4_05_a5_gate.rs`
- `crates/sim-model/examples/m4_05_experiment.rs`

The changes outside the migration system module are 1,896 production source lines added and 66 removed; the other non-module changes are the new 866-line parity suite, one benchmark initializer field, and formatting/lint compatibility edits to the restored experiment examples. The whole implementation commit records 4,072 added and 248 removed lines. No existing test, golden, snapshot, or CI file changed.

## Migration experiment

The five-seed, 365-day M3P sustained population and M4-03 food-zero comparisons are pending. They will be recorded with the committed [experiment harness](../../crates/sim-model/examples/m4_06_experiment.rs) and raw data under [docs/data/m4_06](../data/m4_06/).
