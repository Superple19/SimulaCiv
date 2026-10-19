# M4 Counterfactual Contract

**Status: DRAFT — NOT FROZEN**

**Scope:** M4-00 research question, terminology, invariants, and planned stage boundaries. This draft does not authorize implementation.

**Baseline:** M3 graduated at `v0.4.0-m3`; see the [M3 Final Runtime Profile and Graduation Gate](../performance/M3_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md).

## 1. Research question

> **In a deterministic world, how precisely can cause and effect be separated?**

Counterfactual runs can begin from the same canonical snapshot, apply different interventions at explicit phase boundaries, and compare their later logical states. The M1 contract freezes a stateless PRNG addressed by `(master_seed, replicate_id, day, phase, subsystem_id, AgentId, draw_index)`; `RngCoordinate` and `coordinate_prng_u64` / `coordinate_prng_f32` implement that coordinate mapping. For each coordinate evaluated by both runs, the same run identity yields the same random value regardless of evaluation order. This supports common random numbers for paired branches, while a branch-dependent change in which coordinates are evaluated can still change which draws affect outcomes ([M1 Contract Freeze, C04](M1_CONTRACT_FREEZE.md); [`prng.rs`](../../crates/sim-core/src/prng.rs)).

The runtime also has canonical SHA-256 encodings for state, metrics, and events (`canonical_state_hash`, `canonical_metrics_hash`, `canonical_event_hash` and profile-specific variants), plus canonical snapshot encode/restore functions. These provide exact comparison and resume boundaries; the M3 contract and graduation report record the separate M2/M3/M3P/propagation profiles and their parity gates ([`hashing.rs`](../../crates/sim-model/src/hashing.rs); [`snapshot.rs`](../../crates/sim-model/src/snapshot.rs); [M3 Interaction Contract](M3_INTERACTION_CONTRACT.md); [M3 graduation report](../performance/M3_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)).

This combination makes paired causal experiments plausible, but it does not by itself define intervention semantics, phase-level observation, branch storage, or how to match agents created on only one branch. Those are open design decisions in §7.

## 2. Terminology

These definitions are proposed for M4 and remain subject to review.

- **Base run:** execution from a declared initial state or restored snapshot, with a fixed configuration, master seed, replicate ID, and horizon, and with no M4 interventions.
- **Snapshot boundary:** a point at which the runner emits canonical snapshot bytes for a defined resume cursor. Existing snapshot encoders serialize canonical logical state and metadata; the Phase 11 hook emits a snapshot only when its boundary flag is true ([`encode_snapshot`, `decode_snapshot`, `phase11_snapshot_if_boundary`](../../crates/sim-model/src/snapshot.rs)). M4 must define whether intervention application occurs immediately before or after the boundary.
- **Branch:** an isolated continuation from the same base-run snapshot and run identity, with a declared intervention set. “Isolated” is a semantic requirement; the storage/copy strategy is unresolved.
- **Intervention:** an explicit, typed change applied at a defined `(day, phase boundary)`. Candidate examples are a configuration parameter override, an `AgentId` field override, or a forced action for one agent on one day. An intervention may change model inputs or state only according to its declared type; it must not redefine the frozen RNG coordinate tuple or remap existing draws ([M1 C04](M1_CONTRACT_FREEZE.md); [`RngCoordinate`](../../crates/sim-core/src/prng.rs)).
- **Divergence point:** the earliest `(day, phase)` at which the branch's canonical logical state differs from the base run. Current day-run outcomes expose daily results (`M3DayOutcome` contains `executed_day`, optional metrics, events, and an optional snapshot), not digest fields or a per-phase digest trace. Canonical state hash functions can hash a supplied state at a checkpoint, and ordinary parity gates compare day checkpoints; exact first-phase detection would need a phase-boundary recorder or callback that captures comparable canonical state digests after each phase ([`M3DayOutcome`, `run_m3_reference_day`, `run_m3_storage_days`](../../crates/sim-model/src/runner.rs); [`canonical_state_hash`, `canonical_m3_state_hash`](../../crates/sim-model/src/hashing.rs); [M3 parity gates](M3_INTERACTION_CONTRACT.md)).
- **Divergence set:** for day `d`, `D_d` is the set of `AgentId`s whose canonical agent record differs between branch and base state at the agreed day boundary. Records are compared by logical `AgentId`, not physical `DenseSlot`; canonical M1 state encoding sorts agents by `AgentId` ([`canonical_state_bytes`](../../crates/sim-model/src/hashing.rs); [M1 C01/C03](M1_CONTRACT_FREEZE.md)). Births make cross-branch identity matching an open question (§7).
- **Divergence curve:** the sequence `|D_d|` over the declared observation horizon, with the day-boundary comparison rule and population denominator reported alongside it.
- **Common-random-numbers (CRN) efficiency:** for replicate `s`, let `Δ_s = Y(intervention, s) - Y(base, s)` for a declared scalar outcome `Y`. Compare the sample variance of paired differences using the same seed/replicate coordinates with the variance of the difference estimator from independently seeded base and intervention samples of the same size. Report `CRN efficiency ratio = Var(independent estimator) / Var(paired estimator)`; values above `1` indicate variance reduction from pairing, `1` no change, and below `1` increased variance. The zero paired-variance reporting convention remains open (§7).
- **Minimal cause:** given a finite candidate intervention set `I` and a deterministic outcome predicate `P`, find a subset `S ⊆ I` of minimum cardinality for which `P(run(S))` is true. The predicate must be explicit and machine-checkable, for example `metric(day=50) >= threshold`, `agent(id).health(day=20) <= 0`, or `D_50` contains a specified `AgentId`. Tie-breaking among equally small subsets is not yet defined (§7).

