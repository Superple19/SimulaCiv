# SimulaCiv

[![CI](https://github.com/Superple19/SimulaCiv/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Superple19/SimulaCiv/actions/workflows/ci.yml)

**A deterministic, high-performance runtime for large-scale discrete simulation.**

SimulaCiv explores how simulations can remain reproducible, scalable, and mechanically inspectable as workload size and interaction complexity increase.

The current reference workload is an agent-based synthetic society. The project is about **simulation runtime engineering**, not social-science modeling, economic forecasting, or a civilization game.

## Status

**M4 graduated — `v0.5.0-m4`.** M3 graduated at `v0.4.0-m3` ([M3 final audit](docs/reports/M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)); settlement migration was delivered in [M4-06](docs/reports/M4_06_MIGRATION.md).

Canonical State, Metrics, and Events digests and snapshots are verified on Linux, Windows, and macOS in CI.

| Milestone | Status | Result |
|---|---|---|
| M0 — `v0.1.0-m0` | Complete | Deterministic executable reference semantics and replay gate |
| M1 — `v0.2.0-m1` | Complete | Frozen semantic contracts and machine-readable contract manifest |
| M2 — `v0.3.0-m2` | Graduated | High-performance runtime with canonical parity |
| M3 — `v0.4.0-m3` | Graduated — M3-00 through M3-11 complete; settlement migration delivered in M4-06 | Deterministic relationship-graph runtime, M3P population dynamics, incremental topology, feature switches, and propagation; [final audit](docs/reports/M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md), [M4-06 migration](docs/reports/M4_06_MIGRATION.md) |
| M4 — Counterfactual | Graduated — `v0.5.0-m4` | Static system registry, snapshot-backed branching, divergence measurement, CRN efficiency, minimal-cause search, opt-in settlement migration, and measured many-branch execution; [M4 final audit](docs/reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md), [M4 branch viewer](viewer/README.md#m4-branch-comparison) |
| M5 — Reproducibility and Research Hardening | In progress — M5-02 Repository layout; M5-01 inventory complete | Organize reproducibility records and repository navigation under the frozen M5 layout; [M5 contract](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md), [M5-01 inventory](docs/reproducibility/M5_01_INVENTORY.md) |

The M2 production path uses authoritative Segmented SoA storage. Serial remains the default; deterministic Phase4 Rayon execution is an explicit opt-in, and Auto policy remains experimental. Canonical State, Metrics, Events, and Day 100/250/500 snapshots were validated for exact parity. The measured workloads scale near-linearly through 100k agents; the 200k high-K results remain follow-up evidence. Timings depend on workload and host.

## Design philosophy

**Correct first. Freeze the semantics. Then optimize. Then increase complexity.**

M0 establishes the reference behavior, M1 freezes its contracts, and M2 optimizes execution without semantic drift. M4 asks **“In a deterministic world, how precisely can cause and effect be separated?”** The next direction is M5 **Reproducibility and research hardening**, as recorded in the [M4 final audit](docs/reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md#m5-proposal-reproducibility-and-research-hardening).

## What the runtime explores

- deterministic execution, coordinate-based randomness, and replay
- explicit behavioral contracts and canonical outputs
- data-oriented state storage and phase execution
- scalable processing of state and interactions
- deterministic parallel execution
- performance improvements that preserve frozen semantics

## Runtime structure

```text
crates/
├─ sim-core/              stable IDs, money/day primitives, coordinate-based PRNG
└─ sim-model/             reference model, phases, production runners, storage,
   ├─ src/                events, metrics, canonical hashing, snapshots
   └─ examples/           committed trace exporters and research harnesses

analysis/                 standard-library documentation link checker and analysis tools

docs/
├─ README.md               documentation map
├─ design/                 design specification
├─ contracts/              M1–M5 contracts and index
├─ reports/                M2–M4 performance reports and audits
├─ data/                   committed experiment data and per-stage indexes
└─ reproducibility/        M5 claims ledger and evidence inventory

viewer/                   static local viewer for M3 traces and M4 branches
```

The runtime keeps logical `AgentId` identity separate from physical `DenseSlot` layout. The M2 production runner uses transient runtime scratch for execution; canonical state and snapshots remain defined by the frozen contracts.

## Reference workload

The model represents a synthetic population with biological, economic, group, and interaction state. It provides a repeatable workload for exercising the runtime; it is **not intended to predict real societies**.

## M3 stage plan

- **M3-00 — Reference workload:** complete; relationship graph, neighbor targeting, global Phase 6B, and separate M3 goldens.
- **M3-01 — Optimized serial path:** complete; exact M3-00 parity and [serial baseline profile](docs/reports/M3_01_SERIAL_BASELINE.md).
- **M3-02 — Deterministic parallel Phase 6B:** complete; conflict-based scheduling with serial as the default and parallel as explicit opt-in.
- **Optional contention dial after M3-02:** any graph mode that controls endpoint contention needs its own contract amendment.
- **M3-03 (formerly M3-02b) — Search and bookkeeping reduction:** complete; explicit serial opt-in with exact parity ([report](docs/reports/M3_03_SEARCH_REDUCTION.md)).
- **M3-04 (formerly M3-E) — Equilibrium workload:** complete; benchmark-only configuration search ([workload report](docs/reports/M3_04_EQUILIBRIUM_WORKLOAD.md); [sustained workload](docs/reports/M3_04_SUSTAINED_WORKLOAD.md)).
- **M3-05 (formerly M3-S) — Scale limit search:** complete; static-world scale measurements ([scale report](docs/reports/M3_05_SCALE_LIMIT.md)).
- **M3-06 (formerly M3-K) — Settlement lookup indexing:** complete ([measurements](docs/reports/M3_06_SETTLEMENT_INDEX.md)).
- **M3-07 — Residual scaling diagnosis:** complete; controlled scale rerun and access-pattern analysis ([report](docs/reports/M3_07_SCALE_RESIDUAL.md)).
- **M3-08 (formerly M3-P) — Population dynamics:** complete; births, dynamic graph cleanup, snapshots, sustained CI gate, and [performance profile](docs/reports/M3_08_POPULATION_DYNAMICS.md).
- **M3-09 — Incremental topology maintenance:** complete; append newborn CSR and Phase 4 projection rows on non-compaction days ([performance report](docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md)).
- **M3-10 — Feature switches:** complete; explicit runtime feature dispatch under frozen compatibility rules.
- **M3-11 (formerly M3-05) — Propagation:** complete; synchronous multi-hop spread with serial default and explicit parallel opt-in ([performance report](docs/reports/M3_11_PROPAGATION.md)).
- **Spatial migration (formerly M3-03):** delivered as a registered system in [M4-06](docs/reports/M4_06_MIGRATION.md).
- **M3 audit and graduation:** complete; [M3-GRADUATE](docs/reports/M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md), with no M4 architecture change indicated by current evidence.

M3 is graduated for the measured workloads; settlement migration was delivered in M4-06.

The central research question is:

> How much interaction complexity can a deterministic simulation runtime support before its execution model or data architecture must fundamentally change?

## M4 direction — graduated — `v0.5.0-m4`

M4 studies counterfactual branching: **In a deterministic world, how precisely can cause and effect be separated?** M4-00 through M4-07 and the branch-comparison viewer are complete. The [final audit](docs/reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md) answers the research question and records the remaining limits. Divergence measurements are in the [M4-03 report](docs/reports/M4_03_DIVERGENCE.md), paired versus independent CRN estimates are in the [M4-04 report](docs/reports/M4_04_CRN_EFFICIENCY.md), minimal-cause experiments are in the [M4-05 report](docs/reports/M4_05_MINIMAL_CAUSE.md), migration experiments are in the [M4-06 report](docs/reports/M4_06_MIGRATION.md), many-branch results are in the [M4-07 report](docs/reports/M4_07_MANY_BRANCH.md), and the [branch viewer](viewer/README.md#m4-branch-comparison) includes a [sample trace](docs/data/m4_viewer/food_0_h90.jsonl). See also: [M4-01 report](docs/reports/M4_01_SYSTEM_REGISTRY.md), [M4-02 report](docs/reports/M4_02_BRANCHING.md), [M4-03 frozen contract](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md#12-m4-03-divergence-measurement), [M4-04 frozen contract](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md#13-m4-04-crn-efficiency), [M4-05 frozen contract](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md#14-m4-05-minimal-cause-search), [M4-06 frozen contract](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md#15-m4-06-settlement-migration-system), [M4-07 frozen contract](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md#16-m4-07-many-branch-performance).

Hosted Linux, Windows, and macOS CI passed on commit `5a4972c`.

## Build and validate

The Rust toolchain is pinned in [`rust-toolchain.toml`](rust-toolchain.toml). From the repository root:

```sh
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Run the baseline benchmark with:

```sh
cargo bench --bench m0_baseline_bench
```

Open the [read-only viewer](viewer/index.html) for local M3 trace exploration and M4 branch comparison.

## Further reading

- [Documentation map](docs/README.md)
- [Contract index](docs/contracts/README.md)
- [Performance report index](docs/reports/README.md)
- [Research data index](docs/data/README.md)
- [M5 reproducibility contract](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md)
- [M5-01 claim inventory](docs/reproducibility/M5_01_INVENTORY.md)
- [System design specification](docs/design/SIMULACIV_DESIGN_SPECIFICATION.md)
- [M1 contract freeze](docs/contracts/M1_CONTRACT_FREEZE.md)
- [M2 optimization contract](docs/contracts/M2_OPTIMIZATION_CONTRACT.md)
- [M3 interaction contract](docs/contracts/M3_INTERACTION_CONTRACT.md)
- [M4 counterfactual contract](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md)
- [M2 final runtime profile and graduation gate](docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)
- [M3 final runtime profile and graduation gate](docs/reports/M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)
- [M4 final audit and graduation gate](docs/reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md)
