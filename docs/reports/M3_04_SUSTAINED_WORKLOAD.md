<a id="m3-sustained-workload-benchmark"></a>
# M3-04 (formerly M3-E) Sustained Workload Benchmark
Status: current


This benchmark evaluates audit option (b): a sustained-interaction workload configuration. It changes no simulation source behavior or frozen M3 semantics. The new workload changes only food-supply values in the copied M2-40 configuration.

## Food-supply grid

The search used the M2-40 configuration at `N=10,000`, `K=50`, `k=8`, `p_local=0.5`, seed `81985529216486895`, replicate `7`, for 200 days. Each candidate changed one permitted food-supply knob; all other M2-40 values, including action weights and biases, traits, interaction amounts, theft probability, starvation threshold, and M3 settings, remained fixed.

The acceptance gate required at least 5,000 living agents after every day 1–200; no zero-keyed day in days 100–200; mean keyed entries over days 100–200 at least 10% of mean alive population; and at least one day with a GiveFood `Some` target in days 100–200.

| Candidate | Minimum alive, days 1–200 | Alive day 200 | Mean keyed, days 100–200 | Mean alive, days 100–200 | Zero-keyed days, 100–200 | First zero-keyed day | GiveFood Some days, 100–200 | Accepted |
|---|---:|---:|---:|---:|---:|---:|---:|:---:|
| M2-40 unchanged | 0 | 0 | 0.00 | 0.00 | 101 | 73 | 0 | No |
| `base_metabolic_cost=0.75` | 0 | 0 | 0.95 | 26.82 | 92 | 108 | 9 | No |
| `base_metabolic_cost=0.5` | 1,029 | 1,029 | 3,724.12 | 8,438.94 | 0 | none | 101 | No |
| `base_metabolic_cost=0.49` | 2,890 | 2,890 | 4,091.51 | 9,127.91 | 0 | none | 101 | No |
| `base_metabolic_cost=0.48` | 6,038 | 6,038 | 4,403.65 | 9,648.17 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.47` | 8,672 | 8,672 | 4,587.50 | 9,908.99 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.46` | 9,746 | 9,746 | 4,632.71 | 9,983.74 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.455` | 9,898 | 9,898 | 4,628.77 | 9,993.97 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.451` | 9,954 | 9,954 | 4,618.33 | 9,996.80 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.45` | 9,965 | 9,965 | 4,614.69 | 9,997.81 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.4` | 10,000 | 10,000 | 3,954.89 | 10,000.00 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.35` | 10,000 | 10,000 | 2,631.50 | 10,000.00 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.3` | 10,000 | 10,000 | 2,474.92 | 10,000.00 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.275` | 10,000 | 10,000 | 2,468.03 | 10,000.00 | 0 | none | 101 | Yes |
| `base_metabolic_cost=0.25` | 10,000 | 10,000 | 2,463.59 | 10,000.00 | 0 | none | 101 | Yes |
| `health_decay_rate=0.025` | 0 | 0 | 0.00 | 1.35 | 101 | 98 | 0 | No |
| `health_decay_rate=0.01` | 0 | 0 | 1,436.45 | 5,531.86 | 25 | 176 | 76 | No |
| `health_decay_rate=0.0075` | 2,488 | 2,488 | 2,706.81 | 9,355.75 | 0 | none | 101 | No |
| `health_decay_rate=0.005` | 10,000 | 10,000 | 3,694.50 | 10,000.00 | 0 | none | 101 | Yes |
| `base_work_yield=3.0` | 0 | 0 | 0.00 | 1.14 | 101 | 94 | 0 | No |
| `base_work_yield=4.0` | 2 | 2 | 18.61 | 141.48 | 62 | 134 | 33 | No |
| `initial_settlement_resource=4000.0` | 0 | 0 | 0.00 | 0.00 | 101 | 73 | 0 | No |
| `regrowth_rate=0.2` | 0 | 0 | 0.00 | 0.00 | 101 | 73 | 0 | No |
| `carrying_capacity=20000.0` | 0 | 0 | 0.00 | 0.00 | 101 | 73 | 0 | No |

The selected candidate is `base_metabolic_cost=0.48`. Every passing candidate changed one knob; relative percent change from its M2-40 value was used to compare different units. The selected 52% reduction is the smallest tested relative change that passed all gates (`1.0 → 0.48`). The next closer candidate, `0.49`, failed the alive-population floor. The only other one-knob passing candidate, `health_decay_rate=0.005`, changes that knob by 90%.

| Food-supply field | M2-40 | Sustained workload |
|---|---:|---:|
| `world.initial_settlement_resource` | 2,000.0 | 2,000.0 |
| `environment.carrying_capacity` | 10,000.0 | 10,000.0 |
| `environment.regrowth_rate` | 0.1 | 0.1 |
| `environment.base_metabolic_cost` | 1.0 | 0.48 |
| `environment.health_decay_rate` | 0.05 | 0.05 |
| `economy.base_work_yield` | 2.0 | 2.0 |

## N=100,000 stability check

The selected configuration was run for 200 days at `N=100,000`, `K=500`, `k=8`, seed `81985529216486895`, replicate `7`, with each `p_local` value. The same acceptance checks were applied. The minima occur at day 200; the run remains above the 50% alive floor, although population is not constant.

| `p_local` | Minimum alive, days 1–200 | Alive day 200 | Mean keyed, days 100–200 | Mean alive, days 100–200 | Keyed zero days, 100–200 | GiveFood Some days, 100–200 | Accepted |
|---:|---:|---:|---:|---:|---:|:---:|:---:|
| 0 | 58,490 | 58,490 | 43,852.35 | 96,343.66 | 0 | 101 | Yes |
| 0.5 | 58,425 | 58,425 | 43,859.93 | 96,352.81 | 0 | 101 | Yes |
| 1 | 57,967 | 57,967 | 43,678.33 | 96,030.22 | 0 | 101 | Yes |

## Measurement protocol

- **Host:** AMD Ryzen 5 9600X, 6 cores / 12 logical processors; Windows 11 Pro; `rustc 1.98.1`.
- **Configuration:** the M2-40 benchmark TOML with only `base_metabolic_cost=0.48` changed, plus M3 profile fields. `K=N/200`; `k=8`; seed `81985529216486895`; replicate `7`.
- **Cells:** `N ∈ {10,000, 100,000}`, `p_local ∈ {0, 0.5, 1}`, horizons day 0 and day 150.
- **Whole tick:** uninstrumented `run_m3_storage_days` and explicit parallel Phase 6B runner, pools `{1,4,6}`, chunk 256. Pools, initial states, graph/projection and run contexts are prepared outside timers. Each paired sample clones the same checkpoint and advances three consecutive days with scratch reuse; two warmups and 11 timed samples are interleaved. Metrics/events are on; snapshots are off.
- **Phase profile:** the opt-in Phase 4/Phase 6B timing APIs are called through the same serial storage phase driver as M3-01. The manual profile is compared each day with the production serial runner for exact State hash, daily Metrics record, and Events vector. Whole-tick figures use the uninstrumented production runner separately.
- **Timing-hook overhead:** the hooks added measurable wall time. Search/setup and per-entry divisions are therefore approximate; hook overhead is shown with the search/work results. No timing or counter code was added to `src/`.
- **Benchmark command:** `cargo bench -p sim-model --bench m3_sustained_workload_bench`.
- **Wall time:** the full run that produced the tables took 412.593 seconds (paired measurement section 341.813 seconds). After aligning the zero-key check to days 100–200, a full rerun took 399.289 seconds (measurement section 336.647 seconds); grid/stability outcomes and all parity assertions were unchanged.

## Measurements

## Workload and interaction mix

Each row summarizes the three-day serial profile starting at its checkpoint. Conflict and level statistics were measured with the parallel resolver from the same checkpoint; all serial/parallel digest comparisons passed.

| Checkpoint | N | p_local | Alive at checkpoint | Mean food after first sample day | Keyed/day | Cross-partition/day | Conflict fraction | Mean contention | Levels/day | Max level size |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.0 | 10,000 | 24.8417 | 2,473.7 | 2,473.7 | 0.532972 | 1.195185 | 4.33 | 1,747 |
| day 150 | 10,000 | 0.0 | 10,000 | 2.2172 | 4,632.7 | 4,632.7 | 0.799358 | 1.413750 | 7.33 | 2,375 |
| day 0 | 10,000 | 0.5 | 10,000 | 24.8417 | 2,473.7 | 1,238.0 | 0.528892 | 1.192620 | 5.00 | 1,777 |
| day 150 | 10,000 | 0.5 | 10,000 | 2.2277 | 4,632.3 | 2,318.0 | 0.798930 | 1.414225 | 7.67 | 2,365 |
| day 0 | 10,000 | 1.0 | 10,000 | 24.8417 | 2,473.7 | 0.0 | 0.537552 | 1.196500 | 5.00 | 1,785 |
| day 150 | 10,000 | 1.0 | 10,000 | 2.2182 | 4,631.3 | 0.0 | 0.802440 | 1.414487 | 7.00 | 2,398 |
| day 0 | 100,000 | 0.0 | 100,000 | 24.8299 | 24,584.3 | 24,584.3 | 0.540591 | 1.199967 | 6.33 | 17,265 |
| day 150 | 100,000 | 0.0 | 100,000 | 2.1562 | 46,632.7 | 46,632.7 | 0.799221 | 1.414745 | 8.33 | 23,861 |
| day 0 | 100,000 | 0.5 | 100,000 | 24.8299 | 24,584.3 | 12,288.0 | 0.538757 | 1.199484 | 6.00 | 17,267 |
| day 150 | 100,000 | 0.5 | 99,998 | 2.1566 | 46,634.7 | 23,334.0 | 0.798802 | 1.414863 | 8.33 | 23,845 |
| day 0 | 100,000 | 1.0 | 100,000 | 24.8299 | 24,584.3 | 0.0 | 0.541160 | 1.199553 | 6.00 | 17,354 |
| day 150 | 100,000 | 1.0 | 99,997 | 2.1701 | 46,627.0 | 0.0 | 0.800992 | 1.415784 | 8.33 | 23,814 |

Counts below are daily means over those three days. Action order is Work / BuyFood / SellFood / GiveFood / StealFood / Idle. Target counts are Some / None.

| Checkpoint | N | p_local | Actions/day (Work / Buy / Sell / Give / Steal / Idle) | Give Some / None per day | Steal Some / None per day | Keyed/day | Applied-positive/day | Applied-zero/day | Zero-target/day | Target-ineligible/day | TheftFailed/day |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.0 | 1,363.0 / 1,296.7 / 1,325.7 / 2,195.0 / 2,473.7 / 1,346.0 | 0.0 / 2,195.0 | 2,473.7 / 0.0 | 2,473.7 | 1,226.3 | 0.0 | 2,195.0 | 0.0 | 1,247.3 |
| day 150 | 10,000 | 0.0 | 1,377.3 / 1,331.7 / 1,321.0 / 2,206.0 / 2,428.3 / 1,335.7 | 2,206.0 / 0.0 | 2,426.7 / 1.7 | 4,632.7 | 2,357.7 | 59.0 | 1.7 | 1,136.0 | 1,080.0 |
| day 0 | 10,000 | 0.5 | 1,363.0 / 1,296.7 / 1,325.7 / 2,195.0 / 2,473.7 / 1,346.0 | 0.0 / 2,195.0 | 2,473.7 / 0.0 | 2,473.7 | 1,226.3 | 0.0 | 2,195.0 | 0.0 | 1,247.3 |
| day 150 | 10,000 | 0.5 | 1,377.3 / 1,331.7 / 1,321.0 / 2,206.0 / 2,428.3 / 1,335.7 | 2,206.0 / 0.0 | 2,426.3 / 2.0 | 4,632.3 | 2,350.3 | 58.3 | 2.0 | 1,131.3 | 1,092.3 |
| day 0 | 10,000 | 1.0 | 1,363.0 / 1,296.7 / 1,325.7 / 2,195.0 / 2,473.7 / 1,346.0 | 0.0 / 2,195.0 | 2,473.7 / 0.0 | 2,473.7 | 1,226.3 | 0.0 | 2,195.0 | 0.0 | 1,247.3 |
| day 150 | 10,000 | 1.0 | 1,377.3 / 1,331.7 / 1,321.0 / 2,206.0 / 2,428.3 / 1,335.7 | 2,206.0 / 0.0 | 2,425.3 / 3.0 | 4,631.3 | 2,371.7 | 59.0 | 3.0 | 1,119.7 | 1,081.0 |
| day 0 | 100,000 | 0.0 | 13,323.7 / 13,233.7 / 13,290.7 / 22,010.3 / 24,584.3 / 13,557.3 | 0.0 / 22,010.3 | 24,584.3 / 0.0 | 24,584.3 | 12,283.7 | 0.0 | 22,010.3 | 0.0 | 12,300.7 |
| day 150 | 100,000 | 0.0 | 13,424.3 / 13,340.7 / 13,305.3 / 22,167.0 / 24,483.3 / 13,276.7 | 22,167.0 / 0.0 | 24,465.7 / 17.7 | 46,632.7 | 23,340.7 | 579.7 | 17.7 | 11,580.3 | 11,132.0 |
| day 0 | 100,000 | 0.5 | 13,323.7 / 13,233.7 / 13,290.7 / 22,010.3 / 24,584.3 / 13,557.3 | 0.0 / 22,010.3 | 24,584.3 / 0.0 | 24,584.3 | 12,283.7 | 0.0 | 22,010.3 | 0.0 | 12,300.7 |
| day 150 | 100,000 | 0.5 | 13,424.3 / 13,340.0 / 13,305.0 / 22,167.0 / 24,482.7 / 13,276.3 | 22,167.0 / 0.0 | 24,467.7 / 15.0 | 46,634.7 | 23,253.3 | 605.0 | 15.0 | 11,655.3 | 11,121.0 |
| day 0 | 100,000 | 1.0 | 13,323.7 / 13,233.7 / 13,290.7 / 22,010.3 / 24,584.3 / 13,557.3 | 0.0 / 22,010.3 | 24,584.3 / 0.0 | 24,584.3 | 12,283.7 | 0.0 | 22,010.3 | 0.0 | 12,300.7 |
| day 150 | 100,000 | 1.0 | 13,424.0 / 13,339.7 / 13,305.0 / 22,166.7 / 24,483.3 / 13,276.3 | 22,166.7 / 0.0 | 24,460.3 / 23.0 | 46,627.0 | 23,291.7 | 580.3 | 23.0 | 11,661.7 | 11,093.3 |

## Search/setup versus Applied-positive work

Search/setup is Phase 4 neighbor filtering/target draw plus Phase 6B validation/slot lookup, key generation, and sort. Applied work is the measured interval for keyed entries ending Applied with amount > 0. Values are median ± MAD over 11 samples; hook overhead makes per-entry divisions approximate.

| Checkpoint | N | p_local | Search/setup µs/tick | Applied-positive work µs/tick | Ratio | Search µs/Applied | Work µs/Applied | P4 hook overhead | P6B hook overhead |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.0 | 416.367 ± 14.933 | 44.733 ± 1.833 | 9.115 ± 0.207 | 0.340 ± 0.012 | 0.036 ± 0.001 | 108.13 ± 1.19% | 32.34 ± 4.05% |
| day 150 | 10,000 | 0.0 | 547.300 ± 51.567 | 76.933 ± 2.100 | 7.404 ± 0.381 | 0.232 ± 0.022 | 0.033 ± 0.001 | 77.06 ± 1.51% | 47.51 ± 3.76% |
| day 0 | 10,000 | 0.5 | 390.833 ± 18.933 | 43.633 ± 2.900 | 8.735 ± 0.203 | 0.319 ± 0.015 | 0.036 ± 0.002 | 108.32 ± 4.34% | 35.51 ± 9.26% |
| day 150 | 10,000 | 0.5 | 555.567 ± 43.767 | 74.300 ± 2.733 | 7.510 ± 0.266 | 0.236 ± 0.019 | 0.032 ± 0.001 | 77.13 ± 1.45% | 31.82 ± 4.03% |
| day 0 | 10,000 | 1.0 | 358.100 ± 30.367 | 41.733 ± 1.800 | 8.655 ± 0.355 | 0.292 ± 0.025 | 0.034 ± 0.001 | 109.09 ± 1.35% | 38.15 ± 4.31% |
| day 150 | 10,000 | 1.0 | 573.033 ± 22.133 | 78.367 ± 1.467 | 7.432 ± 0.086 | 0.242 ± 0.009 | 0.033 ± 0.001 | 76.71 ± 0.70% | 52.90 ± 4.44% |
| day 0 | 100,000 | 0.0 | 5,978.067 ± 244.433 | 603.433 ± 20.733 | 9.939 ± 0.630 | 0.487 ± 0.020 | 0.049 ± 0.002 | 76.73 ± 7.31% | 46.12 ± 8.14% |
| day 150 | 100,000 | 0.0 | 8,081.533 ± 148.867 | 1,004.700 ± 22.533 | 8.124 ± 0.170 | 0.346 ± 0.006 | 0.043 ± 0.001 | 59.31 ± 5.38% | 47.78 ± 7.68% |
| day 0 | 100,000 | 0.5 | 6,260.600 ± 151.700 | 597.967 ± 5.933 | 10.582 ± 0.172 | 0.510 ± 0.012 | 0.049 ± 0.000 | 74.08 ± 8.39% | 46.36 ± 3.44% |
| day 150 | 100,000 | 0.5 | 8,092.100 ± 193.567 | 982.867 ± 32.867 | 8.230 ± 0.222 | 0.348 ± 0.008 | 0.042 ± 0.001 | 61.71 ± 5.07% | 48.99 ± 4.02% |
| day 0 | 100,000 | 1.0 | 5,271.267 ± 58.333 | 542.600 ± 8.767 | 9.756 ± 0.129 | 0.429 ± 0.005 | 0.044 ± 0.001 | 86.98 ± 11.34% | 55.43 ± 2.85% |
| day 150 | 100,000 | 1.0 | 7,274.600 ± 130.633 | 930.767 ± 14.300 | 7.927 ± 0.133 | 0.312 ± 0.006 | 0.040 ± 0.001 | 60.07 ± 4.25% | 58.51 ± 1.98% |

## Serial per-phase profile

Values are µs/tick, median ± MAD over 11 samples, each averaging three days. Phase 4 intent/projection is hook-timed. Phase 6B validation, key, sort, and commit/output are the opt-in serial timing stages. Event staging is separate from Phase 10 metrics and Phase 11 flush.

### N=10,000, checkpoint day 0 (sample days 0–2)

| Phase (µs/tick, median ± MAD) | p_local=0 | p_local=0.5 | p_local=1 |
|---|---:|---:|---:|
| phase1_regrowth | 0.133 ± 0.033 | 0.200 ± 0.033 | 0.100 ± 0.033 |
| phase2_degradation | 10.867 ± 0.300 | 10.100 ± 1.067 | 9.600 ± 1.200 |
| phase3_features | 57.767 ± 1.800 | 52.400 ± 6.000 | 50.433 ± 7.267 |
| phase4_selection | 497.633 ± 3.700 | 468.333 ± 14.033 | 457.667 ± 27.333 |
| phase4_intent_projection_hook_wall | 435.700 ± 9.000 | 394.233 ± 21.133 | 376.567 ± 30.467 |
| phase5_partition | 129.733 ± 5.000 | 118.267 ± 9.733 | 108.100 ± 12.167 |
| phase6a_work | 107.467 ± 1.600 | 102.200 ± 9.033 | 88.433 ± 8.533 |
| phase6b_validation_slot | 111.267 ± 7.700 | 104.000 ± 9.900 | 91.033 ± 11.033 |
| phase6b_key_generation | 46.933 ± 4.533 | 42.967 ± 3.867 | 37.433 ± 4.500 |
| phase6b_sort | 68.367 ± 3.300 | 69.533 ± 3.133 | 63.800 ± 4.033 |
| phase6b_commit_output | 180.500 ± 6.967 | 183.800 ± 8.533 | 164.867 ± 5.933 |
| phase7_market | 238.800 ± 5.633 | 215.467 ± 18.467 | 205.967 ± 26.867 |
| phase8_welfare | 128.167 ± 1.567 | 112.667 ± 6.500 | 106.700 ± 11.333 |
| phase9_mortality | 109.067 ± 0.433 | 89.567 ± 8.567 | 87.733 ± 11.067 |
| event_staging | 114.067 ± 4.233 | 116.067 ± 4.633 | 106.300 ± 1.900 |
| phase10_metrics | 262.033 ± 1.767 | 223.300 ± 19.400 | 220.700 ± 28.000 |
| phase11_event_flush | 145.567 ± 1.600 | 131.867 ± 12.133 | 132.767 ± 12.000 |

### N=10,000, checkpoint day 150 (sample days 150–152)

| Phase (µs/tick, median ± MAD) | p_local=0 | p_local=0.5 | p_local=1 |
|---|---:|---:|---:|
| phase1_regrowth | 0.133 ± 0.033 | 0.133 ± 0.033 | 0.133 ± 0.033 |
| phase2_degradation | 9.833 ± 1.200 | 9.900 ± 0.967 | 10.333 ± 1.067 |
| phase3_features | 53.633 ± 4.700 | 50.667 ± 7.300 | 51.133 ± 6.900 |
| phase4_selection | 474.333 ± 23.200 | 473.267 ± 22.700 | 492.800 ± 4.067 |
| phase4_intent_projection_hook_wall | 503.233 ± 39.833 | 504.900 ± 37.133 | 528.033 ± 17.900 |
| phase5_partition | 112.400 ± 15.900 | 119.433 ± 14.533 | 131.667 ± 1.833 |
| phase6a_work | 99.333 ± 10.200 | 108.400 ± 10.933 | 102.033 ± 7.567 |
| phase6b_validation_slot | 113.867 ± 20.867 | 119.700 ± 14.733 | 121.467 ± 13.167 |
| phase6b_key_generation | 32.367 ± 6.800 | 34.033 ± 5.500 | 34.600 ± 4.800 |
| phase6b_sort | 131.233 ± 2.633 | 126.867 ± 8.567 | 130.733 ± 4.300 |
| phase6b_commit_output | 286.300 ± 9.033 | 284.700 ± 12.633 | 292.200 ± 2.267 |
| phase7_market | 226.033 ± 21.033 | 258.433 ± 23.300 | 223.200 ± 8.900 |
| phase8_welfare | 250.933 ± 27.567 | 257.333 ± 25.600 | 276.767 ± 9.933 |
| phase9_mortality | 98.300 ± 11.167 | 98.667 ± 10.967 | 109.300 ± 0.433 |
| event_staging | 221.100 ± 14.267 | 216.933 ± 5.600 | 224.700 ± 2.500 |
| phase10_metrics | 318.500 ± 34.067 | 319.133 ± 30.867 | 343.500 ± 6.167 |
| phase11_event_flush | 259.100 ± 8.467 | 252.600 ± 14.567 | 266.633 ± 2.633 |

### N=100,000, checkpoint day 0 (sample days 0–2)

| Phase (µs/tick, median ± MAD) | p_local=0 | p_local=0.5 | p_local=1 |
|---|---:|---:|---:|
| phase1_regrowth | 0.533 ± 0.033 | 0.600 ± 0.033 | 0.533 ± 0.033 |
| phase2_degradation | 98.467 ± 6.933 | 102.333 ± 4.667 | 83.200 ± 0.667 |
| phase3_features | 782.000 ± 85.600 | 857.267 ± 44.733 | 599.167 ± 9.533 |
| phase4_selection | 4,638.700 ± 169.333 | 4,900.933 ± 51.333 | 4,260.000 ± 10.633 |
| phase4_intent_projection_hook_wall | 4,664.067 ± 209.933 | 4,848.867 ± 135.300 | 4,045.933 ± 57.500 |
| phase5_partition | 1,462.467 ± 46.600 | 1,593.300 ± 22.767 | 1,194.733 ± 16.867 |
| phase6a_work | 1,327.400 ± 86.633 | 1,439.467 ± 72.267 | 1,148.700 ± 24.167 |
| phase6b_validation_slot | 2,111.400 ± 131.000 | 2,206.800 ± 42.800 | 1,851.400 ± 17.533 |
| phase6b_key_generation | 611.200 ± 65.633 | 681.500 ± 13.733 | 464.000 ± 2.833 |
| phase6b_sort | 1,007.000 ± 34.633 | 1,081.067 ± 55.467 | 932.367 ± 13.233 |
| phase6b_commit_output | 2,556.167 ± 84.100 | 2,306.667 ± 52.100 | 2,098.533 ± 29.233 |
| phase7_market | 3,710.133 ± 212.700 | 3,809.467 ± 98.767 | 3,125.400 ± 21.233 |
| phase8_welfare | 1,326.700 ± 64.933 | 1,419.033 ± 74.667 | 1,044.367 ± 12.867 |
| phase9_mortality | 1,076.300 ± 94.233 | 1,090.767 ± 12.333 | 842.133 ± 4.767 |
| event_staging | 1,003.767 ± 29.967 | 1,016.900 ± 18.700 | 911.600 ± 12.533 |
| phase10_metrics | 2,694.300 ± 72.700 | 2,816.800 ± 125.733 | 2,171.133 ± 16.433 |
| phase11_event_flush | 1,622.700 ± 25.567 | 1,694.400 ± 32.567 | 1,436.667 ± 16.467 |

### N=100,000, checkpoint day 150 (sample days 150–152)

| Phase (µs/tick, median ± MAD) | p_local=0 | p_local=0.5 | p_local=1 |
|---|---:|---:|---:|
| phase1_regrowth | 0.567 ± 0.033 | 0.567 ± 0.033 | 0.567 ± 0.033 |
| phase2_degradation | 98.467 ± 4.933 | 100.400 ± 8.367 | 82.800 ± 0.533 |
| phase3_features | 802.267 ± 69.300 | 844.600 ± 89.700 | 604.967 ± 3.333 |
| phase4_selection | 4,681.467 ± 112.800 | 4,795.300 ± 128.467 | 4,274.133 ± 4.467 |
| phase4_intent_projection_hook_wall | 5,522.867 ± 66.400 | 5,608.600 ± 116.133 | 5,019.567 ± 60.467 |
| phase5_partition | 1,480.067 ± 126.033 | 1,500.300 ± 31.933 | 1,197.833 ± 5.900 |
| phase6a_work | 1,278.400 ± 87.333 | 1,350.167 ± 72.167 | 1,102.367 ± 23.767 |
| phase6b_validation_slot | 2,421.600 ± 51.900 | 2,387.067 ± 51.500 | 2,160.667 ± 59.933 |
| phase6b_key_generation | 512.867 ± 46.433 | 517.900 ± 38.067 | 378.800 ± 1.733 |
| phase6b_sort | 1,954.600 ± 55.667 | 1,962.233 ± 78.367 | 1,764.067 ± 11.767 |
| phase6b_commit_output | 3,795.267 ± 146.867 | 3,699.900 ± 129.200 | 3,413.033 ± 124.867 |
| phase7_market | 3,646.900 ± 164.667 | 3,650.333 ± 56.900 | 3,129.000 ± 36.033 |
| phase8_welfare | 3,058.167 ± 271.133 | 3,124.033 ± 191.067 | 2,481.900 ± 26.067 |
| phase9_mortality | 1,095.367 ± 81.800 | 1,101.333 ± 106.967 | 845.067 ± 1.900 |
| event_staging | 2,114.667 ± 60.767 | 2,109.167 ± 65.867 | 1,903.667 ± 12.000 |
| phase10_metrics | 3,873.300 ± 93.900 | 3,863.933 ± 94.800 | 3,038.100 ± 38.033 |
| phase11_event_flush | 3,190.567 ± 135.167 | 3,176.933 ± 84.233 | 2,828.767 ± 33.467 |

## Paired whole-tick timings

Uninstrumented production-runner results, µs/tick, median ± MAD over 11 samples. Each serial/parallel pair starts from clones of the same checkpoint and runs three days with scratch reuse.

### Pool 1, chunk 256

| Checkpoint | N | p_local | Serial µs/tick | Parallel µs/tick | Paired speedup |
|---|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.00 | 2,221.433 ± 150.667 | 2,063.533 ± 99.600 | 1.07652× |
| day 150 | 10,000 | 0.00 | 2,431.300 ± 32.233 | 2,824.833 ± 236.000 | 0.86069× |
| day 0 | 10,000 | 0.50 | 2,306.933 ± 99.967 | 2,420.733 ± 58.833 | 0.95299× |
| day 150 | 10,000 | 0.50 | 2,731.500 ± 291.267 | 3,031.400 ± 89.900 | 0.90107× |
| day 0 | 10,000 | 1.00 | 1,954.633 ± 123.667 | 2,224.167 ± 228.633 | 0.87882× |
| day 150 | 10,000 | 1.00 | 3,020.533 ± 93.500 | 2,993.333 ± 109.833 | 1.00909× |
| day 0 | 100,000 | 0.00 | 25,925.800 ± 1,750.833 | 27,445.433 ± 758.900 | 0.94463× |
| day 150 | 100,000 | 0.00 | 30,757.600 ± 871.433 | 32,012.967 ± 400.333 | 0.96079× |
| day 0 | 100,000 | 0.50 | 27,123.400 ± 1,210.067 | 28,168.767 ± 367.967 | 0.96289× |
| day 150 | 100,000 | 0.50 | 33,513.267 ± 1,340.367 | 36,801.900 ± 2,389.300 | 0.91064× |
| day 0 | 100,000 | 1.00 | 22,304.867 ± 312.033 | 23,829.233 ± 577.900 | 0.93603× |
| day 150 | 100,000 | 1.00 | 30,133.333 ± 459.433 | 32,213.400 ± 443.167 | 0.93543× |

### Pool 4, chunk 256

| Checkpoint | N | p_local | Serial µs/tick | Parallel µs/tick | Paired speedup |
|---|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.00 | 2,354.200 ± 100.200 | 2,451.233 ± 102.733 | 0.96041× |
| day 150 | 10,000 | 0.00 | 2,617.600 ± 162.900 | 3,066.000 ± 209.233 | 0.85375× |
| day 0 | 10,000 | 0.50 | 2,385.500 ± 23.167 | 2,329.267 ± 191.133 | 1.02414× |
| day 150 | 10,000 | 0.50 | 2,901.567 ± 210.700 | 2,924.467 ± 157.867 | 0.99217× |
| day 0 | 10,000 | 1.00 | 2,320.267 ± 85.900 | 2,193.733 ± 106.333 | 1.05768× |
| day 150 | 10,000 | 1.00 | 2,781.967 ± 319.300 | 2,921.300 ± 89.000 | 0.95230× |
| day 0 | 100,000 | 0.00 | 27,254.767 ± 956.533 | 27,785.967 ± 379.667 | 0.98088× |
| day 150 | 100,000 | 0.00 | 29,687.033 ± 82.533 | 30,885.033 ± 201.300 | 0.96121× |
| day 0 | 100,000 | 0.50 | 26,373.433 ± 1,542.400 | 26,795.000 ± 1,066.433 | 0.98427× |
| day 150 | 100,000 | 0.50 | 30,147.333 ± 683.600 | 32,412.200 ± 1,443.633 | 0.93012× |
| day 0 | 100,000 | 1.00 | 22,263.867 ± 169.333 | 23,412.833 ± 247.233 | 0.95093× |
| day 150 | 100,000 | 1.00 | 30,441.600 ± 661.600 | 31,147.300 ± 274.967 | 0.97734× |

### Pool 6, chunk 256

| Checkpoint | N | p_local | Serial µs/tick | Parallel µs/tick | Paired speedup |
|---|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.00 | 2,348.000 ± 70.967 | 2,442.100 ± 119.300 | 0.96147× |
| day 150 | 10,000 | 0.00 | 3,042.433 ± 52.900 | 2,922.833 ± 192.000 | 1.04092× |
| day 0 | 10,000 | 0.50 | 2,134.700 ± 271.233 | 2,495.133 ± 126.400 | 0.85555× |
| day 150 | 10,000 | 0.50 | 2,991.300 ± 131.200 | 2,967.433 ± 193.067 | 1.00804× |
| day 0 | 10,000 | 1.00 | 2,123.000 ± 268.900 | 2,391.533 ± 218.067 | 0.88771× |
| day 150 | 10,000 | 1.00 | 2,810.400 ± 288.267 | 3,092.933 ± 149.033 | 0.90865× |
| day 0 | 100,000 | 0.00 | 26,919.867 ± 1,223.500 | 26,945.333 ± 901.333 | 0.99905× |
| day 150 | 100,000 | 0.00 | 29,742.900 ± 437.433 | 30,705.000 ± 473.233 | 0.96867× |
| day 0 | 100,000 | 0.50 | 26,021.400 ± 1,637.033 | 27,969.867 ± 577.567 | 0.93034× |
| day 150 | 100,000 | 0.50 | 29,902.000 ± 138.067 | 30,537.667 ± 286.233 | 0.97918× |
| day 0 | 100,000 | 1.00 | 22,254.900 ± 200.067 | 23,073.700 ± 251.100 | 0.96451× |
| day 150 | 100,000 | 1.00 | 29,899.167 ± 421.767 | 30,630.333 ± 294.333 | 0.97613× |

Across all 36 paired results the median whole-tick speedup was 0.96100× (pool 1: 0.94033×; pool 4: 0.96928×; pool 6: 0.96659×).

## Parallel Phase 6B stage times

Values are median ± MAD in µs/tick, serial → parallel. Parallel used the explicit caller-owned pool and chunk size 256; serial has no dependency-level build stage.

### Pool 1, chunk 256

Each cell is serial median ± MAD → parallel median ± MAD in µs/tick.

| Checkpoint | N | p_local | Validation / slot | Key generation | Sort | Level build | Evaluation / writeback / output |
|---|---:|---:|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.00 | 93.433 ± 12.700 → 189.433 ± 26.300 | 38.733 ± 5.567 → 20.567 ± 1.900 | 70.800 ± 9.167 → 65.267 ± 8.667 | 0.000 ± 0.000 → 12.933 ± 1.133 | 176.167 ± 16.267 → 125.867 ± 9.833 |
| day 150 | 10,000 | 0.00 | 135.733 ± 11.233 → 168.400 ± 9.067 | 38.967 ± 1.900 → 20.767 ± 0.433 | 136.767 ± 5.600 → 108.233 ± 0.900 | 0.000 ± 0.000 → 22.600 ± 1.100 | 293.133 ± 6.167 → 183.267 ± 6.900 |
| day 0 | 10,000 | 0.50 | 112.500 ± 5.767 → 173.867 ± 5.467 | 46.600 ± 1.500 → 17.233 ± 1.133 | 72.400 ± 2.800 → 62.500 ± 3.367 | 0.000 ± 0.000 → 13.300 ± 0.800 | 176.733 ± 5.800 → 118.933 ± 4.367 |
| day 150 | 10,000 | 0.50 | 100.900 ± 7.933 → 180.967 ± 11.500 | 25.433 ± 1.033 → 23.633 ± 2.433 | 120.200 ± 2.600 → 121.300 ± 6.500 | 0.000 ± 0.000 → 23.667 ± 1.933 | 275.167 ± 6.467 → 184.000 ± 6.300 |
| day 0 | 10,000 | 1.00 | 81.433 ± 0.500 → 166.500 ± 4.167 | 33.400 ± 0.500 → 18.067 ± 0.500 | 62.400 ± 0.633 → 60.667 ± 4.433 | 0.000 ± 0.000 → 12.667 ± 1.033 | 161.067 ± 5.233 → 117.800 ± 1.333 |
| day 150 | 10,000 | 1.00 | 133.767 ± 4.133 → 169.567 ± 4.433 | 39.000 ± 1.800 → 21.500 ± 1.033 | 136.500 ± 3.267 → 115.267 ± 7.433 | 0.000 ± 0.000 → 22.733 ± 1.500 | 295.833 ± 11.400 → 185.100 ± 7.733 |
| day 0 | 100,000 | 0.00 | 1,472.900 ± 49.633 → 2,273.733 ± 133.867 | 614.733 ± 73.867 → 62.133 ± 1.933 | 1,056.233 ± 40.867 → 695.867 ± 47.867 | 0.000 ± 0.000 → 198.033 ± 5.033 | 1,965.533 ± 60.333 → 1,444.100 ± 16.000 |
| day 150 | 100,000 | 0.00 | 1,202.667 ± 37.033 → 2,146.133 ± 62.500 | 380.400 ± 3.800 → 106.000 ± 2.033 | 1,792.733 ± 26.500 → 1,277.300 ± 4.900 | 0.000 ± 0.000 → 350.033 ± 10.033 | 2,989.800 ± 18.100 → 2,030.133 ± 80.500 |
| day 0 | 100,000 | 0.50 | 1,419.100 ± 56.233 → 2,398.033 ± 160.233 | 609.633 ± 52.467 → 74.500 ± 4.800 | 1,047.800 ± 33.733 → 726.833 ± 17.067 | 0.000 ± 0.000 → 207.667 ± 7.733 | 2,035.867 ± 57.800 → 1,485.233 ± 67.467 |
| day 150 | 100,000 | 0.50 | 1,611.867 ± 178.233 → 2,429.800 ± 170.700 | 462.733 ± 49.900 → 121.867 ± 20.900 | 2,032.833 ± 76.533 → 1,389.500 ± 84.000 | 0.000 ± 0.000 → 377.833 ± 33.067 | 3,270.433 ± 159.700 → 2,169.500 ± 129.500 |
| day 0 | 100,000 | 1.00 | 1,079.700 ± 28.167 → 2,216.333 ± 118.067 | 470.633 ± 4.400 → 63.367 ± 1.833 | 964.567 ± 16.400 → 648.767 ± 9.000 | 0.000 ± 0.000 → 189.633 ± 5.200 | 1,815.300 ± 19.433 → 1,404.200 ± 27.133 |
| day 150 | 100,000 | 1.00 | 1,243.533 ± 63.567 → 2,242.033 ± 95.733 | 379.300 ± 2.367 → 107.867 ± 2.600 | 1,839.733 ± 30.967 → 1,287.033 ± 11.633 | 0.000 ± 0.000 → 352.567 ± 20.267 | 3,033.433 ± 29.300 → 2,256.100 ± 113.133 |

### Pool 4, chunk 256

Each cell is serial median ± MAD → parallel median ± MAD in µs/tick.

| Checkpoint | N | p_local | Validation / slot | Key generation | Sort | Level build | Evaluation / writeback / output |
|---|---:|---:|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.00 | 113.867 ± 2.900 → 166.267 ± 9.700 | 46.967 ± 1.833 → 28.533 ± 2.033 | 72.567 ± 1.367 → 92.733 ± 17.133 | 0.000 ± 0.000 → 20.033 ± 3.500 | 177.000 ± 2.633 → 160.300 ± 12.033 |
| day 150 | 10,000 | 0.00 | 135.767 ± 4.300 → 157.467 ± 5.467 | 38.800 ± 0.433 → 32.867 ± 2.467 | 137.000 ± 2.567 → 144.033 ± 29.567 | 0.000 ± 0.000 → 28.000 ± 5.867 | 293.433 ± 7.033 → 223.300 ± 27.667 |
| day 0 | 10,000 | 0.50 | 113.700 ± 10.700 → 169.600 ± 9.633 | 46.600 ± 3.967 → 27.667 ± 0.933 | 73.067 ± 3.167 → 79.233 ± 16.267 | 0.000 ± 0.000 → 17.200 ± 3.433 | 178.067 ± 7.300 → 175.500 ± 8.667 |
| day 150 | 10,000 | 0.50 | 131.367 ± 11.067 → 158.267 ± 13.067 | 37.200 ± 2.933 → 30.633 ± 1.700 | 136.533 ± 6.200 → 176.467 ± 3.767 | 0.000 ± 0.000 → 34.333 ± 3.933 | 293.367 ± 13.067 → 225.300 ± 17.833 |
| day 0 | 10,000 | 1.00 | 104.133 ± 14.833 → 154.467 ± 14.467 | 42.833 ± 5.067 → 28.733 ± 3.100 | 71.667 ± 2.600 → 57.367 ± 1.200 | 0.000 ± 0.000 → 14.300 ± 1.900 | 171.200 ± 10.367 → 147.867 ± 13.700 |
| day 150 | 10,000 | 1.00 | 137.067 ± 9.633 → 150.600 ± 5.700 | 39.500 ± 2.700 → 31.300 ± 2.700 | 137.300 ± 11.133 → 144.100 ± 34.333 | 0.000 ± 0.000 → 27.567 ± 5.733 | 297.867 ± 6.667 → 217.333 ± 8.867 |
| day 0 | 100,000 | 0.00 | 1,339.533 ± 136.933 → 1,828.667 ± 32.567 | 612.900 ± 65.733 → 46.267 ± 3.233 | 1,039.967 ± 48.300 → 677.867 ± 34.500 | 0.000 ± 0.000 → 205.800 ± 4.433 | 1,961.867 ± 41.700 → 1,280.067 ± 42.300 |
| day 150 | 100,000 | 0.00 | 1,192.567 ± 14.867 → 1,575.567 ± 73.767 | 377.233 ± 1.433 → 57.367 ± 4.667 | 1,809.200 ± 31.767 → 1,469.633 ± 67.967 | 0.000 ± 0.000 → 371.833 ± 11.533 | 3,004.200 ± 34.667 → 1,732.833 ± 51.967 |
| day 0 | 100,000 | 0.50 | 1,300.567 ± 148.633 → 1,712.733 ± 105.367 | 544.133 ± 77.567 → 46.333 ± 2.967 | 1,021.733 ± 66.033 → 690.433 ± 40.167 | 0.000 ± 0.000 → 197.900 ± 3.467 | 1,978.000 ± 92.533 → 1,276.833 ± 47.833 |
| day 150 | 100,000 | 0.50 | 1,582.600 ± 221.533 → 1,685.833 ± 67.533 | 456.267 ± 73.067 → 59.067 ± 5.600 | 1,973.733 ± 117.300 → 1,294.367 ± 19.400 | 0.000 ± 0.000 → 369.833 ± 14.067 | 3,304.533 ± 128.100 → 1,703.267 ± 56.833 |
| day 0 | 100,000 | 1.00 | 1,099.967 ± 50.933 → 1,643.300 ± 120.100 | 468.600 ± 3.667 → 44.000 ± 2.033 | 945.433 ± 16.433 → 719.267 ± 67.433 | 0.000 ± 0.000 → 191.733 ± 4.567 | 1,807.700 ± 18.233 → 1,182.200 ± 83.300 |
| day 150 | 100,000 | 1.00 | 1,247.700 ± 53.600 → 1,635.633 ± 63.167 | 378.833 ± 2.033 → 58.033 ± 2.600 | 1,792.867 ± 7.333 → 1,348.067 ± 63.500 | 0.000 ± 0.000 → 359.200 ± 13.100 | 3,014.200 ± 27.467 → 1,851.800 ± 96.767 |

### Pool 6, chunk 256

Each cell is serial median ± MAD → parallel median ± MAD in µs/tick.

| Checkpoint | N | p_local | Validation / slot | Key generation | Sort | Level build | Evaluation / writeback / output |
|---|---:|---:|---:|---:|---:|---:|---:|
| day 0 | 10,000 | 0.00 | 113.500 ± 4.433 → 157.933 ± 12.133 | 47.033 ± 1.600 → 31.233 ± 2.900 | 72.333 ± 2.433 → 72.933 ± 15.833 | 0.000 ± 0.000 → 17.500 ± 3.567 | 178.367 ± 2.767 → 172.067 ± 16.167 |
| day 150 | 10,000 | 0.00 | 123.000 ± 14.167 → 172.367 ± 11.600 | 34.800 ± 4.767 → 35.733 ± 2.300 | 136.800 ± 8.067 → 178.633 ± 32.267 | 0.000 ± 0.000 → 35.233 ± 4.700 | 295.533 ± 6.367 → 265.533 ± 18.133 |
| day 0 | 10,000 | 0.50 | 94.067 ± 10.900 → 160.667 ± 11.167 | 39.300 ± 4.833 → 31.400 ± 3.467 | 71.067 ± 2.533 → 66.100 ± 8.800 | 0.000 ± 0.000 → 15.067 ± 1.433 | 167.933 ± 6.000 → 179.133 ± 4.733 |
| day 150 | 10,000 | 0.50 | 135.900 ± 3.800 → 159.267 ± 6.967 | 35.000 ± 5.433 → 38.500 ± 1.467 | 132.600 ± 8.600 → 121.867 ± 14.033 | 0.000 ± 0.000 → 24.100 ± 2.600 | 292.200 ± 7.433 → 259.600 ± 12.233 |
| day 0 | 10,000 | 1.00 | 113.200 ± 5.867 → 158.867 ± 7.333 | 46.733 ± 2.533 → 34.367 ± 1.567 | 73.133 ± 3.433 → 65.400 ± 9.400 | 0.000 ± 0.000 → 17.700 ± 3.767 | 178.400 ± 7.333 → 180.367 ± 16.800 |
| day 150 | 10,000 | 1.00 | 134.700 ± 2.567 → 164.033 ± 14.300 | 36.833 ± 2.733 → 35.333 ± 1.900 | 132.267 ± 7.600 → 152.500 ± 28.900 | 0.000 ± 0.000 → 28.667 ± 4.800 | 290.600 ± 12.767 → 254.033 ± 22.333 |
| day 0 | 100,000 | 0.00 | 1,355.700 ± 126.167 → 1,568.433 ± 92.333 | 585.100 ± 96.267 → 47.467 ± 1.533 | 1,064.667 ± 73.733 → 644.167 ± 3.133 | 0.000 ± 0.000 → 218.900 ± 22.700 | 1,970.267 ± 116.200 → 1,216.067 ± 34.900 |
| day 150 | 100,000 | 0.00 | 1,228.533 ± 38.367 → 1,387.800 ± 47.533 | 379.867 ± 4.767 → 56.300 ± 6.433 | 1,798.533 ± 33.467 → 1,304.533 ± 15.500 | 0.000 ± 0.000 → 354.067 ± 16.167 | 3,003.867 ± 34.833 → 1,632.933 ± 51.433 |
| day 0 | 100,000 | 0.50 | 1,369.000 ± 42.800 → 1,541.900 ± 64.600 | 619.333 ± 18.667 → 46.767 ± 2.967 | 1,061.467 ± 28.800 → 688.700 ± 33.100 | 0.000 ± 0.000 → 203.400 ± 12.400 | 1,950.700 ± 32.533 → 1,227.367 ± 29.767 |
| day 150 | 100,000 | 0.50 | 1,214.800 ± 34.433 → 1,419.767 ± 74.167 | 377.967 ± 2.833 → 55.567 ± 4.333 | 1,801.567 ± 8.000 → 1,289.600 ± 4.067 | 0.000 ± 0.000 → 335.033 ± 9.300 | 2,992.500 ± 28.200 → 1,563.000 ± 53.433 |
| day 0 | 100,000 | 1.00 | 1,045.233 ± 12.833 → 1,412.833 ± 36.500 | 466.867 ± 3.767 → 42.000 ± 3.533 | 979.933 ± 34.033 → 645.900 ± 2.667 | 0.000 ± 0.000 → 188.333 ± 3.133 | 1,788.700 ± 6.133 → 1,228.033 ± 50.200 |
| day 150 | 100,000 | 1.00 | 1,193.400 ± 24.533 → 1,393.467 ± 58.600 | 378.633 ± 1.767 → 53.067 ± 2.467 | 1,802.800 ± 15.200 → 1,298.300 ± 14.600 | 0.000 ± 0.000 → 349.033 ± 9.067 | 3,031.367 ± 31.400 → 1,702.800 ± 58.667 |

## Comparison with M3-02 interim-audit findings

| M3-02 finding | Sustained workload result | Status |
|---|---|---|
| Endpoint conflict fraction around 0.53–0.54, largely independent of locality | At day 0 it was 0.528892–0.541160; after 150 days it was 0.798802–0.802440. At both horizons the three locality values stayed close. | Holds at day 0; changes after day 150 |
| About 5–6 dependency levels/day | Day 0 measured 4.33–6.33 levels/day; day 150 measured 7.00–8.33. | Similar at day 0; more levels at day 150 |
| Parallel Phase 6B median whole-tick speedup about 0.97×; one-thread Rayon slower | Across all 36 paired results median speedup was 0.96100× (pool 1: 0.94033×; pool 4: 0.96928×; pool 6: 0.96659×). | No general whole-tick gain; broadly holds |
| Search/setup was about 8–10× Applied-positive work | Day 0 ratios were 8.655–10.582×; day 150 ratios were 7.404–8.230×. | Holds in direction; lower after day 150 |
| GiveFood found no targets in the short active window | GiveFood Some remained zero at day 0; every measured day-150 cell had GiveFood Some targets. | Changes by day 150 |
| About 50% of keyed entries ended TheftFailed | Day 0 keyed entries were all StealFood and about half ended TheftFailed; at day 150 TheftFailed was about 23.8% of keyed entries while TargetIneligible increased. | Holds at day 0; changes by day 150 |

## Findings

**Measured facts:** the selected one-knob configuration passed the 200-day gates at N=10,000 and for all three N=100,000 locality settings. Day-150 checkpoints retained about 100% of the population; mean food among the living was about 2.0–2.2, GiveFood had eligible targets, and keyed volume was roughly 46% of alive population per day.

**Inference:** locality still did not determine endpoint conflict fraction in this workload. The day-150 food shortage shifted targeting toward GiveFood, increased conflicts and dependency levels, and changed the StealFood outcome mix. The tested parallel Phase 6B path still did not give a consistent whole-tick speedup.

## Limitations

- One Windows host and one seed/replicate configuration were measured.
- The 200-day gate permits population decline; alive population fell to roughly 58% by day 200.
- Phase profiles use an opt-in timing wrapper and manual phase driver. Each profiled day was compared to `run_m3_storage_days` for exact State hash, Metrics record, and Events vector. Whole-tick timings use the uninstrumented production runner.
- Timing-hook overhead was material; per-entry splits are approximate.
- The relationship graph is static; this benchmark does not measure migration or dynamic topology.
