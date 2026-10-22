# M3-01 Serial Baseline
Status: current

M3-01 complete

- **Reference:** M3-00 single-threaded WorldState runner
- **Measured path:** serial Hybrid / SegmentedAgentStorage authority
- **Baseline commit:** `e2c96ca90350f87ad91cf4d55b29a2f783a3e297`
- **Decision:** PASS — exact digest and snapshot parity; serial baseline recorded

## Executive summary

The M3-01 storage runner matched the M3-00 reference on the committed golden, the complete 60-cell 100-day differential matrix, a 10,000-agent 100-day release differential, snapshot pause/resume, and reversed storage slots. The M3 State, Metrics, and Events digests matched exactly. No M2 expected values or snapshots were changed.

On this host, the measured full-tick medians ranged from 1.927 ms at 10k to 30.110 ms at 100k. The 50k-to-100k alpha ranged from 0.805 to 1.242 across `k` / `p_local` cells, so these samples do not support one universal scaling exponent. One-time graph construction grows substantially with `N` and `k`; projection construction is smaller but visible and should be amortized over the run.

The conflict fraction is about 0.529–0.555 and mean contention about 1.193–1.207 across measured cells. The cross-partition interaction count falls with `p_local` and reaches zero at `p_local = 1`, while endpoint conflict remains present. This is useful M3-02 scheduling evidence, not a measured parallel speedup.

## Host and method

- **CPU:** AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors
- **OS:** Windows 11 Pro, build 26200
- **Rust:** `1.98.1-x86_64-pc-windows-msvc`, selected by `rust-toolchain.toml`
- **Authority:** Hybrid / SegmentedAgentStorage, serial only
- **Configuration:** M2-40 `GATE_CONFIG_TOML` from `m0_baseline_bench.rs`, unchanged action weights and biases; only `profile = "m3"`, `relationship_degree_k`, and `p_local` were added or varied. Workloads use `K = N / 200`.

The full-tick benchmark command was `cargo bench --bench m3_01_serial_baseline_bench`. It completed in **114.405 seconds**, including release compilation. Each of the 45 `N × k × p_local` cells used two warmups and seven timed samples. Each sample cloned the same initialized state, constructed its graph/projection and runner context before timing, then called `run_m3_storage_days` for three consecutive days with scratch reuse. Metrics and events were enabled; snapshots were disabled. The table reports median ± MAD in µs/tick.

Graph and initial slot-projection costs were timed separately after initialization, with two warmups and seven samples per cell. Graph timing excludes projection construction; projection timing uses a prebuilt graph and storage instance.

Interaction volume and conflict density were collected in a separate untimed three-day pass per cell. It used the public storage Phase 4/Phase 5 paths, `count_cross_partition_interactions_storage_per_day`, and public `compute_m3_resolution_key` / `compare_keyed_interactions` to reconstruct that day's global keyed stream. The conflict fraction and mean contention follow §9.9. The pass is deterministic from the same initial state as each full-tick sample, so all seven timed samples have the same three-day interaction-volume trajectory.

Per-phase profiling used a temporary bench-only driver calling the public serial phase APIs in production order, with the same two warmups, seven timed samples, three-day scratch reuse, events enabled, and snapshots disabled. It profiled `N=100k, k=8` for every `p_local`, plus `N=10k/50k/100k, p_local=0.5, k=8`. Temporary local instrumentation split Phase 6B into validation/slot lookup, key construction/sort, and sequential commit/output assembly; that instrumentation was removed after measurement and is not committed. The profile-only pass took **18.796 seconds** including compilation. The combined full-tick and profile measurement wall time was **133.201 seconds**.

The temporary per-phase run used `$env:SIMCIV_M3_PHASE6B_TRACE_FILE = Join-Path $env:TEMP 'simulaciv_m3_01_phase6b_trace.csv'` followed by `cargo bench --bench m3_01_serial_baseline_bench -- --profile-only`. This measurement-only mode and the Phase 6B trace hook were removed after the profile data was recorded; the committed harness's default command runs the full-tick, one-time-cost, volume/conflict, and release-differential sections.

## Full-tick results and scaling

