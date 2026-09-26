# M2-34 Post-Algorithm Reprofiling & Next Optimization Gate

- **Task ID:** M2-34
- **Baseline commit:** 28b85a963d212e70d5c4f236ccc82af74dca601c
- **Runtime:** Full Hybrid Segmented SoA authority
- **Included optimizations:** M2-30 Phase4 candidate pre-index; M2-32 Phase8 one-pass welfare eligibility; M2-33 Phase3 scarcity lookup index
- **Scope:** Profiling and analysis only. No production, benchmark, or test changes.
- **Decision:** **P5-FIRST**

## Executive Summary

The M2-30, M2-32, and M2-33 structural scaling debt is no longer visible in the production profile. Full ticks remain near-linear through N=50,000, with both workload families around alpha 1.0 beyond N=10,000. The leading phases are P5, P6B, P4 Selection, P4 Indexed Intent, and P10. P5 is the next target because it accounts for 13–15% of the tick, its sort and allocation costs are measurable, and a verified fast path can retain the arbitrary-order public fallback. The decision is **P5-FIRST**.

## 1. Correctness and Canonical Parity

The profiling gate changed no production source, benchmark, test, fixture, or CI files. Profiling instrumentation ran outside the repository; this document records its results.

공식 benchmark의 canonical hash가 기대값과 일치했고 Day 100/250/500 snapshot parity도 통과했습니다.

- State: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
- Metrics: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b`
- Events: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`

test, fixture, golden, snapshot, canonical hash, CI 파일 변경도 없습니다.

## 2. 측정 방법

Production full tick은 `run_hybrid_authority_days` 경로로 측정했습니다. 두 warm-up block 이후, 7개 sample에서 각 3일을 실행하고 tick당 median 및 MAD를 계산했습니다. 매 sample의 초기 state 생성은 timer 밖에서 했고, production runner scratch는 각 3일 sample 안에서 재사용했습니다.

Phase profile은 같은 phase API를 production 순서로 실행했고 Phase3 indexed scratch와 Phase8 one-pass scratch를 재사용했습니다. Phase1부터 Phase11 event flush까지 측정했습니다. M2-31과 비교할 수 있도록 `snapshot_boundary=false`로 두었으며, snapshot serialization은 profile에 포함하지 않았습니다. Phase timer의 share는 별도 production full-tick median으로 나눴습니다.

## 3. Full-tick scaling

| N | A: K=2 µs/tick ± MAD | A ticks/s | B: K | B: µs/tick ± MAD | B ticks/s |
|---:|---:|---:|---:|---:|
| 100 | 21.33 ± 0.43 | 46,875 | 1 | 14.90 ± 0.13 | 67,114 |
| 250 | 45.27 ± 1.07 | 22,091 | 2 | 36.07 ± 0.23 | 27,726 |
| 500 | 93.23 ± 1.20 | 10,726 | 3 | 77.57 ± 1.87 | 12,892 |
| 1,000 | 191.47 ± 7.60 | 5,223 | 5 | 171.00 ± 6.20 | 5,848 |
| 2,500 | 532.10 ± 12.60 | 1,879 | 13 | 466.50 ± 10.97 | 2,144 |
| 5,000 | 1,047.63 ± 12.60 | 955 | 25 | 1,024.13 ± 11.27 | 976 |
| 10,000 | 2,108.20 ± 9.03 | 474 | 50 | 2,153.67 ± 4.73 | 464 |
| 20,000 | 4,264.37 ± 48.17 | 235 | 100 | 4,438.63 ± 176.10 | 225 |
| 50,000 | 10,912.53 ± 72.67 | 92 | 250 | 11,382.23 ± 213.50 | 88 |

- **A α:** 1k→10k `1.042`, 10k→20k `1.016`, 20k→50k `1.025`
- **B α:** 1k→10k `1.100`, 10k→20k `1.043`, 20k→50k `1.028`

고정 그룹 크기의 1k→10k 구간은 작은 N의 startup 비용 영향을 받지만, 10k 이후에는 선형에 가까운 scaling입니다.

## 4. Full phase profile

수치는 median `µs/tick (full-tick share%)`입니다. α는 `1k→10k / 10k→20k`입니다. P1은 timer resolution 이하라 α 비교에서 제외해야 합니다.

**Workload A — fixed settlements K=2**

