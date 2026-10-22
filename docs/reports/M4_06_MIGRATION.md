# M4-06 Settlement Migration
Status: current

M4-06 complete; reference/storage parity, the M4-01 forward gate, the A-5 migration-off gate, and the report-only experiment are recorded below.


## A-5 migration-off performance gate

The gate paired the existing direct `run_m3p_storage_days` path with the existing configured M3P dispatch path using explicit `migration = false`. Both paths start each sample from the same initialized state; each paired operation runs three consecutive days with metrics and events enabled and snapshots off. The migration-off digest and per-day outcomes matched exactly. The measured gate uses 2 warmups, 41 timed samples, alternating path order, and one fixed repetition count per cell chosen during warmup to make each paired sample at least one second. Migration-on hook cost is a separate descriptive measurement with migration enabled and `migration_probability = 0`, so it contains no movement commits.

| N | K | r | direct median ± MAD (µs / 3 days) | configured-off median ± MAD (µs / 3 days) | delta median / MAD (µs) | U (µs) | T (µs) | path MAD/median | status | migration hook (ns / alive-agent-day) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|
| 10,000 | 50 | 20 | 29,676.57 ± 76.55 | 29,656.82 ± 104.90 | -15.55 / 86.54 | 46.16 | 1,483.83 | 0.26% / 0.35% | PASS | 831.80 |
| 100,000 | 500 | 2 | 483,937.90 ± 1,380.05 | 483,268.30 ± 1,051.15 | -507.60 / 1,846.95 | 45.35 | 24,196.90 | 0.29% / 0.22% | PASS | 1,058.04 |

The table reports medians from the committed [A-5 harness](../../crates/sim-model/examples/m4_06_a5_gate.rs) and its 41 per-cell sample arrays in [a5_gate_final.json](../data/m4_06/a5_gate_final.json). A-5 validity required each path MAD/median ≤ 5%; PASS required the fixed-seed bootstrap upper bound `U` ≤ `T = 5% × direct median`. Both cells were valid PASS with no retry. The migration-on figures are the active hook with zero moves, reported only as descriptive cost.

| Host record | Before | After | Applied controls | Source |
|---|---:|---:|---|---|
| Total CPU load | 12.54% | 9.42% | informational only | [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs), [a5_host_final.json](../data/m4_06/a5_host_final.json) |
| Free physical memory | 76,628,592 KiB | 76,652,208 KiB | informational only | [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs), [a5_host_final.json](../data/m4_06/a5_host_final.json) |
| Benchmark process | Windows x86_64, 12 logical processors | High priority, affinity `0x555` (reported mask `0x555`) | applied and verified by the harness | [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs), [a5_gate_final.json](../data/m4_06/a5_gate_final.json) |

The isolated final gate ran from 05:15:47 to 05:20:24 local time. The first run is retained in [a5_gate.json](../data/m4_06/a5_gate.json) as superseded history; it compared implicit and explicit off configurations through the same direct runner. The configured-dispatch hook-cost run and the run concurrent with the workspace suite are also retained as [a5_gate_configured_cost_superseded.json](../data/m4_06/a5_gate_configured_cost_superseded.json) and [a5_gate_concurrent_suite.json](../data/m4_06/a5_gate_concurrent_suite.json); neither is used for the final table.

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

A follow-up contract review corrected the two migration event GroupId payload encoders in `hashing.rs` from u16 to u32 little-endian (+2/−2 production source lines) and updated the M3M Events golden in the new test file only. A new byte-level assertion checks the frozen field widths. No pre-existing test, golden, snapshot, or CI file changed.

### Extensibility debt

The migration implementation added 1,896 production lines and removed 66 outside `migration.rs`. The categorized file totals are:

- **configuration — 207 added / 8 removed:** `config.rs`. Migration adds a switch and required settings but does not generalize system-specific config contributions.
- **digest domains — 673 / 0:** `hashing.rs`. Each effective profile needs its own frozen State, Metrics, and Events preimage wrapper.
- **profile codecs — 249 / 18:** `m3p.rs`, `m3pprop.rs`, and `m3prop.rs`. The migration snapshot wrapper must preserve each profile's existing base bytes and append the profile-specific migration section.
- **contexts, registry, and storage wiring — 767 / 40:** `runner.rs`, `system_registry.rs`, `lib.rs`, `storage.rs`, and `relationship_graph.rs`. The code adds migration state to profile contexts, registers the hook, refreshes the cross-partition edge count, and exposes the migration entry points.

These categories sum to the reported `+1,896 / −66`. M4-01's registry describes systems and orders hooks, but it did not generalize per-combination state and codec wiring. Migration therefore needed profile-specific digest/snapshot wrappers and context plumbing outside its own system module. This is the M4-01 extensibility debt exposed by the forward gate; the gate itself still passed because no runner function or generic-loop body changed.

