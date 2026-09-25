# M1 Contract Freeze Specification: Contracts C01–C10

- **Milestone:** M1 — Contract Freeze
- **Baseline Commit:** `86f2f2d34389f85fdf27c0d295af30361f034367`
- **Reference Oracle:** M0 Reference Model (`sim-core`, `sim-model`)
- **Validation Status:** Complete (521/521 tests passed, 500-day pause/resume equivalence verified)
- **Authority:** Authoritative semantic specification for M2 High-Performance Runtime

---

## 1. Freeze Declaration & Purpose

This document formally declares the **M1 Contract Freeze** for SimulaCiv. 

During the M0 development phase, architectural, operational, and data-exchange contracts were iteratively designed, implemented, and empirically validated in an executable single-threaded reference model. With the completion of the M0-16B determinism graduation gate, M0 has achieved complete semantic verification across:
- All 11 daily lifecycle phases.
- Exact coordinate-addressed PRNG determinism.
- Pause/resume equivalence over a 500-day horizon (Contract C08).
- Canonical SHA-256 determinism oracles for State, Metrics, and Events (Contract C10).
- Observer independence under all telemetry configurations.
- Storage representation invariance under physical layout permutations.

By this freeze:
1. **M0 reference behavior** is locked as the trusted executable semantic oracle.
2. **Contracts C01 through C10**, as reconciled herein, are frozen as the mandatory semantic specification.
3. **M2 runtime implementations** (data-parallel Segmented SoA, Rayon multi-threading) must strictly conform to these frozen contracts and pass automated differential testing against M0.
4. Any future semantic breaking change requires an explicit **Contract Amendment** (§8).

---

## 2. M0 Empirical Validation Evidence Summary

The freeze is grounded in empirical evidence established by 521 passing acceptance and regression tests:

| Test Target / Gate | Tests | Scope & Evidence |
| :--- | :---: | :--- |
| `sim-core` unit tests | 0 | In-crate unit test runner |
| `golden_prng_tests` | 7 | Exact Mix13 math, coordinate collision resistance, uniformity, and external golden vectors |
| `sim-model` unit tests | 1 | In-crate unit test runner (subsystem ID mappings) |
| `config_tests` | 15 | Comprehensive configuration parsing and strict domain invariant validation |
| `initialization_tests` | 8 | Deterministic agent trait sampling, resource allocation, and initial world state generation |
| `phase1_2_tests` | 12 | Phase 1 resource regrowth and Phase 2 metabolic health/food degradation |
| `phase3_features_tests` | 10 | Normalized feature extraction, bounds safety, and division-by-zero protection |
| `phase4_decision_tests` | 12 | Primary action selection logits, Softmax sampling, and trait-biased choice |
| `phase4_intent_tests` | 13 | Intent formulation, candidate neighbor selection, and zero-target preclassification |
| `phase5_partition_tests` | 12 | Locality partitioning by settlement `GroupId` and pure functional isolation |
| `phase6a_work_tests` | 11 | Phase 6A proportional resource rationing and atomic harvest commitment |
| `phase6b_targeted_tests` | 27 | Phase 6B `ResolutionKey` sequential ordering, TOCTOU live validation, and food transfer |
| `phase7_market_tests` | 36 | Bilateral pool clearing, supply reconciliation, integer tax withholding, and currency conservation |
| `phase8_welfare_tests` | 37 | Institutional treasury redistribution, starvation threshold checks, and remainder allocation |
| `phase9_mortality_tests` | 30 | Authoritative deceased status commitment and tombstone non-compaction |
| `phase10_metrics_tests` | 41 | Daily macroscopic metrics observation, rank-weighted Gini calculation, and observer independence |
| `phase11_snapshot_tests` | 50 | Canonical binary snapshot encoding/decoding, float bit preservation, and restore cursor |
| `phase11_event_tests` | 47 | C07 event records, canonical key ordering, atomic flush, and duplicate rejection |
| `determinism_oracle_tests`| 66 | Independent Python fixed vectors, SHA-256 preimages, and oracle purity |
| `m0_runner_tests` | 50 | Full-day runner integration (Phases 1–11), telemetry toggles, and physical layout invariance |
| `m0_determinism_gate_tests`| 36 | Final graduation gate: 500-day pause/resume equivalence, replay, and layout invariance |
| **Total** | **521** | **All tests passing; zero warnings across formatting and lints** |