| Phase | 10k | 20k | α |
|---|---:|---:|---:|
| P1 | 0.03 (0.00%) | 0.13 (0.00%) | timer floor |
| P2 | 8.13 (0.39%) | 16.57 (0.39%) | 0.972 / 1.026 |
| P3 | 23.77 (1.13%) | 57.13 (1.34%) | 0.862 / 1.265 |
| P4 Selection | 305.60 (14.50%) | 609.30 (14.29%) | 1.030 / 0.996 |
| P4 Indexed Intent | 264.90 (12.57%) | 563.90 (13.22%) | 1.027 / 1.090 |
| P5 | 307.83 (14.60%) | 638.47 (14.97%) | 1.120 / 1.052 |
| P6A | 70.27 (3.33%) | 142.73 (3.35%) | 1.021 / 1.022 |
| P6B | 300.17 (14.24%) | 655.33 (15.37%) | 1.183 / 1.126 |
| P7 | 138.10 (6.55%) | 277.90 (6.52%) | 1.008 / 1.009 |
| P8 | 94.30 (4.47%) | 212.00 (4.97%) | 0.983 / 1.169 |
| P9 | 76.43 (3.63%) | 157.10 (3.68%) | 1.001 / 1.039 |
| Event staging | 97.97 (4.65%) | 211.27 (4.95%) | 0.724 / 1.109 |
| P10 | 193.33 (9.17%) | 400.23 (9.39%) | 1.048 / 1.050 |
| P11 Event flush | 101.27 (4.80%) | 207.37 (4.86%) | 0.904 / 1.034 |

**Workload B — fixed group size (~200 agents/group)**

| Phase | 10k | 20k | α |
|---|---:|---:|---:|
| P1 | 0.07 (0.00%) | 0.17 (0.00%) | timer floor |
| P2 | 8.17 (0.38%) | 16.33 (0.37%) | 0.991 / 1.000 |
| P3 | 37.47 (1.74%) | 86.23 (1.94%) | 1.176 / 1.203 |
| P4 Selection | 304.87 (14.16%) | 606.43 (13.66%) | 1.044 / 0.992 |
| P4 Indexed Intent | 250.67 (11.64%) | 519.97 (11.71%) | 1.018 / 1.053 |
| P5 | 278.60 (12.94%) | 593.30 (13.37%) | 1.183 / 1.091 |
| P6A | 86.70 (4.03%) | 170.60 (3.84%) | 1.087 / 0.977 |
| P6B | 315.83 (14.66%) | 630.03 (14.19%) | 1.185 / 0.996 |
| P7 | 189.77 (8.81%) | 380.00 (8.56%) | 1.094 / 1.002 |
| P8 | 101.37 (4.71%) | 206.60 (4.65%) | 1.027 / 1.027 |
| P9 | 76.43 (3.55%) | 156.60 (3.53%) | 1.002 / 1.035 |
| Event staging | 96.60 (4.49%) | 185.63 (4.18%) | 1.458 / 0.942 |
| P10 | 198.13 (9.20%) | 425.77 (9.59%) | 1.060 / 1.104 |
| P11 Event flush | 139.80 (6.49%) | 287.50 (6.48%) | 1.292 / 1.040 |

## 5. P3/P8 regression check

**Phase3:** fixed-group workload에서 direct lookup의 latency는 N=1k/10k/20k/50k에 각각 약 `2.50/37.47/86.23/250.43 µs`였습니다. α는 `1.176`, `1.203`, `1.164`로, K가 커져도 기존 N×K scan 형태의 성장은 재현되지 않았습니다. 직접 비교 probe에서 K=50의 N=10k lookup 비교 수는 선형 기준 255,000회에서 67,600회로, K=100/N=20k는 1,010,000회에서 154,800회로 줄었습니다.

**Phase8:** one-pass 경로는 eligibility용 storage 방문을 N회로 유지합니다. M2-32 direct probe는 K=50/100에서 기존 500k/2M 방문을 10k/20k 방문으로 줄였고, one-pass α는 1k→10k `1.000`, 10k→20k `0.985`였습니다. 이번 profile에서도 B의 P8은 10k `101.37 µs`, 20k `206.60 µs`, 50k `564.20 µs`로 α가 각각 `1.027`, `1.096`이었습니다.

