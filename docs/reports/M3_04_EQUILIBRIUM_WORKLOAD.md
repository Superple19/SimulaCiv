<a id="m3-equilibrium-workload"></a>
# M3-04 (formerly M3-E) Equilibrium Workload
Status: current


This report records a benchmark-only search for a food-supply configuration with sustained Phase 6B interactions. It changes no simulation semantics or production behavior.

## Acceptance rule and method

The search started from the M3 sustained-workload configuration (`base_metabolic_cost = 0.48`) and changed only one permitted food-supply field at a time. It evaluated `N=10,000`, `K=50`, `k=8`, `p_local=0.5`, seed `81985529216486895`, replicate `7`, through day 300. The evaluation window is days 50–300 inclusive.

A candidate passes when all six conditions hold in that window:

- deaths on every day are at most `0.1% of N` and total deaths are at most `1% of N`
- absolute mean-alive-food drift from day 50 to day 300 is at most `5%` of day-50 mean food
- keyed Phase 6B entries are nonzero every day and their mean is at least `10%` of mean alive population
- at least one day contains a GiveFood intent with a non-null target

Food means are computed from live storage food values after each completed day. Keyed counts and GiveFood targets are counted from the day's Phase 6B `FoodTransferred` events. The benchmark uses the public serial M3 storage runner, with metrics and events enabled and snapshots disabled. It advances one day at a time, reusing one run context per candidate. Candidate evaluations are independent runs with identical seed and replicate.

The grid had one baseline plus 51 one-field candidates. Relative change in the table is `abs(new - baseline) / abs(baseline)` for the changed field, using the baseline values shown in the configuration table below. All candidates changed zero or one field; no action weight, bias, interaction amount, theft probability, starvation threshold, trait, or M3 setting was tuned.

## Full deterministic grid

