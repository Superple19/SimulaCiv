# M3-08 (formerly M3-P) Population Dynamics
Status: current

M3-08 implementation and sustained-workload gate complete

**Measured path:** serial Hybrid / `SegmentedAgentStorage` M3P runner  
**Reference configuration:** M2-40 with the changes listed below  
**Host:** AMD Ryzen 5 9600X, 6 physical / 12 logical processors; Windows 11 Pro, build 26200; Rust and Cargo `1.98.1`

## Sustained configuration

The selected workload changes only the runtime inputs needed for M3P, food supply, and the requested decision policy. M2 and M3 defaults, goldens, and snapshot bytes remain unchanged.

| Setting | M2-40 | M3P sustained |
|---|---:|---:|
| `world.initial_settlement_resource` | 2,000 | 1,000,000 |
| `environment.carrying_capacity` | 10,000 | 10,000,000 |
| `environment.regrowth_rate` | 0.1 | 0.1 |
| `environment.base_metabolic_cost` | 1.0 | 1.0 |
| `environment.health_decay_rate` | 0.05 | 0.2 |
| `economy.base_work_yield` | 2.0 | 1.9 |
| `initial_food`, traits, economy prices, interaction amounts, starvation threshold | M2-40 values | unchanged |
| `profile`, relationship graph | M2 | `m3p`, `k=8`, `p_local=0.5` |
| Birth rule | absent | `birth_probability=0.0001`, `birth_food_cost=0.5`, `birth_min_health=0.5`, `birth_food_threshold=0.5`, `births_max_per_day=0` (unlimited), compaction interval 50 days |

The decision policy adds positive hunger weights to Work and BuyFood and positive food-surplus weights to SellFood and GiveFood. In action order `[Work, BuyFood, SellFood, GiveFood, StealFood, Idle]`, the action biases are `[2, 0, 0, 0, 0, 0]`; the nonzero feature weights are Work/hunger `1`, BuyFood/hunger `1`, SellFood/food-surplus `1`, and GiveFood/food-surplus `1`. All other decision weights and the M2-40 trait weights are unchanged.

All runs use master seed `81985529216486895`, replicate `7`, `k=8`, and `p_local=0.5`. For `N=10,000` the workload has `K=50`; for `N=100,000`, `K=500`.

## Sustained gate

The selected configuration passed the 1,000-day release run at `N=10,000`. Alive population was **10,089–10,758** over days 100–1,000, inside the required 9,000–11,000 band. GiveFood found at least one target on all measured days in days 100–1,000. Every 50-day window had both births and deaths:

| Days | Alive at end | Births | Deaths | Days with GiveFood target |
|---:|---:|---:|---:|---:|
| 1–50 | 10,045 | 50 | 5 | 32 |
| 51–100 | 10,089 | 51 | 7 | 50 |
| 101–150 | 10,139 | 62 | 12 | 50 |
| 151–200 | 10,174 | 44 | 9 | 50 |
| 201–250 | 10,222 | 54 | 6 | 50 |
| 251–300 | 10,264 | 58 | 16 | 50 |
| 301–350 | 10,303 | 46 | 7 | 50 |
| 351–400 | 10,341 | 48 | 10 | 50 |
| 401–450 | 10,380 | 58 | 19 | 50 |
| 451–500 | 10,429 | 63 | 14 | 50 |
| 501–550 | 10,461 | 52 | 20 | 50 |
| 551–600 | 10,505 | 59 | 15 | 50 |
| 601–650 | 10,561 | 73 | 17 | 50 |
| 651–700 | 10,589 | 52 | 24 | 50 |
| 701–750 | 10,618 | 43 | 14 | 50 |
| 751–800 | 10,667 | 65 | 16 | 50 |
| 801–850 | 10,691 | 53 | 29 | 50 |
| 851–900 | 10,720 | 51 | 22 | 50 |
| 901–950 | 10,744 | 52 | 28 | 50 |
| 951–1,000 | 10,755 | 51 | 40 | 50 |

The CI-size run at `N=1,000`, `K=5` stayed within **1,010–1,076 alive** on every day from 100 through 1,000. The new debug test took **5.36 seconds** on this host, below the two-minute target. It asserts the per-day `[900, 1,100]` population band.

## Measurement method