All 45 cells are reported below. Scaling alpha is `ln(T₂/T₁) / ln(N₂/N₁)` for adjacent populations. Values are per cell and are not averaged across configurations.

### Full-tick samples

| N | K | k | p_local | median ± MAD (µs/tick) |
|---:|---:|---:|---:|---:|
| 10,000 | 50 | 4 | 0.00 | 2158.4 ± 18.8 |
| 10,000 | 50 | 8 | 0.00 | 2177.4 ± 4.8 |
| 10,000 | 50 | 16 | 0.00 | 2341.7 ± 19.3 |
| 10,000 | 50 | 4 | 0.25 | 2089.2 ± 24.4 |
| 10,000 | 50 | 8 | 0.25 | 2004.1 ± 107.7 |
| 10,000 | 50 | 16 | 0.25 | 2016.3 ± 79.7 |
| 10,000 | 50 | 4 | 0.50 | 1964.6 ± 68.5 |
| 10,000 | 50 | 8 | 0.50 | 2099.0 ± 125.5 |
| 10,000 | 50 | 16 | 0.50 | 2619.3 ± 65.6 |
| 10,000 | 50 | 4 | 0.75 | 2388.9 ± 44.3 |
| 10,000 | 50 | 8 | 0.75 | 1927.1 ± 80.5 |
| 10,000 | 50 | 16 | 0.75 | 2582.7 ± 108.3 |
| 10,000 | 50 | 4 | 1.00 | 2173.8 ± 188.5 |
| 10,000 | 50 | 8 | 1.00 | 2450.9 ± 101.9 |
| 10,000 | 50 | 16 | 1.00 | 2568.6 ± 82.8 |
| 50,000 | 250 | 4 | 0.00 | 12929.7 ± 877.1 |
| 50,000 | 250 | 8 | 0.00 | 13179.0 ± 597.3 |
| 50,000 | 250 | 16 | 0.00 | 13775.9 ± 306.6 |
| 50,000 | 250 | 4 | 0.25 | 12778.5 ± 337.5 |
| 50,000 | 250 | 8 | 0.25 | 11428.6 ± 1513.6 |
| 50,000 | 250 | 16 | 0.25 | 14209.7 ± 480.0 |
| 50,000 | 250 | 4 | 0.50 | 12909.2 ± 261.2 |
| 50,000 | 250 | 8 | 0.50 | 12464.4 ± 812.6 |
| 50,000 | 250 | 16 | 0.50 | 13987.0 ± 620.8 |
| 50,000 | 250 | 4 | 0.75 | 12450.6 ± 104.8 |
| 50,000 | 250 | 8 | 0.75 | 13185.1 ± 701.0 |
| 50,000 | 250 | 16 | 0.75 | 13506.6 ± 1117.0 |
| 50,000 | 250 | 4 | 1.00 | 11422.3 ± 545.9 |
| 50,000 | 250 | 8 | 1.00 | 12830.5 ± 479.8 |
| 50,000 | 250 | 16 | 1.00 | 13459.6 ± 1026.7 |
| 100,000 | 500 | 4 | 0.00 | 29462.8 ± 1665.5 |
| 100,000 | 500 | 8 | 0.00 | 29449.0 ± 698.3 |
| 100,000 | 500 | 16 | 0.00 | 29007.7 ± 1433.8 |
| 100,000 | 500 | 4 | 0.25 | 28223.4 ± 1254.9 |
| 100,000 | 500 | 8 | 0.25 | 27034.6 ± 1376.3 |
| 100,000 | 500 | 16 | 0.25 | 30109.7 ± 1707.6 |
| 100,000 | 500 | 4 | 0.50 | 29597.6 ± 1167.2 |
| 100,000 | 500 | 8 | 0.50 | 26744.9 ± 1926.2 |
| 100,000 | 500 | 16 | 0.50 | 24435.7 ± 220.3 |
| 100,000 | 500 | 4 | 0.75 | 25513.0 ± 1744.5 |
| 100,000 | 500 | 8 | 0.75 | 25579.8 ± 1893.3 |
| 100,000 | 500 | 16 | 0.75 | 25118.5 ± 969.4 |
| 100,000 | 500 | 4 | 1.00 | 26892.4 ± 1239.1 |
| 100,000 | 500 | 8 | 1.00 | 25968.4 ± 2718.7 |
| 100,000 | 500 | 16 | 1.00 | 28345.0 ± 2239.6 |

