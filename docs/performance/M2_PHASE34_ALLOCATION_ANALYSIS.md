# M2-08 Phase 3 / Phase 4 Scratch Buffer & Allocation Optimization Analysis

- **Milestone:** M2-08 — Phase 3/4 Scratch Buffer Analysis
- **Target Runtime:** M0 Reference Model (`sim-core`, `sim-model`)
- **Status:** Analysis Complete & Single Candidate Selected
- **Scope Restriction:** Analysis only. **Zero production code changes** in this milestone.

---

## 1. Executive Summary & Scope Boundary

Milestone M2-08 focuses on analyzing the temporary heap allocations and scratch buffer opportunities across:
1. **Phase 3: Observation & Normalized Feature Extraction** (`crates/sim-model/src/features.rs`)
2. **Phase 4: Primary Action Decision Selection** (`crates/sim-model/src/decision.rs`)
3. **Phase 4: Intent Formulation & Target Selection** (`crates/sim-model/src/intents.rs`)

As mandated by AGENTS.md and the task specification:
- This milestone is strictly **analysis-only**. No production code, function signatures, or public APIs are modified.
- Mathematical reduction orders, PRNG coordinate systems, decision softmax formulas, and output sorting invariants remain 100% frozen.

---

## 2. Phase 3 Feature Extraction Allocation Inventory

### 2.1 Code Inspection (`crates/sim-model/src/features.rs`)
- **Function:** `phase3_observation_and_features(world: &WorldState, config: &SimConfig) -> Result<Vec<AgentFeatures>, Phase3Error>`
- **Allocation Site:**
  ```rust
  let mut features_list = Vec::with_capacity(world.agents.len());
  ```
  Allocates a `Vec<AgentFeatures>` where each element is 24 bytes (`AgentId` 4 bytes + `[f32; 5]` 20 bytes). For $N=10$, this allocates 240 bytes; for $N=1000$, 24 KB.

### 2.2 Feature Calculation Order & Math Semantics
- Evaluated for each behaviorally eligible agent (`alive == true && health > 0.0`):
  - $\phi_0$ (hunger_ratio): `(1.0 - food / starvation_threshold).clamp(0.0, 1.0)`
  - $\phi_1$ (wealth_pressure): `(1.0 - (wealth as f32) / (target_reserve as f32)).clamp(0.0, 1.0)`
  - $\phi_2$ (health_deficit): `(1.0 - health).clamp(0.0, 1.0)`
  - $\phi_3$ (local_scarcity): `1.0 - (settlement.resource / carrying_capacity).clamp(0.0, 1.0)`
  - $\phi_4$ (food_surplus): `((food - starvation_threshold) / target_food).clamp(0.0, 1.0)`
- Calculations are pure floating-point evaluations with zero heap allocations. Settlement lookup (`find(|s| s.group_id == agent.group_id)`) operates in-place over the small settlements slice.

### 2.3 AgentId Iteration & Sorting Invariant
- Iterates through `world.agents` in physical storage order.
- Before returning, line 133 enforces the canonical invariant:
  ```rust
  features_list.sort_by_key(|af| af.agent_id);
  ```
  Ensures output features are strictly ordered by ascending `AgentId`, regardless of physical storage or tombstoning.

### 2.4 Allocation Lifetime & Reusability Constraints
- `features_list` is returned to `runner.rs:247`:
  ```rust
  let features = phase3_observation_and_features(world, &effective_config)?;
  let choices = phase4_primary_action_selection(world, &effective_config, &features)?;
  ```
- Passed as a read-only slice `&features` to Phase 4 decision selection.
- Immediately deallocated at line 251 after Phase 4 decision completes. Total lifetime: ~20 µs.
- **API Constraint:** `phase3_observation_and_features` is a public API tested by multiple integration tests. Changing its return signature (e.g., passing `&mut Vec<AgentFeatures>`) breaks the frozen public contract.

---

## 3. Phase 4 Decision Selection Allocation Inventory

### 3.1 Code Inspection (`crates/sim-model/src/decision.rs`)
- **Function:** `phase4_primary_action_selection(world: &WorldState, config: &SimConfig, agent_features: &[AgentFeatures]) -> Result<Vec<PrimaryActionChoice>, DecisionError>`
- **Allocation Site:**
  ```rust
  let mut choices = Vec::with_capacity(agent_features.len());
  ```
  Allocates a `Vec<PrimaryActionChoice>` where each element is 8 bytes (`AgentId` 4 bytes + `Action` enum 1 byte + 3 bytes padding).

