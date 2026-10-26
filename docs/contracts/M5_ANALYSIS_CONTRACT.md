# M5 Analysis Contract

Status: **DRAFT — NOT FROZEN**  
Baseline: `v0.5.0-m4`

Research question: **Can the project's research conclusions be reproduced from explicit evidence, and which conclusions survive rechecks?**

M0–M4 used contract-first simulation work. M5 analysis instead grew a prose-extracted 32,465-row reference ledger and multiple status writers; the repeated regressions show that prose rows are the wrong verdict unit. This draft defines one small, reviewable conclusion registry and one deterministic verdict calculation.

## 1. Unit and registry

The unit is a **conclusion plus its supporting experiment**, not each reported number. A conclusion is one sentence with a testable predicate over specified outputs. Its scope is the tuple `(profile, configuration, ordered cell IDs, horizon, output/predicate, revision role)`. `contracts/conclusions.toml` is hand-written and reviewed. ID prefixes match their registered type (`DET`, `TIM`, or `QUAL`). `docs/reproducibility/claims.csv.gz` remains a navigation index only; its rows and labels never decide a verdict.

Each `[[conclusion]]` requires `id`, `milestone`, `statement`, `verbatim`, `type`, `source`, `experiment`, `report_revision`, `report_path_at_revision`, `tier`, and per-role `coverage`. `id` is unique; `experiment` is one stable ID resolving to one protocol record. `report_revision` is exactly one full commit SHA: the first commit whose source report contains the verbatim result. Find it with `git log --follow`; verify `verbatim` in the cited anchor at that commit's then-path, and store the then-path in `report_path_at_revision`. The pin is the first report SHA containing that exact text at that path. For a contract-sourced result, apply the same first-containing rule to the cited frozen contract. `statement` records the original outcome, not an M5 outcome. Legacy `prior_verbatim_conclusion` and `prior_pass_criterion` preserve old M5-05 wording but do not define the active predicate. `type` is `deterministic`, `timing`, or `qualitative`; timing includes host-dependent time and OS resource measurements judged against a registered relative criterion.

Tier 1 consists of every deterministic conclusion plus the explicitly named main-text performance conclusions in the Tier 1 list below. Each Tier 1 experiment has a complete protocol record with ordered `ordered_cell_ids`, `predicate_kind` and all `predicate_parameters`, `host_id`, `harness_paths`, report SHA/then-path, scope, and historical/rerun sampling where timed. Tier 2 contains all other conclusions; its protocol record may be an `incomplete` stub, and its verdict is `unverified` until a complete record exists. A Tier 2 result cannot be promoted into main text by prose alone. Coverage is recorded independently for `report` and `head` roles, using `sufficient`, `partial`, `rerunnable`, or `missing`; these are coverage labels, not verdicts. The `report` role is the pinned report revision. The `head` role is keyed by the exact head SHA.

The Tier 1 performance conclusions are M2's M0-reference runtime speedup (disabled and full-telemetry cells) and explicit-Rayon speedup; M3-09 incremental equal-live, M3 propagation Phase 9c and whole-tick speedups, M3 static-scale completion and phase first-break; and the M4 branch-parallel speedups. The result is asserted only at the cells listed in its protocol record.

The registry may include `coverage` (`sufficient`, `partial`, `rerunnable`, or `missing`) and `paper_role` (`main` or `limitations`) as editorial metadata, not verdicts. `sufficient` means recorded evidence covers every declared cell for the named role; `partial` means some, but not all, cells or required artifacts are covered; `rerunnable` means a named test/harness and inputs exist but no result at the cited revision is recorded; `missing` means a required source or output is unavailable. Coverage is never a third verdict vocabulary. A `[[limitation]]` requires `id`, `milestone`, `statement`, `source`, and `reason`; non-testable context belongs there, not in the conclusion set. “Registered” means present as a validated record in the cited TOML/JSONL registry. “Testable” means the record declares an output predicate that returns a defined result for every declared cell.

## 2. Evidence records

`contracts/evidence.jsonl` is append-only. Each record has a unique id, `conclusion_id`, `role`, evidence `kind`, full-SHA `revision`, `toolchain`, `protocol_id`, date, and artifact paths with SHA-256 hashes. For `head` records, `revision` is the exact `head_sha` and the record also stores `results_relevant_digest`. `protocol_id` must equal the conclusion's `experiment` id. `toolchain` records exact `rustc -Vv`, target triple, Cargo.lock SHA-256, Python/uv and analysis lock SHA-256 when relevant, host OS/build, CPU model/architecture, core counts, and external tools; irrelevant fields are explicit null.

