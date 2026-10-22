# M4-03 Divergence Measurement
Status: current

M4-03 implementation, the §12.6 performance gate, and the A-1 corrected report-only experiment rerun are complete. The original gate run remains as a protocol deviation; the confirmatory run with High priority and affinity `0x555` passed all three cells.


## Original performance gate (protocol deviation; superseded)

The gate compared `run_counterfactual_branch_with_divergence_series` against the M4-02 `run_counterfactual_branch` API. Each pair used the same day-0 snapshot and a no-op `BaseMetabolicCost` override, serial execution, `H = 30`, metrics and events enabled, and snapshots disabled. The fixtures were the M3P sustained profile for M3P cells and the M3 propagation profile for the M3+propagation cell; seed `81985529216486895`, replicate `7`, `k = 8`, `p_local = 0.5`, and `K = N/200`.

All cells ran in one release process. Each used two warmups and 41 paired timed samples. Warmup selected `r = 1` for every cell; each timed sample measured one M4-02 call and one M4-03 call, alternating path order. Deltas are `M4-03 series − M4-02 full API`. The bootstrap used the fixed §11.7 A-3 seed and 10,000 resamples. No retry was needed. The report includes per-operation medians and MADs in microseconds.

Host: AMD Ryzen 5 9600X, 6 cores / 12 logical processors, Windows 11 Pro. High priority and affinity were not applied. Total CPU load/free RAM were 2% / 74,932 MB before and 0% / 75,026 MB after. These values are informational under A-5. Timed cell wall time summed to 1,176.1 seconds; release compilation and the untimed initial parity check are excluded. This run is retained as a protocol deviation because the requested High priority and affinity controls were omitted; it is superseded by the confirmatory run below.

| Cell | `r` | M4-02 median ± MAD (µs) | M4-03 median ± MAD (µs) | Δ median ± MAD (µs) | U (µs) | T (µs) | M4-02 / M4-03 path MAD/median | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial, N=10,000 | 1 | 623,848.6 ± 23,172.5 | 628,807.8 ± 25,365.4 | 11,927.7 ± 23,071.7 | 25,522.1 | 31,192.4 | 3.714% / 4.034% | **PASS** |
| M3P serial, N=100,000 | 1 | 9,153,254.6 ± 109,429.2 | 9,180,285.3 ± 84,082.5 | 38,610.5 ± 127,941.0 | 78,466.9 | 457,662.7 | 1.196% / 0.916% | **PASS** |
| M3 + propagation serial, N=100,000 | 1 | 4,537,758.9 ± 26,111.4 | 4,550,553.6 ± 26,240.7 | 4,463.6 ± 47,880.5 | 11,250.6 | 226,887.9 | 0.575% / 0.577% | **PASS** |

For all three cells, both path spreads were at most 5%, and bootstrap `U <= T`. The API returned bit-identical M4-02 `BranchResult` values in the untimed parity checks.

## Confirmatory performance gate (High priority and affinity)

The §12.6 cells were rerun after the normalization amendment. The release harness compared the same M4-03 series API against the M4-02 full API, with two warmups, 41 paired samples, alternating path order, and A-5's fixed repetition count selected once per cell to make a sample at least one second. `r` remained fixed for all samples. Each cell passed an untimed exact `BranchResult` equality check. `U` uses the frozen deterministic 10,000-resample bootstrap; `T = 5%` of the M4-02 median.

Host: AMD Ryzen 5 9600X, 6 cores / 12 logical processors, Windows 11 Pro. The measurement process was confirmed at **High** priority with processor affinity mask `0x555` (decimal 1365). Total CPU load was 6% before and 0% after; free physical memory was 76,847,328 KB before and 76,832,552 KB after. Timed-run wall time was 22 minutes 55 seconds, including the initial parity checks and warmups; release compilation was excluded.

