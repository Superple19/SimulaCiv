# M2-37 Post-P5/P6B Reprofiling & Parallelism Gate

- **Task:** M2-37
- **Baseline:** `master` at `d1e850c1b80dd8c8e8750af5bbfe3f8d8a4eea71`
- **Runtime under measurement:** production Full Hybrid Segmented SoA authority
- **Included production paths:** M2-30 Phase4 candidate pre-index, M2-32 Phase8 one-pass eligibility, M2-33 Phase3 scarcity lookup, M2-35 Phase5 stable bucketing, M2-36 trusted Phase6B resolution
- **Scope:** profile, analysis, and report only
- **Decision:** **RAYON-FIRST**

## Executive Summary

Full Hybrid ticks remain close to linear through 100,000 agents. The measured 1k→10k exponents are 1.045 for fixed K=2 and 1.064 for fixed group size; the 50k→100k exponents rise to 1.145 and 1.137, respectively. The 100k workload completed within the same benchmark run, so it was retained.

The production phase profile places Phase4 Selection and Indexed Intent together at 27–30% of the paired full-tick measurement at 10k, and about 29–32% at 50k. Adding P2, P3, and P9 gives a deterministic, independent-work candidate set of roughly 33–37%. Its ideal Amdahl gain is material, and output can be merged by fixed chunk position while preserving each agent's scalar calculation and coordinate RNG mapping. The next gate is **RAYON-FIRST**, beginning with Phase4 Selection and Phase4 Indexed Intent.

Phase10 still uses its allocating storage wrapper in the production runner. The existing scratch API removes two Vec allocations, but this run measured little or no repeatable tick-time gain. P5 and P6B remain meaningful phases, though their large full sorts and repeated slot reads have already been removed. SIMD has a smaller measured arithmetic ceiling and a greater bit-exact risk. No cache-miss, bandwidth, alignment-penalty, or GPU-transfer measurements were collected.

## 1. Baseline and Production Path

The workspace was clean on `master` at the stated baseline commit before profiling. The benchmark run exercised `run_hybrid_authority_days` with event and metrics output enabled and snapshot boundaries disabled. This is the production Hybrid runner, so Phase5 uses its ordered owned path and Phase6B uses its trusted ordered path. The public fallback is retained for arbitrary-order inputs and was not used by these production samples.

The host reported an AMD Ryzen 5 9600X with 6 physical cores and 12 logical processors. Rayon is not currently a dependency, and the root manifest has no `[workspace.dependencies]` section.

Workloads:

- **A:** fixed settlements, K=2.
- **B:** approximately 200 agents per settlement, K=`ceil(N/200)`.
- Populations: 100, 250, 500, 1,000, 2,500, 5,000, 10,000, 20,000, 50,000, and 100,000.

## 2. Methodology

The official benchmark used two warm-up blocks and seven timed samples. Each full-tick sample ran three production days from the same initialized starting state, with initialization outside the timer and runner scratch reused during those three days. Event staging and event flush were enabled. Snapshot serialization was excluded.

For the phase table, a temporary profiler called the same production phase APIs in runner order, reused Phase3/4/5/6B/8 scratch for each three-day sample, and timed each phase boundary. It also ran the real production runner on the same initial state and compared the three `DayOutcome` values and final canonical state hash. Those outputs matched. The temporary profiler and its temporary visibility adjustment were removed after measurement; neither is present in the repository changes.

Each listed value is median ± MAD. Phase shares use the median from the adjacent, same-workload, same-process production runner samples collected with the phase samples. The independent full-scaling table below uses production-runner samples from a separate block. Phase timings and isolated component probes are kept distinct; component probes are not additive phase totals.

The phase profiler and separate scaling block differed by up to roughly 20% at N=10k and converged at larger populations. This is treated as process/order variation, not as a code change: the profile shares use their paired runner denominator, and the standalone runner medians are used for the scale table. Both sets are retained so the denominator is visible.

## 3. Full-Tick Scaling

`ticks/s` is `1,000,000 / median µs/tick`. Alpha is `ln(T₂/T₁) / ln(N₂/N₁)` for 1k→10k, 10k→20k, 20k→50k, and 50k→100k.

