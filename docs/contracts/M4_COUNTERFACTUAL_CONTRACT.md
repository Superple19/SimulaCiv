# M4 Counterfactual Contract

**Status: M4-00 FROZEN**

**Scope:** M4-00 freezes the counterfactual framework, terminology, invariants, and planned stage boundaries. Stage-specific contracts and implementation details remain subject to their own DRAFT → review → FROZEN process.

**Baseline:** M3 graduated at `v0.4.0-m3`; see the [M3 Final Runtime Profile and Graduation Gate](../performance/M3_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md).

## 1. Research question

> **In a deterministic world, how precisely can cause and effect be separated?**

Counterfactual runs can begin from the same canonical snapshot, apply different interventions at explicit phase boundaries, and compare their later logical states. The M1 contract freezes a stateless PRNG addressed by `(master_seed, replicate_id, day, phase, subsystem_id, AgentId, draw_index)`; `RngCoordinate` and `coordinate_prng_u64` / `coordinate_prng_f32` implement that coordinate mapping. For each coordinate evaluated by both runs, the same run identity yields the same random value regardless of evaluation order. This supports common random numbers for paired branches, while a branch-dependent change in which coordinates are evaluated can still change which draws affect outcomes ([M1 Contract Freeze, C04](M1_CONTRACT_FREEZE.md); [`prng.rs`](../../crates/sim-core/src/prng.rs)).

The runtime also has canonical SHA-256 encodings for state, metrics, and events (`canonical_state_hash`, `canonical_metrics_hash`, `canonical_event_hash` and profile-specific variants), plus canonical snapshot encode/restore functions. These provide exact comparison and resume boundaries; the M3 contract and graduation report record the separate M2/M3/M3P/propagation profiles and their parity gates ([`hashing.rs`](../../crates/sim-model/src/hashing.rs); [`snapshot.rs`](../../crates/sim-model/src/snapshot.rs); [M3 Interaction Contract](M3_INTERACTION_CONTRACT.md); [M3 graduation report](../performance/M3_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)).

This combination makes paired causal experiments plausible. M4-00 now freezes the framework-level intervention classes, divergence search, branch restore model, and entity matching in §8; §7 contains only details assigned to later stage freezes.

## 2. Terminology

These framework-level definitions are frozen by M4-00. Stage-specific schemas and algorithms remain for later contract sections.

