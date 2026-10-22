<a id="m3-scale-diagnosis-population-and-settlement-count"></a>
# M3-05 (formerly M3-S) Scale Diagnosis: Population and Settlement Count
Status: current


This is a measurement-only diagnosis of the superlinear intervals recorded in [M3 Scale Limit](M3_SCALE_LIMIT.md). It separates population `N` from settlement count `K`, counts the settlement-keyed scans reached by the M3 serial storage path, decomposes Phase 3, and remeasures the N=1,000,000 / k=16 parallel Phase 6B case. No semantic optimization was implemented.

## Host and method

- **Host:** AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors; Windows 11 Pro; `rustc 1.98.1`; `x86_64-pc-windows-msvc`; 95.61 GiB RAM.
- **Workload:** the M3 equilibrium preset (`base_metabolic_cost=0.3129`), seed `81985529216486895`, replicate `7`, `p_local=0.5`, serial, day 0, metrics and events enabled, snapshots disabled.
- **Sampling:** same as the M3 scale bench: two warmups, seven samples, three consecutive days per sample with scratch reuse. Per-phase values are median ± MAD over the 21 timed day observations. Setup and initialization are outside timed regions.
- **Fixed-K sweep:** `K=500`, `k=8`, `N ∈ {100,000, 250,000, 500,000, 1,000,000}`.
- **Fixed-N sweep:** `N=250,000`, `k=8`, `K ∈ {250, 500, 1,250, 2,500, 5,000}`. With round-robin membership, the minimum settlement sizes are 1,000, 500, 200, 100, and 50. The corresponding minimum local capacities are 999, 499, 199, 99, and 49; cross-settlement capacities also exceed 8. No §2.2 cells were skipped.
- **Phase 3:** an opt-in timing wrapper measures scarcity-index preparation, the per-agent feature loop, and the final sort. It shares the same implementation as the existing untimed API. The bench asserts exact `AgentFeatures` equality against the untimed path. A second untimed run is outside each timing interval.
- **Parallel Phase 6B:** `N=1,000,000`, `K=5,000`, `k=16`, 6-thread caller-owned pool, chunk 256, day-0 checkpoint. Stage timings use the existing timing wrappers. A separate paired full-tick measurement uses the uninstrumented runners over the same checkpoint; digests are computed outside timers.
- **Parity:** the serial phase driver is compared with `run_m3_storage_days` for each profile sample. The parallel stage and full-tick pairs compare State, Metrics, and Events digests exactly.
- **Commands:** `cargo bench --bench m3_scale_limit_bench -- --diagnose`; follow-up Phase 3 run `cargo bench --bench m3_scale_limit_bench -- --diagnose --p3-only`; paired Phase 6B/full-tick rerun `cargo bench --bench m3_scale_limit_bench -- --diagnose --parallel-only`.
- **Wall time:** full N/K diagnostic 7 min 29 sec; Phase 3 repeat 2 sec; Phase 6B/full-tick repeat 3 min 25 sec. Combined bench wall time was about 10 min 56 sec.

The exact stdout logs were written under ignored `target/` files and were not committed.

## Per-phase scaling with N at fixed K=500

`alpha = log(T₂/T₁) / log(N₂/N₁)`. These are measured medians, not a fitted asymptotic model. At the first interval, several phases show a cache/working-set transition; from 250k onward, the phase alphas are mostly close to 1.

| Phase | 100k→250k | 250k→500k | 500k→1M |
|---|---:|---:|---:|
| P1 regrowth | 0.14573 | 0.00000 | 0.00000 |
| P2 degradation | 1.27130 | 1.02573 | 0.98665 |
| P3 features | 1.31733 | 1.02396 | 0.94463 |
| P4 action selection | 1.10614 | 1.04562 | 0.96718 |
| P4 intent/projection | 1.43722 | 1.12011 | 1.03027 |
| P5 partition | 0.90984 | 0.95430 | 0.90816 |
| P6A work resolution | 1.53640 | 1.24668 | 1.09747 |
| P6B targeted resolution | 1.30200 | 1.12652 | 1.12374 |
| P7 market | 1.61697 | 1.05300 | 1.07698 |
| P8 welfare | 1.57162 | 1.13166 | 1.08693 |
| P9 mortality | 1.86394 | 1.13910 | 1.05114 |
| P10 metrics | 1.48305 | 1.14408 | 1.06895 |
| P11 events | 1.10557 | 1.04275 | 1.02065 |