| N | A K | A µs/tick ± MAD | A ticks/s | B K | B µs/tick ± MAD | B ticks/s |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 2 | 15.77 ± 0.53 | 63,425 | 1 | 14.37 ± 0.20 | 69,606 |
| 250 | 2 | 34.27 ± 0.53 | 29,183 | 2 | 34.67 ± 0.87 | 28,846 |
| 500 | 2 | 72.63 ± 2.30 | 13,768 | 3 | 71.97 ± 1.37 | 13,895 |
| 1,000 | 2 | 145.50 ± 2.63 | 6,873 | 5 | 153.23 ± 3.23 | 6,526 |
| 2,500 | 2 | 379.07 ± 2.10 | 2,638 | 13 | 413.90 ± 10.43 | 2,416 |
| 5,000 | 2 | 792.73 ± 2.00 | 1,262 | 25 | 858.57 ± 4.60 | 1,165 |
| 10,000 | 2 | 1,612.40 ± 4.43 | 620 | 50 | 1,774.57 ± 20.00 | 564 |
| 20,000 | 2 | 3,279.83 ± 1.20 | 305 | 100 | 3,573.40 ± 12.90 | 280 |
| 50,000 | 2 | 9,155.03 ± 73.63 | 109 | 250 | 9,638.33 ± 117.97 | 104 |
| 100,000 | 2 | 20,245.07 ± 240.23 | 49 | 500 | 21,202.10 ± 178.33 | 47 |

| Workload | 1k→10k α | 10k→20k α | 20k→50k α | 50k→100k α |
|---|---:|---:|---:|---:|
| A: fixed K=2 | 1.045 | 1.024 | 1.120 | 1.145 |
| B: fixed group size | 1.064 | 1.010 | 1.083 | 1.137 |

The profile supports near-linear scaling through 50k, followed by mild superlinear growth toward 100k. It does not show the previous N×K Phase4 candidate-scan growth.

## 4. Full Phase Profile

Values are `µs/tick (share of paired production full tick)`. Alpha is shown as `10k→20k / 20k→50k`; P1 is below timer resolution and has no useful alpha.

### A — Fixed K=2

| Phase | 10k µs (share) | 20k µs (share) | 50k µs (share) | α 10→20 / 20→50 |
|---|---:|---:|---:|---:|
| P1 | 0.07 (0.0%) | 0.07 (0.0%) | 0.10 (0.0%) | — |
| P2 | 8.27 (0.4%) | 16.33 (0.4%) | 40.70 (0.4%) | 0.982 / 0.997 |
| P3 | 31.13 (1.6%) | 55.33 (1.5%) | 141.23 (1.5%) | 0.830 / 1.023 |
| P4 Selection | 313.17 (16.0%) | 612.37 (16.3%) | 1,515.07 (16.0%) | 0.967 / 0.989 |
| P4 Indexed Intent | 273.93 (14.0%) | 557.27 (14.9%) | 1,373.20 (14.5%) | 1.025 / 0.984 |
| P5 | 130.83 (6.7%) | 204.50 (5.5%) | 463.47 (4.9%) | 0.644 / 0.893 |
| P6A | 69.27 (3.5%) | 126.90 (3.4%) | 315.07 (3.3%) | 0.873 / 0.992 |
| P6B | 297.97 (15.2%) | 539.20 (14.4%) | 1,317.30 (14.0%) | 0.856 / 0.975 |
| P7 | 141.60 (7.2%) | 285.30 (7.6%) | 676.20 (7.2%) | 1.011 / 0.942 |
| P8 | 108.50 (5.5%) | 186.77 (5.0%) | 524.67 (5.6%) | 0.784 / 1.127 |
| P9 | 81.67 (4.2%) | 164.40 (4.4%) | 432.33 (4.6%) | 1.009 / 1.055 |
| Event staging | 115.87 (5.9%) | 190.97 (5.1%) | 644.77 (6.8%) | 0.721 / 1.328 |
| P10 | 195.80 (10.0%) | 402.93 (10.8%) | 1,150.57 (12.2%) | 1.041 / 1.145 |
| P11 event flush | 84.50 (4.3%) | 173.33 (4.6%) | 458.33 (4.9%) | 1.036 / 1.061 |

### B — Fixed Group Size (~200)

