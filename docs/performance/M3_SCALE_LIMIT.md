<a id="m3-scale-limit-search"></a>
# M3-05 (formerly M3-S) Scale Limit Search

This report records the requested static-world scale search on the equilibrium preset. It is a benchmark-only study; it changes no simulation semantics or production behavior. The subsequent M3-K settlement-index before/after measurements are recorded in [M3 Settlement Index](M3_SETTLEMENT_INDEX.md); the measurements below are unchanged.

## Executive summary

- The full matrix completed through `N=1,000,000`, with `K=N/200`, `k ∈ {8,16}`, `p_local=0.5`, at day 0 and after a 50-day checkpoint.
- All 16 serial/parallel paired cells had identical final State, Metrics, and Events digests. No capacity, overflow, or runner error occurred.
- First-break criteria triggered. The graph-plus-projection setup exceeded ten day-0 serial-tick equivalents at `N=100,000, k=16` (`10.9230×`). Full-tick alpha exceeded `1.15` in every k/horizon profile on `100k→250k`. Per-agent serial tick time increased by more than 25% at `N=250,000` for three profiles and at `N=500,000` for `k=16`, day 50.
- Memory did not approach the 50%-of-RAM limit. The largest sampled OS working set was `872,611,840` bytes at `N=1,000,000, k=16`, against `102,663,720,960` bytes of host RAM.
- The largest configured population, one million agents, completed for both degrees and both checkpoints.

## Host, configuration, and method

- **Host:** AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors; Windows 11 Pro; `rustc 1.98.1`; `x86_64-pc-windows-msvc`.
- **RAM:** `102,663,720,960` bytes (`95.61 GiB`); the 50% threshold is `51,331,860,480` bytes.
- **Configuration:** the M2-40 food-supply fields unchanged except `environment.base_metabolic_cost=0.3129`, the selected M3 equilibrium preset. M3 fields are `profile=m3`, `relationship_degree_k ∈ {8,16}`, and `p_local=0.5`. Seed `81985529216486895`, replicate `7`, `K=N/200`.
- **Equilibrium boundary:** `0.3129` passed the defined N=10,000 grid but its N=100,000 day-50-to-300 food drift was `6.125%`, above the 5% gate; the selected configuration is still used here as requested. These scale cells are day-0/day-50 checkpoints with 3-day samples, not a long-horizon stability recheck.
- **Cells:** `N ∈ {100,000,250,000,500,000,1,000,000}`; day 0 and a day-50 checkpoint.
- **Whole tick:** production `run_m3_storage_days` and `run_m3_storage_days_with_parallel_phase6b`, pool size 6 and chunk 256. The pool is built once. Initialization, graph/projection construction, checkpoint restoration, and run-context construction are outside the timer. Each paired sample starts from two clones of the same checkpoint, alternates serial-first / parallel-first order, and runs three consecutive days with scratch reuse. Two warmups and seven timed pairs are used; outputs and digests are computed outside the timers.
- **Phase profile:** a bench-only serial phase driver times P1–P11, splitting Phase 4 into action selection and intent/projection. Phase 6B substage durations use its existing opt-in timing API. For every profile sample day, the driver’s complete `M3DayOutcome` and State hash equal the production serial runner. Each profile has two warmups and seven timed samples of three days; medians and MADs use the 21 timed day observations.
- **Memory:** SoA columns use actual `Vec::capacity()` values. CSR and projection Vec allocation sizes are captured around their standalone construction. Serial scratch is the retained heap after three warmed days, subtracting graph, projection, and membership storage. Parallel extra scratch is the retained parallel-context delta over the serial context. A bench-only global allocator byte counter is enabled only during these untimed memory probes. OS working set is sampled every 2 ms during a separate warm memory pass. The allocator counter is disabled during all timing samples; its disabled check remains in the benchmark allocator, so timing numbers include that small harness cost.
- **Command:** `cargo bench -p sim-model --bench m3_scale_limit_bench`.
- **Total wall time:** `1,532.853 s` (`25 min 32.853 s`), measured from the benchmark log creation through its final write; this includes Cargo invocation and compilation.

## Initialization, graph, projection, and memory

Initialization is `initialize_world` plus Hybrid storage construction. Graph and projection times are separately timed. Setup tick equivalents use `(graph + projection) / day-0 serial median tick` for the same `N,k`.

| N | K | k | initialization µs | graph µs | projection µs | graph + projection / day-0 tick |
|---:|---:|---:|---:|---:|---:|---:|
| 100,000 | 500 | 8 | 4,268.200 | 135,048.900 | 12,745.600 | 5.7368× |
| 100,000 | 500 | 16 | 4,188.700 | 281,483.700 | 24,133.900 | 10.9230× |
| 250,000 | 1,250 | 8 | 15,140.300 | 379,674.400 | 44,063.900 | 4.9696× |
| 250,000 | 1,250 | 16 | 11,454.800 | 733,430.100 | 76,195.100 | 9.0191× |
| 500,000 | 2,500 | 8 | 35,351.700 | 823,693.100 | 121,741.900 | 4.7614× |
| 500,000 | 2,500 | 16 | 34,325.500 | 1,592,962.100 | 174,626.200 | 8.0241× |
| 1,000,000 | 5,000 | 8 | 72,215.200 | 1,830,176.100 | 380,880.700 | 4.4953× |
| 1,000,000 | 5,000 | 16 | 76,160.700 | 4,621,439.700 | 836,813.500 | 10.0286× |