## 3. M4 stage plan — planned

Each stage gets its own contract section and follows **DRAFT → review → FROZEN** before implementation. This stage plan is not itself authorization to begin any implementation.

| Stage | Planned scope |
|---|---|
| **M4-00 — Counterfactual contract** | Define the research question, terms, inherited invariants, housekeeping options, and open questions. This document is the draft. |
| **M4-01 — System registration architecture** | Specify how systems declare state columns, phase slot, RNG subsystem ID, digest and snapshot contributions, feature switch, and read/write sets. Plan to port the relationship graph, population dynamics, and propagation through this architecture. Require all existing M2/M3/M3P/propagation goldens and snapshot bytes to remain unchanged. |
| **M4-02 — Branching primitive and divergence-point detection** | Specify snapshot-based branch creation, intervention application, and exact first-divergence reporting. |
| **M4-03 — Divergence measurement** | Specify divergence sets and curves, comparison population, and metrics beyond agent-record differences. |
| **M4-04 — CRN efficiency** | Specify paired and independent estimators, replicate counts, and variance reporting. |
| **M4-05 — Minimal-cause search** | Specify candidate-set construction, predicates, minimality, tie-breaking, and search limits. |
| **M4-06 — Settlement migration system** | Use settlement migration as the first newly registered system; report how much code outside the registered system changes. |
| **M4-07 — Many-branch performance** | Measure shared-state and copy-on-write options, cross-branch parallelism, and GPU suitability. A GPU implementation is not implied. |
| **Viewer follow-up after M4-02** | Add a branch-comparison view to the existing read-only trace viewer after the branch record format is frozen. |
| **M4 audit** | Answer the research question from recorded correctness and performance evidence and classify remaining work. |

All entries are planned and require their own frozen contract before implementation.

## 4. Invariants carried from M0–M3

1. **Deterministic randomness:** existing profiles retain the frozen coordinate tuple, packing, generator, and subsystem/draw mappings. An intervention changes declared model input or state; it does not alter how existing coordinates map to random values ([M1 C04](M1_CONTRACT_FREEZE.md); [`RngCoordinate`, `coordinate_prng_u64`, `coordinate_prng_f32`](../../crates/sim-core/src/prng.rs)).
2. **Canonical compatibility:** M4 additions must not change canonical State, Metrics, or Events bytes/digests or snapshot bytes for existing M2, M3, M3P, and propagation-enabled profiles. The hashing module keeps profile-specific domains, and the M3 contract requires existing profile encodings to remain exact ([`hashing.rs`](../../crates/sim-model/src/hashing.rs); [M3 §16.5](M3_INTERACTION_CONTRACT.md)).
3. **Execution policy:** serial remains the production default; parallel execution remains an explicit opt-in using caller-owned pools. Existing M3 parallel entry points take a pool and chunk size rather than selecting a pool automatically ([`run_m3_storage_days`, `run_m3_storage_days_with_parallel_phase6b`, `run_m3prop_storage_days_with_parallel_spread`](../../crates/sim-model/src/runner.rs); [M3 §10 and §16.5](M3_INTERACTION_CONTRACT.md)).
4. **Reference-oracle methodology:** optimized execution is compared against an independent semantic reference using logical results, canonical digests, and snapshot-resume parity. This is the M2 differential-validation rule and the M3 reference/storage gate pattern ([M2 Optimization Contract §4](M2_OPTIMIZATION_CONTRACT.md); [M3 Interaction Contract §§2, 9, 10, 13, 16](M3_INTERACTION_CONTRACT.md)).

