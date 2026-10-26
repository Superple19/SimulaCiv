# M5 Analysis Contract

Status: **DRAFT — NOT FROZEN**  
Baseline: `v0.5.0-m4`

Research question: **Can the project's research conclusions be reproduced from explicit evidence, and which conclusions survive rechecks?**

M0–M4 used contract-first simulation work. M5 analysis instead grew a prose-extracted 32,465-row reference ledger and multiple status writers; the repeated regressions show that prose rows are the wrong verdict unit. This draft defines one small, reviewable conclusion registry and one deterministic verdict calculation.

## 1. Unit and registry

The unit is a **conclusion plus its supporting experiment**, not each reported number. A conclusion is one sentence with a testable predicate over specified outputs. Its scope is the tuple `(profile, configuration, ordered cell IDs, horizon, output/predicate, revision role)`. `contracts/conclusions.toml` is hand-written and reviewed. `docs/reproducibility/claims.csv.gz` remains a navigation index only; its rows and labels never decide a verdict.

Each `[[conclusion]]` requires `id`, `milestone`, `statement`, `verbatim`, `type`, `source`, `experiment`, `revisions`, and `aggregation`. `id` is unique; `experiment` is one stable ID that resolves to exactly one protocol record in `contracts/experiments.toml`, never free text. A protocol record pins the source section, row order, and artifacts defining its cell set; vague phrases such as “selected cells” do not define a cell set. `type` is `deterministic`, `timing`, or `qualitative`; `timing` includes host-dependent time and OS resource measurements judged against a registered relative criterion. `source` is a report or contract path plus anchor. `verbatim` is the exact source sentence or complete table-cell text at that anchor; a mismatch blocks evidence registration. `statement` states the original conclusion, never an M5 outcome. `revisions` contains full commit SHAs only. Legacy `prior_verbatim_conclusion` and `prior_pass_criterion` fields preserve superseded M5-05 wording; they do not replace the active source text or predicate. In evidence, `role=report` means the full SHA of the revision where the source result was reported; `role=head` means the full SHA being assessed. These are separate questions, not aliases in the registry. Optional `claim_keys` link to the ledger without verdict meaning.

The registry may include `coverage` (`sufficient`, `partial`, `rerunnable`, or `missing`) and `paper_role` (`main` or `limitations`) as editorial metadata, not verdicts. `sufficient` means recorded evidence covers every declared cell for the named role; `partial` means some, but not all, cells or required artifacts are covered; `rerunnable` means a named test/harness and inputs exist but no result at the cited revision is recorded; `missing` means a required source or output is unavailable. Coverage is never a third verdict vocabulary. A `[[limitation]]` requires `id`, `milestone`, `statement`, `source`, and `reason`; non-testable context belongs there, not in the conclusion set. “Registered” means present as a validated record in the cited TOML/JSONL registry. “Testable” means the record declares an output predicate that returns a defined result for every declared cell.

## 2. Evidence records

`contracts/evidence.jsonl` is append-only. Each record has a unique id, `conclusion_id`, `role`, evidence `kind`, full-SHA `revision`, `toolchain`, `protocol_id`, date, and artifact paths with SHA-256 hashes. `protocol_id` must equal the conclusion's `experiment` id. `toolchain` is a JSON object with exact `rustc -Vv` output, target triple, Cargo.lock SHA-256, Python and uv versions plus analysis lock SHA-256 when Python is used, host OS/build, CPU model/architecture, physical/logical core counts, and exact external tool versions used; fields irrelevant to the protocol are explicit `null`. There are no implicit toolchain defaults. Optional `supersedes` ids retain prior records for audit. An artifact is hashed from its bytes; a missing hash or a hash mismatch is not evidence.

Fixed strength order and type/role acceptance:

| Strength | Kind | Deterministic | Timing | Qualitative |
|---:|---|---|---|---|
| 6 | `deterministic_exact` | report or head; exact fields/bytes at that revision | no | no |
| 5 | `a5_report_revision` | no | report role only; frozen A-5 protocol | no |
| 4 | `a5_head` | no | head role only; frozen A-5 protocol | no |
| 3 | `structured_recovered_output` | archival context only | archival context only | archival context only |
| 2 | `qualitative_source` | no | no | report or head; complete registered predicate and hashed source |
| 1 | `historical_protocol` | archival only | archival only | archival only |
| 0 | `none` | no | no | no |

This table is exhaustive; ranks are fixed and cannot be overridden. `structured_recovered_output` records provenance only; it never establishes reproduction. Superseded-protocol records are history only. A deterministic exact match may be established on another host because the comparison is byte/field equality; the full toolchain is still recorded. Timing evidence is accepted only on the reference host with the registered toolchain and protocol; a difference makes it `inconclusive`. A qualitative record must evaluate the complete registered predicate against hashed evidence; prose alone is insufficient. An unexpected kind/type pair is invalid and yields `needs-decision`.

## 3. Verdicts

A pure function returns one verdict for each `(conclusion_id, role)` from the complete current evidence set: `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive`, `unverified`, or `needs-decision`. It uses the frozen rules in [REPRODUCTION_CRITERIA.md](../reproducibility/REPRODUCTION_CRITERIA.md); no caller may override them.

