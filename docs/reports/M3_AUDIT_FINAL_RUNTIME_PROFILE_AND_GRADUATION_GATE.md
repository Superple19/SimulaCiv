# M3 Final Runtime Profile and Graduation Gate
Status: current


**Decision: M3-GRADUATE**
**Scope:** M3-00 through M3-11 complete; spatial migration remains deferred without a stage number.
**Evidence:** committed M3 contract, parity tests, and performance reports listed below.

## Executive summary

M3 now has a deterministic reference and optimized serial paths for relationship-graph interactions, optional explicit parallel execution for Phase 6B and propagation, dynamic M3P population/topology maintenance, and explicit feature dispatch. Existing M2/M3/M3P behavior is preserved by separate profiles, digest domains, snapshots, and parity gates. The stage record and evidence links follow.

The measurements support graduating M3 without replacing the architecture for the workloads tested. Hosted CI passed on Linux, Windows, and macOS for graduation commit `f568376c509040cda4cccdd87e888e739a5514dd` ([Actions run 39194209453](https://github.com/Superple19/SimulaCiv/actions/runs/39194209453)). M3 static-world profiling completed through one million agents, M3P sustained profiling reached one hundred thousand, and M3-11 measured multi-hop propagation on both populations. High-K scaling, M3P/propagation whole-tick variance, and deferred spatial migration remain limitations. [M3-05 Scale Limit](M3_05_SCALE_LIMIT.md#executive-summary), [M3-08 Population Dynamics](M3_08_POPULATION_DYNAMICS.md#full-tick-timings-and-compaction-intervals), [M3-11 Propagation](M3_11_PROPAGATION.md#summary)

## Milestone questions

| Milestone | Question | One-line answer | Recorded evidence |
|---|---|---|---|
| M0 | Is simulation replay reproducible? | Yes: deterministic reference execution and canonical replay establish the baseline. | [M2-40 final gate](M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#1-baseline-and-objective), [M3-01 parity](M3_01_SERIAL_BASELINE.md#release-differential-and-parity) |
| M1 | Can semantics be frozen before optimization? | Yes: subsequent optimized paths are checked against the frozen canonical behavior. | [M2-40 correctness gate](M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#20-graduation-criteria) |
| M2 | Can runtime speed improve while canonical behavior stays exact? | Yes: the SoA runtime and explicit parallel option preserve digests and snapshots through the measured 100k range. | [M2-40 executive summary](M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#executive-summary) |
| M3 | When does interaction complexity force an execution-model or data-architecture change? | Static structures held to 1M agents; dynamic topology broke the rebuild-per-change design (M3P/M3 5.23x at 100k), and incremental maintenance fixed it without replacement (1.49x). Parallelism paid off only when per-level work was large (propagation). | [M3-05 scale results](M3_05_SCALE_LIMIT.md#executive-summary), [M3-11 propagation](M3_11_PROPAGATION.md#summary), [M3-08 equal-live comparison](M3_08_POPULATION_DYNAMICS.md#equal-live-population-tick-comparison), [M3-09 follow-up](M3_09_INCREMENTAL_TOPOLOGY.md#follow-up-m3p-phase-9b-materialization-reduction) |

## How M3 answered its question

1. **Static world:** the scale search completed its static M3 workload through one million agents. It also flagged first-break criteria beginning in the 100k-to-250k interval, so completion at one million is not a claim of linear scaling at every size. [M3-05 Scale Limit](M3_05_SCALE_LIMIT.md#executive-summary)
2. **Small parallel work and search savings:** parallel Phase 6B measured a `0.967×` median whole-tick speedup, and search reduction produced no paired whole-tick saving beyond MAD. **Inference:** the per-entry work and skipped target-search work were too small to repay parallel dispatch or search setup on those workloads. [M3-02 Parallel Phase 6B](M3_02_PARALLEL_PHASE6B.md#executive-summary), [M3-03 Search Reduction](M3_03_SEARCH_REDUCTION.md#executive-summary)
3. **Settlement lookup debt:** the scale diagnosis identified repeated settlement-keyed linear scans whose per-partition work grows with K, yielding an O(K²) family when K partitions each scan K settlements. M3-06 replaced these scans with indexed lookups. At one million agents, the paired serial whole-tick median was `17.7%` lower after the change; the measurement report cautions that the entire difference cannot be attributed to indexing alone. [M3 Scale Diagnosis](M3_05_SCALE_DIAGNOSIS.md#settlement-keyed-lookup-work-per-day), [M3 Settlement Index](M3_06_SETTLEMENT_INDEX.md#whole-tick-result-at-n1000000)
4. **Residual locality:** after indexing, the controlled residual profile found large layout-sensitive slowdowns in Phase 4 and noisy phase-level alpha outliers; many high alphas did not pass the report's MAD reliability gate. **Inference:** access pattern and locality explain some residual cost, but the report does not attribute every noisy phase to a specific cache or TLB effect. [M3-07 Scale Residual](M3_07_SCALE_RESIDUAL.md#normal-versus-reversed-slot-layout), [re-measured alphas](M3_07_SCALE_RESIDUAL.md#remeasured-scaling-alphas)
5. **Dynamic topology:** births invalidated the static design's assumption that the graph and slot projection could be rebuilt cheaply after every change. At equal live population of 103,241, the M3-08 M3P/M3 tick ratio was `5.23×` at 100k. [M3-08 Population Dynamics](M3_08_POPULATION_DYNAMICS.md#equal-live-population-tick-comparison)
6. **Incremental maintenance:** M3-09 appended graph/projection rows between compactions; its follow-up reduced normal-day Phase 9b to `1.375 ms` median and the equal-live M3P/M3 ratio to `1.49×` paired. [M3-09 incremental append path](M3_09_INCREMENTAL_TOPOLOGY.md#incremental-append-path), [follow-up equal-live comparison](M3_09_INCREMENTAL_TOPOLOGY.md#equal-live-m3pm3-rerun-and-interleaving)
7. **Propagation parallelism:** among the M3 interaction workloads compared here, M3-11 was the first to show a large parallel win: about `3.2×` Phase 9c and `2.26×` whole-tick speedup at a large frontier. **Inference:** this is the other side of M3-02's result: parallelizing tiny keyed entries did not repay coordination, while processing a sufficiently large propagation level did. [M3-11 Phase 9c measurements](M3_11_PROPAGATION.md#phase-9c-measurements), [whole-tick measurements](M3_11_PROPAGATION.md#whole-tick-measurements)

## Scope and stage record

M3-00 through M3-11 are complete. Spatial migration, previously M3-03, remains deferred and unnumbered. The optional contention dial remains unselected. [M3 Interim Audit](M3_AUDIT_INTERIM_AUDIT.md), [M3-02 Parallel Phase 6B](M3_02_PARALLEL_PHASE6B.md)

| Stage | Result | Evidence |
|---|---|---|
| M3-00 reference workload | Complete | [M3-01 serial baseline and reference gates](M3_01_SERIAL_BASELINE.md#release-differential-and-parity) |
| M3-01 optimized serial path | Complete; exact reference parity | [Serial baseline](M3_01_SERIAL_BASELINE.md) |
| M3-02 parallel Phase 6B | Complete; explicit opt-in, serial remains default | [Parallel Phase 6B](M3_02_PARALLEL_PHASE6B.md) |
| M3-03 search/bookkeeping reduction | Complete; explicit serial opt-in | [Search reduction](M3_03_SEARCH_REDUCTION.md) |
| M3-04 equilibrium workload | Complete; benchmark-only configuration | [Equilibrium](M3_04_EQUILIBRIUM_WORKLOAD.md), [sustained workload](M3_04_SUSTAINED_WORKLOAD.md) |
| M3-05 scale limit search | Complete; measurement-only | [Scale limit](M3_05_SCALE_LIMIT.md) |
| M3-06 settlement indexing | Complete | [Settlement index](M3_06_SETTLEMENT_INDEX.md) |
| M3-07 residual scaling diagnosis | Complete; measurement-only | [Scale residual](M3_07_SCALE_RESIDUAL.md) |
| M3-08 population dynamics | Complete; M3P births, cleanup, snapshots, sustained gate | [Population dynamics](M3_08_POPULATION_DYNAMICS.md) |
| M3-09 incremental topology | Complete; incremental M3P append path | [Incremental topology and follow-up](M3_09_INCREMENTAL_TOPOLOGY.md) |
| M3-10 feature switches | Complete; explicit configured dispatch | — |
| M3-11 propagation | Complete; reference, storage, M3P, snapshot-v3, and explicit parallel paths | [Propagation profile](M3_11_PROPAGATION.md) |

## Correctness and compatibility record

- M2 goldens, snapshots, and behavior remain unchanged. M3 and M3P use separate profile digests and committed goldens; M3P propagation adds its own golden checkpoints. The M3-01 and M3-11 reports record reference/storage differential and snapshot-resume parity. [M3-01 release differential and parity](M3_01_SERIAL_BASELINE.md#release-differential-and-parity), [M3-11 golden and parity tables](M3_11_PROPAGATION.md#m3-11-golden-daily-spread)
- M3-01 records its complete 100-day CI differential matrix, snapshot pause/resume, and shuffled-slot parity. M3-02 records extended release parity across the frozen matrix and chunk sweep. [M3-01 serial baseline](M3_01_SERIAL_BASELINE.md), [M3-02 extended release parity](M3_02_PARALLEL_PHASE6B.md#extended-release-parity)
- M3-08 and M3-09 preserve birth, compaction, incremental topology, snapshot, and sustained-population parity. M3-11 tests compare daily propagation records across reference, serial storage, and parallel storage, including M3P births, compaction, and schema-v3 resume. [M3-08 sustained gate](M3_08_POPULATION_DYNAMICS.md#sustained-gate), [M3-09 test runtime and parity](M3_09_INCREMENTAL_TOPOLOGY.md#test-runtime), [M3-11 implementation gates](M3_11_PROPAGATION.md#workload-and-method)
- Formatting, Clippy, and the workspace test suite passed locally on Windows. Hosted CI passed on Linux, Windows, and macOS for graduation commit `f568376c509040cda4cccdd87e888e739a5514dd` ([Actions run 39194209453](https://github.com/Superple19/SimulaCiv/actions/runs/39194209453)).

## Runtime and scaling evidence

The static M3 scale suite completed through one million agents without capacity or runner errors, but it also reported first-break criteria in the 100k-to-250k interval and a graph-plus-projection setup cost above ten day-0 ticks for the 100k, degree-16 case. The later noise-controlled residual run found that the earlier phase-alpha outliers above two did not reproduce as reliable estimates: those cells exceeded the report's five-percent MAD gate. Reliable P2 and P9 intervals were near linear; the other noisy phase intervals remain inconclusive. This means M3 supports the tested scale, while high-K/setup cost is still measurable engineering debt. [M3-05 executive summary and setup table](M3_05_SCALE_LIMIT.md#executive-summary), [M3-07 re-measured scaling alphas](M3_07_SCALE_RESIDUAL.md#remeasured-scaling-alphas)

The M3-09 follow-up reduced normal-day Phase 9b at one hundred thousand agents to **1.375 ms median** (hook-timed range 1.269–1.482 ms). In the equal-live rerun, incremental M3P/M3 measured **1.49×** for the paired schedule and **1.47×** non-interleaved. These are single-host measurements; the phase value includes temporary instrumentation, and the runner comparison uses its documented three-day sample method. [M3-09 incremental append path](M3_09_INCREMENTAL_TOPOLOGY.md#incremental-append-path), [equal-live M3P/M3 rerun](M3_09_INCREMENTAL_TOPOLOGY.md#equal-live-m3pm3-rerun-and-interleaving)

## M3-11 answer: propagation and parallel spread

**Measured facts:** the fixed-topology M3 grid first showed lower Phase 9c medians on a processed frontier of 728 sources for pools 2/4 and 852 for pool 6. At one hundred thousand agents, degree 16, sixteen hops, and transmit probability 0.2, a level reached 29,753 sources; Phase 9c measured 208.693 ms serial versus 65.283 ms on pool 6. Whole-tick speedup for that case was 2.262× interleaved and 2.294× non-interleaved. [M3-11 Phase 9c measurements](M3_11_PROPAGATION.md#phase-9c-measurements), [whole-tick measurements](M3_11_PROPAGATION.md#whole-tick-measurements)

The sustained M3P extension also measured a clear Phase 9c gain at one hundred thousand agents and sixteen hops: the largest frontier was 8,124, and pool 6 measured 38.602 ms against 86.331 ms serial. Its whole-tick speedups were 1.144× interleaved and 1.134× non-interleaved. At four hops the Phase 9c gain was smaller, and some whole-tick pool/cell medians regressed. Whole-tick MADs were high in several cells. [M3-11 M3P sustained-population extension](M3_11_PROPAGATION.md#m3p-sustained-population-extension)

**Answer:** yes, parallel spread beats serial for Phase 9c when propagation produces sufficiently large frontiers. The measured crossover depends on the frontier work and host; these results do not define a portable threshold. Whole-tick speedup is less consistent because spread is only one phase and the surrounding runner work stays serial. Serial remains the default; parallel propagation is an explicit opt-in. This is a measured answer for the tested M3 and M3P configurations, not a guarantee for other graph densities or hosts. [M3-11 summary and interpretation](M3_11_PROPAGATION.md#summary), [M3P extension interpretation](M3_11_PROPAGATION.md#m3p-sustained-population-extension)

## Does the runtime need a new architecture?

**Measured-scope conclusion:** no architecture replacement is required for the measured workloads. Segmented SoA storage, transient AgentId-to-slot projections, indexed settlement lookups, incremental M3P topology updates, and deterministic per-level propagation cover the tested graph and population workloads. [M3-05 Scale Limit](M3_05_SCALE_LIMIT.md), [M3-07 Scale Residual](M3_07_SCALE_RESIDUAL.md), [M3-09 follow-up](M3_09_INCREMENTAL_TOPOLOGY.md), [M3-11 Propagation](M3_11_PROPAGATION.md)

**Proposed M4 research direction — not frozen:** study deterministic counterfactual branching using coordinate-based RNG as common random numbers. Build it on an extensible system-registration architecture that generalizes the M3-10 feature switches, with settlement migration as the first plugin-style feature. This is a proposal, not a conclusion measured by M3 or an implementation commitment.

## Limitations and remaining follow-up

- Performance reports use one Windows host. The newer M3P propagation run had variable whole-tick MADs. [M3-11 method and noise note](M3_11_PROPAGATION.md#workload-and-method)
- M3 static scale testing reached one million agents, but the M3P propagation performance extension covered the sustained configuration through one hundred thousand agents only. The fixed-topology and M3P cells use different configurations and should not be combined into a single scaling curve. [M3-05 Scale Limit](M3_05_SCALE_LIMIT.md#host-configuration-and-method), [M3-11 M3P extension](M3_11_PROPAGATION.md#m3p-sustained-population-extension)
- Spatial migration remains deferred. The optional contention dial is also unselected. [M3 Interim Audit](M3_AUDIT_INTERIM_AUDIT.md), [M3-02 Parallel Phase 6B](M3_02_PARALLEL_PHASE6B.md)

## Graduation record

**M3-GRADUATE.** M3-00 through M3-11 are complete, their documented correctness gates passed locally and in hosted CI, and the measured evidence does not identify an architectural or correctness blocker. Serial remains the default; explicit parallel execution remains opt-in. Spatial migration is deferred.

**Hosted CI record:** graduation commit `f568376c509040cda4cccdd87e888e739a5514dd` passed all three operating-system jobs in [Actions run 39194209453](https://github.com/Superple19/SimulaCiv/actions/runs/39194209453).

Later update (2026-10-21): spatial migration was delivered as a registered system in M4-06; see [M4-06 report](M4_06_MIGRATION.md). The text above records the status at the time of this document.
