# M2-17 Hot Path SoA Expansion Analysis: Phases 2 & 3

## 1. Executive Summary & Objective

In **M2-16** and **M2-16.1**, we demonstrated that a reusable Structure-of-Arrays (SoA) scratch buffer (`AgentDynamicSoAScratch`) delivered significant performance gains for **Phase 10: Macroscopic Metrics Observation** (improving 500-day Phase 10 execution from 0.150 ms to 0.046 ms, a **3.26x speedup**; and delivering up to 1.24x scaling speedups at $N=1000$).

The objective of **M2-17** is to rigorously evaluate whether this SoA scratch approach should be expanded to earlier hot execution phases in the daily simulation loop:
1. **Phase 2: Biological Degradation** ($O(N)$ metabolic decay and starvation calculation)
2. **Phase 3: Observation & Normalized Feature Extraction** ($O(N)$ agent-level normalized feature vector generation)

Phase 4 (Decision) was deliberately excluded due to its tight coupling with coordinate PRNG sequences and complex dispatch structures.

### Key Finding

| Execution Mode | N=100 | N=250 | N=500 | N=1000 | Speedup vs AoS |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **Combined Phase 2+3 AoS Baseline** | **0.37 µs** | **0.95 µs** | **1.91 µs** | **3.95 µs** | **1.00x** (Baseline) |
| **Combined Phase 2+3 Ephemeral SoA** | **0.55 µs** | **1.36 µs** | **2.71 µs** | **5.88 µs** | **0.67x** (33% Slower) |
| **Combined Phase 2+3 Pure SoA (Zero Roundtrip)** | **0.30 µs** | **0.75 µs** | **1.50 µs** | **3.12 µs** | **1.27x** (27% Faster) |

**Empirical Verdict:**
- **Ephemeral SoA Scratch Expansion is an Anti-Pattern for In-Place Write Phases:** When the authoritative simulation state remains Array-of-Structures (`Vec<AgentState>`), the conversion round-trip (`collect_from_agents` AoS $\to$ SoA followed by `write_back_phase2` SoA $\to$ AoS) incurs **~2.76 µs/day of memory bandwidth overhead at $N=1000$**. This overhead completely wipes out the contiguous streaming gains, resulting in an overall **33% slowdown**.
- **Pure SoA Kernel is 1.27x Faster:** If agent data is *already* resident in contiguous column vectors (as envisioned in a full M2 runtime rewrite), executing Phase 2 and Phase 3 sequentially achieves a consistent **1.27x speedup** across all population scales.
- **Architectural Decision:** Phases 2 and 3 must **remain in-place AoS** within the current M2 reference runtime. Ephemeral SoA buffering should only be applied to read-heavy, sorting-dominated phases like Phase 10. Full SoA migration should only occur if the authoritative storage of `WorldState` itself transitions to SoA.

---

## 2. Architecture & Implementation of Hot Path SoA Expansion

### 2.1 Extension of `AgentDynamicSoAScratch`

To support both Phase 2 and Phase 3 without referring back to authoritative `AgentState` records during observation, `AgentDynamicSoAScratch` was extended with `group_ids`:

```rust
pub struct AgentDynamicSoAScratch {
    pub health: Vec<f32>,
    pub food: Vec<f32>,
    pub wealth: Vec<Money>,
    pub alive: Vec<bool>,
    pub agent_ids: Vec<AgentId>,
    pub group_ids: Vec<GroupId>,       // Added in M2-17 for Phase 3 locality lookup
    pub indices: Vec<usize>,
    pub settlement_indices: Vec<usize>,
}
```

Two dedicated operational methods were added to the scratch view:
1. `update_biological_degradation(f_metabolic, decay_rate)`: Contiguous SIMD-friendly vector traversal across `health`, `food`, and `alive` parallel slices.
2. `write_back_phase2(agents)`: Efficient sequential stream copying updated `health` and `food` fields back to the authoritative `Vec<AgentState>`.

### 2.2 Phase 2 SoA Implementation

Phase 2 computes metabolic consumption and starvation health decay for every alive agent:
- $F_{\text{consumed}} = \min(\text{food}, F_{\text{metabolic}})$
- $F_{\text{deficit}} = F_{\text{metabolic}} - F_{\text{consumed}}$
- $\Delta H = -\text{decay\_rate} \times F_{\text{deficit}}$
- $\text{food} \leftarrow \max(0.0, \text{food} - F_{\text{consumed}})$
- $\text{health} \leftarrow \text{clamp}(\text{health} + \Delta H, 0.0, 1.0)$