| Cell | `r` | M4-02 median ± MAD (µs) | M4-03 median ± MAD (µs) | Δ median ± MAD (µs) | U (µs) | T (µs) | Path MAD/median | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial, N=10,000 | 2 | 606,857.950 ± 15,908.450 | 613,662.300 ± 9,359.650 | 8,479.100 ± 16,267.500 | 17,450.100 | 30,342.897 | 2.621% / 1.525% | **PASS** |
| M3P serial, N=100,000 | 1 | 8,970,325.600 ± 106,223.500 | 8,992,494.800 ± 84,147.600 | 48,004.400 ± 126,632.600 | 99,662.800 | 448,516.280 | 1.184% / 0.936% | **PASS** |
| M3 + propagation serial, N=100,000 | 1 | 5,008,457.000 ± 35,650.100 | 5,026,765.000 ± 38,417.000 | 12,759.800 ± 56,103.200 | 24,353.500 | 250,422.850 | 0.712% / 0.764% | **PASS** |

All three cells satisfy A-5 path-spread validity and `U <= T`. M4-02 `BranchResult` equality held for the untimed parity checks.

## First divergence experiments (historical; superseded for A-1 statistics)

The table below preserves the original experiment report. Its alive-only normalized fractions and cascade classifications are superseded by the A-1 corrected rerun at the end of this document; raw divergence counts and the measured run setup are retained as history.

The report-only experiment used the M3P sustained fixture at `N = 10,000`, `K = 50`, `k = 8`, `p_local = 0.5`, replicate `7`, and `H = 90`. Seeds were `{0, 1, 3, 7, 81985529216486895}`. For each seed, the target was the lowest live `AgentId` in the largest settlement; ties chose the lowest `GroupId`. This selected `AgentId(0)` in `GroupId(0)` for all five runs. Each case compared its M4-03 branch result with a separately run M4-02 branch result; all 20 comparisons were bit-identical.

| Intervention | First divergence (day / slot / component) | Cascade onset and plateau | Doubling time (days) | Peak `|D|` | Peak fraction | Days to peak | Target death / onset relation | Newborn CRN loss |
|---|---|---|---:|---:|---:|---:|---|---:|
| Set target food to 0 | `0 / InterventionBoundary / agent_rows` (5/5) | onset median 5 (range 1–12); plateau median 5 (1–12) | median 4.17 (2.70–4.73) | median 526 (495–892) | median 0.0521 (0.0491–0.0884) | median 88 (80–89) | no target death in 5/5 | 0 in 5/5 |
| Set target health to 0.01 | `0 / InterventionBoundary / agent_rows` (5/5) | onset median 9 (range 3–10); plateau median 9 (3–10) | median 4.15 (4.11–5.56) | median 819 (428–891) | median 0.0812 (0.0424–0.0883) | 89 in 5/5 | no target death in 5/5 | 0 in 5/5 |
| Force target Idle on day 0 | `0 / Core(Phase6A) / agent_rows` (3/5); no divergence (2/5) | onset in 1/5 at day 7, plateau 7; no cascade in 4/5 | 4.74 for the one cascade | median 1 (0–286) | median 0.000100 (0–0.02834) | available 3/5: median 0, range 0–89 | no target death in the horizon | 0 in 5/5 |
| Increase BaseMetabolicCost by 1% | `0 / Core(Phase2) / agent_rows` (5/5) | no cascade in 5/5 | absent | median 10,093 (10,086–10,096) | median 1.00030 (1.00000–1.00040) | median 88 (88–89) | not applicable to a config intervention | 0 in 5/5 |

The target had no `MortalityCommitted` event in any of the 20 runs. The first-divergence slots, component names, and onset/death relation are taken from the API records and the Phase 9 event stream; absence of a target-death event is not evidence about what would happen after day 90.

Under the pre-A-1 alive-only denominator, the historical table could show peak fractions above `1.0`; §12 A-1 replaces that denominator with matched entities having a record on either side.

### Component flags and alive population

Flags below summarize the union of days with a change across the five seeds. They are reported independently from `D_d`.

| Intervention | `settlement_global_changed` | `graph_changed` | `allocator_changed` | `propagation_changed` |
|---|---|---|---|---|
| Food = 0 | days 0–89 | none | none | none |
| Health = 0.01 | days 0–89 | none | none | none |
| Forced Idle | union of days 0–89 across the 3 runs that diverged | none | none | none |
| BaseMetabolicCost +1% | days 0–89 | days 13–89 | none | none |

Alive population stayed between 10,000 and 10,095 across all base trajectories. Branch populations for the three local interventions also stayed in that band. With the configuration override, branch population ranged from 10,000 to 10,094. On the final executed day (day 89), base population ranged from 10,084 to 10,095 in every case; branch population ranged from 10,084 to 10,095 for the local cases and 10,081 to 10,094 for the configuration case. No target died and no run lost its entire population.

