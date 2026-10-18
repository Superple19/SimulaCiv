<a id="m3-k-settlement-index-measurement"></a>
# M3-06 (formerly M3-K) Settlement Index Measurement

This report records the fixed-N, fixed-K, and `K=N/200` reruns after M3-K. The change replaces repeated storage-path GroupId scans with one transient source-order-preserving index per phase call. The original [M3 scale diagnosis](M3_SCALE_DIAGNOSIS.md) and [M3 scale-limit measurements](M3_SCALE_LIMIT.md) remain unchanged and provide the before values.

## Host and method

- **Host:** AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors; Windows 11 Pro; `rustc 1.98.1`; `x86_64-pc-windows-msvc`; 95.61 GiB RAM.
- **Workload:** M2-40 equilibrium preset with `base_metabolic_cost=0.3129`, seed `81985529216486895`, replicate `7`, `p_local=0.5`, `k=8`, day 0, metrics and events enabled, snapshots disabled.
- **Sampling:** two warmups, seven samples, three consecutive days per sample with scratch reuse. Per-phase values are median ± MAD over 21 timed day observations. Initialization, relationship graph, and projection construction are outside the timer.
- **Fixed-K series:** `K=500`, `N ∈ {100,000, 250,000, 500,000, 1,000,000}`.
- **Fixed-N series:** `N=250,000`, `K ∈ {250, 500, 1,250, 2,500, 5,000}`. All cells passed the graph-capacity checks; none were skipped.
- **Proportional-K series:** `K=N/200` over the same four populations. The N=100,000 and N=250,000 profiles reuse the cells already sampled in the fixed-K and fixed-N series; the 500,000 and 1,000,000 cells were measured in this run.
- **Phase profile:** the bench-only serial driver runs beside `run_m3_storage_days` from the same initial checkpoint and asserts exact day outcome and State digest equality. Its P11 event counter uses the same GroupId-indexed update as production, with the existing vector still determining event order.
- **Whole tick:** the N=1,000,000, K=5,000 result is a paired serial/parallel run, 2 warmups and 7 interleaved samples, 3 days per sample, using the six-thread pool and chunk size 256. Digest calculation is outside the timed region; paired State, Metrics, and Events digests matched.
- **Command:** `cargo bench -p sim-model --bench m3_scale_limit_bench -- --diagnose`.
- **Total wall time:** 693 seconds (11 min 33 sec), including the fixed-N, fixed-K, proportional-K, Phase 3 subphase, and existing parallel Phase 6B diagnostics.

## Settlement lookup work before and after

The before counts are from the prior call-site analysis in [M3 Scale Diagnosis](M3_SCALE_DIAGNOSIS.md). The after counts are the source-level operations reached by the M3 storage path for the same day-0 workload. Initialization assigns dense GroupIds `0..K`, so these cells use the direct-index representation. Index construction is per phase call; event output continues to use the existing first-seen vector order.

| Phase / operation | Before at K=500 | After at K=500 | Before at K=5,000 | After at K=5,000 |
|---|---:|---|---:|---|
| P3 scarcity | 1,000,000 lookups; 9,980,000 binary-search comparisons | 1,000,000 direct queries; 500 index entries; 0 GroupId comparisons and 0 index-sort comparisons | 1,000,000 lookups; 13,362,000 binary-search comparisons | 1,000,000 direct queries; 5,000 index entries; 0 GroupId comparisons and 0 index-sort comparisons |
| P6A settlement lookup | 1,000 `.find` calls; 250,500 elements scanned | 1,000 direct index queries; no settlement-vector scan | 10,000 `.find` calls; 25,005,000 elements scanned | 10,000 direct index queries; no settlement-vector scan |
| P6B settlement membership | 500 `.any` calls; 125,250 elements scanned | 500 direct index queries; no settlement-vector scan | 5,000 `.any` calls; 12,502,500 elements scanned | 5,000 direct index queries; no settlement-vector scan |
| P7 settlement lookup | 1,000 `.find` calls; 250,500 elements scanned | 1,000 direct index queries; no settlement-vector scan | 10,000 `.find` calls; 25,005,000 elements scanned | 10,000 direct index queries; no settlement-vector scan |
| P10 duplicate validation | 500 prefix `.any` calls; 124,750 elements scanned | 500 duplicate-flag checks after one O(K) index preparation | 5,000 prefix `.any` calls; 12,497,500 elements scanned | 5,000 duplicate-flag checks after one O(K) index preparation |
| P11 event grouping | 1,000 vector `.find` calls; 250,000 counter entries scanned | 1,000 GroupId-map lookups; at most 500 distinct vector entries | 10,000 vector `.find` calls; 25,000,000 counter entries scanned | 10,000 GroupId-map lookups; at most 5,000 distinct vector entries |

