# M2-40 Final Runtime Profile & M2 Graduation Gate

- **Baseline:** `master` at `329aa564d3a2bf9d569e576448bf373dbffe093e`; working tree clean before profiling
- **Runtime:** Hybrid Segmented SoA authority
- **Production modes measured:** Serial (`run_hybrid_authority_days`) and explicit Rayon (`run_hybrid_authority_days_with_phase4_policy`)
- **Reference Rayon profile:** caller-owned 6-thread pool, chunk size 256; host-specific measurement configuration only
- **Decision:** **M2-GRADUATE**

## Executive summary

The M2 runtime meets its architectural target through 100k agents: native Segmented SoA authority, eliminated N×K candidate scans, exact replay parity, and an explicit deterministic Phase4 Rayon opt-in while the serial runner remains the default. Both execution modes complete a 100k-agent tick in a practical 17–22 ms on the measured host. From 50k to 100k, whole-tick alpha ranges from 1.058 to 1.102; A has phase-local P5/P6A/P6B timing outliers, but they do not move aggregate alpha above 1.058. M2-39's three Auto-policy runs remain part of the evidence: all-population thresholds were `none → 3,500 → none` and selected chunks were `128 → 128 → 256`; Auto remains experimental.

The 200k extension remains runnable at about 38–48 ms/tick and has alpha 1.118–1.192 from 100k. That range is mildly superlinear and is retained as follow-up evidence, not treated as proof of an N² hot path. At 200k, B has 1,000 groups; settlement-count work and per-group output allocation deserve another profile if that population becomes a normal target.

P4 remains the largest serial phase family (roughly 29–37% of profile time). Explicit Rayon roughly halves its measured P4 time and improves full ticks by 1.08–1.22× at 10k–100k for the fixed 6/256 reference. After that speedup, P6B and P10 commonly lead the profile. P2/P3/P9 together account for only about 6.6–7.3% of Serial ticks; a modeled 3× speedup of all three would yield about 1.05× whole-tick gain before scheduler and merge costs. P10 vector scratch reuse remains below 0.2% observed whole-tick benefit. Neither result warrants another M2 implementation gate.

**M2-GRADUATE.** No correctness debt remains. Move additional constant-factor, allocation, host-specific parallel tuning, alignment, and GPU work to follow-up milestones. Auto stays experimental; the serial production runner remains default.

## 1. Baseline and objective

The baseline commit was `329aa564d3a2bf9d569e576448bf373dbffe093e` on `master`, with a clean worktree. The report-only task made no persistent production, test, benchmark, fixture, golden, hash, snapshot, or CI changes.

The binding M2 contract requires a high-throughput runtime while preserving the frozen M0 semantics and canonical State/Metrics/Events digests. The gate rechecks the current authoritative SoA path, serial production, explicit Rayon opt-in, scaling, remaining optimization costs, and the frozen replay contract. Auto was excluded from performance baselines.

## 2. Host, explicit Rayon reference, and workloads

The host is an AMD Ryzen 5 9600X, 6 physical cores and 12 logical processors, running Rust `1.98.1`. The explicit Rayon reference uses one prebuilt caller-owned 6-thread pool and chunk size 256 for all samples. Six threads match the physical core count; chunk 256 was selected as a simple representative from the M2-39 candidate set. The three M2-39 policy runs selected different chunks, so 256 is not a portable default or a claim of universal optimality.

- **A:** fixed K=2 settlements.
- **B:** approximately 200 agents per settlement, `K = ceil(N/200)`.
- **Measured populations:** 100, 250, 500, 1k, 2.5k, 5k, 10k, 20k, 50k, 100k, and 200k.

The caller-owned pool is constructed before timing and reused. Each sample starts from the same initialized world, with initialization outside the timer, and runs three consecutive production days so the runner reuses scratch between days. Serial and explicit Rayon are interleaved in alternating order. Metrics and events are enabled; snapshot boundaries are disabled. Each mode/population cell has two warmups and seven timed samples. Tables report median ± MAD; throughput is `1,000,000 / median µs/tick`.

Phase profiles use the same production runner and phase call boundaries, with temporary timers accumulated over three days and scratch reused across those days. Their share denominator is the instrumented run's own wall time; the separate full-tick table is the uninstrumented paired runner measurement. The two profile blocks varied by several percent and are shown separately rather than adding the phase medians into the full-tick table. P4 intent includes its serial candidate-index build and API validation/merge; additional isolated probes below break out candidate construction and Rayon intent stages. Phase subcomponent probes are non-additive. P11 here means event validation/order/flush; snapshot serialization is off.

## 3. Serial and Explicit Rayon full-tick results

All times are µs/tick. Rayon uses the 6-thread / 256-chunk reference profile. The benchmark pairs each mode on the same input; speedup is Serial median divided by Rayon median.

| Workload | N / K | Serial µs ± MAD | Serial ticks/s | Explicit Rayon µs ± MAD | Rayon ticks/s | Speedup |
|---|---:|---:|---:|---:|---:|---:|
| A | 100 / 2 | 22.33 ± 0.47 | 44,776 | 38.47 ± 0.43 | 25,997 | 0.581× |
| A | 250 / 2 | 46.10 ± 0.27 | 21,692 | 72.50 ± 3.20 | 13,793 | 0.636× |
| A | 500 / 2 | 94.10 ± 2.80 | 10,627 | 116.47 ± 2.93 | 8,586 | 0.808× |
| A | 1,000 / 2 | 184.67 ± 4.70 | 5,415 | 204.87 ± 8.90 | 4,881 | 0.901× |
| A | 2,500 / 2 | 498.57 ± 14.57 | 2,006 | 467.07 ± 4.27 | 2,141 | 1.067× |
| A | 5,000 / 2 | 969.37 ± 14.27 | 1,032 | 852.67 ± 17.07 | 1,173 | 1.137× |
| A | 10,000 / 2 | 1,984.37 ± 27.87 | 504 | 1,769.00 ± 24.90 | 565 | 1.122× |
| A | 20,000 / 2 | 3,860.40 ± 73.83 | 259 | 3,364.50 ± 55.80 | 297 | 1.147× |
| A | 50,000 / 2 | 9,783.93 ± 116.97 | 102 | 8,122.00 ± 203.17 | 123 | 1.205× |
| A | 100,000 / 2 | 20,371.40 ± 121.27 | 49 | 16,997.30 ± 353.93 | 59 | 1.199× |
| A | 200,000 / 2 | 44,539.83 ± 425.63 | 22 | 37,517.07 ± 407.20 | 27 | 1.187× |
| B | 100 / 1 | 17.43 ± 1.10 | 57,361 | 43.93 ± 9.03 | 22,762 | 0.397× |
| B | 250 / 2 | 35.30 ± 0.43 | 28,329 | 59.03 ± 0.50 | 16,940 | 0.598× |
| B | 500 / 3 | 78.17 ± 2.53 | 12,793 | 114.47 ± 4.67 | 8,736 | 0.683× |
| B | 1,000 / 5 | 154.83 ± 2.63 | 6,459 | 176.23 ± 4.03 | 5,674 | 0.879× |
| B | 2,500 / 13 | 409.47 ± 3.33 | 2,442 | 407.17 ± 7.93 | 2,456 | 1.006× |
| B | 5,000 / 25 | 897.50 ± 26.43 | 1,114 | 806.33 ± 40.27 | 1,240 | 1.113× |
| B | 10,000 / 50 | 1,846.40 ± 69.40 | 542 | 1,666.33 ± 52.77 | 600 | 1.108× |
| B | 20,000 / 100 | 3,850.83 ± 72.70 | 260 | 3,385.83 ± 111.77 | 295 | 1.137× |
| B | 50,000 / 250 | 10,081.03 ± 44.73 | 99 | 8,662.20 ± 117.70 | 115 | 1.164× |
| B | 100,000 / 500 | 21,632.20 ± 194.00 | 46 | 18,210.73 ± 228.07 | 55 | 1.188× |
| B | 200,000 / 1,000 | 48,333.77 ± 374.07 | 21 | 41,204.73 ± 694.17 | 24 | 1.173× |

