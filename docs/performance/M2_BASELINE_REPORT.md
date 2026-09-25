# M2-01 Performance Baseline Audit Report

- **Milestone:** M2-01 — Performance Baseline Audit
- **Target Runtime:** M0 Reference Model (`sim-core`, `sim-model`)
- **Semantic Specification:** [`docs/contracts/M1_CONTRACT_FREEZE.md`](file:///c:/AI/SimulaCiv/docs/contracts/M1_CONTRACT_FREEZE.md)
- **Manifest:** [`contracts/m1_contract.toml`](file:///c:/AI/SimulaCiv/contracts/m1_contract.toml)
- **Status:** Baseline Established & Frozen for M2 Optimization Comparison

---

## 1. Executive Summary

This report establishes the empirical performance baseline of the **M0 Reference Runtime** prior to introducing high-performance data structures, parallel scheduling, and SIMD vectorization in Milestone M2.

All measurements were taken using the canonical M0-16B graduation fixture over a 500-day execution horizon, verified against the frozen M1 determinism oracles with **100% bit-exact equivalence**.

### Key Baseline Metrics:
- **Total Execution Time (500 Days, N=10):** `1.13 ms` (Full Telemetry), `0.74 ms` (Telemetry Disabled)
- **Average Time per Day:** `2.26 µs / day` (Full Telemetry), `1.49 µs / day` (Telemetry Disabled)
- **Daily Simulation Throughput:** `442,477 days / sec` (Full Telemetry), `671,140 days / sec` (Telemetry Disabled)
- **Telemetry Overhead:** `+52%` total runtime penalty when macroscopic metrics and event streaming are enabled
- **Scaling Horizon (N=1000):** `1,716 µs / day` (~`1.72 ns / agent / day`)

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

The baseline run strictly validated bitwise identity against the frozen M1 determinism oracles:

| Oracle | Canonical SHA-256 Digest | Status |
| :--- | :--- | :---: |
| **`CanonicalStateHash` (Day 500)** | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` | **MATCH** |
| **`CanonicalMetricsHash` (Days 0..499)** | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` | **MATCH** |
| **`CanonicalEventHash` (Cumulative)** | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` | **MATCH** |

---

## 5. Execution Time & Phase Breakdown

### 5.1 Cumulative Phase Breakdown (500 Days, N=10)

```
+------------------------------------+------------+-------------+-----------+
| Phase / Component                  |  Time (ms) | Avg/Day(µs) | Share (%) |
+------------------------------------+------------+-------------+-----------+
| Phase 8: Welfare Distribution      |   0.252 ms |     0.50 µs |     21.0% |
| Phase 10: Macroscopic Metrics      |   0.196 ms |     0.39 µs |     16.4% |
| Phase 11: Event Flush & Sort       |   0.153 ms |     0.31 µs |     12.8% |
| Phase 9: Mortality Commitment      |   0.133 ms |     0.27 µs |     11.1% |
| Event Staging (Adapters)           |   0.082 ms |     0.16 µs |      6.8% |
| Phase 4: Decision & Intent Gen     |   0.079 ms |     0.16 µs |      6.6% |
| Phase 7: Market Clearance          |   0.078 ms |     0.16 µs |      6.5% |
| Phase 6B: Targeted Resolution      |   0.065 ms |     0.13 µs |      5.4% |
| Phase 5: Locality Partitioning     |   0.042 ms |     0.08 µs |      3.5% |
| Phase 3: Observation & Features    |   0.035 ms |     0.07 µs |      2.9% |
| Phase 6A: Work Resolution          |   0.033 ms |     0.07 µs |      2.7% |
| Phase 2: Biological Degradation    |   0.014 ms |     0.03 µs |      1.1% |
| Phase 11: Snapshot Emission (d199) |   0.013 ms |     0.03 µs |      1.1% |
| Runner / Context Overhead          |   0.013 ms |     0.03 µs |      1.1% |
| Phase 1: Environment Regrowth      |   0.012 ms |     0.02 µs |      1.0% |
+------------------------------------+------------+-------------+-----------+
| Total Instrumented Execution       |   1.200 ms |     2.40 µs |    100.0% |
+------------------------------------+------------+-------------+-----------+
```

### 5.2 Key Phase Findings
1. **Welfare Distribution (Phase 8)** is the largest single runtime consumer (~21.0%) due to sorting eligible agents by `AgentId` and multi-settlement treasury allocation loops.
2. **Macroscopic Metrics (Phase 10)** consumes ~16.4% of execution time, dominated by sorting living agent wealth vectors to compute the rank-weighted Gini coefficient.
3. **Event Staging, Flush, and Canonical Sort (Phase 11)** consumes nearly ~20% of runtime (Staging 6.8% + Flush/Sort 12.8%) due to constructing `EventRecord` envelopes, appending into intermediate collections, and lexicographical sorting by `EventKey`.

---

## 6. Telemetry Mode Cost Analysis

Comparing execution over 500 days across the 4 valid telemetry combinations:

| Telemetry Mode | Total Time | Avg Time / Day | Throughput | Cost vs Disabled |
| :--- | :---: | :---: | :---: | :---: |
| **Disabled** (`metrics: false, events: false`) | `0.74 ms` | `1.49 µs` | 671,140 days/sec | **Baseline (1.00x)** |
| **Events only** (`metrics: false, events: true`) | `0.96 ms` | `1.91 µs` | 520,833 days/sec | +29.7% |
| **Metrics only** (`metrics: true, events: false`) | `0.96 ms` | `1.91 µs` | 520,833 days/sec | +29.7% |
| **Full Telemetry** (`metrics: true, events: true`) | `1.13 ms` | `2.26 µs` | 442,477 days/sec | **+52.7%** |

### Snapshot Cost Analysis:
- Non-snapshot day execution: `~2.2 µs`
- Day 199 (with canonical binary snapshot encoding): `~14.5 µs`
- **Snapshot Emission Cost:** `~12.3 µs` per snapshot event.

---

## 7. Population Scaling Analysis

Scaling agent population from $N=10$ to $N=1000$ over 50 simulation days:

```
+------------+-------------+------------------+-----------------+--------------------+
| Population | Settlements | Total Time (ms)  | Avg / Day (µs)  | Per Agent/Day (ns) |
+------------+-------------+------------------+-----------------+--------------------+
| 10         | 2           |          0.33 ms |         6.56 µs |           656.0 ns |
| 50         | 2           |          1.47 ms |        29.30 µs |           586.0 ns |
| 100        | 2           |          5.16 ms |       103.13 µs |         1,031.3 ns |
| 250        | 2           |         13.17 ms |       263.35 µs |         1,053.4 ns |
| 500        | 2           |         30.39 ms |       607.88 µs |         1,215.8 ns |
| 1000       | 2           |         85.82 ms |     1,716.36 µs |         1,716.4 ns |
+------------+-------------+------------------+-----------------+--------------------+
```

### Scaling Observations:
- **Sub-Linear to Linear Scaling up to N=50**: High fixed per-phase framework overhead dominates small populations.
- **Super-Linear Growth above N=250**: Per-agent cost increases from `586 ns` to `1,716 ns` ($2.93\times$ increase per agent).
- **Primary Bottlenecks under Scaling**:
  - Phase 10 wealth sorting: $O(N \log N)$
  - Phase 11 event buffering and lexicographical sorting: $O(E \log E)$ where $E \approx O(N)$
  - Sequential single-threaded execution across settlements.
  - Array-of-Structures (AoS) cache misses: as $N$ grows beyond L1 cache capacity, scanning the `Vec<AgentState>` becomes memory-bandwidth bound.

---

## 8. Allocation & Micro-Architectural Bottlenecks

### 8.1 Heap Allocation Hotspots (M0 Reference Code)
1. **Config Cloning**: In `run_m0_day` (line 236), `let mut effective_config = config.clone()` executes on every single day, triggering dynamic heap reallocation for matrix vectors and biases.
2. **Phase Intermediate Collections**:
   - Phase 3: Allocates `Vec<AgentFeatureVector>` (heap array of 5-element float tuples).
   - Phase 4: Allocates `Vec<PrimaryActionChoice>` and `Vec<Intent>`.
   - Phase 5: Allocates `BTreeMap<GroupId, SettlementIntents>` with inner vectors.
   - Phases 6A, 6B, 7, 8: Allocate per-settlement resolution vectors.
3. **Event Envelope Allocations**:
   - `EventBuffer` instantiates a new vector each day.
   - Conversion adapters (`events_from_work_resolution`, `events_from_targeted_resolution`, etc.) construct small individual vectors and push to buffer.

### 8.2 Memory Layout Inefficiencies (AoS)
- `AgentState` struct size is 52 bytes:
  - Accessing `health` (4 bytes) in Phase 9 or `wealth` (8 bytes) in Phase 10 loads an entire 64-byte cache line, wasting over 85% of memory bus bandwidth on irrelevant fields.
- Non-vectorizable single-threaded iteration prevents CPU SIMD execution.

---

## 9. M2 Optimization Candidates & Priority Roadmap

Based on the empirical breakdown and profiling data, the M2 performance roadmap is prioritized as follows:

| Priority | Optimization Target | Target Phase | Expected Impact | Validation Requirement |
| :---: | :--- | :--- | :--- | :--- |
| **P1** | **Zero-Allocation Execution & Buffer Reuse** | Phases 1–11, Runner | Eliminates 80%+ of heap allocations; removes config cloning; pre-allocated scratchpad buffers. | Exact hash match |
| **P2** | **Segmented Structure-of-Arrays (SoA)** | State Storage, Phases 1–4, 9, 10 | Transforms memory layout from 52-byte AoS to contiguous property arrays (`food: Vec<f32>`, `health: Vec<f32>`). Increases cache efficiency $4\times$–$6\times$. | C03 Storage Invariance |
| **P3** | **Parallel Partition Scheduling (Rayon)** | Phases 5, 6A, 6B, 7, 8 | Parallelizes independent settlement processing across available CPU cores (6 cores / 12 threads). | Deterministic event key order |
| **P4** | **SIMD Vectorized Math Kernels** | Phases 1, 2, 3, 4 | AVX2/AVX-512 vectorization of metabolic decay, trait checks, and Softmax logits. | IEEE-754 bit-exactness |
| **P5** | **In-Place Sorting & Gini Optimization** | Phase 10, Phase 11 | Replaces allocating sorts with pre-allocated in-place radix/quicksort for wealth and event keys. | C09/C10 contract match |
| **P6** | **DenseSlot Active Entity Compaction** | Phase 1–9 Iteration | Skips tombstoned dead agents in computation loops without perturbing logical `AgentId`. | C01 AgentId immutability |

---

## 10. Audit Sign-off

- Baseline measurements completed and reproducible via:
  ```bash
  cargo bench --bench m0_baseline_bench
  python scripts/profile_m0_baseline.py
  ```
- All M1 contracts and M0 reference behavior remain intact.
- Workspace test suite: **546 passed; 0 failed**.
