# Contract index

| Milestone | Contract | State |
|---|---|---|
| M1 | [M1 contract freeze](M1_CONTRACT_FREEZE.md) | Frozen semantic reference, including canonical state, RNG, events, and snapshots. |
| M2 | [M2 optimization contract](M2_OPTIMIZATION_CONTRACT.md) | Frozen implementation boundary for preserving M1 behavior in optimized paths. |
| M3 | [M3 interaction contract](M3_INTERACTION_CONTRACT.md) | Frozen through the completed M3 workload and stages; later planned stages are marked separately. |
| M4 | [M4 counterfactual contract](M4_COUNTERFACTUAL_CONTRACT.md) | M4-00 through M4-07 frozen and complete. |
| M5 | [M5 reproducibility contract](M5_REPRODUCIBILITY_CONTRACT.md) | M5-00 and M5-02 frozen; M5-01 inventory complete; later stages planned. |

The machine-readable M1 projection stays at [`contracts/m1_contract.toml`](../../contracts/m1_contract.toml), at the repository root. Existing tests and consumers read that exact path; moving it would break the established interface without adding value.

The machine-readable decision and stage registry begins with M5 at [`contracts/decisions.toml`](../../contracts/decisions.toml). M1–M4 contracts remain historical documents and are not generated from this registry.