P1 is sub-microsecond at these settings, so its alpha is not informative. For the other phases, the 250k→1M result is consistent with near-linear growth at fixed K. The strongest initial transition is P9, but it subsides to alpha 1.14 and 1.05 in the two larger intervals.

## Per-phase time as K increases at N=250,000

Values are median ± MAD in µs/day. Larger MADs at K≥1,250 are retained as measured and make small between-cell differences less conclusive.

| Phase | K=250 | K=500 | K=1,250 | K=2,500 | K=5,000 |
|---|---:|---:|---:|---:|---:|
| P1 regrowth | 0.500 ± 0.100 | 0.800 ± 0.000 | 1.300 ± 0.100 | 2.400 ± 0.500 | 3.300 ± 0.400 |
| P2 degradation | 263.800 ± 2.600 | 268.000 ± 8.500 | 289.300 ± 22.200 | 367.300 ± 35.300 | 378.600 ± 63.800 |
| P3 features | 2,001.400 ± 42.300 | 2,221.600 ± 28.400 | 2,872.500 ± 311.600 | 9,737.700 ± 3,368.000 | 18,885.800 ± 4,087.200 |
| P4 action selection | 12,379.600 ± 62.300 | 12,365.100 ± 68.000 | 12,696.500 ± 1,059.000 | 28,036.500 ± 7,096.600 | 27,356.900 ± 3,962.700 |
| P4 intent/projection | 9,661.700 ± 271.500 | 9,643.100 ± 274.400 | 10,095.100 ± 1,044.800 | 24,713.600 ± 5,885.100 | 26,049.100 ± 3,749.100 |
| P5 partition | 3,534.000 ± 98.200 | 3,596.300 ± 71.200 | 4,248.300 ± 536.900 | 11,009.400 ± 2,860.000 | 15,021.200 ± 2,550.800 |
| P6A work resolution | 4,436.900 ± 164.600 | 4,796.800 ± 410.700 | 6,899.200 ± 1,892.400 | 28,520.000 ± 9,203.300 | 38,512.700 ± 5,149.900 |
| P6B targeted resolution | 15,334.300 ± 368.000 | 16,280.800 ± 464.800 | 18,750.600 ± 2,967.500 | 48,957.000 ± 11,027.900 | 66,340.200 ± 11,497.900 |
| P7 market | 11,417.900 ± 207.000 | 12,063.500 ± 357.800 | 16,807.000 ± 3,328.800 | 45,262.200 ± 6,681.300 | 80,214.400 ± 8,859.300 |
| P8 welfare | 5,116.800 ± 77.100 | 5,121.200 ± 95.300 | 5,780.400 ± 726.100 | 13,482.100 ± 4,362.500 | 15,233.400 ± 3,777.200 |
| P9 mortality | 4,505.700 ± 67.700 | 4,504.200 ± 44.800 | 4,858.000 ± 964.000 | 10,683.700 ± 3,345.300 | 12,174.700 ± 2,881.800 |
| P10 metrics | 10,549.700 ± 849.300 | 10,882.400 ± 470.100 | 11,241.800 ± 3,038.300 | 24,870.300 ± 5,182.300 | 31,798.600 ± 5,179.600 |
| P11 events | 6,411.000 ± 49.400 | 6,797.400 ± 106.100 | 7,980.200 ± 1,172.500 | 21,697.700 ± 4,165.100 | 41,350.500 ± 3,529.900 |

## Settlement-keyed lookup work per day

Counts below are for the observed day-0 M3 storage workload at N=1,000,000. All K settlement groups produced a Phase 5 partition. For each linear `.find`/`.any`, the element count is the number of entries visited up to and including a match (or the full vector on a miss); the duplicate check sums the exact scanned prefix lengths. Phase 11 scans its event-group counter vector, not the settlement slice, but it is also a GroupId-keyed linear search.