### Findings

- **Fact:** a one-agent intervention did not always produce a cascade. Food and health interventions crossed the frozen onset threshold in all five seeds. Forced Idle crossed it in one seed, produced a small `|D|` without a cascade in two seeds, and had no divergence in two seeds.
- **Fact:** none of the target agents died during the 90-day horizon. The onset-versus-death comparison is therefore unavailable for these cases; this sample does not show whether cascade onset follows target death.
- **Fact:** the global configuration change first differed at Phase 2 and had a peak divergence fraction slightly above `1.0`, while none of its five runs met the relative cascade threshold. Food and health changes began with one changed agent record at the intervention boundary and later crossed the threshold.
- **Inference:** under this workload, the cascade rule describes growth relative to an already observed divergence maximum. A broad configuration shift can therefore remain “no cascade” even when most agent records eventually differ. Local food and health changes spread under these seeds, but the sample does not establish a general causal rule.
- **Fact:** newborn CRN loss was zero in all cases. No matched births received different AgentIds in this experiment.

## Test time

The new `m4_03_divergence_tests` integration target took 12.44 seconds in debug. The new synthetic cascade unit tests completed in less than 0.01 seconds. No existing test, golden, snapshot, or CI file changed; the new divergence-series golden is in `crates/sim-model/tests/fixtures/m4_03_divergence_golden.txt`.

## A-1 corrected experiment rerun

The 20 original report-only H=90 runs were recomputed after A-1 using the same M3P sustained fixture, `N=10,000`, `K=50`, `k=8`, `p_local=0.5`, replicate `7`, seeds `{0, 1, 3, 7, 81985529216486895}`, and target `AgentId(0)` in `GroupId(0)`. The target is the lowest live AgentId in the largest settlement. Ten additional runs extend food=0 and health=0.01 to H=365 on those same seeds. Every series run asserted exact equality with a separate M4-02 `run_counterfactual_branch` result.

The table gives medians and ranges across five seeds. `L` is now record-based, including retained-dead records; the old alive-only peak fractions and classifications shown in the historical section above are superseded.

| Intervention | First divergence | Classification | Cascade onset / plateau (days) | Peak `|D|` | Peak fraction | Days to peak | Target death | Newborn CRN loss |
|---|---|---|---:|---:|---:|---:|---|---:|
| Food = 0 | `0 / InterventionBoundary / agent_rows (5/5)` | Cascade 5/5 | 5 (1–12) / 5 (1–12) | 526 (495–892) | 0.0521 (0.0491–0.0883) | 88 (80–89) | none (5/5) | 0 |
| Health = 0.01 | `0 / InterventionBoundary / agent_rows (5/5)` | Cascade 5/5 | 9 (3–10) / 9 (3–10) | 819 (428–891) | 0.0811 (0.0424–0.0883) | 89 (89–89) | none (5/5) | 0 |
| Force Idle on day 0 | `0 / Core(Phase6A) / agent_rows (3/5); no divergence (2/5)` | Cascade 1/5; NoCascade 2/5; NoDivergence 2/5 | 7 (7–7) / 7 (7–7) | 1 (0–286) | 0.0001 (0.0000–0.0283) | 0 (0–89) | none (5/5) | 0 |
| BaseMetabolicCost +1% | `0 / Core(Phase2) / agent_rows (5/5)` | ImmediateGlobalDivergence 5/5 | not evaluated (immediate global) | 10093 (10086–10096) | 0.9998 (0.9994–0.9999) | 88 (88–89) | none (5/5) | 0 |

All normalized fractions in this corrected sample are at most `1.0`. The global +1% configuration intervention is immediate global divergence on day 0 for all five seeds; cascade onset is therefore not evaluated. The old “peak fraction > 1” values were a denominator artifact, not new simulation output.

### Daily settlement spread in the 90-day runs

For each day, the table reports the median across five seeds and the seed range for changed records in the target settlement (`GroupId 0`) and all other settlements combined; the touched count is the min–max number of settlements with at least one changed record. These counts partition `|D_d|` for the food and health interventions.