The results-relevant digest is SHA-256 over the byte-sorted list of each relative path, NUL, and SHA-256(file bytes), with a final newline. Its input paths are all files under `crates/*/src/`, every tracked `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and the harness files named by that experiment's protocol. A `head` result applies to a later commit only if this digest is identical. Otherwise that later `(conclusion_id, head_sha)` verdict is `unverified (stale)`. `contracts/hosts.toml` is the sole reference-host record. Optional `supersedes` ids retain prior records for audit.

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

This table is exhaustive; ranks are fixed and cannot be overridden. `structured_recovered_output` records provenance only; it never establishes reproduction. Superseded-protocol records are history only. A deterministic exact match may be established on another host because the comparison is byte/field equality; the full toolchain is still recorded. Timing evidence is accepted only on the reference host with the registered toolchain and protocol; a disqualifying difference makes it `inconclusive`. The specific disqualifiers are CPU model, physical/logical core counts, rustc version or target, and A-5 settings; OS build differences are recorded but do not disqualify. An unexpected kind/type pair is invalid and yields `needs-decision`.

## 3. Verdicts

A pure function returns one verdict for each `(conclusion_id, role)` from the current evidence set: `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive`, `unverified`, `unverified (stale)`, or `needs-decision`. It uses the frozen rules in [REPRODUCTION_CRITERIA.md](../reproducibility/REPRODUCTION_CRITERIA.md); no caller may override them. A Tier 2 conclusion with an incomplete protocol is `unverified` regardless of its old ledger label.

| Existing label/source | Registry verdict rule |
|---|---|
| M5-05 `holds` / `conclusion holds`, `does not hold`, `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive` | Descriptive source labels only; recompute from valid registered predicates. An invalid cell is `inconclusive`. |
| M3-05 pilot `preserved`, `changed`, `within-noise`, `threshold side changed`, `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive (validity)`, `reproduced (environmental)` | Recompute from valid evidence. `preserved` maps to the appropriate reproduced band only when valid; `changed` maps to `not reproduced` only when valid; `within-noise` and failed validity map to `inconclusive`. `threshold side changed` is `not reproduced` only when the original threshold margin exceeds the registered noise bound, otherwise `inconclusive`. Environmental values use the frozen timing bands and remain labeled environmental. |
| `bit-exact`, `bit-exact after normalization`, `bit-exact after documented normalization` | `reproduced` only with exact deterministic evidence and a named reviewed normalization; otherwise `needs-decision`. |
| `missing-data`, `missing-source`, `unrecoverable`, `unverifiable`, recovered-output mismatch/weak/unverifiable | `unverified` if there is no applicable accepted evidence; recovered evidence alone does not prove reproduction. |
| `historical-protocol`, `historical-log`, `recovered-original-output` (formatted/rounded/unit/other-run/derived), `recovered-weak`, `recovered-output-mismatch` | Archival evidence only; `unverified` absent independent accepted evidence. |
| `analysis-only`, `unmapped`, `not evaluated`, unspecified scope, absolute-time-only claim, or a null claim without a declared threshold/predicate | `needs-decision`; do not register non-testable prose as a conclusion. |
| `config-value` or other input-only labels | Not a conclusion; omit from the registry and verdict set. |

Predicates use only this closed set, with all parameters stored in the protocol record:

| Predicate | Required parameters and rule |
|---|---|
| `exact` | Deterministic fields/bytes or a named registered normalization; exact equality. |
| `ratio_band` | Ordered cell IDs, one historical ratio per cell, ratio direction, and frozen 10%/20% deviation bands from [REPRODUCTION_CRITERIA.md](../reproducibility/REPRODUCTION_CRITERIA.md). |
| `threshold_side` | Ordered cell IDs, threshold, comparison direction, historical value per cell, and the criteria noise rule; values within the noise band are inconclusive. |
| `completion` | Ordered cell IDs and the required completion/error state. |
| `no_gain` | Ordered paired cells, paired effect and MAD fields, and the exact `abs(effect) <= MAD` predicate. |

No other predicate kind is accepted. A qualitative source without one of these predicates remains Tier 2 with an incomplete protocol or moves to limitations. A finite paired-variance ratio above 1 means pairing lowers estimator variance. Any registered proportion states its numerator and denominator; “majority” means strictly more than half, with exact-zero cells excluded only when the protocol says so. A minimum cause is the satisfying set with lowest cardinality; equal-size ties use the M4-05 lexicographically smallest sorted intervention-ID rule.

For each cell, merge only valid, hashed records that cover that cell, choosing the highest-strength record. A record failing protocol validity or missing an artifact hash is ignored, not evidence. Same-cell, same-strength `reproduced` and `reproduced (loose)` resolve to `reproduced (loose)`; a same-strength pass/fail conflict resolves to `inconclusive`; identical verdicts remain unchanged. Report and head roles are evaluated separately. No append-order or latest-record tie-break is allowed.

For `all`, aggregate in this order: `needs-decision` > `not reproduced` > `inconclusive` > `unverified` > `reproduced (loose)` > `reproduced`. If every cell is unverified, the aggregate is `unverified`; if only some are unverified, the aggregate is `inconclusive`. A cell with only failed-validity records is `inconclusive`; a cell with no applicable valid record, including one whose only records lack hashes, is `unverified`. Unhashed records are ignored and never create `needs-decision`. Other aggregation operators must be listed in the contract before use.

No records, or historical-only records, yield `unverified`. Failed A-5 validity yields `inconclusive`. Unhashed records are ignored. A timing host/toolchain mismatch yields `inconclusive`; a deterministic cross-host byte-identical result remains valid. Absolute elapsed time without a frozen relative criterion and null/absence claims without an explicit threshold yield `needs-decision`.

`contradicts` means a valid record's measured outcome fails the experiment predicate. “Equal or higher strength” means the fixed rank table above within the same conclusion and role; there are no strength defaults. A protocol ID is the exact `experiment` key resolving to one protocol record. Supersession is a graph, not an overwrite: a link must resolve to a record for the same conclusion and role and may supersede only equal- or lower-strength evidence. A weaker superseding record, cross-conclusion/role link, missing target, self-link, or cycle makes that conclusion's evidence set invalid and returns `needs-decision`. Valid supersession removes the target and its superseded ancestors from the active set; all remaining records are considered together, so append order cannot change a verdict. No generator writes a verdict or any decision status.

## 4. Gates and paper scope

Required gates stay limited to explicit conclusion/evidence schema and anchor checks, evidence hash and supersession validation, this verdict function, registered report generation, data/tool integrity, and simulation determinism/parity tests. Every **new** check must be declared, have a red test (a test that feeds deliberate invalid input and asserts failure), and be reachable from a named `just` gate target. Existing exceptions are listed with deadlines in the migration plan. “Prose heuristic” means a regex, text position, or wording guess used to infer verdicts, evidence classes, or generated-file ownership; it cannot be a new gate rule.

The M5 report may state only registered Tier 1 conclusions in main text; Tier 2 remains unverified until its protocol is complete. A **main-text claim** is a factual assertion in report prose, a table/figure title or caption, or the executive summary; limitations are not main-text results. Each main-text result names its role and has accepted evidence for that role: report evidence for historical reproduction, head evidence for the exact current SHA. A **partial coverage** result means at least one declared cell is evidenced and at least one is not; it cannot be summarized as a complete conclusion. Every reported conclusion cites its ID and evidence records.

### Registered normalizations

Only these M5-04 transformations may support a “bit-exact after documented normalization” result:

| Normalization ID | Field-specific transformation |
|---|---|
| `m4_04_unbounded_ci_high` | For the listed upper confidence-bound fields only, JSON `null` and CSV `inf` both represent positive infinity. |
| `m4_05_schema_v1_to_v2` | Normalize the M4-05 output schema marker from version 1 to version 2. |
| `m4_05_guarantee_labels_v1_to_v2` | Normalize `Minimum`→`minimum`, `OneMinimal`→`one_minimal`, and `None`→`incomplete` or `exhausted_no_cause` according to the recorded search completion state. |

No unlisted normalization can be added during comparison; it requires a reviewed registry amendment first.

The proposed module and recipe migration is in [M5_ANALYSIS_MIGRATION_PLAN.md](M5_ANALYSIS_MIGRATION_PLAN.md); it is a plan, not authorization to move or delete files.