- **Base run:** execution from a declared initial state or restored snapshot, with a fixed configuration, master seed, replicate ID, and horizon, and with no M4 interventions.
- **Snapshot boundary:** a point at which the runner emits canonical snapshot bytes for a defined resume cursor. Existing snapshot encoders serialize canonical logical state and metadata; the Phase 11 hook emits a snapshot only when its boundary flag is true ([`encode_snapshot`, `decode_snapshot`, `phase11_snapshot_if_boundary`](../../crates/sim-model/src/snapshot.rs)). The framework applies interventions at their declared boundary; M4-02 pins the exact boundary/API ordering relative to restore and phase execution.
- **Branch:** an isolated continuation from the same base-run snapshot and run identity, with a declared intervention set. The first version restores a snapshot independently for each branch; copy-on-write is deferred to M4-07 (§8).
- **Intervention:** an explicit, typed change applied at a defined `(day, phase boundary)`. M4's first-version types are: (a) a configuration parameter override effective from that boundary onward; (b) a field override (`food`, `health`, or `wealth`) for one `AgentId`; and (c) a forced action for one `AgentId` on one day. Reject duplicate targets at the same boundary: the same configuration parameter, the same `(AgentId, field)`, or the same `(AgentId, day)` forced-action target. Apply type classes in `(a), (b), (c)` order, then agent-targeted operations in ascending `AgentId` order. Interventions never redefine the frozen RNG coordinate tuple ([M1 C04](M1_CONTRACT_FREEZE.md); [`RngCoordinate`](../../crates/sim-core/src/prng.rs)). Exact config field schema, field-override duration, and boundary/API details belong to M4-02.
- **Divergence point:** the earliest `(day, phase)` at which the branch's canonical logical state differs from the base run. Current day-run outcomes expose daily results (`M3DayOutcome` contains `executed_day`, optional metrics, events, and an optional snapshot), not digest fields or a per-phase digest trace. The frozen detection method first compares day-boundary canonical State digests to locate the first differing day, then reruns only that day from both day-start states with an opt-in phase-boundary recorder. The recorder compares canonical State digests after each phase; this adds no fields to existing digest schemas ([`M3DayOutcome`, `run_m3_reference_day`, `run_m3_storage_days`](../../crates/sim-model/src/runner.rs); [`canonical_state_hash`, `canonical_m3_state_hash`](../../crates/sim-model/src/hashing.rs); [M3 parity gates](M3_INTERACTION_CONTRACT.md)).
- **Divergence set:** for day `d`, `D_d` is the set of matched logical entities whose canonical agent record differs between branch and base state at the agreed day boundary. Agents present at the branch point match by `AgentId`; agents born afterward match by lineage key `(parent AgentId, parent's birth ordinal)`. This is an M4 comparison key, not a field in current M3P `BirthRecord` (which records parent, child, and group); canonical M1 state records remain independent of physical `DenseSlot` and sort by `AgentId` ([`BirthRecord`, `apply_phase9b_storage`](../../crates/sim-model/src/m3p.rs); [`canonical_state_bytes`](../../crates/sim-model/src/hashing.rs); [M1 C01/C03](M1_CONTRACT_FREEZE.md)).
- **Divergence curve:** the sequence `|D_d|` over the declared observation horizon, with the day-boundary comparison rule and population denominator reported alongside it.
- **Common-random-numbers (CRN) efficiency:** for replicate `s`, let `Δ_s = Y(intervention, s) - Y(base, s)` for a declared scalar outcome `Y`. Compare the sample variance of paired differences using the same seed/replicate coordinates with the variance of the difference estimator from independently seeded base and intervention samples of the same size. Report `CRN efficiency ratio = Var(independent estimator) / Var(paired estimator)`; values above `1` indicate variance reduction from pairing, `1` no change, and below `1` increased variance. If paired variance is zero, report **“exact (paired variance 0)”** separately from the ratio; the ratio is not finite in that case. M4-04 freezes minimum replicate count and confidence method.
- **Minimal cause:** given a finite candidate intervention set `I` and a deterministic outcome predicate `P`, find a subset `S ⊆ I` of minimum cardinality for which `P(run(S))` is true. The predicate must be explicit and machine-checkable, for example `metric(day=50) >= threshold`, `agent(id).health(day=20) <= 0`, or `D_50` contains a specified `AgentId`. Among equally small subsets, the default tie-break is the lexicographically smallest sorted list of intervention IDs. M4-05 freezes the predicate language and search limits.

### Known precision limits

CRN is shared only for coordinates both branches evaluate: identical coordinate inputs produce identical PRNG values, but different survival, action choices, or births can change which coordinates are evaluated ([`coordinate_prng_u64`, `coordinate_prng_f32`](../../crates/sim-core/src/prng.rs); [`apply_phase9b`, `apply_phase9b_storage`, `plan_phase9b_from_view`](../../crates/sim-model/src/m3p.rs)). In M3P, children receive IDs from the global `next_agent_id` cursor and the cursor advances by the number of successful births; because the RNG coordinate contains an entity `AgentId`, divergent birth counts can shift later newborn IDs and their coordinates. A lineage-keyed RNG identity is a candidate for M4-02, not a change to M3P; existing M3P allocation and coordinate semantics stay unchanged ([`M3pReferenceState::next_agent_id`, `apply_phase9b_storage`, `plan_phase9b_from_view`](../../crates/sim-model/src/m3p.rs); [`RngCoordinate`](../../crates/sim-core/src/prng.rs)).

## 3. M4 stage plan — planned

Each stage gets its own contract section and follows **DRAFT → review → FROZEN** before implementation. This stage plan is not itself authorization to begin any implementation.

| Stage | Planned scope |
|---|---|
| **M4-00 — Counterfactual contract** | Freeze the research question, framework terminology, inherited invariants, housekeeping options, and stage boundaries. This document is frozen; later sections freeze their own details. |
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

The task baseline reports that the full workspace suite takes about five minutes locally; this duration has not been independently measured for M4-00. Possible responses are:

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
- No stage-specific implementation details beyond the framework decisions in §8; later stage sections must freeze their own schemas, APIs, and algorithms.

## 7. Open questions

These questions remain for the named later-stage freeze; none reopens the M4-00 decisions in §8.

