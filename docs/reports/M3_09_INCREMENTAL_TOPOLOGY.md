# M3-09 Incremental Topology Maintenance
Status: current

M3-09 implementation and parity gate complete

**Measured path:** explicit serial `run_m3p_storage_days_incremental_topology` on Hybrid / `SegmentedAgentStorage` authority
**Oracle:** existing `run_m3p_storage_days` and M3P WorldState reference
**Host:** AMD Ryzen 5 9600X, 6 physical / 12 logical processors; Windows 11 Pro, build 26200; Rust and Cargo `1.98.1`

## Summary

M3-08 measured about 114 ms of normal-day Phase 9b work at 100k live agents when its apply time and full projection rebuild are combined: 96.5 ms apply plus 17.5 ms projection rebuild. In M3-09, the incremental runner's hook-timed Phase 9b total was 21.9 ms on the same workload. Its uninstrumented 100k full tick was 74–75 ms, compared with the current serial oracle's 157–158 ms in the paired run.

The new path matched the existing serial M3P runner and WorldState reference on the committed goldens, the 36-cell matrix, unlimited births, shuffled slots, pause/resume across compaction, and the sustained 1,000-day workload. Existing M2, M3, and M3P expected values remained unchanged.

## Workload and method

The workload is the accepted M3-08 sustained configuration: M2-40 food-supply and decision-policy changes, M3P `k=8`, `p_local=0.5`, birth probability `0.0001`, unlimited births, and compaction interval 50 days. Measurements used `N=10,000, K=50` and `N=100,000, K=500`, master seed `81985529216486895`, and replicate `7`. Configuration details are in [M3-08 Population Dynamics](M3_08_POPULATION_DYNAMICS.md).

The temporary full-tick command was `cargo run --release -p sim-model --example m3p_09_measure`. It used two paired trajectories per population, with days 0–99 as an unmeasured trajectory warm-up and executed days 100–400 measured. Serial and incremental one-day calls were interleaved in alternating order from the same initial configuration. Each call reused its runner context and scratch. Metrics and events were enabled; snapshots were disabled. Initialization, graph creation, and context creation were outside the timer. Every paired daily `M3pDayOutcome` was compared exactly, and final State hashes matched after the timed horizon. The runner does not request a State digest on each normal tick; the final hash check was outside the timer, so the dirty CSR hash stayed deferred during normal execution. Each run reports the median ± MAD across 301 day samples, separated into 294 normal days and seven compaction days.

The equal-live comparison used the day-400 M3P checkpoint. It used two warm-ups, 11 timed samples, three consecutive days per sample, with contexts constructed before timing and metrics/events enabled. The M3 profile used the same live M3P agent records and settlement state. Baseline and incremental M3P outcomes matched for every sample.

Per-phase and Phase 9b substage timings came from temporary `Instant` hooks in the runner and Phase 9b planner, enabled only for one release profile run per population with `cargo run --release -p sim-model --example m3p_09_measure -- phase`. The hooks printed timing records and were removed afterward. These timings are secondary, include hook/printing overhead, and are not additive. Full-tick timings above were collected with profiling disabled. The incremental M3P/serial M3 ratio is measured separately with two warm-ups and 11 timed samples of three consecutive days each; each runner context is built outside its sample timer.

## Full-tick results

All values are µs/tick, median ± MAD. “M3-08 serial” is the previously published full-rebuild baseline from [M3-08 Population Dynamics](M3_08_POPULATION_DYNAMICS.md). “Current serial” is the M3P oracle measured in the same paired run as the incremental path.

| N | Run | M3-08 serial | Current serial oracle | Incremental M3P | Current serial / incremental |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 1 | 9,180 ± 855 | 9,416 ± 2,055 | 5,498 ± 842 | 1.71× |
| 10,000 | 2 | 9,189 ± 1,079 | 12,029 ± 943 | 5,876 ± 105 | 2.05× |
| 100,000 | 1 | 181,383 ± 1,758 | 157,025 ± 8,200 | 75,313 ± 6,552 | 2.08× |
| 100,000 | 2 | 178,525 ± 7,807 | 158,008 ± 8,438 | 74,070 ± 5,124 | 2.13× |