---

## 3. C01–C10 Frozen Contract Matrix

Each clause is classified under exactly one freeze disposition:
- **`FROZEN`**: Fully implemented in production code and empirically validated by M0 tests. Binding on M2.
- **`DEFERRED`**: Architecturally planned for future milestones, but not implemented in M0 v0.1. Not binding on M2 baseline.
- **`REMOVED_FROM_M1`**: Draft prose contradicted by validated M0 semantics or obsolete aspirational language.

| Contract | Draft Requirement | Validated M0 Semantic Implementation | Validation Evidence | Disposition |
| :--- | :--- | :--- | :--- | :--- |
| **C01: Agent Identity** | Unique `AgentId`, never reused even after death | Strongly typed `AgentId(u32)` allocated sequentially at init; tombstones retained on death; never reused or deleted | `test_20_no_agent_id_reuse_or_deletion`, `types.rs` | **`FROZEN`** |
| | External references use `AgentId`, never `DenseSlot` | All events, intents, commands, and resolutions use `AgentId` exclusively | `events.rs`, `resolution.rs` | **`FROZEN`** |
| | `DenseSlot` non-canonicality | `DenseSlot(u32)` excluded from canonical hashes and snapshot payloads; rebuilt on restore; permutation does not affect simulation trajectory | `test_43_dense_slot_permutation_same_final_state_hash`, `m0_determinism_gate_tests` (test 26) | **`FROZEN`** |
| | Dynamic birth allocation & monotonic allocation of new entities | Not implemented in M0 v0.1 (population is fixed at initial count with mortality decrements; deceased agents retained as tombstones) | M0 v0.1 scope boundary | **`DEFERRED`** |
| | Runtime array compaction via `swap_remove` | Not permitted in M0; deceased agents remain in place as dead tombstones to preserve indexing stability | `test_21_no_swap_remove_compaction_in_m0` | **`FROZEN`** |
| **C02: Time & Phase Ordering** | Non-overlapping, sequential phases | Strict sequential execution of Phases 1 through 11 via `run_m0_day` | `runner.rs`, `m0_runner_tests` (tests 1–9) | **`FROZEN`** |
| | Day cursor progression | `world.current_day` advances $D \to D + 1$ strictly upon successful Day Complete; failed days do not advance | `runner.rs`, `m0_runner_tests` (tests 12–15) | **`FROZEN`** |
| | Read-only vs mutation phase separation | Phases 3, 4, 10 are strictly read-only; mutations occur only in designated resolution phases (1, 2, 6A, 6B, 7, 8, 9) | `features.rs`, `decision.rs`, `metrics.rs` | **`FROZEN`** |
| | Multithreaded phase barrier primitives & runtime capability flags | Single-threaded runner composition; no runtime barrier flags exist in M0 production code; parallel synchronization and capability flags are an M2 obligation | `runner.rs` | **`DEFERRED`** (M2 obligation) |
| **C03: State Storage** | Authoritative logical state definition | World state explicitly defined by `WorldState`, `AgentState`, and `SettlementState` | `state.rs`, `m0_determinism_gate_tests` | **`FROZEN`** |
| | Stable ID indexing | Entities indexed and resolved by stable logical IDs (`AgentId`, `GroupId`) | `state.rs`, `hashing.rs` | **`FROZEN`** |
| | Storage layout invariance | Trajectory and determinism hashes invariant to agent array order, settlement array order, and slot assignments | `m0_determinism_gate_tests` (tests 24–28) | **`FROZEN`** |
| | Physical container binding (`Vec<Agent>`) | Physical storage is an implementation detail; M2 is authorized to use Segmented Structure-of-Arrays (SoA) | M0/M2 boundary (§7) | **`REMOVED_FROM_M1`** (non-binding) |
| **C04: RNG Determinism** | Stateless coordinate-addressed PRNG | `SplitMix64-CoordinateMixer` (Stafford Mix13) evaluating $(\text{MasterSeed}, \text{ReplicateId}, \text{Day}, \text{Phase}, \text{SubsystemId}, \text{AgentId}, \text{DrawIndex})$ | `prng.rs`, `golden_prng_tests` | **`FROZEN`** |
| | Exact coordinate packing & bit-mixing | Word 0 and Word 1 64-bit packing; golden constants `K_PRIME`, `K_MUL1`, `K_MUL2` | `prng.rs:43-69`, `golden_prng_tests` | **`FROZEN`** |
| | Exact `f32` conversion | Upper 24 bits divided by $16777216.0$ producing uniform float in $[0.0, 1.0)$ | `prng.rs:72-76`, `golden_prng_tests` | **`FROZEN`** |
| | Subsystem IDs & tie-breaking | Subsystem IDs: 0 = Initialization, 1 = Decision, 2 = TheftTarget, 3 = MutualAidTarget, 4 = TheftSuccess, 5 = ResolutionPriority; ResolutionKey priority mixer uses Subsystem 5; TheftSuccess uses Subsystem 4; Phase 4 target selection uses Subsystem 2 and 3; ResolutionKey lexical tie-break | `subsystems.rs`, `resolution.rs` | **`FROZEN`** |
| | Draft claim of empirical 1 vs 16 thread test in M0 | Replaced by verified mathematical coordinate determinism in M0 + M2 parallel differential requirement | §6, `m0_determinism_gate_tests` | **`REMOVED_FROM_M1`** |
| **C05: Intent Contract** | Zero mutation authority | Emitting an `Intent` confers zero state modification rights | `decision.rs`, `intents.rs` | **`FROZEN`** |
| | Immutable decision capture | Canonical 6 primary actions; neighbor targeting evaluated in Phase 4; target `None` preclassified as zero-op | `intents.rs`, `phase4_intent_tests` | **`FROZEN`** |
| | Resolver TOCTOU validation without recomputation | Resolvers inspect live state (food reserves, death) and clamp/cancel without altering behavioral intent | `resolution.rs`, `phase6b_targeted_tests` | **`FROZEN`** |
| | Speculative action schemas (reproduction, migration) | Only the canonical 6 actions (Work, BuyFood, SellFood, GiveFood, StealFood, Idle) are frozen for baseline | `decision.rs` | **`DEFERRED`** |
| **C06: Command Resolution** | Locality partitioning | Intents partitioned deterministically by settlement `GroupId` | `partitioning.rs`, `phase5_partition_tests` | **`FROZEN`** |
| | Phase 6A work rationing | Proportional rationing against settlement resource; atomic harvest commitment | `resolution.rs:183-360`, `phase6a_work_tests` | **`FROZEN`** |
| | Phase 6B targeted interaction | Sequential processing in ascending `ResolutionKey` order with lexical tie-break; immediate live-state commit | `resolution.rs:650-860`, `phase6b_targeted_tests` | **`FROZEN`** |
| | Phase 7 market clearance | Bilateral settlement clearing; seller supply reconciliation against live food; buyer affordability integer clamping; tax withholding | `resolution.rs:1140-1610`, `phase7_market_tests` | **`FROZEN`** |
| | Phase 8 welfare distribution | Institutional treasury redistribution to agents below `starvation_threshold`; remainder allocated to lowest `AgentId`s | `resolution.rs:1740-2040`, `phase8_welfare_tests` | **`FROZEN`** |
| | Currency conservation | Financial transactions strictly satisfy $\Delta \text{Wealth} + \Delta \text{Treasury} = 0$; signed `proceeds_balance` reconciliation | `phase7_market_tests`, `phase8_welfare_tests` | **`FROZEN`** |
| **C07: Event Contract** | Dual event categories | `StateTransitionEvent` (WorkResolved, FoodTransferred, MarketCleared, WelfareDistributed, MortalityCommitted) and `ObservationEvent` (DailyMetricsObserved, SnapshotEmitted) | `events.rs`, `phase11_event_tests` | **`FROZEN`** |
| | Canonical EventKey ordering | $\text{EventKey} = (\text{day}, \text{phase}, \text{partition\_key}, \text{local\_sequence})$; lexical ascending sort | `events.rs:26-47`, `hashing.rs` | **`FROZEN`** |
| | Atomic flush & validation | `phase11_flush_events` validates bounds, rejects duplicate keys, empties buffer on success, leaves buffer intact on error | `events.rs:215-322`, `phase11_event_tests` | **`FROZEN`** |
| | Observer independence | Recording and flushing events causes zero state perturbation | `m0_determinism_gate_tests` (test 21) | **`FROZEN`** |
| | Speculative event variants (environmental shocks, regeneration) | Resource regeneration and environmental shock events are excluded from the baseline M0 event vocabulary (frozen baseline contains exactly the 7 variants across StateTransition and Observation categories) | `events.rs` | **`DEFERRED`** |
| **C08: Snapshot / Restore** | Pause/resume equivalence | Trajectory A (continuous 500 days) $\equiv$ Trajectory B (day 0..199 $\to$ snapshot $\to$ restore at 200 $\to$ day 200..499) | `m0_determinism_gate_tests` (tests 8–10) | **`FROZEN`** |
| | Binary snapshot encoding | Magic `"SIMCIVM0"`, schema version 1, header metadata, sorted entity tables, exact `f32` bit preservation | `snapshot.rs:10-250`, `phase11_snapshot_tests` | **`FROZEN`** |
| | Resume cursor semantics | Snapshot produced at day $D$ carries resume cursor $D + 1$ in metadata; restore sets `world.current_day = D + 1` | `runner.rs`, `m0_runner_tests` (tests 16–21) | **`FROZEN`** |
| | Snapshot event day distinction | `SnapshotEmitted` event key has `key.day = D` (occurrence day) and payload `day = D + 1` (resume cursor) | `runner.rs`, `m0_runner_tests` (tests 18–19) | **`FROZEN`** |
| | Multi-rate scheduled future event queues in snapshot | Excluded in M0 baseline; coordinate-based PRNG is stateless | `snapshot.rs` | **`DEFERRED`** |
| **C09: Metrics / Observation** | Macroscopic statistics | `DailyMetrics { day, population, wealth_gini, total_food_reserves, total_treasury }` | `metrics.rs`, `phase10_metrics_tests` | **`FROZEN`** |
| | Exact statistical formulas | Population count, food/treasury sums, and rank-weighted integer-sum Gini $G = \frac{2W - (n+1)S}{nS}$ bounded in $[0.0, 1.0]$ | `metrics.rs:72-243`, `phase10_metrics_tests` | **`FROZEN`** |
| | Observer independence | Running with metrics enabled vs disabled produces identical authoritative simulation state | `m0_determinism_gate_tests` (test 20) | **`FROZEN`** |
| | High-order analytical metrics (sentiment, polarization) | Excluded from runtime observation contract | `metrics.rs` | **`DEFERRED`** |
| **C10: Persistence & Determinism** | Three determinism oracles | `CanonicalStateHash`, `CanonicalMetricsHash`, `CanonicalEventHash` using SHA-256 | `hashing.rs`, `determinism_oracle_tests` | **`FROZEN`** |
| | Canonical binary preimages | Domain separation prefixes (`"SIMCIV_STATE_V1"`, `"SIMCIV_METRICS_V1"`, `"SIMCIV_EVENTS_V1"`), strictly sorted entities/records | `hashing.rs:136-580`, `determinism_oracle_tests` | **`FROZEN`** |
| | DenseSlot & metadata exclusion | `DenseSlot` and snapshot metadata excluded from `CanonicalStateHash` | `hashing.rs`, `m0_determinism_gate_tests` | **`FROZEN`** |
| | Parquet determinism role | Parquet is an analytical transport format only; never a determinism oracle | §6, `hashing.rs` | **`FROZEN`** |