### Adjacent-population scaling alpha

| k | p_local | α 10k→50k | α 50k→100k |
|---:|---:|---:|---:|
| 4 | 0.00 | 1.112 | 1.188 |
| 4 | 0.25 | 1.125 | 1.143 |
| 4 | 0.50 | 1.170 | 1.197 |
| 4 | 0.75 | 1.026 | 1.035 |
| 4 | 1.00 | 1.031 | 1.235 |
| 8 | 0.00 | 1.119 | 1.160 |
| 8 | 0.25 | 1.082 | 1.242 |
| 8 | 0.50 | 1.107 | 1.101 |
| 8 | 0.75 | 1.195 | 0.956 |
| 8 | 1.00 | 1.029 | 1.017 |
| 16 | 0.00 | 1.101 | 1.074 |
| 16 | 0.25 | 1.213 | 1.083 |
| 16 | 0.50 | 1.041 | 0.805 |
| 16 | 0.75 | 1.028 | 0.895 |
| 16 | 1.00 | 1.029 | 1.074 |

### One-time graph and projection costs

| N | K | k | p_local | graph build median ± MAD (µs) | projection build median ± MAD (µs) |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 50 | 4 | 0.00 | 9036.6 ± 32.5 | 702.2 ± 26.6 |
| 10,000 | 50 | 8 | 0.00 | 17719.5 ± 59.2 | 895.3 ± 16.6 |
| 10,000 | 50 | 16 | 0.00 | 36242.8 ± 68.4 | 1504.7 ± 6.3 |
| 10,000 | 50 | 4 | 0.25 | 7264.3 ± 17.6 | 635.8 ± 1.3 |
| 10,000 | 50 | 8 | 0.25 | 14556.9 ± 23.1 | 873.7 ± 1.1 |
| 10,000 | 50 | 16 | 0.25 | 30548.4 ± 287.2 | 1495.4 ± 9.1 |
| 10,000 | 50 | 4 | 0.50 | 5613.7 ± 10.7 | 638.4 ± 2.2 |
| 10,000 | 50 | 8 | 0.50 | 13487.0 ± 50.6 | 1264.2 ± 5.7 |
| 10,000 | 50 | 16 | 0.50 | 24644.0 ± 948.2 | 1550.4 ± 58.0 |
| 10,000 | 50 | 4 | 0.75 | 4519.0 ± 37.7 | 892.8 ± 11.9 |
| 10,000 | 50 | 8 | 0.75 | 10370.4 ± 32.7 | 1259.7 ± 6.8 |
| 10,000 | 50 | 16 | 0.75 | 16132.8 ± 30.7 | 1499.4 ± 1.8 |
| 10,000 | 50 | 4 | 1.00 | 2061.7 ± 9.2 | 641.4 ± 0.8 |
| 10,000 | 50 | 8 | 1.00 | 5408.8 ± 34.3 | 885.8 ± 7.6 |
| 10,000 | 50 | 16 | 1.00 | 10744.5 ± 232.1 | 2226.8 ± 35.6 |
| 50,000 | 250 | 4 | 0.00 | 56083.5 ± 265.4 | 5599.8 ± 243.6 |
| 50,000 | 250 | 8 | 0.00 | 110366.4 ± 836.8 | 7976.8 ± 115.9 |
| 50,000 | 250 | 16 | 0.00 | 220586.7 ± 1115.3 | 13408.2 ± 691.7 |
| 50,000 | 250 | 4 | 0.25 | 46151.4 ± 1033.0 | 5605.9 ± 32.4 |
| 50,000 | 250 | 8 | 0.25 | 90929.6 ± 465.4 | 8158.9 ± 150.5 |
| 50,000 | 250 | 16 | 0.25 | 185192.4 ± 460.1 | 13008.2 ± 458.1 |
| 50,000 | 250 | 4 | 0.50 | 33373.4 ± 1405.8 | 5598.9 ± 99.4 |
| 50,000 | 250 | 8 | 0.50 | 73412.0 ± 3111.3 | 8029.2 ± 228.5 |
| 50,000 | 250 | 16 | 0.50 | 142146.4 ± 381.2 | 13354.6 ± 283.8 |
| 50,000 | 250 | 4 | 0.75 | 24864.8 ± 24.6 | 5543.3 ± 21.5 |
| 50,000 | 250 | 8 | 0.75 | 54978.4 ± 1343.4 | 7460.3 ± 1212.1 |
| 50,000 | 250 | 16 | 0.75 | 97179.5 ± 3331.9 | 12369.9 ± 1842.9 |
| 50,000 | 250 | 4 | 1.00 | 12354.7 ± 1364.2 | 4459.1 ± 487.0 |
| 50,000 | 250 | 8 | 1.00 | 34476.6 ± 628.1 | 8558.6 ± 285.0 |
| 50,000 | 250 | 16 | 1.00 | 51766.5 ± 2465.5 | 9235.2 ± 370.4 |
| 100,000 | 500 | 4 | 0.00 | 117992.4 ± 2192.5 | 11834.0 ± 1686.7 |
| 100,000 | 500 | 8 | 0.00 | 235110.2 ± 2500.8 | 17507.7 ± 1228.7 |
| 100,000 | 500 | 16 | 0.00 | 464407.2 ± 2331.3 | 24886.9 ± 2266.0 |
| 100,000 | 500 | 4 | 0.25 | 93682.3 ± 1485.1 | 10994.9 ± 1864.6 |
| 100,000 | 500 | 8 | 0.25 | 188966.1 ± 2221.0 | 17666.8 ± 323.5 |
| 100,000 | 500 | 16 | 0.25 | 385842.2 ± 2911.6 | 29494.1 ± 2108.5 |
| 100,000 | 500 | 4 | 0.50 | 74893.8 ± 1207.8 | 10916.4 ± 1733.6 |
| 100,000 | 500 | 8 | 0.50 | 152884.7 ± 1862.7 | 13806.1 ± 1727.4 |
| 100,000 | 500 | 16 | 0.50 | 290165.7 ± 3299.0 | 23133.6 ± 2855.1 |
| 100,000 | 500 | 4 | 0.75 | 50017.7 ± 1991.7 | 8692.9 ± 274.4 |
| 100,000 | 500 | 8 | 0.75 | 111405.5 ± 3909.5 | 12557.5 ± 587.0 |
| 100,000 | 500 | 16 | 0.75 | 196307.8 ± 4380.1 | 25852.6 ± 2722.1 |
| 100,000 | 500 | 4 | 1.00 | 24802.0 ± 1955.3 | 11521.0 ± 1144.9 |
| 100,000 | 500 | 8 | 1.00 | 58885.9 ± 2152.6 | 12372.9 ± 432.0 |
| 100,000 | 500 | 16 | 1.00 | 97346.4 ± 3689.6 | 26347.4 ± 2342.1 |

