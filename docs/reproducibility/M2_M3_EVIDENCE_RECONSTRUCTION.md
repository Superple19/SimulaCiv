# M2/M3 Evidence Reconstruction Inventory

**Status:** generated from the committed claims ledger, current report sources, Git history, source map, recovered-evidence manifest, and M5-05 regeneration results. No timing or simulation harness was run.

## Scope and method

The source ledger filter selected **484** M2/M3 headline/key claims across **22** reports with status `missing-data` or `missing-source`. Table claims are grouped by source table within a section; prose claims are grouped by paragraph or list item. This inventory is a reconstruction plan, not a verification of the historical measurements.

Report creation commits are the earliest additions found with `git log --follow --diff-filter=A`; report-era harness presence is checked in that commit tree and again at current HEAD. Recovered evidence cites exact `MANIFEST.json#/items/<index>` entries. M5-05 coverage is assigned only to a prose group at the exact report/anchor; representative M5-05 cells do not count as coverage for a full historical results table.

Class labels: (a) deterministic replay at original revision; (b) timing with committed harness; (c) timing with complete recovered harness source; (d) reconstructed timing harness from a sufficiently specified report protocol; (e) protocol underspecified; (f) conclusion already covered by M5-05, with historical details excluded from that coverage.

A-5 lower bounds use 2 warmups plus 41 samples, with each sample at least 1 second: 43 seconds per inferred table row/cell. They exclude build/setup, repetitions within each sample beyond the 1-second floor, noise retries, and cells not enumerated as table rows; they are workload floors, not elapsed-time predictions. Deterministic replay is not an A-5 timing run.

## Counts

| Class | Experiments | Claims |
|---|---:|---:|
| (a) deterministic replay | 2 | 3 |
| (b) committed harness | 69 | 402 |
| (c) complete recovered source | 5 | 31 |
| (d) reconstructable protocol | 2 | 26 |
| (e) underspecified | 0 | 0 |
| (f) M5-05 conclusion covered | 3 | 22 |

Timing-class A-5 minimum for estimable (b)–(d) groups: **2,537 seconds (0.70 hours)**. This lower bound covers groups with an enumerated table-row or explicit-N cell count; **61** additional (b)–(d) groups have no defensible cell count and are unestimated. Class (e) is unestimated because the protocol is incomplete; class (a) deterministic replay has no A-5 duration estimate.

## Ten largest experiment groups

| # | Report and anchor | Claims | Class |
|---:|---|---:|---|
| 1 | `M3_05_SCALE_LIMIT.md#first-break-findings` | 80 | b |
| 2 | `M3_04_SUSTAINED_WORKLOAD.md#comparison-with-m3-02-interim-audit-findings` | 48 | b |
| 3 | `M2_09_PHASE34_SCRATCH_BUFFER_ANALYSIS.md#3-allocation-inventory-summary` | 27 | b |
| 4 | `M3_05_SCALE_LIMIT.md#first-break-findings` | 20 | b |
| 5 | `M3_08_POPULATION_DYNAMICS.md#findings` | 19 | d |
| 6 | `M3_11_PROPAGATION.md#summary` | 15 | f |
| 7 | `M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md#executive-summary` | 13 | b |
| 8 | `M3_09_INCREMENTAL_TOPOLOGY.md#findings` | 13 | c |
| 9 | `M3_09_INCREMENTAL_TOPOLOGY.md#summary` | 12 | c |
| 10 | `M2_37_POST_P5_P6B_REPROFILING.md#executive-summary` | 10 | b |

## Evidence notes

- The CSV records the exact protocol excerpts with report line references, the report-creation commit, harness/test availability at that commit and HEAD, recovered manifest entries, and any M5-05 conclusion result.
- A blank A-5 estimate means the group is not an A-5 timing task or is not estimable from the recorded protocol; it does not mean zero compute cost.
- The recovered archive is evidence only. No recovered binary was executed and no recovered source was compiled in this inventory task.
