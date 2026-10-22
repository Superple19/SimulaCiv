# Research data

Committed data follows the [M5-00 size policy](../contracts/M5_REPRODUCIBILITY_CONTRACT.md#4-data-size-policy-frozen): each committed data file is at most 1 MiB; larger outputs are prepared as release assets with checksums and a `MANIFEST.json`, and the user uploads the assets. Already-pushed files remain in repository history and are not rewritten. Several existing M4-06 data files predate the policy and are retained as required by that decision.

| Folder | Contents |
|---|---|
| [`m4_02/`](m4_02/README.md) | M4-02 historical paired runs, host records, and worked-example traces. |
| [`m4_03/`](m4_03/README.md) | M4-03 divergence experiments and gate records. |
| [`m4_04/`](m4_04/README.md) | M4-04 CRN experiment outputs. |
| [`m4_05/`](m4_05/README.md) | M4-05 minimal-cause search logs and probes. |
| [`m4_06/`](m4_06/README.md) | M4-06 migration gates and experiments; superseded runs are in `superseded/`. |
| [`m4_07/`](m4_07/README.md) | M4-07 branch, day-scan, and search outputs; invalid attempts are in `superseded/`. |
| [`m4_viewer/`](m4_viewer/README.md) | Sample JSONL trace for the branch-comparison viewer. |
| [`m4_inventory/`](m4_inventory/README.md) | Recovery inventory for M4-02 through M4-05 data, sources, and local binaries. |

Each stage folder's README maps files to their producing harnesses and report use. M5 adds manifests and report-generated data when those stages are authorized.
