# M3-09 Incremental Topology Maintenance

**Status:** M3-09 implementation and parity gate complete
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