| N | k | SoA Vec capacities B | CSR Vec B | projection Vec B | membership Vec B | serial scratch retained B | parallel Phase 6B extra scratch B | sampled OS peak working set B |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100,000 | 8 | 4,700,000 | 4,400,008 | 7,200,008 | 800,000 | 18,008,792 | 28,266,480 | 85,704,704 |
| 100,000 | 16 | 4,700,000 | 7,600,008 | 13,600,008 | 800,000 | 18,008,824 | 28,252,464 | 97,243,136 |
| 250,000 | 8 | 11,750,000 | 11,000,008 | 18,000,008 | 2,000,000 | 45,019,300 | 70,478,936 | 205,148,160 |
| 250,000 | 16 | 11,750,000 | 19,000,008 | 34,000,008 | 2,000,000 | 45,019,332 | 70,489,384 | 229,572,608 |
| 500,000 | 8 | 23,500,000 | 22,000,008 | 36,000,008 | 4,000,000 | 90,060,024 | 138,694,080 | 394,526,720 |
| 500,000 | 16 | 23,500,000 | 38,000,008 | 68,000,008 | 4,000,000 | 90,060,056 | 141,009,184 | 443,191,296 |
| 1,000,000 | 8 | 47,000,000 | 44,000,008 | 72,000,008 | 8,000,000 | 180,136,864 | 262,661,520 | 776,093,696 |
| 1,000,000 | 16 | 47,000,000 | 76,000,008 | 136,000,008 | 8,000,000 | 180,136,896 | 268,331,856 | 872,611,840 |

SoA capacity bytes include all 12 authoritative columns. CSR and projection figures are retained Vec allocation bytes; scratch figures are retained context heap after warmup. The working-set probe ran outside timed samples.

## Whole-tick measurements

Each value is median ± MAD µs/tick over seven paired samples. Speedup is serial median divided by parallel median.

| N | K | k | checkpoint day | serial median ± MAD µs | parallel median ± MAD µs | parallel speedup |
|---:|---:|---:|---:|---:|---:|---:|
| 100,000 | 500 | 8 | 0 | 25,762.567 ± 833.300 | 26,787.100 ± 588.533 | 0.9618× |
| 100,000 | 500 | 8 | 50 | 27,230.033 ± 840.700 | 28,733.467 ± 439.067 | 0.9477× |
| 100,000 | 500 | 16 | 0 | 27,979.333 ± 620.567 | 28,311.067 ± 788.800 | 0.9883× |
| 100,000 | 500 | 16 | 50 | 30,806.033 ± 887.367 | 28,679.333 ± 968.933 | 1.0742× |
| 250,000 | 1,250 | 8 | 0 | 85,265.933 ± 1,099.400 | 84,485.067 ± 1,627.000 | 1.0092× |
| 250,000 | 1,250 | 8 | 50 | 92,075.967 ± 723.200 | 89,604.233 ± 416.000 | 1.0276× |
| 250,000 | 1,250 | 16 | 0 | 89,767.700 ± 1,045.167 | 87,480.100 ± 2,335.433 | 1.0261× |
| 250,000 | 1,250 | 16 | 50 | 93,876.867 ± 1,173.133 | 90,778.100 ± 1,838.900 | 1.0341× |
| 500,000 | 2,500 | 8 | 0 | 198,563.467 ± 4,042.067 | 194,257.167 ± 4,005.433 | 1.0222× |
| 500,000 | 2,500 | 8 | 50 | 208,374.333 ± 5,079.500 | 201,752.400 ± 3,893.467 | 1.0328× |
| 500,000 | 2,500 | 16 | 0 | 220,285.733 ± 2,236.667 | 210,299.767 ± 3,564.233 | 1.0475× |
| 500,000 | 2,500 | 16 | 50 | 229,782.767 ± 2,464.933 | 220,797.000 ± 1,498.400 | 1.0407× |
| 1,000,000 | 5,000 | 8 | 0 | 491,862.000 ± 2,744.733 | 470,940.933 ± 3,768.000 | 1.0444× |
| 1,000,000 | 5,000 | 8 | 50 | 525,062.533 ± 12,027.867 | 495,442.700 ± 2,757.400 | 1.0598× |
| 1,000,000 | 5,000 | 16 | 0 | 544,267.300 ± 9,625.867 | 655,973.867 ± 15,646.600 | 0.8297× |
| 1,000,000 | 5,000 | 16 | 50 | 526,061.433 ± 7,624.400 | 646,881.267 ± 3,866.100 | 0.8132× |

## Serial per-phase profile

Values are median ± MAD µs/day from the bench-only profile driver. `P6B_total` is the outer resolver wall interval; P6B substage timings are listed separately below. `P11_events` includes event conversion and flush, with P10 metrics timed separately.

