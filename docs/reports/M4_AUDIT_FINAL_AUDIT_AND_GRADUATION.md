# M4 Final Audit and Graduation
Status: current

M4-GRADUATE — `v0.5.0-m4`. Hosted Linux, Windows, and macOS CI passed on commit `5a4972c`.


## Research question and answer

> In a deterministic world, how precisely can cause and effect be separated?

| Level | Recorded answer | Caveats |
|---|---|---|
| Record | In the M3P sustained workload, food and health interventions reached about 99% normalized agent-record divergence within 365 days. [M4-03 H=365 results](M4_03_DIVERGENCE.md#h365-extension) | Five seeds, one M3P workload and one host; the result is specific to a static relationship graph and the tested horizon. |
| Aggregate | At H=365, M4-04's CRN efficiency ratios ranged from about 1.5× to 80× across the reported outcomes. For the health intervention, the mean wealth effect was concentrated in the target settlement: -3,320.8 world-wide versus -3,278.25 in the target settlement. [M4-04 results](M4_04_CRN_EFFICIENCY.md#health_0_01) | One workload and master-seed configuration, with 100 paired and 100 independent replicate identities; the ratios vary substantially by outcome and horizon. |
| Cause | Minimal causes existed for the tested predicate, but were non-unique and seed-dependent: one seed had 15 satisfying four-item sets; seeds 0 and 1 had no cause, while seed 3 had a three-item minimum. [M4-05 non-uniqueness](M4_05_MINIMAL_CAUSE.md#a-1-report-addendum-direct-full-set-effect-and-cause-non-uniqueness) | N=2,000, H=90, R=10, one candidate construction and an inclusive threshold whose result lies on the 0.1-step mean boundary. This does not identify a general causal law. |

These results answer the question for a small set of explicitly tested M3P workloads and interventions. They do not establish precision bounds for other models, intervention classes, or long-horizon environments.

## M4 delivery record

| Stage | Delivered | Evidence |
|---|---|---|
| M4-00 | Frozen counterfactual terminology, intervention framework, entity matching, and compatibility invariants. | [M4 contract §§1–9](../contracts/M4_COUNTERFACTUAL_CONTRACT.md) |
| M4-01 | Static system registry and generic WorldState/storage day loops; parity retained. | [System registry report](M4_01_SYSTEM_REGISTRY.md) |
| M4-02 | Snapshot-backed branch API, interventions, and first-divergence location. | [Branching report](M4_02_BRANCHING.md) |
| M4-03 | Daily divergence sets/curves, component flags, cascade summaries, and experiments. | [Divergence report](M4_03_DIVERGENCE.md) |
| M4-04 | Paired and independent CRN estimators, bootstrap intervals, and long-horizon results. | [CRN report](M4_04_CRN_EFFICIENCY.md) |
| M4-05 | Deterministic batch interventions, candidate construction, exhaustive/ddmin search, and logs. | [Minimal-cause report](M4_05_MINIMAL_CAUSE.md) |
| M4-06 | Registered settlement migration profiles, codecs, parity gates, and migration experiments. | [Migration report](M4_06_MIGRATION.md) |
| M4-07 | Opt-in cross-branch parallel evaluation, direct-column day scan, CoW decision, and GPU assessment. | [Many-branch report](M4_07_MANY_BRANCH.md) |
| Viewer | Local M4 branch comparison view and an exporter-verified sample trace. | [Viewer instructions](../../viewer/README.md#m4-branch-comparison), [sample JSONL](../data/m4_viewer/food_0_h90.jsonl), [exporter](../../crates/sim-model/examples/m4_branch_trace.rs) |

The frozen stage definitions are in the [M4 Counterfactual Contract](../contracts/M4_COUNTERFACTUAL_CONTRACT.md). The viewer reads committed JSONL locally; it makes no network requests.

## How M4 answered its question

1. **M4-01 registered systems behind generic day loops.** The loop architecture generalized, but per-profile state and codec wiring remained duplicated. [M4-01 report](M4_01_SYSTEM_REGISTRY.md)
2. **M4-02 made a branch reproducible and located its first divergence.** Snapshot-backed base and intervention runs established the comparison path; the A-5 gate eventually passed after its measurement protocol was corrected. [M4-02 report](M4_02_BRANCHING.md)
3. **M4-03 showed record-level divergence approaching 99%.** Food and health interventions reached about 99% normalized divergence at H=365 in the tested M3P workload. [M4-03 report](M4_03_DIVERGENCE.md)
4. **M4-04 showed that CRN retained aggregate signal where independent runs had substantial noise.** At H=365, ratios were about 1.5–80×. For health=0.01, the target-settlement wealth effect (-3,278.25) was nearly the world effect (-3,320.8); paired estimates distinguished both from zero, while independent world-level estimates had substantially more variance. [M4-04 report](M4_04_CRN_EFFICIENCY.md)
5. **M4-05 found minimal causes, but not one unique cause set.** The exhaustive case had 15 satisfying minimum-size sets, and the minimum changed or disappeared across seeds. [M4-05 report](M4_05_MINIMAL_CAUSE.md)
6. **M4-06 added settlement migration without editing generic loop bodies.** The forward gate passed, but profile-specific state/codec/context plumbing added 1,896 production lines outside the migration module. [M4-06 report](M4_06_MIGRATION.md)
7. **M4-07 made independent branch work parallel.** Six-thread measurements were 4.3–5.1× faster for M4-04 branch evaluation; restore plus clone was only 1.03–3.12% of branch time, so copy-on-write was not needed for the measured cells. [M4-07 report](M4_07_MANY_BRANCH.md)

## Engine findings

### M4-02 A-5 gate history

The gate required a sequence of corrections because short measurements were noisy on the Windows host; the earlier results remain in the [M4-02 report](M4_02_BRANCHING.md) and were not re-judged under later rules.

| Protocol | What changed and why |
|---|---|
| A-1 | Used a MAD/median ratio for paired deltas; this cannot pass when the delta is near zero because the ratio becomes unstable or unbounded. |
| A-2 | Used `median + 2×MAD`, which measures sample spread rather than uncertainty of the median. Its total-CPU validity rule also included the harness's own core. |
| A-3 | Introduced the bootstrap upper bound and per-process CPU accounting; the Windows process counter returned zero or lost its trace. |
| A-4 | Dropped the unreliable process counters and retained path-spread validity; short cells still exceeded the spread limit. |
| A-5 | Batches paired operations until each timed sample is at least one second, with a fixed repetition count selected during warmup. The final nine cells passed. |

The A-1 to A-3 changes were measurement-rule design errors, not simulation implementation costs. A-5's final table records host controls, 41 samples, medians/MADs, and passing results for all nine cells. [M4-02 final A-5 table](M4_02_BRANCHING.md#final-confirmatory-measurement-under-frozen-117-a-5)

### Parallel branches, CoW, and GPU

On the recorded Windows 11 / Ryzen 5 9600X host, six-thread M4-04 branch evaluation measured 4.25–5.11× speedup, and M4-05 case (a) search measured 4.30×, with exact serial/parallel outputs. These were single paired API invocations rather than A-5 performance gates. Restore plus day-start clone was 1.03–3.12% of branch time in the four measured cells, below the frozen 20% threshold; copy-on-write was therefore recorded as **not needed** for those workloads. [M4-07 report](M4_07_MANY_BRANCH.md)

The GPU work was a written assessment only. Independent branches and subsets expose coarse-grained parallel work, but no GPU transfer or accelerator benchmark was run. Ordered Phase 6B, deterministic scalar math, reductions, and divergent births leave determinism and synchronization costs unresolved; the available evidence does not support a GPU implementation. [M4-07 GPU assessment](M4_07_MANY_BRANCH.md#gpu-suitability-assessment)

## Debts and limits

- **Registry extensibility is incomplete.** M4-01 generalized the day loop but not per-profile state, snapshot codec, digest, and context wiring: production wiring is 2,775 lines versus the 2,070-line baseline. M4-06 added 1,896 production lines outside its system module, primarily profile codecs, digest domains, contexts, and config. [M4-01 known debt](M4_01_SYSTEM_REGISTRY.md#known-debt), [M4-06 forward-gate inventory](M4_06_MIGRATION.md#implementation-and-forward-gate)
- **The M4-02 branch profile list was initially hard-coded.** The M4-06 follow-up derived supported migration combinations from the registry and `SimulationSystems`; this was corrected after migration profiles were added. [M4-06 branch API follow-up](M4_06_MIGRATION.md#branch-api-rerun-with-the-registered-migration-system)
- **Migration parameters remain uncalibrated.** In one experiment, probability 0.25 and threshold 0 produced a median 113,644 moves over 365 days (about 311/day, roughly 3% of the initial population per day). With probability 0.02, threshold 0.2, cost 10, and cooldown 30, the five-seed branch experiment made zero moves. [M4-06 migration results](M4_06_MIGRATION.md#population-distribution-and-money), [threshold follow-up](M4_06_MIGRATION.md#branch-api-rerun-with-the-registered-migration-system)
- **The M4-05 predicate sat on its replicate-mean boundary.** At R=10, integer population effects yield 0.1 increments; observed effects equal to -1.0 satisfy the inclusive `<= -1` predicate. [M4-05 boundary note](M4_05_MINIMAL_CAUSE.md#a-1-report-addendum-direct-full-set-effect-and-cause-non-uniqueness)
- **M3P compaction removes dead agents' wealth.** Agent-plus-treasury money is therefore not conserved across death cleanup; the migration transfer itself remains balanced. [M4-06 money note](M4_06_MIGRATION.md#population-distribution-and-money)
- **Newborn CRN alignment can be lost at long horizons.** M4-04 measured mean newborn CRN loss of 33.02 at H=365 for the global BaseMetabolicCost intervention because AgentId allocation diverged. M3P RNG identity remains unchanged. [M4-04 follow-up](M4_04_CRN_EFFICIENCY.md#follow-ups)
- **Harness and raw-data recovery is incomplete.** [The M4 inventory](../data/m4/INVENTORY.md) records M4-02 A-5 samples as unavailable, with some earlier sources only partially recoverable; M4-03/04/05 principal raw experiment data survives, but some source files were restored from session history or remain partial. Several M2/M3 phase tables also depended on temporary profiling probes explicitly removed after measurement, including M2-40 allocation/profile probes and M3-01, M3-08, and M3-09 phase timers. Their reports retain the summarized results, but not every raw sample or original probe. [M2-40 validation record](M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#23-validation), [M3-01 method](M3_01_SERIAL_BASELINE.md#host-and-method), [M3-08 method](M3_08_POPULATION_DYNAMICS.md#measurement-method), [M3-09 follow-up](M3_09_INCREMENTAL_TOPOLOGY.md#follow-up-m3p-phase-9b-materialization-reduction)
- **External validity is limited.** Most M4 measurements used one Windows host, one M3P sustained workload, and a small number of master seeds. Replicate counts improve estimator precision within that workload but do not substitute for additional workloads and models.

## M5 proposal: Reproducibility and research hardening

**Decision:** make reproducibility and research hardening the next milestone, before widening the model or adding inference features.

- Regenerate M0–M4 report numbers from committed code, requiring bit-exact results wherever execution is deterministic.
- Add PyO3 bindings and a Python experiment runner.
- Commit raw data and generate report tables and plots from that data.
- Rerun across multiple seeds and workloads with effect-size-first design, including migration calibration.
- Add a literature review and position the project against existing simulation and counterfactual systems.
- Clean up documentation: consistent names, summary-first reports, per-stage contract files, and a newcomer-focused README.
- Draft a technical report from the reproducible evidence.

Later candidates, not part of the M5 commitment: inverse inference, distributed determinism across machines, and learning agents.

## Graduation gate

Hosted Linux, Windows, and macOS CI passed on graduation commit `5a4972c`. M4 is graduated as `v0.5.0-m4`.