Baseline 설정에서 N=10k A/B와 첫 4일 동안 Phase8 eligible recipient는 0명이었습니다. 별도 production Phase8 stress probe에서 food를 threshold 이하로 만들어 N=10k 전체를 eligible하게 하자 recipient 10,000건의 payout 실행이 추가로 A에서 약 159µs, B에서 약 138µs 걸렸습니다. 이 stress 수치는 production baseline과 분리한 micro/probe 값입니다.

## 6. N=10k/20k Top 5

각 항목은 `phase: latency/share, α`입니다. Bottleneck class는 profile과 소스 경로를 함께 본 분류입니다.

**A — fixed settlements**

- N=10k: P5 `307.83µs/14.60%, α1.120` (sort/allocation); P4 Selection `305.60/14.50%, 1.030` (compute/floating-point); P6B `300.17/14.24%, 1.183` (sort/allocation/sequential commit); P4 Intent `264.90/12.57%, 1.027` (lookup/branch); P10 `193.33/9.17%, 1.048` (validation/sort/allocation).
- N=20k: P6B `655.33/15.37%, α1.126`; P5 `638.47/14.97%, 1.052`; P4 Selection `609.30/14.29%, 0.996`; P4 Intent `563.90/13.22%, 1.090`; P10 `400.23/9.39%, 1.050`.

**B — fixed group size**

- N=10k: P6B `315.83µs/14.66%, α1.185`; P4 Selection `304.87/14.16%, 1.044`; P5 `278.60/12.94%, 1.183`; P4 Intent `250.67/11.64%, 1.018`; P10 `198.13/9.20%, 1.060`.
- N=20k: P6B `630.03/14.19%, α0.996`; P4 Selection `606.43/13.66%, 0.992`; P5 `593.30/13.37%, 1.091`; P4 Intent `519.97/11.71%, 1.053`; P10 `425.77/9.59%, 1.104`.

## 7. Phase5 deep profile

N=10k production P5 was `307.83 µs` in A and `278.60 µs` in B. Isolated probes on generated Phase4 intents measured duplicate HashSet validation at about 51µs, sort-only at about 143µs (A)/154µs (B), and bucket fill at about 108µs/48µs. The probes are isolated sub-operations and are not additive substitutes for the production phase total.

The generated input was AgentId-sorted in both workloads, but not `(GroupId, AgentId)`-sorted. Group assignment cycles by AgentId, so simply skipping the canonical pair sort does not apply at K>1.

## 8. Phase5 allocation findings

At N=10k, `Intent` is 20 bytes, so the cloned intent vector requests about **200 KB per tick**. Source inspection shows a per-tick duplicate-initiator HashSet, cloned-and-sorted Vec, growing output partition Vec, and a new `current_intents` Vec for each group after `mem::take`. Estimated inner bucket capacity is ~328 KB for K=2 and ~256 KB for K=50; these are capacity estimates, not allocator-instrumented byte counts. Approximate inner-vector growth is about 28 allocations for two 5k-agent buckets versus ~450 growth steps for fifty ~200-agent buckets. No global allocator instrumentation was used. The production runner reuses Phase4 buffers, but Phase5 returns newly owned partition buckets every tick.

A stable GroupId bucket path could use AgentId-sorted input while appending to each group in source order, then emit buckets by sorted GroupId. It would need a verified fast path and a fallback for arbitrary unsorted public input.

## 9. Phase6B deep profile

Production profile latency was `300.17/315.83 µs` A/B at N=10k and `655.33/630.03 µs` at N=20k. B is about 5% slower at 10k but faster at 20k; there is no consistent large B-only penalty.

At N=10k the generated input had 4,635 targeted intents: 2,189 zero-target and 2,446 keyed. A separate day-0 component probe measured about 7,081 structural slot lookups, 11.3µs ResolutionKey generation, 32.9/24.2µs per-group keyed sorting A/B, and 72/93µs cloning the returned resolution records. The commit lookup probe varied between roughly 19 and 46µs across runs, so treat these as isolated estimates, not additive production totals.

Phase6B validates settlement/agent structure before mutation, sorts partitions by GroupId, classifies zero-target and keyed intents, sorts keyed interactions by ResolutionKey, then performs live-state checks and sequential commits. Its returned structure contains both the zero/keyed streams and combined resolutions: at 4,635 records, the duplicated record payload is approximately 445 KB (`9,270 × 48-byte records`) before Vec capacity overhead. B has more per-group vectors and output clones; its keyed sorts are smaller, which partly offsets that cost.

## 10. Phase6B optimization candidates