- Release runs used the serial `run_m3p_storage_days` path, with metrics and events enabled and snapshots disabled. Initialization, graph creation, and context construction were outside the per-day timer; the same runner context and scratch were reused across days.
- Full-tick measurements covered executed days 100–400 inclusive. Each value is one uninstrumented runner call. The report gives median ± MAD across those daily samples and separates days on which `day % compaction_interval_days == 0` from other days.
- Per-phase profiles used temporary opt-in timers in `runner.rs` and `m3p.rs`, enabled only for the profile run. They were removed after measurement. The profile run's final M3P State hash matched the full-run State hash at both population sizes. Phase medians are secondary and exclude the time spent printing each timing record; the enclosing Phase 9b timer includes its temporary substage-log overhead.
- Memory is the process Working Set (`Get-Process ... WorkingSet64`) sampled after the graph and context were built and after days 100, 200, 300, and 400. It includes the runner and retained scratch, not only authoritative state.
- The equal-live comparison uses 11 timed samples after two warmups, three consecutive days per sample, with contexts constructed before timing. The M3P checkpoint at day 400 had the same live population as the M3 comparison state. Metrics and events were enabled in both paths.
- The measurement driver and timing hooks were temporary local files/changes; neither is retained in the repository.

## Full-tick timings and compaction intervals

Times are µs/tick, median ± MAD over executed days 100–400. Two complete runs were recorded per cell. The M3P alive range is over the same measured days.

| N | Interval | Full-run median ± MAD, run 1 | Run 2 | Normal-day median, run 2 | Compaction-day median, run 2 | Compaction samples, run 2 | Alive range |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | 10 | 9,575 ± 1,189 | 12,988 ± 3,027 | 12,871 | 15,986 | 31 | 10,094–10,341 |
| 10,000 | 50 | 9,180 ± 855 | 9,189 ± 1,079 | 9,180 | 10,289 | 7 | 10,094–10,341 |
| 10,000 | 200 | 9,214 ± 876 | 9,458 ± 1,333 | 9,432 | 12,310 | 2 | 10,094–10,341 |
| 100,000 | 10 | 181,000 ± 2,105 | 181,900 ± 3,293 | 181,265 | 200,144 | 31 | 100,932–103,245 |
| 100,000 | 50 | 181,383 ± 1,758 | 178,525 ± 7,807 | 177,819 | 190,693 | 7 | 100,932–103,241 |
| 100,000 | 200 | 179,441 ± 3,165 | 178,989 ± 4,019 | 178,810 | 226,642 | 2 | 100,932–103,237 |

The interval-200 compaction-day medians have only two samples and are not stable estimates. The two 10k interval-10 full-run medians differ materially; retain both measurements rather than claiming a precise interval effect.

## Per-phase profile, interval 50

Median ± MAD in µs over 301 instrumented days. Phase 4 is split into selection and intent generation. Phase 9b apply time excludes the separately timed projection refresh.

| Phase | 10k normal | 10k compaction | 100k normal | 100k compaction |
|---|---:|---:|---:|---:|
| Phase 1 regrowth | <1 | <1 | <1 | <1 |
| Phase 2 degradation | 8 ± 0 | 8 ± 0 | 107 ± 9 | 113 ± 4 |
| Phase 3 features | 33 ± 1 | 35 ± 1 | 367 ± 17 | 372 ± 12 |
| Phase 4 selection | 498 ± 6 | 498 ± 4 | 5,623 ± 290 | 5,810 ± 50 |
| Phase 4 intents | 178 ± 6 | 208 ± 21 | 3,878 ± 236 | 3,865 ± 112 |
| Phase 5 partition | 101 ± 10 | 97 ± 7 | 1,640 ± 197 | 1,663 ± 21 |
| Phase 6A work | 233 ± 9 | 247 ± 22 | 3,930 ± 424 | 3,924 ± 429 |
| Phase 6B targeted | 154 ± 12 | 155 ± 14 | 2,401 ± 282 | 2,386 ± 106 |
| Phase 7 market | 155 ± 15 | 163 ± 12 | 2,519 ± 277 | 2,382 ± 104 |
| Phase 8 welfare | 107 ± 8 | 106 ± 7 | 1,532 ± 184 | 1,559 ± 136 |
| Phase 9 mortality | 79 ± 2 | 78 ± 0 | 1,205 ± 153 | 1,221 ± 197 |
| Phase 9b apply total | 5,176 ± 554 | 6,357 ± 130 | 96,511 ± 7,688 | 115,449 ± 11,251 |
| Event staging | 132 ± 11 | 145 ± 7 | 1,326 ± 89 | 1,311 ± 33 |
| Phase 10 metrics | 263 ± 6 | 268 ± 10 | 4,173 ± 342 | 4,370 ± 1,060 |
| Phase 11 events | 181 ± 5 | 183 ± 8 | 2,537 ± 165 | 2,588 ± 294 |