At 100k, explicit Rayon reduces Serial full-tick medians from 20.37 to 17.00 ms in A and 21.63 to 18.21 ms in B. The 200k extension remains below 50 ms/tick in both modes.

## 4. Full-tick scaling

Alpha is `ln(T₂/T₁) / ln(N₂/N₁)`. The graduation range ends at 100k; the 100k→200k row is an extension.

| Workload / mode | 1k→10k | 10k→20k | 20k→50k | 50k→100k | 100k→200k extension |
|---|---:|---:|---:|---:|---:|
| A Serial | 1.031 | 0.960 | 1.015 | 1.058 | 1.129 |
| A Explicit Rayon | 0.936 | 0.927 | 0.962 | 1.065 | 1.142 |
| B Serial | 1.076 | 1.060 | 1.050 | 1.102 | 1.160 |
| B Explicit Rayon | 0.976 | 1.023 | 1.025 | 1.072 | 1.178 |

The 50k→100k B/Serial alpha is 1.102, so that interval was checked phase by phase. B's corresponding phases stay close to linear. A has repeatable phase-local alpha outliers in P5/P6A/P6B (about 1.27–1.65 across the final paired profiles) despite fixed K=2; their whole-tick shares remain about 5%, 5%, and 13–15%, and A's full-tick alpha is 1.058. The data therefore point to phase-local working-set/allocation/cache effects, not a demonstrated quadratic scan; no hardware counters were collected to attribute the cause. The 100k→200k measurements are mildly superlinear, most noticeably in B as settlement count reaches 1,000. That extension remains practical but is beyond the core graduation range; keep high-K lookup and allocation behavior on the follow-up list. Do not multiply milestone results into a cumulative speedup chain.

## 5. Serial production phase profile

Phase values are median µs/tick; parenthesized values are shares of the instrumented Serial profile run. The `P4 intent pipeline` includes the serial candidate-index build and the intent API's validation/output work. P10 includes the daily-metrics event append when events are enabled. P1 is at timer resolution.

### A — fixed K=2

| Phase | 10k µs (share) | 20k µs (share) | 50k µs (share) | 100k µs (share) |
|---|---:|---:|---:|---:|
| P1 regrowth | 0.03 (0.00%) | 0.03 (0.00%) | 0.07 (0.00%) | 0.00 (0.00%) |
| P2 degradation | 8.17 (0.51%) | 16.33 (0.50%) | 40.77 (0.46%) | 81.77 (0.40%) |
| P3 features | 22.57 (1.40%) | 44.73 (1.36%) | 145.83 (1.64%) | 284.87 (1.41%) |
| P4 Selection | 316.47 (19.60%) | 632.37 (19.26%) | 1,588.80 (17.91%) | 3,147.57 (15.53%) |
| P4 Intent pipeline | 274.27 (16.99%) | 554.50 (16.89%) | 1,441.23 (16.24%) | 3,028.07 (14.94%) |
| P5 bucketing | 77.50 (4.80%) | 153.00 (4.66%) | 393.57 (4.44%) | 1,004.13 (4.95%) |
| P6A work | 59.73 (3.70%) | 122.53 (3.73%) | 320.63 (3.61%) | 1,004.17 (4.95%) |
| P6B targeted | 197.40 (12.23%) | 421.37 (12.83%) | 1,117.07 (12.59%) | 3,062.47 (15.11%) |
| P7 market | 128.13 (7.94%) | 265.97 (8.10%) | 674.97 (7.61%) | 1,449.80 (7.15%) |
| P8 welfare | 92.43 (5.72%) | 183.90 (5.60%) | 486.67 (5.49%) | 1,068.47 (5.27%) |
| P9 mortality | 76.40 (4.73%) | 156.13 (4.76%) | 413.93 (4.67%) | 987.30 (4.87%) |
| Event staging | 79.83 (4.94%) | 156.90 (4.78%) | 607.27 (6.84%) | 1,277.63 (6.30%) |
| P10 metrics | 191.77 (11.88%) | 395.70 (12.05%) | 1,094.23 (12.33%) | 2,492.00 (12.29%) |
| P11 event flush | 80.67 (5.00%) | 173.00 (5.27%) | 464.73 (5.24%) | 1,056.63 (5.21%) |

### B — about 200 agents per settlement

| Phase | 10k µs (share) | 20k µs (share) | 50k µs (share) | 100k µs (share) |
|---|---:|---:|---:|---:|
| P1 regrowth | 0.07 (0.00%) | 0.10 (0.00%) | 0.27 (0.00%) | 0.27 (0.00%) |
| P2 degradation | 8.13 (0.46%) | 16.27 (0.45%) | 41.93 (0.39%) | 81.63 (0.39%) |
| P3 features | 36.50 (2.07%) | 83.50 (2.32%) | 288.47 (2.66%) | 592.90 (2.85%) |
| P4 Selection | 314.50 (17.87%) | 634.07 (17.61%) | 1,599.27 (14.77%) | 3,156.60 (15.17%) |
| P4 Intent pipeline | 261.17 (14.84%) | 538.70 (14.96%) | 1,448.57 (13.38%) | 2,902.87 (13.95%) |
| P5 bucketing | 91.73 (5.21%) | 185.17 (5.14%) | 606.70 (5.60%) | 1,112.47 (5.34%) |
| P6A work | 78.03 (4.43%) | 164.33 (4.57%) | 584.57 (5.40%) | 1,152.60 (5.54%) |
| P6B targeted | 198.03 (11.25%) | 420.87 (11.69%) | 1,292.93 (11.94%) | 2,509.17 (12.06%) |
| P7 market | 170.50 (9.69%) | 355.33 (9.87%) | 1,148.20 (10.61%) | 2,255.40 (10.84%) |
| P8 welfare | 94.97 (5.40%) | 198.77 (5.52%) | 544.47 (5.03%) | 1,160.53 (5.58%) |
| P9 mortality | 76.00 (4.32%) | 155.80 (4.33%) | 415.43 (3.84%) | 847.33 (4.07%) |
| Event staging | 92.80 (5.27%) | 173.57 (4.82%) | 478.57 (4.42%) | 908.97 (4.37%) |
| P10 metrics | 195.00 (11.08%) | 403.47 (11.21%) | 1,186.03 (10.96%) | 2,339.37 (11.24%) |
| P11 event flush | 119.10 (6.77%) | 254.83 (7.08%) | 729.90 (6.74%) | 1,432.00 (6.88%) |