| N | k | day | P1 | P2 | P3 | P4 select | P4 intents | P5 | P6A | P6B total | P7 | P8 | P9 | P10 | P11 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100,000 | 8 | 0 | 0.600 ± 0.100 | 84.000 ± 1.400 | 679.100 ± 129.000 | 4,466.000 ± 206.800 | 2,630.200 ± 230.900 | 1,650.100 ± 363.800 | 1,134.900 ± 63.200 | 5,277.900 ± 1,029.800 | 3,108.100 ± 483.000 | 1,505.200 ± 234.000 | 1,017.700 ± 177.700 | 2,866.500 ± 804.700 | 2,662.100 ± 231.800 |
| 100,000 | 8 | 50 | 0.500 ± 0.100 | 107.300 ± 10.800 | 835.600 ± 160.900 | 4,524.900 ± 245.000 | 3,152.400 ± 281.100 | 1,620.400 ± 93.800 | 1,392.500 ± 108.700 | 5,302.900 ± 569.000 | 3,038.000 ± 332.300 | 1,336.600 ± 223.300 | 1,056.200 ± 194.200 | 4,334.400 ± 401.400 | 2,729.500 ± 162.300 |
| 100,000 | 16 | 0 | 0.600 ± 0.100 | 105.400 ± 9.600 | 848.500 ± 32.500 | 4,953.500 ± 113.000 | 4,137.600 ± 315.400 | 1,544.900 ± 228.600 | 1,382.700 ± 127.700 | 5,248.600 ± 367.600 | 2,948.300 ± 339.700 | 1,301.700 ± 199.100 | 920.000 ± 77.200 | 2,852.300 ± 873.600 | 2,750.600 ± 112.100 |
| 100,000 | 16 | 50 | 0.600 ± 0.100 | 84.300 ± 3.500 | 748.100 ± 138.900 | 4,841.500 ± 188.400 | 4,142.800 ± 336.600 | 1,612.300 ± 148.900 | 1,374.400 ± 89.500 | 5,206.400 ± 649.000 | 3,492.900 ± 434.200 | 1,741.900 ± 29.600 | 1,234.900 ± 16.700 | 4,357.800 ± 212.200 | 2,675.500 ± 132.700 |
| 250,000 | 8 | 0 | 1.200 ± 0.100 | 217.000 ± 13.700 | 1,991.300 ± 399.400 | 11,518.200 ± 584.500 | 8,943.500 ± 760.400 | 3,004.700 ± 105.000 | 4,680.400 ± 580.900 | 15,881.200 ± 1,589.900 | 13,727.600 ± 553.700 | 5,208.400 ± 178.200 | 4,562.300 ± 121.700 | 8,373.800 ± 2,365.300 | 6,990.800 ± 368.600 |
| 250,000 | 8 | 50 | 1.300 ± 0.100 | 214.600 ± 9.300 | 2,031.600 ± 421.700 | 11,742.200 ± 680.700 | 9,617.700 ± 665.800 | 3,816.700 ± 276.200 | 5,026.800 ± 525.200 | 16,522.000 ± 1,303.500 | 13,308.700 ± 856.100 | 3,932.000 ± 247.100 | 3,837.400 ± 599.200 | 11,186.500 ± 1,290.300 | 7,206.800 ± 389.800 |
| 250,000 | 16 | 0 | 1.200 ± 0.100 | 260.400 ± 12.200 | 2,437.200 ± 302.200 | 12,247.400 ± 213.000 | 11,969.700 ± 405.800 | 3,135.400 ± 228.200 | 4,565.700 ± 588.700 | 16,070.600 ± 1,905.000 | 13,398.800 ± 1,106.700 | 5,206.800 ± 146.200 | 3,668.900 ± 450.200 | 8,315.800 ± 2,237.900 | 7,015.400 ± 363.200 |
| 250,000 | 16 | 50 | 1.300 ± 0.100 | 208.600 ± 4.300 | 1,908.700 ± 347.300 | 11,722.800 ± 641.100 | 12,129.500 ± 958.800 | 4,007.100 ± 150.500 | 5,008.000 ± 283.200 | 16,615.000 ± 974.500 | 14,030.000 ± 1,065.500 | 3,877.900 ± 201.500 | 3,469.700 ± 280.000 | 11,611.800 ± 1,252.200 | 7,223.600 ± 208.100 |
| 500,000 | 8 | 0 | 1.600 ± 0.100 | 465.100 ± 53.000 | 6,973.900 ± 462.700 | 22,875.600 ± 593.100 | 20,663.400 ± 887.500 | 7,960.400 ± 1,221.300 | 14,543.800 ± 902.000 | 37,407.700 ± 3,438.000 | 32,269.200 ± 1,242.300 | 10,507.700 ± 1,266.700 | 8,723.900 ± 1,545.500 | 17,986.100 ± 4,277.300 | 15,802.700 ± 895.900 |
| 500,000 | 8 | 50 | 1.700 ± 0.200 | 428.100 ± 21.600 | 6,824.900 ± 762.600 | 23,625.400 ± 626.900 | 20,172.900 ± 1,443.000 | 7,224.000 ± 770.500 | 14,643.100 ± 533.300 | 36,086.500 ± 2,567.500 | 32,093.700 ± 1,204.000 | 8,773.700 ± 687.700 | 7,359.900 ± 570.800 | 27,438.100 ± 1,895.500 | 16,298.800 ± 789.300 |
| 500,000 | 16 | 0 | 1.700 ± 0.100 | 540.800 ± 23.100 | 7,187.800 ± 549.600 | 24,647.300 ± 117.800 | 26,236.500 ± 440.000 | 8,454.600 ± 245.600 | 15,266.900 ± 411.100 | 38,269.700 ± 1,551.700 | 35,145.600 ± 1,257.700 | 11,629.300 ± 266.000 | 10,164.900 ± 361.500 | 23,765.600 ± 682.700 | 17,334.300 ± 299.600 |
| 500,000 | 16 | 50 | 1.700 ± 0.100 | 545.400 ± 14.700 | 6,857.500 ± 383.700 | 24,838.600 ± 273.000 | 26,250.900 ± 370.200 | 8,602.600 ± 155.700 | 15,127.200 ± 421.600 | 38,530.600 ± 2,072.300 | 35,039.600 ± 481.100 | 11,748.600 ± 399.300 | 9,988.900 ± 211.900 | 29,865.900 ± 919.100 | 17,160.200 ± 418.800 |
| 1,000,000 | 8 | 0 | 2.500 ± 0.100 | 1,067.300 ± 21.200 | 30,669.300 ± 342.100 | 49,272.000 ± 218.400 | 44,323.700 ± 678.800 | 17,892.300 ± 661.900 | 36,743.700 ± 279.700 | 85,130.300 ± 2,180.500 | 79,509.100 ± 967.700 | 24,712.500 ± 603.000 | 21,349.800 ± 452.000 | 51,456.600 ± 1,995.700 | 39,857.500 ± 395.500 |
| 1,000,000 | 8 | 50 | 2.600 ± 0.200 | 1,080.600 ± 26.800 | 30,406.300 ± 553.800 | 49,155.600 ± 311.800 | 45,916.600 ± 1,646.900 | 18,352.400 ± 351.900 | 36,411.200 ± 687.700 | 86,439.600 ± 3,543.300 | 81,153.700 ± 809.200 | 24,262.800 ± 587.000 | 21,640.000 ± 383.100 | 66,199.700 ± 1,371.300 | 40,049.600 ± 765.400 |
| 1,000,000 | 16 | 0 | 2.600 ± 0.100 | 1,120.200 ± 59.000 | 30,280.900 ± 900.200 | 49,807.200 ± 509.500 | 58,298.200 ± 2,309.600 | 18,811.900 ± 637.000 | 40,513.500 ± 1,419.500 | 109,231.200 ± 15,138.700 | 88,409.300 ± 4,508.400 | 26,046.700 ± 792.200 | 22,460.400 ± 918.100 | 53,225.200 ± 14,999.100 | 41,440.900 ± 1,141.800 |
| 1,000,000 | 16 | 50 | 2.600 ± 0.100 | 1,096.900 ± 35.700 | 30,767.100 ± 895.200 | 50,666.700 ± 1,577.200 | 58,319.200 ± 4,061.000 | 19,466.000 ± 1,447.700 | 41,483.500 ± 2,476.600 | 101,094.100 ± 10,561.800 | 97,470.000 ± 7,395.400 | 28,904.600 ± 3,240.600 | 22,689.700 ± 1,635.900 | 68,600.400 ± 4,039.500 | 42,061.300 ± 1,237.400 |

