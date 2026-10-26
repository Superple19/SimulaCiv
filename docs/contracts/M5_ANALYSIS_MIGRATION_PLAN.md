# M5 Analysis Migration Plan

Status: **DRAFT — PROPOSAL ONLY**
Parent: [M5 Analysis Contract](M5_ANALYSIS_CONTRACT.md)

This plan redoes the earlier file triage against the current import/call graph and frozen decision references. It authorizes no file move, rename, deletion, generator edit, or decision-status change.

## Migration order and constraints

1. Implement and review the conclusion/evidence model in the parent contract before changing verdict ownership.
2. Replace M5-05's selected-run registry with `contracts/conclusions.toml`; append evidence records and compute verdicts without rewriting historical evidence. `docs/reproducibility/m5_05_conclusions.toml` remains a historical source: carry forward each `verbatim_conclusion` and `pass_criterion`, then mark it superseded with a dated Later update note before its consumers are retired. The M5-05 report remains evidence until replacement outputs render and check byte-identically where deterministic.
3. Preserve generated outputs that are cited as evidence and keep their check targets reachable: `claims.csv.gz` as a reference index, the M5-01 inventory, M5-04 and M5-05 reports, generated figures, `docs/data/m5_05/STATUS.md`, and any summary cited by a conclusion. A generated file is not removable merely because its generator is proposed for merge/archive.
4. Only after each replacement is checked should old one-off recovery and inventory commands leave required gates. Preserve their sources and results in an archive with links.
5. Change `m2_m3_reconstruction_finalize.py` to stop writing `contracts/decisions.toml`. It may emit a proposed review artifact, but humans alone change decision status; do not archive the current writer until that behavior is replaced and reviewed.

Later update (2026-10-26): the draft `contracts/conclusions.toml` supersedes `docs/reproducibility/m5_05_conclusions.toml` as the proposed conclusion index. All 13 legacy `verbatim_conclusion` and `pass_criterion` values are preserved in the new registry's active or `prior_*` fields; the old registry remains untouched pending freeze.

### Frozen decisions to amend before a migration changes their behavior

| Decision | Why it must be reviewed first |
|---|---|
| `M5-00-D-3`, `M5-00-D-8` | They make every numeric claim, rather than every conclusion, the acceptance unit. Amend to keep the old ledger as a locator only. |
| `M5-00-A-3` | It freezes the data/claim workflow consumed by the old M5-05 run registry. Amend its source reference before archiving that consumer. |
| `M5-02-D-4` | It reserves `analysis/legacy` for inactive tools. Amend to name the archive destination and state that archiving removes gate ownership, not provenance. |
| `M5-02-D-6`, `M5-03-D-6` | They freeze the prose-extracted claim schema and ledger content. Amend before changing extraction or verdict meaning. |
| `M5-03-D-4` | It freezes the current `just` analysis interface. Amend before merging or retiring the per-report targets. |
| `M5-04-D-6` | It defines the old per-claim M4 status set. Replace its verdict wording before retiring that path. |
| `M5-05-D-2`, `M5-05-D-3`, `M5-05-A1-1`, `M5-05-A1-2`, `M5-05-A1-4` | They freeze selected-run A-5 comparisons, `m5_05_conclusions.toml`, and its 13-run schedule. Replace the source registry and stage accounting before archiving their consumers. |
| `M5-05-M3-09-SCOPE-WITHDRAWAL`, `M5-05-M3-09-PROTOCOL-RECHECK` | They bind the M3-09 scope result to the old registry file. Preserve the result, then amend the reference to the new conclusion/evidence records. |
| `M5-05-D-6` | It requires cited harnesses, data, and generated M5-05 tables. Keep until the replacement report and its checks cover the same evidence. |

Decision IDs above are frozen in `contracts/decisions.toml`; update them through a separately reviewed decision change, not as a side effect of a generator.

## Planned gate targets

No gate manifest exists yet. These are proposed targets for the future verdict path; each must be added to the repository's explicit gate wiring and receive a red test before the corresponding migration is complete.

| Planned target | Function | Current state |
|---|---|---|
| `conclusions-check` | Validate conclusion and protocol schemas, required verbatim source text, anchors, full revisions, and ID uniqueness. | Not implemented. |
| `evidence-check` | Validate append-only evidence records, content hashes, role/kind compatibility, and same-conclusion supersession graph. | Not implemented. |
| `verdicts-check` | Compute each `(conclusion_id, role)` result order-independently from the full current evidence set and frozen criteria. | Not implemented. |
| `report-m5-final-check` | Render conclusions and cited evidence from the registries and compare the committed report. | Not implemented. |