## 6. Explicit Rayon production phase profile

The pool is used only by P4 Selection and indexed Intent. P2/P3/P5/P6A/P6B/P7/P8/P9, event staging, P10, and P11 remain serial. This keeps Rayon time in P4 rather than attributing it to unrelated phases.

### A — fixed K=2

| Phase | 10k µs (share) | 20k µs (share) | 50k µs (share) | 100k µs (share) |
|---|---:|---:|---:|---:|
| P1 regrowth | 0.03 (0.00%) | 0.03 (0.00%) | 0.07 (0.00%) | 0.03 (0.00%) |
| P2 degradation | 10.23 (0.77%) | 20.07 (0.71%) | 41.13 (0.57%) | 80.80 (0.50%) |
| P3 features | 23.03 (1.73%) | 45.13 (1.61%) | 144.53 (1.99%) | 281.33 (1.73%) |
| P4 Selection | 119.67 (9.00%) | 251.53 (8.95%) | 667.77 (9.19%) | 1,235.83 (7.62%) |
| P4 Intent pipeline | 172.60 (12.99%) | 377.07 (13.42%) | 801.73 (11.03%) | 1,705.07 (10.51%) |
| P5 bucketing | 82.57 (6.21%) | 164.60 (5.86%) | 397.80 (5.47%) | 993.60 (6.13%) |
| P6A work | 65.07 (4.90%) | 126.77 (4.51%) | 305.83 (4.21%) | 648.67 (4.00%) |
| P6B targeted | 202.97 (15.27%) | 424.20 (15.09%) | 1,096.23 (15.08%) | 2,769.77 (17.07%) |
| P7 market | 129.93 (9.78%) | 268.47 (9.55%) | 667.20 (9.18%) | 1,434.87 (8.85%) |
| P8 welfare | 92.17 (6.94%) | 186.30 (6.63%) | 496.60 (6.83%) | 1,138.57 (7.02%) |
| P9 mortality | 76.33 (5.74%) | 155.77 (5.54%) | 411.13 (5.66%) | 909.40 (5.61%) |
| Event staging | 80.37 (6.05%) | 161.63 (5.75%) | 640.80 (8.82%) | 1,305.27 (8.05%) |
| P10 metrics | 193.00 (14.52%) | 401.67 (14.29%) | 1,061.13 (14.60%) | 2,480.47 (15.29%) |
| P11 event flush | 80.77 (6.08%) | 172.33 (6.13%) | 458.63 (6.31%) | 1,054.50 (6.50%) |

### B — about 200 agents per settlement

| Phase | 10k µs (share) | 20k µs (share) | 50k µs (share) | 100k µs (share) |
|---|---:|---:|---:|---:|
| P1 regrowth | 0.07 (0.00%) | 0.10 (0.00%) | 0.23 (0.00%) | 0.27 (0.00%) |
| P2 degradation | 10.43 (0.70%) | 18.37 (0.60%) | 43.27 (0.50%) | 81.93 (0.47%) |
| P3 features | 36.83 (2.46%) | 83.27 (2.70%) | 279.07 (3.20%) | 593.40 (3.40%) |
| P4 Selection | 125.17 (8.37%) | 268.23 (8.70%) | 639.60 (7.34%) | 1,252.53 (7.17%) |
| P4 Intent pipeline | 175.37 (11.72%) | 342.90 (11.12%) | 852.33 (9.77%) | 1,906.57 (10.91%) |
| P5 bucketing | 92.87 (6.21%) | 186.37 (6.05%) | 566.60 (6.50%) | 1,110.70 (6.35%) |
| P6A work | 89.03 (5.95%) | 174.33 (5.66%) | 462.40 (5.30%) | 933.03 (5.34%) |
| P6B targeted | 202.00 (13.50%) | 419.77 (13.62%) | 1,189.43 (13.64%) | 2,298.77 (13.15%) |
| P7 market | 170.13 (11.37%) | 357.73 (11.60%) | 1,094.27 (12.55%) | 2,220.43 (12.70%) |
| P8 welfare | 93.63 (6.26%) | 191.43 (6.21%) | 536.37 (6.15%) | 1,091.80 (6.25%) |
| P9 mortality | 76.23 (5.10%) | 157.23 (5.10%) | 418.23 (4.80%) | 844.40 (4.83%) |
| Event staging | 93.17 (6.23%) | 176.37 (5.72%) | 469.53 (5.38%) | 903.23 (5.17%) |
| P10 metrics | 195.03 (13.03%) | 400.10 (12.98%) | 1,125.43 (12.91%) | 2,393.00 (13.69%) |
| P11 event flush | 120.00 (8.02%) | 252.13 (8.18%) | 737.87 (8.46%) | 1,442.30 (8.25%) |

## 7. Phase scaling alpha

P1 is below useful timer resolution and is omitted. Alphas are from the phase profile medians, not from the separate full-tick table. The 50k→100k phase rows explain the near-linear full-tick result; timer variation in small phases is not interpreted as a structural algorithmic change.

| Mode | Workload | Phase | 10→20k | 20→50k | 50→100k |
|---|---|---|---:|---:|---:|
| Serial | A | P2 | 1.000 | 0.998 | 1.004 |
| Serial | A | P3 | 0.987 | 1.290 | 0.966 |
| Serial | A | P4 Selection | 0.999 | 1.005 | 0.986 |
| Serial | A | P4 Intent pipeline | 1.016 | 1.042 | 1.071 |
| Serial | A | P5 bucketing | 0.981 | 1.031 | 1.351 |
| Serial | A | P6A work | 1.037 | 1.050 | 1.647 |
| Serial | A | P6B targeted | 1.094 | 1.064 | 1.455 |
| Serial | A | P7 market | 1.054 | 1.016 | 1.103 |
| Serial | A | P8 welfare | 0.992 | 1.062 | 1.135 |
| Serial | A | P9 mortality | 1.031 | 1.064 | 1.254 |
| Serial | A | Event staging | 0.975 | 1.477 | 1.073 |
| Serial | A | P10 metrics | 1.045 | 1.110 | 1.187 |
| Serial | A | P11 event flush | 1.101 | 1.078 | 1.185 |
| Serial | B | P2 | 1.000 | 1.033 | 0.961 |
| Serial | B | P3 | 1.194 | 1.353 | 1.039 |
| Serial | B | P4 Selection | 1.012 | 1.010 | 0.981 |
| Serial | B | P4 Intent pipeline | 1.045 | 1.080 | 1.003 |
| Serial | B | P5 bucketing | 1.013 | 1.295 | 0.875 |
| Serial | B | P6A work | 1.074 | 1.385 | 0.979 |
| Serial | B | P6B targeted | 1.088 | 1.225 | 0.957 |
| Serial | B | P7 market | 1.059 | 1.280 | 0.974 |
| Serial | B | P8 welfare | 1.066 | 1.100 | 1.092 |
| Serial | B | P9 mortality | 1.036 | 1.070 | 1.028 |
| Serial | B | Event staging | 0.903 | 1.107 | 0.926 |
| Serial | B | P10 metrics | 1.049 | 1.177 | 0.980 |
| Serial | B | P11 event flush | 1.097 | 1.148 | 0.972 |
| Rayon | A | P2 | 0.972 | 0.783 | 0.974 |
| Rayon | A | P3 | 0.970 | 1.270 | 0.961 |
| Rayon | A | P4 Selection | 1.072 | 1.066 | 0.888 |
| Rayon | A | P4 Intent pipeline | 1.127 | 0.823 | 1.089 |
| Rayon | A | P5 bucketing | 0.995 | 0.963 | 1.321 |
| Rayon | A | P6A work | 0.962 | 0.961 | 1.085 |
| Rayon | A | P6B targeted | 1.064 | 1.036 | 1.337 |
| Rayon | A | P7 market | 1.047 | 0.994 | 1.105 |
| Rayon | A | P8 welfare | 1.015 | 1.070 | 1.197 |
| Rayon | A | P9 mortality | 1.029 | 1.059 | 1.145 |
| Rayon | A | Event staging | 1.008 | 1.503 | 1.026 |
| Rayon | A | P10 metrics | 1.057 | 1.060 | 1.225 |
| Rayon | A | P11 event flush | 1.093 | 1.068 | 1.201 |
| Rayon | B | P2 | 0.816 | 0.935 | 0.921 |
| Rayon | B | P3 | 1.177 | 1.320 | 1.088 |
| Rayon | B | P4 Selection | 1.100 | 0.948 | 0.970 |
| Rayon | B | P4 Intent pipeline | 0.967 | 0.994 | 1.161 |
| Rayon | B | P5 bucketing | 1.005 | 1.214 | 0.971 |
| Rayon | B | P6A work | 0.969 | 1.065 | 1.013 |
| Rayon | B | P6B targeted | 1.055 | 1.137 | 0.951 |
| Rayon | B | P7 market | 1.072 | 1.220 | 1.021 |
| Rayon | B | P8 welfare | 1.032 | 1.124 | 1.025 |
| Rayon | B | P9 mortality | 1.044 | 1.068 | 1.014 |
| Rayon | B | Event staging | 0.921 | 1.069 | 0.944 |
| Rayon | B | P10 metrics | 1.037 | 1.129 | 1.088 |
| Rayon | B | P11 event flush | 1.071 | 1.172 | 0.967 |

