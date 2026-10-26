# M5 Analysis Contract

Status: **DRAFT — NOT FROZEN**  
Baseline: `v0.5.0-m4`

Research question: **Can the project's research conclusions be reproduced from explicit evidence, and which conclusions survive rechecks?**

M0–M4 used contract-first simulation work. M5 analysis instead grew a prose-extracted 32,465-row reference ledger and multiple status writers; the repeated regressions show that prose rows are the wrong verdict unit. This draft defines one small, reviewable conclusion registry and one deterministic verdict calculation.

## 1. Unit and registry

The unit is a **conclusion plus its supporting experiment**, not each reported number. A conclusion is one falsifiable sentence scoped by a stable experiment id and a full source revision. `contracts/conclusions.toml` is hand-written and reviewed. `docs/reproducibility/claims.csv.gz` remains a navigation index only; its rows and labels never decide a verdict.

Each `[[conclusion]]` requires `id`, `milestone`, `statement`, `verbatim`, `type`, `source`, `experiment`, `revisions`, and `aggregation`. `id` and `experiment` are unique stable identifiers. `type` is `deterministic`, `timing`, or `qualitative`. `source` is a report path plus anchor. `verbatim` is the exact source sentence at that anchor; a mismatch blocks evidence registration. `statement` states that original conclusion in its original scope, never an M5 outcome. `revisions` contains full commit SHAs only. In an evidence record, `role=report` means the source-report revision and its recorded toolchain; `role=head` means the exact branch tip being assessed. Roles are not encoded as aliases in the registry. Optional `claim_keys` link to the ledger without verdict meaning.

The registry may include `coverage` (`sufficient`, `partial`, `rerunnable`, or `missing`) and `paper_role` (`main` or `limitations`) as review metadata, not verdicts. `sufficient` means recorded evidence covers the registered cells and role; `partial` covers only part of them; `rerunnable` means a named harness/test exists but its result at the pinned source revision is absent; `missing` means the required source or output is unavailable. A `[[limitation]]` requires `id`, `milestone`, `statement`, `source`, and `reason`; non-falsifiable context belongs there, not in the conclusion set.

## 2. Evidence records

`contracts/evidence.jsonl` is append-only. Each record has a unique id, `conclusion_id`, `role`, evidence `kind`, full-SHA `revision`, exact toolchain, protocol id, date, and artifact paths with SHA-256 hashes. Optional `supersedes` ids retain the prior records for audit. An artifact is hashed from its bytes; a missing hash or a hash mismatch is not evidence.

Fixed strength order and type/role acceptance:

| Strength | Kind | Deterministic | Timing | Qualitative |
|---:|---|---|---|---|
| 6 | `deterministic_exact` | report or head; exact fields/bytes at that revision | no | no |
| 5 | `a5_report_revision` | no | report role only; frozen A-5 protocol | no |
| 4 | `a5_head` | no | head role only; frozen A-5 protocol | no |
| 3 | `structured_recovered_output` | archival context only | archival context only | archival context only |
| 2 | `qualitative_source` | no | no | report or head; bounded predicate and hashed source |
| 1 | `historical_protocol` | archival only | archival only | archival only |
| 0 | `none` | no | no | no |

This table is exhaustive. Recovered or superseded-protocol records cannot by themselves reproduce a conclusion. Deterministic evidence requires byte/field equality; timing evidence must match the registered host, toolchain and protocol; qualitative evidence must test the exact bounded statement.

## 3. Verdicts

A pure function returns one verdict for each `(conclusion_id, role)` from the complete current evidence set: `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive`, `unverified`, or `needs-decision`. It uses the frozen rules in [REPRODUCTION_CRITERIA.md](../reproducibility/REPRODUCTION_CRITERIA.md); no caller may override them.