The population model also removes dead agents and their records during M3P compaction. Their wealth leaves the agent-plus-treasury total, so money is not conserved across death cleanup in M3P. This explains why M4-04's `total_wealth` outcome can change when deaths occur; migration transfers themselves remain balanced.

### Migration hook follow-up

The descriptive follow-up pairs the direct M3P runner with the same runner configured with migration at `p=0` and `p=0.05`. It uses the A-5 sampling shape: 2 warmups, 41 paired samples, alternating order, a fixed warmup-derived repetition count for at least one second per paired sample, three days per operation, High priority, and affinity `0x555`. The committed [hook harness](../../crates/sim-model/examples/m4_06_hook_followup.rs) writes the measurements to [hook_followup.json](../data/m4_06/hook_followup.json); the pre-state-assertion run is preserved separately as [hook_followup_pre_state_check.json](../data/m4_06/hook_followup_pre_state_check.json). The `p=0` harness path also asserts per-day metrics/events and final agent/settlement state parity.

| N | p | r (this run) | Before hook (ns/alive-agent-day) | After hook (median ± MAD) | Moves per timed sample | Path delta median ± MAD (µs/3 days) |
|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | 0 | 16 | 831.80 | 480.62 ± 10.56 | 0 | 14,456.98 ± 317.63 |
| 10,000 | 0.05 | 13 | — | 445.68 ± 10.02 | 8,524 | 13,401.51 ± 301.43 |
| 100,000 | 0 | 1 | 1,058.04 | 695.31 ± 33.17 | 0 | 208,629.10 ± 9,953.70 |
| 100,000 | 0.05 | 1 | — | 721.51 ± 20.72 | 7,601 | 216,490.00 ± 6,218.20 |

