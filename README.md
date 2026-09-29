# SimulaCiv

**A deterministic, high-performance runtime for large-scale discrete simulation.**

SimulaCiv explores how simulations can remain reproducible, scalable, and mechanically inspectable as workload size and interaction complexity increase.

The current reference workload is an agent-based synthetic society. The project is about **simulation runtime engineering**, not social-science modeling, economic forecasting, or a civilization game.

## Status

**M2 graduated — `v0.3.0-m2`.** M3 implementation has not started; the direction below is research scope, not a frozen M3 specification.

| Milestone | Status | Result |
|---|---|---|
| M0 — `v0.1.0-m0` | Complete | Deterministic executable reference semantics and replay gate |
| M1 — `v0.2.0-m1` | Complete | Frozen semantic contracts and machine-readable contract manifest |
| M2 — `v0.3.0-m2` | Graduated | High-performance runtime with canonical parity |
| M3 | Research direction; not started | Increase interaction and topology complexity while retaining determinism and scalable execution |

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

## M3 research direction

M3 will investigate complex interaction scaling across agent count and interaction density, including:

- sparse relationship graphs
- spatial locality
- dynamic topology
- deterministic conflict resolution
- propagation workloads

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

## Further reading

- [System design specification](SIMULACIV_DESIGN_SPECIFICATION.md)
- [M1 contract freeze](docs/contracts/M1_CONTRACT_FREEZE.md)
- [M2 optimization contract](docs/contracts/M2_OPTIMIZATION_CONTRACT.md)
- [M2 final runtime profile and graduation gate](docs/performance/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md)
