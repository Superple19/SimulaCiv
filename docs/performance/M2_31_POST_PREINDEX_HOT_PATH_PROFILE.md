# M2-31 Post-Preindex Hot-Path Reprofiling

- **Task ID:** `M2-31`
- **Baseline commit:** `bb7c649f6c76321e5a62f880137ef53bf546c5fc`
- **Runtime:** Full Hybrid Segmented SoA authority, with the M2-30 Phase 4 candidate pre-index
- **Scope:** Reprofiling and analysis only. No production, benchmark, or test changes.
- **Decision:** **ALGORITHM-FIRST**

## 1. Executive Summary

The M2-30 group/action candidate index removed the dominant quadratic Phase 4 target-discovery scan. From N=1,000 to N=10,000, indexed intent alpha was approximately 1.05 in both workload families, and the candidate-index build alpha was approximately 1.0. The actual multi-day Hybrid runner measured full-tick alpha 1.019 for fixed two settlements and 1.071 for approximately fixed group size.

The new profile exposed two remaining settlement-count-dependent scans:

- Phase 3 linearly searches the settlement scarcity list for every eligible agent.
- Phase 8 scans all agents once per settlement to collect welfare recipients.

Both are O(N × K), where K is the settlement count. In the approximately 200 agents per settlement workload, K grows with N. Between N=10,000 and N=20,000, Phase 3 grew about 4.6× and Phase 8 about 3.0×. They do not yet dominate the whole tick at N=20,000, but they are the clearest remaining asymptotic risks.

When K is fixed, O(N × K) is effectively O(N); when K grows with N as in the fixed-group-size workload, it carries O(N²) scaling risk.

The measured N=10,000 hot paths are Phase 4 Selection, Phase 5, Phase 6B, and indexed Phase 4 Intent. SIMD or Rayon may help later, but first remove the Phase 3 and Phase 8 repeated scans.

## 2. Baseline and Correctness