| Day | Food target settlement | Food other settlements | Food touched | Health target settlement | Health other settlements | Health touched |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1.0 (1.0–3.0) | 0.0 (0.0–2.0) | 1–3 | 1.0 (1.0–1.0) | 0.0 (0.0–0.0) | 1–1 |
| 1 | 1.0 (1.0–26.0) | 1.0 (0.0–2.0) | 1–3 | 1.0 (1.0–1.0) | 0.0 (0.0–0.0) | 1–1 |
| 2 | 3.0 (1.0–49.0) | 1.0 (0.0–2.0) | 1–3 | 1.0 (1.0–1.0) | 0.0 (0.0–0.0) | 1–1 |
| 3 | 3.0 (1.0–49.0) | 1.0 (0.0–2.0) | 1–3 | 1.0 (1.0–26.0) | 0.0 (0.0–0.0) | 1–1 |
| 4 | 3.0 (1.0–70.0) | 1.0 (0.0–2.0) | 1–3 | 1.0 (1.0–26.0) | 0.0 (0.0–0.0) | 1–1 |
| 5 | 3.0 (1.0–84.0) | 1.0 (1.0–62.0) | 2–3 | 1.0 (1.0–26.0) | 0.0 (0.0–0.0) | 1–1 |
| 6 | 3.0 (1.0–94.0) | 1.0 (1.0–85.0) | 2–3 | 1.0 (1.0–44.0) | 0.0 (0.0–0.0) | 1–1 |
| 7 | 3.0 (1.0–109.0) | 1.0 (1.0–95.0) | 2–3 | 1.0 (1.0–64.0) | 0.0 (0.0–0.0) | 1–1 |
| 8 | 3.0 (1.0–121.0) | 1.0 (1.0–117.0) | 2–3 | 1.0 (1.0–71.0) | 0.0 (0.0–0.0) | 1–1 |
| 9 | 3.0 (1.0–128.0) | 1.0 (1.0–147.0) | 2–3 | 23.0 (1.0–71.0) | 0.0 (0.0–0.0) | 1–1 |
| 10 | 30.0 (1.0–133.0) | 2.0 (1.0–175.0) | 2–3 | 44.0 (21.0–82.0) | 0.0 (0.0–0.0) | 1–1 |
| 11 | 43.0 (1.0–141.0) | 27.0 (1.0–178.0) | 2–3 | 57.0 (40.0–93.0) | 0.0 (0.0–0.0) | 1–1 |
| 12 | 61.0 (16.0–142.0) | 44.0 (1.0–200.0) | 2–3 | 73.0 (54.0–100.0) | 0.0 (0.0–0.0) | 1–1 |
| 13 | 78.0 (36.0–143.0) | 56.0 (1.0–214.0) | 2–3 | 90.0 (69.0–109.0) | 0.0 (0.0–0.0) | 1–1 |
| 14 | 92.0 (58.0–151.0) | 62.0 (1.0–225.0) | 2–3 | 103.0 (78.0–113.0) | 0.0 (0.0–0.0) | 1–1 |
| 15 | 97.0 (68.0–153.0) | 98.0 (1.0–233.0) | 2–3 | 107.0 (88.0–123.0) | 0.0 (0.0–0.0) | 1–1 |
| 16 | 109.0 (78.0–155.0) | 108.0 (1.0–248.0) | 2–3 | 117.0 (98.0–130.0) | 0.0 (0.0–0.0) | 1–1 |
| 17 | 118.0 (89.0–157.0) | 117.0 (1.0–258.0) | 2–3 | 121.0 (104.0–134.0) | 0.0 (0.0–0.0) | 1–1 |
| 18 | 123.0 (97.0–160.0) | 123.0 (1.0–269.0) | 2–3 | 122.0 (108.0–134.0) | 0.0 (0.0–0.0) | 1–1 |
| 19 | 130.0 (100.0–161.0) | 127.0 (1.0–271.0) | 2–3 | 123.0 (116.0–136.0) | 0.0 (0.0–0.0) | 1–1 |
| 20 | 132.0 (106.0–162.0) | 130.0 (1.0–278.0) | 2–3 | 126.0 (119.0–137.0) | 0.0 (0.0–0.0) | 1–1 |
| 21 | 130.0 (112.0–162.0) | 131.0 (1.0–283.0) | 2–3 | 128.0 (121.0–138.0) | 0.0 (0.0–0.0) | 1–1 |
| 22 | 135.0 (115.0–162.0) | 133.0 (1.0–287.0) | 2–3 | 134.0 (123.0–141.0) | 0.0 (0.0–0.0) | 1–1 |
| 23 | 139.0 (120.0–162.0) | 131.0 (1.0–294.0) | 2–3 | 138.0 (124.0–146.0) | 0.0 (0.0–0.0) | 1–1 |
| 24 | 143.0 (121.0–163.0) | 128.0 (1.0–300.0) | 2–3 | 139.0 (126.0–150.0) | 0.0 (0.0–1.0) | 1–2 |
| 25 | 146.0 (123.0–164.0) | 130.0 (1.0–302.0) | 2–3 | 142.0 (125.0–149.0) | 0.0 (0.0–1.0) | 1–2 |
| 26 | 148.0 (125.0–165.0) | 132.0 (1.0–306.0) | 2–3 | 144.0 (127.0–147.0) | 0.0 (0.0–2.0) | 1–3 |
| 27 | 150.0 (124.0–167.0) | 135.0 (1.0–310.0) | 2–3 | 145.0 (129.0–149.0) | 0.0 (0.0–2.0) | 1–3 |
| 28 | 152.0 (131.0–165.0) | 152.0 (1.0–311.0) | 2–3 | 147.0 (136.0–150.0) | 0.0 (0.0–2.0) | 1–3 |
| 29 | 154.0 (133.0–167.0) | 163.0 (10.0–306.0) | 2–3 | 146.0 (142.0–149.0) | 0.0 (0.0–2.0) | 1–3 |
| 30 | 155.0 (137.0–167.0) | 180.0 (25.0–303.0) | 2–3 | 148.0 (143.0–150.0) | 0.0 (0.0–2.0) | 1–3 |
| 31 | 154.0 (138.0–167.0) | 197.0 (36.0–303.0) | 2–3 | 147.0 (144.0–148.0) | 0.0 (0.0–2.0) | 1–3 |
| 32 | 155.0 (138.0–169.0) | 205.0 (47.0–303.0) | 2–3 | 147.0 (145.0–149.0) | 0.0 (0.0–2.0) | 1–3 |
| 33 | 152.0 (134.0–170.0) | 214.0 (52.0–305.0) | 2–3 | 148.0 (144.0–149.0) | 0.0 (0.0–16.0) | 1–3 |
| 34 | 154.0 (134.0–169.0) | 218.0 (62.0–306.0) | 2–3 | 148.0 (142.0–150.0) | 0.0 (0.0–16.0) | 1–3 |
| 35 | 154.0 (135.0–170.0) | 228.0 (74.0–309.0) | 2–3 | 148.0 (142.0–151.0) | 0.0 (0.0–16.0) | 1–3 |
| 36 | 156.0 (137.0–170.0) | 234.0 (82.0–312.0) | 2–3 | 148.0 (142.0–155.0) | 0.0 (0.0–38.0) | 1–3 |
| 37 | 155.0 (137.0–169.0) | 237.0 (89.0–312.0) | 2–3 | 149.0 (142.0–154.0) | 0.0 (0.0–49.0) | 1–3 |
| 38 | 155.0 (136.0–171.0) | 240.0 (97.0–312.0) | 2–3 | 149.0 (141.0–154.0) | 0.0 (0.0–74.0) | 1–3 |
| 39 | 155.0 (136.0–170.0) | 247.0 (101.0–314.0) | 2–3 | 150.0 (142.0–155.0) | 0.0 (0.0–96.0) | 1–3 |
| 40 | 156.0 (139.0–169.0) | 253.0 (104.0–313.0) | 2–3 | 150.0 (144.0–156.0) | 0.0 (0.0–113.0) | 1–3 |
| 41 | 155.0 (137.0–171.0) | 255.0 (106.0–310.0) | 2–3 | 152.0 (142.0–156.0) | 0.0 (0.0–118.0) | 1–3 |
| 42 | 155.0 (141.0–170.0) | 256.0 (113.0–315.0) | 2–3 | 153.0 (145.0–155.0) | 0.0 (0.0–133.0) | 1–3 |
| 43 | 157.0 (141.0–172.0) | 259.0 (118.0–311.0) | 2–3 | 154.0 (146.0–156.0) | 0.0 (0.0–139.0) | 1–3 |
| 44 | 157.0 (144.0–171.0) | 262.0 (123.0–309.0) | 2–3 | 155.0 (149.0–156.0) | 0.0 (0.0–150.0) | 1–3 |
| 45 | 157.0 (144.0–174.0) | 258.0 (124.0–312.0) | 2–3 | 156.0 (147.0–157.0) | 0.0 (0.0–159.0) | 1–3 |
| 46 | 157.0 (148.0–173.0) | 262.0 (126.0–308.0) | 2–3 | 156.0 (149.0–157.0) | 0.0 (0.0–171.0) | 1–3 |
| 47 | 159.0 (149.0–172.0) | 260.0 (127.0–310.0) | 2–3 | 156.0 (150.0–161.0) | 0.0 (0.0–183.0) | 1–3 |
| 48 | 161.0 (151.0–172.0) | 259.0 (130.0–310.0) | 2–3 | 156.0 (151.0–163.0) | 0.0 (0.0–192.0) | 1–3 |
| 49 | 160.0 (151.0–173.0) | 264.0 (130.0–307.0) | 2–3 | 157.0 (151.0–162.0) | 0.0 (0.0–199.0) | 1–4 |
| 50 | 159.0 (149.0–173.0) | 270.0 (130.0–304.0) | 2–4 | 157.0 (151.0–163.0) | 0.0 (0.0–204.0) | 1–4 |
| 51 | 160.0 (149.0–174.0) | 282.0 (131.0–302.0) | 2–4 | 157.0 (151.0–163.0) | 0.0 (0.0–214.0) | 1–5 |
| 52 | 160.0 (150.0–175.0) | 288.0 (131.0–314.0) | 2–4 | 159.0 (152.0–163.0) | 1.0 (0.0–220.0) | 1–5 |
| 53 | 160.0 (149.0–174.0) | 293.0 (133.0–324.0) | 2–4 | 159.0 (157.0–162.0) | 1.0 (0.0–235.0) | 1–5 |
| 54 | 160.0 (150.0–171.0) | 293.0 (136.0–338.0) | 2–4 | 159.0 (157.0–162.0) | 1.0 (0.0–239.0) | 1–5 |
| 55 | 162.0 (149.0–172.0) | 292.0 (136.0–347.0) | 2–4 | 161.0 (159.0–164.0) | 2.0 (0.0–245.0) | 1–5 |
| 56 | 162.0 (149.0–170.0) | 294.0 (134.0–352.0) | 2–4 | 161.0 (160.0–164.0) | 16.0 (0.0–256.0) | 1–5 |
| 57 | 165.0 (149.0–171.0) | 294.0 (135.0–361.0) | 2–4 | 163.0 (161.0–167.0) | 34.0 (0.0–292.0) | 1–5 |
| 58 | 166.0 (152.0–173.0) | 297.0 (138.0–370.0) | 3–4 | 163.0 (161.0–168.0) | 49.0 (1.0–327.0) | 2–5 |
| 59 | 166.0 (152.0–172.0) | 304.0 (136.0–386.0) | 3–4 | 163.0 (162.0–170.0) | 63.0 (1.0–355.0) | 2–5 |
| 60 | 167.0 (151.0–174.0) | 302.0 (139.0–392.0) | 3–5 | 162.0 (160.0–170.0) | 96.0 (1.0–369.0) | 2–5 |
| 61 | 168.0 (154.0–172.0) | 300.0 (139.0–392.0) | 3–5 | 164.0 (159.0–170.0) | 120.0 (1.0–386.0) | 2–5 |
| 62 | 168.0 (158.0–171.0) | 301.0 (150.0–394.0) | 3–5 | 167.0 (160.0–169.0) | 144.0 (10.0–396.0) | 2–5 |
| 63 | 169.0 (163.0–171.0) | 299.0 (149.0–398.0) | 3–5 | 168.0 (161.0–169.0) | 164.0 (11.0–422.0) | 2–5 |
| 64 | 171.0 (162.0–173.0) | 299.0 (153.0–403.0) | 3–5 | 167.0 (162.0–170.0) | 187.0 (16.0–440.0) | 2–6 |
| 65 | 171.0 (160.0–175.0) | 298.0 (162.0–405.0) | 3–6 | 167.0 (163.0–170.0) | 205.0 (24.0–443.0) | 2–6 |
| 66 | 169.0 (161.0–176.0) | 301.0 (170.0–418.0) | 3–6 | 165.0 (164.0–171.0) | 226.0 (29.0–450.0) | 2–6 |
| 67 | 166.0 (161.0–177.0) | 301.0 (172.0–438.0) | 3–6 | 167.0 (163.0–171.0) | 239.0 (29.0–465.0) | 2–6 |
| 68 | 166.0 (161.0–177.0) | 302.0 (181.0–456.0) | 3–6 | 166.0 (161.0–171.0) | 253.0 (37.0–478.0) | 2–6 |
| 69 | 165.0 (161.0–179.0) | 298.0 (190.0–469.0) | 3–6 | 166.0 (162.0–171.0) | 264.0 (46.0–484.0) | 3–6 |
| 70 | 166.0 (162.0–176.0) | 302.0 (205.0–487.0) | 3–6 | 165.0 (163.0–173.0) | 283.0 (59.0–486.0) | 3–6 |
| 71 | 167.0 (162.0–178.0) | 300.0 (211.0–506.0) | 3–6 | 166.0 (164.0–173.0) | 291.0 (64.0–488.0) | 3–6 |
| 72 | 167.0 (164.0–179.0) | 304.0 (226.0–528.0) | 3–6 | 166.0 (164.0–172.0) | 299.0 (69.0–502.0) | 3–6 |
| 73 | 166.0 (164.0–179.0) | 304.0 (229.0–545.0) | 3–6 | 166.0 (163.0–172.0) | 311.0 (73.0–500.0) | 4–6 |
| 74 | 165.0 (164.0–177.0) | 308.0 (240.0–572.0) | 3–6 | 167.0 (164.0–171.0) | 321.0 (77.0–515.0) | 4–6 |
| 75 | 166.0 (165.0–178.0) | 312.0 (240.0–582.0) | 3–6 | 168.0 (164.0–172.0) | 345.0 (77.0–523.0) | 4–6 |
| 76 | 168.0 (165.0–182.0) | 318.0 (242.0–595.0) | 3–6 | 170.0 (165.0–173.0) | 369.0 (80.0–527.0) | 4–6 |
| 77 | 168.0 (164.0–184.0) | 321.0 (265.0–607.0) | 3–6 | 170.0 (166.0–175.0) | 400.0 (104.0–524.0) | 4–6 |
| 78 | 169.0 (165.0–185.0) | 322.0 (275.0–618.0) | 3–6 | 171.0 (167.0–173.0) | 417.0 (109.0–527.0) | 4–6 |
| 79 | 171.0 (167.0–185.0) | 326.0 (285.0–634.0) | 3–6 | 172.0 (168.0–174.0) | 441.0 (121.0–535.0) | 4–6 |
| 80 | 173.0 (166.0–185.0) | 323.0 (294.0–648.0) | 3–6 | 174.0 (170.0–178.0) | 454.0 (130.0–551.0) | 4–6 |
| 81 | 172.0 (169.0–185.0) | 322.0 (308.0–661.0) | 3–6 | 174.0 (172.0–177.0) | 473.0 (138.0–566.0) | 4–7 |
| 82 | 174.0 (169.0–186.0) | 321.0 (308.0–670.0) | 3–6 | 177.0 (173.0–178.0) | 492.0 (147.0–578.0) | 4–7 |
| 83 | 177.0 (168.0–188.0) | 324.0 (311.0–678.0) | 3–6 | 177.0 (173.0–178.0) | 530.0 (166.0–595.0) | 4–7 |
| 84 | 179.0 (169.0–190.0) | 331.0 (310.0–673.0) | 3–6 | 178.0 (174.0–180.0) | 560.0 (186.0–603.0) | 4–7 |
| 85 | 178.0 (169.0–187.0) | 345.0 (312.0–686.0) | 3–6 | 179.0 (172.0–180.0) | 590.0 (204.0–614.0) | 4–8 |
| 86 | 177.0 (170.0–185.0) | 346.0 (316.0–692.0) | 3–6 | 179.0 (172.0–182.0) | 600.0 (213.0–625.0) | 4–9 |
| 87 | 176.0 (170.0–188.0) | 349.0 (316.0–698.0) | 3–6 | 180.0 (172.0–182.0) | 617.0 (227.0–644.0) | 4–9 |
| 88 | 177.0 (171.0–188.0) | 355.0 (314.0–704.0) | 3–6 | 180.0 (173.0–182.0) | 631.0 (239.0–677.0) | 4–9 |
| 89 | 176.0 (172.0–189.0) | 354.0 (313.0–703.0) | 3–6 | 180.0 (176.0–182.0) | 639.0 (252.0–709.0) | 4–9 |

