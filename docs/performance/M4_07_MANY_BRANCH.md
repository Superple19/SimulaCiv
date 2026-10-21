# M4-07 Many-Branch Performance

**Status: M4-07 COMPLETE.** The opt-in parallel modes and direct-column day scan match their serial/SHA oracles. Copy-on-write was not implemented because restore plus day-start clone stayed below the frozen 20% threshold in all four measured cells.

## Host and method

Measurements ran in release mode on Windows 11 Pro, AMD Ryzen 5 9600X (6 physical cores, 12 logical processors). Harness: [`m4_07_many_branch.rs`](../../crates/sim-model/examples/m4_07_many_branch.rs). The M4-04 configuration and outcomes match the [M4-04 recorded workload](M4_04_CRN_EFFICIENCY.md): M3P, N=10,000, K=50, k=8, p_local=0.5, seed `81985529216486895`, initial replicate 7, 100 replicates, horizons `{7,30,90,365}`. One serial and one six-thread parallel API call were timed per intervention; the calls' order alternated across interventions. The parallel pool was caller-owned with chunk size 1. Each pair asserted full result equality before writing its row. These timings are descriptive single pairs, not an A-5 gate; no warmup or repeat distribution was collected for them.

The day-scan comparison used the same M3P config and a 30-day food-zero branch at N=10,000 and N=100,000. The SHA series path and direct-column first-divergence path both started from the same snapshot; the harness compared the first divergence and every daily divergence record. The M4-05 case (a) comparison used N=2,000, K=3, ten replicates, H=90, and the eight lowest numeric AgentIds in GroupId 0. Search evaluation logs were compared after normalizing only their output paths.

For the copy-on-write decision, each cell used one warmup and three measured calls to the full branch analysis API. The total included the API's normal analysis work; restore time covers both `BranchRuntime::restore` calls, and clone time covers day-start state cloning. The table reports medians of the three measured samples and the median per-sample restore-plus-clone share.

The commands were:

```text
cargo run --release -p sim-model --example m4_07_many_branch -- --only=crn
cargo run --release -p sim-model --example m4_07_many_branch -- --only=scan
cargo run --release -p sim-model --example m4_07_many_branch -- --only=search
cargo run --release -p sim-model --example m4_07_many_branch -- --only=cow
```

The JSONL headers record the repository HEAD available when each harness process started (`cf751f2`). At run time the worktree also contained the direct-scan metadata fix and measurement harness, later committed as `f335bc3` and `c7a659e`. One initial M4-05 run used lexicographic truncation and chunk 256, which did not represent the requested candidate set or parallelize ten replicates. Its raw logs remain marked invalid in [`docs/data/m4_07/README.md`](../data/m4_07/README.md) and are excluded from every result below.

## M4-04 replicate experiment: serial versus parallel

Source: [harness](../../crates/sim-model/examples/m4_07_many_branch.rs), [`crn_parallel.jsonl`](../data/m4_07/crn_parallel.jsonl). The table reports complete API wall time; each call computes all four horizons and all five outcomes from the same branch-point state. “Exact” means the returned `CrnEfficiencyResult` values were equal field-for-field.

| Intervention | Serial | Parallel, 6 threads | Observed speedup | Call order | Result |
|---|---:|---:|---:|---|---|
| food = 0 | 1,241.17 s | 243.09 s | 5.11× | serial, parallel | exact |
| health = 0.01 | 1,157.14 s | 247.96 s | 4.67× | parallel, serial | exact |
| BaseMetabolicCost +1% | 1,083.92 s | 254.92 s | 4.25× | serial, parallel | exact |

The six API calls took 4,228.20 seconds of measured wall time in total. The speedups are observations from one paired invocation per path, not a host-independent prediction.

**Inference:** independent replicate trajectories expose useful coarse-grained CPU parallelism here. This result does not show that parallelizing work inside one trajectory or on a GPU would pay off.

## Direct-column day scan

Source: [harness](../../crates/sim-model/examples/m4_07_many_branch.rs), [`day_scan.jsonl`](../data/m4_07/day_scan.jsonl). Both modes returned the same first divergence and complete daily `D_d` records. The direct mode computes the requested first-divergence digest only, while the oracle emits the SHA-based divergence series.

| Population | Horizon | SHA series API | Direct-column API | Observed speedup | Parity |
|---:|---:|---:|---:|---:|---|
| 10,000 | 30 days | 619.73 ms | 508.06 ms | 1.22× | exact |
| 100,000 | 30 days | 10.09 s | 9.80 s | 1.03× | exact |

