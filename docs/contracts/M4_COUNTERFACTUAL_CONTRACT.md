# M4 Counterfactual Contract

**Status: M4-00 FROZEN; §10 M4-01 FROZEN; §11 M4-02 FROZEN; §12 M4-03 FROZEN; §13 M4-04 FROZEN; §14 M4-05 FROZEN; §15 M4-06 DRAFT**

**Scope:** M4-00 freezes the counterfactual framework; §§10–14 freeze M4-01 through M4-05; §15 drafts M4-06 settlement migration. Later stages still require their own DRAFT → review → FROZEN contract.

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
| **M4-01 — System registration architecture** | **COMPLETE.** Static registry and generic WorldState/SegmentedAgentStorage loops preserve profile behavior; see the [M4-01 report](../performance/M4_01_SYSTEM_REGISTRY.md). |
| **M4-02 — Branching primitive and divergence-point detection** | **COMPLETE.** Snapshot-backed branches and exact first-divergence reporting pass the A-5 gate; see the [M4-02 report](../performance/M4_02_BRANCHING.md) and [§11](#11-m4-02-branching-and-divergence-detection). |
| **M4-03 — Divergence measurement** | **COMPLETE.** The frozen series, daily component partition, cascade metrics, and target-death analysis pass the §12.6 A-5 gate; see the [M4-03 report](../performance/M4_03_DIVERGENCE.md) and [§12](#12-m4-03-divergence-measurement). |
| **M4-04 — CRN efficiency** | **COMPLETE.** Frozen paired/independent estimators and bootstrap reporting; 100-replicate M3P sustained experiment recorded in the [M4-04 report](../performance/M4_04_CRN_EFFICIENCY.md). |
| **M4-05 — Minimal-cause search** | **COMPLETE.** Deterministic intervention batches, cached paired evaluation, exhaustive minimum/ddmin search, JSONL replay, the N=2,000 golden, and the M3P N=10,000 A-5 gate pass; see the [M4-05 report](../performance/M4_05_MINIMAL_CAUSE.md) and [§14](#14-m4-05-minimal-cause-search). |
| **M4-06 — Settlement migration system** | **PLANNED — DRAFT.** Use settlement migration as the first newly registered system and test the M4-01 forward gate; see [§15](#15-m4-06-settlement-migration-system). |
| **M4-07 — Many-branch performance** | Measure shared-state and copy-on-write options, cross-branch parallelism, and GPU suitability. A GPU implementation is not implied. M4-02 follow-up candidate: day-scan digest cost measured at 70–160 ns/agent-day; compare direct column equality with SHA-256 only on request. |
| **Viewer follow-up after M4-02** | Add a branch-comparison view to the existing read-only trace viewer after the branch record format is frozen. |
| **M4 audit** | Answer the research question from recorded correctness and performance evidence and classify remaining work. |

M4-00 through M4-05 are complete. M4-06 and later stages remain planned.

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
| OQ-8 | Which expensive CI matrices can move to release mode without changing which changes are gated? | **Non-blocking** for M4-00; per-target timings are recorded in §10.1, and the CI layout decision is due before M4-01 gates freeze. |
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
| OQ-8 | **OPEN — NON-BLOCKING** | Per-target workspace test timings are recorded in §10.1. Decide before M4-01 freezes CI gates whether to keep the current single required workspace test step or split equivalent gates; do not drop scenarios. |
| OQ-9 | **OPEN — NON-BLOCKING** | Decide before M4-03 whether a new population-regulated profile is needed; M3P semantics remain unchanged. |

## 9. Freeze record

This freeze resolves the M4-00 framework decisions for first-version intervention classes and ordering, two-level divergence detection, snapshot-backed branches, cross-branch birth matching, and zero-variance CRN reporting. It records system registration and minimal-cause details for their own later freezes. M4-00 authorizes the M4-01 contract to be requested; it does not start M4-01 implementation or freeze any later-stage contract.

## 10. M4-01 System registration architecture

**Status: M4-01 FROZEN**

**Scope:** freeze a static, compile-time registry for the existing M3 extensions only. M1/M2 core phases remain fixed; implementation follows the §10.5 migration and gates.

### 10.1 Current state and OQ-8 test-time measurement

The feature switches are currently the fixed `SimulationSystems` triple (`relationship_graph`, `population_dynamics`, `propagation`); `SimConfig::runner_selection` maps the effective set to one of five runner selections, and `run_configured_storage_days` dispatches to the corresponding legacy runner ([`SimulationSystems`, `SimConfig::runner_selection`](../../crates/sim-model/src/config.rs); [`ConfiguredStorageRunContext`, `run_configured_storage_days`](../../crates/sim-model/src/runner.rs); [M3 §15](M3_INTERACTION_CONTRACT.md)).

| Extension | Current state and execution path |
|---|---|
| Relationship graph | `RelationshipGraph` owns canonical CSR and its hash. M3 reference/storage contexts keep the graph beside `WorldState` or `SegmentedAgentStorage`; the slot projection and scratch are transient context data. M3P places the graph in `M3pReferenceState` and `M3pStorageRunContext`. Phase 4 targeting and Phase 9c propagation read the graph; Phase 6B consumes already-generated intents and does not read CSR ([`RelationshipGraph`, `RelationshipGraph::generate`](../../crates/sim-model/src/relationship_graph.rs); [`M3ReferenceContext`, `M3StorageRunContext`, `M3pStorageRunContext`](../../crates/sim-model/src/runner.rs); [`M3pReferenceState`](../../crates/sim-model/src/m3p.rs); [`generate_intents_m3_into`, `generate_intents_m3_storage_into`](../../crates/sim-model/src/intents.rs); [`phase6b_targeted_resolution_m3`, `phase6b_targeted_resolution_storage_m3_with_scratch`](../../crates/sim-model/src/resolution.rs); [`apply_phase9c_for_layout`](../../crates/sim-model/src/propagation.rs)). |
| Population dynamics | M3P Phase 9b runs after mortality and plans/commits births, graph changes, compaction, and the `next_agent_id` cursor. The cursor and mutable graph are in M3P reference/runtime context; new or surviving agent values remain in the core world/storage columns ([`M3pReferenceState`, `apply_phase9b`, `apply_phase9b_storage`, `apply_phase9b_storage_incremental`](../../crates/sim-model/src/m3p.rs); [`M3pStorageRunContext`, `run_m3p_storage_day_inner`](../../crates/sim-model/src/runner.rs); [`WorldState`, `AgentState`](../../crates/sim-model/src/state.rs); [`SegmentedAgentStorage::push`](../../crates/sim-model/src/storage.rs)). |
| Propagation | Phase 9c owns `informed` and `informed_day` rows in a `BTreeMap<AgentId, PropagationAgentState>` in reference state or runner context; it is not embedded in the core agent record. `apply_phase9c_for_layout` is shared by reference, storage, and explicit parallel wrappers ([`M3PropagationReferenceState`, `run_m3prop_reference_day`](../../crates/sim-model/src/m3prop.rs); [`M3PropagationStorageRunContext`, `M3pPropagationStorageRunContext`](../../crates/sim-model/src/runner.rs); [`PropagationAgentState`, `apply_phase9c_for_layout`](../../crates/sim-model/src/propagation.rs); [M3 §16.1 and §16.3](M3_INTERACTION_CONTRACT.md)). |

The persisted representations are profile-specific today. M2/M3 use the base snapshot encoder; M3 regenerates its static graph on restore. M3P schema v2 wraps the base snapshot with `next_agent_id`, initial money supply, canonical CSR, and its hash; propagation schema v3 wraps the matching base profile with its feature manifest and propagation rows ([`encode_snapshot`, `decode_snapshot`](../../crates/sim-model/src/snapshot.rs); [`encode_m3p_snapshot`, `decode_m3p_snapshot`](../../crates/sim-model/src/m3p.rs); [`encode_m3prop_snapshot`, `decode_m3prop_snapshot`](../../crates/sim-model/src/m3prop.rs); [M3 §2.1, §13.7, §15.1, §16.5](M3_INTERACTION_CONTRACT.md)). Canonical M3 State appends the CSR hash; M3P adds the allocator cursor; propagation adds canonical propagation rows. M3 `M3DailyMetrics` appends cross-partition edge count, total directed edge count, and daily cross-partition interaction count to the M1 metrics. M3P appends births, removals, replaced edges, live population, and allocator cursor; propagation appends its six daily counters. M3 uses the existing event vocabulary; M3P adds Birth/Cleanup variants; propagation adds no event kinds but uses the propagation digest domain ([`M3DailyMetrics`](../../crates/sim-model/src/metrics.rs); [`M3pDailyMetrics`, `M3pEvent`, `apply_phase9b`](../../crates/sim-model/src/m3p.rs); [`M3PropDailyMetrics`](../../crates/sim-model/src/m3prop.rs); [`PropagationDailyCounts`](../../crates/sim-model/src/propagation.rs); [`canonical_m3_state_bytes`, `canonical_m3p_state_bytes`, `canonical_m3prop_state_bytes`, `canonical_m3pprop_state_bytes`, `canonical_m3p_event_bytes`, `canonical_m3prop_event_bytes`](../../crates/sim-model/src/hashing.rs); [M3 §13.7, §15.2, §16.4–16.5](M3_INTERACTION_CONTRACT.md)).

The current code has separate reference, storage, incremental-topology, and parallel integration surfaces: three reference day entrypoints, six serial/non-parallel storage entrypoints (including separate M3P incremental variants), and three explicit parallel storage entrypoints. A source inventory finds 14 involved Rust modules: nine extension-facing modules (`config.rs`, `runner.rs`, `relationship_graph.rs`, `intents.rs`, `resolution.rs`, `m3p.rs`, `m3prop.rs`, `propagation.rs`, and `hashing.rs`) plus `snapshot.rs`, `state.rs`, `storage.rs`, `metrics.rs`, and `events.rs`. In `runner.rs`, a manual count finds 24 M3-family runner/dispatch/helper definitions between lines 1409–2550; M3 contexts and orchestration occupy about 2,070 lines across lines 232–903 and 1409–2806. This is a profile-wiring surface estimate, not an exact duplicated-line count: phase primitives and some kernels are shared, and identical lines were not mechanically diffed ([`run_m3_reference_day`, `run_m3prop_storage_days_inner`, `run_m3p_storage_days_incremental_topology`, `run_m3p_propagation_storage_day`, `run_configured_storage_days`](../../crates/sim-model/src/runner.rs); [`run_m3p_reference_day`, `apply_phase9b_storage`](../../crates/sim-model/src/m3p.rs); [`run_m3prop_reference_day`](../../crates/sim-model/src/m3prop.rs); [`apply_phase9c_for_layout`](../../crates/sim-model/src/propagation.rs)). M4-01 must record the same inventory method and before/after counts when the registry port is measured.

OQ-8 was measured on the local Windows host at HEAD `47d00659fba40ce65c3bcc07a290bd24621815a2`. `cargo metadata --no-deps --format-version 1` enumerated the test targets and doctests; each was run sequentially with its package-specific `cargo test -p … --lib`, `--test <name>`, or `--doc` command. Cargo startup and any incremental build work are included in each command's wall time. All 52 targets passed. These are serialized per-target command times, not the wall time of one monolithic `cargo test --workspace` invocation.

| Test target | Wall time |
|---|---:|
| `sim-model/m3p_propagation_tests` | 169.42 s |
| `sim-model/m3_search_reduction_tests` | 80.09 s |
| `sim-model/m3_parallel_runner_tests` | 76.18 s |
| `sim-model/m3p_incremental_topology_tests` | 71.50 s |
| `sim-model/m3_storage_runner_tests` | 44.72 s |
| `sim-model/m3p_storage_runner_tests` | 35.30 s |
| `sim-model/m3p_golden_tests` | 6.44 s |
| `sim-model/m3p_sustained_gate_tests` | 6.02 s |
| **All 52 targets, sequential command wall sum** | **512.08 s (8m 32.08s)** |

The current GitHub Actions workflow runs `cargo test --workspace` as one required step in each of its Linux, Windows, and macOS jobs ([`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)). The per-target sum is useful for locating expensive suites but should not be treated as the hosted job duration or a direct comparison with a single Cargo invocation.

### 10.2 Static descriptor

Use a typed `SystemDescriptor` registry as a compile-time static slice. It contains no dynamic loading or runtime plugin discovery. A descriptor records:

| Field | Frozen descriptor content |
|---|---|
| `system_id`, `name` | Stable registry identity and readable name, distinct from RNG subsystem IDs. |
| `feature_switch_key` | The key in `[simulation.systems]` that enables this system. |
| `dependencies` | Required registered system IDs. |
| `phase_slot` | Initialization or an insertion point relative to core phases 1–11; systems with multiple hooks declare an ordered slot list. |
| `rng_subsystem_ids` | All existing subsystem IDs owned/consumed by the system; no existing coordinate or ID mapping changes. |
| `owned_state_columns` | Canonical system-owned columns/state, distinguished from transient execution scratch. |
| `snapshot_section` | Existing schema version, section order, and codec contribution, if the system is enabled. |
| `digest_contribution` | Applicable legacy profile domain tag, field/section order, endianness, and exact byte encoding contribution. |
| `metrics_fields`, `event_kinds` | Existing per-day fields and event variants emitted by the system. |
| `read_set`, `write_set` | Core and registered system columns accessed at each phase slot. These declarations validate composition only in M4-01. |

The initial descriptors cover only:

| System | Existing mapping and frozen access ownership |
|---|---|
| Relationship graph | RNG subsystem ID 6; built at initialization. Owns canonical CSR/hash. Phase 4 neighbor targeting and Phase 9c propagation read CSR. When enabled, its Phase 6B hook selects the M3 global resolver, which consumes already-generated intents and does not read CSR. For M3P, graph mutation at Phase 9b is declared as a population-dynamics write ordered after the relationship-graph dependency. |
| Population dynamics | RNG subsystem ID 7; Phase 9b after mortality. Owns the monotonic `next_agent_id` cursor and birth/cleanup metrics/events; reads eligible parent and graph-pool data, writes core agent rows, allocator state, and graph rows through its declared dependency. |
| Propagation | RNG subsystem ID 8; Phase 9c after Phase 9b when enabled, otherwise after Phase 9. Owns `informed` and `informed_day`; reads live-agent and graph columns and writes its own columns and daily counters. |

The IDs and phase behavior above are the existing frozen M3 mapping, not newly assigned IDs ([M3 §2.1, §13.3, §16.1–16.3](M3_INTERACTION_CONTRACT.md); [`M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID`](../../crates/sim-model/src/relationship_graph.rs); [`M3P_BIRTH_SUBSYSTEM_ID`](../../crates/sim-model/src/m3p.rs); [`M3_PROPAGATION_SUBSYSTEM_ID`](../../crates/sim-model/src/propagation.rs)). The descriptor schema and access-validation rules are frozen here. Implementations must derive per-slot read/write sets from the phase code and verify their completeness in review/tests; these declarations do not grant additional state access or scheduling authority.

### 10.3 M4-01 scope and compatibility map

Keep the M1/M2 core phases 1–11 fixed. Register only the relationship graph, population dynamics, and propagation extensions. The legacy profile-to-system-set mapping is:

| Effective profile | Registered system set |
|---|---|
| `m2` | none |
| `m3` | relationship graph |
| `m3p` | relationship graph + population dynamics |
| `m3` + propagation | relationship graph + propagation |
| `m3p` + propagation | relationship graph + population dynamics + propagation |

This mapping mirrors `SimulationSystems::for_profile`, `effective_systems`, and `runner_selection`; the propagation combinations are an orthogonal opt-in ([`SimulationSystems::for_profile`, `SimConfig::effective_systems`, `SimConfig::runner_selection`](../../crates/sim-model/src/config.rs); [M3 §15.1–15.3](M3_INTERACTION_CONTRACT.md)). For every combination, the registry must select encoders/codecs that produce byte-identical State/Metrics/Events digests and snapshot bytes to the current path. Feature manifests remain outside legacy digest preimages, and v1/v2/v3 snapshot contents and order remain unchanged ([`canonical_*_bytes`](../../crates/sim-model/src/hashing.rs); [`encode_snapshot`](../../crates/sim-model/src/snapshot.rs); [`encode_m3p_snapshot`](../../crates/sim-model/src/m3p.rs); [`encode_m3prop_snapshot`](../../crates/sim-model/src/m3prop.rs); M3 §15.2 and §16.5).

**Primary goal — FROZEN:** provide one generic day loop per storage layout: one for the `WorldState` reference layout and one for `SegmentedAgentStorage`. Each loop executes fixed core phases 1–11 and invokes registered system hooks at their declared phase slots. A per-system execution option selects serial execution, incremental topology maintenance, or explicit parallel execution with a caller-owned pool; the option is passed to the generic loop rather than selecting a separate runner function. Serial remains the default and the loop never creates or resizes a pool. Existing public profile-specific runner entrypoints and signatures remain as thin wrappers that select the same system set/options and call the appropriate generic loop. Their day results, errors, and compatibility behavior stay unchanged.

### 10.4 Startup validation and execution limits

Before state construction or mutation, validate the static registry and selected feature set:

1. `system_id` and descriptor names are unique across the registry; RNG subsystem IDs are unique across all registered descriptors, and existing subsystem ownership/mapping is unchanged (M1 IDs 0–5, relationship graph 6, population dynamics 7, propagation 8; [M1 C04](M1_CONTRACT_FREEZE.md); [M3 §§2.1, 13.3, 16.1](M3_INTERACTION_CONTRACT.md)).
2. Every declared dependency is registered and enabled when its dependent system is enabled.
3. No two systems write the same logical column in the same phase slot unless an explicit deterministic order is declared.
4. Phase slots have a total deterministic order relative to core phases; ties use an explicit stable order rather than container iteration order.
5. Read/write declarations cover only logical columns and are used for validation and audit in M4-01. They do not schedule tasks, infer independence, or enable parallel execution.

The current feature validation already rejects missing dependencies and profile/switch disagreements before runner dispatch ([`SimConfig::validate`, `SimConfig::runner_selection`](../../crates/sim-model/src/config.rs); M3 §15.3). The registry must preserve that failure boundary and error precedence. There is no automatic parallelism in this stage.

### 10.5 Migration plan

Port one extension at a time behind the registry. For each step, keep the legacy runner as the oracle until exact parity passes; only then convert that legacy public entrypoint into a thin wrapper over the generic loop:

1. Register relationship graph and route the existing M3 Phase 4/6B paths through its descriptor.
2. Register Phase 9b population dynamics while retaining the M3P reference and storage runners, including the incremental-topology path.
3. Register Phase 9c propagation for M3 and M3P combinations, then retain the existing explicit parallel spread path as opt-in.

After each port, require unchanged M2/M3/M3P/propagation goldens, snapshot bytes, parity matrices, resume checks, and error behavior. The old path remains available as an oracle until that step passes; no golden is re-blessed to accommodate the registry. The current references and counterparts are `run_m3_reference_day` / `run_m3_storage_days`, `run_m3p_reference_day` / `run_m3p_storage_days`, and `run_m3prop_reference_day` / `run_m3prop_storage_days` ([`runner.rs`](../../crates/sim-model/src/runner.rs); [`m3p.rs`](../../crates/sim-model/src/m3p.rs); [`m3prop.rs`](../../crates/sim-model/src/m3prop.rs)).

### 10.6 Success criteria

- All existing tests, profile goldens, snapshot byte checks, deterministic replay gates, and hosted 3-OS CI pass unchanged.
- Report runner entrypoints and M3-family wiring lines before/after using the §10.1 inventory method. The target is to replace per-combination day-loop functions with the two generic layout loops while keeping profile-specific wrappers thin.
- A test-only no-op system can be switched on/off without changing existing digests, snapshot bytes, events, or metrics; adding it requires zero new runner functions and zero generic-loop edits beyond the registry entry.
- The same no-new-runner/no-generic-loop-edit condition is a forward gate for M4-06 settlement migration and must be recorded in that stage's report.
- Whole-tick performance gate: compare the registry path with the current runner for M3 and M3P at `N = 10,000` and `N = 100,000` on their sustained configurations, plus M3+propagation at `N = 100,000`. Use paired, uninstrumented whole-tick samples from identical checkpoints, contexts/scratch built outside timing, metrics/events on, snapshots off, two warmups, at least 11 paired interleaved samples, and three consecutive days per sample with scratch reuse. For every cell, require `abs(median_registry - median_current) <= max(0.02 * median_current, MAD_registry, MAD_current)` and report the medians, MADs, and result.

### 10.7 Non-goals

- No new simulation semantics, state fields, RNG coordinates, digest domains, snapshot versions, metrics, or events.
- No refactor or reorder of M1/M2 core phases.
- No automatic parallel scheduling or change to serial defaults.
- No counterfactual branch/intervention implementation; those belong to M4-02 onward.
- No dynamic loading or GPU execution.

### 10.8 Review questions and adopted answers

The answers below are adopted by the M4-01 decision register in §10.9. They are retained here to show the questions each decision resolves.

| ID | Question | Status | Adopted answer |
|---|---|---|---|
| Q1 | How are registry and system IDs represented and ordered? | **FROZEN** | Use stable typed IDs in a compile-time static slice; keep them separate from RNG subsystem IDs and avoid hash-map iteration. |
| Q2 | How are multiple hooks and write conflicts represented when a system participates at more than one phase slot? | **FROZEN** | Declare per-slot access sets and an explicit total order for the few permitted write overlaps; reject undeclared overlap before state construction. |
| Q3 | How are state ownership and cross-system mutation represented for graph updates during Phase 9b? | **FROZEN** | RelationshipGraph owns CSR/hash; PopulationDynamics declares ordered write access through its graph dependency. Keep transient slot projections and scratch outside owned canonical columns. |
| Q4 | Should descriptor-driven digest/snapshot encoding replace legacy codecs or select them? | **FROZEN** | Have descriptors select existing profile encoders and exact section order first; do not generically concatenate contributions until byte-equivalence is proven. |
| Q5 | How are no-op registration tests kept out of legacy production feature sets? | **FROZEN** | Use a test-only descriptor with empty state/digest/snapshot/metrics/event contributions and assert every existing profile's bytes remain exact. |
| Q6 | What is the final performance-noise threshold and representative cell set? | **FROZEN** | Use the §10.6 M3/M3P/M3+propagation cell set and require the registry median to be within `max(2% of current median, larger per-path MAD)`; report every cell. |
| Q7 | How should CI use the measured per-target timings? | **FROZEN** | Keep the full workspace test step required on Linux, Windows, and macOS. Any later split keeps every split job required and covers exactly the same tests. |

### 10.9 Decision register

| ID | Status | Frozen decision |
|---|---|---|
| D0 — generic loops | **FROZEN** | Implement one phase 1–11 generic day loop for `WorldState` reference execution and one for `SegmentedAgentStorage`. Each dispatches registered hooks at frozen slots. Per-system execution options carry serial, incremental-topology, or explicit caller-pool parallel behavior into the loop; do not create a runner function per combination or mode. Keep existing public APIs and signatures as thin wrappers. |
| Q1 | **FROZEN** | Use stable typed registry IDs in a compile-time static slice, distinct from RNG subsystem IDs. Use a deterministic registry order and no dynamic loading. |
| Q2 | **FROZEN** | Declare access by phase slot. Reject conflicting writes unless an explicit deterministic order is recorded; phase-slot ordering is total and stable. |
| Q3 | **FROZEN** | RelationshipGraph owns canonical CSR/hash. PopulationDynamics may write graph rows at Phase 9b through its declared dependency. Transient projection/scratch are not owned canonical columns. |
| Q4 | **FROZEN** | Descriptors select existing profile encoders and exact snapshot section order. Do not replace legacy codecs with generic concatenation unless byte equivalence is proven. |
| Q5 | **FROZEN** | The no-op descriptor is test-only, contributes no state, RNG, digest, snapshot, metrics, or events, and must leave all existing profile bytes unchanged. Adding it requires no runner function and no generic-loop changes beyond its registry entry. |
| Q6 | **FROZEN** | Measure M3 and M3P at 10k/100k on sustained configurations and M3+propagation at 100k. On paired uninstrumented whole-tick medians, require `abs(median_registry - median_current) <= max(0.02 * median_current, MAD_registry, MAD_current)`. Report every cell using the §10.6 protocol. |
| Q7 | **FROZEN** | Keep the full workspace test step required on all three OSes. Any later split remains fully required and covers the same tests. |
| Migration | **FROZEN** | Port one system at a time. The legacy runner remains the oracle until exact parity passes; then that public entrypoint becomes a thin wrapper over the generic loop. |
| M4-06 forward gate | **FROZEN** | Settlement migration must add no runner function and no generic-loop edit beyond its system/registry entry; record this in the M4-06 report. |

### 10.10 Freeze record

This freeze sets §10 to FROZEN and fixes the static descriptor fields, validation rules, legacy profile compatibility map, two generic day loops, per-system execution options, migration order, parity requirements, performance gate, and CI requirement. It authorizes the M4-01 implementation task to be requested; it does not start implementation. M4-06 must preserve the no-new-runner/no-generic-loop-edit forward gate recorded above.

## 11. M4-02 Branching and Divergence Detection

**Status: FROZEN**

**Scope:** freeze a snapshot-backed paired-run primitive using M4-00 interventions and the two-level divergence search. Existing profile runners and canonical encoders remain the behavior oracles ([run_m3_reference_day, run_m3_storage_days](../../crates/sim-model/src/runner.rs); [run_m3p_reference_day](../../crates/sim-model/src/m3p.rs); [run_m3prop_reference_day](../../crates/sim-model/src/m3prop.rs); [run_m3pprop_reference_day](../../crates/sim-model/src/m3pprop.rs)).

### 11.1 Branch request and result

The public entry point is run_counterfactual_branch(&BranchRequest) -> Result<BranchResult, BranchError>.

BranchRequest contains:
- canonical snapshot bytes at resume cursor d;
- matching SimConfig and M0RunContext;
- exactly one typed M4-00 intervention;
- horizon H;
- an execution option: Serial or IncrementalTopology.
- optional day boundaries whose existing canonical snapshots should be returned.

ConfigParameter is limited to daily model inputs in environment, economy, interaction, decision, and population settings. It excludes run identity (profile, master seed, replicate), initialization-only dimensions, graph topology settings, and propagation settings embedded in propagation snapshot metadata. ConfigValue is a typed finite f32, Money, or u32 matching the selected field; the resulting config must pass existing validation before either branch is mutated.

The public type shape is:

```rust
pub struct BranchRequest {
    pub snapshot: CanonicalSnapshot,
    pub config: SimConfig,
    pub run_context: M0RunContext,
    pub intervention: BranchIntervention,
    pub horizon_days: u32,
    pub execution: BranchExecutionOption,
    pub snapshot_days: Vec<u32>,
}

pub enum BranchIntervention {
    ConfigOverride { parameter: ConfigParameter, value: ConfigValue },
    AgentFieldOverride { agent_id: AgentId, field: AgentField, value: AgentFieldValue },
    ForcedAction { agent_id: AgentId, action: Action },
}

pub enum AgentField { Food, Health, Wealth }
pub enum AgentFieldValue { Food(f32), Health(f32), Wealth(Money) }

pub enum BranchExecutionOption { Serial, IncrementalTopology, ExplicitParallel }

pub enum ConfigParameter {
    CarryingCapacity, RegrowthRate, BaseMetabolicCost, HealthDecayRate,
    BaseWorkYield, FoodPrice, TargetFood, TargetReserve, TaxRate, WelfarePayment,
    GiftAmount, TheftAmount, TheftSuccessProbability, StarvationThreshold,
    DecisionTemperature, ActionBias { action_index: u8 },
    BaseWeight { action_index: u8, feature_index: u8 },
    TraitWeightCooperation, TraitWeightAggression, TraitWeightRiskTolerance,
    BirthMinHealth, BirthFoodThreshold, BirthProbability, BirthFoodCost,
    BirthsMaxPerDay, CompactionIntervalDays,
}
pub enum ConfigValue { F32(f32), Money(Money), U32(u32) }

pub struct BranchResult {
    pub base_days: Vec<BranchDayRecord>,
    pub branch_days: Vec<BranchDayRecord>,
    pub divergence: Option<FirstDivergence>,
    pub daily_differences: Vec<DailyDifference>,
    pub forced_action: Option<ForcedActionOutcome>,
}
```

BranchDayRecord contains the State/Metrics/Events digests and records, plus the optional canonical snapshot for that day. FirstDivergence contains day, phase slot, differing slot (or InterventionBoundary), differing components, optional SystemId, and descriptor RNG subsystem IDs. DailyDifference contains the matched-agent divergence count, separate graph/allocator/propagation difference flags, matched/unmatched births, and newborn CRN loss. BranchError includes typed unsupported-profile, unsupported-execution-option, invalid-snapshot, and intervention-validation errors, and preserves wrapped runner errors. ExplicitParallel is a rejection-only request value; this stage does not accept a pool or execute parallel work.

Supported profiles are M3, M3P, M3 with propagation, and M3P with propagation. Incremental-topology execution uses the existing M3P incremental implementation; for static-topology M3 profiles it is equivalent to serial execution. Explicit parallel execution is rejected with typed UnsupportedExecutionOption. The horizon scan runs on SegmentedAgentStorage through run_registered_storage_day_loop; the phase-location rerun runs on WorldState through run_registered_world_day_loop, using the already-gated reference/storage parity ([run_registered_storage_day_loop, run_registered_world_day_loop](../../crates/sim-model/src/runner.rs); profile paths in [m3p.rs](../../crates/sim-model/src/m3p.rs), [m3prop.rs](../../crates/sim-model/src/m3prop.rs), and [m3pprop.rs](../../crates/sim-model/src/m3pprop.rs)).

BranchResult contains each branch's per-day State digests, Metrics and Events digests and records, snapshots at requested existing boundaries, first-divergence details, the per-day |D_d| curve and component-difference flags, lineage-matched birth pairs, unmatched births, newborn CRN-loss count, and forced-action outcome (forced_applied or forced_fallback). The API does not add fields to existing canonical digests or snapshot formats.

### 11.2 Resume and intervention boundary

The snapshot cursor d names the next day to execute. Restore both base and branch at d, then apply the branch intervention at the start of day d, before Phase 1. A config override persists through the remaining horizon; an agent-field override is a one-shot assignment at that boundary; a forced action applies on day d only. Validate the entire request and intervention before mutating either branch state. A missing or dead target AgentId for a field override or forced action is a validation error before mutation.

The M4-00 intervention type order is config override, agent-field override, forced action; agent-targeted entries are ordered by ascending AgentId. M4-02 accepts exactly one intervention, so duplicate-target conflicts are not representable at this API boundary. Interventions do not alter any RNG coordinate or coordinate-to-value mapping ([M4-00 §8, OQ-1](#8-decision-register); [RngCoordinate](../../crates/sim-core/src/prng.rs)).

### 11.3 Forced-action semantics

Natural Phase 4 selection still runs. For the targeted agent, replace only the selected action kind with the forced kind; then perform ordinary target selection and intent construction using the existing predicates, coordinates, ordering, and index formula. If no valid candidate exists, preserve the existing no-candidate outcome and report forced_fallback; otherwise report forced_applied. Targeting a missing or dead AgentId fails before either branch is changed. Phase 4 action selection and target generation remain the source of the unmodified behavior ([phase4_primary_action_selection_into](../../crates/sim-model/src/decision.rs); [generate_intents_m3_into](../../crates/sim-model/src/intents.rs)).

### 11.4 Two-level divergence detection and recorder

1. **Day scan:** after each completed day, compare profile-specific canonical State digests and identify the first differing day. Store per-day base/branch metric and event records and digests, plus |D_d|. D_d counts changed agent records only. Separate daily booleans indicate whether graph CSR, allocator cursor, and propagation rows differ. Day-digest and clone costs are reported separately and excluded from the orchestration budget ([canonical_m3_state_hash, canonical_m3p_state_hash, canonical_m3prop_state_hash, canonical_m3pprop_state_hash](../../crates/sim-model/src/hashing.rs)).
2. **Phase locate:** only if the day scan found a differing day, rerun that day from both exact day-start states using the WorldState generic loop and an opt-in recorder. After every core phase and registered hook, record component hashes for canonical core agent rows in AgentId order, graph CSR, allocator cursor, propagation rows, and metrics-so-far. The recorder hashes components independently and does not invoke whole-profile validation, so the Phase 9b→9c interval remains recordable even while propagation rows await newborn synchronization ([run_registered_world_day_loop](../../crates/sim-model/src/runner.rs); [sync_propagation_rows](../../crates/sim-model/src/m3pprop.rs)).

Report the first differing (day, phase slot), the first differing slot where applicable, every differing component, the hook SystemId when the slot is a registered hook, and that descriptor's RNG subsystem IDs ([SystemDescriptor::rng_subsystem_ids](../../crates/sim-model/src/system_registry.rs)). A difference already present immediately after intervention application is reported with slot InterventionBoundary. Recorder output is transient and does not enter simulation state, snapshots, or canonical digest schemas.

### 11.5 Divergence sets, lineage, and CRN

For every day, match agents present at the branch point by AgentId. Match later births by lineage key (ancestor AgentId present at the branch point, ordinal path). A parent's birth ordinal is the zero-based count of its successful births since the branch point in Phase 9b processing order. A child of a branch-point agent has path [ordinal]; a child of a later-born parent appends its own ordinal to its parent's lineage path. Derive ordinals and lineage keys in the analysis layer from ordered birth records; do not store them in state, snapshots, or digests ([BirthRecord, plan_phase9b_from_view](../../crates/sim-model/src/m3p.rs)).

Report matched and unmatched births. Keep the existing AgentId-based RNG identity and report newborn CRN loss as the number of lineage-matched births whose AgentIds differ. Divergent birth counts may shift next_agent_id and subsequent newborn RNG coordinates; this is expected and does not change M3P ([M3pReferenceState::next_agent_id, plan_phase9b_from_view](../../crates/sim-model/src/m3p.rs); [RngCoordinate](../../crates/sim-core/src/prng.rs)).

### 11.6 Correctness gates and test list

- Existing M2/M3/M3P/propagation goldens, canonical digests, snapshot bytes, and M4-01 registry gates pass unchanged.
- Base-branch identity with a no-op intervention passes for M3, M3P, M3+propagation, and M3P+propagation for all H days: equal State digests, metric/event records and digests, snapshots, recorder traces, and |D_d| = 0.
- A field override identifies the first divergent day; a forced-action case verifies action replacement, ordinary target selection, fallback reporting, and unchanged RNG coordinates.
- A recorder differential locates only the first divergent day, including the first phase/hook, differing slot and components, and registered descriptor subsystem IDs. Include M3P+propagation Phase 9b→9c.
- Repeated identical requests produce identical branch outputs and detector results.
- Snapshot pause/resume preserves both branch outputs and divergence curves; mutating one branch does not affect the other branch or source snapshot.
- Invalid, missing, dead, or unsupported intervention targets/options fail before mutation with stable typed errors.
- M3P cases cover equal and divergent birth streams, nested lineage paths, matched/unmatched births, AgentId shifts, and the reported CRN-loss count.
- Divergence-set tests cover unchanged and changed agent records, deaths, births, and DenseSlot permutation invariance.

### 11.7 Performance gate

Use the M4-01 warmup/sample/MAD method for paired base-plus-branch runs from the same snapshot at N = 10,000 and N = 100,000, H = 30. Compare the branch API with two direct runner passes from that snapshot. Added orchestration cost for intervention dispatch, day-to-day result comparison, and result assembly must be at most max(5% of the paired direct-run median, the larger paired MAD). Measure snapshot restore, day-digest, and day-start clone costs separately and exclude them from this budget.

Measure phase-locate replay separately. Recorder overhead must be at most max(10% of the two ordinary one-day passes, their larger MAD). Record all cells and host details.

### 11.8 Non-goals

- No change to M1/M2/M3/M3P semantics, RNG coordinates, canonical digest domains, event schemas, or snapshot schemas.
- No explicit parallel execution, copy-on-write, cross-branch scheduler, GPU work, minimal-cause search, or new RNG identity profile.
- No per-phase digest in canonical State/Metrics/Events. Recorder hashes are transient.
- No change to M3P AgentId allocation or RNG identity.

### 11.9 Decision register

| ID | Decision | Status |
|---|---|---|
| Q1 | Support M3, M3P, M3+propagation, and M3P+propagation. Use BranchRequest / BranchResult; scan on SegmentedAgentStorage and phase-locate on WorldState. Allow serial and incremental-topology execution (incremental mode is equivalent to serial on static M3 topology); reject explicit parallel with a typed error. | **FROZEN** |
| Q2 | d is the next day to execute. Apply interventions at day d start before Phase 1. Config overrides persist, agent-field overrides apply once, forced actions apply on day d only. | **FROZEN** |
| Q3 | Run natural Phase 4 selection, replace only the chosen action kind, then use ordinary targeting and unchanged coordinates. Record forced_applied or forced_fallback; missing/dead targets fail validation before mutation. | **FROZEN** |
| Q4 | Record component hashes after each core phase and hook for canonical agent rows, graph CSR, allocator cursor, propagation rows, and metrics-so-far; do not use whole-profile validation at transient boundaries. Report first differing slot, components, hook SystemId, and descriptor RNG subsystem IDs; use InterventionBoundary for a boundary difference. | **FROZEN** |
| Q5 | Birth ordinals are zero-based successful births per parent since the branch point in Phase 9b order. Lineage key is branch-point ancestor AgentId plus ordinal path; analysis-only, derived from birth records. | **FROZEN** |
| Q6 | Keep AgentId RNG identity and report newborn CRN loss as matched births with differing AgentIds. M3P remains unchanged. | **FROZEN** |
| Q7 | Retain one in-memory day-start clone per branch, replacing it daily; at most two additional states total. | **FROZEN** |
| Q8 | Accept 5% orchestration and 10% recorder budgets. Report clone and digest costs separately and exclude them from those budgets. | **FROZEN** |
| Q9 | D_d counts agent records only; graph, allocator, and propagation differences are separate per-day boolean flags. | **FROZEN** |
| Q10 | Config overrides are limited to the enumerated daily model parameters. Run identity, initialization dimensions, relationship-graph topology settings, and propagation settings carried in snapshot metadata cannot be overridden. | **FROZEN** |

### 11.10 Freeze record

This freeze authorizes the M4-02 implementation task; it does not itself alter any runtime profile. Existing digest domains, snapshot schemas, events, and RNG coordinates remain unchanged.

## §11.7 A-1 — Measurement protocol amendment (FROZEN)

This approved amendment changes how the §11.7 costs are isolated and compared. It does not change the branch semantics, the 5% orchestration limit, or the 10% recorder limit in §11.7.

### Orchestration gate

In one process, pair the branch API with analysis disabled against two direct storage passes from the same snapshot. Use paired samples and both execution orders. The analysis-disabled path performs the same branch simulation and returns the same simulation outputs, but skips day digests, `D_d`, lineage matching, component flags, and day-start clones. Do not subtract estimates from separate probes.

Require the measured analysis-disabled branch API overhead to be at most `max(5% of the direct-pair median, larger paired MAD)`.

### Analysis-cost record

In the same paired samples, measure the full branch API and the analysis-disabled branch API. Report their difference split into digest, `D_d`, lineage, and day-start clone costs in ns per agent-day. This is a regression baseline; M4-02 assigns no absolute limit to it.

### Recorder gate

Compare the lockstep locator with a comparison-disabled lockstep replay that runs the same two states to the same stopping boundary. Report the replay cost itself separately. Require comparison overhead to be at most `max(10% of that replay median, larger paired MAD)`.

### High-variance cells

If a cell has `MAD / median > 5%`, rerun it with 41 samples. If it remains above 5%, report the cell as **inconclusive**, never PASS.

### Freeze record

This user-approved amendment freezes the measurement method for the M4-02 follow-up. Measurement switches are internal and doc-hidden; normal API defaults remain unchanged. The amendment does not mark M4-02 complete or alter any simulation profile, digest, snapshot, event, RNG, or gate result. The supporting historical and follow-up measurements are recorded in the [M4-02 report](../performance/M4_02_BRANCHING.md).

## §11.7 A-2 — Decision rule amendment (FROZEN)

This user-approved amendment supersedes only the A-1 gate decision rule. The paired, same-process measurement protocol remains unchanged. The A-1 measurements remain historical and are not re-judged under A-2. A-1's `MAD / median(delta)` rule cannot pass a near-zero overhead because its denominator approaches zero; A-2 instead compares the measured delta and its spread to the fixed path-relative limit.

### Orchestration cells

For each paired sample, `delta = analysis-disabled API − direct pair`. Let `T = 0.05 × median(direct pair)`. Classify the cell as:

- **PASS** iff `median(delta) + 2 × MAD(delta) <= T`.
- **FAIL** iff `median(delta) > T`.
- **INCONCLUSIVE** otherwise.

Path MADs do not widen `T`.

### Recorder cells

Compare the full one-day locator API against the comparison-disabled one-day locator API; do not use the replay-prefix timing as the decision baseline. For each paired sample, `delta = full locator API − comparison-disabled API`. Let `T = 0.05 × median(comparison-disabled API)`. Apply the same three-way classification: **PASS** iff `median(delta) + 2 × MAD(delta) <= T`; **FAIL** iff `median(delta) > T`; otherwise **INCONCLUSIVE**. Path MADs do not widen `T`. Report the comparison-disabled API time and replay cost separately.

### Host validity and retry

Record total CPU load every 30 seconds during the run and establish the pre-run idle baseline. If any load sample during a cell exceeds that baseline by more than 20%, the cell is invalid and must be rerun once using the same protocol. If the retry is also invalid, report it as **INVALID**. Record the harness process peak working set. An invalid result is not a PASS, FAIL, or INCONCLUSIVE result.

### Freeze record

This approved A-2 amendment replaces A-1's decision rule only. Pairing, same-process execution, analysis-disabled and comparison-disabled paths, warmups, sample counts, execution-order alternation, and other measurement protocol details remain as frozen in A-1. Preserve A-1 results as superseded history; do not reinterpret them using A-2.

## §11.7 A-3 — Validity and uncertainty amendment (FROZEN)

This user-approved amendment supersedes only A-2's validity and decision rules. The paired, same-process measurement protocol remains unchanged. A-1 and A-2 results remain historical and are not re-judged. The A-2 host rule counted the benchmark harness's own CPU use as external load; on this 12-logical-processor host, one fully occupied core contributes about 8.3 percentage points. Also, `median + 2 × MAD` describes sample spread, not uncertainty in the median, so adding samples does not turn that spread into a confidence bound.

### Host and path validity

Sample total CPU load every 30 seconds and at cell boundaries. For each sample, compute the harness process's CPU share over the same sampling interval, normalized by the host's logical processor count. Define `other-process CPU = total CPU − harness process CPU share`. Establish the idle baseline as the median of pre-run total-CPU samples. A cell is valid only if every assigned sample has `other-process CPU <= idle baseline median + 5 percentage points` and each of its two timed paths has `MAD / median <= 5%`. For orchestration, the two paths are the direct pair and analysis-disabled API; for recorder, they are the full locator API and comparison-disabled API. If a cell is invalid, rerun it once with the same protocol; if it is still invalid, report **INVALID**.

### Decision rule

For each cell, bootstrap the paired signed deltas with replacement. Initialize a xorshift64 generator to `0xA3_11_70` independently for each cell; advance it with `x ^= x << 13; x ^= x >> 7; x ^= x << 17`. Use 10,000 bootstrap resamples, each of the original sample count, and sample indices `next_u64 % sample_count`. For each resample, calculate the median delta. Sort the 10,000 resampled medians and define `U` as the nearest-rank one-sided 95th percentile (the 9,500th value, one-based). Report the sample median delta, its MAD, `U`, and `T`.

For orchestration, `delta = analysis-disabled API − direct pair` and `T = 0.05 × median(direct pair)`. For recorder, `delta = full locator API − comparison-disabled API` and `T = 0.05 × median(comparison-disabled API)`; replay-prefix timing remains a separate reported cost. Classify a valid cell as **PASS** iff `U <= T`; **FAIL** iff `median(delta) > T`; otherwise **INCONCLUSIVE**. Path MADs do not widen `T`.

### Freeze record

This approved A-3 amendment replaces A-2's validity and decision rules only. Pairing, same-process execution, compared paths, warmups, timed sample count, execution-order alternation, and other measurement protocol details remain unchanged. A-1 and A-2 measurements are retained as historical results and are not re-judged under A-3.

## §11.7 A-4 — Path-spread validity amendment (FROZEN)

This user-approved amendment replaces the A-3 host-load validity test for M4-02 and sets the 41-sample protocol for this gate; the A-3 decision rule is unchanged. Its rationale is that Windows per-process CPU counters proved unreliable in the prior attempts (a zero rate-counter sample and a lost trace), while host total CPU includes the benchmark process itself. Do not use per-process CPU counters for validity.

### Validity and sampling

A cell is valid iff both timed paths have `MAD / median <= 5%`. Record total CPU load and free RAM before and after the run as host information only; neither value affects validity. Use two warmups and 41 timed samples per cell. If a cell is invalid, rerun it once with the same protocol; that result is final, and a cell still invalid after its retry is reported as **INVALID**.

### Decision rule

The A-3 deterministic percentile bootstrap and thresholds are unchanged. Bootstrap the paired signed deltas with replacement using 10,000 resamples, each the original sample count, and the fixed xorshift64 procedure and seed defined in A-3. Let `U` be the nearest-rank one-sided 95th percentile of bootstrap median deltas. Report median(delta), MAD(delta), `U`, and `T`. For orchestration, `T = 0.05 × median(direct pair)`; for recorder, `T = 0.05 × median(comparison-disabled API)`. **PASS** iff `U <= T`; **FAIL** iff `median(delta) > T`; otherwise **INCONCLUSIVE**. Path MADs do not widen `T`.

### Freeze record

This approved A-4 amendment replaces A-3's host-load validity rule and sets the 41-sample protocol for the M4-02 gate; the A-3 bootstrap decision rule and threshold definitions remain frozen. It does not re-judge A-1, A-2, or A-3 measurements. No further M4-02 measurement-rule amendment is part of this task.

## §11.7 A-5 — Batched sample amendment (FROZEN)

This user-approved amendment supersedes only how one timed sample is built. Short 48–165 ms samples are comparable to the roughly 15.6 ms Windows scheduling granularity, which caused path MAD to exceed 5% in some 10k cells; all 100k cells, with 1–8 s samples, had 1–2% path MAD. All A-4 validity, decision, threshold, retry, host-information, and bootstrap rules remain unchanged.

During warmup, choose a repetition count `r` once per cell so one timed sample lasts at least one second, then keep `r` fixed for all samples and any retry of that cell. Each timed sample repeats its paired operation `r` times back-to-back and reports per-operation time as the sample time divided by `r`. Preserve the paired-path alternation order within each sample. Use 41 timed samples; apply A-4 validity and its one retry, then A-3's deterministic bootstrap rule and frozen threshold. Report `r` for every cell.

### Freeze record

This approved A-5 amendment changes only sample construction to reduce short-cell scheduling noise. It keeps the existing 41-sample count, validity rule, bootstrap decision, thresholds, and one-retry limit.

## M4 standard performance-gate protocol (FROZEN)

Unless a later M4 stage contract specifies otherwise, use paired measurements in one process with the stage's declared baselines and two warmups. During warmup, choose `r` once per cell so a sample lasts at least one second; each sample repeats the paired operation `r` times with the paired-path alternation order preserved, and per-operation time is the sample time divided by `r`. Keep `r` fixed through samples and a retry. Use 41 timed samples per cell. Validity requires both paths to have `MAD / median <= 5%`; report host CPU and free RAM before and after as information only; rerun an invalid cell once. Compute `U` with the deterministic 10,000-resample bootstrap fixed in §11.7 A-3/A-4. Apply the stage's frozen `T` using **PASS** iff `U <= T`, **FAIL** iff `median(delta) > T`, and **INCONCLUSIVE** otherwise. Report `r`, median(delta), MAD(delta), `U`, `T`, path medians and MADs, host details, and all retry outcomes.

## 12. M4-03 Divergence Measurement

**Status: FROZEN**

**Scope:** measure how differences in an M4-02 branch evolve over time. Branch execution, interventions, profile digests, snapshots, and event bytes remain unchanged. The M4-03 series is analysis output only.

### 12.1 Divergence curve and denominator

For each executed day `d`, `D_d` is the frozen M4-02 set of matched logical agent records whose canonical agent fields differ at the day boundary. Entities present at the branch point match by `AgentId`; later births match by the frozen lineage key. `|D_d|` remains the existing agent-record-only `DailyDifference.changed_agent_records`; it is not redefined by this section ([`DailyDifference`, `difference_for_day_fast_impl`, `difference_for_day_canonical`, `record_births`, `record_shared_births`](../../crates/sim-model/src/counterfactual.rs)).

Let `L_d` be the number of matched logical entities with an agent record on at least one side at day `d`, whether alive or retained dead. The normalized divergence is `|D_d| / L_d`, with value zero when `L_d = 0`; this keeps the fraction at or below one. The prior alive-only denominator could produce peak fractions greater than one because dead records remain in the matched divergence set. Let `u_d` be the count of unmatched post-branch birth records present on either side at that boundary, summed across the base-only and branch-only records; a retained dead record is included while it remains in state. Report `u_d` separately and report the unmatched-birth fraction `u_d / (L_d + u_d)`, with value zero when the denominator is zero. Existing `unmatched_base_births`, `unmatched_branch_births`, and `newborn_crn_loss` remain available unchanged.

### 12.2 Component differences

Keep `graph_changed`, `allocator_changed`, and `propagation_changed` separate from `D_d`. Define `settlement_global_changed` as any difference in canonical State bytes outside agent rows, graph, allocator, and propagation components. This is the catch-all component; together these partitions cover canonical State. The implementation compares the fields present in the canonical State encoding—`current_day` and settlements (including resources and treasuries)—while keeping agent `group_id` inside the agent record. `initial_money_supply` is not part of the current canonical State encoding and is not included in this flag ([`WorldState`, `SettlementState`](../../crates/sim-model/src/state.rs); [`DailyDifference`, `difference_for_day_fast_impl`](../../crates/sim-model/src/counterfactual.rs)). Metrics and event differences remain outcome observations, not State components.

For every day, test the partition invariant: base and branch State digests differ **iff** `|D_d| > 0` or at least one of `graph_changed`, `allocator_changed`, `propagation_changed`, or `settlement_global_changed` is true. The first-divergence record continues to report the first slot, differing components, hook `SystemId`, and that system's RNG subsystem IDs using M4-02's recorder ([`FirstDivergence`, `first_phase_difference`, `run_counterfactual_branch_inner`](../../crates/sim-model/src/counterfactual.rs); [`SystemDescriptor`, `SYSTEM_REGISTRY`](../../crates/sim-model/src/system_registry.rs)).

### 12.3 Cascade and spread metrics

Report the first divergent day separately from cascade onset. If `|D_d| / L_d >= 0.5` on the first divergent day, classify the run as **immediate global divergence** and do not evaluate cascade onset. Otherwise, starting after the first divergent day, let `m` be the largest `|D|` on earlier days since first divergence. Cascade onset is the first day `d` where `|D_d| >= max(2m, m + 5)`. Plateau length is `onset_day - first_divergent_day`. If no day meets the threshold, report **no cascade**.

From cascade onset through the earlier of ten daily observations or the peak day (inclusive), compute the least-squares slope of `ln(1 + |D_d|)` against day offset. Report doubling time `ln(2) / slope` only when slope is positive; otherwise it is absent. No linear-growth slope is reported. Do not report saturation. Report only peak `|D_d|`, peak normalized fraction, and elapsed days from first divergence to peak in addition to onset, plateau, slope, and doubling time. Ties for peak use the earliest day. The growth slope is absent if fewer than two daily observations are available.

For each intervention targeting an agent, compare cascade onset with that agent's first death day in each run, as recorded by `StateTransitionEvent::MortalityCommitted`; report before, same day, after, or no death during the horizon. This is temporal association, not causal attribution ([`StateTransitionEvent::MortalityCommitted`, `events_from_mortality_resolution`](../../crates/sim-model/src/events.rs); [`run_counterfactual_branch_inner`](../../crates/sim-model/src/counterfactual.rs)).

### 12.4 Population-regulation handling

Use the existing M3P profile unchanged and report alive population per day. Revisit whether a population-regulated profile is needed before M4-04. M4-03 does not add or alter population regulation; any future regulation requires a distinct profile and its own contract.

### 12.5 Gates and tests

- CI identity gate: `N = 2,000`, horizon `H = 30`, no-op intervention, all four supported combinations: M3, M3P, M3+propagation, and M3P+propagation.
- CI intervention gate: `N = 2,000`, one fixed seed, `H = 60`, one case for each M4-00 intervention type: configuration override, agent-field override, and forced action.
- CI cascade gate: a deterministic flat-then-cascade series verifies the frozen onset threshold, plateau, slope, doubling time, and no-cascade behavior. The example uses `|D_d| = 3` for 11 days followed by a series crossing the defined threshold.
- CI golden gate: commit new M4-03 divergence-series golden files; do not alter any M2/M3/M3P/propagation golden, snapshot, CI file, or existing test.
- CI budget: added M4-03 tests must take no more than 60 seconds total. Multi-seed and `N = 10,000` runs are report-only.
- Every daily State digest differs iff `|D_d| > 0` or one or more component flags are true. Cover agent records, graph, allocator, propagation, and settlement/global fields independently.
- Verify deterministic output, lineage matching, target-death day, forced-action comparison with the natural Phase 4 choice, and unchanged M4-02 outputs. Report `forced_matches_natural = true` when the forced action equals that natural choice; this is an expected no-op.

### 12.6 Performance gate

Use the [M4 standard performance-gate protocol](#m4-standard-performance-gate-protocol-frozen), with `T = 5%`. For each cell, compare the full branch API returning the M4-03 divergence series against the M4-02 full branch API without the series, using identical snapshots, intervention, execution option, horizon, and run identity. Required cells are M3P serial at `N = 10,000` and `N = 100,000`, and M3+propagation serial at `N = 100,000`. A cell passes only under the standard protocol's A-5 validity and bootstrap decision rules. If any cell is not PASS, stop before report-only experiments and do not mark M4-03 complete.

### 12.7 Decision register

| ID | Frozen decision |
|---|---|
| OQ-12.1 | Normalize `|D_d|` by `L_d`, the matched logical entities with a record on at least one side, alive or retained dead (`L_d = 0` gives zero); this prevents fractions above one. Report `u_d`, the base-only plus branch-only post-branch birth records present at the boundary, separately and as `u_d/(L_d+u_d)` (`0` when the denominator is zero). Frozen `D_d` is unchanged. |
| OQ-12.2 | If the first divergent day's `|D_d|/L_d >= 0.5`, report immediate global divergence and do not evaluate cascade onset. Otherwise, cascade onset is the first day after first divergence with `|D_d| >= max(2m, m+5)`, where `m` is the maximum on earlier days since first divergence. Plateau is onset minus first divergent day; otherwise report no cascade. |
| OQ-12.3 | Growth is the least-squares slope of `ln(1+|D_d|)` from onset over at most ten daily observations and ending no later than the peak. Doubling time is `ln(2)/slope` only for positive slope. No linear slope. |
| OQ-12.4 | No saturation output. Report peak `|D|`, peak normalized fraction, and days to peak. |
| OQ-12.5 | `settlement_global_changed` is the catch-all difference in canonical State bytes outside agent rows, graph, allocator, and propagation. Enforce the daily State-digest partition invariant in §12.2. |
| OQ-12.6 | Required CI cases are the `N=2,000` identity gate for all four combinations (`H=30`), one `H=60` seed for each intervention type, a flat-then-cascade case, and new divergence-series goldens. Added CI time is at most 60 seconds. Multi-seed and `N=10,000` cases are report-only. |
| OQ-12.7 | Performance compares the full M4-03 API against the M4-02 full API under A-5, `T=5%`, for M3P serial at 10k/100k and M3+propagation serial at 100k. |
| M4-00 OQ-9 | Keep M3P unchanged and report alive population per day. Revisit whether regulation is needed before M4-04. |
| A-1 forced-action observation | Report `forced_matches_natural = true` if the forced action equals the natural Phase 4 choice; this is an expected no-op. |

### 12.8 Freeze record

This freeze defines M4-03 divergence normalization, unmatched-birth reporting, component partitioning and its invariant, cascade and spread metrics, required CI gates, and the A-5 performance comparison. It authorizes the M4-03 implementation task to be requested; it does not start that implementation.

### 12.9 A-1 — Normalization and classification amendment (FROZEN)

This user-approved amendment supersedes only the `L_d` definition in §12.1 and OQ-12.1, the first-day classification in §12.3 and OQ-12.2, and the forced-action reporting rule in §12.5. `L_d` counts matched logical entities with a record on either side, alive or retained dead; the old alive-only denominator could yield peak normalized fractions above one. A first-divergent-day fraction of at least `0.5` is classified as **immediate global divergence**, with cascade onset left unevaluated. For a forced action, report `forced_matches_natural = true` when it matches the natural Phase 4 choice. No branch execution, M4-02 result, digest, snapshot, RNG, event, or intervention semantics change.

## 13. M4-04 CRN Efficiency

**Status: FROZEN**

**Scope:** quantify how much pairing a base run and an intervention branch with common random numbers (CRN) reduces uncertainty in declared scalar outcomes. The replicate-set runner supports M3P and M3P+propagation, whose snapshots preserve the graph. M4-04 adds analysis only; it does not change intervention, simulation, RNG, digest, snapshot, or M2/M3/M3P semantics.

### 13.1 Declared outcomes

Each experiment declares one or more scalar outcomes `Y`, an extraction rule, and an observation boundary before runs begin. Examples include:

- `alive_population(H)`, from `DailyMetrics.population` or `M3pDailyMetrics.live_population`;
- `total_wealth(H)`, the sum of all canonical agent-record wealth values present at the day boundary, including retained-dead records, plus all settlement treasuries;
- `divergent_records(H) = |D_H|`, from the M4-03 `DivergenceDayRecord.changed_agent_records`;
- `settlement_treasury(g,H)` or another single settlement-level scalar, with the chosen `GroupId` declared in advance.

Phase 10 currently records population, wealth Gini, food reserves, and total treasury, but not total agent wealth; wealth outcomes therefore require extraction from the requested day snapshot. Sum agent `Money` values in ascending `AgentId` order and treasury values in ascending `GroupId` order using checked `i128`; for target-settlement wealth, sum all records assigned to that group, including retained-dead records, plus that settlement's treasury. A compaction that removes dead records changes the extracted population exactly as it changes canonical state ([`DailyMetrics`, `M3DailyMetrics`](../../crates/sim-model/src/metrics.rs); [`M3pDailyMetrics`](../../crates/sim-model/src/m3p.rs); [`SettlementState`, `SegmentedAgentStorage`](../../crates/sim-model/src/state.rs); [`canonical_m3p_state_bytes_from_storage`](../../crates/sim-model/src/hashing.rs)). Mean health sums living agents' `f32` health values as `f64` in ascending `AgentId` order and divides by the alive count; it is undefined if no agents are alive. Treat each settlement outcome as its own scalar rather than combining a vector of settlements into an unspecified statistic.

### 13.2 Paired and independent estimators

All runs start from the same canonical logical branch-point snapshot, including identical world, graph, allocator, and propagation state. Re-encode only the snapshot envelope's `replicate_id` for each post-branch run identity; preserve the M3P graph and other state sections byte-for-byte. For paired replicate `s`, the base and intervention use identical `(master_seed, replicate_id)` values. Let `Δ_s = Y(intervention,s) - Y(base,s)`. With `n` pairs, estimate the intervention effect by `mean(Δ_s)` and its sampling variance by `s²_Δ / n`, where `s²_Δ` is the sample variance of paired differences.

For the independent comparison, run `n` base samples and `n` intervention samples from disjoint, independently assigned post-branch replicate IDs. Estimate the same effect by `mean(Y_I) - mean(Y_B)` and its sampling variance by `(s²_I + s²_B) / n`. The CRN efficiency ratio is the independent estimator variance divided by the paired estimator variance. A ratio above one indicates variance reduction from pairing; below one indicates higher variance under pairing. This follows the framework definition in [M4-00 §2](#2-terminology).

Coordinate RNG is keyed by seed, replicate, day, phase, subsystem, entity, and draw index, so paired branches share values only for coordinates both execute ([`RngCoordinate`, `coordinate_prng_u64`, `coordinate_prng_f32`](../../crates/sim-core/src/prng.rs)). Divergent state can change which draws occur; the estimator remains a paired experimental design and must report that limitation rather than assume every random draw was shared ([`run_counterfactual_branch`, `prepare_branch`](../../crates/sim-model/src/counterfactual.rs)).

### 13.3 Replicate count and confidence method

Use `100` paired run identities and independent samples of `100` base plus `100` intervention run identities for each outcome and configuration. Paired and independent-base runs use post-branch replicate IDs `{1..=100}`; independent-intervention runs use `{100001..=100100}`. Thus, the independent base and intervention samples are disjoint, while the paired base trajectories are the same base sample used by the independent estimator. Reuse those base values; do not run a second independent-base set. All runs use the same `master_seed` and canonical logical branch-point snapshot, with only the post-branch `replicate_id` changing.

Report the sample means, estimated variances, variance ratio, and a two-sided 95% percentile-bootstrap interval for that ratio, using 10,000 deterministic bootstrap resamples and xorshift64 seed `0xA3_11_70` from the M4 standard performance protocol. Each resample draws paired observations as `(base, intervention)` pairs; the resampled paired-base values are also used as the independent-base sample, and independent-intervention values are resampled separately. Compute the paired-mean 95% percentile interval from the same paired resamples. A bootstrap resample with zero paired variance is assigned an infinite ratio, including the indeterminate zero-over-zero case, yielding a conservative upper interval.

If the observed paired variance itself is exactly zero, use the frozen M4-00 output **“exact (paired variance 0)”** separately from the ratio; do not report a finite ratio ([M4-00 OQ-5](#8-decision-register)).

### 13.4 Newborn identity and CRN loss

Report M4-03 `newborn_crn_loss` alongside each paired outcome comparison. This is the number of lineage-matched births whose AgentIds differ, not a claim that all other random coordinates were shared ([`DivergenceDayRecord`, `DivergenceSummary`](../../crates/sim-model/src/counterfactual.rs); [`M3pReferenceState::next_agent_id`, `plan_phase9b_from_view`](../../crates/sim-model/src/m3p.rs)). In the corrected M4-03 report-only experiments, loss remained zero for the 20 H=90 cases and the ten H=365 food/health runs ([A-1 experiment results](../performance/M4_03_DIVERGENCE.md#a-1-corrected-experiment-rerun)).

Keep OQ-4 option A, AgentId-based RNG identity; newborn CRN loss was zero in all 20 H=90 and ten H=365 report-only runs. Report any nonzero loss and interpret the CRN ratio in light of the unmatched newborn coordinates. A lineage-keyed RNG identity would be a new profile decision; it must not alter M3P ([`RngCoordinate`](../../crates/sim-core/src/prng.rs); [`apply_phase9b_storage`, `plan_phase9b_from_view`](../../crates/sim-model/src/m3p.rs)).

### 13.5 Population-regulation question

Keep M3P unchanged; a separate population-regulated profile is not required for this stage. Across the ten H=365 corrected runs, alive population stayed between 10,000 and 10,313, within 3.13% of the initial population ([M4-03 H=365 results](../performance/M4_03_DIVERGENCE.md#a-1-corrected-experiment-rerun)). Report population with each outcome. Reconsider regulation for substantially longer horizons or a different workload; any density-dependent rule requires a new profile and its own contract.

### 13.6 Gates and test list

The CI gate uses M3P at `N = 2,000`, `n = 10`, `H = 30`, one agent-field intervention, and a new committed estimator golden. The added test target must take at most 60 seconds. Tests cover:

- deterministic repeated values for every declared outcome and run identity;
- a no-op intervention identity case with paired variance zero, reported using the exact M4-00 phrase;
- exact paired branch outputs against `run_counterfactual_branch` and the M4-03 series output, including snapshots at declared boundaries;
- estimator unit cases with hand-computable paired and independent samples, including a zero paired variance and bootstrap reproducibility;
- disjoint independent run identities, equal sample counts, and explicit rejection of missing or non-finite outcome values;
- reporting newborn CRN loss and daily alive population for M3P; existing M2/M3/M3P goldens, snapshots, and branch results remain unchanged.

The base and intervention runs use the public snapshot-backed branch API; the analysis reads the returned `BranchDayRecord` metrics/events and M4-03 series without changing the run result ([`BranchRequest`, `BranchDayRecord`, `run_counterfactual_branch`, `run_counterfactual_branch_with_divergence_series`](../../crates/sim-model/src/counterfactual.rs)).

### 13.7 Measurement plan

Run M3P sustained at `N = 10,000`, master seed `81985529216486895`, target chosen as in M4-03, interventions food=0, health=0.01, and BaseMetabolicCost +1%, and horizons `{7, 30, 90, 365}`. For every intervention × horizon × outcome, report paired and independent mean effects, paired and independent estimator variances, the ratio with its 95% interval or the exact-zero phrase, mean `|D_H|` from paired runs, and newborn CRN loss. `|D_H|` is context only and is not defined for independent runs.

Report simulation time, snapshot construction/extraction, outcome extraction, and bootstrap time separately. Use the M4 standard performance-gate protocol for any paired timing comparison; do not include analysis timers in the primary uninstrumented runtime measurement. These are research measurements, not a production crossover gate.

### 13.8 Decision register

| ID | Frozen decision |
|---|---|---|
| D-1 | Use 100 paired replicates; independent base IDs are `{1..=100}` and intervention IDs `{100001..=100100}`. Paired IDs are `{1..=100}`. |
| D-2 | Keep the canonical world, graph, allocator, and propagation state identical across runs; re-encode only the M3P snapshot envelope/run context for each post-branch replicate ID. |
| D-3 | Estimate paired and independent mean effects and variances as in §13.2. Estimate the variance ratio with a deterministic 10,000-resample, two-sided 95% percentile bootstrap using seed `0xA3_11_70`; use paired resamples for the paired mean interval. |
| D-4 | If observed paired variance is zero, report **“exact (paired variance 0)”** and no ratio. Bootstrap resamples with zero paired variance map to an infinite ratio. |
| D-5 | Outcomes at day H are alive population; total wealth over all agent records plus all settlement treasuries; mean health of living agents; target-settlement living population; and target-settlement wealth including its treasury. `|D_H|` is paired-only context. |
| D-6 | Horizons are `{7, 30, 90, 365}`. The report-only workload is M3P sustained at N=10,000, master seed `81985529216486895`, with food=0, health=0.01, and BaseMetabolicCost +1% interventions. |
| D-7 | The first replicate-set API supports M3P and M3P+propagation, whose snapshots preserve graph state. M3 and M3+propagation are deferred because their snapshots omit the graph and changing replicate identity would regenerate branch-point topology. |
| D-8 | Keep AgentId-based newborn RNG identity; measured newborn CRN loss is zero. Keep M3P unchanged; measured H=365 population drift is within 3.13%. |
| D-9 | CI uses M3P, N=2,000, 10 replicates, H=30, one agent-field intervention, and a new golden; added CI time is at most 60 seconds. |

### 13.9 Freeze record

This freeze defines M4-04 outcomes, replicate identities, estimators, bootstrap reporting, initial profile support, CI gate, and report-only experiment matrix. The replicate runner reuses M4-02 branch execution and leaves all simulation outputs unchanged. It authorizes M4-04 implementation and measurement; it does not mark M4-04 complete.

## 14. M4-05 Minimal-Cause Search

**Status: FROZEN**

**Scope:** search finite candidate intervention sets for a minimum-cardinality set satisfying a declared predicate over the M4-04 scalar outcomes. The M4-02 single-intervention API and all simulation semantics remain unchanged. M4-05 adds a batch analysis API for serial `m3` and `m3p` runs. Every intervention in a batch applies at the branch-point cursor boundary; a forced action targets that cursor day.

### 14.1 Intervention batches

The existing `BranchRequest` contains one `BranchIntervention`; `prepare_branch` applies one config override, one field override, or one forced-action directive. M4-05 adds a separate identified batch request and leaves the M4-02 types and API unchanged. The batch result contains paired base and intervention day records for the requested horizon. A batch containing one intervention must be bit-identical to `run_counterfactual_branch`, including error result and precedence, State/Metrics/Events digests and records, and snapshot bytes. The forced-action candidate's day must equal the branch-point cursor for this single-item equivalence gate ([`BranchRequest`, `BranchIntervention`, `run_counterfactual_branch`, `prepare_branch`](../../crates/sim-model/src/counterfactual.rs)).

Reject the whole batch before mutating branch state if any target is invalid or duplicate. Distinct config parameters may appear together. Reject duplicate targets using M4-00 §2 rules: repeated config parameter, repeated `(AgentId, field)`, or repeated `(AgentId, day)` forced action. Apply classes in `(a) config overrides, (b) agent-field overrides, (c) forced actions` order. Sort config overrides by stable parameter name; within agent-targeted classes sort by ascending `AgentId`, then field name or day as a secondary key. Validate each typed value, apply config overrides to a cloned config in this order, validate the final config once, validate all targets, and only then apply state overrides. All forced-action candidates in the batch target the branch-point cursor day and distinct agents ([`apply_config_override`, `prepare_branch`, `BranchRuntime::validate_live_target`, `ForcedActionDirective::apply_choices`](../../crates/sim-model/src/counterfactual.rs)).

### 14.2 Intervention IDs and candidate builders

Every candidate receives a canonical ASCII ID generated from its typed target and value. IDs use decimal AgentId/day values without leading zeros, stable enum names, and lowercase fixed-width hexadecimal bit values:

- food or health field override: `F:agent=<id>:<field>=0x<8-hex-f32-bits>`;
- wealth field override: `F:agent=<id>:wealth=0x<16-hex-i64-two's-complement-bits>`;
- config override: `C:<ConfigParameterName>=0x<bits>`, with eight hex digits for `f32`/`u32` and sixteen for `Money`;
- forced action: `A:agent=<id>:day=<d>:<stable-action-name>`, where action names are `work`, `buy_food`, `sell_food`, `give_food`, `steal_food`, or `idle`.

Encode config parameter IDs using the exact PascalCase names of `ConfigParameter`; indexed variants use `ActionBias[<index>]` and `BaseWeight[<action-index>,<feature-index>]`. Encode fields as `food`, `health`, or `wealth`; `Money` uses 16 lowercase hex digits for the i64 two's-complement bits. Generate candidate sets only from caller-declared finite domains. Agent-field builders take a sorted AgentId list (optionally filtered to one settlement), field, and allowed values; config builders take an explicit list of parameters and values; action builders take an explicit agent list, the branch-point cursor day, and an action list. Generate each typed candidate once, reject duplicate IDs, then sort by bytewise lexicographic ID string. Rust enum discriminants, locale collation, and caller input order do not define IDs. This ID ordering implements M4-00 §2's lexicographically smallest sorted ID-list tie-break ([`ConfigParameter`, `AgentField`, `BranchIntervention`](../../crates/sim-model/src/counterfactual.rs); [`Action`](../../crates/sim-model/src/decision.rs); [`Money`](../../crates/sim-core/src/primitives.rs)).

### 14.3 Profiles, replicates, and cached base outcomes

Support only serial `m3` and `m3p`. The default paired evaluation uses `R = 10` post-branch replicate IDs `{1..=10}` with the same master seed. Base and intervention evaluations for each replicate use the same ID and therefore the same coordinate PRNG values where coordinates match. An exploratory single-replicate mode requires an explicit replicate ID and is labeled exploratory in results.

Cache the base outcome for every run identity before evaluating candidate subsets; reuse it for all subset comparisons. `m3` snapshots do not store the graph; `BranchRuntime::restore` reconstructs its `M3StorageRunContext` from config and run context. Thus each `m3` replicate deterministically rebuilds its graph, and its base/intervention pair shares that graph. `m3p` restores the relationship graph from its snapshot; the same saved graph is shared across the replicate set. This profile-specific distinction is reported with the experiment and does not change either profile's graph or snapshot semantics ([`BranchRuntime::restore`, `M3StorageRunContext::new`, `M3pStorageRunContext::from_reference_state`](../../crates/sim-model/src/counterfactual.rs); [M3P snapshot decoder](../../crates/sim-model/src/m3p.rs)).

### 14.4 Predicate

At day `H`, compute the scalar effect `Δ_r = Y(branch,r,H) − Y(base,r,H)` using M4-04's canonical outcome extraction. Supported outcomes are world or one-GroupId scoped alive population, total wealth, mean health of living agents, target-settlement alive population, and target-settlement wealth, with M4-04's checked wealth and undefined-health rules. The default paired predicate is evaluated on the arithmetic mean of replicate effects in ascending replicate-ID order; it compares against a declared threshold using exactly one of `<`, `<=`, `>`, or `>=`, with no epsilon. Equality comparison is not supported. Exploratory mode applies the predicate to its one effect. The predicate is part of the request identity ([`CrnOutcome`, `extract_outcomes`](../../crates/sim-model/src/crn_efficiency.rs); [M4-04 §13.1–13.3](#13-m4-04-crn-efficiency)).

Do not support arbitrary per-record health/food/wealth predicates in this stage. `D_H` membership would require snapshot-level record comparison because current `DivergenceDayRecord` stores counts rather than a set of AgentIds; M4-03 also showed 98.4–99.1% record divergence at H=365 while M4-04 found a much smaller health-intervention total-wealth effect concentrated in the target settlement ([`DivergenceDayRecord`](../../crates/sim-model/src/counterfactual.rs); [M4-03 H=365 results](../performance/M4_03_DIVERGENCE.md#a-1-corrected-experiment-rerun); [M4-04 results](../performance/M4_04_CRN_EFFICIENCY.md#health_0_01)).

### 14.5 Search guarantees

For `|I| <= 10`, exhaustively enumerate subsets in increasing cardinality and, within each cardinality, in lexicographic order of their sorted ID lists. Stop at the first satisfying subset. Its guarantee is `minimum`; the order preserves M4-00's tie-break ([M4-00 §2](#2-terminology)).

For `|I| > 10`, run deterministic `ddmin` starting from the complete candidate set. Partition the sorted IDs into contiguous chunks; try complements in chunk order, refining granularity deterministically until no reduction succeeds. Afterward check every one-item removal, repeating reductions until none passes. If the full set satisfies the predicate and that verification completes, guarantee `1-minimal`: no single remaining item can be removed while keeping the predicate true. This is not a minimum-cardinality guarantee. If the initial full set does not satisfy, report no found cause with guarantee `none` rather than claiming no subset can satisfy. In all cases apply the lexicographic tie-break when selecting among satisfying sets of equal observed size.

### 14.6 Limits, resume, and JSONL evaluation log

Each request requires positive `max_branch_evaluations` and `max_wall_time`. One branch evaluation is one profile run for one run identity at horizon `H`; the `R` base runs used to fill the cache and the `R` intervention runs for each subset count against the maximum. A cache hit consumes the same logical evaluation slot as its original run. On either limit, return status `incomplete`, the smallest satisfying set found so far (lexicographically smallest among ties), guarantee `none`, the logical evaluation count, and the stop reason.

Write an appendable JSONL log with a header containing schema version and a SHA-256 request fingerprint over profile, snapshot identity/bytes, config, candidates, predicate, horizon, evaluation mode, and replicate IDs. Limits and output path are stored separately so a caller may explicitly resume with larger budgets. The log records the cached base outcome per replicate and each subset evaluation with sorted subset IDs, per-replicate effects, mean effect, predicate value/result, and evaluation error if any. Cache records are accepted only when the request fingerprint matches. With the same request and limits, replaying the log must produce the identical result and normalized log; increasing a limit is an explicit continuation. Wall-time interruption is recorded in the log so replay of that same request returns the same incomplete result instead of silently continuing farther.

`max_branch_evaluations` counts base and intervention runs, including logical cache hits, so result guarantees do not depend on cache warmth. A JSONL output/read-write failure is an error, not an incomplete search result. The log and search result are transient analysis artifacts: they do not modify simulation State, Metrics, Events, snapshots, hashes, or schemas.

### 14.7 Gates and tests

The correctness suite covers batch-of-one exact parity against M4-02; duplicate rejection and class/target ordering; candidate ID stability under shuffled builder inputs; exhaustive minimum and lexical tie-break; `ddmin` one-minimality; resume-log result equality; and an incomplete-limit result that returns the best found set with guarantee `none`. A synthetic predicate has a known minimum and a tie case. The CI gate runs at `N = 2,000` on a small exhaustive candidate set and commits a new M4-05 golden; the added CI tests must take at most 60 seconds.

The timed gate compares batch-of-one with the M4-02 single-intervention API on M3P serial, `N = 10,000`, `H = 30`, with `T = 5%`. It uses the M4 standard A-5 protocol (2 warmups, `r` fixed per cell to make a sample at least one second, 41 paired samples, both orders, path spread validity, deterministic bootstrap, and retry rule). A cell passes only under the A-5 rule and its frozen threshold. No other timed result is a performance gate unless separately specified.

### 14.8 Decision register

| ID | Frozen decision |
|---|---|
| D-1 | Add a batch API beside `BranchRequest`; keep the M4-02 API unchanged. Batch-of-one output, errors, precedence, digests, events, and snapshots are bit-identical to M4-02. |
| D-2 | Use canonical ASCII IDs and bytewise lexicographic sorting as specified in §14.2; the M4-00 tie-break is the lexicographically smallest sorted ID list. |
| D-3 | Reject all duplicate targets before mutation. Allow distinct config parameters; apply `(a),(b),(c)` class order, configs by stable parameter name, then agent-targeted interventions by ascending AgentId. Validate the complete config batch atomically after typed value checks. |
| D-4 | Support only serial `m3` and `m3p`. Rebuild the M3 graph deterministically per replicate; preserve the M3P snapshot graph. Base and treatment in each pair share the same run identity and graph. |
| D-5 | Predicates use M4-04 scalar outcomes scoped to the world or one settlement; effects are `Y(branch)-Y(base)` at H and use only `<`, `<=`, `>`, `>=` against a threshold. Do not support arbitrary record predicates. |
| D-6 | Default to paired CRN with `R=10`, replicate IDs `{1..=10}` and cached base outcomes per replicate. Single-replicate mode requires a declared replicate ID and is labeled exploratory. |
| D-7 | Exhaustive cardinality/lexicographic search for `|I| <= 10` guarantees `minimum`. Deterministic `ddmin` for larger sets guarantees `1-minimal` only after single-removal verification. |
| D-8 | Require evaluation and wall-time limits. Limit results have status `incomplete`, guarantee `none`, best satisfying set found, and JSONL evaluation log. Same request/log/limits replay identically; larger budgets explicitly continue from cache. |
| D-9 | JSONL contains a request header, per-replicate cached base outcomes, and per-subset IDs, replicate effects, mean, predicate value/result, and errors. Logging/cache changes do not enter simulation state or canonical formats. |
| D-10 | CI uses N=2,000, a small exhaustive case with a synthetic known minimum, and a new golden; added CI time is at most 60 s. The A-5 timed gate is batch-of-one vs M4-02 on M3P serial N=10,000, H=30, T=5%. |

### 14.9 Freeze record

This freeze defines M4-05 batch semantics, candidate identity, predicate inputs, paired evaluation and base caching, search guarantees, limits, resume behavior, JSONL output, and gates. It authorizes the M4-05 implementation and experiments in this request; it does not mark M4-05 complete. M4-02/03/04 outputs and all existing profile semantics remain unchanged.

### 14.10 A-1 — Explicit search completion and direct subset evaluation (FROZEN)

This user-approved amendment supersedes only the guarantee values for exhaustive no-cause and limit results in §§14.5–14.8 and adds a direct subset-evaluation helper. Guarantees serialize as snake_case, and the JSONL evaluation-log header schema is version `2`.

- A completed exhaustive search with no satisfying subset returns status `no_cause` and guarantee `exhausted_no_cause`. This proves absence only under the request's profile, replicate set, and paired or exploratory evaluation mode.
- Any branch-evaluation or wall-time limit returns status and guarantee `incomplete`, preserving the best satisfying set found so far.
- For `|I| > 10`, if the initial complete set does not satisfy the predicate, return status `no_cause` with guarantee `unproven_no_cause`. `ddmin` has not tested every subset, so it cannot prove absence. A completed reduction still returns `one_minimal`.
- `evaluate_subset(request, subset_ids)` resolves the sorted, unique IDs against `request.candidates`, runs exactly that paired subset through the existing batch runner, and returns per-replicate effects, their mean, and the predicate result. It performs no padding, search, or JSONL-cache mutation; the request's evaluation and wall-time limits apply.

No simulation transition, intervention, RNG coordinate, digest, snapshot, event, M2–M4 behavior, or search ordering changes. The new M4-05 CI golden is intentionally updated to assert the serialized guarantee schema.

## 15. M4-06 Settlement Migration System

**Status: DRAFT — NOT FROZEN**

**Scope:** propose settlement membership changes as the first new registered M4 system. M4-06 must leave all existing profiles and their canonical outputs unchanged. This section is a design draft; the model, profile, phase ordering, and persistence details below require review before implementation.

### 15.1 Motivation and current runtime boundary

The M3-08 report measured a 100k-agent M3P tick at `165.1 ms` versus `31.6 ms` for M3 at equal live population (`5.23×`); its Phase 9b graph and projection rebuilds dominated much of the difference ([M3-08 equal-live comparison](../performance/M3_08_POPULATION_DYNAMICS.md#equal-live-population-tick-comparison)). M3-09 incremental topology reduced the reported M3P/M3 ratio to `1.49×` by removing those per-day rebuilds ([M3-09 report](../performance/M3_09_INCREMENTAL_TOPOLOGY.md)). Migration tests whether a changing settlement assignment can be added through the registered-system boundary without rebuilding the execution architecture.

`AgentState` stores `group_id`, and `SegmentedAgentStorage` stores it in the `economy.group_id` column with a mutable slice accessor. Initialization currently assigns groups round-robin; the existing Phase 9b population system also assigns each newborn its parent's group. No current production phase migrates an existing agent ([`AgentState`](../../crates/sim-model/src/state.rs); [`initialize_world`](../../crates/sim-model/src/initialization.rs); [`SegmentedAgentStorage::group_ids_mut`, `SegmentedAgentStorage::push`](../../crates/sim-model/src/storage.rs); [`apply_phase9b`, `apply_phase9b_storage_incremental`](../../crates/sim-model/src/m3p.rs)).

Current group membership affects later settlement-specific processing: Phase 3 resolves settlement scarcity; M3 Phase 4 candidates come from the AgentId graph and live agent fields, while its separate cross-partition count compares initiator and target group IDs; Phase 5 buckets intents by group; and Phase 6B output, markets, welfare, metrics, and events use group IDs ([`phase3_observation_and_features_storage_with_scratch`](../../crates/sim-model/src/features.rs); [`generate_intents_m3_storage_into`, `count_cross_partition_interactions_per_day`](../../crates/sim-model/src/intents.rs); [`phase5_partition_intents`, `phase5_partition_intents_baseline`](../../crates/sim-model/src/partitioning.rs); [`phase6b_targeted_resolution_m3`, `phase7_market_clearance_storage`, `phase8_welfare_distribution_storage`](../../crates/sim-model/src/resolution.rs); [`M3StorageRegisteredDay::observe`, `M3StorageRegisteredDay::finish_day`](../../crates/sim-model/src/runner.rs)). A migration performed after Phase 9 affects Phase 10/11 outputs for that day and the next day's Phases 1–8; it cannot retroactively change already committed interactions.

### 15.2 Goal and migration model options

The goal is to model deterministic agent movement between existing settlements through a registered system. A migration decision has a configured probability and cost, and uses coordinate-addressed randomness so agent iteration order or unrelated draws do not shift another agent's decision. M4-06 does not specify the final RNG draw mapping; subsystem ID `9` is reserved below, while draw indices and destination sampling remain open.

Candidate pressure rules:

- **Resource pressure:** compare the current settlement's resource-per-live-agent value with one or more alternative settlements; move toward a settlement with better capacity.
- **Food pressure:** compare a deterministic summary of agent food or settlement food availability and move away from a low-food settlement.
- **Random movement:** choose uniformly from an explicitly ordered set of other settlements, independent of pressure.

**Proposal:** begin with resource-per-live-agent pressure over all other existing `GroupId`s, select the highest-capacity destination that exceeds the source by a declared margin, and break ties by ascending `GroupId`. A move attempt is then admitted by `move_probability` in `[0,1]`. The destination set is ordered explicitly; no hash-map iteration defines the choice. Whether to combine a pressure rule with a random fallback remains open.

For movement cost, options are an agent-food cost, an agent-wealth cost, or a settlement-paid cost. **Proposal:** deduct a finite nonnegative `migration_food_cost` from the moving agent's current food, requiring sufficient food before a move is admitted. This reuses an existing agent column and avoids adding a second treasury transfer rule. Whether an unaffordable agent skips the move or emits a failed-attempt record is open; recommend skip with no RNG redraw and no event.

Process eligible agents once in ascending `AgentId` order, plan all destinations and costs against the Phase 9/9b boundary state, validate the complete plan, then commit group and cost changes. This avoids one migration changing another agent's same-day pressure calculation. The eligibility predicate, whether dead or health-zero agents are excluded, and whether one destination may accept any number of agents must be frozen before implementation.

### 15.3 Registered-system descriptor and phase slot

Add a distinct `SystemId::SettlementMigration` and feature-switch key; `SystemId` is intentionally separate from the numeric RNG subsystem IDs. Reserve RNG subsystem ID `9` for the migration profile only; existing RelationshipGraph, PopulationDynamics, and Propagation descriptors use IDs `6`, `7`, and `8` respectively ([`SystemId`, `FeatureSwitchKey`, `SystemDescriptor`, `SYSTEM_REGISTRY`](../../crates/sim-model/src/system_registry.rs); [`M3P_BIRTH_SUBSYSTEM_ID`](../../crates/sim-model/src/m3p.rs)).

Proposed descriptor:

| Descriptor field | Draft value |
|---|---|
| System identity | `SettlementMigration`; feature key `settlement_migration` |
| Dependencies | RelationshipGraph; order after PopulationDynamics only when that system is enabled |
| RNG subsystem | `9` |
| Phase slot | After core Phase 9 and any Phase 9b population hook, before Phase 9c propagation |
| Owned persistent columns | None for the initial stateless rule; writes the existing agent group and food columns |
| Snapshot contribution | A migration-profile section/version, plus any future migration state if selected |
| Digest contribution | New profile-specific State, Metrics, and Events domains |
| Metrics/events | Daily migrated count and cost; a typed migration event with AgentId, source, destination, and cost |
| Declared access | Read agent identity/alive/health/food/group, settlement identity/resource, and derived live group counts; write agent group and food plus migration metrics/events |

Current `PhaseSlot` has `Phase9B` and `Phase9C` but no distinct slot between them. Both generic day loops invoke `Phase9B` hooks after core Phase 9 and invoke `Phase9C` hooks afterward; registry hooks are sorted by `(PhaseSlot, SystemId)` ([`PhaseSlot`, `validate_system_registry`](../../crates/sim-model/src/system_registry.rs); [`run_registered_world_day_loop_impl`, `run_registered_storage_day_loop`](../../crates/sim-model/src/runner.rs)). **Proposal:** use the existing Phase9C slot, with Migration ordered before Propagation in the registry, so the generic loop bodies remain unchanged. The current registry's order is a stable enum sort, not a topological scheduler; the implementation must prove the chosen descriptor/SystemId order and dependency validation produce migration-before-propagation. Adding a new Phase9D slot that requires editing generic-loop bodies would fail the frozen M4-01 forward gate and must be reported, not worked around.

### 15.4 Relationship graph and population dynamics interaction

The relationship graph is keyed by `AgentId`; its rows are not keyed by storage slot. Phase 4 projects those rows to slots for execution. M3P Phase 9b attaches newborn edges using the parent's current group and updates graph rows only for births/compaction ([`RelationshipGraph`](../../crates/sim-model/src/relationship_graph.rs); [`M3Phase4SlotProjection`](../../crates/sim-model/src/intents.rs); [`plan_phase9b_from_view`, `apply_phase9b_storage_incremental`](../../crates/sim-model/src/m3p.rs)).

Graph choices:

- **Keep identity edges:** existing CSR endpoints follow the same AgentIds after a move. Edges may become cross-partition or local as `group_id` changes; the graph and CSR hash do not change because of movement.
- **Rewire on move:** resample one or both endpoints to preserve a locality target; this mutates CSR and its digest contribution on every affected move and must update the M3-09 incremental topology and snapshot state.

**Proposal:** keep identity edges for M4-06. The graph describes relationships between agents, not their current settlement membership; locality can change as an observed workload property. Rewiring would couple migration to dynamic topology maintenance and obscure the first system-registration gate. Under this proposal, cross-partition edge and interaction metrics are recalculated against current group IDs at the declared observation boundary; exact daily metric names and accumulation order remain to be frozen.

When PopulationDynamics is enabled, its Phase 9b birth/cleanup work precedes the proposed migration slot. **Proposal:** newborns do not migrate on their birth day and become eligible from day `D+1`, matching the current rule that they act beginning the next day. Existing live agents may migrate on day `D`. Whether migration runs with m3p at all, and how migration affects parent/child group inheritance on subsequent births, remains open.

### 15.5 Profile, digest, snapshot, metrics, and events

Propose a new profile name `m3m` for M3 plus migration. A combined migration + population profile (for example `m3pm`) and propagation combinations need their own explicit mapping; they must not be inferred from a partial feature set. `SimulationProfile`, `SimulationSystems`, and `SimConfig::runner_selection` currently map only the existing m2/m3/m3p profiles and propagation combinations ([`SimulationProfile`, `SimulationSystems`, `SimConfig::runner_selection`](../../crates/sim-model/src/config.rs)).

For the new profile, propose domains `SIMCIV_M3M_STATE_V1`, `SIMCIV_M3M_METRICS_V1`, and `SIMCIV_M3M_EVENTS_V1`, and a profile-specific snapshot schema version `4`. The canonical agent record already encodes `group_id`; migration therefore changes canonical state through that existing field, while the new domain identifies the profile semantics. Snapshot v4 should store the canonical relationship CSR, current group assignments, and any persistent migration state chosen by the freeze. Existing M2/M3/M3P/propagation digest domains and snapshot bytes remain unchanged ([domain constants](../../crates/sim-model/src/hashing.rs); [`encode_m3p_snapshot`, `decode_m3p_snapshot`](../../crates/sim-model/src/m3p.rs); [`M3PROP_SNAPSHOT_SCHEMA_VERSION`](../../crates/sim-model/src/m3prop.rs)).

Proposed daily metrics append `migrations` and checked `migration_food_cost`; if a pressure metric is useful, record it only as a declared M3M metric and define its accumulation order. Proposed event payload is `SettlementMigrated { agent_id, from_group_id, to_group_id, food_cost }`, ordered by ascending AgentId in the post-Phase-9c event stream. These are profile-specific M3M records/digests; existing `M3pEvent`, `M3pDailyMetrics`, and frozen M3/M3P schemas do not change ([`M3pEvent`, `M3pDailyMetrics`](../../crates/sim-model/src/m3p.rs); [`M3pPropagationDailyMetrics`](../../crates/sim-model/src/m3pprop.rs)).

### 15.6 M4-01 forward gate and implementation inventory

The frozen M4-01 forward gate applies without exception: M4-06 adds **zero public runner functions** and makes **zero edits to the bodies** of `run_registered_world_day_loop_impl` and `run_registered_storage_day_loop`. A changed generic-loop body is a gate failure to report; do not work around it by renaming a loop or moving the same dispatch into another generic function. Existing generic loops already accept registered hook callbacks; M4-06 may add a system-owned implementation and adapt existing system-specific hook dispatch, but must count and list that code honestly ([M4-01 forward gate](#10-m4-01-system-registration-architecture); generic loops and M3P day entry points in [`runner.rs`](../../crates/sim-model/src/runner.rs)).

The implementation report must include the production runner/context inventory and wiring-line count before/after using the §10.1 method, list every changed file, and report changed lines outside the new migration system's own source files. Expected integration files—not a permission to edit any of them—include `config.rs`, `system_registry.rs`, existing system-specific dispatch in `runner.rs`/`m3p.rs`, canonical hashing and snapshot codecs, and exports/tests. Identify which files actually changed. Any new public runner entry point or generic-loop-body edit fails the gate rather than getting excluded from the count.

### 15.7 Correctness gates and tests

Before marking M4-06 complete:

- compare the M3M WorldState reference and Segmented SoA storage paths per day for State/Metrics/Events digests, migration records, intents, Phase 6B resolutions, and errors;
- test exact replay across several seeds, candidate orderings, and DenseSlot permutations;
- test the migration plan's deterministic AgentId ordering, destination tie-break, cost boundary, no-candidate/no-move behavior, and validation failure atomicity;
- test snapshot pause/resume with the migration system enabled and any selected graph/population/propagation composition;
- preserve all existing M2/M3/M3P/propagation golden digests and snapshot bytes unchanged, and add separate M3M goldens;
- test the proposed sustained-workload configuration long enough to report alive population, migration counts, and settlement occupancy; include M3P if that profile composition is selected;
- assert the M4-01 forward gate: no new runner functions, no generic-loop-body edits, and a complete touched-file/line-count record.

### 15.8 Performance budget

Use the frozen [M4 standard performance-gate protocol](#m4-standard-performance-gate-protocol-frozen): same-process paired runs, two warmups, fixed `r` chosen so each sample is at least one second, 41 timed samples, alternating operation order, `MAD/median <= 5%` validity, deterministic bootstrap, and one retry for an invalid cell. **Proposal:** measure uninstrumented whole-tick M3M against the same workload with migration disabled, at `N = 10,000` and `N = 100,000`, and set `T = 5%` of the baseline median. Report migration phase time and all cells; this draft does not freeze whether a 5% overhead is the right budget or whether a M3/M3P runner is the preferred baseline.

### 15.9 Non-goals and open questions

M4-06 does not change existing M2/M3/M3P behavior, add an automatic parallel scheduler, revise the generic day loops, or introduce unregistered migrations. Its target is the first registered settlement-movement workload; it does not define a general agent-transport framework.

| Question | Status | Options / consequence | Recommendation |
|---|---|---|---|
| OQ-15.1 Pressure and destination pool | **Blocking** | Resource-per-capita vs food pressure; pressure-only vs random fallback; all settlements vs an explicit neighbor set. These choices define movement semantics and RNG draws. | Start with resource-per-capita over all other settlements, deterministic maximum with lowest-GroupId tie-break. |
| OQ-15.2 Probability coordinates and eligibility | **Blocking** | Per-agent draws and the alive/health predicate determine draw coverage and must be frozen with subsystem 9. | One stateless decision coordinate per eligible live agent in ascending AgentId order; no retry draw. Freeze the exact coordinate fields/draw indices. |
| OQ-15.3 Cost and failure behavior | **Blocking** | Food, wealth, or treasury cost; skip vs event on an unaffordable move; checked arithmetic and atomic commit. | Food cost, insufficient food means no move and no migration event. |
| OQ-15.4 Phase slot and registry ordering | **Blocking** | Existing Phase9C before Propagation preserves generic-loop bodies but shares a slot; a distinct slot may require a forbidden loop edit. | Use Phase9C with explicit stable Migration-before-Propagation hook ordering; prove it with registry tests. If this cannot be done without generic-loop edits, report the gate failure. |
| OQ-15.5 Graph semantics | **Blocking** | Keep edges attached to AgentIds or rewire after migration. Rewiring changes CSR hashes and incremental/snapshot work. | Keep edges fixed; recompute partition-relative measurements from current groups. |
| OQ-15.6 Population interaction | **Blocking** | Enable migration only on m3, or compose with m3p; allow newborn migration on day D or defer to D+1. | Defer newborn movement to D+1; decide profile composition before freezing. |
| OQ-15.7 Profile and persistence | **Blocking** | `m3m` alone vs composed profiles; schema version and any owned migration state; digest/event/metrics domain layout. | Use separate profile-specific domains and snapshot v4; persist only state required to reproduce future moves. |
| OQ-15.8 Performance threshold | **Non-blocking for draft; blocking before freeze** | 5% proposed whole-tick overhead against a disabled-system baseline, or another declared threshold/baseline. | Use A-5 and retain the 5% proposal unless measurements show it is inappropriate. |
| OQ-15.9 Migration metric detail | **Non-blocking for draft; blocking before freeze** | Count successful moves only, or also attempted/pressure statistics; each extra value changes M3M metrics bytes. | Start with successful move count and total cost only. |

No implementation is authorized by this draft. Resolve the blocking questions, review the M4-01 forward gate, and freeze §15 before requesting M4-06 implementation.