Food divergence first appears in both the target and other settlements across seeds; health divergence initially remains in the target settlement, then reaches other settlements. By day 89, food touches 3–6 settlements and health touches 4–9 of the 50. The full daily values above show the spread beyond the target settlement; they do not establish a general bound for other graph or workload configurations.

### H=365 extension

Each run executes 365 days indexed `0..364`; the final row is the state after the 365th executed day. Last-30-day slope is the linear change in `|D|` per day over the last 30 intervals. Peak fraction and endpoint fraction use the corrected record-based denominator.

| Intervention | Seed | Peak day index | Peak `|D|` | Peak fraction | After day 365 `|D|` | Day-365 fraction | Last-30-day `|D|` slope/day | Target death day | Settlements touched at end |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Food = 0 | 0 | 364 | 10150 | 0.9852 | 10150 | 0.9852 | 2.57 | none | 50 |
| Food = 0 | 1 | 361 | 10164 | 0.9866 | 10159 | 0.9858 | 1.77 | none | 50 |
| Food = 0 | 3 | 364 | 10171 | 0.9881 | 10171 | 0.9881 | 2.00 | none | 50 |
| Food = 0 | 7 | 364 | 10188 | 0.9897 | 10188 | 0.9897 | 2.13 | none | 50 |
| Food = 0 | 81985529216486895 | 364 | 10153 | 0.9839 | 10153 | 0.9839 | 3.67 | none | 50 |
| Health = 0.01 | 0 | 364 | 10183 | 0.9884 | 10183 | 0.9884 | 3.10 | 240 | 50 |
| Health = 0.01 | 1 | 363 | 10157 | 0.9854 | 10156 | 0.9853 | 1.67 | 141 | 50 |
| Health = 0.01 | 3 | 364 | 10200 | 0.9908 | 10200 | 0.9908 | 2.57 | 211 | 50 |
| Health = 0.01 | 7 | 364 | 10194 | 0.9902 | 10194 | 0.9902 | 2.17 | none | 50 |
| Health = 0.01 | 81985529216486895 | 364 | 10188 | 0.9873 | 10188 | 0.9873 | 2.80 | 126 | 50 |