At 200k, the instrumented extension shows increased phase alpha in several paths, while the full-tick range remains 44.5–48.3 ms Serial and 37.5–41.2 ms Explicit Rayon. This higher-K regime is recorded as a follow-up profile target; it is outside the 100k graduation scaling criterion.

## 8. Current Top 5

Each phase is ranked independently by its measured median at the workload/population. Alpha is 10k→20k / 20k→50k / 50k→100k from the phase profile. The optimization class describes the dominant work, not a promise that it is safe or profitable to optimize.

| Mode | Workload | N | Rank | Phase | Latency | Share | Alpha 10→20 / 20→50 / 50→100 | Optimization class |
|---|---|---:|---:|---|---:|---:|---|---|
| Serial | A | 10000 | 1 | P4 selection | 316.47 µs | 19.60% | 0.999 / 1.005 / 0.986 | scalar per-agent math; parallelizable |
| Serial | A | 10000 | 2 | P4 intent pipeline | 274.27 µs | 16.99% | 1.016 / 1.042 / 1.071 | candidate build, validation, chunk work/merge |
| Serial | A | 10000 | 3 | P6B targeted | 197.40 µs | 12.23% | 1.094 / 1.064 / 1.455 | key sort, live-state reads, sequential commits, result copies |
| Serial | A | 10000 | 4 | P10 metrics | 191.77 µs | 11.88% | 1.045 / 1.110 / 1.187 | HashSet validation, index/Gini ordering |
| Serial | A | 10000 | 5 | P7 market | 128.13 µs | 7.94% | 1.054 / 1.016 / 1.103 | sequential market planning/checked settlement updates |
| Rayon | A | 10000 | 1 | P6B targeted | 202.97 µs | 15.27% | 1.064 / 1.036 / 1.337 | key sort, live-state reads, sequential commits, result copies |
| Rayon | A | 10000 | 2 | P10 metrics | 193.00 µs | 14.52% | 1.057 / 1.060 / 1.225 | HashSet validation, index/Gini ordering |
| Rayon | A | 10000 | 3 | P4 intent pipeline | 172.60 µs | 12.99% | 1.127 / 0.823 / 1.089 | candidate build, validation, chunk work/merge |
| Rayon | A | 10000 | 4 | P7 market | 129.93 µs | 9.78% | 1.047 / 0.994 / 1.105 | sequential market planning/checked settlement updates |
| Rayon | A | 10000 | 5 | P4 selection | 119.67 µs | 9.00% | 1.072 / 1.066 / 0.888 | scalar per-agent math; parallelizable |
| Serial | A | 50000 | 1 | P4 selection | 1588.80 µs | 17.91% | 0.999 / 1.005 / 0.986 | scalar per-agent math; parallelizable |
| Serial | A | 50000 | 2 | P4 intent pipeline | 1441.23 µs | 16.24% | 1.016 / 1.042 / 1.071 | candidate build, validation, chunk work/merge |
| Serial | A | 50000 | 3 | P6B targeted | 1117.07 µs | 12.59% | 1.094 / 1.064 / 1.455 | key sort, live-state reads, sequential commits, result copies |
| Serial | A | 50000 | 4 | P10 metrics | 1094.23 µs | 12.33% | 1.045 / 1.110 / 1.187 | HashSet validation, index/Gini ordering |
| Serial | A | 50000 | 5 | P7 market | 674.97 µs | 7.61% | 1.054 / 1.016 / 1.103 | sequential market planning/checked settlement updates |
| Rayon | A | 50000 | 1 | P6B targeted | 1096.23 µs | 15.08% | 1.064 / 1.036 / 1.337 | key sort, live-state reads, sequential commits, result copies |
| Rayon | A | 50000 | 2 | P10 metrics | 1061.13 µs | 14.60% | 1.057 / 1.060 / 1.225 | HashSet validation, index/Gini ordering |
| Rayon | A | 50000 | 3 | P4 intent pipeline | 801.73 µs | 11.03% | 1.127 / 0.823 / 1.089 | candidate build, validation, chunk work/merge |
| Rayon | A | 50000 | 4 | P4 selection | 667.77 µs | 9.19% | 1.072 / 1.066 / 0.888 | scalar per-agent math; parallelizable |
| Rayon | A | 50000 | 5 | P7 market | 667.20 µs | 9.18% | 1.047 / 0.994 / 1.105 | sequential market planning/checked settlement updates |
| Serial | A | 100000 | 1 | P4 selection | 3147.57 µs | 15.53% | 0.999 / 1.005 / 0.986 | scalar per-agent math; parallelizable |
| Serial | A | 100000 | 2 | P6B targeted | 3062.47 µs | 15.11% | 1.094 / 1.064 / 1.455 | key sort, live-state reads, sequential commits, result copies |
| Serial | A | 100000 | 3 | P4 intent pipeline | 3028.07 µs | 14.94% | 1.016 / 1.042 / 1.071 | candidate build, validation, chunk work/merge |
| Serial | A | 100000 | 4 | P10 metrics | 2492.00 µs | 12.29% | 1.045 / 1.110 / 1.187 | HashSet validation, index/Gini ordering |
| Serial | A | 100000 | 5 | P7 market | 1449.80 µs | 7.15% | 1.054 / 1.016 / 1.103 | sequential market planning/checked settlement updates |
| Rayon | A | 100000 | 1 | P6B targeted | 2769.77 µs | 17.07% | 1.064 / 1.036 / 1.337 | key sort, live-state reads, sequential commits, result copies |
| Rayon | A | 100000 | 2 | P10 metrics | 2480.47 µs | 15.29% | 1.057 / 1.060 / 1.225 | HashSet validation, index/Gini ordering |
| Rayon | A | 100000 | 3 | P4 intent pipeline | 1705.07 µs | 10.51% | 1.127 / 0.823 / 1.089 | candidate build, validation, chunk work/merge |
| Rayon | A | 100000 | 4 | P7 market | 1434.87 µs | 8.85% | 1.047 / 0.994 / 1.105 | sequential market planning/checked settlement updates |
| Rayon | A | 100000 | 5 | Event staging | 1305.27 µs | 8.05% | 1.008 / 1.503 / 1.026 | event conversion, per-settlement staging allocations |
| Serial | B | 10000 | 1 | P4 selection | 314.50 µs | 17.87% | 1.012 / 1.010 / 0.981 | scalar per-agent math; parallelizable |
| Serial | B | 10000 | 2 | P4 intent pipeline | 261.17 µs | 14.84% | 1.045 / 1.080 / 1.003 | candidate build, validation, chunk work/merge |
| Serial | B | 10000 | 3 | P6B targeted | 198.03 µs | 11.25% | 1.088 / 1.225 / 0.957 | key sort, live-state reads, sequential commits, result copies |
| Serial | B | 10000 | 4 | P10 metrics | 195.00 µs | 11.08% | 1.049 / 1.177 / 0.980 | HashSet validation, index/Gini ordering |
| Serial | B | 10000 | 5 | P7 market | 170.50 µs | 9.69% | 1.059 / 1.280 / 0.974 | sequential market planning/checked settlement updates |
| Rayon | B | 10000 | 1 | P6B targeted | 202.00 µs | 13.50% | 1.055 / 1.137 / 0.951 | key sort, live-state reads, sequential commits, result copies |
| Rayon | B | 10000 | 2 | P10 metrics | 195.03 µs | 13.03% | 1.037 / 1.129 / 1.088 | HashSet validation, index/Gini ordering |
| Rayon | B | 10000 | 3 | P4 intent pipeline | 175.37 µs | 11.72% | 0.967 / 0.994 / 1.161 | candidate build, validation, chunk work/merge |
| Rayon | B | 10000 | 4 | P7 market | 170.13 µs | 11.37% | 1.072 / 1.220 / 1.021 | sequential market planning/checked settlement updates |
| Rayon | B | 10000 | 5 | P4 selection | 125.17 µs | 8.37% | 1.100 / 0.948 / 0.970 | scalar per-agent math; parallelizable |
| Serial | B | 50000 | 1 | P4 selection | 1599.27 µs | 14.77% | 1.012 / 1.010 / 0.981 | scalar per-agent math; parallelizable |
| Serial | B | 50000 | 2 | P4 intent pipeline | 1448.57 µs | 13.38% | 1.045 / 1.080 / 1.003 | candidate build, validation, chunk work/merge |
| Serial | B | 50000 | 3 | P6B targeted | 1292.93 µs | 11.94% | 1.088 / 1.225 / 0.957 | key sort, live-state reads, sequential commits, result copies |
| Serial | B | 50000 | 4 | P10 metrics | 1186.03 µs | 10.96% | 1.049 / 1.177 / 0.980 | HashSet validation, index/Gini ordering |
| Serial | B | 50000 | 5 | P7 market | 1148.20 µs | 10.61% | 1.059 / 1.280 / 0.974 | sequential market planning/checked settlement updates |
| Rayon | B | 50000 | 1 | P6B targeted | 1189.43 µs | 13.64% | 1.055 / 1.137 / 0.951 | key sort, live-state reads, sequential commits, result copies |
| Rayon | B | 50000 | 2 | P10 metrics | 1125.43 µs | 12.91% | 1.037 / 1.129 / 1.088 | HashSet validation, index/Gini ordering |
| Rayon | B | 50000 | 3 | P7 market | 1094.27 µs | 12.55% | 1.072 / 1.220 / 1.021 | sequential market planning/checked settlement updates |
| Rayon | B | 50000 | 4 | P4 intent pipeline | 852.33 µs | 9.77% | 0.967 / 0.994 / 1.161 | candidate build, validation, chunk work/merge |
| Rayon | B | 50000 | 5 | P11 event flush | 737.87 µs | 8.46% | 1.071 / 1.172 / 0.967 | canonical event validation/sort |
| Serial | B | 100000 | 1 | P4 selection | 3156.60 µs | 15.17% | 1.012 / 1.010 / 0.981 | scalar per-agent math; parallelizable |
| Serial | B | 100000 | 2 | P4 intent pipeline | 2902.87 µs | 13.95% | 1.045 / 1.080 / 1.003 | candidate build, validation, chunk work/merge |
| Serial | B | 100000 | 3 | P6B targeted | 2509.17 µs | 12.06% | 1.088 / 1.225 / 0.957 | key sort, live-state reads, sequential commits, result copies |
| Serial | B | 100000 | 4 | P10 metrics | 2339.37 µs | 11.24% | 1.049 / 1.177 / 0.980 | HashSet validation, index/Gini ordering |
| Serial | B | 100000 | 5 | P7 market | 2255.40 µs | 10.84% | 1.059 / 1.280 / 0.974 | sequential market planning/checked settlement updates |
| Rayon | B | 100000 | 1 | P10 metrics | 2393.00 µs | 13.69% | 1.037 / 1.129 / 1.088 | HashSet validation, index/Gini ordering |
| Rayon | B | 100000 | 2 | P6B targeted | 2298.77 µs | 13.15% | 1.055 / 1.137 / 0.951 | key sort, live-state reads, sequential commits, result copies |
| Rayon | B | 100000 | 3 | P7 market | 2220.43 µs | 12.70% | 1.072 / 1.220 / 1.021 | sequential market planning/checked settlement updates |
| Rayon | B | 100000 | 4 | P4 intent pipeline | 1906.57 µs | 10.91% | 0.967 / 0.994 / 1.161 | candidate build, validation, chunk work/merge |
| Rayon | B | 100000 | 5 | P11 event flush | 1442.30 µs | 8.25% | 1.071 / 1.172 / 0.967 | canonical event validation/sort |

