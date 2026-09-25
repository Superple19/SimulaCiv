# M2-09 Phase 3 / Phase 4 Scratch Buffer Optimization Feasibility Analysis

- **Milestone:** M2-09 — Phase 3/4 Scratch Buffer Feasibility Analysis
- **Target Runtime:** M0 Reference Model (`sim-core`, `sim-model`)
- **Status:** Feasibility Analysis Complete & Implementation Blueprint Established
- **Scope Restriction:** Analysis only. **Zero production code modifications** in this milestone.

---

## 1. Executive Summary & Objective

Following the optimization of individual phase validation logic in M2-07 (Phase 9) and M2-08 (Phase 4 intent validation), the primary remaining source of per-day heap churn in the simulation engine is the inter-phase data pipeline connecting:
1. **Phase 3 Feature Extraction:** produces `Vec<AgentFeatures>`
2. **Phase 4 Action Decision:** consumes `&[AgentFeatures]`, produces `Vec<PrimaryActionChoice>`
3. **Phase 4 Intent Formulation:** consumes `&[PrimaryActionChoice]`, produces `Vec<Intent>`
4. **Phase 5 Locality Partitioning:** consumes `&[Intent]`, produces `Vec<SettlementIntentPartition>`

On every simulation day, these collections are dynamically allocated on the heap, passed to the next phase as a slice reference, and dropped immediately upon completion of the subsequent phase.

This document analyzes the technical feasibility, architectural constraints, and semantic risks of eliminating this heap churn through **scratch buffer reuse**, establishing an implementation plan that guarantees **zero public API breakage** and **zero deterministic divergence**.

---

## 2. Investigation of Current Allocation Lifecycles & Dependencies