Across the five H=365 runs, food=0 peak fractions have median/range `0.9866 (0.9839–0.9897)` and endpoint fractions `0.9858 (0.9839–0.9897)`. Health=0.01 peak fractions are `0.9884 (0.9854–0.9908)` and endpoint fractions `0.9884 (0.9853–0.9908)`. Peak occurs at day index 362–364. Every run touches all 50 settlements by the endpoint. Food targets do not die; health targets die in four of five runs (days 126, 141, 211, and 240), after cascade onset in each of those runs (days 8–10).

### Findings

- **Fact:** the corrected H=90 food and health runs still cascade in all five seeds, with peak fractions around 5.2% and 8.1% respectively. Forced Idle gives no divergence in two seeds, a small no-cascade divergence in two, and a cascade in one. `forced_matches_natural` is `false` in all five forced-Idle runs, including the two no-divergence runs; the summary field reports action-choice equality, not equality of final state.
- **Fact:** H=365 food and health runs end at 98.4–99.1% normalized divergence, and the endpoint is within 0.1 percentage points of the peak in every run. The last-30-day `|D|` slope is positive in every run (about 1.7–3.7 records/day).
- **Inference:** by day 365 both cases are near saturation but still grow slowly in the observed window; there is no measured decay.
- **Inference:** divergence is not confined to the target settlement in this workload. It continues to spread through the static graph; the measurements cannot distinguish a true plateau from continued slow growth beyond day 365.
- **Fact:** in four health=0.01 H=365 runs the target dies after cascade onset; the fifth target survives through the horizon. In the 90-day cases the health target did not die. **Inference:** target death is not the observed onset trigger in these runs; this remains temporal evidence, not causal proof.