## Phase 6B serial substage profile

The serial resolver does not build dependency levels, so `dependency_level_build` is zero / not applicable in this profile.

| N | k | day | validation / slot µs | key generation µs | global sort µs | dependency level build µs | evaluation, writeback, output µs |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 100,000 | 8 | 0 | 1,217.400 ± 140.000 | 611.400 ± 217.700 | 1,047.600 ± 100.600 | 0 ± 0 | 2,354.000 ± 599.600 |
| 100,000 | 8 | 50 | 1,512.900 ± 217.900 | 611.400 ± 53.200 | 1,089.300 ± 44.200 | 0 ± 0 | 2,036.200 ± 295.700 |
| 100,000 | 16 | 0 | 1,284.100 ± 179.900 | 609.900 ± 191.300 | 1,025.000 ± 80.100 | 0 ± 0 | 2,000.900 ± 272.300 |
| 100,000 | 16 | 50 | 1,334.200 ± 229.200 | 608.800 ± 212.600 | 1,037.400 ± 88.800 | 0 ± 0 | 2,071.000 ± 311.000 |
| 250,000 | 8 | 0 | 5,180.500 ± 560.100 | 1,734.000 ± 565.900 | 2,733.800 ± 201.300 | 0 ± 0 | 5,897.500 ± 491.300 |
| 250,000 | 8 | 50 | 5,790.400 ± 716.400 | 1,846.500 ± 260.700 | 2,918.300 ± 266.900 | 0 ± 0 | 6,061.100 ± 674.100 |
| 250,000 | 16 | 0 | 5,034.600 ± 594.300 | 1,704.900 ± 339.900 | 2,751.500 ± 142.700 | 0 ± 0 | 5,855.000 ± 637.400 |
| 250,000 | 16 | 50 | 5,782.800 ± 416.500 | 1,761.500 ± 332.800 | 2,993.500 ± 178.300 | 0 ± 0 | 6,001.400 ± 370.000 |
| 500,000 | 8 | 0 | 14,074.800 ± 1,110.500 | 4,032.900 ± 614.800 | 5,980.900 ± 556.400 | 0 ± 0 | 12,600.500 ± 928.000 |
| 500,000 | 8 | 50 | 14,130.500 ± 1,116.300 | 3,706.600 ± 779.500 | 5,977.200 ± 400.600 | 0 ± 0 | 12,588.900 ± 1,370.700 |
| 500,000 | 16 | 0 | 15,140.300 ± 524.600 | 4,041.900 ± 598.500 | 6,310.600 ± 89.300 | 0 ± 0 | 12,762.500 ± 597.500 |
| 500,000 | 16 | 50 | 14,995.200 ± 657.600 | 4,062.700 ± 40.500 | 6,315.700 ± 109.100 | 0 ± 0 | 13,150.000 ± 783.700 |
| 1,000,000 | 8 | 0 | 36,303.500 ± 875.300 | 8,812.100 ± 545.300 | 12,501.200 ± 201.400 | 0 ± 0 | 27,634.500 ± 672.400 |
| 1,000,000 | 8 | 50 | 36,977.700 ± 1,140.200 | 8,803.600 ± 1,002.400 | 12,713.000 ± 113.100 | 0 ± 0 | 28,173.900 ± 1,808.400 |
| 1,000,000 | 16 | 0 | 42,621.800 ± 3,512.800 | 11,605.000 ± 2,448.400 | 13,058.000 ± 588.800 | 0 ± 0 | 36,736.100 ± 7,777.000 |
| 1,000,000 | 16 | 50 | 42,174.400 ± 2,402.200 | 9,302.900 ± 1,946.800 | 13,276.800 ± 278.800 | 0 ± 0 | 36,002.700 ± 5,624.400 |