The before P6A/P6B/P7/P10/P11 work grows with K because each query scans the relevant vector or prefix. After the change, P6A/P6B/P7 use direct GroupId-to-source-index queries, P10 checks precomputed duplicate markers in source order, and P11 queries a transient map without iterating it. P3 uses direct lookup for these dense IDs and a sorted source-index fallback for sparse or duplicate GroupIds.

## Phase 3 subphases

Values are median ± MAD µs/call from the timing wrapper. The wrapper asserts equality with the untimed output, and the bench recorded the feature output as AgentId-sorted before the final sort in both configurations.

| N | K | Result | Scarcity scratch prepare | Per-agent loop including lookup | Final sort | Total |
|---:|---:|---|---:|---:|---:|---:|
| 250,000 | 1,250 | Before | 1.100 ± 0.000 | 1,411.000 ± 2.500 | 62.400 ± 0.500 | 1,474.000 ± 3.000 |
| 250,000 | 1,250 | After | 2.600 ± 0.100 | 790.600 ± 2.800 | 92.800 ± 3.800 | 884.500 ± 4.300 |
| 1,000,000 | 5,000 | Before | 4.800 ± 0.100 | 22,843.800 ± 1,492.500 | 384.800 ± 14.900 | 23,248.300 ± 1,493.600 |
| 1,000,000 | 5,000 | After | 13.900 ± 1.300 | 3,213.600 ± 7.600 | 365.600 ± 22.800 | 3,591.500 ± 27.200 |

Before values are from the Phase 3-only rerun in [M3 Scale Diagnosis](M3_SCALE_DIAGNOSIS.md). Scratch preparation now builds the direct index, while the per-agent lookup loop fell from 22.844 ms to 3.214 ms at N=1M. The output was already sorted before the final sort; the sort remains in place to preserve the existing canonical fallback.

## Fixed-K scaling alpha

`alpha = log(T₂/T₁) / log(N₂/N₁)`. “Before” is the M3-K baseline recorded in [M3 Scale Diagnosis](M3_SCALE_DIAGNOSIS.md); “after” is this rerun. K is fixed at 500. These are measured medians, not fitted asymptotic estimates.

| Phase | 100k→250k before → after | 250k→500k before → after | 500k→1M before → after |
|---|---:|---:|---:|
| P1 regrowth | 0.14573 → 0.44251 | 0.00000 → -0.16993 | 0.00000 → 0.00000 |
| P2 degradation | 1.27130 → 1.26675 | 1.02573 → 0.98688 | 0.98665 → 1.01857 |
| P3 features | 1.31733 → 0.99552 | 1.02396 → 1.01398 | 0.94463 → 1.00209 |
| P4 action selection | 1.10614 → 1.18857 | 1.04562 → 0.99123 | 0.96718 → 0.98656 |
| P4 intent/projection | 1.43722 → 1.41809 | 1.12011 → 1.12453 | 1.03027 → 1.14864 |
| P5 partition | 0.90984 → 0.89783 | 0.95430 → 0.90654 | 0.90816 → 1.05979 |
| P6A work resolution | 1.53640 → 1.51493 | 1.24668 → 1.46251 | 1.09747 → 1.22839 |
| P6B total | 1.30200 → 1.16382 | 1.12652 → 1.07314 | 1.12374 → 1.31816 |
| P7 market | 1.61697 → 1.68411 | 1.05300 → 1.16551 | 1.07698 → 1.22691 |
| P8 welfare | 1.57162 → 1.54719 | 1.13166 → 1.11794 | 1.08693 → 1.09194 |
| P9 mortality | 1.86394 → 1.81043 | 1.13910 → 1.07848 | 1.05114 → 1.13152 |
| P10 metrics | 1.48305 → 1.49020 | 1.14408 → 1.05773 | 1.06895 → 1.03178 |
| P11 events | 1.10557 → 1.08464 | 1.04275 → 1.00718 | 1.02065 → 1.07849 |

The P3 alpha is near 1 for all three post-change intervals, while its lookup work changed from binary search to one direct query per eligible agent. P1 is sub-microsecond and its alpha is not informative.

## Fixed-N Phase times versus K

N is 250,000. Entries are new median ± MAD µs/day. The prior measurements are in the fixed-N table in [M3 Scale Diagnosis](M3_SCALE_DIAGNOSIS.md).