| Phase | 10k µs (share) | 20k µs (share) | 50k µs (share) | α 10→20 / 20→50 |
|---|---:|---:|---:|---:|
| P1 | 0.13 (0.0%) | 0.10 (0.0%) | 0.17 (0.0%) | — |
| P2 | 8.20 (0.4%) | 16.30 (0.4%) | 40.67 (0.4%) | 0.991 / 0.998 |
| P3 | 47.87 (2.2%) | 96.57 (2.5%) | 273.07 (2.7%) | 1.012 / 1.134 |
| P4 Selection | 310.97 (14.3%) | 610.33 (15.6%) | 1,524.63 (15.3%) | 0.973 / 0.999 |
| P4 Indexed Intent | 274.80 (12.6%) | 533.87 (13.7%) | 1,345.80 (13.5%) | 0.958 / 1.009 |
| P5 | 152.37 (7.0%) | 234.93 (6.0%) | 600.73 (6.0%) | 0.625 / 1.025 |
| P6A | 87.80 (4.0%) | 169.67 (4.3%) | 431.70 (4.3%) | 0.950 / 1.019 |
| P6B | 292.43 (13.4%) | 495.13 (12.7%) | 1,299.23 (13.0%) | 0.760 / 1.053 |
| P7 | 211.90 (9.7%) | 379.83 (9.7%) | 1,000.27 (10.0%) | 0.842 / 1.057 |
| P8 | 127.93 (5.9%) | 216.73 (5.5%) | 534.83 (5.4%) | 0.761 / 0.986 |
| P9 | 82.70 (3.8%) | 165.43 (4.2%) | 431.93 (4.3%) | 1.000 / 1.047 |
| Event staging | 110.77 (5.1%) | 181.80 (4.6%) | 432.40 (4.3%) | 0.715 / 0.946 |
| P10 | 224.70 (10.3%) | 441.80 (11.3%) | 1,115.70 (11.2%) | 0.975 / 1.011 |
| P11 event flush | 121.10 (5.6%) | 254.97 (6.5%) | 670.07 (6.7%) | 1.074 / 1.055 |

P1 alpha is intentionally omitted because its timings are at timer resolution. Some smaller phase alphas vary with cache state and bounded settlement counts; the full-tick alpha is the stronger scaling signal.

The listed phase medians account for about 94–97% of their paired runner medians. The remaining 3–6% is runner/context work outside the phase timers and median-combination variation; it is not assigned to a simulation phase.

## 5. M2-35 Phase5 Regression Check

The production runner supplies Phase4 intents in strictly ascending AgentId order. The profiler asserted this condition on every profiled day before calling `phase5_partition_intents_from_vec_with_scratch`; all profiled samples took the ordered bucket path. The runner calls this fast path directly after Phase4. The public arbitrary-order fallback therefore had zero production hits.

On the ordered path, source review and the M2-35 work audit confirm:

- no full `(GroupId, AgentId)` sort over N intents;
- no N-element `Intent` clone;
- no duplicate-initiator HashSet;
- the ascending-AgentId scan and GroupId lookups remain;
- only the K-element partition result is sorted by GroupId; bucket output remains owned and canonical.

The current production phase timer and the M2-35 isolated gate differ:

| Workload | N | M2-35 owned-fast gate µs | M2-37 production P5 µs (share) |
|---|---:|---:|---:|
| A, K=2 | 10k | 76.90 | 130.83 (6.7%) |
| A, K=2 | 20k | 152.40 | 204.50 (5.5%) |
| A, K=2 | 50k | 393.00 | 463.47 (4.9%) |
| B, K=50/100/250 | 10k | 89.10 | 152.37 (7.0%) |
| B, K=50/100/250 | 20k | 180.60 | 234.93 (6.0%) |
| B, K=50/100/250 | 50k | 500.50 | 600.73 (6.0%) |

M2-35 repeatedly measured one fixed Phase4 intent vector, copying it into caller scratch before starting the P5 timer. M2-37 measured P5 after each production Phase4 generation across a three-day sample, with evolving state and the real preceding cache/allocation work. These P5 phase numbers are not an isolated apples-to-apples microbenchmark. The same-process full-tick pair in the current M2-35 gate remained consistent with the earlier result: at N=10k/K=50, P5 was 226.80 µs baseline → 92.10 µs fast, and full tick was 1,895.60 µs baseline → 1,767.17 µs fast. Current whole-tick medians also remain in the same range. The timing difference is recorded as a workload/method difference, not a demonstrated semantic or full-tick regression.

The current M2-30 controlled Phase4 comparison at N=10k is a useful pre-index reference, but it is an ablation with the other current phases enabled, not a historical M2-29 whole-runtime run:

| Workload | Candidate slot visits: full scan → index build | P4 Intent: full scan → indexed (µs) | Full tick: full scan → indexed (µs) |
|---|---:|---:|---:|
| A, K=2 | 46,350,000 → 10,000 | 23,588.97 → 268.20 | 25,220.90 → 1,635.10 |
| B, K=50 | 46,350,000 → 10,000 | 13,651.43 → 248.10 | 15,794.57 → 1,807.17 |

This controlled comparison isolates the old repeated candidate scan. It is kept separate from the cumulative table in section 16 because it is not a historical milestone build.

## 6. M2-36 Phase6B Regression Check

