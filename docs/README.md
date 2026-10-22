# Documentation map

Start with the [project README](../README.md) for the runtime overview and milestone status. This directory groups the specifications, contracts, reports, data, and reproducibility records.

| Location | Purpose | Start here |
|---|---|---|
| [`design/`](design/SIMULACIV_DESIGN_SPECIFICATION.md) | System and simulation design specification | [Design specification](design/SIMULACIV_DESIGN_SPECIFICATION.md) |
| [`contracts/`](contracts/README.md) | Frozen semantic/runtime contracts and current M5 contract work | [Contract index](contracts/README.md) |
| [`reports/`](reports/README.md) | M2–M4 performance reports and audits | [Report index](reports/README.md) |
| [`data/`](data/README.md) | Committed experiment and viewer data | [Data index and size policy](data/README.md) |
| [`reproducibility/`](reproducibility/M5_01_INVENTORY.md) | M5 claim ledger and source/data inventory | [M5-01 inventory](reproducibility/M5_01_INVENTORY.md) |
| [`../analysis/`](../analysis/check_links.py) | Standard-library documentation link checker and analysis tools | [Link checker](../analysis/check_links.py) |
| [`../viewer/`](../viewer/README.md) | Local M3 trace and M4 branch-comparison viewer | [Viewer guide](../viewer/README.md) |

`contracts/m1_contract.toml` remains at the repository root because automated tests and contract consumers use that stable path; see the [contract index](contracts/README.md).