| Candidate | Relative change | Minimum alive | Max deaths/day | Deaths, days 50–300 | Food drift | Mean keyed / mean alive | Zero-key days | GiveFood Some days | Gates | Pass |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| sustained base `0.48` | 0.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `base_metabolic_cost=0.475` | 1.042% | 1 | 366 | 9,999 | 28.3178% | 42.02% | 52 | 198 | 2/6 | no |
| `base_metabolic_cost=0.47` | 2.083% | 1 | 360 | 9,999 | 35.5209% | 41.91% | 43 | 207 | 2/6 | no |
| `base_metabolic_cost=0.46` | 4.167% | 4 | 329 | 9,996 | 84.6974% | 41.67% | 33 | 218 | 2/6 | no |
| `base_metabolic_cost=0.45` | 6.250% | 9 | 331 | 9,991 | 98.9877% | 41.43% | 19 | 229 | 2/6 | no |
| `base_metabolic_cost=0.44` | 8.333% | 43 | 311 | 9,957 | 98.4305% | 41.14% | 2 | 249 | 2/6 | no |
| `base_metabolic_cost=0.42` | 12.500% | 2,264 | 305 | 7,736 | 99.2348% | 40.74% | 0 | 251 | 3/6 | no |
| `base_metabolic_cost=0.4` | 16.667% | 9,871 | 16 | 129 | 93.7763% | 39.75% | 0 | 251 | 3/6 | no |
| `base_metabolic_cost=0.35` | 27.083% | 10,000 | 0 | 0 | 45.8578% | 28.36% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.32` | 33.333% | 10,000 | 0 | 0 | 12.3251% | 25.00% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.313` | 34.792% | 10,000 | 0 | 0 | 5.0755% | 24.91% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.3129` | 34.812% | 10,000 | 0 | 0 | 4.9743% | 24.91% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.3128` | 34.833% | 10,000 | 0 | 0 | 4.8716% | 24.91% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.31275` | 34.844% | 10,000 | 0 | 0 | 4.8209% | 24.91% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.3125` | 34.896% | 10,000 | 0 | 0 | 4.5657% | 24.90% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.312` | 35.000% | 10,000 | 0 | 0 | 4.0567% | 24.89% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.31` | 35.417% | 10,000 | 0 | 0 | 2.0293% | 24.88% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.305` | 36.458% | 10,000 | 0 | 0 | 2.9637% | 24.84% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.303` | 36.875% | 10,000 | 0 | 0 | 4.9347% | 24.83% | 0 | 251 | 6/6 | yes |
| `base_metabolic_cost=0.30275` | 36.927% | 10,000 | 0 | 0 | 5.1800% | 24.82% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.3025` | 36.979% | 10,000 | 0 | 0 | 5.4244% | 24.82% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.302` | 37.083% | 10,000 | 0 | 0 | 5.9139% | 24.82% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.3015` | 37.187% | 10,000 | 0 | 0 | 6.4019% | 24.82% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.301` | 37.292% | 10,000 | 0 | 0 | 6.8888% | 24.82% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.3005` | 37.396% | 10,000 | 0 | 0 | 7.3749% | 24.81% | 0 | 251 | 5/6 | no |
| `base_metabolic_cost=0.3` | 37.500% | 10,000 | 0 | 0 | 7.8609% | 24.81% | 0 | 251 | 5/6 | no |
| `health_decay_rate=0.04` | 20.000% | 3 | 350 | 9,997 | 81.4160% | 42.16% | 41 | 210 | 2/6 | no |
| `health_decay_rate=0.03` | 40.000% | 28 | 308 | 9,972 | 99.1982% | 42.19% | 5 | 246 | 2/6 | no |
| `health_decay_rate=0.02` | 60.000% | 6,367 | 248 | 3,633 | 99.6485% | 43.02% | 0 | 251 | 3/6 | no |
| `health_decay_rate=0.01` | 80.000% | 10,000 | 0 | 0 | 95.0124% | 44.81% | 0 | 251 | 5/6 | no |
| `health_decay_rate=0.005` | 90.000% | 10,000 | 0 | 0 | 90.8976% | 44.86% | 0 | 251 | 5/6 | no |
| `regrowth_rate=0.11` | 10.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `regrowth_rate=0.15` | 50.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `regrowth_rate=0.2` | 100.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `regrowth_rate=0.3` | 200.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `regrowth_rate=0.5` | 400.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `carrying_capacity=11000` | 10.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `carrying_capacity=12000` | 20.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `carrying_capacity=15000` | 50.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `carrying_capacity=20000` | 100.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `carrying_capacity=30000` | 200.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `initial_settlement_resource=2100` | 5.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `initial_settlement_resource=2200` | 10.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `initial_settlement_resource=2500` | 25.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `initial_settlement_resource=3000` | 50.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `initial_settlement_resource=4000` | 100.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `initial_settlement_resource=6000` | 200.000% | 2 | 371 | 9,998 | 59.3053% | 42.13% | 62 | 189 | 2/6 | no |
| `base_work_yield=2.1` | 5.000% | 1 | 356 | 9,999 | 33.9155% | 41.83% | 39 | 212 | 2/6 | no |
| `base_work_yield=2.25` | 12.500% | 27 | 314 | 9,973 | 95.6706% | 41.28% | 1 | 247 | 2/6 | no |
| `base_work_yield=2.5` | 25.000% | 7,885 | 167 | 2,115 | 96.5087% | 40.27% | 0 | 251 | 3/6 | no |
| `base_work_yield=3` | 50.000% | 10,000 | 0 | 0 | 18.7593% | 25.52% | 0 | 251 | 5/6 | no |
| `base_work_yield=4` | 100.000% | 10,000 | 0 | 0 | 106.9874% | 24.65% | 0 | 251 | 5/6 | no |

Gate count is out of six, counting daily mortality, cumulative mortality, food drift, no zero-key day, keyed volume, and GiveFood Some as separate conditions. The `min alive` column spans the full 300-day run; the other acceptance counts use days 50–300.

## Selected configuration and 300-day result

The selected candidate is `base_metabolic_cost=0.3129`. It changes one field from the `0.48` sustained base and is the smallest relative change in the tested grid that passes all six N=10,000 gates. The immediately closer tested value, `0.313`, misses the food-drift limit at `5.0755%`. The result is close to the threshold and should be read as a measured grid point, not a continuous optimum.

| Food-supply field | M2-40 | Sustained search base | Selected equilibrium preset |
|---|---:|---:|---:|
| `world.initial_settlement_resource` | 2,000.0 | 2,000.0 | 2,000.0 |
| `environment.carrying_capacity` | 10,000.0 | 10,000.0 | 10,000.0 |
| `environment.regrowth_rate` | 0.1 | 0.1 | 0.1 |
| `environment.base_metabolic_cost` | 1.0 | 0.48 | 0.3129 |
| `environment.health_decay_rate` | 0.05 | 0.05 | 0.05 |
| `economy.base_work_yield` | 2.0 | 2.0 | 2.0 |

At N=10,000, all 10,000 agents were alive on days 50 and 300, with zero deaths in the evaluation window and zero deaths on any day. Mean food among living agents moved from `24.883567` on day 50 to `24.378613` on day 300 (`2.0293%` drift). Mean keyed entries were `2,488.02/day` against `10,000` mean alive; there were no zero-key days, and GiveFood had a non-null target on all 251 days.

The selected configuration was also run at N=100,000, K=500, for 300 days at each requested locality. Population stayed at 100,000, there were no deaths, keyed entries were nonzero every day, and GiveFood found targets every day. Mean keyed entries were `24,986.90`, `24,961.48`, and `24,897.61` per day at `p_local=0`, `0.5`, and `1`. Food drift was `6.1259%`, `6.1253%`, and `6.1260%`, respectively, so this cross-size check misses the 5% food-drift criterion at all three locality settings. The selected point passes the defined N=10,000 grid; the N=100,000 drift difference is recorded as a scale sensitivity, not hidden by retuning.

## Reproduction

Run the full deterministic search and the selected-candidate N=100,000 locality check with:

```powershell
cargo bench -p sim-model --bench m3_equilibrium_bench
```

The `m3_trace` example has an `equilibrium` preset using the selected food-supply values. Example serial trace commands:

```powershell
cargo run --release -p sim-model --example m3_trace -- --config equilibrium --n 10000 --settlements 50 --k 8 --p-local 0.5 --days 300 --seed 81985529216486895 --replicate 7 --runner serial --output target/m3-equilibrium-10k.jsonl
cargo run --release -p sim-model --example m3_trace -- --config equilibrium --n 100000 --settlements 500 --k 8 --p-local 0 --days 300 --seed 81985529216486895 --replicate 7 --runner serial --output target/m3-equilibrium-100k-p0.jsonl
cargo run --release -p sim-model --example m3_trace -- --config equilibrium --n 100000 --settlements 500 --k 8 --p-local 0.5 --days 300 --seed 81985529216486895 --replicate 7 --runner serial --output target/m3-equilibrium-100k-p05.jsonl
cargo run --release -p sim-model --example m3_trace -- --config equilibrium --n 100000 --settlements 500 --k 8 --p-local 1 --days 300 --seed 81985529216486895 --replicate 7 --runner serial --output target/m3-equilibrium-100k-p1.jsonl
```

The N=10,000 command was run locally; the example's internal final-digest comparison against a plain run succeeded. Its final digests were State `19d51fe54b86f3457808bf0525707523568a32bd3dee171341d5c4449bb0ed88`, Metrics `a95aac1a8c6662b6db15f030a933be66ab7327e730acdff545f9a584a10c13da`, and Events `87b271c49964d3b184c3c65d5f61d0e3dbd1dfb0ccceb7ff140d2e8dce35c3ee`. The trace file is local under `target/` and is not committed.

## Host and runtime

- Host: AMD Ryzen 5 9600X, Windows 11 Pro, `x86_64-pc-windows-msvc`
- Compiler: `rustc 1.98.1 (48a229cea 2026-09-01)`
- Search command: `cargo bench -p sim-model --bench m3_equilibrium_bench`
- Search and N=100,000 verification wall time: `94.436 s` (release bench profile, including the Cargo invocation)

This step changed only the bench registration, the new benchmark, the `m3_trace` equilibrium preset, and this report. Production simulation source, existing tests, goldens, snapshots, frozen contract sections, and CI were unchanged.