Compaction days retain the full-rebuild path and were materially slower than normal days:

| N | Run | Current serial compaction day | Incremental M3P compaction day |
|---:|---:|---:|---:|
| 10,000 | 1 | 10,584 ± 357 | 10,316 ± 456 |
| 10,000 | 2 | 14,040 ± 355 | 13,638 ± 373 |
| 100,000 | 1 | 180,253 ± 6,033 | 167,121 ± 9,218 |
| 100,000 | 2 | 171,920 ± 10,566 | 166,147 ± 5,109 |

The 10k serial full-tick samples varied across runs; the 100k paired runs were closer. The historical M3-08 measurements and current-oracle rerun are both retained so run-to-run differences are visible.

## Phase profile

Normal-day profile medians ± MAD, µs/tick. Phase 4 is split into action selection and intent generation. Phase 9b totals include the incremental apply path and projection update; the serial M3-08 baseline is listed for comparison.

| Phase | 10k incremental | 100k incremental |
|---|---:|---:|
| Phase 1 regrowth | <1 | <1 |
| Phase 2 degradation | 8 ± 0 | 96 ± 7 |
| Phase 3 features | 32 ± 0 | 350 ± 14 |
| Phase 4 selection | 498 ± 3 | 5,317 ± 288 |
| Phase 4 intents | 181 ± 7 | 3,761 ± 321 |
| Phase 5 partition | 100 ± 7 | 1,598 ± 207 |
| Phase 6A work | 226 ± 7 | 3,586 ± 446 |
| Phase 6B targeted | 155 ± 12 | 2,270 ± 343 |
| Phase 7 market | 148 ± 10 | 2,396 ± 303 |
| Phase 8 welfare | 110 ± 10 | 1,323 ± 227 |
| Phase 9 mortality | 78 ± 1 | 1,085 ± 178 |
| Phase 9b topology | 1,147 ± 28 | 21,871 ± 3,983 |
| Phase 10 metrics | 261 ± 3 | 3,527 ± 421 |
| Phase 11 events | 559 ± 7 | 7,276 ± 813 |

## Phase 9b breakdown

The M3-08 baseline reported apply time and projection rebuild separately. Their median sum is shown as a rough combined comparison; it is not a separately timed aggregate. Incremental-path substage rows are hook-timed and approximate.

| N | Day | M3-08 apply | M3-08 projection rebuild | M3-08 combined medians | M3-09 incremental Phase 9b total |
|---:|---|---:|---:|---:|---:|
| 10,000 | Normal | 5,176 ± 554 | 910 ± 165 | 6,086 | 1,147 ± 28 |
| 10,000 | Compaction | 6,357 ± 130 | 911 ± 9 | 7,268 | 7,139 ± 260 |
| 100,000 | Normal | 96,511 ± 7,688 | 17,523 ± 1,452 | 114,034 | 21,871 ± 3,983 |
| 100,000 | Compaction | 115,449 ± 11,251 | 17,112 ± 589 | 132,561 | 116,950 ± 8,644 |

M3-09 substage medians ± MAD in µs:

| N | Day | Birth planning | Graph attachment | Compaction rebuild | CSR append | Projection stage/reserve | Projection append | Storage append/rebuild | Projection rebuild |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | Normal | 98 ± 2 | 5 ± 4 | — | <1 | <1 | <1 | 4 ± 0 | — |
| 10,000 | Compaction | 100 ± 3 | 917 ± 58 | 3,913 ± 109 | — | — | — | 144 ± 3 | 915 ± 14 |
| 100,000 | Normal | 1,450 ± 146 | 59 ± 10 | — | <1 | 8 ± 2 | <1 | 63 ± 3 | — |
| 100,000 | Compaction | 1,883 ± 75 | 11,496 ± 208 | 67,351 ± 11,400 | — | — | — | 3,030 ± 291 | 16,376 ± 4,054 |

The compaction rebuild column includes dead-target repair and the full canonical CSR reconstruction. The projection rebuild occurs on compaction days; normal days append only newborn projection rows. Baseline M3-08 CSR reconstruction alone was 37,064 ± 2,667 µs on normal 100k days. Normal M3-09 days no longer perform that full CSR hash/rebuild or the full projection rebuild.