### Interaction volume and conflict density

| N | K | k | p_local | keyed entries/day | cross-partition interactions/day | conflict fraction | mean contention |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | 50 | 4 | 0.00 | 2473.7 | 2473.7 | 0.5545 | 1.2072 |
| 10,000 | 50 | 8 | 0.00 | 2473.7 | 2473.7 | 0.5330 | 1.1952 |
| 10,000 | 50 | 16 | 0.00 | 2473.7 | 2473.7 | 0.5525 | 1.2058 |
| 10,000 | 50 | 4 | 0.25 | 2473.7 | 1859.3 | 0.5486 | 1.2024 |
| 10,000 | 50 | 8 | 0.25 | 2473.7 | 1859.7 | 0.5296 | 1.1936 |
| 10,000 | 50 | 16 | 0.25 | 2473.7 | 1866.7 | 0.5553 | 1.2063 |
| 10,000 | 50 | 4 | 0.50 | 2473.7 | 1257.0 | 0.5380 | 1.1998 |
| 10,000 | 50 | 8 | 0.50 | 2473.7 | 1238.0 | 0.5289 | 1.1926 |
| 10,000 | 50 | 16 | 0.50 | 2473.7 | 1242.7 | 0.5438 | 1.2044 |
| 10,000 | 50 | 4 | 0.75 | 2473.7 | 641.0 | 0.5471 | 1.2025 |
| 10,000 | 50 | 8 | 0.75 | 2473.7 | 618.0 | 0.5371 | 1.1976 |
| 10,000 | 50 | 16 | 0.75 | 2473.7 | 602.0 | 0.5456 | 1.2034 |
| 10,000 | 50 | 4 | 1.00 | 2473.7 | 0.0 | 0.5400 | 1.2006 |
| 10,000 | 50 | 8 | 1.00 | 2473.7 | 0.0 | 0.5376 | 1.1965 |
| 10,000 | 50 | 16 | 1.00 | 2473.7 | 0.0 | 0.5418 | 1.1988 |
| 50,000 | 250 | 4 | 0.00 | 12255.7 | 12255.7 | 0.5346 | 1.1970 |
| 50,000 | 250 | 8 | 0.00 | 12255.7 | 12255.7 | 0.5390 | 1.1984 |
| 50,000 | 250 | 16 | 0.00 | 12256.7 | 12256.7 | 0.5390 | 1.1991 |
| 50,000 | 250 | 4 | 0.25 | 12255.7 | 9176.7 | 0.5368 | 1.1976 |
| 50,000 | 250 | 8 | 0.25 | 12255.7 | 9234.0 | 0.5380 | 1.1977 |
| 50,000 | 250 | 16 | 0.25 | 12255.7 | 9242.0 | 0.5416 | 1.1990 |
| 50,000 | 250 | 4 | 0.50 | 12255.7 | 6163.0 | 0.5335 | 1.1964 |
| 50,000 | 250 | 8 | 0.50 | 12255.7 | 6130.7 | 0.5369 | 1.1966 |
| 50,000 | 250 | 16 | 0.50 | 12255.7 | 6168.7 | 0.5369 | 1.1981 |
| 50,000 | 250 | 4 | 0.75 | 12255.7 | 3089.3 | 0.5315 | 1.1961 |
| 50,000 | 250 | 8 | 0.75 | 12255.7 | 3064.7 | 0.5388 | 1.1985 |
| 50,000 | 250 | 16 | 0.75 | 12255.7 | 3111.3 | 0.5390 | 1.1989 |
| 50,000 | 250 | 4 | 1.00 | 12255.7 | 0.0 | 0.5338 | 1.1959 |
| 50,000 | 250 | 8 | 1.00 | 12255.7 | 0.0 | 0.5413 | 1.1991 |
| 50,000 | 250 | 16 | 1.00 | 12255.7 | 0.0 | 0.5359 | 1.1983 |
| 100,000 | 500 | 4 | 0.00 | 24584.3 | 24584.3 | 0.5387 | 1.1987 |
| 100,000 | 500 | 8 | 0.00 | 24584.3 | 24584.3 | 0.5406 | 1.2000 |
| 100,000 | 500 | 16 | 0.00 | 24584.3 | 24584.3 | 0.5430 | 1.2008 |
| 100,000 | 500 | 4 | 0.25 | 24584.3 | 18417.3 | 0.5419 | 1.2003 |
| 100,000 | 500 | 8 | 0.25 | 24584.3 | 18539.0 | 0.5388 | 1.1991 |
| 100,000 | 500 | 16 | 0.25 | 24584.3 | 18529.3 | 0.5400 | 1.1991 |
| 100,000 | 500 | 4 | 0.50 | 24584.3 | 12291.3 | 0.5403 | 1.1995 |
| 100,000 | 500 | 8 | 0.50 | 24584.3 | 12288.0 | 0.5388 | 1.1995 |
| 100,000 | 500 | 16 | 0.50 | 24584.3 | 12343.7 | 0.5386 | 1.1989 |
| 100,000 | 500 | 4 | 0.75 | 24584.3 | 6172.0 | 0.5393 | 1.1996 |
| 100,000 | 500 | 8 | 0.75 | 24584.3 | 6137.7 | 0.5368 | 1.1980 |
| 100,000 | 500 | 16 | 0.75 | 24584.3 | 6200.3 | 0.5372 | 1.1971 |
| 100,000 | 500 | 4 | 1.00 | 24584.3 | 0.0 | 0.5406 | 1.2000 |
| 100,000 | 500 | 8 | 1.00 | 24584.3 | 0.0 | 0.5412 | 1.1996 |
| 100,000 | 500 | 16 | 1.00 | 24584.3 | 0.0 | 0.5402 | 1.1990 |