In SoA:
```rust
pub fn update_biological_degradation(&mut self, f_metabolic: f32, decay_rate: f32) {
    let n = self.health.len();
    for i in 0..n {
        if !self.alive[i] {
            continue;
        }
        let f_consumed = self.food[i].min(f_metabolic);
        let f_deficit = f_metabolic - f_consumed;
        let health_delta = -decay_rate * f_deficit;

        self.food[i] = (self.food[i] - f_consumed).max(0.0);
        self.health[i] = (self.health[i] + health_delta).clamp(0.0, 1.0);
    }
}
```
Because `self.food`, `self.health`, and `self.alive` are non-aliasing slices, LLVM can auto-vectorize this loop using packed SSE/AVX instructions.

### 2.3 Phase 3 SoA Implementation & Settlement Scarcity Optimization

Phase 3 produces a 5-dimensional normalized feature vector for every behaviorally eligible agent (`alive && health > 0.0`):
$$\vec{\phi} = [\text{hunger\_ratio}, \text{wealth\_pressure}, \text{health\_deficit}, \text{local\_scarcity}, \text{food\_surplus}]$$

In the baseline implementation, each eligible agent performed a linear search over `world.settlements` to obtain `settlement.resource` and compute `local_scarcity`. 

In `phase3_observation_and_features_soa_into`, two critical optimizations were introduced:
1. **Precomputed Settlement Scarcity:** Before streaming agents, settlement scarcity values are precomputed once on the stack (`[(GroupId, f32); 8]`), eliminating repetitive floating-point divisions and clamps across thousands of agent iterations:
   $$\text{local\_scarcity}_g = 1.0 - \text{clamp}(R_g / K, 0.0, 1.0)$$
2. **Sequential Streaming:** The observation pass streams linearly through the parallel SoA vectors, avoiding cache line misses from large `AgentState` struct padding.

---

## 3. Canonical Determinism & Correctness Verification (M1 Gate)

To guarantee that the SoA Phase 2 and Phase 3 kernels introduce zero deviation in numerical rounding, PRNG state, or canonical order, a full 500-day graduation trajectory was executed using the Combined SoA Phase 2+3 pipeline.

### Verification Results

| Hash Identifier | Canonical Graduation Expected | SoA Expansion Actual | Match Status |
| :--- | :--- | :--- | :---: |
| **CanonicalStateHash** | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` | **100% BIT-EXACT** |
| **CanonicalMetricsHash**| `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` | **100% BIT-EXACT** |
| **CanonicalEventHash**  | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` | **100% BIT-EXACT** |

**Zero Divergence:**
- All 546 workspace tests passed.
- All 66 oracle determinism tests passed.
- Canonical state, metrics, and event log hashes match the frozen M1 contract bit-for-bit over 500 simulated days.

---

## 4. Empirical Performance & Ablation Results

### 4.1 500-Day Canonical Trajectory (N=10, Cumulative Timing)

| Component | Time (ms) | Avg/Day (µs) | Share of SoA Run |
| :--- | :---: | :---: | :---: |
| **Total 500-Day Execution Time** | **0.671 ms** | **1.34 µs** | **100.0%** |
| AoS $\to$ SoA Collect Overhead | 0.019 ms | 0.04 µs | 2.8% |
| Phase 2 SoA Update | 0.012 ms | 0.02 µs | 1.8% |
| Phase 3 SoA Feature Extraction | 0.016 ms | 0.03 µs | 2.4% |
| SoA $\to$ AoS Write-back Overhead | 0.010 ms | 0.02 µs | 1.5% |
| Combined Phase 2+3 SoA Pipeline | 0.057 ms | 0.11 µs | 8.5% |

Simulation throughput reached **744,934 days/sec** on the graduation configuration ($N=10$).

### 4.2 Population Scaling Benchmark (50 Days, ReplicateId=7)

Latency measured in **microseconds per day (µs/day)**:

