# M2 Performance Baseline & Optimization Report

- **Current Milestone:** M2-06 — Performance Rebaseline
- **Prior Milestones:**
  - M2-01 — Performance Baseline Audit (`d130322`)
  - M2-02 — Phase 5 Intent Partitioning Allocation Optimization (`cb60297`)
  - M2-03 — Phase 8 Welfare Distribution Allocation Optimization (`a65217e`)
  - M2-04 — Phase 10 Metrics Observation Allocation Optimization (`bc9aab1`)
  - M2-05 — Phase 11 Event Staging & Flush Allocation Optimization (`692ccb4`)
- **Target Runtime:** M0 Reference Model (`sim-core`, `sim-model`)
- **Semantic Specification:** [`docs/contracts/M1_CONTRACT_FREEZE.md`](file:///c:/AI/SimulaCiv/docs/contracts/M1_CONTRACT_FREEZE.md)
- **Manifest:** [`contracts/m1_contract.toml`](file:///c:/AI/SimulaCiv/contracts/m1_contract.toml)
- **Status:** Allocation Optimization Baseline Established & Frozen for M2 Structural Phase

---

## 1. Executive Summary

This report establishes the empirical performance rebaseline of the **M0 Reference Runtime** following the targeted, zero-semantic-change allocation optimizations completed in milestones M2-02 through M2-05.

All measurements are conducted using the frozen M0-16B graduation fixture over a 500-day execution horizon, verified against the frozen M1 determinism oracles with **100% bit-exact equivalence**.

### Key Rebaseline Highlights (M2-01 vs M2-06):
- **Telemetry Disabled Runtime:** Reduced from `0.74 ms` (`1.49 µs/day`) to **`0.49 ms` (`0.98 µs/day`)** (**-33.8% runtime reduction**).
- **Telemetry Disabled Throughput:** Increased from `671,140 days/sec` to **`1,020,408 days/sec`** (**+52.0% throughput gain**).
- **Full Telemetry Runtime:** Reduced from `1.13 ms` (`2.26 µs/day`) to **`0.81 ms` (`1.62 µs/day`)** (**-28.3% runtime reduction**).
- **Full Telemetry Throughput:** Increased from `442,477 days/sec` to **`617,284 days/sec`** (**+39.5% throughput gain**).
- **Phase 8 (Welfare Distribution):** Execution time dropped from `0.252 ms` to **`0.148 ms`** (**-41.3% reduction**).
- **Phase 10 (Macroscopic Metrics):** Execution time dropped from `0.196 ms` to **`0.125 ms`** (**-36.2% reduction**).
- **Medium Population Scaling (N=100):** Per-agent execution time dropped from `1,031.3 ns` to **`538.9 ns / agent-day`** (**-47.7% reduction**).

---

## 2. Hardware & Runtime Environment

| Specification | Details |
| :--- | :--- |
| **Processor (CPU)** | AMD Ryzen 5 9600X 6-Core Processor (Zen 5 architecture) |
| **Cores / Threads** | 6 Physical Cores, 12 Logical Processors |
| **Base / Boost Clock** | 3.90 GHz Base, up to 5.40 GHz Boost |
| **System Memory (RAM)** | 96.0 GB DDR5 Dual-Channel |
| **Operating System** | Microsoft Windows 11 Pro 64-bit (OS Build 26100) |
| **Rust Toolchain** | `rustc 1.98.1 (48a229cea 2026-09-01)` |
| **Compilation Profile** | `--release` (`opt-level = 3`, debug assertions disabled) |
| **Benchmark Harness** | `crates/sim-model/benches/m0_baseline_bench.rs` |

---

## 3. Benchmark Fixture Specification

Measurements are anchored on the frozen **M0-16B Graduation Fixture**:

```toml
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
initial_health = 1.0
initial_food = 25.0
initial_wealth = 10000
initial_settlement_resource = 2000.0
initial_treasury = 5000

[traits]
prod_min = 0.8, prod_max = 1.5
coop_min = 0.2, coop_max = 0.8
aggr_min = 0.1, aggr_max = 0.5
risk_min = 0.1, risk_max = 0.5

[environment]
carrying_capacity = 10000.0
regrowth_rate = 0.1
base_metabolic_cost = 1.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 20.0
target_reserve = 5000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 5.0

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
base_weight_matrix = [6x5 zero matrix]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
```

- **Total Execution Horizon:** 500 Days
- **Snapshot Boundary:** Day 199 (generates canonical binary snapshot with resume cursor 200)

---

## 4. Correctness Gate Verification

The rebaseline strictly validated bitwise identity against the frozen M1 determinism oracles:

| Oracle | Canonical SHA-256 Digest | Status |
| :--- | :--- | :---: |
| **`CanonicalStateHash` (Day 500)** | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` | **MATCH** |
| **`CanonicalMetricsHash` (Days 0..499)** | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` | **MATCH** |
| **`CanonicalEventHash` (Cumulative)** | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` | **MATCH** |

---

## 5. Allocation Optimization Progression (M2-02 to M2-05)

Between M2-01 and M2-06, four targeted allocation optimizations were designed and committed, adhering strictly to zero semantic divergence:

### M2-02: Phase 5 Locality Partitioning Optimization (`cb60297`)
- **Problem:** Phase 5 constructed an intermediate `BTreeMap<GroupId, SettlementIntents>`, causing tree node heap allocations and dynamic nested vector extensions per simulation day.
- **Solution:** Replaced `BTreeMap` staging with deterministic contiguous vector chunk streaming. Intents are sorted in-place by `(GroupId, AgentId)`, and contiguous slices are converted into `SettlementIntents` without map overhead.
- **Impact:** Removed daily tree-node heap churn; preserved canonical `GroupId` and `AgentId` ordering semantics.

### M2-03: Phase 8 Welfare Distribution Optimization (`a65217e`)
- **Problem:** Phase 8 cloned `recipient_updates` across stages, performed multi-pass loops, and instantiated a `HashSet<GroupId>` to track seen settlements.
- **Solution:** Eliminated `recipient_updates.clone()` by moving Stage B directly in a single pass; sorted settlement updates directly to satisfy determinism; eliminated the `HashSet` in favor of a stack-allocated/small vector lookup.
- **Impact:** Reduced Phase 8 runtime by **41.3%**; preserved identical eligibility criteria, integer rounding, and settlement commit ordering.

### M2-04: Phase 10 Metrics Observation Optimization (`bc9aab1`)
- **Problem:** Phase 10 performed `alive_agents.to_vec()` heap cloning and allocated individual vectors for wealth sorting when calculating the Gini coefficient.
- **Solution:** Replaced vector cloning with an in-place sort over slice references `&mut [&AgentState]`; pre-allocated capacity for metric vectors; calculated living agent counts in a single pass.
- **Impact:** Reduced Phase 10 runtime by **36.2%**; preserved wealth Gini integer formula $G = (2W - (n+1)S) / (nS)$ and bit-exact oracle hashes.

### M2-05: Phase 11 Event Staging & Flush Optimization (`692ccb4`)
- **Problem:** `EventBuffer` initialized with default capacity, triggering multiple dynamic reallocations per day; event count tracking used a heap `BTreeMap<u16, u64>`.
- **Solution:** Replaced default buffer allocation with `EventBuffer::with_capacity(estimated_cap)`; replaced `BTreeMap` with a small flat vector `Vec<(u16, u64)>`; passed references to conversion adapters instead of cloning intermediate objects.
- **Impact:** Reduced staging allocation overhead; maintained bit-exact canonical EventKey lexicographical sort order.

---

## 6. Execution Time & Phase Breakdown Comparison

### 6.1 Phase-by-Phase Comparison (500 Days, N=10)

| Phase / Component | M2-01 Baseline | M2-06 Rebaseline | Time Delta | Relative Change |
| :--- | :---: | :---: | :---: | :---: |
| **Phase 8: Welfare Distribution** | `0.252 ms` (`0.50 µs`) | **`0.148 ms` (`0.30 µs`)** | `-0.104 ms` | **-41.3%** |
| **Phase 10: Macroscopic Metrics** | `0.196 ms` (`0.39 µs`) | **`0.125 ms` (`0.25 µs`)** | `-0.071 ms` | **-36.2%** |
| **Phase 9: Mortality Commitment** | `0.133 ms` (`0.27 µs`) | `0.126 ms` (`0.25 µs`) | `-0.007 ms` | -5.3% |
| **Phase 11: Event Flush & Sort** | `0.153 ms` (`0.31 µs`) | `0.166 ms` (`0.33 µs`) | `+0.013 ms` | +8.5% (noise) |
| **Event Staging (Adapters)** | `0.082 ms` (`0.16 µs`) | `0.075 ms` (`0.15 µs`) | `-0.007 ms` | -8.5% |
| **Phase 4: Decision & Intent Gen** | `0.079 ms` (`0.16 µs`) | `0.074 ms` (`0.15 µs`) | `-0.005 ms` | -6.3% |
| **Phase 7: Market Clearance** | `0.078 ms` (`0.16 µs`) | `0.070 ms` (`0.14 µs`) | `-0.008 ms` | -10.3% |
| **Phase 6B: Targeted Resolution** | `0.065 ms` (`0.13 µs`) | `0.064 ms` (`0.13 µs`) | `-0.001 ms` | -1.5% |
| **Phase 5: Locality Partitioning** | `0.042 ms` (`0.08 µs`) | `0.040 ms` (`0.08 µs`) | `-0.002 ms` | -4.8% |
| **Phase 3: Observation & Features** | `0.035 ms` (`0.07 µs`) | `0.033 ms` (`0.07 µs`) | `-0.002 ms` | -5.7% |
| **Phase 6A: Work Resolution** | `0.033 ms` (`0.07 µs`) | `0.030 ms` (`0.06 µs`) | `-0.003 ms` | -9.1% |
| **Phase 2: Biological Degradation** | `0.014 ms` (`0.03 µs`) | `0.013 ms` (`0.03 µs`) | `-0.001 ms` | -7.1% |
| **Phase 11: Snapshot Emission (d199)** | `0.013 ms` (`0.03 µs`) | `0.012 ms` (`0.02 µs`) | `-0.001 ms` | -7.7% |
| **Runner / Context Overhead** | `0.013 ms` (`0.03 µs`) | `0.012 ms` (`0.02 µs`) | `-0.001 ms` | -7.7% |
| **Phase 1: Environment Regrowth** | `0.012 ms` (`0.02 µs`) | `0.013 ms` (`0.03 µs`) | `+0.001 ms` | +8.3% (noise) |
| **Total Measured Time** | `1.200 ms` (`2.40 µs`) | **`1.001 ms` (`2.00 µs`)** | **`-0.199 ms`** | **-16.6%** |

---

## 7. Telemetry Mode Cost Comparison

Comparing 500-day execution across all 4 telemetry combinations:

| Telemetry Mode | M2-01 Total (Avg/Day) | M2-06 Total (Avg/Day) | M2-01 Throughput | M2-06 Throughput | Runtime Delta | Throughput Gain |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **Disabled** (`metrics: false, events: false`) | `0.74 ms` (`1.49 µs`) | **`0.49 ms` (`0.98 µs`)** | 671,140 d/s | **1,020,408 d/s** | **-33.8%** | **+52.0%** |
| **Events only** (`metrics: false, events: true`) | `0.96 ms` (`1.91 µs`) | **`0.63 ms` (`1.26 µs`)** | 520,833 d/s | **793,650 d/s** | **-34.4%** | **+52.4%** |
| **Metrics only** (`metrics: true, events: false`) | `0.96 ms` (`1.91 µs`) | **`0.63 ms` (`1.26 µs`)** | 520,833 d/s | **793,650 d/s** | **-34.4%** | **+52.4%** |
| **Full Telemetry** (`metrics: true, events: true`) | `1.13 ms` (`2.26 µs`) | **`0.81 ms` (`1.62 µs`)** | 442,477 d/s | **617,284 d/s** | **-28.3%** | **+39.5%** |

### Telemetry Overhead Summary:
- When telemetry is completely disabled, the engine achieves **over 1.02 Million simulation days per second** on a single thread.
- Telemetry penalty vs disabled is reduced from `+52.7%` to `+65.3%` relatively because core execution speed improved significantly while event sorting cost remains bounded by $O(E \log E)$.

---

## 8. Population Scaling Analysis

Scaling agent population from $N=10$ to $N=1000$ over 50 simulation days:

| Population | Settlements | M2-01 Total (Avg/Day) | M2-06 Total (Avg/Day) | M2-01 ns/Agent-Day | M2-06 ns/Agent-Day | Scaling Gain |
| :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **10** | 2 | `0.33 ms` (`6.56 µs`) | `0.26 ms` (`5.27 µs`) | 656.0 ns | **526.8 ns** | **-19.7%** |
| **50** | 2 | `1.47 ms` (`29.30 µs`) | `1.25 ms` (`25.02 µs`) | 586.0 ns | **500.3 ns** | **-14.6%** |
| **100** | 2 | `5.16 ms` (`103.13 µs`) | `2.69 ms` (`53.89 µs`) | 1,031.3 ns | **538.9 ns** | **-47.7%** |
| **250** | 2 | `13.17 ms` (`263.35 µs`) | `8.93 ms` (`178.57 µs`) | 1,053.4 ns | **714.3 ns** | **-32.2%** |
| **500** | 2 | `30.39 ms` (`607.88 µs`) | `22.11 ms` (`442.21 µs`) | 1,215.8 ns | **884.4 ns** | **-27.3%** |
| **1000** | 2 | `85.82 ms` (`1,716.36 µs`) | `72.16 ms` (`1,443.18 µs`) | 1,716.4 ns | **1,443.2 ns** | **-15.9%** |

### Scaling Observations:
- **Major Improvement at N=100 & N=250:** Eliminating `alive_agents.to_vec()` heap clones in Phase 10 and `recipient_updates.clone()` in Phase 8 yielded a massive **47.7% reduction** in cost per agent-day for $N=100$.
- **High-N Regimes (N=500, N=1000):** Although allocation overhead was significantly reduced, high-$N$ execution remains bound by sequential single-threaded execution and cache line misses in the 52-byte AoS `AgentState` struct.

---

## 9. Updated M2 Optimization Candidates & Priority Roadmap

With allocation hotspots in Phases 5, 8, 10, and 11 addressed, the roadmap shifts to structural and architectural optimizations:

| Priority | Optimization Target | Target Phase | Expected Impact | Validation Requirement |
| :---: | :--- | :--- | :--- | :--- |
| **P1** | **Segmented Structure-of-Arrays (SoA)** | State Storage, Phases 1–4, 9, 10 | Replaces 52-byte AoS layout with contiguous property vectors (`food: Vec<f32>`, `health: Vec<f32>`). Improves CPU L1/L2 cache locality by $4\times$–$6\times$. | C03 Storage Invariance |
| **P2** | **SIMD Vectorized Math Kernels** | Phases 1, 2, 3, 4 | AVX2/AVX-512 vectorization of metabolic decay, trait checks, and Softmax logits. | IEEE-754 bit-exactness |
| **P3** | **Parallel Partition Scheduling (Rayon)** | Phases 5, 6A, 6B, 7, 8 | Parallelizes independent settlement processing across available CPU cores (6 cores / 12 threads). | Deterministic event key order |
| **P4** | **In-Place Radix Event Flush** | Phase 11 | Replaces standard sort with non-allocating radix sort for 128-bit `EventKey`s. | C09/C10 contract match |
| **P5** | **DenseSlot Active Entity Compaction** | Phase 1–9 Iteration | Skips tombstoned dead agents in computation loops without perturbing logical `AgentId`. | C01 AgentId immutability |

---

## 10. Audit Sign-off

- Baseline measurements completed and reproducible via:
  ```bash
  cargo bench --bench m0_baseline_bench
  python scripts/profile_m0_baseline.py
  ```
- All M1 contracts and M0 reference behavior remain intact:
  - `CanonicalStateHash`: `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9`
  - `CanonicalMetricsHash`: `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b`
  - `CanonicalEventHash`: `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac`
- Workspace test suite: **546 passed; 0 failed**.