## Equal-live M3P/M3 comparison

Median µs/tick at the equal-live checkpoint. The M3-08 baseline values come from its prior 11-sample comparison; current serial-oracle and incremental values use this report's 11-sample, three-day method.

| Initial N | Live agents | M3-08 M3P | M3-08 M3 | M3-08 ratio | Current serial M3P | Incremental M3P | Current M3 | Incremental / M3 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | 10,341 | 8,868 | 2,632 | 3.37× | 3,000 ± 97 | 1,673 ± 119 | 781 ± 44 | 2.14× |
| 100,000 | 103,241 | 165,123 | 31,601 | 5.23× | 61,542 ± 939 | 28,802 ± 1,390 | 11,145 ± 151 | 2.58× |

## Findings

**Measured facts:** at 100k, the normal-day Phase 9b total moved from about 114 ms in the M3-08 apply-plus-projection measurements to about 22 ms in the incremental profile. The full tick moved from the M3-08 serial medians around 179–181 ms to paired incremental medians around 74–75 ms. The equal-live M3P/M3 ratio moved from 5.23× in M3-08 to 2.58× for the incremental path in this run. On compaction days the incremental path still runs the canonical rebuild; the hook-timed 100k Phase 9b total was about 117 ms.

**Inference:** the normal-day gain comes from skipping full CSR reconstruction and projection rebuilding, while compaction cost remains dominated by required full graph cleanup and reconstruction. The residual normal-day Phase 9b time is about 22 ms at 100k. `apply_phase9b_storage_incremental` still materializes storage agents and validates a planning state each day; those scans/copies are candidates for later measurement, not part of this stage. The equal-live comparison and phase-hook values have more run-to-run and instrumentation sensitivity than the uninstrumented paired full-tick result.

## Scope and limitations

The new runner is explicit opt-in. Existing M3P reference and serial storage runners remain the oracles. M2/M3 paths, snapshot schema, M3P goldens, and event/digest encodings are unchanged. Measurements are from one host; phase timings include temporary hook overhead and should be treated as secondary evidence.

## Follow-up: M3P Phase 9b materialization reduction

This follow-up diagnoses the remaining normal-day Phase 9b cost and removes the full-population materialization and copy from the incremental append path. The existing M3P reference runner and serial storage runner remain unchanged behaviorally and continue to serve as the differential oracles.

### Baseline diagnosis

Temporary `Instant` instrumentation measured the incremental Phase 9b on three normal days (101–103) at `N=100,000`, `K=500`, `k=8`, `p_local=0.5`, using the sustained configuration. The full-state timers are in `apply_phase9b_storage_incremental`; planner substage timers are in `plan_phase9b`. Values below are the observed ranges across two short profile runs, in microseconds. The original M3-09 profile recorded about 21,871 µs for the complete normal-day Phase 9b; this diagnostic rerun was noisier.

| Baseline operation | Time per normal day | Allocation or work observed |
|---|---:|---|
| `SegmentedAgentStorage::to_agents` | 986–1,152 | Fresh `Vec<AgentState>` allocation and copy of 100,000 records; `AgentState` is 48 bytes, about 4.8 MB |
| `world.settlements.clone()` | 0.7–1.8 | Fresh 500-entry settlement vector, about 8 KB |
| `relationship_graph.clone()` | 412–558 | Three CSR vectors copied and allocated: source IDs, row offsets, and neighbors; about 4.4 MB for this graph |
| `M3pReferenceState::from_parts` / `validate_state_parts` | 22,556–31,847 | Fresh 100,000-ID validation vector, sorting, then checking roughly 800,000 edges against the sorted IDs |
| `plan_phase9b` | 2,138–2,634 | Candidate index build, eligible-parent scan/sort, RNG draws, sorted full-population slot scratch, planned-agent copy, and newborn planning |
| Commit | 55–348 | Copy staged food values back to storage and append any newborns |
| **Whole Phase 9b** | **27,154–36,149** | The full graph/state validation dominated this instrumented run |