| Phase | K=250 | K=500 | K=1,250 | K=2,500 | K=5,000 |
|---|---:|---:|---:|---:|---:|
| P1 regrowth | 0.600 ± 0.000 | 0.800 ± 0.100 | 1.200 ± 0.100 | 1.700 ± 0.100 | 2.400 ± 0.200 |
| P2 degradation | 210.000 ± 4.100 | 257.400 ± 25.700 | 210.600 ± 6.300 | 260.300 ± 32.300 | 214.300 ± 9.600 |
| P3 features | 888.000 ± 65.300 | 887.500 ± 68.200 | 910.200 ± 78.600 | 938.300 ± 119.200 | 975.500 ± 130.800 |
| P4 action selection | 11,435.900 ± 615.900 | 11,964.000 ± 911.500 | 12,138.800 ± 948.800 | 12,930.600 ± 995.800 | 12,369.700 ± 927.900 |
| P4 intent/projection | 8,317.300 ± 512.500 | 9,239.300 ± 873.500 | 9,014.400 ± 713.600 | 9,426.600 ± 647.600 | 9,422.800 ± 709.600 |
| P5 partition | 2,705.700 ± 159.200 | 3,424.200 ± 851.700 | 3,544.300 ± 479.100 | 4,422.700 ± 859.300 | 5,366.200 ± 479.200 |
| P6A work resolution | 3,750.500 ± 441.400 | 3,943.600 ± 517.400 | 4,829.200 ± 829.200 | 5,527.600 ± 1,215.800 | 6,387.700 ± 568.500 |
| P6B total | 14,359.600 ± 1,087.000 | 14,566.500 ± 1,027.000 | 16,400.500 ± 1,249.100 | 17,708.100 ± 1,792.200 | 19,082.700 ± 2,531.100 |
| P7 market | 10,080.600 ± 798.100 | 9,946.800 ± 771.800 | 12,842.700 ± 1,238.800 | 15,760.900 ± 2,877.000 | 21,547.300 ± 3,244.200 |
| P8 welfare | 3,756.600 ± 202.000 | 3,750.600 ± 149.700 | 3,895.800 ± 314.200 | 4,414.200 ± 672.100 | 5,191.800 ± 655.000 |
| P9 mortality | 3,386.500 ± 149.900 | 3,317.800 ± 178.500 | 3,499.900 ± 395.600 | 4,250.200 ± 829.600 | 4,589.900 ± 536.400 |
| P10 metrics | 8,175.800 ± 2,306.600 | 8,154.600 ± 2,323.100 | 8,076.800 ± 1,982.800 | 8,131.100 ± 2,783.400 | 9,476.800 ± 1,828.100 |
| P11 events | 6,037.000 ± 260.400 | 6,171.800 ± 337.000 | 6,859.400 ± 404.900 | 7,633.300 ± 643.700 | 8,714.300 ± 627.700 |

Selected old-to-new median comparisons show the K-sensitive scans falling at K=5,000: P3 `18,885.800 → 975.500 µs` (19.4× lower), P6A `38,512.700 → 6,387.700 µs` (6.0×), P6B `66,340.200 → 19,082.700 µs` (3.5×), P7 `80,214.400 → 21,547.300 µs` (3.7×), P10 `31,798.600 → 9,476.800 µs` (3.4×), and P11 `41,350.500 → 8,714.300 µs` (4.7×). These ratios compare medians from the two recorded runs; MADs are shown above for the new run.

## `K=N/200` alpha before and after

The before values are the `k=8`, day-0 serial phase alphas in [M3 Scale Limit](M3_SCALE_LIMIT.md). The after values use the same fixed profile and workload family in this run. Each value is the alpha between adjacent N medians.

| Phase | 100k→250k before → after | 250k→500k before → after | 500k→1M before → after |
|---|---:|---:|---:|
| P1 regrowth | 0.75647 → 0.75647 | 0.41504 → 0.50250 | 0.64386 → 0.95693 |
| P2 degradation | 1.03579 → 0.99277 | 1.09985 → 1.38099 | 1.19835 → 1.28921 |
| P3 features | 1.17405 → 0.97886 | 1.80826 → 1.08337 | 2.13676 → 1.84806 |
| P4 action selection | 1.03399 → 1.12018 | 0.98989 → 1.01877 | 1.10696 → 1.72348 |
| P4 intent/projection | 1.33568 → 1.32762 | 1.20817 → 1.22921 | 1.10100 → 2.77362 |
| P5 partition | 0.65410 → 0.83574 | 1.40562 → 1.28920 | 1.16843 → 2.12812 |
| P6A work resolution | 1.54628 → 1.57506 | 1.63570 → 1.68251 | 1.09747 → 2.14141 |
| P6B total | 1.20225 → 1.16091 | 1.23602 → 1.34393 | 1.18634 → 2.37580 |
| P7 market | 1.62110 → 1.71706 | 1.23308 → 1.38082 | 1.30096 → 2.36486 |
| P8 welfare | 1.35475 → 1.25910 | 1.01253 → 1.46298 | 1.08693 → 2.87924 |
| P9 mortality | 1.63734 → 1.52243 | 1.13910 → 1.48457 | 1.05114 → 2.79205 |
| P10 metrics | 1.16995 → 1.18183 | 1.10293 → 1.22395 | 1.51647 → 2.34764 |
| P11 events | 1.05368 → 1.09701 | 1.17664 → 1.17301 | 1.33468 → 2.09474 |