---

## 4. Authoritative M1 Daily Lifecycle Specification

The simulation day is executed sequentially in 11 immutable phases followed by Day Complete:

```
+-------------------------------------------------------------------------------------------------------------+
|                                    M1 DAILY SIMULATION LIFECYCLE                                            |
+-------+-----------------------------+-----------+----------------------+--------------------+---------------+
| Phase | Name                        | Authority | Primary Input        | Primary Output     | Determinism   |
+-------+-----------------------------+-----------+----------------------+--------------------+---------------+
| 1     | Environment Regrowth        | Mutate    | World State, Config  | Settlement Resource| Closed Formula|
| 2     | Biological Degradation      | Mutate    | World State, Config  | Agent Food, Health | Closed Formula|
| 3     | Observation & Features      | Read-Only | World State, Config  | Feature Vectors    | Pure Function |
| 4     | Decision & Intent Generation| Read-Only | Feature Vectors, Rng | Intent Collection  | Coord (1, 2, 3)|
| 5     | Locality Partitioning       | Pure Read | Intent Collection    | Locality Buckets   | Pure Grouping |
| 6A    | Work Resolution             | Mutate    | Work Intents, Res    | Harvest Allocations| Proportional  |
| 6B    | Targeted Resolution         | Mutate    | Targeted Intents, Rng| Food Transfers     | ResKey(5)/Draw(4)|
| 7     | Settlement Market Clearance | Mutate    | Market Intents, State| Trade Clearances   | Bilateral Pool|
| 8     | Institutional Welfare       | Mutate    | Settlement Treasury  | Welfare Payouts    | Canonical Dist|
| 9     | Mortality Commitment        | Mutate    | Agent Health / Food  | Tombstone Status   | Live Check    |
| 10    | Macroscopic Observation     | Read-Only | World State          | DailyMetrics       | Rank-Weight Gini|
| 11    | Snapshot & Event Flush      | Read/Flush| Buffer, World State  | Snapshot, Events   | Canonical Sort|
+-------+-----------------------------+-----------+----------------------+--------------------+---------------+
| Day Complete: If all phases succeed, advance world.current_day from D to D + 1.                             |
+-------------------------------------------------------------------------------------------------------------+
```

