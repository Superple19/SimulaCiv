# M3 Final Runtime Profile and Graduation Gate

**Decision: M3-GRADUATE**
**Scope:** M3-00 through M3-11 complete; spatial migration remains deferred without a stage number.
**Evidence:** committed M3 contract, parity tests, and performance reports listed below.

## Executive summary

M3 now has a deterministic reference and optimized serial paths for relationship-graph interactions, optional explicit parallel execution for Phase 6B and propagation, dynamic M3P population/topology maintenance, and explicit feature dispatch. Existing M2/M3/M3P behavior is preserved by separate profiles, digest domains, snapshots, and parity gates. Stage completion and evidence are recorded in [§8 of the M3 Interaction Contract](../contracts/M3_INTERACTION_CONTRACT.md#8-m3-stage-plan-planned--not-frozen).

The data supports graduating M3 without an M4 architecture change for the measured workloads. This is a scoped conclusion: M3 static-world profiling reached one million agents, M3P sustained profiling reached one hundred thousand, and M3-11 propagation tested multi-hop spread on both populations. The high-K scale extension, M3P/propagation whole-tick variance, and deferred spatial migration remain follow-up boundaries rather than evidence that the current architecture must be replaced. [M3-05 Scale Limit](M3_SCALE_LIMIT.md#executive-summary), [M3-09 Incremental Topology](M3_09_INCREMENTAL_TOPOLOGY.md#follow-up-m3p-phase-9b-materialization-reduction), [M3-11 Propagation](M3_11_PROPAGATION.md#summary)

## Scope and stage record

M3-00 through M3-11 are complete. Spatial migration, previously M3-03, remains deferred and unnumbered. The optional contention dial remains an unselected future stage and would need its own contract amendment. [M3 Interaction Contract §8](../contracts/M3_INTERACTION_CONTRACT.md#8-m3-stage-plan-planned--not-frozen)

| Stage | Result | Evidence |
|---|---|---|
| M3-00 reference workload | Complete | [Frozen contract and reference gates](../contracts/M3_INTERACTION_CONTRACT.md#1-baseline-and-terminology) |
| M3-01 optimized serial path | Complete; exact reference parity | [Serial baseline](M3_01_SERIAL_BASELINE.md) |
| M3-02 parallel Phase 6B | Complete; explicit opt-in, serial remains default | [Parallel Phase 6B](M3_02_PARALLEL_PHASE6B.md) |
| M3-03 search/bookkeeping reduction | Complete; explicit serial opt-in | [Search reduction](M3_02B_SEARCH_REDUCTION.md) |
| M3-04 equilibrium workload | Complete; benchmark-only configuration | [Equilibrium](M3_EQUILIBRIUM_WORKLOAD.md), [sustained workload](M3_SUSTAINED_WORKLOAD.md) |
| M3-05 scale limit search | Complete; measurement-only | [Scale limit](M3_SCALE_LIMIT.md) |
| M3-06 settlement indexing | Complete | [Settlement index](M3_SETTLEMENT_INDEX.md) |
| M3-07 residual scaling diagnosis | Complete; measurement-only | [Scale residual](M3_07_SCALE_RESIDUAL.md) |
| M3-08 population dynamics | Complete; M3P births, cleanup, snapshots, sustained gate | [Population dynamics](M3_08_POPULATION_DYNAMICS.md) |
| M3-09 incremental topology | Complete; incremental M3P append path | [Incremental topology and follow-up](M3_09_INCREMENTAL_TOPOLOGY.md) |
| M3-10 feature switches | Complete; explicit configured dispatch | [Frozen feature-switch contract](../contracts/M3_INTERACTION_CONTRACT.md#15-m3-10-feature-switches) |
| M3-11 propagation | Complete; reference, storage, M3P, snapshot-v3, and explicit parallel paths | [Propagation profile](M3_11_PROPAGATION.md) |

## Correctness and compatibility record

- M2 goldens, snapshots, and behavior remain unchanged. M3 and M3P use separate profile digests and committed goldens; M3P propagation adds its own golden checkpoints. The M3-01 and M3-11 reports record reference/storage differential and snapshot-resume parity. [M3-01 release differential and parity](M3_01_SERIAL_BASELINE.md#release-differential-and-parity), [M3-11 golden and parity tables](M3_11_PROPAGATION.md#m3-11-golden-daily-spread)
- M3-01 records its complete 100-day CI differential matrix, snapshot pause/resume, and shuffled-slot parity. M3-02 records extended release parity across the frozen matrix and chunk sweep. [M3-01 serial baseline](M3_01_SERIAL_BASELINE.md), [M3-02 extended release parity](M3_02_PARALLEL_PHASE6B.md#extended-release-parity)
- M3-08 and M3-09 preserve birth, compaction, incremental topology, snapshot, and sustained-population parity. M3-11 tests compare daily propagation records across reference, serial storage, and parallel storage, including M3P births, compaction, and schema-v3 resume. [M3-08 sustained gate](M3_08_POPULATION_DYNAMICS.md#sustained-gate), [M3-09 test runtime and parity](M3_09_INCREMENTAL_TOPOLOGY.md#test-runtime), [M3-11 implementation gates](M3_11_PROPAGATION.md#workload-and-method)
- The CI workflow runs formatting, Clippy, and the workspace test suite on Linux, Windows, and macOS. This local graduation run was executed on Windows; hosted CI results for the latest unpushed commits were not available in this task. [CI workflow](../../.github/workflows/ci.yml)

## Runtime and scaling evidence

The static M3 scale suite completed through one million agents without capacity or runner errors, but it also reported first-break criteria in the 100k-to-250k interval and a graph-plus-projection setup cost above ten day-0 ticks for the 100k, degree-16 case. The later noise-controlled residual run found that the earlier phase-alpha outliers above two did not reproduce as reliable estimates: those cells exceeded the report's five-percent MAD gate. Reliable P2 and P9 intervals were near linear; the other noisy phase intervals remain inconclusive. This means M3 supports the tested scale, while high-K/setup cost is still measurable engineering debt. [M3-05 executive summary and setup table](M3_SCALE_LIMIT.md#executive-summary), [M3-07 re-measured scaling alphas](M3_07_SCALE_RESIDUAL.md#remeasured-scaling-alphas)

The M3-09 follow-up reduced normal-day Phase 9b at one hundred thousand agents to **1.375 ms median** (hook-timed range 1.269–1.482 ms). In the equal-live rerun, incremental M3P/M3 measured **1.49×** for the paired schedule and **1.47×** non-interleaved. These are single-host measurements; the phase value includes temporary instrumentation, and the runner comparison uses its documented three-day sample method. [M3-09 incremental append path](M3_09_INCREMENTAL_TOPOLOGY.md#incremental-append-path), [equal-live M3P/M3 rerun](M3_09_INCREMENTAL_TOPOLOGY.md#equal-live-m3pm3-rerun-and-interleaving)

## M3-11 answer: propagation and parallel spread

**Measured facts:** the fixed-topology M3 grid first showed lower Phase 9c medians on a processed frontier of 728 sources for pools 2/4 and 852 for pool 6. At one hundred thousand agents, degree 16, sixteen hops, and transmit probability 0.2, a level reached 29,753 sources; Phase 9c measured 208.693 ms serial versus 65.283 ms on pool 6. Whole-tick speedup for that case was 2.262× interleaved and 2.294× non-interleaved. [M3-11 Phase 9c measurements](M3_11_PROPAGATION.md#phase-9c-measurements), [whole-tick measurements](M3_11_PROPAGATION.md#whole-tick-measurements)

The sustained M3P extension also measured a clear Phase 9c gain at one hundred thousand agents and sixteen hops: the largest frontier was 8,124, and pool 6 measured 38.602 ms against 86.331 ms serial. Its whole-tick speedups were 1.144× interleaved and 1.134× non-interleaved. At four hops the Phase 9c gain was smaller, and some whole-tick pool/cell medians regressed. Whole-tick MADs were high in several cells. [M3-11 M3P sustained-population extension](M3_11_PROPAGATION.md#m3p-sustained-population-extension)

**Answer:** yes, parallel spread beats serial for Phase 9c when propagation produces sufficiently large frontiers. The measured crossover depends on the frontier work and host; these results do not define a portable threshold. Whole-tick speedup is less consistent because spread is only one phase and the surrounding runner work stays serial. Serial remains the default; parallel propagation is an explicit opt-in. This is a measured answer for the tested M3 and M3P configurations, not a guarantee for other graph densities or hosts. [M3-11 summary and interpretation](M3_11_PROPAGATION.md#summary), [M3P extension interpretation](M3_11_PROPAGATION.md#m3p-sustained-population-extension)

## Does the runtime need a new architecture?

**Inference:** the evidence does not show that the M3 execution model or data architecture must fundamentally change. Segmented SoA storage, transient AgentId-to-slot projections, indexed settlement lookups, incremental M3P topology updates, and deterministic per-level propagation cover the measured graph, population, and parallel workloads. The data instead identifies bounded improvements and extensions: graph/projection setup at scale, high-K behavior, one-host performance variance, and workloads with spatial migration. M4 is not justified by the current results; it can be reconsidered if a later workload demonstrates a limit that these structures cannot address. [M3-05 Scale Limit](M3_SCALE_LIMIT.md), [M3-07 Scale Residual](M3_07_SCALE_RESIDUAL.md), [M3-09 follow-up](M3_09_INCREMENTAL_TOPOLOGY.md), [M3-11 Propagation](M3_11_PROPAGATION.md)

## Limitations and remaining follow-up

- Performance reports use one Windows host. The newer M3P propagation run had variable whole-tick MADs, and no post-commit Linux/macOS CI run was observed locally. The workflow remains configured for all three operating systems. [M3-11 method and noise note](M3_11_PROPAGATION.md#workload-and-method), [CI workflow](../../.github/workflows/ci.yml)
- M3 static scale testing reached one million agents, but the M3P propagation performance extension covered the sustained configuration through one hundred thousand agents only. The fixed-topology and M3P cells use different configurations and should not be combined into a single scaling curve. [M3-05 Scale Limit](M3_SCALE_LIMIT.md#host-configuration-and-method), [M3-11 M3P extension](M3_11_PROPAGATION.md#m3p-sustained-population-extension)
- Spatial migration remains deferred. Reactivating it requires a contract that defines membership changes, graph updates, deterministic partition changes, and snapshot behavior. The optional contention dial is also unselected and requires its own amendment. [M3 Interaction Contract §8](../contracts/M3_INTERACTION_CONTRACT.md#8-m3-stage-plan-planned--not-frozen)

## Graduation record

**M3-GRADUATE.** M3-00 through M3-11 are complete, their documented correctness gates passed locally, and the measured evidence does not identify an architectural or correctness blocker. Serial remains the default; explicit parallel execution remains opt-in. Spatial migration is deferred. This audit does not create a new version or tag.