The planner's reusable `sampling_index.members`, parent vectors, `agent_slots`, and `planned_agents` retain their capacities after warm-up, but the old path still filled and sorted the full slot index and copied all `AgentState` values into `planned_agents` every normal day. `births` is an owned result vector; newborn row construction and `M3pCandidateIndex::sample_target` also create small per-birth/per-edge temporary vectors. Allocator totals were not collected with a custom allocator; these allocation sizes/counts are from the concrete allocation sites and collection capacities in the code.

Relevant sites: [`apply_phase9b_storage_incremental`](../../crates/sim-model/src/m3p.rs), [`plan_phase9b_from_view`](../../crates/sim-model/src/m3p.rs), [`validate_state_parts`](../../crates/sim-model/src/m3p.rs), [`SegmentedAgentStorage::to_agents`](../../crates/sim-model/src/storage.rs), [`SegmentedAgentStorage::slot_of` and `agent_at`](../../crates/sim-model/src/storage.rs), and [`M3pStorageRunContext::validate_for`](../../crates/sim-model/src/runner.rs).

### Incremental append path

The normal non-compaction branch now plans directly from the SoA columns. It scans live membership and birth eligibility from storage, resolves only successful parents through the existing `slot_of` map, and stages only parent food deltas, newborn records, CSR rows, and projection rows. It checks all parents and stages the graph/projection append before changing authoritative columns; capacity is reserved before the infallible append/commit sequence. Compaction days retain the canonical full rebuild path.

The incremental runner validates its complete starting state before its day loop. During that loop, graph changes are limited to the checked Phase 9b append; the append plan validates source/target identities and the Phase 4 projection stage validates target slots. Thus the normal Phase 9b path no longer constructs a `WorldState` from SoA, clones the graph, repeats `validate_state_parts`, sorts a full-population slot vector, copies all agents to a planning vector, or copies that vector's food column back.

One temporary profile of days 101–103 measured the revised normal-day Phase 9b at 1,269–1,482 µs (median 1,375 µs). Its main remaining work was candidate-index build (about 0.71–1.02 ms), eligibility scan and birth draws (about 0.44–0.67 ms combined), and graph/projection staging (about 0.06–0.14 ms). The full Phase 9b duration is roughly 16 times lower than the original report's 21.9 ms profile; this is a comparison of hook-timed runs on one host, not a cross-host guarantee.

The public incremental runner still performs `M3pStorageRunContext::validate_for` once per API call, before its day loop. That boundary check remains outside Phase 9b and is included in the one-day full-tick measurements below. A multi-day call pays it once per call. It protects caller-owned state between separate invocations; it was not bypassed for timing.

### Full-tick rerun

Host and configuration match this report: AMD Ryzen 5 9600X, 6 physical / 12 logical processors, Windows 11 Pro build 26200, Rust/Cargo `1.98.1`; M2-40 sustained configuration with M3P `k=8`, `p_local=0.5`, unlimited births, and compaction interval 50. Each run executes days 0–400 as one-day public runner calls; days 0–99 warm the trajectory and days 100–400 give 301 measured ticks. Metrics and events are enabled, snapshots are off, and the runner contexts are reused. Two repeats were measured. The paired run alternates serial and incremental calls by day; the non-interleaved run completes the serial trajectory and then the incremental trajectory from the same initial configuration. Paired daily outcomes and final State/Metrics/Events digests matched exactly.

All values are µs/tick, median ± MAD.

| N | Run | Paired serial | Paired incremental | Non-interleaved serial | Non-interleaved incremental |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 1 | 9,219 ± 829 | 3,357 ± 214 | 9,050 ± 928 | 4,033 ± 277 |
| 10,000 | 2 | 9,153 ± 1,120 | 3,341 ± 209 | 9,033 ± 561 | 3,329 ± 248 |
| 100,000 | 1 | 157,341 ± 11,501 | 52,550 ± 5,634 | 154,620 ± 10,381 | 52,625 ± 5,416 |
| 100,000 | 2 | 153,712 ± 10,886 | 51,460 ± 6,188 | 156,480 ± 11,641 | 52,516 ± 5,479 |