| ID | Question | Status |
|---|---|---|
| OQ-1 detail | Which exact config fields/types/ranges and intervention serialization are supported, how long do field overrides persist, and what are the operational boundary hooks? | **Deferred / blocking** for the M4-02 freeze. |
| OQ-2 detail | What recorder API and per-phase record format implement the frozen two-level search? | **Deferred / blocking** for the M4-02 freeze. |
| OQ-4 detail | How is a parent's birth ordinal assigned, and should newborn stochastic identity remain AgentId-based or use a lineage key in a new profile? | **Deferred / blocking** for M4-02 decisions involving newborn CRN alignment. |
| OQ-5 detail | What minimum replicate count and confidence method are required for CRN estimates? | **Deferred / blocking** for the M4-04 freeze. |
| OQ-6 | How are system phase slots, subsystem IDs, state-column registration, and read/write conflicts allocated and validated? | **Deferred / blocking** for the M4-01 freeze. |
| OQ-7 | What outcome-predicate language, search limits, and intervention-ID schema does minimal-cause search use? | **Deferred / blocking** for the M4-05 freeze; the default tie-break is already frozen in §8. |
| OQ-8 | Which expensive CI matrices can move to release mode without changing which changes are gated? | **Non-blocking** for M4-00; measure before M4-01 CI gates are frozen. |
| OQ-9 | Does M4 need a population-regulated profile for experiments beyond the M3P sustained horizon? | **Non-blocking** for M4-00; decide before M4-03. |

## 8. Decision register

M4-00 freezes the framework decisions below. Details explicitly assigned to a later stage remain open until that stage's own contract freeze.

| ID | Status | Decision |
|---|---|---|
| OQ-1 | **FROZEN** | First-version interventions are config parameter overrides effective from the boundary onward, `food` / `health` / `wealth` field overrides for one `AgentId`, and one-day forced actions for one `AgentId`. Reject duplicate targets at the same boundary. Apply type classes in `(a), (b), (c)` order and agent-targeted entries by ascending `AgentId`; interventions do not change RNG coordinates. Field-override duration and exact config schema are M4-02 details. |
| OQ-2 | **FROZEN** | Find the first differing day by comparing day-boundary canonical State digests. Rerun only that day from both day-start states with an opt-in phase-boundary recorder to identify the first differing phase. Existing digest schemas remain unchanged. |
| OQ-3 | **FROZEN** | First-version branches use snapshot-backed restore. Copy-on-write is deferred to M4-07. |
| OQ-4 | **FROZEN** | Match agents existing at the branch point by `AgentId`; match later births by lineage key `(parent AgentId, parent's birth ordinal)`. The global M3P `next_agent_id` allocator can shift subsequently allocated newborn IDs after divergent births, changing their AgentId-based RNG coordinates and silently removing CRN alignment for those newborn draws. Consider lineage-keyed RNG identity in M4-02 as a candidate new profile; M3P remains unchanged ([`apply_phase9b_storage`, `plan_phase9b_from_view`](../../crates/sim-model/src/m3p.rs); [`RngCoordinate`](../../crates/sim-core/src/prng.rs)). |
| OQ-5 | **FROZEN** | If paired variance is zero, report “exact (paired variance 0)” separately; do not present a finite CRN ratio for a zero denominator. M4-04 freezes minimum replicate count and confidence method. |
| OQ-6 | **DEFERRED** | System-registration details are decided in the M4-01 freeze. |
| OQ-7 | **PARTLY FROZEN / DEFERRED** | Default equal-cardinality cause tie-break is the lexicographically smallest sorted list of intervention IDs. Predicate language, intervention-ID schema, and search limits are decided in the M4-05 freeze. |
| OQ-8 | **OPEN — NON-BLOCKING** | Measure CI workload before M4-01 freezes CI gates; no correctness gate is weakened by this M4-00 decision. |
| OQ-9 | **OPEN — NON-BLOCKING** | Decide before M4-03 whether a new population-regulated profile is needed; M3P semantics remain unchanged. |

## 9. Freeze record

This freeze resolves the M4-00 framework decisions for first-version intervention classes and ordering, two-level divergence detection, snapshot-backed branches, cross-branch birth matching, and zero-variance CRN reporting. It records system registration and minimal-cause details for their own later freezes. M4-00 authorizes the M4-01 contract to be requested; it does not start M4-01 implementation or freeze any later-stage contract.