| Existing label/source | Registry verdict rule |
|---|---|
| M5-05 `holds` / `conclusion holds`, `does not hold`, `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive` | Descriptive source labels only; recompute from valid registered predicates. An invalid cell is `inconclusive`. |
| M3-05 pilot `preserved`, `changed`, `within-noise`, `threshold side changed`, `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive (validity)`, `reproduced (environmental)` | Recompute from valid evidence. `preserved` maps to the appropriate reproduced band only when valid; `changed` maps to `not reproduced` only when valid; `within-noise` and failed validity map to `inconclusive`. `threshold side changed` is `not reproduced` only when the original threshold margin exceeds the registered noise bound, otherwise `inconclusive`. Environmental values use the frozen timing bands and remain labeled environmental. |
| `bit-exact`, `bit-exact after normalization`, `bit-exact after documented normalization` | `reproduced` only with exact deterministic evidence and a named reviewed normalization; otherwise `needs-decision`. |
| `missing-data`, `missing-source`, `unrecoverable`, `unverifiable`, recovered-output mismatch/weak/unverifiable | `unverified` if there is no applicable accepted evidence; recovered evidence alone does not prove reproduction. |
| `historical-protocol`, `historical-log`, `recovered-original-output` (formatted/rounded/unit/other-run/derived), `recovered-weak`, `recovered-output-mismatch` | Archival evidence only; `unverified` absent independent accepted evidence. |
| `analysis-only`, `unmapped`, `not evaluated`, unspecified scope, absolute-time-only claim, or a null claim without a declared threshold/predicate | `needs-decision`; do not register non-testable prose as a conclusion. |
| `config-value` or other input-only labels | Not a conclusion; omit from the registry and verdict set. |

For a no-gain claim, use its declared paired-effect/MAD or confidence-interval predicate; the ±10/20% timing-ratio bands do not apply near a ratio of 1. A finite paired-variance ratio above 1 means pairing lowers estimator variance. Any registered proportion must state its numerator and denominator; “majority” means strictly more than half, with exact-zero cells excluded only when the protocol says so. A minimum cause is the satisfying set with lowest cardinality; equal-size ties use the M4-05 lexicographically smallest sorted intervention-ID rule.

Algorithm: compute independently for each `(conclusion_id, role)`; report and head results never override or rank against each other. Validate records and supersession links; remove records superseded by valid same-conclusion, same-role records; select the highest-strength accepted evidence within that role; calculate cell verdicts; then apply the explicit aggregation. No append-order or latest-record tie-break is allowed. Equal-strength records that agree yield their shared result; conflicting equal-strength records yield `inconclusive`. For `all`, any valid `not reproduced` cell yields `not reproduced`; otherwise any failed-validity or incomplete measured cell yields `inconclusive`; otherwise any loose cell yields `reproduced (loose)`; only all exact/pass cells yield `reproduced`. Preserve and report each cell result alongside the aggregate. Every other aggregation must define its ordered cell IDs and predicate in the experiment record.

No records, or historical-only records, yield `unverified`. Failed A-5 validity yields `inconclusive`. Unhashed or hash-mismatched artifacts yield `needs-decision` unless separate valid records determine the result. A timing host/toolchain mismatch yields `inconclusive`; a deterministic cross-host byte-identical result remains valid. Absolute elapsed time without a frozen relative criterion and null/absence claims without an explicit threshold yield `needs-decision`. A missing cell in an otherwise evidenced multi-cell result makes the aggregate `inconclusive`, while a wholly absent experiment remains `unverified`.

`contradicts` means a valid record's measured outcome fails the experiment's frozen predicate. “Equal or higher strength” means the fixed rank table above within the same conclusion and role; no other strength defaults exist. A protocol ID is the exact `experiment` key that resolves to one protocol record. A `report_revision` from legacy files means role `report`; `HEAD` means the exact full SHA in a `head` evidence record. Supersession is a graph, not an overwrite: a link must resolve to an existing record for the same conclusion and role and may supersede only equal- or lower-strength evidence. A weaker superseding record, cross-conclusion/role link, missing target, self-link, or cycle makes that conclusion's evidence set invalid and returns `needs-decision`. Valid supersession removes the target and its superseded ancestors from the active set; all remaining records are considered together, so append order cannot change a verdict. No generator writes a verdict or any decision status.

## 4. Gates and paper scope

Required gates stay limited to explicit conclusion/evidence schema and anchor checks, evidence hash and supersession validation, this verdict function, registered report generation, data/tool integrity, and simulation determinism/parity tests. Every **new** check must be declared, have a red test (a test that feeds deliberate invalid input and asserts failure), and be reachable from a named `just` gate target. Existing exceptions are listed with deadlines in the migration plan. “Prose heuristic” means a regex, text position, or wording guess used to infer verdicts, evidence classes, or generated-file ownership; it cannot be a new gate rule.

The M5 report may state only registered conclusions. A **main-text claim** is a factual assertion in report prose, a table/figure title or caption, or the executive summary; limitations are not main-text results. Each main-text result names its role and has accepted evidence for that role: report evidence for historical reproduction, head evidence for a current conclusion. A **partial coverage** result means at least one declared cell is evidenced and at least one is not; it cannot be summarized as a complete conclusion. The report covers M0/M1 reproducibility and frozen semantics; M2 speed under canonical parity; M3 scaling and interaction complexity; and M4 branching, divergence, CRN, minimal causes, migration, and branch-parallel execution. Every reported conclusion cites its ID and evidence records.

The proposed module and recipe migration is in [M5_ANALYSIS_MIGRATION_PLAN.md](M5_ANALYSIS_MIGRATION_PLAN.md); it is a plan, not authorization to move or delete files.