## 9. P4 final assessment

P4 is the largest combined Serial phase family at every requested profile size. Its share falls from about 33–37% at 10k to about 29–30% at 100k as other phases and event work grow. With the explicit Rayon reference, P4 Selection + Intent falls to about 18–22% of the profiled tick; P6B and P10 then tend to rank above it. At 100k, explicit Rayon reduces full-tick medians by 19.9% in A and 18.8% in B.

The following separate P4 Intent probe splits the current Rayon call path. It starts from the Phase2 storage state and precomputed choices, retains the candidate index between samples, and is not additive with the full phase profile.

| Workload | N | Index build | Serial validation / slot prep | Parallel construction + chunk merge | Coverage validation + final sort | Whole Intent pipeline |
|---|---:|---:|---:|---:|---:|---:|
| A | 10k | 78.90 ± 0.30 | 16.70 ± 0.20 | 61.90 ± 3.50 | 10.30 ± 0.00 | 168.50 ± 4.60 |
| A | 20k | 159.10 ± 0.30 | 31.90 ± 0.50 | 109.60 ± 7.30 | 18.90 ± 0.80 | 324.90 ± 10.70 |
| A | 50k | 649.40 ± 1.90 | 141.30 ± 1.00 | 241.50 ± 3.50 | 65.50 ± 1.30 | 1,086.30 ± 10.30 |
| A | 100k | 802.20 ± 7.70 | 145.90 ± 10.90 | 573.80 ± 50.50 | 101.80 ± 9.90 | 1,630.60 ± 45.60 |
| B | 10k | 130.40 ± 0.80 | 28.60 ± 1.30 | 60.80 ± 2.40 | 14.10 ± 0.90 | 226.80 ± 8.20 |
| B | 20k | 264.70 ± 4.40 | 57.40 ± 1.50 | 98.70 ± 1.10 | 27.00 ± 0.30 | 435.40 ± 17.50 |
| B | 50k | 414.20 ± 0.90 | 75.00 ± 2.20 | 227.50 ± 11.60 | 43.20 ± 0.30 | 763.10 ± 13.60 |
| B | 100k | 1,356.80 ± 12.80 | 247.70 ± 19.50 | 467.10 ± 19.60 | 143.30 ± 9.60 | 2,177.80 ± 117.80 |