## Import and gate graph

`analysis/simciv_analysis/__init__.py` exports no runtime API. The report entry points import shared I/O, table/figure, findings, provenance, and data-source helpers. M5-01 additionally imports ledger extraction and recovered-claim audit/triage; M5-04 imports claim overrides and source mapping; M5-05 imports its separate conclusion file and computes the selected-run report. The `just` recipes invoke these entry points directly. The intended convergence is one explicit registry/evidence/report path; shared byte-writing, hashing, tools, links, integrity, and delivery checks stay shared.

Generated evidence checks retained through the migration:

| Evidence output | Current check to preserve until replacement is checked |
|---|---|
| `docs/reproducibility/claims.csv.gz` | `claims-check` (reference-index bytes only) |
| M5-01 inventory QMD/Markdown and figures | `report-m5-01-check` |
| M5-04 regeneration report and figures | `report-m5-04-check` |
| M5-05 regeneration report and figure | `report-m5-05-check` |
| registered generated artifacts and DVC summaries | `generated-artifacts-check`, `data-integrity-check` |

## Proposed module disposition

The dispositions follow the current direct imports, report entry points, and named gate calls above. Grouped rows enumerate every current module.

| Disposition | Current modules | Reason |
|---|---|---|
| Keep | `__init__.py`, `io.py`, `tables.py`, `figures.py`, `findings.py`, `links.py`, `provenance.py`, `tool_versions.py`, `check_tools.py`, `quarto_check.py`, `action_refs.py`, `git_hooks.py`, `data_integrity.py`, `dvc_push_guard.py`, `dvc_path_guard.py`, `dvc_freeze_guard.py`, `decisions.py`, `ci_analysis.py`, `ci_clean.py`, `ci_linux.py`, `ship.py` | Common output, hashing, report, toolchain, integrity, and delivery dependencies. Keep the DVC-open guard as a documented heuristic exception pending its M5-06 replacement. |
| Merge | `claims.py`, `sources.py`, `m5_05.py`, `report_m5_01.py`, `report_m5_04.py`, `report_m5_05.py`, `generated_artifacts.py`, `generated_artifacts.toml`, `ledger_transition_guard.py` | Retain `claims.py` only for the reference index; consolidate verdict and report generation around conclusions/evidence while preserving generated-output checks. |
| Merge | `m3_05_pilot_results.py`, `recovered_claim_audit.py`, `recovered_claims.py`, `recovered_mismatch_triage.py` | Their outputs are still imported by report/claim modules or cited as evidence. Fold the M3-05 verdict output into the single verdict function; keep recovered artifacts as provenance only. |
| Archive | `m2_m3_evidence_inventory.py`, `m2_m3_reconstruction_finalize.py`, `m2_m3_reconstruction.py`, `m3_05_pilot_runtime.py`, `m3_05_pilot.py`, `m5_05_diagnosis_postprocess.py`, `m5_05_diagnosis.py`, `m5_05_dvc_status.py`, `profile_m0_baseline.py`, `reconstruct_all.py`, `reconstruction.py` | One-off recovery, migration, pilot, and historical run procedures remain provenance, not the verdict engine. `m2_m3_reconstruction_finalize.py` must first lose its decision-status write. Its tests stay available until replacement checks are reviewed. |
| Delete | None proposed | No source is approved for destruction by this plan. |

## Proposed `just` recipe disposition