## 5. Pre-M4 housekeeping

### 5.1 CI duration

The task baseline reports that the full workspace suite takes about five minutes locally; this duration has not been re-measured as part of this draft. Possible responses are:

- Keep the complete required gate on each change and first report per-target timings to identify avoidable duplication.
- Move only expensive, redundant matrix execution to a release-mode job that remains required for the same change; retain a fast, required correctness subset if its coverage is independently sufficient.
- Add a scheduled release-mode run for extended scale coverage, but do not use a scheduled-only job as a substitute for a required correctness gate.

**Recommendation — proposal:** measure the current CI breakdown first, then split execution mode only where the same required gates remain blocking. Do not reduce matrix dimensions, horizons, or parity assertions to meet a time target.

### 5.2 Sustained population drift

The M3-08 report records the selected M3P sustained configuration at `N=10,000` with 10,089–10,758 alive agents over days 100–1,000 and 10,755 alive on day 1,000. The run stayed within its stated ±10% band but trends upward over that horizon ([M3-08 Population Dynamics, sustained gate](../performance/M3_08_POPULATION_DYNAMICS.md#sustained-gate)).

Options are to keep that profile as a bounded benchmark workload; design density-dependent birth/death feedback before long-horizon counterfactual experiments; or maintain separate workload presets for bounded and regulated populations. Any new population regulation changes behavior and therefore belongs in a new profile with its own contract and goldens, not as a silent change to M3P.

**Recommendation — proposal:** retain M3P unchanged for compatibility and decide whether M4 experiments require density regulation before freezing M4-01/02 workloads. If regulation is needed, define it as a new profile and compare it separately.

## 6. M4-00 non-goals

- No implementation or runtime API change.
- No semantic change to existing profiles or RNG coordinates.
- No GPU implementation or GPU-specific execution semantics.
- This draft does not freeze intervention types, branch representation, or divergence matching.

## 7. Open questions

| ID | Question | Status |
|---|---|---|
| OQ-1 | Which intervention types are supported in the first version, and how are conflicting interventions ordered or rejected? | **Blocking** for M4-02 semantics. |
| OQ-2 | What phase-boundary digest/record format identifies the first exact `(day, phase)` divergence while preserving existing digest schemas? | **Blocking** for M4-02 divergence detection. |
| OQ-3 | How are branches stored initially: independent state copies, snapshot-backed restore, or shared immutable state with copy-on-write? | **Blocking** for M4-02 resource and isolation behavior; deeper many-branch optimization can wait until M4-07. |
| OQ-4 | When births differ between branches, are agents compared only by exact `AgentId`, or is a lineage/matching relation required for divergence sets and outcome attribution? | **Blocking** for interpreting divergence and minimal-cause results involving births. |
| OQ-5 | What happens when the paired estimator variance is zero, and how many seeds/replicates are required for a CRN claim? | **Blocking** for M4-04 reporting rules. |
| OQ-6 | How are system phase slots, subsystem IDs, and read/write conflicts allocated and validated by M4-01 registration? | **Blocking** for M4-01 architecture and preservation of coordinate semantics. |
| OQ-7 | What is the supported outcome-predicate language and deterministic tie-break for equally small causes? | **Blocking** for M4-05 correctness and reproducibility. |
| OQ-8 | Which expensive CI matrices can move to release mode without changing which changes are gated? | **Non-blocking** for this draft; must be settled before M4 CI gates are frozen. |
| OQ-9 | Should M4 include a population-regulated profile for experiments beyond the M3P sustained horizon? | **Non-blocking** for M4-00; blocking only if long-horizon regulated-population claims become an acceptance gate. |