The parallel construction column includes per-intent target selection, `pool.install`, chunk-local output collection, fixed-index ordering, and merge; it is not scheduler-only. Candidate-index construction remains serial. At 100k it measured 0.80 ms for A and 1.36 ms for B in this separate probe. No Auto threshold or chunk tuner is promoted from this host-only evidence.

**Assessment:** P4 remains the largest Serial target and a substantial Explicit Rayon target, but its opt-in backend already supplies repeatable whole-tick gains with exact replay. Further P4 scheduler, index, or chunk-allocation work is useful follow-up, not a reason to keep M2 open.

## 10. Rayon expansion: P2 / P3 / P9

| Workload / N | P2 | P3 | P9 | Combined candidate share |
|---|---:|---:|---:|---:|
| A / 10k | 0.51% | 1.40% | 4.73% | 6.64% |
| B / 10k | 0.46% | 2.07% | 4.32% | 6.85% |
| A / 100k | 0.40% | 1.41% | 4.87% | 6.68% |
| B / 100k | 0.39% | 2.85% | 4.07% | 7.31% |

Neither per-phase parallel speedup nor scheduler overhead was measured here. The scenarios below use 3× (six workers at 50% efficiency) and 5.1× (85% efficiency); they are ideal bounds, before scheduling or merge cost.

| Phase | Measured Serial share | Scenario phase speedup | Ideal whole-tick gain | Scheduling / deterministic merge |
|---|---:|---:|---:|---|
| P2 | 0.39–0.51% | 3× / 5.1× | 0.26–0.34% / 0.31–0.41% | No output reduction; fixed-slot writes are straightforward, scheduler cost unmeasured |
| P3 | 1.40–2.85% | 3× / 5.1× | 0.94–1.92% / 1.14–2.30% | Features must merge by AgentId; scarcity data is read-only, but staging/merge is required |
| P9 | 4.07–4.87% | 3× / 5.1× | 2.82–3.39% / 3.39–4.12% | Mortality result IDs/events need fixed AgentId merge and exact error precedence |

Together, the measured 6.64–7.31% share gives 1.046–1.051× and 1.055–1.063× whole-tick ceilings. P2's per-agent updates are independent; P3 needs AgentId-ordered output; P9 needs ordered mortality results and error behavior. Fixed-index staging/merge and more differential tests add complexity. The measured share does not support **RAYON-EXPAND** now.

## 11. Phase6B final assessment

Phase6B is about 11–15% of Serial profile time and 13–17% of Explicit Rayon time. The isolated resolver probe below is not additive with the production profile; its timers are inside the per-settlement loop.

| Workload / N | Validate, slot lookup, key | Group staging | Keyed sort | Live-state reads / sequential commit | Owned results / combined clone |
|---|---:|---:|---:|---:|---:|
| A / 10k | 70.80 µs (45.1%) | 19.40 (12.4%) | 39.60 (25.2%) | 16.10 (10.3%) | 8.70 (5.5%) |
| B / 10k | 67.70 (43.2%) | 25.60 (16.3%) | 24.30 (15.5%) | 16.60 (10.6%) | 15.00 (9.6%) |
| A / 100k | 1,737.90 (48.1%) | 308.90 (8.5%) | 704.90 (19.5%) | 308.30 (8.5%) | 529.80 (14.7%) |
| B / 100k | 1,748.90 (57.7%) | 367.70 (12.1%) | 326.60 (10.8%) | 282.40 (9.3%) | 198.30 (6.5%) |

Separate microprobes measured ResolutionKey generation at 6.7–6.8 µs for 2,446 keyed interactions at 10k and 67.1–67.2 µs for 24,405 at 100k. Canonical keyed sorting measured 36.4–36.8 µs and 596.7–597.1 µs respectively. At 100k the resolver sees 46,462 targeted intents: 24,405 keyed, 22,057 zero-target, 12,317 Applied outcomes; the combined stream copies 46,462 records.

Compatible opportunities are scratch capacity and allocation management. ResolutionKey order, live-state reads before each commit, and sequential commit order are frozen. The `zero_target`, `keyed_stream`, and `resolutions` shape is frozen, so the combined clone cannot be removed here. Linear settlement-membership checks remain per partition; measured through K=500 they are not a whole-tick hotspot, but revisit if K≈1,000 becomes a normal workload. This does not justify **P6B-MORE** for M2.

## 12. Phase10 final assessment

The Hybrid production path uses `storage.phase10_metrics`: fresh agent-index and settlement-index vectors plus an allocated duplicate-ID HashSet, population/index/food reduction, treasury summation, and wealth ordering/Gini. The existing storage scratch API reuses the two index vectors; it does not remove the HashSet or Gini work.

At 100k, internal Phase10 medians were:

| Workload / scratch | Validation + HashSet | Population/index/food | Treasury order/sum | Wealth order/Gini |
|---|---:|---:|---:|---:|
| A fresh | 844.50 µs | 125.70 | <0.10 | 160.70 |
| A reused | 846.90 | 125.30 | <0.10 | 161.40 |
| B fresh | 893.90 | 126.20 | 0.30 | 161.50 |
| B reused | 893.70 | 126.70 | 0.20 | 161.30 |

| Workload | N | Fresh µs/tick | Reused µs/tick | Saved | Whole-tick gain |
|---|---:|---:|---:|---:|---:|
| A | 10k | 104.20 | 104.30 | -0.10 | -0.005% |
| A | 20k | 219.20 | 212.30 | 6.90 | 0.179% |
| A | 50k | 562.80 | 557.10 | 5.70 | 0.058% |
| A | 100k | 1,125.60 | 1,124.10 | 1.50 | 0.007% |
| B | 10k | 104.90 | 105.20 | -0.30 | -0.016% |
| B | 20k | 215.50 | 214.70 | 0.80 | 0.021% |
| B | 50k | 582.80 | 570.10 | 12.70 | 0.126% |
| B | 100k | 1,173.90 | 1,176.00 | -2.10 | -0.010% |