### Phase Rules:
1. **Phase 1 (Regrowth)**: Closed-form logistic resource regeneration $R_{t+1} = \min(K, R_t + r \cdot R_t \cdot (1 - R_t / K))$.
2. **Phase 2 (Degradation)**: Subtracts metabolic cost from food; if food is depleted, applies starvation health decay.
3. **Phase 3 (Observation)**: Evaluates normalized feature vectors $[x_0, x_1, x_2, x_3, x_4] \in [0.0, 1.0]^5$ without side-effects.
4. **Phase 4 (Decision)**: Computes action utilities using Softmax (Subsystem 1: `Decision`); evaluates candidate neighbor sampling (Subsystem 2: `TheftTarget`, Subsystem 3: `MutualAidTarget`); formulates immutable `Intent`s.
5. **Phase 5 (Partitioning)**: Groups intents into settlement buckets by `GroupId`. Pure functional operation.
6. **Phase 6A (Work)**: Proportional biomass rationing against local settlement resource; commits harvest food additions.
7. **Phase 6B (Targeted)**: Sorts intents in ascending `ResolutionKey` order (Subsystem 5: `ResolutionPriority`); evaluates theft success via coordinate draw (Subsystem 4: `TheftSuccess`); commits immediate food transfers.
8. **Phase 7 (Market)**: Reconciles seller supply against live food; calculates integer affordable units; executes pool clearance; withholds tax to treasury; maintains currency conservation.
9. **Phase 8 (Welfare)**: Identifies agents with `food < starvation_threshold`; distributes treasury funds; breaks remainder ties by lowest `AgentId`.
10. **Phase 9 (Mortality)**: Sets `alive = false` for agents with `health <= 0.0`; never removes, compacts, or reallocates agent slots.
11. **Phase 10 (Metrics)**: If enabled, computes `DailyMetrics` (population, reserves, treasury, rank-weighted integer-sum Gini). Zero mutation.
12. **Phase 11 (Snapshot & Flush)**: If epoch boundary, encodes canonical binary snapshot (magic `"SIMCIVM0"`, schema version 1) with resume cursor $D + 1$. Flushes pending `EventBuffer` canonically by `(day, phase, partition_key, local_sequence)` ascending.
13. **Day Complete**: `world.current_day` increments $D \to D + 1$ only after Phase 11 successfully finishes.