The trusted production path skips the partition-reference sort and zero-target sort, retains keyed `ResolutionKey` sorting, caches structural slots, and re-reads live state before each sequential commit. Arbitrary-order public inputs use the canonical baseline fallback. There is no slot reorder or compaction during Phase6B; the scratch is transient and does not enter world state, storage, snapshots, hashes, events, or result shape.

The sequential-commit regression test passed: interaction A changes a target's state, and ordered interaction B sees the updated state immediately before commit. `cargo test --workspace` also passed the Phase6B canonical/fallback and error-precedence tests.

| N | Baseline structural slot reads | Baseline repeated commit reads | Baseline total | Fast-path structural reads | Repeated reads removed |
|---:|---:|---:|---:|---:|---:|
| 10k | 7,081 | 4,892 | 11,973 | 7,081 | 4,892 |
| 20k | 14,174 | 9,786 | 23,960 | 14,174 | 9,786 |
| 50k | 35,446 | 24,388 | 59,834 | 35,446 | 24,388 |

At N=10k, the keyed ResolutionKey sort performed 26,801 comparisons for K=2 and 14,394 for K=50; it remains in the production path. Partition and zero-target sort comparisons are skipped. M2-36's isolated baseline→scratch Phase6B medians were 236.50→194.90 µs for K=2 and 251.00→193.50 µs for K=50. The full phase profile, which measures evolving production days, was 297.97 µs (15.2%) and 292.43 µs (13.4%) respectively; the two methods use different input/state timing and are reported separately.

The current arbitrary-order fallback probe measured 224.80 µs baseline and 225.60 µs through the dispatcher at N=10k (+0.36%). The trusted production runner does not pay the public dispatch check.

The remaining P6B work is keyed-key generation/sort, immediate dynamic-state reads, sequential commit, owned resolution construction, and the frozen combined-result clone. Component probes at N=10k/K=50 were 7.40 µs key generation, 38.30 µs keyed sort, and 18.00 µs combined-result clone; they are non-additive isolated probes. Removing the combined clone would alter the frozen public result shape, so it is not a compatible target.

## 7. New Top 5 Phases

Alpha is `10k→20k / 20k→50k`. Dominant cost classes are source/profile-based classifications; component probes are not additive.

### A — Fixed K=2

| N | Rank | Phase | Latency / share | Alpha | Dominant cost class |
|---:|---:|---|---:|---:|---|
| 10k | 1 | P4 Selection | 313.17 µs / 16.0% | 0.967 / 0.989 | scalar compute, floating point |
| 10k | 2 | P6B | 297.97 µs / 15.2% | 0.856 / 0.975 | keyed sort, sequential commit, result allocation |
| 10k | 3 | P4 Indexed Intent | 273.93 µs / 14.0% | 1.025 / 0.984 | candidate lookup, branch, validation, writes |
| 10k | 4 | P10 | 195.80 µs / 10.0% | 1.041 / 1.145 | validation, wealth ordering, Gini, allocation |
| 10k | 5 | P7 | 141.60 µs / 7.2% | 1.011 / 0.942 | settlement processing, checked arithmetic |
| 20k | 1 | P4 Selection | 612.37 µs / 16.3% | 0.967 / 0.989 | scalar compute, floating point |
| 20k | 2 | P4 Indexed Intent | 557.27 µs / 14.9% | 1.025 / 0.984 | candidate lookup, branch, validation, writes |
| 20k | 3 | P6B | 539.20 µs / 14.4% | 0.856 / 0.975 | keyed sort, sequential commit, result allocation |
| 20k | 4 | P10 | 402.93 µs / 10.8% | 1.041 / 1.145 | validation, wealth ordering, Gini, allocation |
| 20k | 5 | P7 | 285.30 µs / 7.6% | 1.011 / 0.942 | settlement processing, checked arithmetic |
| 50k | 1 | P4 Selection | 1,515.07 µs / 16.0% | 0.967 / 0.989 | scalar compute, floating point |
| 50k | 2 | P4 Indexed Intent | 1,373.20 µs / 14.5% | 1.025 / 0.984 | candidate lookup, branch, validation, writes |
| 50k | 3 | P6B | 1,317.30 µs / 14.0% | 0.856 / 0.975 | keyed sort, sequential commit, result allocation |
| 50k | 4 | P10 | 1,150.57 µs / 12.2% | 1.041 / 1.145 | validation, wealth ordering, Gini, allocation |
| 50k | 5 | P7 | 676.20 µs / 7.2% | 1.011 / 0.942 | settlement processing, checked arithmetic |

### B — Fixed Group Size (~200)

