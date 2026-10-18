# SimulaCiv

[![CI](https://github.com/Superple19/SimulaCiv/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Superple19/SimulaCiv/actions/workflows/ci.yml)

**A deterministic, high-performance runtime for large-scale discrete simulation.**

SimulaCiv explores how simulations can remain reproducible, scalable, and mechanically inspectable as workload size and interaction complexity increase.

The current reference workload is an agent-based synthetic society. The project is about **simulation runtime engineering**, not social-science modeling, economic forecasting, or a civilization game.

## Status

**M2 graduated — `v0.3.0-m2`. M3-00, M3-01, M3-02, and M3-02b are complete ([interim audit](docs/performance/M3_INTERIM_AUDIT.md)); M3-E and M3-S are planned benchmark and measurement stages.** Population dynamics (M3-P) requires its own contract before implementation.

Canonical State, Metrics, and Events digests and snapshots are verified on Linux, Windows, and macOS in CI.

| Milestone | Status | Result |
|---|---|---|
| M0 — `v0.1.0-m0` | Complete | Deterministic executable reference semantics and replay gate |
| M1 — `v0.2.0-m1` | Complete | Frozen semantic contracts and machine-readable contract manifest |
| M2 — `v0.3.0-m2` | Graduated | High-performance runtime with canonical parity |
| M3 | In progress — M3-00, M3-01, M3-02, and M3-02b complete; M3-E and M3-S planned | Reference and optimized relationship-graph paths, deterministic parallel Phase 6B, opt-in search reduction, exact digest parity, [serial baseline](docs/performance/M3_01_SERIAL_BASELINE.md), [parallel Phase 6B profile](docs/performance/M3_02_PARALLEL_PHASE6B.md), and [search-reduction report](docs/performance/M3_02B_SEARCH_REDUCTION.md); M3-00 goldens verified on Linux, Windows, and macOS |

The M2 production path uses authoritative Segmented SoA storage. Serial remains the default; deterministic Phase4 Rayon execution is an explicit opt-in, and Auto policy remains experimental. Canonical State, Metrics, Events, and Day 100/250/500 snapshots were validated for exact parity. The measured workloads scale near-linearly through 100k agents; the 200k high-K results remain follow-up evidence. Timings depend on workload and host.

## Design philosophy

**Correct first. Freeze the semantics. Then optimize. Then increase complexity.**

M0 establishes the reference behavior, M1 freezes its contracts, and M2 optimizes execution without semantic drift. The next research question is how to increase interaction complexity, not only agent count.

## What the runtime explores

- deterministic execution, coordinate-based randomness, and replay
- explicit behavioral contracts and canonical outputs
- data-oriented state storage and phase execution
- scalable processing of state and interactions
- deterministic parallel execution
- performance improvements that preserve frozen semantics

## Runtime structure

```text
contracts/
└─ m1_contract.toml       machine-readable frozen contract manifest

crates/
├─ sim-core/              stable IDs, money/day primitives, coordinate-based PRNG
└─ sim-model/             reference model, phases, production runners, storage,
                          events, metrics, canonical hashing, snapshots

docs/
├─ contracts/             M1 semantic freeze and M2 optimization contract
└─ performance/           M2 profiling, gates, and graduation evidence
```

The runtime keeps logical `AgentId` identity separate from physical `DenseSlot` layout. The M2 production runner uses transient runtime scratch for execution; canonical state and snapshots remain defined by the frozen contracts.

## Reference workload

The model represents a synthetic population with biological, economic, group, and interaction state. It provides a repeatable workload for exercising the runtime; it is **not intended to predict real societies**.

## M3 stage plan

- **M3-00 — Reference workload:** complete; relationship graph, neighbor targeting, global Phase 6B, and separate M3 goldens.
- **M3-01 — Optimized serial path:** complete; exact M3-00 parity and [serial baseline profile](docs/performance/M3_01_SERIAL_BASELINE.md).
- **M3-02 — Deterministic parallel Phase 6B:** conflict-based scheduling with serial as the default and parallel as explicit opt-in.
- **M3-03 — Spatial migration:** change settlement membership and daily partitions.
- **M3-04 — Dynamic topology:** create and remove graph edges, persisting topology in snapshots.
- **M3-05 — Propagation:** model multi-hop spread with explicit intra-tick ordering.
- **M3 audit and graduation:** answer the research question and assess whether M4 needs a new architecture.

Completing M3-02 is sufficient to answer the central research question; M3-03 onward deepens the investigation.

The central research question is:

> How much interaction complexity can a deterministic simulation runtime support before its execution model or data architecture must fundamentally change?

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

Open the [read-only M3 trace viewer](viewer/index.html) to load local JSONL traces in the browser.

## Further reading

- [System design specification](SIMULACIV_DESIGN_SPECIFICATION.md)
- [M1 contract freeze](docs/contracts/M1_CONTRACT_FREEZE.md)
- [M2 optimization contract](docs/contracts/M2_OPTIMIZATION_CONTRACT.md)
- [M2 final runtime profile and graduation gate](docs/performance/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)