### Per-phase profile

Columns are `N / p_local`; all cells use `K=N/200` and `k=8`. Phase 6B validation includes input checks and cached slot resolution, key+sort includes global key generation and ordering, and commit+output includes sequential storage updates and owned grouped-result assembly.

| Phase (µs/tick, median ± MAD) | 10k / .5 | 50k / .5 | 100k / 0 | 100k / 0.25 | 100k / 0.5 | 100k / 0.75 | 100k / 1 |
|---|---:|---:|---:|---:|---:|---:|---:|
| P1 | 0.2 ± 0.0 | 0.3 ± 0.0 | 0.4 ± 0.0 | 0.5 ± 0.0 | 0.5 ± 0.0 | 0.5 ± 0.0 | 0.5 ± 0.0 |
| P2 | 10.7 ± 0.4 | 46.5 ± 2.0 | 82.8 ± 0.5 | 98.0 ± 8.1 | 103.3 ± 3.3 | 104.5 ± 6.2 | 95.3 ± 4.8 |
| P3 | 58.4 ± 1.1 | 357.5 ± 18.8 | 585.7 ± 3.0 | 840.2 ± 44.7 | 829.6 ± 57.6 | 821.5 ± 64.4 | 701.9 ± 31.6 |
| P4 selection | 520.6 ± 8.0 | 2471.8 ± 23.6 | 4310.1 ± 21.3 | 4999.4 ± 186.2 | 5079.2 ± 128.7 | 4903.2 ± 56.5 | 4798.3 ± 107.4 |
| P4 intent+projection | 267.2 ± 2.2 | 1599.4 ± 30.0 | 3033.9 ± 49.9 | 3872.3 ± 164.5 | 3814.4 ± 107.0 | 4009.9 ± 258.2 | 3891.1 ± 180.5 |
| P5 | 142.1 ± 4.9 | 773.0 ± 33.1 | 1698.8 ± 62.0 | 1910.6 ± 85.0 | 1452.3 ± 249.4 | 1604.4 ± 156.1 | 1565.5 ± 67.3 |
| P6A | 114.0 ± 5.0 | 632.5 ± 19.6 | 1029.8 ± 10.4 | 1373.0 ± 142.3 | 1248.7 ± 121.3 | 1296.3 ± 92.1 | 1513.8 ± 56.5 |
| P6B validation+slot | 108.4 ± 6.6 | 735.3 ± 54.0 | 1058.2 ± 9.1 | 1499.6 ± 123.8 | 1384.1 ± 91.2 | 1442.1 ± 115.6 | 1713.3 ± 122.0 |
| P6B key+sort | 122.9 ± 4.3 | 735.0 ± 34.7 | 1497.5 ± 19.4 | 1842.6 ± 68.2 | 1709.1 ± 98.0 | 1674.5 ± 57.3 | 1664.5 ± 55.8 |
| P6B commit+output | 85.2 ± 3.3 | 466.7 ± 28.9 | 1493.2 ± 110.2 | 1477.8 ± 193.6 | 1066.6 ± 154.7 | 1129.4 ± 155.2 | 1082.3 ± 25.2 |
| P7 | 265.1 ± 7.4 | 1443.7 ± 71.0 | 2788.3 ± 38.5 | 3597.4 ± 393.7 | 3367.7 ± 118.4 | 3440.5 ± 527.4 | 3786.6 ± 324.9 |
| P8 | 138.4 ± 3.2 | 709.7 ± 19.8 | 1204.0 ± 13.5 | 1573.8 ± 24.8 | 1579.4 ± 125.9 | 1434.2 ± 36.1 | 1653.8 ± 49.8 |
| P9 | 108.7 ± 2.5 | 546.7 ± 41.9 | 844.7 ± 4.0 | 1105.5 ± 14.4 | 1173.3 ± 69.2 | 983.5 ± 13.2 | 1123.1 ± 24.9 |
| event staging | 137.5 ± 7.3 | 564.1 ± 23.4 | 939.3 ± 3.6 | 1069.4 ± 35.5 | 1041.9 ± 27.9 | 1033.8 ± 26.0 | 1077.4 ± 38.8 |
| P10 | 258.3 ± 10.7 | 1421.5 ± 63.9 | 2474.0 ± 0.9 | 3098.2 ± 88.8 | 3040.0 ± 246.2 | 2667.0 ± 81.6 | 3041.1 ± 57.6 |
| P11 event flush | 171.7 ± 2.3 | 885.9 ± 35.5 | 1561.1 ± 7.4 | 1786.9 ± 44.9 | 1860.0 ± 80.5 | 1736.8 ± 23.4 | 1823.9 ± 81.6 |