| N | Rank | Phase | Latency / share | Alpha | Dominant cost class |
|---:|---:|---|---:|---:|---|
| 10k | 1 | P4 Selection | 310.97 µs / 14.3% | 0.973 / 0.999 | scalar compute, floating point |
| 10k | 2 | P6B | 292.43 µs / 13.4% | 0.760 / 1.053 | keyed sort, sequential commit, result allocation |
| 10k | 3 | P4 Indexed Intent | 274.80 µs / 12.6% | 0.958 / 1.009 | candidate lookup, branch, validation, writes |
| 10k | 4 | P10 | 224.70 µs / 10.3% | 0.975 / 1.011 | validation, wealth ordering, Gini, allocation |
| 10k | 5 | P7 | 211.90 µs / 9.7% | 0.842 / 1.057 | settlement processing, checked arithmetic |
| 20k | 1 | P4 Selection | 610.33 µs / 15.6% | 0.973 / 0.999 | scalar compute, floating point |
| 20k | 2 | P4 Indexed Intent | 533.87 µs / 13.7% | 0.958 / 1.009 | candidate lookup, branch, validation, writes |
| 20k | 3 | P6B | 495.13 µs / 12.7% | 0.760 / 1.053 | keyed sort, sequential commit, result allocation |
| 20k | 4 | P10 | 441.80 µs / 11.3% | 0.975 / 1.011 | validation, wealth ordering, Gini, allocation |
| 20k | 5 | P7 | 379.83 µs / 9.7% | 0.842 / 1.057 | settlement processing, checked arithmetic |
| 50k | 1 | P4 Selection | 1,524.63 µs / 15.3% | 0.973 / 0.999 | scalar compute, floating point |
| 50k | 2 | P4 Indexed Intent | 1,345.80 µs / 13.5% | 0.958 / 1.009 | candidate lookup, branch, validation, writes |
| 50k | 3 | P6B | 1,299.23 µs / 13.0% | 0.760 / 1.053 | keyed sort, sequential commit, result allocation |
| 50k | 4 | P10 | 1,115.70 µs / 11.2% | 0.975 / 1.011 | validation, wealth ordering, Gini, allocation |
| 50k | 5 | P7 | 1,000.27 µs / 10.0% | 0.842 / 1.057 | settlement processing, checked arithmetic |

## 8. Phase4 Selection and Indexed Intent

P4 Selection measured 310–313 µs at N=10k, or 14.3–16.0% of its paired full tick. This is close to the M2-34 absolute values (~305 µs). The reported share moved from 14.50% to 16.0% for A and from 14.16% to 14.3% for B; cross-run denominator variation limits the precision of that change. Selection alpha remains close to one through N=50k.

Current N=10k isolated component probes (`µs`, median ± MAD):

| Component | A, K=2 | B, K=50 |
|---|---:|---:|
| Feature access probe | 3.52 ± 0.01 | 3.55 ± 0.00 |
| Utility/trait math | 17.15 ± 0.01 | 17.98 ± 0.62 |
| `stable_softmax` | 101.72 ± 0.30 | 111.90 ± 9.19 |
| Coordinate RNG | 24.18 ± 0.01 | 24.86 ± 0.57 |
| CDF selection | 13.26 ± 0.00 | 13.15 ± 0.06 |
| Output write | 1.96 ± 0.01 | 1.99 ± 0.01 |

These probes isolate kernels and do not sum to whole P4 Selection. The softmax probe is about 5% of the paired full tick, while the whole selection is about 14–16%. A fourfold speedup of the isolated softmax has an ideal whole-tick ceiling near 1.04×. Vectorizing its `exp` or changing f32 arithmetic could also change exact values.

Phase4 Indexed Intent is 262–274 µs at N=10k in the production phase profile. The separate M2-30 candidate-index probe measured an approximately 79–82 µs index build and roughly 248–268 µs indexed-generation probe; these subprobes overlap with the end-to-end phase path and must not be added together. The complete production intent call includes index build, GroupId/action lookup, two `partition_point` searches for self-exclusion, coordinate-RNG target mapping, choice validation, intent construction, and output writes. Only build and complete indexed call were timed separately; the inner lookup/validation/write terms were not independently timed.

The pre-index scan considered 46.35 million candidate slots for 4,635 targeted initiators at N=10k. The index path builds over 10,000 storage slots and performs 4,635 target lookups. RNG coordinates remain per-agent and stateless.

## 9. Phase10 Scratch Assessment

The production Hybrid runner still calls `storage.phase10_metrics`, which delegates to the allocating `phase10_observe_storage` wrapper. The scratch-taking `phase10_observe_storage_with_scratch` API exists and can reuse the `indices` and `settlement_indices` vectors. The wrapper creates two Vecs per tick; for N>32 the metrics validator also creates a HashSet, which scratch reuse does not remove.