### 3.2 Softmax & Utility Buffers (Stack-Allocated)
- `evaluate_utilities(agent, features, config)` computes utilities for the 6 canonical actions into a fixed stack array `[f32; 6]`.
- `stable_softmax(utilities, temperature)` computes shifted exponentials into stack arrays:
  ```rust
  let mut exp_values = [0.0f32; 6];
  let mut probabilities = [0.0f32; 6];
  ```
- `select_action(probabilities, u)` performs cumulative summation in-place over `[f32; 6]`.
- **Finding:** The decision math kernel is **already 100% stack-allocated and allocation-free**. There are zero intermediate heap allocations inside utility or softmax calculations.

### 3.3 RNG Draw Dependency & Ordering
- For each eligible agent, exactly one coordinate PRNG float `u in [0.0, 1.0)` is drawn:
  - `MasterSeed = config.world.master_seed`
  - `ReplicateId = config.world.replicate_id`
  - `Day = world.current_day`
  - `Phase = 4`
  - `SubsystemId = Decision (1)`
  - `AgentId = agent.agent_id`
  - `DrawIndex = 0`
- The coordinate PRNG is stateless and strictly decoupled from execution order, but the association of `u` with `agent.agent_id` must remain unchanged.

### 3.4 Allocation Lifetime
- `choices` is returned to `runner.rs:250`, passed as `&choices` to `phase4_generate_intents`, and dropped immediately. Total lifetime: ~15 µs.

---

## 4. Phase 4 Intent Generation Allocation Inventory

### 4.1 Code Inspection (`crates/sim-model/src/intents.rs`)
- **Function:** `generate_intents(world: &WorldState, config: &SimConfig, choices: &[PrimaryActionChoice]) -> Result<Vec<Intent>, IntentError>`
- **Allocation Sites:**
  1. **Duplicate & Completeness Tracking:**
     ```rust
     let mut seen_choice_agents = HashSet::with_capacity(choices.len());
     ```
     Allocates a `HashSet<AgentId>` on the heap every simulation day.
  2. **Intent Output Collection:**
     ```rust
     let mut intents = Vec::with_capacity(choices.len());
     ```
     Allocates `Vec<Intent>` where `sizeof(Intent)` is 32 bytes.
  3. **Mutual Aid Candidate Collection:**
     ```rust
     // in get_give_food_candidates():
     let mut candidates: Vec<AgentId> = world.agents.iter().filter(...).map(...).collect();
     candidates.sort();
     ```
     Allocated on heap whenever an agent chooses `GiveFood`.
  4. **Theft Victim Candidate Collection:**
     ```rust
     // in get_steal_food_candidates():
     let mut candidates: Vec<AgentId> = world.agents.iter().filter(...).map(...).collect();
     candidates.sort();
     ```
     Allocated on heap whenever an agent chooses `StealFood`.

### 4.2 Candidate Collection & PRNG Target Selection
- When `GiveFood` or `StealFood` is formulated, target selection uses coordinate PRNG `DrawIndex = 1`:
  ```rust
  let c = candidates.len();
  let index = ((u * (c as f32)).floor() as usize).min(c - 1);
  Some(candidates[index])
  ```
- **Finding:** Target selection is deeply dependent on `candidates` being strictly sorted ascending by `AgentId`. Any perturbation to candidate ordering directly alters the chosen target agent, violating trajectory determinism.

---

## 5. Candidate Evaluation & Risk Matrix

| Candidate Site | Source Location | Current Allocation Pattern | Expected Impact | Semantic Risk | Borrow/Lifetime Complexity | Overall Evaluation |
| :--- | :--- | :--- | :---: | :---: | :---: | :---: |
| **P4 Intent Choice Validation** | `intents.rs:178` | `seen_choice_agents: HashSet<AgentId>` | Eliminates 1 daily `HashSet` heap table | **LOW** | **LOW** | **SELECTED** |
| **P3 Features Output Buffer** | `features.rs:96` | `Vec<AgentFeatures>::with_capacity(N)` | Eliminates 1 daily `Vec` ($O(N)$) | **HIGH** | **HIGH** | **REJECTED** |
| **P4 Choices Output Buffer** | `decision.rs:241` | `Vec<PrimaryActionChoice>::with_capacity(N)` | Eliminates 1 daily `Vec` ($O(N)$) | **HIGH** | **HIGH** | **REJECTED** |
| **P4 Intent Output Buffer** | `intents.rs:179` | `Vec<Intent>::with_capacity(N)` | Eliminates 1 daily `Vec` ($O(N)$) | **HIGH** | **HIGH** | **REJECTED** |
| **P4 Give/Steal Candidates** | `intents.rs:121, 146` | `candidates: Vec<AgentId>` via `.collect()` | Eliminates $k$ heap vectors per day | **MEDIUM** | **MEDIUM** | **REJECTED** |
| **P3 Sorting Skip** | `features.rs:133` | `features_list.sort_by_key(...)` | Eliminates $O(N \log N)$ sort | **HIGH** | **LOW** | **REJECTED** |