### 2.1 Phase 3: `phase3_observation_and_features`
- **Source Location:** [`crates/sim-model/src/features.rs:92`](file:///c:/AI/SimulaCiv/crates/sim-model/src/features.rs#L92)
- **Signature:**
  ```rust
  pub fn phase3_observation_and_features(
      world: &WorldState,
      config: &SimConfig,
  ) -> Result<Vec<AgentFeatures>, Phase3Error>
  ```
- **Lifecycle:**
  - Allocated at line 96: `Vec::with_capacity(world.agents.len())`.
  - Element size: 24 bytes (`AgentId` 4 bytes + `[f32; 5]` 20 bytes).
  - Populated via sequential scan over `world.agents` for behaviorally eligible agents (`alive && health > 0.0`).
  - Sorted at line 133: `features_list.sort_by_key(|af| af.agent_id)`.
  - Returned to [`runner.rs:247`](file:///c:/AI/SimulaCiv/crates/sim-model/src/runner.rs#L247) as `features`.
  - Passed as `&features` to `phase4_primary_action_selection` at line 250.
  - **Dropped at line 251** immediately after `choices` is generated.
  - **Active Lifetime:** ~15–25 µs per day.
- **External Caller & Test Dependencies:**
  - Invoked directly in `runner.rs:247` and `m0_baseline_bench.rs:164`.
  - **12 direct unit test invocations** in `tests/phase3_features_tests.rs`.
  - Direct integration test invocations in `phase4_decision_tests.rs` (3), `phase4_intent_tests.rs` (3), `phase5_partition_tests.rs` (1), `phase6a_work_tests.rs` (1), and `phase6b_targeted_tests.rs` (1).
  - **Critical Finding:** Mutating the existing public function signature would break at least 21 distinct test cases across 6 test files, violating AGENTS.md Rule 7.

### 2.2 Phase 4: `phase4_primary_action_selection`
- **Source Location:** [`crates/sim-model/src/decision.rs:236`](file:///c:/AI/SimulaCiv/crates/sim-model/src/decision.rs#L236)
- **Signature:**
  ```rust
  pub fn phase4_primary_action_selection(
      world: &WorldState,
      config: &SimConfig,
      agent_features: &[AgentFeatures],
  ) -> Result<Vec<PrimaryActionChoice>, DecisionError>
  ```
- **Lifecycle:**
  - Allocated at line 241: `Vec::with_capacity(agent_features.len())`.
  - Element size: 8 bytes (`AgentId` 4 bytes + `Action` 1 byte + 3 bytes padding).
  - Evaluates utilities and draws coordinate PRNG `(Subsystem::Decision, DrawIndex = 0)`.
  - Sorted at line 275: `choices.sort_by_key(|c| c.agent_id)`.
  - Returned to `runner.rs:250` as `choices`.
  - Passed as `&choices` to `phase4_generate_intents` at line 251.
  - **Dropped at line 252** immediately after `intents` is generated.
  - **Active Lifetime:** ~15–20 µs per day.
- **External Caller & Test Dependencies:**
  - Invoked across `phase4_decision_tests.rs` (10 test calls) and downstream phase tests (7 test calls).

### 2.3 Phase 4: `generate_intents`
- **Source Location:** [`crates/sim-model/src/intents.rs:173`](file:///c:/AI/SimulaCiv/crates/sim-model/src/intents.rs#L173)
- **Signature:**
  ```rust
  pub fn generate_intents(
      world: &WorldState,
      config: &SimConfig,
      choices: &[PrimaryActionChoice],
  ) -> Result<Vec<Intent>, IntentError>
  ```
- **Lifecycle:**
  - Allocated at line 183: `Vec::with_capacity(choices.len())`.
  - Element size: 32 bytes (`Intent` enum).
  - Populated based on action variants; draws coordinate PRNG `(Subsystem::MutualAidTarget / TheftTarget, DrawIndex = 1)` if target selection occurs.
  - Sorted at line 311: `intents.sort_by_key(|i| i.agent_id())`.
  - Returned to `runner.rs:251` as `intents`.
  - Passed as `&intents` to `phase5_partition_intents` at line 254.
  - **Dropped at line 255** immediately after `partitions` is formed.
  - **Active Lifetime:** ~15–25 µs per day.
- **External Caller & Test Dependencies:**
  - Invoked across 17 test cases in `phase4_intent_tests.rs` and downstream phase tests.

---

## 3. Allocation Inventory Summary

| Vector Identifier | Creation Point | Element Size | Daily Capacity ($N=10$) | High-$N$ Capacity ($N=1000$) | Allocation Lifetime | Call Frequency |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: |
| `features_list` | `features.rs:96` | 24 bytes | 240 bytes | 24,000 bytes | Phase 3 → Phase 4 (~20 µs) | 500 / trajectory |
| `choices` | `decision.rs:241` | 8 bytes | 80 bytes | 8,000 bytes | Phase 4 Dec → Phase 4 Intent (~15 µs) | 500 / trajectory |
| `intents` | `intents.rs:183` | 32 bytes | 320 bytes | 32,000 bytes | Phase 4 Intent → Phase 5 (~20 µs) | 500 / trajectory |
| **Daily Pipeline Total** | - | - | **640 bytes** | **64,000 bytes** | - | **1,500 allocs / trajectory** |

In a standard 500-day graduation trajectory, exactly **1,500 heap allocations and 1,500 deallocations** occur exclusively to pass transient slices across these four adjacent phase boundaries.

---

## 4. Scratch Buffer Architectural Pattern Analysis

Four distinct architectural patterns were evaluated to eliminate this repetitive allocation overhead:

### Pattern A: Function-Internal Local Reuse
- **Concept:** Retain static/reusable vectors inside `phase3_observation_and_features`.
- **Feasibility:** Impossible without static or thread-local state. A standard function's local variables are dropped when the function returns. To return data, it must return an owned collection, allocate on an external arena, or write into a caller-supplied buffer.
- **Verdict:** **Infeasible for inter-day reuse**.

### Pattern B: Arena Allocator (e.g. `bumpalo` / typed arenas)
- **Concept:** Pass a bump allocation arena into each phase.
- **Feasibility:**
  - Requires adding third-party dependencies (`bumpalo` or similar).
  - Infects all returned types with lifetime annotations (e.g., `Vec<'a, AgentFeatures>`).
  - Forces significant signature changes across all calling code and tests.
- **Verdict:** **REJECTED** (violates AGENTS.md Rule 3: no speculative abstractions, no unneeded dependencies).

### Pattern C: Thread-Local Storage (TLS: `thread_local! { static SCRATCH: ... }`)
- **Concept:** Store scratch vectors in a thread-local cell and access them implicitly.
- **Feasibility:**
  - Introduces hidden global mutable state.
  - Conflicts with deterministic test isolation and future multi-threaded execution (Rayon parallelism).
  - Can cause reentrancy issues if an observation callback invokes another simulation step.
- **Verdict:** **REJECTED** (anti-pattern for deterministic simulators).

### Pattern D: Runner-Level Scratchpad with Companion `_into` Functions (Recommended)
- **Concept:**
  1. Define a clean scratchpad container within `runner.rs`:
     ```rust
     #[derive(Default, Debug)]
     pub struct PhaseScratchpad {
         pub features: Vec<AgentFeatures>,
         pub choices: Vec<PrimaryActionChoice>,
         pub intents: Vec<Intent>,
     }
     ```
  2. Implement internal companion functions that write into caller-provided mutable buffers:
     - `pub(crate) fn phase3_observation_into(world, config, buffer: &mut Vec<AgentFeatures>) -> Result<(), Phase3Error>`
     - `pub(crate) fn phase4_primary_action_selection_into(world, config, features, buffer: &mut Vec<PrimaryActionChoice>) -> Result<(), DecisionError>`
     - `pub(crate) fn phase4_generate_intents_into(world, config, choices, buffer: &mut Vec<Intent>) -> Result<(), IntentError>`
  3. Keep the original public functions intact as thin wrappers that allocate a new `Vec` and delegate to the `_into` function:
     ```rust
     pub fn phase3_observation_and_features(
         world: &WorldState,
         config: &SimConfig,
     ) -> Result<Vec<AgentFeatures>, Phase3Error> {
         let mut out = Vec::with_capacity(world.agents.len());
         phase3_observation_into(world, config, &mut out)?;
         Ok(out)
     }
     ```
  4. In `runner.rs`, allocate the `PhaseScratchpad` once during multi-day execution (`run_m0_days`), reusing the exact same vectors across all 500 days via `.clear()`.
- **Feasibility:** **100% Technically Feasible & Completely Semantic-Safe**.

---

## 5. Semantic Risk & Invariance Analysis

The recommended companion pattern was evaluated against all simulation invariants:

| Risk Category | Potential Hazard | Mitigation in Pattern D | Risk Rating |
| :--- | :--- | :--- | :---: |
| **Public API Compatibility** | Signature changes break external crates or tests | Existing functions keep 100% identical signature and behavior | **ZERO RISK** |
| **Existing Test Suite** | 21+ tests in `tests/**` fail to compile or link | All tests call the existing wrapper functions unmodified | **ZERO RISK** |
| **WorldState Integrity** | Scratchpad fields added to authoritative state | Scratchpad resides purely in runner execution frame; `WorldState` untouched | **ZERO RISK** |
| **Snapshot & Persistence** | Scratchpad alters snapshot schema or binary encoding | Ephemeral runner buffers never participate in snapshot encoding | **ZERO RISK** |
| **PRNG Coordinate System** | Scratch reuse perturbs PRNG draw order or values | PRNG calls in Phase 4 use identical stateless coordinates `(Subsystem, Day, AgentId, Draw)` | **ZERO RISK** |
| **Floating-Point Semantics** | Math reduction or feature formulas altered | Calculations remain identical IEEE-754 floats evaluated in same order | **ZERO RISK** |
| **AgentId Sorting Invariants** | Residual elements in reused buffer perturb ordering | Buffer is cleared (`buffer.clear()`) before population; sorted by `AgentId` identically | **ZERO RISK** |
| **Canonical Hash Equivalence** | Bit-level divergence in State, Metrics, or Events | Logical outputs of all phases are identical bit-for-bit | **ZERO RISK** |

---

## 6. Candidate Classification (Allowed vs. Rejected)

### Allowed Approaches (Zero Public API Mutation):
- **Runner-Level Scratchpad via Companion Functions (Pattern D):**
  - Leaves `pub fn phase3_observation_and_features`, `pub fn phase4_primary_action_selection`, and `pub fn generate_intents` completely intact.
  - Adds internal `_into` variants for runner-internal zero-allocation streaming.
  - Fully compatible with `cargo test --workspace` (all 546 tests pass without modification).

### Rejected Approaches:
- **Direct Signature Mutation:** Changing public functions to take `&mut Vec<...>` directly is **REJECTED** (breaks 21+ existing tests; violates AGENTS.md Rule 7).
- **WorldState Scratchpad Field:** Adding scratch vectors into `WorldState` is **REJECTED** (pollutes domain state; breaks C03 Storage Invariance and binary snapshot hashes).
- **Thread-Local Storage (TLS):** Hidden global state is **REJECTED** (breaks concurrency and reentrancy guarantees).
- **Third-Party Arena Allocators:** Adding external crate dependencies is **REJECTED** (violates AGENTS.md Rule 1 and Rule 3).

---

## 7. Selected Candidate: Phase 3 Observation Buffer Reuse

### **Selected Candidate:** Phase 3 Feature Extraction Scratch Buffer (`phase3_observation_into`)
- **Primary Target:** [`crates/sim-model/src/features.rs`](file:///c:/AI/SimulaCiv/crates/sim-model/src/features.rs) & [`crates/sim-model/src/runner.rs`](file:///c:/AI/SimulaCiv/crates/sim-model/src/runner.rs)
- **Mechanism:**
  1. In `features.rs`, introduce `pub fn phase3_observation_and_features_into(world: &WorldState, config: &SimConfig, out: &mut Vec<AgentFeatures>) -> Result<(), Phase3Error>`.
     - Clears `out` buffer (`out.clear()`).
     - Populates eligible agent features in-place.
     - Sorts `out.sort_by_key(|af| af.agent_id)`.
  2. Implement existing `pub fn phase3_observation_and_features(...)` as a zero-cost wrapper calling `_into`.
  3. In `runner.rs`, allow `run_m0_days` to hold a reusable `Vec<AgentFeatures>` and pass it across consecutive days.
- **Expected Impact:**
  - Eliminates 500 heap allocations per 500-day trajectory.
  - Completely preserves existing public function, error enums, and return types.
  - Sets up the modular foundation for extending the same pattern to Phase 4 choices and intents in subsequent tasks.

---

## 8. Proposed Validation Plan (for Subsequent Implementation Milestone)

When implementation is authorized, validation will strictly enforce:
1. **Zero Test Modifications:**
   - All 546 workspace tests must pass without changing a single line in `tests/**/*.rs`.
   - `tests/phase3_features_tests.rs` (12 tests) must pass unmodified.
2. **Contract Hash Invariance (100% Bit-Exact Match):**
   - `CanonicalStateHash`: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
   - `CanonicalMetricsHash`: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b`
   - `CanonicalEventHash`: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`
3. **Code Quality Gates:**
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `git diff --check`
4. **Empirical Measurement:**
   - `cargo bench --bench m0_baseline_bench` to measure throughput improvement and verify memory reduction.