The intervention differed at the day-0 boundary in both cells. The direct scan still performs the required `D_d`/lineage analysis and one phase-location rerun, so its wall-time reduction was small at N=100,000.

## M4-05 case (a): serial versus parallel search

Source: [harness](../../crates/sim-model/examples/m4_07_many_branch.rs), [`minimal_cause_final.jsonl`](../data/m4_07/minimal_cause_final.jsonl), and the [serial](../data/m4_07/m4_05_search_serial_numeric8.jsonl) / [parallel](../data/m4_07/m4_05_search_parallel_numeric8.jsonl) evaluation logs. Candidate IDs were `[0,3,6,9,12,15,18,21]`, selected numerically before the candidate builder's canonical ID-string ordering.

| Candidate search | Serial | Parallel, 6 threads, chunk 1 | Observed speedup | Evaluations | Cause | Result/log parity |
|---|---:|---:|---:|---:|---|---|
| Exhaustive case (a), R=10, H=90 | 142.21 s | 33.08 s | 4.30× | 960 | `{0,12,15,21}` | exact |

The serial and parallel outputs had the same `minimum` guarantee, mean effect, predicate value, evaluation count, cause IDs, and JSONL records.

## Copy-on-write decision

Source: [harness](../../crates/sim-model/examples/m4_07_many_branch.rs), [`clone_share.jsonl`](../data/m4_07/clone_share.jsonl). Restore-plus-clone share is the median across the three measured samples; one warmup was discarded.

| N | H | Full branch median | Restore median | Day-start clone median | Restore + clone share |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 30 | 0.545 s | 14.70 ms | 0.355 ms | 2.762% |
| 10,000 | 90 | 2.026 s | 20.45 ms | 0.462 ms | 1.026% |
| 100,000 | 30 | 10.599 s | 325.77 ms | 3.958 ms | 3.115% |
| 100,000 | 90 | 31.226 s | 320.66 ms | 4.418 ms | 1.041% |

Every cell is below the frozen 20% threshold, so copy-on-write is **not needed** for the measured workload. Snapshot-backed branch restoration remains unchanged.

## GPU suitability assessment

**Measured facts:** branch and subset evaluations were independent and retained exact results when dispatched on the CPU pool. The M4-04 speedups were 4.25–5.11×; M4-05 case (a) was 4.30×. The direct day scan improved by 1.03× at N=100,000. No GPU hardware or transfer benchmark was used.

**Inference from the current data layout:** `SegmentedAgentStorage` stores roughly 46 nominal bytes of agent columns per record (including identity and dense-slot columns; excluding allocator/container overhead). An 8-neighbor CSR graph adds roughly 44 bytes per source (u32 source ID, 64-bit row offset, and eight u32 targets). At N=100,000 this is about 9 MB for agent columns plus canonical graph, before slot maps, transient projection, propagation, settlements, and branch-specific state. Keeping one trajectory resident on a device could amortize an initial transfer; synchronizing SoA state and outputs every phase or day would require repeated transfers and would be harder to justify. This payload estimate is derived from [`SegmentedAgentStorage`](../../crates/sim-model/src/storage.rs) and [`RelationshipGraph`](../../crates/sim-model/src/relationship_graph.rs), not measured on an accelerator.

Phase 4's per-agent utility/target work and independent branch or subset evaluations expose parallel work ([`phase4_primary_action_selection_storage_into`](../../crates/sim-model/src/decision.rs); [`ordered_replicate_map`](../../crates/sim-model/src/crn_efficiency.rs)). Global Phase 6B sorting, its dependency levels, and the serial core phase sequence impose ordering and synchronization constraints ([`phase6b_targeted_resolution_storage_m3_parallel_with_scratch`](../../crates/sim-model/src/resolution.rs); [`run_registered_storage_day_loop`](../../crates/sim-model/src/runner.rs)). A device backend would need stable key ordering, fixed reductions, the frozen scalar math, and explicit handling of divergent branch workloads and M3P births. These risks are not resolved by this CPU study. No GPU implementation is recommended from the available evidence.

## Conclusion

The frozen M4-07 bit-identity gates passed: the direct scan matched the SHA oracle, M4-04 replicate estimates matched field-for-field across all interventions and horizons, and M4-05 result/evaluation logs matched. Restore plus clone stayed below 20% in every measured cell, so the contract's “not needed” CoW outcome applies. Serial remains the default; parallel execution is opt-in and its reported speedups are specific to this host and these single-pair measurements.
