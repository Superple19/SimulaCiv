# SimulaCiv

[![CI](https://github.com/Superple19/SimulaCiv/actions/workflows/ci.yml/badge.svg)](https://github.com/Superple19/SimulaCiv/actions/workflows/ci.yml)

**A deterministic, high-performance runtime for large-scale discrete simulation.**

SimulaCiv explores how simulations can remain reproducible, scalable, and mechanically inspectable as workload size and interaction complexity increase.

The current reference workload is an agent-based synthetic society. The project is about **simulation runtime engineering**, not social-science modeling, economic forecasting, or a civilization game.

## Status

See the generated milestone table below for current project and stage status.

<!-- BEGIN GENERATED MILESTONE TABLE -->
| Milestone | Stage | Status | Tag | Summary | Document |
|---|---|---|---|---|---|
| M0 | `M0` | graduated | `v0.1.0-m0` | Established deterministic reference semantics, replay, and canonical outputs. | [source](docs/contracts/M1_CONTRACT_FREEZE.md) |
| M1 | `M1` | graduated | `v0.2.0-m1` | Froze the simulation semantic contracts and machine-readable M1 manifest. | [source](docs/contracts/M1_CONTRACT_FREEZE.md) |
| M2 | `M2` | graduated | `v0.3.0-m2` | Implemented the optimized runtime with canonical parity to the frozen semantics. | [source](docs/contracts/M2_OPTIMIZATION_CONTRACT.md) |
| M3 | `M3` | graduated | `v0.4.0-m3` | Delivered deterministic interaction, topology, population, migration, and propagation workloads. | [source](docs/contracts/M3_INTERACTION_CONTRACT.md) |
| M4 | `M4` | graduated | `v0.5.0-m4` | Delivered the counterfactual research runtime and branch-analysis viewer. | [source](docs/contracts/M4_COUNTERFACTUAL_CONTRACT.md) |
| M5 | `M5-00` | frozen | — | Froze reproducibility definitions, provenance, and the initial data policy. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| M5 | `M5-01` | complete | — | Inventoried numeric M0–M4 claims and their sources and reproducibility status. | [source](docs/reproducibility/M5_01_INVENTORY.md) |
| M5 | `M5-02` | complete | — | Reorganized repository documentation, reports, data, and analysis files. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| M5 | `M5-03` | complete | — | Added the pinned analysis toolchain, deterministic report generation, and analysis CI. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| M5 | `M5-04` | complete | — | Regenerated M4 deterministic outputs and conclusion-bearing timing claims. | [source](docs/reproducibility/M5_04_M4_REGENERATION.md) |
| M5 | `M5-05` | complete | — | Measured sessions one through three and completed the M3-09 protocol-scope recheck. | [source](docs/reproducibility/M5_05_M0_M3_REGENERATION.md) |
| M5 | `M5-06` | planned | — | Add PyO3 bindings and a Python runner with content-digest, configuration, and host provenance instead of commit-hash provenance. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-07` | planned | — | Run multi-seed, multi-workload robustness experiments with effect-size-first design. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-08` | planned | — | Clean up project documentation, generate figures, and prepare newcomer-facing material. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-09` | planned | — | Review related work and draft the technical report. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-AUDIT` | planned | — | Audit M5 reproducibility and research-hardening results. | [source](docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
<!-- END GENERATED MILESTONE TABLE -->

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

analysis/                 pinned Python analysis package, report tooling, and tests

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

## M3 — graduated

See the [M3 graduation audit](docs/reports/M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md).

## M4 — graduated — `v0.5.0-m4`

See the [M4 graduation audit](docs/reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md).

## Build and validate

```sh
just hooks-install
just ci
just ship
```

Run the Rust checks directly as an alternative:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

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