The repository was clean on `master` at baseline commit `bb7c649f6c76321e5a62f880137ef53bf546c5fc`. That commit applies the M2-30 Phase 4 group/action candidate pre-index. The multi-day production entry point is [`run_hybrid_authority_days`](../../crates/sim-model/src/runner.rs#L1096), which owns and reuses Phase 4 candidate-index scratch across its days.

The M2-30 benchmark correctness gate and the M2 SoA gate tests retained the frozen 500-day hashes and snapshot parity at Days 100, 250, and 500:

| Output | Canonical hash |
|---|---|
| State | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` |
| Metrics | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` |
| Events | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` |

The determinism-oracle and M2 SoA gate tests passed. No expected hash, fixture, snapshot schema, or canonical output was changed for this study.

## 3. Benchmark Methodology

Two workload families were profiled:

- **A — fixed settlements:** 2 settlements.
- **B — fixed group size:** approximately 200 agents per settlement.
- **Population:** 100, 250, 500, 1,000, 2,500, 5,000, 10,000, and 20,000.

The M2-30 benchmark uses at least two warm-up ticks and seven samples, reports median and MAD, and averages three days for a full-tick sample. For the M2-31 full-tick measurements, a temporary probe called the real `run_hybrid_authority_days` production API for three days per sample, with two warm-ups and seven samples. The production scratch lived across those three days. The probe covered both workload families through N=20,000.

For phase timings, the probe called the production phase functions in runner order and retained feature, choice, intent, and candidate-index scratch between samples. It reset the initial world/storage outside the phase timers. These phase timings are kernel timings from the production phase APIs; the full-tick figures come from the production runner itself.

Phase subcomponent probes isolate selected operations and are not additive replacements for the production phase totals. Allocation counts below come from source-path inspection; no global allocator instrumentation was used.

Windows ETW stack profiling was attempted with `wpr -start CPU -filemode`, but Windows rejected enabling performance profiling with policy error `0xc5585011`. No ETW stack trace was collected.

## 4. M2-30 Scaling Result

The M2-30 benchmark compared Full Scan against Pre-Index at N=1,000 and N=10,000:

| Workload | Full Scan intent alpha | Pre-Indexed intent alpha | Full Scan tick alpha | Pre-Indexed tick alpha |
|---|---:|---:|---:|---:|
| Fixed settlements (2) | 1.961 | 1.065 | 1.803 | 1.044 |
| Fixed group size (~200) | 1.852 | 1.046 | 1.662 | 1.116 |

The production phase-profile build measurement was 7.9→77.9 µs for workload A and 7.9→78.7 µs for workload B, both near O(N). At N=10,000 the previous candidate discovery inspected 4,635 × 10,000 = 46.35 million slots. The pre-index scans 10,000 slots to build its buckets and performs 4,635 target lookups.

The M2-31 multi-day production runner measured:

| Workload | N=1,000 | N=10,000 | Alpha, 1k→10k | N=20,000 |
|---|---:|---:|---:|---:|
| Fixed settlements (2) | 215.3 µs/tick | 2.250 ms/tick | 1.019 | 4.575 ms/tick |
| Fixed group size (~200) | 215.1 µs/tick | 2.535 ms/tick | 1.071 | 5.215 ms/tick |

Across repeated profile blocks, N=10,000 full-tick medians varied by several percent; the ranges were approximately 2.16–2.25 ms for workload A and 2.48–2.54 ms for workload B. This variation does not change the measured phase ranking or the near-linear 1k→10k whole-tick result.

Phase 4 Intent is no longer the dominant O(N²) bottleneck. At N=10,000 it accounts for about 10–12% of the full tick and its measured alpha is close to 1.0. The fixed-group-size run exposes separate O(N × K) growth in Phase 3 and Phase 8.

## 5. Phase Profile and Scaling

The table reports N=1,000 median values, N=10,000 median ± MAD, phase alpha from N=1,000 to N=10,000, and N=10,000 share of the production full tick. Values are µs/tick. A is fixed settlements; B is fixed group size.

| Phase | N=1k A/B | N=10k A / B | Alpha A / B | Share A / B at 10k |
|---|---:|---:|---:|---:|
| Phase 1 Regrowth | <0.01 / <0.01 | 0.10±0.00 / 0.10±0.00 | below timer resolution | <0.01% / <0.01% |
| Phase 2 Degradation | 0.8 / 0.8 | 8.1±0.1 / 8.2±0.1 | 1.005 / 1.011 | 0.36% / 0.32% |
| Phase 3 Features | 2.7 / 3.4 | 18.7±0.3 / 58.2±3.9 | 0.840 / 1.233 | 0.83% / 2.30% |
| Phase 4 Selection | 23.0 / 24.1 | 301.8±0.7 / 306.2±0.9 | 1.118 / 1.104 | 13.41% / 12.08% |
| Phase 4 Indexed Intent | 23.6 / 23.6 | 260.9±0.7 / 251.3±1.7 | 1.044 / 1.027 | 11.60% / 9.91% |
| Phase 5 Partition | 27.9 / 29.3 | 299.7±4.8 / 342.9±20.9 | 1.031 / 1.068 | 13.32% / 13.52% |
| Phase 6A Work | 6.0 / 7.5 | 71.7±0.7 / 96.3±5.2 | 1.077 / 1.109 | 3.19% / 3.80% |
| Phase 6B Targeted | 19.6 / 21.7 | 269.3±2.7 / 377.1±11.7 | 1.138 / 1.240 | 11.97% / 14.87% |
| Phase 7 Market | 12.6 / 15.8 | 134.8±2.5 / 193.7±23.8 | 1.029 / 1.088 | 5.99% / 7.64% |
| Phase 8 Welfare | 9.0 / 9.7 | 118.8±1.3 / 183.9±6.0 | 1.121 / 1.278 | 5.28% / 7.25% |
| Phase 9 Mortality | 7.6 / 7.6 | 75.9±0.1 / 76.4±0.1 | 0.999 / 1.002 | 3.37% / 3.01% |
| Event Staging | 21.1 / 20.1 | 99.8±3.2 / 106.8±3.6 | 0.675 / 0.725 | 4.44% / 4.21% |
| Phase 10 Metrics | 10.7 / 10.7 | 104.3±0.2 / 106.4±0.6 | 0.989 / 0.998 | 4.64% / 4.20% |
| Phase 11 Event Flush | 14.6 / 6.8 | 94.3±0.6 / 136.6±1.4 | 0.810 / 1.303 | 4.19% / 5.39% |
| **Full production tick** | **215.3 / 215.1** | **2,249.9±44.4 / 2,535.5±7.7** | **1.019 / 1.071** | — |

Phase 1 operates on settlements only and was below reliable per-call timer resolution in the phase replay. Its operation count is O(K), not O(N).

### N=10,000 Top Five

| Workload | Rank | Phase | Latency | Full-tick share |
|---|---:|---|---:|---:|
| A | 1 | Phase 4 Selection | 301.8 µs | 13.4% |
| A | 2 | Phase 5 | 299.7 µs | 13.3% |
| A | 3 | Phase 6B | 269.3 µs | 12.0% |
| A | 4 | Phase 4 Indexed Intent | 260.9 µs | 11.6% |
| A | 5 | Phase 7 | 134.8 µs | 6.0% |
| B | 1 | Phase 6B | 377.1 µs | 14.9% |
| B | 2 | Phase 5 | 342.9 µs | 13.5% |
| B | 3 | Phase 4 Selection | 306.2 µs | 12.1% |
| B | 4 | Phase 4 Indexed Intent | 251.3 µs | 9.9% |
| B | 5 | Phase 7 | 193.7 µs | 7.6% |

At N=1,000 the top phases were P5, P4 Intent, P4 Selection, Event Staging, and P6B in workload A; workload B ranked P5, P4 Selection, P4 Intent, P6B, and Event Staging.

## 6. Remaining Structural Scaling

### Phase 3

[`phase3_observation_and_features_storage_into`](../../crates/sim-model/src/features.rs#L260) builds settlement scarcity values, then each eligible agent uses a linear `.find()` over that list. The resulting search is O(N × K). For fixed group size, K grows with N.

For workload B:

| Population | Settlements | Phase 3 |
|---:|---:|---:|
| 10,000 | 50 | 58.2±3.9 µs |
| 20,000 | 100 | 265.7±2.3 µs |

The observed increase was about **4.6×** for a 2× population increase (local alpha ≈2.2).

### Phase 8

[`phase8_welfare_distribution_storage`](../../crates/sim-model/src/resolution.rs#L3094) scans all N storage slots inside its loop over K settlements while collecting eligible recipients. This is also O(N × K).

For workload B, Phase 8 grew from **183.9±6.0 µs** at N=10,000 to **552.3±6.5 µs** at N=20,000, about **3.0×** for a 2× population increase (local alpha ≈1.6). At N=20,000 it consumed about 10.6% of the full tick. These observations are consistent with the nested scan visible in the implementation.

## 7. Phase 4 Selection

At N=10,000 the production selector took about **302–306 µs**. Isolated primitive probes measured approximately:

| Component | A | B |
|---|---:|---:|
| Feature-read probe | 24.9 µs | 24.9 µs |
| Utility dot and trait math | 65.0 µs | 65.0 µs |
| `stable_softmax` | 102.0 µs | 101.9 µs |
| RNG coordinate/hash | 18.8 µs | 18.8 µs |
| CDF selection | 16.9 µs | 16.7 µs |
| Output push and sort | 4.3 µs | 4.3 µs |

The probes are isolated operations, not additive phase accounting. Softmax was about 34% of selector time at N=10,000 and 42–44% at N=1,000. SIMD could speed up some independent per-agent arithmetic, but dot accumulation order, FMA, `exp`, and cross-platform floating-point behavior put bit-exact replay at risk.

## 8. Phase 4 Indexed Intent

The candidate-index build was about 78 µs at N=10,000 in both workloads. The full build-plus-intent operation was about 261 µs in A and 251 µs in B. Subtracting the separate build probe leaves roughly 183 µs and 173 µs for the combined target selection, validation, intent construction, and output work; this is an estimate rather than an internal timer split.

There were 4,635 targeted initiators and about 2,446 with a nonempty target in the isolated component probe. Each initiator looks up a group/action bucket; self-exclusion performs two binary range searches; logical-to-physical mapping is constant time. Target RNG is only evaluated when a candidate remains. The output is written in choice order. The pre-index scratch and output vectors retain capacity after warm-up.

## 9. Phase 5

At N=10,000, isolated component probes found:

| Component | A | B |
|---|---:|---:|
| Duplicate-initiator HashSet check | 50.3 µs | 50.4 µs |
| Intent clone and sort | 108.9 µs | 116.3 µs |
| Bucket fill | 14.7 µs | 21.2 µs |
| Production phase total | 299.7 µs | 342.9 µs |

The subcomponent probes are not additive substitutes for the production total. Sorting is the largest isolated operation. The current phase also creates a HashSet, cloned intent Vec, partition Vec, and group bucket Vecs per tick. Follow-up candidates are an AgentId-ordered input fast path, scratch reuse, and allocation reduction while retaining the public order-independent behavior.

## 10. Phase 6B

The production phase took **269.3 µs** in A and **377.1 µs** in B at N=10,000. Isolated probes over keyed interactions measured approximately:

| Operation | A | B |
|---|---:|---:|
| ResolutionKey generation | 9.2 µs | 9.4 µs |
| Per-group keyed sort | 31.5 µs | 16.7 µs |
| Two slot lookups per keyed interaction | 20.2 µs | 20.5 µs |

Key generation and sort do not explain the whole phase. B has smaller per-group sorts but a slower full phase, which points to per-group validation, temporary vectors, record construction, and commit overhead as remaining candidates. The ResolutionKey-ordered commit within each settlement remains sequential; parallel work would need to preserve that order. Detailed time for the remaining work could not be separated without ETW or production instrumentation.

## 11. Phase 10

Production Phase 10 measured approximately **104–106 µs** at N=10,000. The benchmark’s subcomponent probe reported about **108 µs** total, including roughly 4.1 µs for food aggregation, less than 0.01 µs for treasury summation, 5.2 µs for wealth ordering, and 8.2 µs for Gini arithmetic. The rest is primarily validation, living/index collection, and scratch setup.

The Hybrid runner currently calls the convenience wrapper, which creates indices and settlement-indices Vecs and a uniqueness HashSet each tick. A scratch-taking API already exists. Any future reuse must preserve canonical living-agent order and the sequential f64 food sum.

## 12. SIMD Assessment

The current machine is an AMD Ryzen 5 9600X with SSE2, AVX2, AVX-512F, and FMA support. CPU feature availability does not imply that the benchmark binary uses those instructions.

| Kernel | Suitability | Determinism risk | Assessment |
|---|---|---|---|
| P2/P9 | High | Low–Medium | Independent contiguous agent updates, but small whole-tick shares |
| P3 | Medium | Medium | Per-agent math is independent; settlement lookup algorithm dominates first |
| P4 Selection | Medium | High | Large share; f32 accumulation, FMA and exp behavior threaten bit-exact parity |
| P4 Intent | Low | Low | HashMap lookup, binary search, and branches |
| P5/P6B | Low | Low | Sorting, irregular memory, branches, and ordered commits |
| P7/P8 | Low–Medium | High for P7; Low–Medium for P8 | Ordered f32/economic semantics; Phase 8 group scan should be fixed first |
| P10 | Low–Medium | High | Canonical f64 sum and ordered wealth/Gini computation |

Current verdict: **LATER**. SIMD should be reconsidered after Phase3/8 algorithm fixes and a bit-exact vector-kernel experiment.

## 13. Rayon Assessment

| Phase | Suitability | Deterministic constraint |
|---|---|---|
| P2 | High | Preserve each agent’s operation order |
| P3 | High after group lookup fix | Indexed output and unchanged per-agent math |
| P4 Selection | High | Stateless per-agent RNG and AgentId-ordered merge |
| P4 Intent | High | Read-only shared index and ordered intent merge |
| P6A | Medium | Keep each settlement’s AgentId-ordered f32 aggregation sequential |
| P6B | Medium across groups, Low within a group | ResolutionKey commit inside each settlement stays sequential |
| P7 | Medium–High across settlements | Keep participant order and per-settlement f32 accumulations fixed |
| P8 | Medium–High after one-pass eligibility collection | Keep canonical AgentId payout/remainder order |
| P9 | High | Merge results in canonical AgentId order |

The M2 contract permits read-only Phase 3/4 concurrency and partitioned settlement execution, but the workspace currently has no Rayon dependency. Current verdict: **LATER**, after the algorithmic work and with deterministic indexed merges.

## 14. Amdahl Upper Bounds

Using measured N=10,000 shares, and assuming only the listed scope is accelerated:

| Candidate | Share A / B | 4× scope speedup A / B | 8× scope speedup A / B |
|---|---:|---:|---:|
| SIMD stable_softmax | 4.5% / 4.0% | 1.035× / 1.031× | 1.040× / 1.035× |
| SIMD entire Phase 4 Selection | 13.4% / 12.1% | 1.112× / 1.100× | 1.133× / 1.118× |
| Rayon P2+P3+P4 Selection+Intent | 26.2% / 24.6% | 1.245× / 1.226× | 1.297× / 1.274× |

These are ideal Amdahl bounds, not measured speedups. At N=20,000 in workload B, Phase3+Phase8 together account for about 15.7% of the full tick; accelerating that combined scope by 4× would cap whole-tick improvement near 1.14×, while removing it entirely would cap it near 1.19×.

## 15. Alignment and Allocation

Sampled SoA Vec pointers had varying mod64 residues; not all columns were 64-byte aligned, and the residue changed across allocations. The element types retain their natural alignment, but ordinary `Vec<T>` does not guarantee 64-byte alignment. Unaligned AVX2/AVX-512 loads are valid; no isolated timing proved that alignment is a current bottleneck.

The design specification says M2 must use 64-byte-aligned arrays, while the optimization contract lists cache-line alignment among permitted layout freedoms. Treat this as storage/performance conformance debt, not a logical correctness failure. See [M2 optimization contract](../contracts/M2_OPTIMIZATION_CONTRACT.md#21-internal-memory-architecture) and [design specification §14.2](../../SIMULACIV_DESIGN_SPECIFICATION.md#142-m2-specific-physical-storage-requirements).

Source-path allocation audit, without global allocator instrumentation:

| Phase | Steady-state observation |
|---|---|
| P4 | Candidate index and caller-owned feature/choice/intent Vecs can reuse capacity after warm-up |
| P5 | HashSet, intent clone/sort, partitions, and group bucket Vecs are created per tick |
| P6B | HashSet and several per-group/per-tick temporary and result Vecs are created |
| P7 | Participant HashSets and per-settlement buyer/seller planning/update Vecs are created |
| P10 | Convenience wrapper creates indices Vecs and uniqueness HashSet per tick |

## 16. GPU Assessment

An AMD Radeon RX 9070 XT was visible on the profiling machine. N=10,000–20,000 full ticks were approximately 2–5 ms. Repeated CPU/GPU transfers, kernel launch overhead, branch-heavy interactions, and deterministic sorting/merge costs make current benefit uncertain. A crossover at roughly N≥100,000 with multi-day GPU-resident state is a future hypothesis, not a measured threshold.

Current verdict: **LATER**.

## 17. Decision and Follow-up Order

**ALGORITHM-FIRST** is the gate decision because Phase3 and Phase8 retain measured O(N × K) scans and grow superlinearly when K scales with N.

Recommended order:

1. **M2-32 — Phase8 group-aware one-pass welfare eligibility**, preserving AgentId payout order and remainder semantics.
2. **M2-33 — Phase3 direct scarcity lookup**, preserving current scarcity values and feature output order.
3. **M2-34 — Reprofile both workload families** after those algorithm changes.
4. Then compare deterministic Rayon over independent Phase4 work with Phase5 allocation/sort reduction.
5. Revisit SIMD, Phase6B temporary-buffer work, and aligned storage using updated whole-runtime shares.

No item in this follow-up list was implemented in M2-31.

## 18. Validation and Scope

The M2-31 profiling gate ran:

- `cargo test --workspace`
- `cargo test -p sim-model --test determinism_oracle_tests -- --nocapture`
- `cargo test -p sim-model --test m2_soa_gate_tests -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench --bench m0_baseline_bench`
- `git diff --check`

All passed. The benchmark correctness gate reported the frozen hashes and bit-exact canonical trajectory. No production source, benchmark, test, fixture, golden, snapshot, hash, or CI file changed for M2-31. This document is the only file in scope.