## M2 serial context

The M2-40 fixed-group-size Serial measurements are historical context only, not a like-for-like comparison: M2 uses same-settlement target selection and settlement-local Phase 6B ordering.

| N / K | M2-40 Serial median ± MAD (µs/tick) | M3-01 comparison caveat |
|---:|---:|---|
| 10k / 50 | 1,846.40 ± 69.40 | Different targeting candidates and Phase 6B ordering |
| 50k / 250 | 10,081.03 ± 44.73 | Different targeting candidates and Phase 6B ordering |
| 100k / 500 | 21,632.20 ± 194.00 | Different targeting candidates and Phase 6B ordering |

## Release differential and parity

The one-off release differential used `N=10,000`, `K=50`, seed `81985529216486895`, `replicate_id=7`, `k=8`, `p_local=0.5`, 100 days, metrics/events enabled, and snapshots disabled. Initialization and graph/context construction were outside each timer. The reference and storage runs were timed once each; this is not a repeated median benchmark.

| Digest | M3-00 reference | M3-01 storage | Result |
|---|---|---|---|
| State | `fb7652ab6dfc09ff88fac5bde22c320433e95c9755c8cd1cef23f3c6515f3b00` | `fb7652ab6dfc09ff88fac5bde22c320433e95c9755c8cd1cef23f3c6515f3b00` | exact |
| Metrics | `fc4a79b5f8ff76ab6f31f4609926644a803aea6726790a735d5e64656bcbd26e` | `fc4a79b5f8ff76ab6f31f4609926644a803aea6726790a735d5e64656bcbd26e` | exact |
| Events | `df279613a6e5ecf446ba79d7e89a7aa7903521bdbf967331c339dd3e196834e6` | `df279613a6e5ecf446ba79d7e89a7aa7903521bdbf967331c339dd3e196834e6` | exact |