## Scaling exponents

`alpha = log(T₂/T₁) / log(N₂/N₁)`. Phase 6B dependency-level construction is not applicable to the serial resolver. Rows contain every measured interval for each k/checkpoint profile.

| k | checkpoint day | phase | 100k→250k | 250k→500k | 500k→1M |
|---:|---:|---|---:|---:|---:|
| 8 | 0 | full tick | 1.30619 | 1.21956 | 1.30865 |
| 8 | 0 | P1 regrowth | 0.75647 | 0.41504 | 0.64386 |
| 8 | 0 | P10 metrics | 1.16995 | 1.10293 | 1.51647 |
| 8 | 0 | P11 events | 1.05368 | 1.17664 | 1.33468 |
| 8 | 0 | P2 degradation | 1.03579 | 1.09985 | 1.19835 |
| 8 | 0 | P3 features | 1.17405 | 1.80826 | 2.13676 |
| 8 | 0 | P4 action selection | 1.03399 | 0.98989 | 1.10696 |
| 8 | 0 | P4 intent/projection | 1.33568 | 1.20817 | 1.10100 |
| 8 | 0 | P5 partition | 0.65410 | 1.40562 | 1.16843 |
| 8 | 0 | P6A work resolution | 1.54628 | 1.63570 | 1.33709 |
| 8 | 0 | P6B validation/slot | 1.58049 | 1.44195 | 1.36699 |
| 8 | 0 | P6B key generation | 1.13767 | 1.21771 | 1.12767 |
| 8 | 0 | P6B sort | 1.04682 | 1.12945 | 1.06363 |
| 8 | 0 | P6B evaluation/writeback/output | 1.00232 | 1.09531 | 1.13299 |
| 8 | 0 | P6B total | 1.20225 | 1.23602 | 1.18634 |
| 8 | 0 | P7 market | 1.62110 | 1.23308 | 1.30096 |
| 8 | 0 | P8 welfare | 1.35475 | 1.01253 | 1.23379 |
| 8 | 0 | P9 mortality | 1.63734 | 0.93521 | 1.29118 |
| 8 | 50 | full tick | 1.32959 | 1.17828 | 1.33331 |
| 8 | 50 | P1 regrowth | 1.04280 | 0.38702 | 0.61298 |
| 8 | 50 | P10 metrics | 1.03474 | 1.29442 | 1.27064 |
| 8 | 50 | P11 events | 1.05961 | 1.17734 | 1.29702 |
| 8 | 50 | P2 degradation | 0.75647 | 0.99630 | 1.33581 |
| 8 | 50 | P3 features | 0.96959 | 1.74819 | 2.15549 |
| 8 | 50 | P4 action selection | 1.04071 | 1.00864 | 1.05702 |
| 8 | 50 | P4 intent/projection | 1.21734 | 1.06865 | 1.18660 |
| 8 | 50 | P5 partition | 0.93498 | 0.92047 | 1.34510 |
| 8 | 50 | P6A work resolution | 1.40096 | 1.54251 | 1.31416 |
| 8 | 50 | P6B validation/slot | 1.46479 | 1.28708 | 1.38784 |
| 8 | 50 | P6B key generation | 1.20627 | 1.00530 | 1.24800 |
| 8 | 50 | P6B sort | 1.07549 | 1.03434 | 1.08876 |
| 8 | 50 | P6B evaluation/writeback/output | 1.19046 | 1.05450 | 1.16221 |
| 8 | 50 | P6B total | 1.24026 | 1.12707 | 1.26023 |
| 8 | 50 | P7 market | 1.61217 | 1.26992 | 1.33837 |
| 8 | 50 | P8 welfare | 1.17759 | 1.15792 | 1.46749 |
| 8 | 50 | P9 mortality | 1.40798 | 0.93956 | 1.55594 |
| 16 | 0 | full tick | 1.27226 | 1.29511 | 1.30494 |
| 16 | 0 | P1 regrowth | 0.75647 | 0.50250 | 0.61298 |
| 16 | 0 | P10 metrics | 1.16779 | 1.51495 | 1.16323 |
| 16 | 0 | P11 events | 1.02182 | 1.30503 | 1.25743 |
| 16 | 0 | P2 degradation | 0.98708 | 1.05437 | 1.05059 |
| 16 | 0 | P3 features | 1.15153 | 1.56033 | 2.07479 |
| 16 | 0 | P4 action selection | 0.98792 | 1.00895 | 1.01492 |
| 16 | 0 | P4 intent/projection | 1.15931 | 1.13219 | 1.15188 |
| 16 | 0 | P5 partition | 0.77246 | 1.43109 | 1.15384 |
| 16 | 0 | P6A work resolution | 1.30366 | 1.74150 | 1.40800 |
| 16 | 0 | P6B validation/slot | 1.49109 | 1.58844 | 1.49320 |
| 16 | 0 | P6B key generation | 1.12188 | 1.24535 | 1.52164 |
| 16 | 0 | P6B sort | 1.07766 | 1.19756 | 1.04908 |
| 16 | 0 | P6B evaluation/writeback/output | 1.17179 | 1.12417 | 1.52529 |
| 16 | 0 | P6B total | 1.22126 | 1.25178 | 1.51311 |
| 16 | 0 | P7 market | 1.65224 | 1.39124 | 1.33085 |
| 16 | 0 | P8 welfare | 1.51294 | 1.15930 | 1.16334 |
| 16 | 0 | P9 mortality | 1.50964 | 1.47018 | 1.14379 |
| 16 | 50 | full tick | 1.21607 | 1.29143 | 1.19496 |
| 16 | 50 | P1 regrowth | 0.84383 | 0.38702 | 0.61298 |
| 16 | 50 | P10 metrics | 1.06959 | 1.36291 | 1.19972 |
| 16 | 50 | P11 events | 1.08395 | 1.24828 | 1.29343 |
| 16 | 50 | P2 degradation | 0.98881 | 1.38658 | 1.00805 |
| 16 | 50 | P3 features | 1.02221 | 1.84509 | 2.16563 |
| 16 | 50 | P4 action selection | 0.96510 | 1.08327 | 1.02845 |
| 16 | 50 | P4 intent/projection | 1.17241 | 1.11385 | 1.15160 |
| 16 | 50 | P5 partition | 0.99358 | 1.10221 | 1.17811 |
| 16 | 50 | P6A work resolution | 1.41115 | 1.59484 | 1.45539 |
| 16 | 50 | P6B validation/slot | 1.60054 | 1.37466 | 1.49187 |
| 16 | 50 | P6B key generation | 1.15949 | 1.20563 | 1.19524 |
| 16 | 50 | P6B sort | 1.15654 | 1.07711 | 1.07189 |
| 16 | 50 | P6B evaluation/writeback/output | 1.16116 | 1.13169 | 1.45304 |
| 16 | 50 | P6B total | 1.26643 | 1.21352 | 1.39162 |
| 16 | 50 | P7 market | 1.51749 | 1.32047 | 1.47597 |
| 16 | 50 | P8 welfare | 0.87343 | 1.59914 | 1.29881 |
| 16 | 50 | P9 mortality | 1.12746 | 1.52551 | 1.18364 |