| Phase and call site | Operation reached per M3 day | Calls K=500 / K=5,000 | Elements scanned K=500 / K=5,000 |
|---|---|---:|---:|
| P6A `phase6a_work_resolution_storage` in `resolution.rs` | `settlements.iter().find` once in planning and once in commit per partition | 1,000 / 10,000 | 250,500 / 25,005,000 |
| P6B `phase6b_targeted_resolution_storage_m3_serial_impl` in `resolution.rs` | `settlements.iter().any` once per partition | 500 / 5,000 | 125,250 / 12,502,500 |
| P7 `phase7_market_clearance_storage` in `resolution.rs` | `settlements.iter().find` once in planning and once in commit per partition | 1,000 / 10,000 | 250,500 / 25,005,000 |
| P10 `phase10_observe_storage_with_scratch` in `metrics.rs` | `settlements[..i].iter().any` once per settlement to reject duplicate GroupIds | 500 / 5,000 | 124,750 / 12,497,500 |
| P11 M3 runner event sequence assembly in `runner.rs` | `phase6_counts.iter_mut().find` for each Work and targeted resolution group | 1,000 / 10,000 | 250,000 / 25,000,000 |

P3 is not a linear settlement scan on this path: `Phase3ScarcityScratch::lookup` binary-searches the sorted GroupId index for each eligible agent. At N=1,000,000 it made 1,000,000 lookups and 9,980,000 comparisons at K=500, or 13,362,000 comparisons at K=5,000. It built K index entries and performed zero index-sort comparisons because the source settlement list was already ordered. The feature output was already in ascending AgentId order before the final sort at both configurations.

The M3 storage Phase 8 path uses the scratch bucket's settlement index, not a settlement `.find` per group. Phase 1, Phase 3 index preparation, and Phase 10 treasury summation also make single settlement passes; they are not repeated GroupId searches.

## Phase 3 subphase split

The final Phase 3-only rerun uses two warmups and seven samples of three repeated Phase 3 calls with scratch reuse on the day-0 state; Phase 3 is read-only, so each call sees the same state. Values are median ± MAD µs. Timed and untimed `AgentFeatures` were exactly equal. `out` was AgentId-sorted before `sort_by_key` in both cases.

| N | K | Scarcity scratch prepare | Per-agent loop, including lookup | Final sort | Timed Phase 3 total |
|---:|---:|---:|---:|---:|---:|
| 250,000 | 1,250 | 1.100 ± 0.000 | 1,411.000 ± 2.500 | 62.400 ± 0.500 | 1,474.000 ± 3.000 |
| 1,000,000 | 5,000 | 4.800 ± 0.100 | 22,843.800 ± 1,492.500 | 384.800 ± 14.900 | 23,248.300 ± 1,493.600 |

The per-agent loop is the dominant Phase 3 stage. Preparation is proportional to K and the final sort handles an already ordered feature vector. **Inference:** K-sensitive cost in Phase 3 is the per-agent GroupId lookup and its data/branch locality, rather than sorting the K-entry index or sorting the feature output. The current lookup is logarithmic in K, not quadratic.

## Parallel Phase 6B at N=1,000,000, k=16

Six threads and chunk 256 were used. Stage values are median ± MAD µs/day over 21 measured days. The parallel level-build stage has no serial counterpart. `evaluation/writeback/output` includes the stage's sequentially ordered result reconstruction.

| Phase 6B stage | Serial median ± MAD | Parallel median ± MAD | Parallel/serial speedup |
|---|---:|---:|---:|
| Validation / slot lookup | 35,192.000 ± 1,263.800 | 24,220.500 ± 2,728.000 | 1.4530× |
| Key generation | 8,952.300 ± 1,010.900 | 240.500 ± 24.400 | 37.2237× |
| Global sort | 11,852.500 ± 702.300 | 9,180.000 ± 426.800 | 1.2911× |
| Dependency-level build | 0 | 4,446.400 ± 499.300 | — |
| Evaluation, writeback, output | 29,634.400 ± 4,158.800 | 16,986.900 ± 2,924.300 | 1.7445× |