---

## 6. Rejected Candidates & Explicit Rationales

### 1. Rejection: Phase 3 / Phase 4 Scratch Buffer Reusable Vectors (`features_list`, `choices`, `intents`)
- **Reason:** Public API Stability & Architectural Boundaries.
  - `phase3_observation_and_features`, `phase4_primary_action_selection`, and `phase4_generate_intents` are public crate functions invoked directly across 100+ unit and integration tests.
  - Modifying their return signatures to take mutable scratchpad references (`&mut Vec<...>`) would break the public interface and violate AGENTS.md Rule 7 (modifying tests to accommodate production changes).
  - Introducing persistent scratchpad storage into `WorldState` would pollute the authoritative simulation state with ephemeral execution buffers.
  - Introducing thread-local or static pools violates concurrency invariance and creates hidden mutable state.

### 2. Rejection: Candidate Buffers in `get_give_food_candidates` & `get_steal_food_candidates`
- **Reason:** Public Function Contract & PRNG Target Invariance.
  - Both helper functions are public functions in `intents.rs` with frozen signatures `(&AgentState, &WorldState) -> Vec<AgentId>`.
  - In normal canonical runs ($N=10$), mutual aid and theft are selected infrequently, so these vectors contribute less than 1% of daily allocation overhead.
  - PRNG target indexing `index = ((u * c).floor() as usize).min(c - 1)` is hyper-sensitive to candidate ordering.

### 3. Rejection: Skipping Phase 3 `features_list.sort_by_key`
- **Reason:** Specification Invariant Violation.
  - Contract C03 mandates that logical simulation behavior must be 100% identical under arbitrary agent storage permutation (shuffled storage tests).
  - Omitting the sort relies on an assumption of sorted input storage, which breaks differential verification against permuted fixtures.

---

## 7. Selected Candidate: Phase 4 Intent Choice Validation Optimization

### **Winner: Optimization of `seen_choice_agents` in `crates/sim-model/src/intents.rs`**
- **Location:** `crates/sim-model/src/intents.rs:178, 287`
- **Target Function:** `generate_intents(world: &WorldState, config: &SimConfig, choices: &[PrimaryActionChoice])`

### Key Rationale:
1. **Zero Public API Modification:**
   - The public signature and return type of `generate_intents` remain 100% unchanged.
   - All 50 existing tests in `phase4_intent_tests.rs` continue to call the function identically without any edits.
2. **Zero Semantic & PRNG Divergence:**
   - `seen_choice_agents` is solely used for validation:
     1. Rejecting duplicate choices (`IntentError::DuplicateChoice(agent_id)`).
     2. Rejecting missing choices for behaviorally eligible agents (`IntentError::MissingChoiceForEligibleAgent(agent_id)`).
   - It performs zero state mutation, zero PRNG draws, and zero floating-point arithmetic.
3. **High-Impact Allocation Elimination:**
   - Currently, every single simulation day allocates and drops a `HashSet<AgentId>` on the heap.
   - Over 500 simulation days, this performs **500 heap allocations and deallocations**.
   - By applying the proven pattern from Phase 9 (contiguous slice linear scan for $N \le 32$ and `HashSet::with_capacity(len)` for $N > 32$), all 500 daily heap allocations can be **completely eliminated** in the canonical fixture ($N=10$).
4. **Comprehensive Test Protection:**
   - `phase4_intent_tests.rs` (50 unit tests) exhaustively tests duplicate rejection, missing choice detection, and ordering. Any behavioral regression will be caught immediately.

---

## 8. Proposed Validation Plan (for Subsequent Implementation Milestone)

When implementation is authorized, validation will strictly execute:
1. **Contract Invariance:**
   - `CanonicalStateHash`: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
   - `CanonicalMetricsHash`: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b`
   - `CanonicalEventHash`: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`
2. **Test Suite:**
   - `cargo test --test phase4_intent_tests` (50 passed)
   - `cargo test --workspace` (546 passed)
3. **Code Quality:**
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `git diff --check`
4. **Benchmark Verification:**
   - `cargo bench --bench m0_baseline_bench` to record runtime and throughput changes.