This scratch reuse removes two vector allocations per tick, but measured whole-tick impact stays below 0.2%. It is low-risk hygiene, not a reason for **P10-CLEANUP**.

## 13. Event staging and Phase11

With metrics and events enabled, emitted record counts scale from about 6k/day at 10k agents to about 60k/day at 100k. Snapshots are disabled, so the Phase11 timing is validation, canonical ordering, and event flush, not snapshot serialization.

| Workload | N | Events / day | Event staging Serial µs (share) | P11 Serial µs (share) | Event staging Rayon µs (share) | P11 Rayon µs (share) |
|---|---:|---:|---:|---:|---:|---:|
| A | 10k | 6,035 | 79.83 (4.94%) | 80.67 (5.00%) | 80.37 (6.05%) | 80.77 (6.08%) |
| A | 50k | 29,925 | 607.27 (6.84%) | 464.73 (5.24%) | 640.80 (8.82%) | 458.63 (6.31%) |
| A | 100k | 59,921 | 1,277.63 (6.30%) | 1,056.63 (5.21%) | 1,305.27 (8.05%) | 1,054.50 (6.50%) |
| B | 10k | 6,083 | 92.80 (5.27%) | 119.10 (6.77%) | 93.17 (6.23%) | 120.00 (8.02%) |
| B | 50k | 30,173 | 478.57 (4.42%) | 729.90 (6.74%) | 469.53 (5.38%) | 737.87 (8.46%) |
| B | 100k | 60,419 | 908.97 (4.37%) | 1,432.00 (6.88%) | 903.23 (5.17%) | 1,442.30 (8.25%) |

Phase11 sorts canonical `EventKey`s after validating records. The event flush is serialization-independent. At 100k, staging plus flush is 11.5% of Serial A, 11.3% of Serial B, 14.6% of Explicit Rayon A, and 13.4% of Explicit Rayon B. It is visible but not the largest current phase family.

## 14. Memory and allocation debt

A temporary counting allocator was enabled only around a 3-day production runner call. It counted allocation and reallocation requests and requested bytes; it did not measure timings, peak live memory, or resident memory. Pools and initial worlds were prepared before counting. Values are per tick, averaged across the 3-day call.

| Workload / mode at N=100k | Events off: alloc calls / requested MB | Events on: records/day; alloc calls / requested MB | Event-enabled delta: calls/day / requested MB/day |
|---|---:|---:|---:|
| A Serial | 89 / 33.31 | 59,921; 101 / 60.55 | +12 / +27.25 |
| A Explicit Rayon | 874 / 36.94 | 59,921; 886 / 64.18 | +12 / +27.25 |
| B Serial | 7,409 / 31.27 | 60,419; 9,411 / 58.52 | +2,002 / +27.25 |
| B Explicit Rayon | 8,194 / 34.90 | 60,419; 10,196 / 62.15 | +2,002 / +27.25 |

The requested-byte column sums allocation sizes and reallocation request sizes; it is not peak memory. Event-enabled deltas include event conversion vectors, the pending buffer, returned events, and related per-group work, not just `EventRecord` payload bytes. The repeated roughly 27.25 MB/day request delta at 100k is consistent across both workloads and modes. The higher B allocation-call count follows its 500 group outputs; Rayon adds about 785 allocation calls/tick at N=100k for chunk-local P4 output. These are allocation counts, not timing attribution.

| Remaining allocation site | Lifetime / scaling | Reusable? | Runtime evidence |
|---|---|---|---|
| P5 owned group partitions and per-group intent vectors | Each tick; O(N+K) output | Lookup/group scratch is reused; returned owned buckets are rebuilt | P5 is about 4–6% of Serial tick and 5–6% of Explicit Rayon tick |
| P6B per-settlement outputs and combined result copies | Each tick; proportional to targeted and zero-target interactions | Validation/keyed scratch is reused; owned public result vectors are not | P6B is about 11–15% Serial and 13–17% Explicit Rayon |
| P10 index vectors and duplicate HashSet | Fresh vectors and HashSet per tick; O(N) and O(K) | Two vectors can be reused; duplicate HashSet remains fresh | Two-vector reuse measured below 0.2% whole-tick gain |
| Rayon chunk-local intent outputs | Each explicit Rayon tick; O(ceil(N/chunk)) vectors plus output records | Pool is reused; chunk result vectors are per invocation | About 785 additional alloc calls/tick at N=100k in the allocator probe |
| Event staging / buffers | Each event-enabled tick; proportional to E records and K groups | EventBuffer and conversion vectors are rebuilt | About 60k records/day at N=100k; staging and flush total about 11–15% of tick |

## 15. Alignment and GPU status

**64-byte alignment: TRACKED DEBT.** SoA fields use ordinary `Vec<T>` and do not promise 64-byte alignment. This gate collected no cache-miss, bandwidth, or alignment-penalty counters. Alignment is not an M2 graduation blocker without such evidence.

**GPU: LATER.** The CPU finishes a 100k-agent tick in about 17–22 ms and a 200k tick in about 38–48 ms. The remaining phases include branch-heavy intent/resolution logic, output staging, and sequential Phase6B commits; no transfer/residency crossover was measured. GPU work remains a separate research effort, not an M2 blocker.

## 16. Cumulative performance progression

All values are µs/tick. Historical entries come from distinct benchmark runs and are indicative only. M2-38 shows the best independently selected Rayon configuration per cell; M2-40 uses the fixed 6-thread / chunk-256 reference. Do not multiply these cross-run ratios into a cumulative speedup.

| Workload / N | M2-34 Serial | M2-35 P5 fast | M2-36 P6B fast | M2-37 Serial | M2-38 best Rayon | M2-40 Serial | M2-40 explicit 6/256 |
|---|---:|---:|---:|---:|---:|---:|---:|
| A / 10k | 2,108.20 | 1,624.70 | 1,628.67 | 1,612.40 | 1,330.23 | 1,984.37 | 1,769.00 |
| A / 20k | 4,264.37 | 3,263.13 | 3,390.03 | 3,279.83 | 2,737.10 | 3,860.40 | 3,364.50 |
| A / 50k | 10,912.53 | 9,337.63 | 9,419.00 | 9,155.03 | 7,319.00 | 9,783.93 | 8,122.00 |
| B / 10k | 2,153.67 | 1,749.60 | 1,786.47 | 1,774.57 | 1,481.23 | 1,846.40 | 1,666.33 |
| B / 20k | 4,438.63 | 3,596.07 | 3,586.33 | 3,573.40 | 3,067.40 | 3,850.83 | 3,385.83 |
| B / 50k | 11,382.23 | 9,727.13 | 9,459.20 | 9,638.33 | 7,935.67 | 10,081.03 | 8,662.20 |
| A / 100k | — | — | — | 20,245.07 | — | 20,371.40 | 16,997.30 |
| B / 100k | — | — | — | 21,202.10 | — | 21,632.20 | 18,210.73 |

