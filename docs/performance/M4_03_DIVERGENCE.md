# M4-03 Divergence Measurement

**Status:** M4-03 divergence-series implementation is committed. The §12.6 A-5 performance gate passes all three cells. Report-only intervention experiments remain to be recorded.

## Performance gate

The gate compared `run_counterfactual_branch_with_divergence_series` against the M4-02 `run_counterfactual_branch` API. Each pair used the same day-0 snapshot and a no-op `BaseMetabolicCost` override, serial execution, `H = 30`, metrics and events enabled, and snapshots disabled. The fixtures were the M3P sustained profile for M3P cells and the M3 propagation profile for the M3+propagation cell; seed `81985529216486895`, replicate `7`, `k = 8`, `p_local = 0.5`, and `K = N/200`.

All cells ran in one release process. Each used two warmups and 41 paired timed samples. Warmup selected `r = 1` for every cell; each timed sample measured one M4-02 call and one M4-03 call, alternating path order. Deltas are `M4-03 series − M4-02 full API`. The bootstrap used the fixed §11.7 A-3 seed and 10,000 resamples. No retry was needed. The report includes per-operation medians and MADs in microseconds.

Host: AMD Ryzen 5 9600X, 6 cores / 12 logical processors, Windows 11 Pro. High priority and affinity were not applied. Total CPU load/free RAM were 2% / 74,932 MB before and 0% / 75,026 MB after. These values are informational under A-5. Timed cell wall time summed to 1,176.1 seconds; release compilation and the untimed initial parity check are excluded.

| Cell | `r` | M4-02 median ± MAD (µs) | M4-03 median ± MAD (µs) | Δ median ± MAD (µs) | U (µs) | T (µs) | Path MAD/median | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial, N=10,000 | 1 | 623,848.6 ± 23,172.5 | 628,807.8 ± 25,365.4 | 11,927.7 ± 23,071.7 | 25,522.1 | 31,192.4 | 3.714% / 4.034% | **PASS** |
| M3P serial, N=100,000 | 1 | 9,153,254.6 ± 109,429.2 | 9,180,285.3 ± 84,082.5 | 38,610.5 ± 127,941.0 | 78,466.9 | 457,662.7 | 1.196% / 0.916% | **PASS** |
| M3 + propagation serial, N=100,000 | 1 | 4,537,758.9 ± 26,111.4 | 4,550,553.6 ± 26,240.7 | 4,463.6 ± 47,880.5 | 11,250.6 | 226,887.9 | 0.575% / 0.577% | **PASS** |

For all three cells, both path spreads were at most 5%, and bootstrap `U <= T`. The API returned bit-identical M4-02 `BranchResult` values in the untimed parity checks.

## Test time

The new `m4_03_divergence_tests` integration target took 12.44 seconds in debug. The new synthetic cascade unit tests completed in less than 0.01 seconds. No existing test, golden, snapshot, or CI file changed; the new divergence-series golden is in `crates/sim-model/tests/fixtures/m4_03_divergence_golden.txt`.