| Disposition | Current recipes | Reason |
|---|---|---|
| Keep | `action-refs-check`, `analysis-sync`, `analysis-test`, `check`, `check-autocrlf`, `check-quarto`, `check-tools`, `ci`, `ci-analysis`, `ci-clean`, `ci-linux`, `ci-rust`, `clippy-cross`, `commit-messages-check`, `data-integrity-check`, `data-integrity-range-check`, `data-integrity-update`, `decisions`, `decisions-check`, `dvc-freeze-check`, `gitleaks-range-check`, `hooks-install`, `large-file-range-check`, `links`, `lint-prose`, `provenance SUMMARY`, `provenance-finalize SUMMARY`, `ship`, `tool-versions-check`, `check-local-data` | Cross-cutting validation, provenance, integrity, and delivery operations remain. |
| Conditional | `dvc-push REMOTE` | Do not retain as a public upload path until `M5-RECOVERED-REMOTE-PRIVACY` is final and the destination is safe for every listed artifact. |
| Merge | `claims`, `claims-check`, `claims-locate KEY`, `ledger-transition-check`, `generated-artifacts-check`, `report-m5-01`, `report-m5-01-check`, `report-m5-04`, `report-m5-04-check`, `report-m5-05`, `report-m5-05-check` | Replace parallel verdict/report paths with registry validation, verdict calculation, and one registered report target; preserve byte-check coverage for cited generated files. |
| Archive | `m3-05-pilot-classify`, `m3-05-pilot-classify-check`, `m3-05-pilot-report`, `m3-05-pilot-report-check`, `m3-05-pilot-runtime`, `m3-05-pilot-runtime-check`, `m5-05-deterministic`, `m5-05-dvc-status`, `m5-05-dvc-status-check`, `m5-05-plan`, `m5-05-timing ENTRY`, `reconstruct-all *ARGS`, `reconstruction-inventory`, `reconstruction-inventory-check`, `recovered-claim-audit`, `recovered-claim-audit-check`, `recovered-claims`, `recovered-claims-check` | Preserve costly DVC and one-off historical procedures outside the routine gate graph. |
| Delete | None proposed | A later review can remove a recipe only after its outputs and provenance are retained. |

## Heuristic checks and deadlines

These are existing heuristics, not approved verdict rules. Keep their current outputs for provenance; do not let them assign conclusion verdicts. Review or replace them by **M5-06**, before their related migration is treated as complete. This deadline applies to existing checks only; the contract's declaration/red-test/reachability rules apply immediately to every new check.

| Heuristic | What it infers today | Deadline / disposition |
|---|---|---|
| `claims.py` document scanner | Numeric claim boundaries, units, tiers, and excluded input-value patterns from Markdown text | M5-06; keep only the reference locator or replace with explicit source annotations. |
| `recovered_claims.py` | Value/context fields from recovered text and table rows | M5-06; archive after its historical evidence links are preserved. |
| `recovered_mismatch_triage.py` | Rounding, units, aggregates, and output-match cause from recovered records | M5-06; archive; no verdict authority. |
| `m2_m3_evidence_inventory.py` | Experiment groups, protocol dimensions, evidence class, and cell counts from headings, text, tables, and patterns | M5-06; archive its result and keep only reviewed explicit experiment specs. |
| `sources.py` | Harness citations and legacy source fallback from report links and text | M5-06; keep explicit `sources.toml` mappings, remove text fallback. |
| `generated_artifacts.py` | Generated-file ownership and reachable check targets from file markers, imports, and `just` text | M5-06; make the generated-artifact manifest authoritative and keep its consistency check. |
| `dvc_path_guard.py` | DVC-output reads by scanning analysis sources/tests | M5-06; keep the red test, replace source-text heuristics with an explicit file-access boundary if feasible. |
| `dvc_freeze_guard.py` | Timing-stage classification from stage names/commands | M5-06; the explicit `meta.kind = "timing"` field is the intended declaration, but the fallback name/command classifier remains an exception until removed. |
| `data_integrity.py` | One archived `STATUS.md` path is hard-coded in a checker | M5-06; replace with the generated-artifact/data manifest entry. |

The deadline is for a reviewed disposition, not permission to weaken or remove the associated check early.

## External review reconciliation

The following records reconcile every item in the external review of `678e61b`. “Adopted” means the draft now incorporates the requested correction; “partial” marks a deliberate scope limit; “not adopted” records why no edit is appropriate. E-rows are the review's evidence-assessment rows grouped one-for-one with its findings and acceptable/too-strict judgments. V-rows enumerate every vague term from its clarity section.