- **ResolutionKey sort:** cannot be skipped from AgentId input order; hashed keys do not follow that order.
- **Upstream ordering:** Phase5 already emits GroupId-sorted partitions and AgentId-sorted intents. A verified fast path could avoid sorting partition refs and already sorted zero-target records, with fallback for external unsorted callers.
- **Group-local scratch:** reusable keyed-item staging may reduce temporary allocations. Zero-target and keyed result vectors become returned owned output, limiting reuse.
- **Slot lookup:** structural validation and commit repeat stable AgentId→slot lookups. Cache slot indices during validation but keep live food, eligibility and sequential commit checks in place.
- **Validation hoisting:** structural membership checks could feed the slot cache; dynamic state validation must stay immediately before each ordered interaction.
- **Result clone:** the combined `resolutions` field duplicates the zero/keyed records. Removing that clone would change the public result shape, so it is not a compatible cleanup under the frozen contract.

## 11. Phase4 Selection

At N=10k, the full phase measured `305.60µs` A and `304.87µs` B, or 14.50%/14.16% of full tick. M2-31 measured about 301.8/306.2µs; absolute time is effectively unchanged while its share increased as other work was reduced.

Independent component probes measured approximately:

| Component | µs |
|---|---:|
| Utility and trait math | 64.5 |
| `stable_softmax` | 101.2 |
| Coordinate PRNG | 26.2 |
| CDF selection | 12.6 |
| Output push/sort | 5.5 |

These probes are not additive; they account for about 210µs against a roughly 280–305µs whole-selection measurement. Softmax alone is about 4.8% of full tick; the whole selection is about 14%.

## 12. Rayon suitability

Suitability uses current measured whole-tick shares; no parallel implementation was tested.

| Phase | Suitability / granularity | Determinism risk | Current share |
|---|---|---|---:|
| P2 | High, per chunk | Low; unique slots, no reduction | 0.38–0.39% |
| P3 | Medium, per chunk after lookup build | Low; ordered output merge | 1.13–1.74% |
| P4 Selection | High, per chunk | Low–Medium; coordinate RNG, AgentId merge | 14.16–14.50% |
| P4 Intent | High, per chunk with chunk-local candidate scratch | Low–Medium; indexed ordered merge | 11.64–12.57% |
| P6B | Medium, per settlement only | Medium; keep each settlement’s keyed commit sequential | 14.24–14.66% |
| P7 | Medium, per settlement | Medium; preserve local buyer/seller ordering | 6.55–8.81% |
| P8 | Medium, per settlement after one-pass grouping | Medium; preserve GroupId and AgentId payout order | 4.47–4.71% |
| P9 | High, per chunk | Low–Medium; AgentId result merge | 3.55–3.63% |

The workspace has no Rayon dependency. Settlement-level work is a poor fit for A’s K=2; B has K=50 at 10k and K=100 at 20k, but the 2–4µs/group P7/P8 work may still be smaller than scheduling overhead.

## 13. Threading granularity

At N=1k, P4 Selection is only about 28µs and P4 Intent about 24µs; pool overhead is likely to erase gains. Use per-chunk rather than per-agent tasks, with 128–256 agents/chunk as a measurement starting point. N=10k provides roughly 40–80 chunks and N=20k 80–160, where P4 work is substantial enough to test. This suggests a likely crossover around 5k–10k, not a measured threshold.

Settlement-level threading is weak for A at all measured populations. For B, P6B may cross over around 10k–20k (K=50–100); P7/P8 have less work per settlement and may need K closer to 100 or more. These crossover estimates require a real Rayon A/B benchmark.

## 14. SIMD suitability

| Kernel | Suitability | Bit-exact risk | Whole-tick ceiling |
|---|---|---|---:|
| P4 utility dot / `stable_softmax` | Medium; largest arithmetic share | High: f32 accumulation order, FMA and `exp` implementation | Utility+softmax 4×: ~1.06× ideal |
| Whole P4 Selection | Medium | High for altered math/reduction | 4×: ~1.12× ideal |
| P2 | High per element, tiny share | Medium; preserve per-agent operation order | 4×: ~1.003× |
| P3 | Medium; lookup/branch limits arithmetic share | Low–Medium | 4×: ~1.01× |
| P8 | Low–Medium; grouping and payouts dominate | Medium | 4×: ~1.01× |
| P9 | Medium–High per element, small share | Low–Medium | 4×: ~1.01× |