P6B validation includes structural validation and slot lookup; its timing does not isolate those two operations further.

## First-break findings

| Criterion | First measured trigger | Other trigger details |
|---|---|---|
| Any phase alpha `>1.15` | `100k→250k` (`N=250k`) in all four `(k, checkpoint)` profiles; full-tick alpha is `1.21607`–`1.32959` | Per-phase and per-stage exceedances for every interval are listed below |
| Serial per-agent tick time `>25%` above the matching 100k baseline | `N=250k` for `k=8` at day 0/day 50 and `k=16` at day 0 | `k=16`, day 50 first crosses at `N=500k` |
| Graph + projection `>10` tick equivalents | `N=100k, k=16`: `10.9230×` | `N=1M, k=16`: `10.0286×`; other cells stay below 10× |
| Memory `>50%` of host RAM | None | Largest sampled working set `872,611,840` B; capacity-based parallel total `715,468,768` B |
| Runtime, overflow, or capacity error | None | All 16 cells completed; largest N was 1,000,000 |

The following list gives every phase or P6B stage exceeding the alpha threshold for each interval. `P6B_dependency_level_build` is not applicable to this serial profile.

| k | checkpoint | interval | phases/stages with alpha `>1.15` (alpha) |
|---:|---:|---:|---|
| 8 | 0 | 100k→250k | full tick (1.30619), P3 (1.17405), P4 intent (1.33568), P6A (1.54628), P6B total (1.20225), P7 (1.62110), P8 (1.35475), P9 (1.63734), P10 (1.16995), P6B validation/slot (1.58049) |
| 8 | 0 | 250k→500k | full tick (1.21956), P3 (1.80826), P4 intent (1.20817), P5 (1.40562), P6A (1.63570), P6B total (1.23602), P7 (1.23308), P11 (1.17664), P6B validation/slot (1.44195), P6B key (1.21771) |
| 8 | 0 | 500k→1M | full tick (1.30865), P2 (1.19835), P3 (2.13676), P5 (1.16843), P6A (1.33709), P6B total (1.18634), P7 (1.30096), P8 (1.23379), P9 (1.29118), P10 (1.51647), P11 (1.33468), P6B validation/slot (1.36699) |
| 8 | 50 | 100k→250k | full tick (1.32959), P4 intent (1.21734), P6A (1.40096), P6B total (1.24026), P7 (1.61217), P8 (1.17759), P9 (1.40798), P6B validation/slot (1.46479), P6B key (1.20627), P6B evaluation (1.19046) |
| 8 | 50 | 250k→500k | full tick (1.17828), P3 (1.74819), P6A (1.54251), P7 (1.26992), P8 (1.15792), P10 (1.29442), P11 (1.17734), P6B validation/slot (1.28708) |
| 8 | 50 | 500k→1M | full tick (1.33331), P2 (1.33581), P3 (2.15549), P4 intent (1.18660), P5 (1.34510), P6A (1.31416), P6B total (1.26023), P7 (1.33837), P8 (1.46749), P9 (1.55594), P10 (1.27064), P11 (1.29702), P6B validation/slot (1.38784), P6B key (1.24800), P6B evaluation (1.16221) |
| 16 | 0 | 100k→250k | full tick (1.27226), P3 (1.15153), P4 intent (1.15931), P6A (1.30366), P6B total (1.22126), P7 (1.65224), P8 (1.51294), P9 (1.50964), P10 (1.16779), P6B validation/slot (1.49109), P6B evaluation (1.17179) |
| 16 | 0 | 250k→500k | full tick (1.29511), P3 (1.56033), P5 (1.43109), P6A (1.74150), P6B total (1.25178), P7 (1.39124), P8 (1.15930), P9 (1.47018), P10 (1.51495), P11 (1.30503), P6B validation/slot (1.58844), P6B key (1.24535), P6B sort (1.19756) |
| 16 | 0 | 500k→1M | full tick (1.30494), P3 (2.07479), P4 intent (1.15188), P5 (1.15384), P6A (1.40800), P6B total (1.51311), P7 (1.33085), P8 (1.16334), P10 (1.16323), P11 (1.25743), P6B validation/slot (1.49320), P6B key (1.52164), P6B evaluation (1.52529) |
| 16 | 50 | 100k→250k | full tick (1.21607), P4 intent (1.17241), P6A (1.41115), P6B total (1.26643), P7 (1.51749), P6B validation/slot (1.60054), P6B key (1.15949), P6B sort (1.15654), P6B evaluation (1.16116) |
| 16 | 50 | 250k→500k | full tick (1.29143), P2 (1.38658), P3 (1.84509), P6A (1.59484), P6B total (1.21352), P7 (1.32047), P8 (1.59914), P9 (1.52551), P10 (1.36291), P11 (1.24828), P6B validation/slot (1.37466), P6B key (1.20563) |
| 16 | 50 | 500k→1M | full tick (1.19496), P3 (2.16563), P4 intent (1.15160), P5 (1.17811), P6A (1.45539), P6B total (1.39162), P7 (1.47597), P8 (1.29881), P9 (1.18364), P10 (1.19972), P11 (1.29343), P6B validation/slot (1.49187), P6B key (1.19524), P6B evaluation (1.45304) |

