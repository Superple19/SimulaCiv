# M2-07 Scratch Buffer & Temporary Allocation Optimization Analysis

- **Milestone:** M2-07 — Scratch Buffer / Temporary Allocation Analysis
- **Target Runtime:** M0 Reference Model (`sim-core`, `sim-model`)
- **Status:** Analysis Complete & Single Candidate Selected
- **Scope Restriction:** Analysis and candidate selection only. **Zero production code changes** in this milestone.

---

## 1. Executive Summary & Scope

Following the successful completion of the M2-06 Performance Rebaseline, this document conducts an exhaustive audit of all remaining temporary collection and heap allocation hotspots across `crates/sim-model/src`.

The objective is to identify heap allocations that occur repeatedly across simulation day cycles and evaluate their suitability for elimination or scratch-buffer reuse, strictly adhering to zero semantic divergence.

### Already Optimized Phases (Excluded from Re-Modification):
As established in milestones M2-02 through M2-05, the following areas are already optimized and are **strictly excluded** from further modification in this phase:
- **Phase 5 (Locality Partitioning):** Contiguous chunk streaming without `BTreeMap` (`cb60297`).
- **Phase 8 (Welfare Distribution):** Single-pass move, clone-free recipient updates, sorted slice duplicate check (`a65217e`).
- **Phase 10 (Macroscopic Metrics):** In-place wealth sorting over `&mut [&AgentState]` slice references, clone-free living agent counts (`bc9aab1`).
- **Phase 11 (Event Staging & Flush):** Pre-allocated `EventBuffer::with_capacity`, flat vector group tracking (`692ccb4`).

---

## 2. Allocation Hotspot Inventory across `crates/sim-model/src`

The table below catalogs every recurring allocation site identified across the active daily simulation loop:

| Subsystem / Phase | Source Location | Current Allocation Pattern | Daily Allocations ($N=10$) | Scaling Behavior | Risk Rating |
| :--- | :--- | :--- | :---: | :---: | :---: |
| **Phase 1: Environment Regrowth** | `phases.rs:12` | In-place loop over `&mut world.settlements` | 0 | $O(1)$ (Zero allocation) | N/A |
| **Phase 2: Biological Degradation** | `phases.rs:35` | In-place loop over `&mut world.agents` | 0 | $O(1)$ (Zero allocation) | N/A |
| **Phase 3: Observation & Features** | `features.rs:95` | `Vec<AgentFeatures>::with_capacity(N)` | 1 Vec | $O(N)$ heap buffer | **MEDIUM** |
| **Phase 4: Action Selection** | `decision.rs:240` | `Vec<PrimaryActionChoice>::with_capacity(N)` | 1 Vec | $O(N)$ heap buffer | **MEDIUM** |
| **Phase 4: Intent Generation (Choices)** | `intents.rs:178` | `HashSet<AgentId>::with_capacity(N)` | 1 HashSet | $O(N)$ hash table | **LOW** |
| **Phase 4: Mutual Aid Candidates** | `intents.rs:121` | `Vec<AgentId>` via `.collect()` & `.sort()` | $k_{\text{give}}$ Vecs | $O(N)$ per GiveFood intent | **MEDIUM** |
| **Phase 4: Theft Victim Candidates** | `intents.rs:146` | `Vec<AgentId>` via `.collect()` & `.sort()` | $k_{\text{steal}}$ Vecs | $O(N)$ per StealFood intent | **MEDIUM** |
| **Phase 4: Intent Output Buffer** | `intents.rs:179` | `Vec<Intent>::with_capacity(N)` | 1 Vec | $O(N)$ heap buffer | **LOW** |
| **Phase 6A: Work Duplicate Worker Check** | `resolution.rs:159` | `HashSet<AgentId>::new()` | 1 HashSet | $O(N)$ hash table | **MEDIUM** |
| **Phase 6A: Work Intermediate Buffers** | `resolution.rs:177, 235` | `work_intents: Vec`, `planned_workers: Vec` | $2 \times S$ Vecs | $O(N)$ across settlements | **MEDIUM** |
| **Phase 6B: Targeted Partition Validation** | `resolution.rs:533, 592` | `seen_groups: HashSet`, `sorted_partitions: Vec` | 1 HashSet, 1 Vec | $O(S)$ heap collections | **HIGH** |
| **Phase 6B: Targeted Stream Assembly** | `resolution.rs:600-603` | `zero_target_records`, `candidate_stream`, etc. | $4 \times S$ Vecs | $O(N)$ intermediate streams | **HIGH** |
| **Phase 7: Market Structural Validation** | `resolution.rs:1189-1200`| `sorted_partitions`, `seen_groups`, `seen_participants` | 2 HashSets, 1 Vec | $O(N + S)$ collections | **HIGH** |
| **Phase 7: Settlement Clearance Buffers**| `resolution.rs:1217-1240`| `raw_buyers`, `raw_sellers`, updates Vecs | $6 \times S$ Vecs | $O(N)$ intermediate plans | **HIGH** |
| **Phase 9: Duplicate Agent Validation** | `phases.rs:126` | `HashSet<AgentId>::new()` | 1 HashSet | $O(N)$ hash table | **LOW** |
| **Phase 9: Newly Deceased Buffer** | `phases.rs:129` | `Vec<AgentId>::new()` (unreserved capacity) | 1 Vec | $O(D)$ where $D \le N$ | **LOW** |