The before values are the zero-move medians from the original [A-5 gate table](#a-5-migration-off-performance-gate), generated by [m4_06_a5_gate.rs](../../crates/sim-model/examples/m4_06_a5_gate.rs) and [a5_gate_final.json](../data/m4_06/a5_gate_final.json). The after values and samples are from the hook harness and linked raw JSON. The move counts cover the full timed sample (`r × 3` days). These values are descriptive; the p=0.05 path includes actual moves and their commits.

## Migration experiment

The report-only experiment used the M3P sustained preset from `m3p_propagation_sustained.toml` with propagation disabled, `N=10,000`, `K=50`, `k=8`, `p_local=0.5`, replicate 7, and seeds `{0, 1, 3, 7, 81985529216486895}`. Migration used probability `0.25`, threshold `0`, cost `10`, and cooldown 30 days. Each table below is generated by the committed [experiment harness](../../crates/sim-model/examples/m4_06_experiment.rs) from [experiment.json](../data/m4_06/experiment.json); that file contains the full daily series, including the settlement occupancy map for migration-on runs.

### Population distribution and money

| Day | Alive median (range) | Settlement Gini median (range) | Per-settlement minimum range | Per-settlement maximum range |
|---:|---:|---:|---:|---:|
| 1 | 10,003 (10,000–10,003) | 0.0003 (0.0000–0.0003) | 200–200 | 200–201 |
| 100 | 10,099 (10,088–10,103) | 0.0424 (0.0394–0.0462) | 172–178 | 247–262 |
| 200 | 10,195 (10,172–10,204) | 0.0361 (0.0312–0.0382) | 177–184 | 235–243 |
| 365 | 10,292 (10,287–10,296) | 0.0295 (0.0264–0.0331) | 183–187 | 236–245 |

The rounded Gini median at day 200 is `0.0361`; the raw JSON has the full precision. The migration-on base trajectories made 113,567–113,718 moves, with median 113,644. They had 365–377 births and 75–81 compacted removals. Final total agent wealth plus settlement treasuries was 100,191,432–100,199,840 from an initial 100,250,000. The full-world total therefore did not stay constant across death cleanup; each observed decrease exactly matched the cumulative wealth of removed tombstone records, and adding removed wealth back kept the recorded conservation check true throughout all five trajectories. The migration transfer check also passed every day: movement-event costs equaled both the daily migration-cost metric and `moves × 10`.

| Seed | Alive at day 365 | Moves | Births | Removed | Final Gini | Settlement occupancy min–max | Final global money | Removed wealth | Adjusted check | Transfer check |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| 0 | 10,296 | 113,718 | 373 | 75 | 0.0314 | 183–236 | 100,199,840 | 50,160 | PASS | PASS |
| 1 | 10,294 | 113,644 | 373 | 78 | 0.0295 | 187–238 | 100,191,432 | 58,568 | PASS | PASS |
| 3 | 10,287 | 113,598 | 365 | 75 | 0.0264 | 185–241 | 100,196,738 | 53,262 | PASS | PASS |
| 7 | 10,290 | 113,655 | 370 | 79 | 0.0284 | 185–237 | 100,197,523 | 52,477 | PASS | PASS |
| 81985529216486895 | 10,292 | 113,567 | 377 | 81 | 0.0331 | 185–245 | 100,192,493 | 57,507 | PASS | PASS |

### Food-zero counterfactual with migration on and off

The intervention set food to zero for the lowest live AgentId in the largest initial settlement; the five seeds selected AgentId 0 in GroupId 0. The table compares migration disabled and enabled within each seed.

The M4-02 `BranchRequest` supports the existing M3/M3P profiles, not the new migration-enabled profile. The harness therefore builds paired M3P storage runs from the same initial state and seed, applies the same food-zero field override before day 1, and toggles migration between the pair. It does not call the M4-02 branch API.

| Seed | First other settlement touched, off / on | Cascade onset day, off / on | Settlements touched by day 365, off / on | Day-365 divergence fraction, off / on |
|---:|---:|---:|---:|---:|
| 0 | 1 / 1 | 6 / 2 | 50 / 50 | 0.9852 / 1.0000 |
| 1 | 6 / 2 | 11 / 4 | 50 / 50 | 0.9858 / 1.0000 |
| 3 | 2 / 2 | 2 / 2 | 50 / 50 | 0.9881 / 1.0000 |
| 7 | 2 / 2 | 3 / 3 | 50 / 50 | 0.9897 / 1.0000 |
| 81985529216486895 | 1 / 1 | 13 / 8 | 50 / 50 | 0.9839 / 1.0000 |

Across seeds, first-touch day had median/range 2 (1–6) with migration off and 2 (1–2) with migration on. Cascade onset had median/range 6 (2–13) off and 3 (2–8) on. Both settings touched all 50 settlements by day 365. The complete daily touched-set counts are in [experiment.json](../data/m4_06/experiment.json), produced by [m4_06_experiment.rs](../../crates/sim-model/examples/m4_06_experiment.rs).

**Measured facts:** migration-on runs made about 114k moves per seed while keeping all settlement populations within 183–245 at day 365. In the food-zero comparison, migration-on reached all 50 settlements by the endpoint and every endpoint divergence fraction was 1.0; migration-off also reached all 50, with endpoint fractions 0.9839–0.9897.

**Inference:** migration did not increase final settlement coverage for this workload, because the migration-off trajectories already touched every settlement by day 365. It advanced cascade onset in three of five seeds and tied in two; the median onset was three days earlier. These results show that migration changes the timing and endpoint divergence in this sustained profile, not that it removes a general inter-settlement bottleneck.

### Branch API rerun with the registered migration system

The food-zero experiment was rerun through `run_counterfactual_branch_with_divergence_series` for migration off/on, using the fixed requested settings `p=0.02`, threshold `0.2`, cost `10`, and cooldown `30`, over five seeds and 365 days. The committed [branch experiment harness](../../crates/sim-model/examples/m4_06_branch_experiment.rs) creates the M3P or M3PM branch-point snapshot and writes every daily move count, divergence set, and cumulative settlement-touch count to [branch_experiment.json](../data/m4_06/branch_experiment.json). No settings were adjusted after observing the result.

| Seed | Moves/day, off / on | First other settlement day, off / on | Cascade onset, off / on | Settlements touched by day 365, off / on | Day-365 fraction, off / on |
|---:|---:|---:|---:|---:|---:|
| 0 | 0 / 0 | 1 / 1 | 6 / 6 | 50 / 50 | 0.9852 / 0.9852 |
| 1 | 0 / 0 | 6 / 6 | 11 / 11 | 50 / 50 | 0.9858 / 0.9858 |
| 3 | 0 / 0 | 2 / 2 | 2 / 2 | 50 / 50 | 0.9881 / 0.9881 |
| 7 | 0 / 0 | 2 / 2 | 3 / 3 | 50 / 50 | 0.9897 / 0.9897 |
| 81985529216486895 | 0 / 0 | 1 / 1 | 13 / 13 | 50 / 50 | 0.9839 / 0.9839 |

The branch API's daily records show no migrations at the specified pressure threshold in either the base or food-zero branch. The cumulative settlement-touch trajectory was identical between migration-off and migration-on runs: median/range 1 (1–3) at day 1, 2 (2–3) at day 10, 3 (2–3) at day 30, 4 (3–6) at day 90, 43 (30–48) at day 180, and 50 (50–50) at day 365. The per-day counts are in [branch_experiment.json](../data/m4_06/branch_experiment.json), generated by [m4_06_branch_experiment.rs](../../crates/sim-model/examples/m4_06_branch_experiment.rs).

**Measured fact:** with this M2-40-derived workload and `threshold=0.2`, the selected destination pressure never passed the strict migration predicate during the 365-day runs, so migration recorded zero moves and the on/off branch results matched. **Inference:** this run does not test whether migration changes inter-settlement spread when moves actually occur; it shows the prescribed realistic settings were inactive for this workload. No threshold or resource settings were tuned to force movement.
