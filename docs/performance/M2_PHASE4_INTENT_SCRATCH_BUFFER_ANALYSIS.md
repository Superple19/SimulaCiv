# M2-12 Phase 4 Intent Generation Scratch Buffer Optimization Feasibility Analysis

- **Milestone:** M2-12 — Phase 4 Intent Generation Scratch Buffer Feasibility Analysis
- **Target Subsystem:** Phase 4 Intent Generation (`crates/sim-model/src/intents.rs`, `crates/sim-model/src/runner.rs`)
- **Status:** Feasibility Analysis Complete & Pre-Implementation Blueprint Established
- **Scope Restriction:** Analysis & Architecture Review Only. **Zero production code modifications** in this milestone.

---

## 1. Executive Summary & Objective

Following the successful implementation of scratch buffer reuse for Phase 3 Feature Extraction (M2-10) and Phase 4 Primary Action Choice Selection (M2-11), the final remaining unoptimized temporary allocation in the Phase 3–4 simulation pipeline is:
- **Phase 4 Intent Formulation:** `generate_intents(...) -> Result<Vec<Intent>, IntentError>`

Currently, `generate_intents` allocates a dynamic `Vec<Intent>` on every simulation day, passes it as a slice reference (`&[Intent]`) to Phase 5 Locality Partitioning, and drops the vector at the conclusion of the daily cycle.

This document performs an exhaustive safety, lifecycle, and architectural analysis for introducing a reusable runner-level scratch buffer to eliminate this allocation churn, ensuring:
1. **Zero Public API Breakage:** 100% backward compatibility for all existing unit, integration, and benchmark callers.
2. **Zero Semantic Divergence:** Bit-exact determinism, identical PRNG draw sequencing, and identical `AgentId` sorting.
3. **Canonical Hash Preservation:** Bit-exact equivalence against M1 frozen graduation fixtures (`CanonicalStateHash`, `CanonicalMetricsHash`, `CanonicalEventHash`).

---

## 2. Current Allocation Flow Analysis