## Serial versus parallel digest parity

Each row is the final digest after the three-day paired sample. All rows match between the production serial and explicit parallel runners.

| N | k | end day | State | Metrics | Events |
|---:|---:|---:|---|---|---|
| 100,000 | 8 | 3 | `c5278ddff6263af4b5acfca986997c5d407812604784d138b6e5e2e42124042a` | `1400c0829a0c5e247d2c4e7e649d6074b175a48d189b431f165b0eaa0b29238b` | `7916ced0d1961a255b2766dcb0013f2f9b615e62351775931d9c9466a1967e31` |
| 100,000 | 8 | 53 | `15cbeb6d00d03709a9e4a9766c5484331470fb6d9f6dd509f700457b683cb0ea` | `62bc11337698bdb23d15deb6cf90257455f867ae7b7c842f650816111993d3a9` | `26d1e12c83954091c92687032844858fe9dab58286d621ac17e2a1bb3959e861` |
| 100,000 | 16 | 3 | `3bb45c7717c51d4def54556565a6ee8448abb8ff843561fbf1dc54600a2f2b0a` | `6e4ea6a0b5d665f45fe688275c8f3d77349cfed21fe4ed8555eaba5b4aeff0c2` | `d5c93f184d1fb156855088ad989e0f2005959650c2a758dcd6eed029feff5aa5` |
| 100,000 | 16 | 53 | `daad18ac07f04284f2ef051e1cce9c64f4b24ab5c55a534edc355394000c72dc` | `e3539ac261f511eb049338317587a991358a9520bdbbcbd64dd3e4ec9491cbc9` | `1f7c37e245743deab40997c20415f3326102d11ee50849096bda9724b5a3cfa2` |
| 250,000 | 8 | 3 | `0da1861ef5fc11cc0b1076a6bfb9b3aecaa597177e896b6e00275c4d809b906a` | `52782709686e568d426903cfc73c270c560a51f2a353eadc6a62d1d40e69b1ea` | `0452f8092dccfeb8ddbbcb5133a002b6c8ca6ed5cba9f298bf426e1ba082d045` |
| 250,000 | 8 | 53 | `315a9c3ff51b4d64757310261eb2c3a8fe4c770bd20bafbd277fb008a118aedd` | `507e76d7e4938667bfaa3c757049859db677db968d9b7c904ae9491ead9de07b` | `c7d3dffdef44b84f9da0d067268c5c66ac98051a069cbfd82e4bc33f64a2e577` |
| 250,000 | 16 | 3 | `4226b879c665b713f3c3c438556b1943d3c467bef7314e8f24d02a534403d8a6` | `cf7dfbe7b706438adc6b77470e826786c3f5a8f1ccef6044fd23aa8fe9261d7f` | `ab9b949bb4a69465f395404fada1127c084cbec8e71aeabc81cdd7070c8d3416` |
| 250,000 | 16 | 53 | `45dba46e0bbdbc2da5b1649837a121ae128b35cf095d40f7afa01aa1d992d536` | `7ed677f3c321bbbd58856623743ac05238c7b044168af75e8b939d54a40173a2` | `05e2b95f238a6cb1aad248ef857a5f0d38a2d5450fb47864f39e323814c8a05c` |
| 500,000 | 8 | 3 | `d242221ec71f68d234873735172ca53f360d780385b8e321f7022e4d72e9761f` | `6ea8367c2f1f82e11bedd57aec63b36b5b6c2c2d3b88309a6ef5d38988f74378` | `a135dc13213536a27d378bcf62a6b1ca468bb5aa5447192305175adfbcf40ffa` |
| 500,000 | 8 | 53 | `314dc91ac4f842d167df619ed0f21a96fb4dd5964da3254ab79864a53e33978d` | `ff1e63eee0a850eef5fa613788480dd0c66d84dcb24cd4f8f173838b2de7cc5c` | `992c4d2d935d9b6d4df10eaba2cf3752cc7c27505b50f21318bbf8aa954c6504` |
| 500,000 | 16 | 3 | `7badcd8e34d745e5e010400c2ef0ec649ce48d4bb223b030ff3898869aa2bcf4` | `9c1d2bdcbd1253197b01eaf6a614e629bef3366d0153312a53321ae74d6917fc` | `733348c8faba8eca2589f951321e342e47724fe53c6eb71b97902f2a032a2759` |
| 500,000 | 16 | 53 | `67b8911c33092ead46746a5d917260f702b4ec89a58d94c1bf8c6e819c6ddaa7` | `96ae6c0ee917ed3982ec1b476c35454a682fbc164590653643f953069e9bb651` | `8e896c41cd6fbd8715b47e47a8d8594a54acc679f981c8d9bfe640587b1be0e1` |
| 1,000,000 | 8 | 3 | `735fe03c46bdf0e53933c9d3cf8c5e448021b3d41f90c4f20afca05c0d4c9095` | `590d9e98fd48dd59f3f68b6c3bd37c663a77f310ee99661a29cd2053058075a9` | `ec596d76362ffdcf71a3061e33a0c8fc1323120fdc2a4a89d91f03ed190ffed4` |
| 1,000,000 | 8 | 53 | `34252f6dad8a84a08f05d11b711f0c9ad61b1acc7d943beda103a15724987965` | `62707070e25ad662d379cd1fddf637584b0fc6fa534cd72131a61d373f05f424` | `cad3b6569e9f489065e8b9ec8842b5de79b89761ee13cd0ca581571261a88c09` |
| 1,000,000 | 16 | 3 | `197ba1e5461cffcb1f8859da96c5a8f2ec2e0fb47ceadb485bcb582e1d13e398` | `f49bebbd17f56bb910711ed58c75a551616e63f0a7bd46a4070ead1cc16e9fa0` | `87672a437638ce0558fc03ad6da659eaa1359c7ffef037cc6c8a9f0ee47573b1` |
| 1,000,000 | 16 | 53 | `6a918fc81bfe0ff5db89e415d81ea3ce4f6bffc9e6b643184435d46230a7d322` | `71fd1c54d2ed2b97f5aac515385aa85bebef9b68826f9abd01de7040034e6995` | `ca6b29f6ff31d3d1d6a34f917de2787bf393abc1a71c3e42d07c575dac36cff9` |