| Review item | Disposition | Reconciliation |
|---|---|---|
| D1 | Adopted | The contract computes per `(conclusion, role)`, specifies accepted kind/type pairs, same-rank ties, cell aggregation, invalid/missing cases, host/toolchain handling, hashes, absolute-time/null cases, and graph validation for supersession. |
| D2 | Adopted | The closed verdict set includes `needs-decision` and the mapping table covers M5-05, M3-05, and criteria labels. |
| D3 | Adopted | Report and head roles are independent; recovered output is provenance only; fixed ranks are applied only within role and over the full active record set. |
| D4 | Adopted | `experiment` is one stable ID resolving to one protocol record; entries that combined unrelated outcomes have been split. |
| D5 | Adopted | The migration plan now states that the old M5-05 registry is historical, carries forward both fields, and requires a dated Later update before retiring consumers. The existing file remains untouched. |
| D6 | Adopted | Registry revisions are full SHAs only; role belongs in evidence. The dated correction in `REPRODUCTION_CRITERIA.md` identifies `681bf29` as the follow-up and `c857d51` as the separate HEAD revision. |
| C1 | Adopted | Statements in the registry describe original report findings; `verbatim` is checked against the cited contract/report text. M2-40 absolute-time wording no longer says “practical.” |
| C2 | Adopted | M0/M1 entries cite the M1 contract's validation matrix; CRN efficiency states the 1.5–80× range; cause findings are separated by case and seed; M3 indexing retains the attribution caveat. |
| C3 | Adopted | Scale completion and its first-break criteria are distinct; M3-11 Phase 9c large-frontier, whole-tick, and crossover outcomes have distinct experiment IDs; migration parity is split into four predicates; M0 replay, initialization, snapshot continuation, and M1 snapshot-byte predicates are distinct even where test infrastructure is shared. |
| C4 | Adopted | Numeric M4 divergence, CRN, local aggregate, and minimum-cause outputs are deterministic; CoW share remains timing. |
| C5 | Adopted | The registry contains the listed M2 Rayon/phase-share, M3 lookup/MAD/serial-baseline, M4 gate/method/newborn-identity findings, plus separate first-break and high-alpha reliability findings. Wiring debt and compaction money loss remain limitations. |
| C6 | Adopted | Calibrated-default and GPU judgments are in `limitations`, not in the falsifiable conclusion list. |
| C7 | Adopted | The M4 anchors resolve to `#correctness-and-completion-status`, `#h365-extension` / `#component-flags-and-alive-population`, and `#implementation-and-forward-gate`. |
| E1 — five HEAD-only M3 timings | Adopted | Phase 6B, search reduction, reversed slots, full-rebuild equal-live, and Phase 9c keep `partial`/limitations coverage; their historical report-role result is explicitly inconclusive until report-revision data exists. |
| E2 — M2 high-population range | Adopted | It is `needs-decision` for verdict purposes because an absolute-time range has no frozen relative pass criterion; M5-05's Rayon result near 16.66–16.67 ms does not turn it into a negative conclusion. |
| E3 — M3 frontier crossover | Adopted | The statement is only the first observed grid points; all three M5-05 cells favor parallel, so no changed ordering is recorded. Historical crossover remains limited by concurrent Cargo work. |
| E4 — M4 record divergence | Adopted | Evidence points to the H=365 `m4_03_a1_experiments.csv` and its M5-04 byte-exact gzip rerun; the unrelated A-5 gate log is removed. |
| E5 — M4 minimal cause | Adopted | Case (a) is the exact single-predicate minimum/non-uniqueness result. Seed variation is a separate partial-evidence entry; its A-5 gate log is not causal evidence, and the helper remains unavailable. |
| E6 — M4 CoW share | Adopted | Reclassified as timing and downgraded from sufficient to partial/limitations because it is a supplemental three-call measurement, not A-5. |
| E7 — M3 static scale | Adopted | One-million completion remains a separate report-revision result. The pilot's five changed phase first-break results and seven changed threshold-side results stay in the cited verdict-review records; setup, aggregate alpha, per-agent, and memory criteria are separate entries, and within-noise results are not called pass/fail changes. |
| E8 — M3 propagation whole tick | Adopted | The 2.26×/2.29× statement names only the measured large-frontier workload; it does not claim support for smaller-frontier whole-tick behavior. The Phase 9c frontier observation remains a separate experiment. |
| E9 — M2 workload B too strict | Adopted | The narrow 50k→100k B claim remains sufficient: M5-05 remeasured serial and Rayon cells (about 1.11× and 1.10×). Workload A's 50k cell remains absent. |
| E10 — deterministic entries | Adopted | Deterministic test/golden entries without a recorded run at the cited tag are labeled rerunnable, not verified; only recorded exact comparisons receive sufficient coverage. |
| E11 — M3-09 equal-live | Adopted | The protocol-matched result is tied to full revision `681bf296ea07d3ddc4391f5f1dfd49255876300c`; the all-days M5-05 cell remains a different protocol. |
| E12 — M4 CRN/local effects | Adopted | M4-04 CRN efficiency and local aggregate conclusions use deterministic types and cite the M5-04 exact regeneration. |
| T1 | Adopted | Imported `sources`, `m5_05`, and recovered-claim modules are Merge; `m3_05_pilot_results` is Merge into the verdict path. The 12 test-bearing archived modules remain tested until replacement coverage is reviewed; none of their test targets is removed here. |
| T2 | Adopted | The frozen-decision table now includes M5-05-D-2, M5-00-A-3, M5-05-A1-2 and the other cited consumers; amend before archiving those consumers. |
| T3 | Adopted | M3-05 pilot results and all generated files cited as evidence retain their generators/checks; `check-local-data` remains. |
| T4 | Adopted | The contract's new-check rule is immediate; existing heuristic exceptions and M5-06 deadlines now include timing-stage inference, generated ownership, prose extraction, DVC path scanning, and hard-coded STATUS path. |
| T5 | Adopted | Proposed explicit gates are listed; no nonexistent gate manifest is claimed. Finalize's status write must be removed before retirement, and public DVC push is conditional on the privacy decision. |
| V1 — “dozens” | Adopted | Removed; registry size is the exact count of `[[conclusion]]` records, not an approximate target category. |
| V2 — “scoped” | Adopted | Replaced by the explicit scope tuple: profile, configuration, ordered cell IDs, horizon, output/predicate, and role. |
| V3 — “falsifiable” | Adopted | Replaced by a testable predicate definition with a result for every declared cell. |
| V4 — “bounded qualitative statement” | Adopted | A qualitative record requires the exact registered predicate evaluated on hashed evidence; prose alone is not accepted. |
| V5 — “supported” | Adopted | Removed from predicates; evidence coverage is stated by exact cells and artifacts. |
| V6 — “registered” | Adopted | Defined as a validated entry in the cited TOML/JSONL registry. |
| V7 — “applicable revision” | Adopted | Removed; report/head role plus a full SHA identifies the question. |
| V8 — `report_revision` | Adopted | Legacy value maps only to role `report`; current registries use full SHAs. |
| V9 — `HEAD` | Adopted | Means the full SHA recorded in the `head` evidence record, not a moving branch name. |
| V10 — “contradicts” | Adopted | Defined as a valid measurement failing the frozen experiment predicate. |
| V11 — “equal or higher strength” | Adopted | Defined by the fixed rank table and compared only within conclusion and role. |
| V12 — “default strength” | Adopted | Removed; there are no implicit or overridable strength defaults. |
| V13 — toolchain format | Adopted | Defined as a JSON object with exact compiler, target, lock hashes, host/CPU, and external tool versions; irrelevant fields are explicit null. |
| V14 — protocol ID | Adopted | Defined as the `experiment` key resolving to one protocol record. |
| V15 — coverage `sufficient` / `partial` | Adopted | Defined as all declared cells covered versus some covered and some not; coverage is editorial metadata, not a verdict. |
| V16 — “smallest set” | Adopted | Minimum cause means minimum cardinality; ties use M4-05's lexicographic sorted-ID rule. |
| V17 — “gate manifest” | Not adopted | No gate-manifest file exists or is needed for this draft; the migration plan now lists explicit proposed `just` targets instead. |
| V18 — “red test” | Adopted | Defined as a test that supplies deliberate invalid input and asserts failure. |
| V19 — “named gate target” | Adopted | Specified as a named `just` gate target reachable in the gate graph. |
| V20 — “prose heuristics” | Adopted | Defined as regex/text-position/wording guesses used to infer verdicts, evidence classes, or generated ownership. |
| V21 — “main-text claim” | Adopted | Defined to include body text, table/figure titles or captions, and the executive summary. |
| V22 — “partial coverage” | Adopted | Defined as at least one declared cell evidenced and at least one not evidenced; cannot support a complete summary. |
| V23 — “substantially” | Adopted | Removed from normalized registry statements; exact historical source wording remains only in `verbatim` where required. |
| V24 — “mildly superlinear” | Adopted | Not used in normalized claims; any timing predicate must instead state the exact alpha/cell boundary. |
| V25 — “many” | Adopted | Removed from normalized conclusions; a count or explicit ratio is required. |
| V26 — “practical” | Adopted | Removed from the normalized M2 absolute-time statement; its original occurrence is retained only in the exact `verbatim` quote. |
| V27 — “selected cells” | Adopted | Replaced in active registry criteria by declared cell IDs; each protocol record pins the source section and raw/table artifacts that enumerate them. |
