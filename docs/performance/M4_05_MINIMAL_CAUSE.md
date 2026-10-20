# M4-05 Minimal-Cause Search

**Status: M4-05 COMPLETE.** The implementation tests, N=2,000 golden, M3P batch-of-one A-5 gate, and first report-only minimal-cause experiments are recorded below.

## A-5 batch-of-one performance gate

**Result: PASS.** The measured operation was the M3P serial batch API with a one-time `food=0` override on `AgentId(0)` against the M4-02 direct branch API. Both returned identical per-day base and branch records, including the day-29 snapshot. The run used master seed `81985529216486895` and replicate ID `7`.

| Profile / workload | Horizon | `r` | Samples | Minimum / median paired sample | Direct median ± MAD | Batch median ± MAD | Delta median ± MAD | Bootstrap U | T | Gate |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial, N=10,000, K=50, k=8, p_local=0.5 | 30 days | 2 | 41 | 2.419 / 2.568 s | 645,218.700 ± 14,521.150 µs | 640,705.050 ± 15,288.150 µs | -3,340.050 ± 19,248.100 µs | 5,659.400 µs | 32,260.935 µs | PASS |

The final run used two warmups, then 41 paired samples. Each sample repeated the direct and batch calls twice, alternating their order, and reported elapsed time per operation. Every measured sample exceeded one second. Path MAD/median was 2.251% for direct and 2.386% for batch, both within the 5% validity bound. The one-sided 95% bootstrap upper bound of the paired median delta was below `T = 5% × direct median`.

The host was Windows 11 Pro on an AMD Ryzen 5 9600X (6 cores, 12 logical processors). Informational host readings were CPU load 2% before and 0% after, and free RAM 75,568 MiB before and 75,547 MiB after. These readings did not affect validity. A preceding A-5 run also passed (r=2, 41 samples, path spreads 2.618% and 2.005%, U=3,096.400 µs, T=31,827.092 µs); the final run above includes the required before/after host readings.

The measurement harness was a temporary release example and was removed after the run. Commands: `cargo run --release -p sim-model --example m4_05_a5_gate` for the initial run, then `target/release/examples/m4_05_a5_gate.exe` for the confirmatory run.

## First minimal-cause experiments

These report-only searches used the M3-08 sustained M3P configuration, `N=2,000`, `K=3`, `k=8`, `p_local=0.5`, master seed `81985529216486895`, paired replicate IDs `1..=10`, and `H=90`. Group 0 was the largest settlement (the lowest GroupId among the tied groups). Its eight and sixteen candidate agents were the lowest live AgentIds in that settlement: `[0,3,6,9,12,15,18,21]` and `[0,3,6,9,12,15,18,21,24,27,30,33,36,39,42,45]`.

| Case | Search | Candidate set | Cause set | Guarantee | Branch evaluations | Search wall time | Effect: cause / full set | Removing one cause item |
|---|---|---|---|---|---:|---:|---:|---|
| (a) | Exhaustive | `health=0.01` on the 8 listed agents; target-settlement alive effect `<= -1` | `F:agent=0:health=0x3c23d70a`, `F:agent=12:health=0x3c23d70a`, `F:agent=15:health=0x3c23d70a`, `F:agent=21:health=0x3c23d70a` | minimum | 960 | 122.638 s | -1.0 / -1.4 | every removal breaks the predicate |
| (b) | ddmin | `health=0.01` on the 16 listed agents; same predicate | `F:agent=6:health=0x3c23d70a`, `F:agent=9:health=0x3c23d70a`, `F:agent=36:health=0x3c23d70a`, `F:agent=42:health=0x3c23d70a` | 1-minimal | 220 | 20.920 s | -1.0 / -2.5 | every removal breaks the predicate |
| (c) | Exhaustive | +1% to BaseMetabolicCost, HealthDecayRate, BaseWorkYield, RegrowthRate, CarryingCapacity, and BirthProbability; world alive effect `<= -5` | none | none | 650 | 83.691 s | n/a / +0.2 | n/a |

Case (a)'s search found the first satisfying 4-item subset and stopped. To measure its full 8-item effect without rerunning all 256 subsets, a separate 20-evaluation probe appended three config overrides with values identical to the base config, forcing the probe through ddmin so its first evaluation was the complete set. The probe took 2.671 s and measured -1.4. The interpretation that this equals the original 8-field full-set effect relies on the three added overrides being exact no-ops; they do not contribute to the cause set or guarantee. Cases (b) and (c) had their original full sets evaluated by their searches.

Case (a) and case (b) had no cause IDs in common. **Inference, limited to this seed and horizon:** the selected target-settlement predicate was satisfied by four of the eight health interventions, while ddmin found a different four-item 1-minimal set from the sixteen-candidate domain. This does not establish that the larger-domain result is globally minimum. The six tested config changes did not meet the world-population threshold; their complete set instead had a mean effect of +0.2 agents. These are single-workload, single-seed observations and do not imply general causal structure.