The paired isolated probe ran after three production days. Latencies are µs/call; `Δ` is wrapper minus scratch.

| Workload | N/K | Wrapper | Scratch | Δ | Two Vec capacity bytes |
|---|---:|---:|---:|---:|---:|
| A | 10k/2 | 228.64 ± 1.12 | 226.78 ± 0.28 | +1.86 | 80,016 |
| B | 10k/50 | 229.17 ± 0.29 | 228.60 ± 1.89 | +0.57 | 80,400 |
| A | 20k/2 | 479.38 ± 1.65 | 478.16 ± 2.00 | +1.22 | 160,016 |
| B | 20k/100 | 478.17 ± 3.35 | 474.50 ± 0.62 | +3.67 | 160,800 |
| A | 50k/2 | 1,289.91 ± 3.11 | 1,290.83 ± 1.78 | −0.92 | 400,016 |
| B | 50k/250 | 1,293.20 ± 3.10 | 1,296.26 ± 2.31 | −3.06 | 402,000 |

This would remove two Vec allocations and their backing capacity requests per tick, but the run did not find a consistent latency improvement. The measured whole-tick delta was below 0.1% in five of six cells; the N=10k/A cell was about 0.1%. Reuse remains a low-risk cleanup, not the next high-return gate. Eliminating all of P10 would have an ideal Amdahl ceiling around 1.10–1.13× from its 10–12% share, but the two-vector cleanup affects only a small part of P10.

## 10. Remaining P5/P6B Cost

**P5:** the stable fast path already removes the N-element Intent clone, full `(GroupId, AgentId)` sort, and HashSet duplicate validation. Remaining work is the N-order scan, GroupId lookup for each intent, owned bucket writes/allocations, and sorting the K output groups. Current production P5 share is about 5–7%. Additional investment should target owned bucket allocation only if a dedicated allocator or same-process production probe shows material cost; changing the public owned partition behavior is not justified by these results.

**P6B:** the slot cache removes 4,892 repeated commit slot lookups at N=10k; partition sort and zero-target sort are absent in the trusted path, while ResolutionKey sort and sequential commits remain. Full production P6B is 13–15% in most profiled cells. The remaining keyed sort, live-state checks, sequential commit, and output construction leave room for focused profiling, but removing the combined result clone would change the frozen public result shape. More P6B work ranks below the independent P4 candidate set.

## 11. Deterministic Rayon Suitability

Suitability and granularity below use the measured phase profile. Determinism risk is for a carefully indexed implementation, not a tested Rayon implementation.

| Phase | Suitability | Determinism risk | Granularity | Notes |
|---|---|---|---|---|
| P2 | HIGH | LOW | agent chunks | Per-agent state updates; no cross-agent reduction |
| P3 | MEDIUM | LOW–MEDIUM | agent chunks | Preserve each feature's scalar order and merge in AgentId order |
| P4 Selection | HIGH | LOW–MEDIUM | agent chunks | Per-agent utility, softmax, and coordinate RNG are independent |
| P4 Indexed Intent | HIGH | LOW–MEDIUM | choice chunks | Candidate index is built once and can be shared read-only; preserve earliest-error precedence |
| P9 | HIGH | LOW–MEDIUM | agent chunks | Merge mortality output in canonical AgentId order |
| P6A | MEDIUM | MEDIUM | settlement | Groups write disjoint membership, but output must remain GroupId ordered |
| P6B | MEDIUM | MEDIUM–HIGH | settlement | Keep each settlement's ResolutionKey sequence and commits sequential; K=2 is coarse-grained poorly |
| P7 | MEDIUM | MEDIUM | settlement | Keep per-settlement buyer/seller order and checked settlement arithmetic |
| P8 | MEDIUM | MEDIUM | settlement | Keep GroupId payout order and AgentId recipient order |

P4 Selection and Intent are the clearest first target: they have useful chunk-level work, coordinate-based RNG, and output positions that can be fixed before scheduling. P6A/P6B/P7/P8 only become plausible settlement-parallel work at larger K; settlement count is too small for useful parallelism in workload A.

## 12. Rayon Execution Design Sketch