### 2.1 Creation Point & Return Type
- **Source Location:** [`crates/sim-model/src/intents.rs:173-185`](file:///c:/AI/SimulaCiv/crates/sim-model/src/intents.rs#L173-L185)
- **Signature:**
  ```rust
  pub fn generate_intents(
      world: &WorldState,
      config: &SimConfig,
      choices: &[PrimaryActionChoice],
  ) -> Result<Vec<Intent>, IntentError>
  ```
- **Allocation Statement:**
  ```rust
  let n = choices.len();
  // ... seen_choice_agents check ...
  let mut intents = Vec::with_capacity(n);
  ```
- **Element Size & Layout:**
  - `Intent` is a 6-variant enum (`Work`, `BuyFood`, `SellFood`, `GiveFood`, `StealFood`, `Idle`).
  - Size: 32 bytes per element (8-byte `AgentId`, 8-byte `GroupId`, 16-byte payload including `Option<AgentId>` and `f32` quantity/amounts).
  - Allocation Capacity: $N$ elements, where $N$ is the number of primary action choices ($N \le \text{world.agents.len()}$).

### 2.2 Daily Lifecycle & Drop Timing
1. **Allocation & Formulation:**
   - In [`runner.rs:280`](file:///c:/AI/SimulaCiv/crates/sim-model/src/runner.rs#L280), `run_m0_day_with_scratch` calls:
     ```rust
     let intents = phase4_generate_intents(world, &effective_config, choices_scratch)?;
     ```
2. **Downstream Consumption (Phase 5):**
   - In [`runner.rs:283`](file:///c:/AI/SimulaCiv/crates/sim-model/src/runner.rs#L283), `intents` is passed by immutable borrowed reference:
     ```rust
     let partitions = phase5_partition_intents(&intents)?;
     ```
   - [`phase5_partition_intents`](file:///c:/AI/SimulaCiv/crates/sim-model/src/partitioning.rs#L46) accepts `&[Intent]`. It validates uniqueness, clones/collects intents into partitioned groups by `GroupId`, and does **not** take ownership of `intents`.
3. **Subsequent Phases (Phases 6A–11):**
   - Downstream resolvers consume `&partitions` (`SettlementIntentPartition`).
   - The original `intents` vector is never referenced again after line 283.
4. **Deallocation:**
   - `intents` remains bound on the stack until the end of `run_m0_day_with_scratch` (line 415), at which point its backing heap buffer is dropped and freed.
   - **Active Logical Lifetime:** ~10–25 µs per simulation day.

### 2.3 500-Day Allocation Churn Estimation
- In a 500-day simulation run (`run_m0_days`), `Vec<Intent>` is allocated and dropped **500 times**.
- Heap allocation volume by population:
  - **$N = 10$:** 500 allocations $\times$ 320 B = 160 KB total heap churn.
  - **$N = 100$:** 500 allocations $\times$ 3.2 KB = 1.6 MB total heap churn.
  - **$N = 500$:** 500 allocations $\times$ 16 KB = 8.0 MB total heap churn.
  - **$N = 1000$:** 500 allocations $\times$ 32 KB = 16.0 MB total heap churn.
- **Secondary Allocations:** Inside `generate_intents`, `get_give_food_candidates` and `get_steal_food_candidates` perform conditional target candidate vector allocations. However, `Vec<Intent>` is **unconditional** (incurred 100% of days regardless of chosen action distribution).

---

## 3. Scratch Buffer Feasibility Analysis

### 3.1 Applicability of the `_into` Pattern
Following the identical architectural pattern established in M2-10 (`phase3_observation_and_features_into`) and M2-11 (`phase4_primary_action_selection_into`), `generate_intents` naturally decomposes into:
```rust
pub fn generate_intents_into(
    world: &WorldState,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    out.clear();
    let needed = choices.len();
    if out.capacity() < needed {
        out.reserve(needed - out.capacity());
    }

    // ... validation, formulation loop, candidate draws, out.push(intent) ...

    out.sort_by_key(|i| i.agent_id());
    Ok(())
}
```
And convenience re-export alias:
```rust
pub use generate_intents_into as phase4_generate_intents_into;
```

### 3.2 Public API Preservation & Test Callers
- **Existing Signature:**
  ```rust
  pub fn generate_intents(
      world: &WorldState,
      config: &SimConfig,
      choices: &[PrimaryActionChoice],
  ) -> Result<Vec<Intent>, IntentError> {
      let mut intents = Vec::with_capacity(choices.len());
      generate_intents_into(world, config, choices, &mut intents)?;
      Ok(intents)
  }
  ```
- **External Caller Invariants:**
  - `phase4_intent_tests.rs` (17 unit test cases), `phase5_partition_tests.rs` (1), `phase6a_work_tests.rs` (1), and `phase6b_targeted_tests.rs` (1) invoke `generate_intents` directly.
  - Maintaining `generate_intents` as a delegation wrapper preserves 100% source and binary compatibility across all 20+ existing test cases without touching a single test file (strictly satisfying AGENTS.md Rule 7).

### 3.3 Runner-Level Scratch Storage Integration
- In `crates/sim-model/src/runner.rs`:
  - `run_m0_day_with_scratch` signature expands to receive `intents_scratch: &mut Vec<Intent>`:
    ```rust
    pub fn run_m0_day_with_scratch(
        world: &mut WorldState,
        config: &SimConfig,
        context: &M0RunContext,
        options: &DayExecutionOptions,
        features_scratch: &mut Vec<AgentFeatures>,
        choices_scratch: &mut Vec<PrimaryActionChoice>,
        intents_scratch: &mut Vec<Intent>,
    ) -> Result<DayOutcome, M0RunError>
    ```
  - `run_m0_day` allocates all 3 scratch buffers locally and delegates to `run_m0_day_with_scratch`.
  - `run_m0_days` allocates `intents_scratch = Vec::with_capacity(world.agents.len())` **once** at runner loop initialization and reuses it across all simulation days.
- **Combined Impact:** The entire Phase 3 $\to$ Phase 4 $\to$ Phase 5 data ingestion pipeline becomes **zero-allocation** across the 500-day simulation loop.

### 3.4 Evaluation of Storage Mechanisms

| Mechanism | Feasibility | Correctness / Purity | Safety & Threading | Recommendation |
| :--- | :---: | :---: | :---: | :---: |
| **Runner-Owned Stack Reference (`&mut Vec<Intent>`)** | **High** | **100% Pure** (No side-effects, deterministic) | Completely thread-safe, no reentrancy traps | **SELECTED (Candidate A)** |
| **`WorldState` Struct Field** | Medium | **VIOLATES PURITY** (Contaminates state hash/snapshot) | Safe but architecturally invalid | **STRICTLY REJECTED** |
| **Thread-Local Storage (`thread_local!`)** | Medium | Low (Hidden mutable state, breaks multi-instance) | Reentrancy hazards, prevents determinism | **STRICTLY REJECTED** |
| **Arena Allocator (`bumpalo` / `typed-arena`)** | Low | High | Requires new `Cargo.toml` dependency (Forbidden) | **STRICTLY REJECTED** |

---

## 4. Semantic Risk Analysis

### 4.1 Intent Ordering & AgentId Sorting
- **Invariant:** Phase 4 intents must be returned in strictly ascending `AgentId` order (`out.sort_by_key(|i| i.agent_id())`).
- **Analysis:**
  - `out.clear()` resets vector length to 0 while keeping allocated capacity.
  - The loop appends newly formulated `Intent` records.
  - `out.sort_by_key(|i| i.agent_id())` is executed prior to returning `Ok(())`.
  - The resulting slice order is bit-identical to newly allocated vectors.
- **Risk Level:** **ZERO / NONE**.

### 4.2 PRNG Draw Sequence Impact
- **Invariants:**
  - Coordinate PRNG draws in Phase 4 are executed exclusively for `GiveFood` and `StealFood` target selection.
  - Coordinate coordinates: `(MasterSeed, ReplicateId, current_day, Phase = 4, SubsystemId, agent_id, DrawIndex = 1)`.
- **Analysis:**
  - Iteration over `choices` follows the exact slice ordering `for (i, choice) in choices.iter().enumerate()`.
  - Replacing the destination vector with an external scratch buffer does not alter iteration order, candidate evaluation, or PRNG draw arguments.
- **Risk Level:** **ZERO / NONE**.

### 4.3 Phase 5 Locality Partitioning Contract
- **Invariants:**
  - Phase 5 `phase5_partition_intents` requires non-duplicate initiator `AgentId`s and bit-identical intent attributes.
- **Analysis:**
  - `phase5_partition_intents(&intents_scratch)` receives `&[Intent]`.
  - Slice elements, order, and values are identical to those generated by a newly allocated vector.
- **Risk Level:** **ZERO / NONE**.

### 4.4 Canonical Hash Impact
- **Invariants:**
  - `CanonicalStateHash`, `CanonicalMetricsHash`, `CanonicalEventHash` must maintain 100% bit-exact match against M1 frozen graduation fixtures.
- **Analysis:**
  - Phase 4 intents are ephemeral decision-time records and are not hashed directly.
  - However, downstream Phase 6–9 state transitions derive strictly from `partitions`, which derive from `intents`.
  - Because `generate_intents_into` produces identical contents and ordering, all downstream state transitions, events, metrics, and canonical hashes remain bit-exact.
- **Risk Level:** **ZERO / NONE**.

---

## 5. Candidate Selection & Blueprint

### 5.1 Selected Candidate: Runner-Level Reusable Scratch Buffer (`_into` Pattern)
- **Candidate ID:** `CANDIDATE-M2-12A`
- **Target Files:**
  - `crates/sim-model/src/intents.rs`
  - `crates/sim-model/src/runner.rs`
- **Design:**
  1. Add `pub fn generate_intents_into(world: &WorldState, config: &SimConfig, choices: &[PrimaryActionChoice], out: &mut Vec<Intent>) -> Result<(), IntentError>`.
  2. Re-export `pub use generate_intents_into as phase4_generate_intents_into;`.
  3. Keep `pub fn generate_intents(...)` as a 3-line delegating wrapper.
  4. Extend `run_m0_day_with_scratch` to accept `intents_scratch: &mut Vec<Intent>`.
  5. In `run_m0_days`, allocate `intents_scratch` once outside the daily execution loop.
- **Risk Assessment:** **LOW RISK**. Zero API breakage, zero semantic drift, zero external dependencies.

---

## 6. Rejected Candidates

### 6.1 Rejected Candidate 1: Breaking Signature Modification of `generate_intents`
- **Rationale:** Replacing `generate_intents(...) -> Result<Vec<Intent>, ...>` with `generate_intents(..., out: &mut Vec<Intent>)` would break 20+ unit and integration tests across the workspace, violating AGENTS.md Rule 7 (test integrity).

### 6.2 Rejected Candidate 2: Global or Thread-Local Scratch Buffer (`thread_local!`)
- **Rationale:** Introducing hidden mutable global state degrades deterministic reproducibility, prevents concurrent multi-instance simulation runners within the same thread, and violates M0/M2 architectural purity principles.

### 6.3 Rejected Candidate 3: Embedding Scratch Buffers inside `WorldState`
- **Rationale:** `WorldState` is the authoritative physical state model of the simulation, subject to canonical snapshot encoding, hashing, and state restoration. Polluting it with runtime scratch memory violates contract C08 (state separation) and mutates snapshot schema hashes.

### 6.4 Rejected Candidate 4: Unsafe Lifetime Transmutation / Unchecked Vector Reuse
- **Rationale:** Raw pointer casting or unsafe memory manipulation to bypass borrow checker constraints introduces undefined behavior risk and is completely unnecessary given clean Rust lifetime semantics.

### 6.5 Rejected Candidate 5: Third-Party Arena Allocator
- **Rationale:** Introducing `bumpalo` or similar libraries requires modifying `Cargo.toml`, violating the constraint against introducing unapproved dependencies in M2.

---

## 7. Implementation Plan for Next Step (M2-13)

When authorized, implementation will proceed in the following order:
1. **`crates/sim-model/src/intents.rs`:**
   - Extract core implementation of `generate_intents` into `generate_intents_into`.
   - Update `generate_intents` to delegate to `generate_intents_into`.
   - Add alias `pub use generate_intents_into as phase4_generate_intents_into;`.
2. **`crates/sim-model/src/runner.rs`:**
   - Import `generate_intents_into` (or `phase4_generate_intents_into`).
   - Add `intents_scratch: &mut Vec<Intent>` parameter to `run_m0_day_with_scratch`.
   - Update `run_m0_day` and `run_m0_days` to manage and pass `intents_scratch`.
3. **Verification Suite:**
   - `cargo test --workspace` (verify 546/546 pass).
   - `cargo bench --bench m0_baseline_bench` (verify 100% bit-exact hash match on State, Metrics, Event hashes).
   - `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`.
   - `git diff --check`.