| Existing label/source | Registry verdict rule |
|---|---|
| M5-05 `reproduced`, `reproduced (loose)`, `not reproduced` | Map to the same verdict only when the matching timing record is valid under the frozen criteria; otherwise `inconclusive`. |
| M3-05 pilot `reproduced`, `reproduced (loose)`, `not reproduced` | Same timing rule. `preserved` maps to `reproduced` only when valid; `changed` maps to `not reproduced` only when valid. |
| M3-05 `inconclusive (validity)`, M5-05 `inconclusive` | `inconclusive`. |
| `bit-exact`, `bit-exact after normalization`, `bit-exact after documented normalization` | `reproduced` only with exact deterministic evidence and a named, reviewed normalization; otherwise `needs-decision`. |
| `holds`, `conclusion holds`, `does not hold`, `changed` | Descriptive labels only; recompute from evidence and criteria. |
| `missing-data`, `missing-source`, `unverifiable`, recovered-output mismatch/weak/unverifiable | `unverified` when no applicable evidence exists; recovered output alone stays `unverified`. |
| `historical-protocol`, `historical-log`, `recovered-original-output` (including formatted/rounded/unit/other-run/derived), `recovered-weak`, `recovered-output-mismatch` | `unverified`; retain as lower-strength archival evidence. |
| `analysis-only`, `unmapped`, `not evaluated`, unspecified scope, absolute-time-only claim, or a null claim without a declared threshold/predicate | `needs-decision`; exclude non-falsifiable prose from the registry. |
| `config-value` or other input-only labels | Not a conclusion; omit from the registry and verdict set. |

For the M5-05 `holds`/`does not hold` and M3-05 pilot `preserved`/`changed` labels, recompute from their registered cell predicates; the labels alone have no status. Pilot `inconclusive (validity)` and any invalid A-5 path map to `inconclusive`. A finite paired-variance ratio above 1 means pairing lowers estimator variance; any qualitative quantifier such as “generally” must be made an explicit proportion in `aggregation`.

Algorithm: validate records and supersession links; remove records superseded by a valid same-conclusion, same-role record; select the highest-strength accepted evidence; compute its cell verdicts; then apply the conclusion's explicit `aggregation`. There is no latest-record or file-order tie-break. If equal-strength accepted records agree, use their common verdict; if they conflict, return `inconclusive`. For multiple cells, deterministic `all` requires every cell exact and fails on any mismatch; timing `all` fails if any valid cell fails, is inconclusive if any cell is invalid/unmeasured, otherwise returns `reproduced (loose)` if any cell is loose and `reproduced` if all are reproduced; other aggregations must be named and specified in the experiment record, including their cell order and predicate.

No records yields `unverified`; historical-only records yield `unverified`; failed A-5 validity yields `inconclusive`; unhashed or hash-mismatched artifacts yield `needs-decision` unless separate valid evidence determines the verdict. Timing evidence on a different host/toolchain is `inconclusive`, not a pass. Absolute elapsed time without a frozen relative criterion and null/absence claims without an explicit threshold are `needs-decision`.

Supersession is a graph, not an overwrite: a link must resolve to an existing record for the same conclusion and role and may supersede only equal- or lower-strength evidence. A weaker superseding record, cross-conclusion/role link, missing target, self-link, or cycle makes that conclusion's evidence set invalid and returns `needs-decision`. Valid supersession removes the target and its superseded ancestors from the active set; all remaining records are considered together, so append order cannot change a verdict. No generator writes a verdict or any decision status.

## 4. Gates and paper scope

Required gates stay limited to explicit conclusion/evidence schema and anchor checks, evidence hash and supersession validation, this verdict function, registered report generation, data/tool integrity, and simulation determinism/parity tests. Every gate check must be declared, have a red test, and be reachable from a named gate. Recovery, fuzzy matching, inventory construction, and one-off migrations are provenance tools, not verdict gates.

The M5 report may state only registered conclusions. Main-text conclusions require the matching accepted evidence for their role; missing evidence, scope gaps, host/toolchain mismatch, and historical-only data appear as `unverified`, `inconclusive`, or limitations. It will cover M0/M1 reproducibility and frozen semantics; M2 speed under canonical parity; M3 scaling and interaction complexity; and M4 branching, divergence, CRN, minimal causes, migration, and many-branch execution. Every reported conclusion cites its id and evidence records.

The proposed module and recipe migration is in [M5_ANALYSIS_MIGRATION_PLAN.md](M5_ANALYSIS_MIGRATION_PLAN.md); it is a plan, not authorization to move or delete files.