The uninstrumented paired full-tick rerun measured serial `443,620.967 ± 4,053.100 µs`, parallel `431,821.233 ± 12,070.433 µs`, or `1.0273×` speedup. State, Metrics, and Events digests matched for all seven paired samples. The earlier `0.8297×` day-0 result in M3_SCALE_LIMIT.md was **not reproduced** in this rerun. The stage data show a net Phase 6B improvement in this run; the 4.45 ms dependency-level build is new parallel work, but the stage profile does not support it as a cause of a whole-tick regression here.

## Findings and bit-identical fix candidates

### Measured facts

- With K fixed at 500, phase times become close to linear after N=250,000: P3 alphas are 1.02 and 0.94, P6A 1.25 and 1.10, P6B 1.13 and 1.12, P7 1.05 and 1.08, and P10 1.14 and 1.07 over the final two intervals.
- At fixed N=250,000, K-dependent work is visible in P6A, P6B, P7, P10, and P11. The source-level element counts above show repeated settlement or event-group scans growing quadratically with K. Phase 10's duplicate-settlement validation has exactly K(K−1)/2 prefix elements on unique settlement input.
- P3 performs one binary lookup per eligible agent; the lookup comparison count rises from about 10.0 to 13.4 per agent between K=500 and K=5,000. Its feature output needs no reordering for the measured round-robin storage layout.
- The N=1,000,000, k=16 parallel Phase 6B rerun improved each measured parallelizable stage and gave a small positive paired whole-tick result; it did not reproduce the previously measured whole-tick slowdown.

### Inferences and smallest fix candidates

| Area | Inference from these measurements and code | Minimal bit-identical candidate; not implemented |
|---|---|---|
| P6A, P6B, and P7 | Repeated per-partition settlement membership/position scans are direct K² sources. The fixed-K sweep removes K growth and the measured Phase 6A/B/7 scan counts grow with K². | Build one transient GroupId-to-source-index lookup for the day and reuse it in those phases. Preserve source-order first-match behavior, duplicate/missing-settlement error precedence, partition processing order, and commit order. |
| P10 | Prefix `.any` duplicate detection is an independent O(K²) source, even when there are no duplicate settlements. | Replace the prefix scan with membership tracking populated in settlement source order; report the same first duplicate and keep validation precedence unchanged. |
| P11 | Event sequence offsets do an additional K² GroupId search over the event counter vector, independent of settlement lookup. | Keep a transient keyed counter index for offset lookup while emitting and updating events in the existing resolution order. Never use map iteration to order events. |
| P3 | Per-agent binary search is the K-sensitive operation; the index preparation and output sort are small. | Resolve each distinct GroupId's scarcity once per day and reuse it for its agents, or retain the sorted index and cache per-group results. Keep the same source-order duplicate semantics and feature arithmetic. Skip the final sort only after proving the output is already AgentId-sorted, with canonical sort fallback otherwise. |
| P4/P5/P8/P9 at high K | Their fixed-N time increases at K≥2,500, but the M3 storage call paths do not contain the repeated settlement `.find` pattern listed above. This run does not isolate a specific cause; partition count, scratch layout, cache pressure, and allocator effects remain possible explanations. | Do not change semantics from this evidence alone. Add a focused phase/allocation measurement first; retain only a change that passes exact M3-00 parity and improves beyond measurement variation. |
| Prior parallel 0.81× result | It did not recur in the paired rerun. No source-level cause for the earlier run-to-run difference was identified. | No code fix indicated by this measurement. Repeat the paired uninstrumented run if the slowdown recurs before changing the scheduler. |

## Validation and scope

- `cargo fmt --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- The benchmark's serial profile matched `run_m3_storage_days`; the timed/untimed Phase 3 outputs matched; parallel and serial paired M3 runs matched State, Metrics, and Events digests.
- No test suite, golden, snapshot, contract, or CI file was changed. The only source change adds an opt-in Phase 3 timing wrapper that shares the existing feature-generation implementation; it does not alter feature values or ordering.

No simulation optimization was implemented by this diagnosis.