M2-36's paired P6B change and M2-40's paired Serial/Explicit Rayon result are the useful local comparisons. Other cross-milestone entries use different sample blocks and are not an apples-to-apples speedup chain.

## 17. M2 objective audit

| M2 objective | Status | Evidence / scope |
|---|---|---|
| Authoritative Segmented SoA | **DONE** | Hybrid production runner uses SegmentedAgentStorage authority; write-back remains at snapshot boundary |
| Remove hot-path AoS adapters | **DONE** | Current Hybrid hot phases operate on storage; no per-phase AoS rematerialization in the production path |
| Deterministic parity | **DONE** | Workspace tests, Phase4 policy differential tests, oracle hashes, and snapshot tests pass |
| Reduce repeated allocation | **PARTIAL** | Scratch persists for several phases; per-day P5/P6B outputs, P10 HashSet/vectors, Rayon chunks, and event buffers remain |
| Remove structural scaling debt | **DONE for targeted scans; FOLLOW-UP for high K** | M2-30/32/33 removed repeated candidate, welfare, and scarcity scans; settlement membership checks remain linear per group lookup |
| Optional parallel backend | **DONE** | Explicit caller-owned Rayon Phase4 path is available; Serial remains default and Auto remains experimental |
| Preserve production semantics | **DONE** | No semantic changes in this report-only gate; frozen order, RNG, result shapes, and event behavior remain |
| Preserve canonical replay | **DONE** | State/Metrics/Events hashes and Day 100/250/500 snapshots match exactly |

## 18. Remaining debt classification

### M2 blocker

**None found.** No unresolved correctness issue or material 50k→100k whole-tick scaling regression blocks graduation.

### Follow-up

- Profile settlement membership scans at larger K. Phase6A, Phase6B validation, and Phase7 still use per-partition linear settlement lookup; this is K-sensitive and may contribute when B reaches K=1,000 at N=200k. The measured 100k range is near-linear; the 200k extension is mildly superlinear but still practical.
- Consider P4 index-build and chunk-output refinements only if a deployment workload justifies them. Pool/chunk tuning is hardware- and workload-specific; Auto remains experimental.
- Consider P6B transient allocation cleanup while retaining ResolutionKey order, live reads, sequential commit, and public result shape.
- Keep P10 scratch reuse in a low-priority hygiene queue; measured whole-tick gain is below 0.2%.
- Track event staging allocations as record counts and group count grow.

### Research

- 64-byte alignment and SIMD need hardware counters or exactness experiments.
- GPU remains LATER until transfer/residency and kernel crossover evidence exists.
- Advanced threading beyond the explicit Phase4 path remains a separate profiling/research task.

## 19. Optimization ROI

The whole-tick figures below are bounds or scenarios derived from measured phase shares; they are not promised outcomes.

| Candidate | Current share | Achievable share reduction / whole-tick model | Complexity | Determinism risk | Contract risk | Added code complexity |
|---|---:|---|---|---|---|---|
| P4 index / scheduler / merge refinement | Serial P4 ~29–37%; explicit P4 ~18–22% | A hypothetical 2× of the remaining explicit P4 phase caps near 1.10×; scheduler-only savings are smaller and not isolated | Medium–High | Medium: fixed ordering and error precedence | Medium: Phase4 semantics frozen | Medium–High |
| P6B compatible scratch cleanup | ~11–15% Serial; ~13–17% Explicit Rayon | 25% reduction of the whole phase would give about 1.03–1.05×; removing the full phase is impossible under sequential-commit/result contracts | Medium–High | Medium | High for ordering, live-state, and result shape | Medium–High |
| P10 index scratch | ~11–12% phase share | Measured vector reuse is <0.2% whole tick | Low | Low | Low | Low |
| P2/P3/P9 Rayon expansion | ~6.6–7.3% combined | Ideal 3× phase speedup gives 1.046–1.051×; 5.1× gives 1.055–1.063× before overhead | Medium–High | Medium: deterministic output merge | Low–Medium | Medium–High |
| Event staging / P11 | ~11–15% combined at 100k | A hypothetical 20% phase reduction gives roughly 1.02–1.04×; record and allocation counts rise with N/K | Medium | Medium: EventKey order is frozen | Medium | Medium |
| P5 owned allocation reduction | ~4–6% Serial; ~5–6% Explicit Rayon | A hypothetical 2× P5 gives about 1.02–1.03× | Medium | Low with canonical bucketing | Low if owned partition results remain | Medium |

The measured P10 and P2/P3/P9 ceilings are too small to displace graduation. P6B/P4/event work remain valid follow-ups if a specific deployment profile raises their value.

## 20. Graduation criteria

| Criterion | Result | Evidence |
|---|---|---|
| Canonical State/Metrics/Events exact | **PASS** | Frozen digests below; existing oracle and policy tests |
| Day 100/250/500 snapshot bytes exact | **PASS** | Phase4 Rayon/snapshot differential tests and SoA gate tests |
| Serial production stable | **PASS** | `run_hybrid_authority_days` remains the unchanged Serial entry point; paired samples cover up to 200k |
| Explicit Rayon exact | **PASS** | Caller-owned pool policy differential, trajectory, error, and snapshot tests |
| No material superlinear hot spot through normal range | **PASS with tracked phase/K follow-up** | Full-tick alpha is 0.960–1.102 from 1k→100k; A has repeatable local P5/P6A/P6B alpha outliers, but their shares stay bounded and whole-tick alpha is 1.058 |
| N=100k practical runtime measured | **PASS** | 16.997–21.632 ms/tick across explicit/serial and workloads |
| Remaining work primarily constant-factor, allocation, or hardware-specific | **PASS with K note** | P10 reuse <0.2%; remaining P6B/P5/event work is allocation/order constrained; 200k extension is tracked separately |
| No unresolved optimization correctness debt | **PASS** | Canonical replay and snapshot gates exact; no fixture or expected-value updates |

## 21. Final decision

**M2-GRADUATE.** The targeted structural scans are removed, serial and explicit Rayon paths preserve the frozen contracts, and the measured runtime through 100k is near-linear and practical. The remaining opportunities are mostly bounded allocation cleanup, host-specific P4 tuning, and higher-K profiling. Keep them in follow-up work rather than extending M2. Preserve Serial as the production default; explicit Rayon remains an opt-in candidate and Auto remains experimental.

## 22. Correctness values

Frozen values remained unchanged and matched exactly:

| Oracle | Expected / observed |
|---|---|
| State | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` |
| Metrics | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` |
| Events | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` |

Day 100, 250, and 500 snapshot bytes matched exactly. No fixture, golden, expected hash, snapshot/event schema, or CI file changed.

## 23. Validation

All required validation was run after temporary profiling instrumentation was removed. The workspace run and each focused command exited successfully; the focused suites reported 14, 66, and 21 tests respectively.

- `cargo test --workspace`
- `cargo test -p sim-model --test phase4_rayon_tests -- --nocapture`
- `cargo test -p sim-model --test determinism_oracle_tests -- --nocapture`
- `cargo test -p sim-model --test m2_soa_gate_tests -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench --bench m0_baseline_bench`
- `git diff --check`

The profiling-only hooks, benchmark extension, counting allocator, and temporary event/profile probes were reverted. The final worktree contains only this new untracked report; no commit was created.