- **Dependency:** if implemented, add Rayon as a direct dependency of `crates/sim-model`; the workspace root currently has no shared dependency table. Keep benchmark-only helpers out of production dependencies.
- **Chunking:** divide canonical AgentId positions into fixed contiguous chunks. Use an indexed range/chunk identifier; do not create one task per agent.
- **P4 Selection:** keep utility accumulation, softmax, coordinate RNG, and CDF order scalar and unchanged within each agent. Each worker writes to chunk-local or preassigned output positions. Merge chunks in ascending chunk index, then preserve AgentId ordering.
- **P4 Intent:** build the group/action candidate index once per tick and share it immutably. Candidate vectors stay AgentId ordered; self-exclusion uses the existing bounded searches; target mapping uses the unchanged coordinate, subsystem, and draw index. Do not add reservation or shared RNG state.
- **Validation:** preserve existing error precedence by tagging per-choice errors with their global choice index and selecting the lowest index after chunk completion; preserve the existing canonical missing-choice check. A serial validation pass is an alternative control if parallel error reconciliation erases the speedup.
- **Scratch ownership:** chunk results are private. If retaining temporary buffers across chunks is beneficial, use standard-library thread-local storage for worker-owned scratch, clear it per chunk, and never place it in WorldState or canonical state. The pre-indexed production path does not need a mutable shared candidate list.
- **Merge:** merge by the fixed chunk index/global choice position, not task completion order or HashMap iteration. HashMap may locate group buckets but must not define result ordering.
- **Snapshot/hash gate:** compare per-day outcomes, event order, state, metrics, and day 100/250/500 snapshots against the existing serial path for each thread/chunk setting.

## 13. Thread-Count and Chunk-Size Experiment Matrix

This host has 6 physical and 12 logical processors. The next Rayon experiment should test:

- threads: `1, 2, 4, 6, 12`;
- chunk sizes: `64, 128, 256, 512, 1024`;
- populations: `1k, 5k, 10k, 20k, 50k`;
- both A and B workloads;
- Phase4 Selection, Phase4 Indexed Intent, and combined full-tick result.

This is 125 thread/chunk/population cells per workload and per measured path. Build each thread pool before the timer and keep it alive for the measured block. Use two warm-up blocks, seven three-day samples, randomized cell order, and a serial production runner control. The one-thread Rayon row measures scheduler overhead; it is not the sequential baseline.

## 14. Amdahl Estimates

The per-agent candidate set is **P2 + P3 + P4 Selection + P4 Indexed Intent + P9**. Shares below are measured at N=10k and N=50k; the projected whole-tick speedups are ideal Amdahl ceilings if that entire set achieves the listed speedup. They are not observed Rayon results.

| Workload / N | Candidate share | Set 2× | Set 4× | Set 6× | Set 8× |
|---|---:|---:|---:|---:|---:|
| A / 10k | 36.2% | 1.221× | 1.372× | 1.431× | 1.463× |
| A / 50k | 37.1% | 1.231× | 1.392× | 1.455× | 1.489× |
| B / 10k | 33.3% | 1.200× | 1.333× | 1.384× | 1.412× |
| B / 50k | 36.2% | 1.221× | 1.373× | 1.432× | 1.464× |

As one realistic model, assume six worker threads and 50%, 70%, or 85% parallel efficiency, giving modeled candidate-set speedups of 3.0×, 4.2×, and 5.1×. The resulting whole-tick estimates are:

| Workload / N | 50% efficiency | 70% efficiency | 85% efficiency |
|---|---:|---:|---:|
| A / 10k | 1.317× | 1.380× | 1.410× |
| A / 50k | 1.329× | 1.394× | 1.425× |
| B / 10k | 1.286× | 1.340× | 1.366× |
| B / 50k | 1.318× | 1.381× | 1.410× |

These models do not subtract scheduler, merge, validation, or memory-bandwidth costs. They are scenario bounds, not expected measured outcomes.

## 15. SIMD, Alignment, and GPU

**SIMD:** `stable_softmax` is the largest isolated P4 arithmetic probe at roughly 102–112 µs/N=10k. A fourfold speedup of softmax alone has an ideal full-tick ceiling near 1.04×; utility math plus softmax remains around a 1.05× ceiling. Accelerating the entire P4 Selection phase has a larger theoretical bound, but Rayon can exploit the whole per-agent phase without changing scalar f32 operation order. SIMD may change accumulation order, enable FMA, use a different `exp`, or vary by ISA; bit-exact replay risk is HIGH. Keep SIMD behind a separate experiment and exact-hash gate.

**Alignment:** SoA columns still use ordinary `Vec<T>` and do not promise 64-byte alignment. No cache-miss, bandwidth, or alignment-penalty counters were collected. Keep alignment as tracked debt; do not rank STORAGE-FIRST without those measurements.

**GPU:** CPU full ticks at N=50k are about 9.2–9.6 ms and at N=100k about 20.2–21.2 ms. No transfer/residency crossover was measured. The remaining work includes branch-heavy intent generation, ordered phases, and sequential Phase6B commit. Keep GPU at **LATER**.

