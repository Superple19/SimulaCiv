# M4-03 Divergence Measurement

**Status:** M4-03 implementation, the §12.6 A-5 performance gate, and the first report-only intervention experiments are complete. All gate cells pass.

## Performance gate

The gate compared `run_counterfactual_branch_with_divergence_series` against the M4-02 `run_counterfactual_branch` API. Each pair used the same day-0 snapshot and a no-op `BaseMetabolicCost` override, serial execution, `H = 30`, metrics and events enabled, and snapshots disabled. The fixtures were the M3P sustained profile for M3P cells and the M3 propagation profile for the M3+propagation cell; seed `81985529216486895`, replicate `7`, `k = 8`, `p_local = 0.5`, and `K = N/200`.

All cells ran in one release process. Each used two warmups and 41 paired timed samples. Warmup selected `r = 1` for every cell; each timed sample measured one M4-02 call and one M4-03 call, alternating path order. Deltas are `M4-03 series − M4-02 full API`. The bootstrap used the fixed §11.7 A-3 seed and 10,000 resamples. No retry was needed. The report includes per-operation medians and MADs in microseconds.

Host: AMD Ryzen 5 9600X, 6 cores / 12 logical processors, Windows 11 Pro. High priority and affinity were not applied. Total CPU load/free RAM were 2% / 74,932 MB before and 0% / 75,026 MB after. These values are informational under A-5. Timed cell wall time summed to 1,176.1 seconds; release compilation and the untimed initial parity check are excluded.

| Cell | `r` | M4-02 median ± MAD (µs) | M4-03 median ± MAD (µs) | Δ median ± MAD (µs) | U (µs) | T (µs) | M4-02 / M4-03 path MAD/median | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial, N=10,000 | 1 | 623,848.6 ± 23,172.5 | 628,807.8 ± 25,365.4 | 11,927.7 ± 23,071.7 | 25,522.1 | 31,192.4 | 3.714% / 4.034% | **PASS** |
| M3P serial, N=100,000 | 1 | 9,153,254.6 ± 109,429.2 | 9,180,285.3 ± 84,082.5 | 38,610.5 ± 127,941.0 | 78,466.9 | 457,662.7 | 1.196% / 0.916% | **PASS** |
| M3 + propagation serial, N=100,000 | 1 | 4,537,758.9 ± 26,111.4 | 4,550,553.6 ± 26,240.7 | 4,463.6 ± 47,880.5 | 11,250.6 | 226,887.9 | 0.575% / 0.577% | **PASS** |

For all three cells, both path spreads were at most 5%, and bootstrap `U <= T`. The API returned bit-identical M4-02 `BranchResult` values in the untimed parity checks.

## First divergence experiments

The report-only experiment used the M3P sustained fixture at `N = 10,000`, `K = 50`, `k = 8`, `p_local = 0.5`, replicate `7`, and `H = 90`. Seeds were `{0, 1, 3, 7, 81985529216486895}`. For each seed, the target was the lowest live `AgentId` in the largest settlement; ties chose the lowest `GroupId`. This selected `AgentId(0)` in `GroupId(0)` for all five runs. Each case compared its M4-03 branch result with a separately run M4-02 branch result; all 20 comparisons were bit-identical.

| Intervention | First divergence (day / slot / component) | Cascade onset and plateau | Doubling time (days) | Peak `|D|` | Peak fraction | Days to peak | Target death / onset relation | Newborn CRN loss |
|---|---|---|---:|---:|---:|---:|---|---:|
| Set target food to 0 | `0 / InterventionBoundary / agent_rows` (5/5) | onset median 5 (range 1–12); plateau median 5 (1–12) | median 4.17 (2.70–4.73) | median 526 (495–892) | median 0.0521 (0.0491–0.0884) | median 88 (80–89) | no target death in 5/5 | 0 in 5/5 |
| Set target health to 0.01 | `0 / InterventionBoundary / agent_rows` (5/5) | onset median 9 (range 3–10); plateau median 9 (3–10) | median 4.15 (4.11–5.56) | median 819 (428–891) | median 0.0812 (0.0424–0.0883) | 89 in 5/5 | no target death in 5/5 | 0 in 5/5 |
| Force target Idle on day 0 | `0 / Core(Phase6A) / agent_rows` (3/5); no divergence (2/5) | onset in 1/5 at day 7, plateau 7; no cascade in 4/5 | 4.74 for the one cascade | median 1 (0–286) | median 0.000100 (0–0.02834) | available 3/5: median 0, range 0–89 | no target death in the horizon | 0 in 5/5 |
| Increase BaseMetabolicCost by 1% | `0 / Core(Phase2) / agent_rows` (5/5) | no cascade in 5/5 | absent | median 10,093 (10,086–10,096) | median 1.00030 (1.00000–1.00040) | median 88 (88–89) | not applicable to a config intervention | 0 in 5/5 |

The target had no `MortalityCommitted` event in any of the 20 runs. The first-divergence slots, component names, and onset/death relation are taken from the API records and the Phase 9 event stream; absence of a target-death event is not evidence about what would happen after day 90.

Peak fractions can exceed `1.0` under the frozen definition: `D_d` counts differing matched records, including dead records, while `L_d` counts matched entities alive on at least one side.

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