| Pop ($N$) | Phase 2 AoS | Phase 2 SoA (Iso) | Phase 3 AoS | Phase 3 SoA (Iso) | Phase 2+3 AoS | Phase 2+3 SoA | Phase 2+3 Pure SoA | Pure Speedup | Net SoA Speedup |
| :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **100** | 0.13 | 0.35 | 0.32 | 0.42 | **0.37** | 0.55 | **0.30** | **1.23x** | 0.67x |
| **250** | 0.28 | 0.84 | 0.80 | 1.03 | **0.95** | 1.36 | **0.75** | **1.27x** | 0.70x |
| **500** | 0.53 | 1.62 | 1.53 | 2.05 | **1.91** | 2.71 | **1.50** | **1.27x** | 0.70x |
| **1000** | 1.12 | 3.67 | 3.62 | 4.43 | **3.95** | 5.88 | **3.12** | **1.27x** | 0.67x |

---

## 5. In-Depth Root Cause & Trade-off Analysis

### 5.1 Arithmetic Intensity vs. Memory Bandwidth

To understand why SoA scratch expansion succeeded in Phase 10 but failed in Phase 2 and Phase 3, we analyze the arithmetic intensity:

1. **Phase 10 (Metrics Observation):**
   - Operations: $O(N)$ food summation, $O(N \log N)$ sorting of living wealth values, $O(N)$ prefix-sum Gini calculation.
   - Access pattern: Pure read-only. **Zero write-back**.
   - Arithmetic intensity: High (sorting and floating-point divisions amortize reading from memory).
   - Scratch reuse effect: Eliminated massive daily heap allocation for `Vec<AgentState>` sorting.
2. **Phase 2 (Biological Degradation):**
   - Operations: 1 min, 1 multiplication, 2 subtractions, 1 max, 1 clamp per agent.
   - Operations per agent: ~6 scalar operations.
   - Memory access in AoS: Single sequential pass over `world.agents` (32 bytes per agent; fits 2 agents per 64-byte L1 cache line). The update is done in-place in L1 cache.
   - Memory access in Ephemeral SoA:
     1. Read `world.agents` to populate scratch vectors.
     2. Write to `scratch.health`, `scratch.food`, `scratch.alive`.
     3. Read `scratch.food`, `scratch.alive`, write `scratch.health`, `scratch.food`.
     4. Read `scratch.health`, `scratch.food`, write back to `world.agents`.
   - Result: Ephemeral SoA touches memory **4 times** instead of **1 time**. For low-arithmetic-intensity loops, memory traffic dominates.

### 5.2 The Conversion Tax

At $N=1000$:
- Phase 2+3 AoS takes **3.95 µs**.
- Pure SoA computation (if data is already SoA) takes **3.12 µs** (a saving of 0.83 µs).
- Conversion tax (`collect_from_agents` + `write_back_phase2`) takes **2.76 µs**.
- Net SoA time: $3.12 + 2.76 = \mathbf{5.88\ \mu s}$ (a net regression of 1.93 µs, or **-33% slower**).

Even though sharing the scratch view between Phase 2 and Phase 3 amortized the collection step, the cost of round-tripping through AoS storage remains higher than the algorithmic benefit.

---

## 6. Architectural Decision & M2 Roadmap

### 6.1 Formal Decision

1. **Retain In-Place AoS Execution for Phases 1 through 9:**
   Do not introduce ephemeral SoA conversion into the daily execution pipeline of `run_m0_day` or `run_m0_day_with_scratch`.
2. **Preserve Prototype APIs for Specialized Harnesses & M2 Benchmarking:**
   Keep the newly introduced helper functions in `sim-model`:
   - `phase2_biological_degradation_soa`
   - `phase2_biological_degradation_with_scratch`
   - `phase3_observation_and_features_soa_into`
   - `phase3_observation_and_features_with_scratch`
   These functions serve as validated reference implementations for subsequent runtime milestones.
3. **M2 Storage Architecture Constraint:**
   Any future migration to Structure-of-Arrays (SoA) must be **end-to-end**:
   - The authoritative `WorldState` representation itself must be stored in contiguous column arrays.
   - Ephemeral or piecemeal phase-by-phase conversion between AoS and SoA storage must be strictly avoided.

---

## 7. Verification Summary

- **Compilation:** `cargo check --workspace` passed cleanly.
- **Unit Tests:** `cargo test --workspace` (546/546 passed).
- **Oracle Tests:** `cargo test -p sim-model --test determinism_oracle_tests -- --nocapture` (66/66 passed).
- **Linter:** `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings).
- **Formatting:** `cargo fmt --check` (100% compliant).
- **Git Diffs:** `git diff --check` (clean).
- **Git Status:** No git commits or branches created.