---

## 5. Frozen Canonical Artifacts & Oracle Requirements

M2 implementation acceptance requires bitwise identity against the following frozen canonical artifacts:

### 5.1 Golden PRNG Vectors
Verified by `crates/sim-core/tests/golden_prng_tests.rs`:
- Mathematical property: `Mix64(0) = 0` (Stafford Mix13 maps zero input to zero)
- Raw Stafford Mix13 golden outputs:
  - `Mix64(0x0123456789abcdef) = 0x9629f58e8ec5b906`
  - `Mix64(0xdeadbeefcafebabe) = 0x7ad6664f09ffe52c`
- Exact Coordinate PRNG fixtures (`coordinate_prng_u64` and `coordinate_prng_f32`):
  - Zero coordinate `(0, 0, 0, 0, 0, 0, 0)`:
    - `u64 = 0x1957a7604e215178`
    - `f32 = 0.09899372` (`0.09899371862411499`)
  - Master seed 1 `(1, 0, 0, 0, 0, 0, 0)`:
    - `u64 = 0x2aa9cfa61473238e`
  - Pattern seed `(0x0123456789abcdef, 0, 0, 0, 0, 0, 0)`:
    - `u64 = 0x7cd5081854be7f81`
  - Mid-simulation coordinate `(0x0123456789abcdef, 7, 123, 4, 1, 42, 0)`:
    - `u64 = 0x99a401447e7d75dc`
    - `f32 = 0.60015875` (`0.6001587510108948`)
  - Theft target coordinate `(0xdeadbeefcafebabe, 3, 999, 4, 2, 123456, 1)`:
    - `u64 = 0x76397dab79ddd38b`
  - Boundary max coordinate `(0xffffffffffffffff, 0xffffffff, 0xffffffff, 11, 5, 0xffffffff, 0xffffffff)`:
    - `u64 = 0xfa2454cf03d537e6`
    - `f32 = 0.9771168` (`0.9771168231964111`)

