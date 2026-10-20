# M4-05 Minimal-Cause Search

This report records the M4-05 batch-of-one A-5 performance gate and report-only minimal-cause experiments. M4-05 is not complete until the experiments and completion gates are recorded.

## A-5 batch-of-one performance gate

**Result: PASS.** The measured operation was the M3P serial batch API with one identified agent-field override against the M4-02 direct branch API. Both returned identical per-day base and branch records, including the day-29 snapshot.

| Profile / workload | Horizon | `r` | Samples | Minimum / median paired sample | Direct median ± MAD | Batch median ± MAD | Delta median ± MAD | Bootstrap U | T | Gate |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial, N=10,000, K=50, k=8, p_local=0.5 | 30 days | 2 | 41 | 2.419 / 2.568 s | 645,218.700 ± 14,521.150 µs | 640,705.050 ± 15,288.150 µs | -3,340.050 ± 19,248.100 µs | 5,659.400 µs | 32,260.935 µs | PASS |

The final run used two warmups, then 41 paired samples. Each sample repeated the direct and batch calls twice, alternating their order, and reported elapsed time per operation. Every measured sample exceeded one second. Path MAD/median was 2.251% for direct and 2.386% for batch, both within the 5% validity bound. The one-sided 95% bootstrap upper bound of the paired median delta was below `T = 5% × direct median`.

The host was Windows 11 Pro on an AMD Ryzen 5 9600X (6 cores, 12 logical processors). Informational host readings were CPU load 2% before and 0% after, and free RAM 75,568 MiB before and 75,547 MiB after. These readings did not affect validity. A preceding A-5 run also passed (r=2, 41 samples, path spreads 2.618% and 2.005%, U=3,096.400 µs, T=31,827.092 µs); the final run above includes the required before/after host readings.

The measurement harness was a temporary release example and was removed after the run. Commands: `cargo run --release -p sim-model --example m4_05_a5_gate` for the initial run, then `target/release/examples/m4_05_a5_gate.exe` for the confirmatory run.