## 16. Cumulative M2 Performance Progression

The M2-34 baseline is taken from the existing M2-34 report. M2-35 values are its owned-fast full-tick medians. M2-36 values show the current benchmark's same-process Phase6B baseline→fast pair; that isolates M2-36 on top of the Phase5 fast path. Cross-milestone ratios are indicative because the entries come from separate benchmark runs.

| Workload | N | M2-34 full tick | M2-35 P5-fast | M2-36 paired P6B baseline → fast | M2-34 → M2-36 fast |
|---|---:|---:|---:|---:|---:|
| A | 10k | 2,108.20 | 1,624.70 | 1,649.57 → 1,628.67 | 1.294× |
| A | 20k | 4,264.37 | 3,263.13 | 3,536.77 → 3,390.03 | 1.258× |
| A | 50k | 10,912.53 | 9,337.63 | 9,807.03 → 9,419.00 | 1.159× |
| B | 10k | 2,153.67 | 1,749.60 | 1,819.60 → 1,786.47 | 1.206× |
| B | 20k | 4,438.63 | 3,596.07 | 3,691.33 → 3,586.33 | 1.238× |
| B | 50k | 11,382.23 | 9,727.13 | 9,835.80 → 9,459.20 | 1.203× |

All values are µs/tick. Within the paired M2-36 samples, P6B improved the whole tick by 1.3%, 4.3%, and 4.1% for A at 10k/20k/50k, and 1.9%, 2.9%, and 4.0% for B. The cumulative ratios do not isolate each milestone; the M2-36 paired comparison is the sound estimate of its local effect.

For pre-index context, the M2-30 controlled Phase4 full-scan variant at N=10k measured 25,220.90 µs in A and 15,794.57 µs in B, against its indexed variants at 1,635.10 and 1,807.17 µs. This is a current-code P4 ablation with the other optimized phases enabled, not an old M2-29 whole-runtime measurement; it is not chained into the cumulative table.

## 17. Next-Milestone Ranking

| Rank | Candidate | Expected whole-tick gain | Complexity | Determinism risk | Public-contract risk | Architectural cleanliness |
|---:|---|---|---|---|---|---|
| 1 | Deterministic Rayon for P4 Selection + Indexed Intent | Ideal 2× candidate-set model: 1.20–1.23× at 10k; 4×: 1.33–1.37×. Six-thread 50–85% model: 1.29–1.41×. Not measured. | High | Low–Medium with fixed merge | Medium: error precedence and output ordering need exact tests | High with immutable index and chunk-local output |
| 2 | Phase10 scratch reuse | Two Vec allocations/tick; this run's latency signal was near zero, mostly below 0.1% whole tick | Low | Low | Low | High |
| 3 | Phase6B additional scratch profiling/cleanup | P6B is 13–15% of production tick; 2× the entire phase would cap near 1.08×, but the remaining compatible portion is smaller | Medium–High | Medium | Medium because order/result shape is frozen | Medium–High if transient only |
| 4 | P5 bucket-allocation follow-up | P5 is about 5–7%; 2× the whole phase caps near 1.03–1.04× | Medium | Low | Low if owned output contract is retained | Medium |
| 5 | P4 SIMD experiment | About 1.04–1.05× ideal for the isolated arithmetic probes; greater if more of Selection is vectorizable | Medium–High | High | Low | Low–Medium |

Alignment stays unranked for lack of hardware-counter evidence; GPU remains LATER.

## 18. Decision Gate

**RAYON-FIRST.** The measured P2/P3/P4 Selection/P4 Indexed Intent/P9 share is 33–37%, with a simple deterministic chunk-and-merge design. Start with P4 Selection and P4 Indexed Intent. Keep scalar arithmetic inside each agent, preserve RNG coordinates, collect output by fixed index, and compare exact outcomes, events, snapshots, and canonical hashes against the serial production runner.

## 19. Validation and Repository State

All requested commands passed after temporary profiling instrumentation was removed:

- `cargo test --workspace`
- `cargo test -p sim-model --test determinism_oracle_tests -- --nocapture` — 66 passed
- `cargo test -p sim-model --test m2_soa_gate_tests -- --nocapture` — 21 passed, including Day 100/250/500 snapshot parity
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench --bench m0_baseline_bench`
- `git diff --check`

The benchmark canonical hashes were bit-exact:

- State: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
- Metrics: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3`
- Events: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`

No production, test, benchmark, fixture, golden, snapshot, canonical expected hash, or CI file changed. The final worktree contains only this uncommitted report; no commit was created.
