# M2 Optimization & Differential Validation Contract

- **Target Milestone:** M2 — Optimized Runtime
- **Semantic Authority:** [`docs/contracts/M1_CONTRACT_FREEZE.md`](file:///c:/AI/SimulaCiv/docs/contracts/M1_CONTRACT_FREEZE.md)
- **Machine-Readable Manifest:** [`contracts/m1_contract.toml`](file:///c:/AI/SimulaCiv/contracts/m1_contract.toml)
- **Reference Oracle:** M0 Executable Reference Model (`sim-core`, `sim-model`)
- **Status:** Binding Operational Contract for M2 Development

---

## 1. Executive Purpose & Governance

This document establishes the binding architectural contract and validation protocol governing all performance optimization work under **Milestone M2 (High-Performance Runtime)**.

Under the SimulaCiv phased architecture:
- **M0** created and validated the executable reference semantics.
- **M1** froze all behavioral, mathematical, format, and determinism contracts.
- **M2** introduces high-throughput data structures, data parallelism, and vectorization.

### Core Architectural Axiom:
> **Performance optimizations must never alter simulation semantics.**
> Any optimized M2 runtime execution must produce logical trajectories and canonical SHA-256 determinism digests bitwise identical to the M0 reference model under identical initial conditions.

---

## 2. Permitted Optimization Freedoms (변경 가능 영역)

M2 implementations are authorized to implement radical engineering optimizations within internal storage and execution mechanics:

### 2.1 Internal Memory Architecture
- **Segmented Structure-of-Arrays (SoA)**: Decomposing monolithic `Vec<AgentState>` into separate columnar arrays for agent properties (e.g., contiguous `f32` vectors for health, food, traits).
- **Columnar Slices & Chunking**: Grouping agent data into cache-line-friendly chunks or settlement-local arenas to maximize CPU L1/L2 cache hit rates.
- **Cache-Line Alignment**: Padding or aligning records and thread structures to 64-byte boundaries to eliminate false sharing.

### 2.2 DenseSlot & Internal Storage Mechanics
- **Dense Slot Compaction**: M2 may freely reuse, compact, or reorganize internal array indices (`DenseSlot`) when agents die, provided the logical `AgentId` remains stable, unique, and strictly mapped.
- **Entity Indexing Tables**: Sparse-to-dense translation tables, lookup handles, or generational slot allocators.

### 2.3 Cache Structure & Arena Allocation
- **Zero-Allocation Execution Loops**: Replacing per-phase heap allocations with pre-allocated thread-local scratchpads, bump allocators, or ring buffers.
- **NUMA-Aware Locality**: Aligning memory allocation with the NUMA node or CPU core processing the corresponding settlement partition.

### 2.4 Layout Representation Transforms
- **Bidirectional Adapters**: On-the-fly zero-copy or bulk conversion between columnar SoA representations used during compute kernels and canonical structures required for determinism hashing or snapshot persistence.

### 2.5 Parallel Scheduling & Concurrency
- **Multi-Threaded Work-Stealing**: Parallel execution of independent settlement partitions using Rayon or custom thread pools.
- **Phase 3 & 4 Read-Only Concurrency**: Embarrassingly parallel evaluation of agent observation feature extraction and action utility logits.
- **Partitioned Settlement Parallelism**: Concurrently executing settlement-local phases (Phases 6A, 6B, 7, 8) across distinct worker threads.

### 2.6 SIMD Vectorization
- **Vectorized Math Kernels**: Utilizing AVX2, AVX-512, or ARM NEON SIMD intrinsics for bulk metabolic decay, trait bounds checking, Softmax exponential sums, and resource consumption.

---

## 3. Strictly Forbidden Semantic Modifications (변경 금지 영역)

No optimization in M2 may alter, bypass, or relax any of the following frozen invariants:

### 3.1 AgentId Semantics
- `AgentId` is an immutable, globally unique logical entity identifier.
- `AgentId`s must be allocated sequentially at initialization and never reused, reallocated, or renumbered across the entire simulation lifetime.
- Tombstones must be semantically retained; agent death changes `alive = false` but never deletes the logical identity.

### 3.2 Canonical State Encoding
- The binary encoding format of logical world states must strictly follow Contract C10.
- State preimages must begin with the domain prefix `"SIMCIV_STATE_V1"`.
- Agents and settlements must be encoded in strictly ascending `AgentId` and `GroupId` order.
- Non-canonical internal storage metadata (`DenseSlot`, memory layout, capacity) must remain excluded from canonical state bytes.

### 3.3 Event Ordering & Key Structure
- The total order of emitted events is governed strictly by the lexicographical order of `EventKey`:
  $$\text{EventKey} = (\text{day}, \text{phase}, \text{partition\_key}, \text{local\_sequence})$$
- Parallel execution must not perturb the global or settlement-local deterministic sequence of events.

### 3.4 RNG Coordinate Semantics
- Deterministic pseudo-randomness is addressed statelessly by the frozen 7-coordinate tuple:
  $$(\text{MasterSeed}, \text{ReplicateId}, \text{Day}, \text{Phase}, \text{SubsystemId}, \text{AgentId}, \text{DrawIndex})$$
- The PRNG algorithm must remain the exact Stafford Mix13 `SplitMix64-CoordinateMixer`.
- Subsystem IDs ($0 \dots 5$), draw indices, and coordinate packing order are strictly immutable.

### 3.5 Daily Phase Execution Sequence
- The sequential progression of Phases 1 through 11 cannot be reordered, fused across mutation boundaries, or executed speculatively without commit isolation.
- `world.current_day` advances $D \to D + 1$ strictly after Phase 11 successfully finishes.

### 3.6 Floating-Point Reduction Order & Precision
- Floating-point calculations must preserve IEEE-754 precision boundaries without loose associativity.
- **Phase 7 Market Reduction Invariance**: In Phase 7 market clearing, sequential `f32` addition and reduction orders are frozen. Fast-math reassociation, compiler vectorization that alters reduction trees, fused multiply-add (FMA) precision alteration, and `f64` conversions of food quantities are strictly prohibited.

### 3.7 Snapshot Logical Format & Resume Semantics
- Binary snapshot payloads must retain the canonical magic constant `"SIMCIVM0"` and schema version 1.
- Snapshots produced at day $D$ must carry resume cursor $D + 1$ and resume execution at day $D + 1$ without state distortion.

### 3.8 Macroscopic Metrics & Arithmetic
- `DailyMetrics` field definitions, population counting, reserve sums, and the integer-sum rank-weighted Gini coefficient formula:
  $$G = \frac{2W - (n+1)S}{nS}$$
  are frozen and must remain exact.

---

## 4. Differential Validation Contract (차등 검증 계약)

Any M2 implementation or component must pass continuous, automated differential testing against the M0 reference model before graduation.

### 4.1 Required Hash Equivalence Criteria
For any identical simulation configuration (`SimConfig`, `MasterSeed`, `ReplicateId`, execution options):
1. **State Hash Identity**:
   $$\text{CanonicalStateHash}(\text{M2}) \equiv \text{CanonicalStateHash}(\text{M0})$$
2. **Metrics Hash Identity**:
   $$\text{CanonicalMetricsHash}(\text{M2}) \equiv \text{CanonicalMetricsHash}(\text{M0})$$
3. **Event Hash Identity**:
   $$\text{CanonicalEventHash}(\text{M2}) \equiv \text{CanonicalEventHash}(\text{M0})$$

### 4.2 Mandatory Validation Scopes

Every optimized M2 module must be verified across five mandatory testing scopes:

```
+---------------------------------------------------------------------------------------------------+
|                                 M2 DIFFERENTIAL VALIDATION SUITE                                  |
+-------+-----------------------------+------------------------------------+------------------------+
| Scope | Target Scope                | Test Description                   | Acceptance Standard    |
+-------+-----------------------------+------------------------------------+------------------------+
| 1     | Single Day Execution        | Isolated daily phase execution     | Exact state/hash match |
| 2     | Multi-Day Trajectory        | Long-horizon continuous run (500d) | Bitwise digest match   |
| 3     | Snapshot Pause/Resume       | Continuous vs Restore continuation | Exact hash equivalence |
| 4     | Observer Independence       | Metrics/Events enabled vs disabled | Authoritative state id |
| 5     | Storage Layout Invariance   | Shuffled slots, SoA vs AoS storage | Canonical hash equality|
+-------+-----------------------------+------------------------------------+------------------------+
```

1. **Single Day Execution**:
   - Each phase evaluated individually and collectively for day $D = 0 \to 1$.
   - Verifies that per-phase state transitions, allocations, and event outputs match M0 bit-by-bit.

2. **Multi-Day Trajectory (500-Day Graduation Horizon)**:
   - Trajectory execution over extended horizons up to 500 days using the frozen M0-16B fixture:
     - `MasterSeed = 81985529216486895`
     - `ReplicateId = 7`
     - `initial_population = 10`
     - `settlement_count = 2`
   - Must produce the exact frozen oracle digests:
     - `CanonicalStateHash`: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
     - `CanonicalMetricsHash`: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b`
     - `CanonicalEventHash`: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`

3. **Snapshot Pause/Resume Continuation**:
   - Running $0 \dots 199 \to \text{Snapshot} \to \text{Restore at } 200 \to 200 \dots 499$.
   - Must match the uninterrupted $0 \dots 499$ continuous run bit-for-bit across State, Metrics, and Events.

4. **Observer Independence**:
   - Running M2 with telemetry toggled across all 4 combinations:
     - `(metrics: true, events: true)`
     - `(metrics: true, events: false)`
     - `(metrics: false, events: true)`
     - `(metrics: false, events: false)`
   - Authoritative state trajectory and final `CanonicalStateHash` must be bitwise identical across all combinations.

5. **Storage Layout & Permutation Invariance**:
   - Permuting internal thread count (e.g. 1 thread vs 4 threads vs 16 threads).
   - Reordering internal dense slot arrays, memory addresses, or settlement evaluation order within an independent phase.
   - Determinism oracles must produce identical canonical SHA-256 hashes regardless of physical concurrency or storage permutation.

---

## 5. M2 Acceptance & Graduation Gate

An M2 milestone or optimization pull request cannot be merged unless:
1. All existing baseline tests (546 tests) pass without regression.
2. The differential validation test suite passes 100% against the M0 reference oracle.
3. No production file under `sim-core` or M0 reference paths has been weakened or altered to accommodate M2.
4. Profiling demonstrates measurable throughput improvement over M0 baseline while preserving bit-exact determinism.