FMA, f32 reductions and ISA-dependent `exp` are meaningful replay risks. SIMD is not the first gate.

## 15. Amdahl estimates

Ideal ceilings from measured N=10k shares; these are not achieved speedups. “Rayon set” conservatively includes per-agent P2/P3/P4 Selection/P4 Intent/P9 only.

| Accelerated scope | Measured share (A/B) | 2× whole-tick A/B | 4× A/B | 8× A/B |
|---|---:|---:|---:|---:|
| P5 | 14.60% / 12.94% | 1.079 / 1.069 | 1.123 / 1.107 | 1.146 / 1.128 |
| P6B | 14.24% / 14.66% | 1.077 / 1.079 | 1.120 / 1.124 | 1.142 / 1.147 |
| P4 Selection | 14.50% / 14.16% | 1.078 / 1.076 | 1.122 / 1.119 | 1.145 / 1.141 |
| Conservative Rayon set | 32.22% / 31.47% | 1.192 / 1.187 | 1.319 / 1.309 | 1.393 / 1.380 |
| P5+P6B, both accelerated | 28.84% / 27.60% | 1.168 / 1.160 | 1.276 / 1.261 | 1.338 / 1.318 |

## 16. Phase10 allocation cleanup

Production currently calls `storage.phase10_metrics`, which allocates `indices` and `settlement_indices` Vecs each tick. The scratch-taking API exists. Reusing those buffers removes **two Vec allocations/tick**; on 64-bit, estimated backing bytes are 80,016 at N=10k/K=2, 80,400 at N=10k/K=50, 160,800 at N=20k/K=100, and 402,000 at N=50k/K=250. The internal uniqueness HashSet allocation remains.

Post-three-day-state wrapper-vs-scratch probes ranged from effectively flat to an 18.6% P10 reduction for one 10k A sample; whole-tick impact ranged from negligible to about 2.4%. Global allocation counts were not instrumented. This is a low-risk cleanup, but its runtime gain is less predictable than the P5/P6B hotspots.

## 17. Alignment and GPU

The SoA columns use ordinary `Vec<T>`, which does not guarantee 64-byte alignment; this remains the documented alignment debt. This run collected no cache-miss or memory-bandwidth counters, so there is no evidence to prioritize storage alignment.

At 50k, full ticks were 10.91–11.38ms. The remaining hot paths are branch-, allocation-, ordering- and sequential-commit-heavy; no GPU residency/transfer crossover was measured. Keep GPU at **LATER** and do not rank STORAGE-FIRST.

## 18. Next milestone ranking

| Rank | Candidate | Expected whole-tick gain basis | Complexity | Semantic/determinism risk | Size | Cleanliness |
|---:|---|---|---|---|---|---|
| 1 | P5 stable GroupId bucketing for verified AgentId order, fallback for arbitrary input | P5 is 12.9–14.6%; ideal 2×/4× gives ~1.07–1.08×/~1.11–1.12× | Medium | Medium | Medium | High |
| 2 | P6B slot cache, ordering fast paths, temporary scratch | P6B is 14.2–14.7%; ideal 2×/4× gives ~1.08×/~1.12× | High | Medium–High | Medium–Large | Medium |
| 3 | Deterministic Rayon for chunks and independent settlements | Conservative candidate set 4×/8× ceiling ~1.31–1.32×/~1.38–1.39× | High | Medium | Large | High with ordered merges |
| 4 | Phase10 Vec scratch reuse | Two allocations/tick removed; measured whole-tick effect near zero to ~2.4% | Low | Low | Small | High |
| 5 | P4 Selection SIMD experiment | P4 is ~14%; ideal 4× ceiling ~1.12× | Medium–High | High | Medium | Low–Medium |

## 19. Decision gate, validation and status

**Final decision: P5-FIRST.** It is a top-two phase in both workloads, has a measurable sort/allocation component, and offers a narrower experiment than Rayon or SIMD. Preserve the public arbitrary-order behavior with a checked fast path and fallback.

Validation passed:

- `cargo test --workspace`
- `cargo test -p sim-model --test determinism_oracle_tests -- --nocapture` — 66 passed
- `cargo test -p sim-model --test m2_soa_gate_tests -- --nocapture` — 18 passed
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench --bench m0_baseline_bench`
- `git diff --check`


The profiling baseline was clean on branch master at commit 28b85a963d212e70d5c4f236ccc82af74dca601c. The profiling gate did not modify production, benchmark, or test files.