---

## 3. Candidate Evaluation & Risk Assessment

### Candidate A: Phase 9 Mortality Commitment Validation (`phases.rs`)
- **Location:** `crates/sim-model/src/phases.rs:123-143` (`phase9_mortality_commitment`)
- **Current Allocation Pattern:**
  - Instantiates `let mut seen_agents = HashSet::new();` every single day.
  - Inserts all agents in `world.agents` to validate unique `AgentId`s.
  - Instantiates `let mut newly_deceased = Vec::new();` with 0 capacity, triggering reallocation upon mortality events.
- **Expected Allocation Reduction:**
  - Completely eliminates 1 dynamic `HashSet` heap allocation per day.
  - Pre-allocates or reuses `newly_deceased` capacity to avoid reallocation.
- **Semantic Risk:** **LOW**.
  - Duplicate detection is a pure precondition check. In authoritative simulation trajectories, `world.agents` is maintained with unique IDs.
  - Checking uniqueness without heap hash table allocation (e.g., via sorted index/slice check or contiguous buffer) preserves identical error behavior (`Phase9Error::DuplicateAgent`).
- **Borrow / Lifetime Difficulty:** **LOW**.
  - Function is self-contained in `phases.rs`, taking `&mut WorldState` and returning `Result<Phase9MortalityResolution, Phase9Error>`.
- **Existing Test Coverage:**
  - `tests/phase9_mortality_tests.rs` contains 30 rigorous tests covering duplicate detection (`test_30_duplicate_agent_fails_fast`), non-finite health, idempotency, and ordering.
- **Applicability:** **Immediate & Highly Safe**.

---

### Candidate B: Phase 4 Intent Generation Duplicate Choice Check (`intents.rs`)
- **Location:** `crates/sim-model/src/intents.rs:178-185` (`generate_intents`)
- **Current Allocation Pattern:**
  - `let mut seen_choice_agents = HashSet::with_capacity(choices.len());` allocated on every day.
- **Expected Allocation Reduction:** Eliminates 1 `HashSet` allocation per day.
- **Semantic Risk:** **LOW**.
  - `choices` is already sorted by `AgentId` in `phase4_primary_action_selection`.
- **Borrow / Lifetime Difficulty:** **LOW**.
- **Reason for Non-Selection as Primary:**
  - `generate_intents` also contains secondary allocation sites (`get_give_food_candidates` and `get_steal_food_candidates`) which have higher semantic coupling with coordinate PRNG draw order. Splitting Phase 4 optimization yields partial benefit compared to fully resolving Phase 9.

---

### Candidate C: Phase 6A Work Resolution Partition & Allocation Buffers (`resolution.rs`)
- **Location:** `crates/sim-model/src/resolution.rs:154-245` (`phase6a_work_resolution`)
- **Current Allocation Pattern:**
  - `seen_workers = HashSet::new()` allocated daily.
  - Per partition: `work_intents: Vec<(AgentId, f32)>` allocated on heap, sorted, and converted into `planned_workers: Vec`.
- **Expected Allocation Reduction:** Eliminates 1 `HashSet` and $2 \times S$ vector allocations per day.
- **Semantic Risk:** **MEDIUM**.
  - Aggregation order must remain strictly ascending `AgentId`.
  - Nested partition loops increase mutable borrow complexity.
- **Borrow / Lifetime Difficulty:** **MEDIUM**.

---