### 5.2 Determinism Oracle Fixed Vectors
Verified by independent Python-backed vectors in `crates/sim-model/tests/determinism_oracle_tests.rs`:
- **Independent State Vector** (`test_51_independent_fixed_vector_state`):
  - Preimage: `"SIMCIV_STATE_V1"`, day 1, 1 agent (`AgentId(42)`, `alive=true`, `birth_day=0`, `health=1.0`, `food=10.0`, `wealth=100`, `productivity=1.0`, `cooperation=0.5`, `aggression=0.1`, `risk_tolerance=0.2`, `group_id=7`), 1 settlement (`GroupId(7)`, `resource=500.0`, `treasury=1000`)
  - Canonical bytes length: 84 bytes
  - `CanonicalStateHash`:
    ```text
    df23dfb04459b91e3d34432897bcc4cdbaa3973b832ae93311dc4b7918da77c4
    ```
- **Independent Metrics Vector** (`test_52_independent_fixed_vector_metrics`):
  - Preimage: `"SIMCIV_METRICS_V1"`, 1 record (`day=1`, `population=100`, `wealth_gini=0.25`, `total_food_reserves=1500.0`, `total_treasury=5000`)
  - Canonical bytes length: 57 bytes
  - `CanonicalMetricsHash`:
    ```text
    92589a21bf9cbec1dbdc910e4040d1d9f8ce46c8c669d4ec29de8f08728b6581
    ```
- **Independent Event Vector** (`test_53_independent_fixed_vector_events`):
  - Preimage: `"SIMCIV_EVENTS_V1"`, 2 records (Event 1: `WelfareDistributed` on day 1 phase 8, Event 2: `MortalityCommitted` on day 1 phase 9)
  - Canonical bytes length: 84 bytes
  - `CanonicalEventHash`:
    ```text
    8b8882d12434930e8f2e7dddf36e2cf8e7c36f4911e8a1d3d87a1b9444d07b17
    ```

### 5.3 M0-16B 500-Day Graduation Trajectory Hashes
Generated from the canonical fixture (`MasterSeed = 81985529216486895`, `ReplicateId = 7`, initial population = 10, settlement count = 2) under `crates/sim-model/tests/m0_determinism_gate_tests.rs`:
- **Final `CanonicalStateHash` (Day 500)**:
  ```text
  b450702d1a3c7fa68b753b2be03cbe3112d7c570535352c8b8744dd2589363a0
  ```
- **Final `CanonicalMetricsHash` (Days 0..499)**:
  ```text
  ee82103f6ebecdb57d77b55f190eec26ae026a35043bf7c327ffbbd688cf0696
  ```
- **Final `CanonicalEventHash` (Cumulative Event Batch)**:
  ```text
  d14cb358215ea78d46152a55928d3ef71168f6381014e3e3bdf8fa87c8d93e83
  ```