Phase 1 was below the microsecond resolution of these timers. The full-tick measurements above are uninstrumented; phase values are a separate, non-additive profile.

## Phase 9b breakdown

Median ± MAD in µs, interval 50, days 100–400. The graph attachment row covers source-row staging and newborn edge sampling; canonical CSR reconstruction is shown separately. Storage rebuild is the SoA clear-and-append work on compaction days.

| Stage | 10k normal | 10k compaction | 100k normal | 100k compaction |
|---|---:|---:|---:|---:|
| Birth planning | 480 ± 36 | 496 ± 36 | 7,529 ± 533 | 7,181 ± 426 |
| Graph attachment | 721 ± 48 | 746 ± 60 | 9,755 ± 539 | 9,254 ± 625 |
| Compaction edge rebuild | 0 | 1,039 ± 38 | 0 | 16,453 ± 2,611 |
| Canonical CSR rebuild | 1,968 ± 461 | 1,988 ± 41 | 37,064 ± 2,667 | 38,281 ± 3,755 |
| Storage rebuild | 4 ± 0 | 140 ± 1 | 77 ± 22 | 3,127 ± 127 |
| Slot projection rebuild | 910 ± 165 | 911 ± 9 | 17,523 ± 1,452 | 17,112 ± 589 |

The normal-day graph rebuild still occurs because births append graph rows. At 100k, the Phase 9b apply median is about 96.5 ms on normal days and 115.4 ms on compaction days; projection rebuild adds about 17.5 ms on either class. The measured stage subtotals do not include all Phase 9b input copying and validation, so they do not sum to the enclosing phase.

## Memory over time

Interval-50 process Working Set in MiB; values are direct Windows Working Set samples after each checkpoint.

| N | Day 0 | Day 100 | Day 200 | Day 300 | Day 400 |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 8.1 | 17.4 | 17.2 | 17.9 | 11.7 |
| 100,000 | 28.9 | 80.6 | 82.4 | 82.9 | 83.6 |

Across all interval settings, the 100k Working Set samples stayed around 81–88 MiB after day 100. The 10k samples varied more, from about 12 to 19 MiB after day 100.

## Equal-live-population tick comparison

At the post-day-400 checkpoint with interval 50, an M3 storage world was built from the M3P live agents and run for the same three-day timed sample shape. This compares runtime cost at the same live population; the profiles have different graph and population-dynamics semantics.

| Initial N | Live agents | M3P median µs/tick | M3 median µs/tick | M3P / M3 |
|---:|---:|---:|---:|---:|
| 10,000 | 10,341 | 8,868 | 2,632 | 3.37× |
| 100,000 | 103,241 | 165,123 | 31,601 | 5.23× |

## Findings

**Measured facts:** the sustained config passed at 10k and the CI-sized 1k gate. During the days 100–400 profile, Phase 9b apply was the largest phase at 100k, around 96–115 ms/tick, and the daily CSR rebuild was about 37–38 ms. A compaction day added about 16 ms of edge cleanup plus about 3 ms of storage rebuild at 100k. Slot projection rebuild took about 17 ms and ran on normal days too because births changed the graph. Full-tick M3P medians were roughly 179–182 ms at 100k; the equal-live M3 path was about 31.6 ms/tick at the day-400 checkpoint.

**Inference:** M3-08's principal scaling cost on this workload is rebuilding canonical graph state and its slot projection for ongoing births, not the one-time initialization or the extra dead-edge cleanup alone. Compaction interval changes the count of expensive compaction days, but on this host the interval-10/50/200 whole-tick medians at 100k were close relative to run-to-run spread. The 200-day interval compaction samples are too few to rank reliably, and 10k interval-10 measurements were inconsistent across repeated runs.