### Candidate D: Phase 6B Targeted Resolution & Phase 7 Market Clearance (`resolution.rs`)
- **Location:** `crates/sim-model/src/resolution.rs:518` & `1175`
- **Current Allocation Pattern:**
  - Multiple `HashSet`s (`seen_groups`, `seen_participants`) and intermediate stream vectors (`raw_buyers`, `raw_sellers`, `zero_target_records`, `keyed_stream`).
- **Semantic Risk:** **HIGH**.
  - Phase 6B has dynamic live-state contention and coordinate PRNG theft resolution.
  - Phase 7 enforces delicate integer financial conservation ($TotalRevenue = \sum SellerNet + Tax$) and signed reconciliation cycling.
- **Borrow / Lifetime Difficulty:** **HIGH**.
- **Reason for Non-Selection:** Modifying these structures introduces significant regression risk to financial and interaction invariants.

---

### Candidate E: Phase 3 to Phase 4 Pipeline Intermediate Vectors (`features.rs`, `decision.rs`, `runner.rs`)
- **Location:** `runner.rs:247-251`
- **Current Allocation Pattern:**
  - `let features = phase3_observation_and_features(...)` allocates `Vec<AgentFeatures>`.
  - `let choices = phase4_primary_action_selection(...)` allocates `Vec<PrimaryActionChoice>`.
  - Both are allocated and immediately dropped after the subsequent phase executes.
- **Semantic Risk:** **HIGH (API Breaking)**.
  - Eliminating these allocations would require altering public function signatures (passing `&mut Vec<...>` scratchpads).
  - This would break public crate APIs and require modifying external integration tests, violating AGENTS.md Rule 7.

---

## 4. Rejected Optimization Candidates Summary

| Candidate | Primary Reason for Rejection |
| :--- | :--- |
| **Phase 6B Targeted Interaction Streams** | High semantic risk: delicate tie-breaking resolution keys and sequential live state mutation. |
| **Phase 7 Market Clearance Buffers** | High invariant risk: integer financial conservation and signed deficit reconciliation loops. |
| **Phase 3/4 Pipeline Buffer Passing** | Public API breakage: altering return types breaks existing unit and contract tests. |
| **Phase 4 Mutual Aid / Theft Candidates** | PRNG sensitivity: candidate ordering directly affects floating-point index calculation into PRNG draws. |
| **Global Arena / Scratchpad Pool** | Over-engineering: introduces global or TLS mutable state, violating M0 architectural simplicity. |

---

## 5. Selected M2-07 Optimization Candidate

### **Winner: Phase 9 Mortality Commitment Allocation Optimization**
- **Target File:** `crates/sim-model/src/phases.rs`
- **Target Function:** `phase9_mortality_commitment(world: &mut WorldState)`

### Key Rationale:
1. **Zero Semantic Impact:**
   - Mortality evaluation logic ($health \le 0.0$) remains completely unchanged.
   - Categorization counts (`already_dead_count`, `survivors_count`, `newly_deceased`) and the canonical sort order of `newly_deceased` by `AgentId` remain identical.
2. **Zero Canonical Hash Impact:**
   - Phase 9 only emits mutations through `Command::MortalityStatusCommitment` when `newly_deceased` is non-empty.
   - `CanonicalStateHash`, `CanonicalMetricsHash`, and `CanonicalEventHash` remain 100% bit-exact.
3. **Clean Elimination of Heap Churn:**
   - Eliminates the daily `HashSet<AgentId>` allocation used solely for duplicate verification.
   - Sizing `newly_deceased` appropriately or utilizing allocation-free uniqueness verification completely removes unnecessary heap allocations from Phase 9.
4. **Comprehensive Test Shield:**
   - 30 existing tests in `tests/phase9_mortality_tests.rs` strictly enforce error contracts (`Phase9Error::DuplicateAgent`, `Phase9Error::NonFiniteHealth`, idempotency, zero PRNG consumption).
   - Any regression will be caught immediately by existing tests without test modifications.

---

## 6. Proposed Validation Strategy (for Subsequent Milestone)

When implementation is authorized in the next step, validation will strictly enforce:
1. **Contract Invariance:**
   - `CanonicalStateHash`: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
   - `CanonicalMetricsHash`: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b`
   - `CanonicalEventHash`: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`
2. **Workspace Test Suite:**
   - `cargo test --workspace` must maintain 546 passed, 0 failed.
   - All 30 tests in `phase9_mortality_tests.rs` must pass unmodified.
3. **Clean Code Quality:**
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `git diff --check`