| Runner | Single 100-day elapsed time |
|---|---:|
| M3-00 reference | 4,462.195 ms |
| M3-01 storage | 158.124 ms |

The same runner differential and parity gates passed for the committed M3 golden configuration, the §9.7 CI matrix, snapshot pause/resume with regenerated graph/projection, and reversed storage slots. No golden, snapshot, or canonical expected value was re-blessed.

## M3-02 predictions from measured conflicts

- At 100k, `k=8`, `p_local=0.5`, the keyed stream averages 24,584.3 entries/day, of which 12,288.0/day are cross-partition. The conflict fraction is 0.5388 and mean contention is 1.1995.
- At `p_local=1`, cross-partition interactions are zero by construction, but the measured conflict fraction remains about 0.54. Settlement locality therefore does not imply that keyed entries have disjoint endpoints; a partition fast path must preserve the M3-02 contract's disjointness condition.
- Across all cells, conflict fraction changes little with `p_local` compared with the cross-partition count. This suggests the endpoint-sharing graph, rather than cross-partition ratio alone, should guide deterministic reservation batches. Roughly 45–47% of keyed entries in these measurements have no shared endpoint with another entry that day; that fraction is an upper-bound indicator for independent work, not predicted speedup.
- The `p_local=1` full-tick measurements remain the baseline for global-stream overhead. The measured differences from other `p_local` values are non-monotonic and do not isolate a causal cost beyond the one-time graph construction and interaction volume.

These are scheduling hypotheses from serial traces. M3-02 must measure actual deterministic-parallel speedup and preserve global order for entries sharing an agent.