---

## 6. M2 Freedom Boundary & Semantic Equivalence

M2 is authorized and expected to implement radical computational and architectural optimizations. The boundary between allowed optimizations and forbidden semantic modifications is strictly defined:

### What M2 is FREE to change:
1. **Memory Layout**: Transition from `Vec<AgentState>` to Segmented Structure-of-Arrays (SoA), columnar slices, or cache-line-aligned storage blocks.
2. **Parallel Scheduling**: Multi-threaded execution using Rayon or custom thread pools across settlements or independent partitions.
3. **Execution Mechanisms**: Replacing single-threaded loops with SIMD vectorization, batch kernels, and work-stealing queues (strictly conditioned on preserving bitwise canonical logical equivalence).
4. **Temporary Allocations**: Pre-allocated ring buffers, thread-local staging arenas, and zero-allocation scratch spaces.
5. **Entity Compaction**: Internal non-canonical dense indices, storage slot arrangements, and memory addresses (provided stable `AgentId` mapping is preserved).

### What M2 is FORBIDDEN to change:
1. **Mathematical Semantics & Reductions**: Regrowth formulas, metabolic costs, trait ranges, Softmax logits, coordinate PRNG bit outputs, pricing math, and wealth Gini rank-weighted integer-sum arithmetic.
   - **Phase 7 Market Reduction Invariance**: In Phase 7 market clearing, canonical `f32` reductions and operand order are strictly frozen. No reassociation, no fused multiply-add (FMA) that alters precision, no altered reduction tree, and no `f64` reinterpretation of food arithmetic are permitted. Any SIMD implementation or parallelization must produce bitwise identical `f32` totals and buyer/seller allocations to M0's sequential reduction.
2. **Phase Execution Sequence**: Running phases out of order or allowing cross-phase pipeline overlapping that perturbs read/write boundaries.
3. **Behavioral Choices**: Overriding agent target selection or intent parameters during resolution.
4. **Canonical Encodings**: Binary formats for snapshots, event keys, or determinism hash preimages.
5. **Determinism**: Producing differing `CanonicalStateHash`, `CanonicalMetricsHash`, or `CanonicalEventHash` values on identical initial conditions.

---

## 7. Deferred Items

The following architectural concepts remain intended for future project stages but are explicitly **excluded from the M1 frozen baseline**:
1. **Dynamic Birth Allocation**: Runtime allocation of new `AgentId`s and generational demographic replacement.
2. **Scheduled Future Event Queues**: Multi-rate temporal schedulers and future-dated event queues.
3. **Spatial Migration**: Agent physical relocation between settlements.
4. **Social & Family Graph Topologies**: Explicit kinship, marriage, and friendship graphs.
5. **Parquet Persistence Engine**: Analytical column-oriented serialization (Parquet remains transport-only; determinism is governed by C10 SHA-256 hashes).
6. **Advanced Macroscopic Analytics**: Sentiment, polarization, cultural drift, and institutional complexity metrics.

---

## 8. Contract Amendment Procedure

After this M1 freeze, any change that modifies:
- The meaning or representation of authoritative state fields,
- Phase sequencing or day cursor timing,
- PRNG coordinate hashing or drawing mechanics,
- Intent or command resolution rules,
- Currency conservation or market clearing equations,
- Event record schemas, key structures, or sorting orders,
- Snapshot binary layouts or resume cursor conventions,
- The mathematical definition of macroscopic metrics, or
- Canonical preimage binary serialization and SHA-256 fixed vectors

**constitutes a Semantic Contract Amendment**.

### Mandatory Amendment Workflow:
1. **Identification**: Formally document the architectural rationale and exact scope of the proposed amendment.
2. **Specification Update**: Amend the relevant contract sections in `docs/contracts/` and update `SIMULACIV_DESIGN_SPECIFICATION.md`.
3. **M0 Oracle Update**: Implement and validate the modified semantics in the M0 reference model first.
4. **Vector Regeneration**: Update canonical golden hashes and acceptance tests to reflect the new accepted behavior.
5. **M2 Alignment**: Implement the amended contract in M2 and verify differential equivalence against updated M0.

No semantic amendment may be introduced as an "optimization" during M2 development.