The final 500k→1M interval has high dispersion in several phase measurements. At N=1M the Phase 4 action-selection MAD was 22.2 ms, Phase 4 intent/projection MAD 33.2 ms, P6A MAD 22.5 ms, P6B MAD 56.6 ms, P7 MAD 60.1 ms, and P10 MAD 40.6 ms. Those alpha values are reported as measured and should not be treated as a clean estimate of asymptotic behavior. The separate paired whole-tick result below had lower relative dispersion.

## Whole-tick result at N=1,000,000

Both rows are day 0, `K=5,000`, `k=8`, `p_local=0.5`, serial M3 storage path. The before value is the prior M3 Scale Limit paired measurement; the after value is the paired measurement in this run.

| Run | Serial median ± MAD µs/tick | Parallel median ± MAD µs/tick | Notes |
|---|---:|---:|---|
| Before | 491,862.000 ± 2,744.733 | 470,940.933 ± 3,768.000 | prior report, pool 6 / chunk 256 |
| After | 404,889.833 ± 8,142.933 | 385,289.167 ± 14,997.967 | paired State/Metrics/Events digest parity; pool 6 / chunk 256 |

The new serial median is 1.215× faster than the prior median (17.7% less time). The paired parallel speedup in the after run was 1.051×. These values are one host's measured results, not a portable performance guarantee.

## Parallel Phase 6B stage profile at N=1,000,000, k=16

Six threads and chunk 256. Values are median ± MAD µs/day. The prior stage profile is from [M3 Scale Diagnosis](M3_SCALE_DIAGNOSIS.md); the new profile uses the same opt-in resolver timing wrapper and exact paired digest checks.

| Phase 6B stage | Serial before | Parallel before | Serial after | Parallel after |
|---|---:|---:|---:|---:|
| Validation / slot lookup | 35,192.000 ± 1,263.800 | 24,220.500 ± 2,728.000 | 33,787.900 ± 1,166.100 | 19,575.900 ± 1,230.300 |
| Key generation | 8,952.300 ± 1,010.900 | 240.500 ± 24.400 | 9,007.400 ± 300.600 | 279.700 ± 22.100 |
| Global sort | 11,852.500 ± 702.300 | 9,180.000 ± 426.800 | 12,758.900 ± 166.800 | 9,243.800 ± 263.600 |
| Dependency-level build | 0 ± 0 | 4,446.400 ± 499.300 | 0 ± 0 | 4,518.200 ± 110.900 |
| Evaluation, writeback, output | 29,634.400 ± 4,158.800 | 16,986.900 ± 2,924.300 | 30,222.200 ± 1,611.900 | 17,191.800 ± 2,540.600 |

The current paired full-tick result was 448,951.867 ± 877.067 µs serial and 428,817.233 ± 1,205.467 µs parallel (1.047×), with exact State, Metrics, and Events parity. The earlier 0.8297× / 0.8132× results in [M3 Scale Limit](M3_SCALE_LIMIT.md) were not reproduced in M3 Scale Diagnosis (1.0273×) or this rerun (1.0470×). The stage table shows no new per-stage regression attributable to the settlement index.

## Validation and limits

- `cargo fmt --check` — passed.
- `cargo clippy --all-targets -- -D warnings` — passed.
- `cargo test --workspace` — passed after each implementation phase group and again with the randomized public-phase parity test; M2/M3 goldens, snapshots, and differential tests passed unchanged.
- `cargo bench -p sim-model --bench m3_scale_limit_bench -- --diagnose` — completed all fixed-K, fixed-N, `K=N/200`, Phase 3 subphase, and parallel Phase 6B diagnostics. The bench's serial/parallel paired digest assertion passed.
- The benchmark log is temporary under ignored `target/`; it is not committed.
- No existing test, golden, snapshot, canonical expected hash, M1 manifest, or CI workflow file changed. The new `m3_settlement_index_tests.rs` adds randomized public-phase parity coverage.

Measurements are from a single Windows host. The N=1M per-phase MADs in the proportional-K profile are high, and per-phase alpha for its last interval is correspondingly uncertain. The fixed-N series shows the repeated lookup work falling as expected; the N=1M full-tick paired median also decreased, but this report does not attribute all of that whole-tick difference to the index alone.