The paired incremental medians are 3,349 µs at 10k and 52,005 µs at 100k. The previous report's incremental medians were 5,687 µs and 74,692 µs respectively, so this rerun measured 41% and 30% lower times. The M3P serial controls were 9,186 µs and 155,527 µs in the paired rerun. These are observed differences for this host and sampling window.

Final paired parity digests were identical between serial and incremental M3P:

| N | State | Metrics | Events |
|---:|---|---|---|
| 10,000 | `c29aade66a34b2cad5678d326df6cab05e73ade5e4023c1bb61d65f49337a728` | `49659024d084fb28c89d69f900e72ebf51e2173065659d7c65a0fc342a8835d1` | `2123c4c2f5c566c7d313f44e751500d5b570abfbfdf04be22afcefda570f46d8` |
| 100,000 | `11c58919a9cf8425a737841a50dde4eed9d4dabb6b17df101b56778058dffb47` | `27ad17a9a89c8d06d69f1470b09bd97d4a481386eece6dffbee8f1835890b757` | `3e8c3daf0b1eb0df847d58d2dddabdedca065d12338934a00a3710e9432ddf2b` |

### Equal-live M3P/M3 rerun and interleaving

The checkpoint is after executed day 400, at 10,341 live agents for the 10k start and 103,241 for the 100k start. Each sample uses two warmups, 11 measured samples, and three consecutive days per sample; contexts are reconstructed from the same checkpoint outside each sample timer. The paired schedule rotates M3P serial, M3P incremental, and M3 order within each sample. The non-interleaved schedule completes all samples for one runner before moving to the next. Values are µs/tick, median ± MAD.

| Initial N | Schedule | M3P serial | M3P incremental | M3 | Incremental M3P / M3 |
|---:|---|---:|---:|---:|---:|
| 10,000 | Paired | 7,081 ± 108 | 2,756 ± 29 | 2,276 ± 101 | 1.21× |
| 10,000 | Non-interleaved | 6,932 ± 177 | 2,605 ± 47 | 2,125 ± 24 | 1.23× |
| 100,000 | Paired | 148,153 ± 4,807 | 40,641 ± 2,183 | 27,315 ± 970 | 1.49× |
| 100,000 | Non-interleaved | 141,599 ± 4,609 | 39,135 ± 1,006 | 26,590 ± 1,918 | 1.47× |

The prior M3-09 equal-live report recorded incremental-M3P/M3 ratios of 2.14× at 10k and 2.58× at 100k. This rerun measured 1.21–1.23× and 1.47–1.49×. The earlier equal-live timing driver was temporary and is not in the repository; the follow-up timings use the public runner APIs and include their normal validation on each three-day call. That boundary makes this a measured rerun, but limits precise attribution of the absolute difference to Phase 9b alone.

For whole ticks, non-interleaved incremental medians were 9.9% higher than paired at 10k and 1.1% higher at 100k. In the equal-live comparison, non-interleaved incremental was 5.5% lower at 10k and 3.7% lower at 100k. The direction changes with measurement shape at 10k and the 100k differences are within a few percent; these runs do not show a stable cache benefit from either schedule.

### Test runtime

The M3-09 test matrix still covers all 36 configurations, all 300-day per-day comparisons, goldens, snapshot/compaction resume, shuffled slots, unlimited births, and the 1,000-day sustained gate. It now computes each day's reference State digest once and uses a fixed four-thread Rayon pool only to run independent matrix cells concurrently; each simulated trajectory remains on its serial M3P path. The integration target completed in 68.22 seconds under the normal Rust test schedule (a forced single test thread took 129.04 seconds). No cases or assertions were removed. The separate day-1 exhausted-ID regression for the new non-compaction append path passed in 0.01 seconds.

**Inference:** the dominant avoidable Phase 9b cost was its redundant full-state materialization and graph validation, not newborn graph construction. Candidate-index construction is now the largest normal-day Phase 9b substage at 100k. Whole-tick timing still includes per-call context validation, so using a long multi-day runner call can have different cost from this one-day-call benchmark. Measurements are from one host and the substage profile is hook-timed.