## Interpretation and limitations

**Measured facts:** the full-tick and phase-alpha gates already flag the `100k→250k` interval; serial per-agent time continues to rise at 500k and 1M. At 1M, `P3_features` alpha is `2.07479`–`2.16563` over `500k→1M`, and P6B validation/slot lookup reaches `1.49187`. Memory remains far below the host threshold. Parallel results vary by cell: they are modestly faster in most cells, but slower at `N=1M, k=16` on both checkpoints.

**Inference:** if choosing a first architectural investigation from this profile, the strongest candidates are P3's feature-generation path and P6B's validation/slot stage. `phase3_observation_and_features_storage_with_scratch` scans agents and sorts the resulting feature records by AgentId; the P3 timer does not isolate that sort from feature arithmetic. The serial M3 Phase 6B resolver also checks settlement membership with a linear `settlements.iter().any(...)` for each partition; the validation timer combines that work with slot lookup. The observed alphas and code structure make these plausible contributors, not proven causes. A finer subphase profile is needed before choosing a redesign.

The results are from one host and one equilibrium configuration. Per-phase timings use a manual bench driver validated against production outputs; timing-hook instrumentation is used for Phase 6B substages. The benchmark allocator counter is inactive during timed samples but its disabled branch remains in the benchmark executable. Day 50 is only a checkpoint for a 3-day sample, not a long-horizon population-stability claim. This step changed no simulation, test, golden, snapshot, contract, or CI source.
