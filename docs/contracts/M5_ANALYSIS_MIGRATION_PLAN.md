# M5 Analysis Migration Plan

Status: **APPROVED PLAN — MIGRATION IN PROGRESS (2026-10-27)**
Parent: [M5 Analysis Contract](M5_ANALYSIS_CONTRACT.md)

This plan redoes the earlier file triage against the current import/call graph and frozen decision references. It authorizes no file move, rename, deletion, generator edit, or decision-status change. All migration steps, including T3, remain planned work; this document does not claim T3 is implemented.

Later update (2026-10-27): the current authorized migration changes routine-gate reachability and supersedes the reviewed frozen decisions; it does not rewrite or remove legacy sources or alter claim-ledger statuses or verdict records.

## Migration order and constraints

1. Implement and review the conclusion/evidence model in the parent contract before changing verdict ownership.
2. Replace M5-05's per-run registry with `contracts/conclusions.toml`; append evidence records and compute verdicts without rewriting historical evidence. `docs/reproducibility/m5_05_conclusions.toml` remains a historical source: carry forward each `verbatim_conclusion` and `pass_criterion`, then mark it superseded with a dated Later update note before its consumers are retired. The M5-05 report remains evidence until replacement outputs render and check byte-identically where deterministic.
3. Preserve generated outputs that are cited as evidence and keep their check targets reachable: `claims.csv.gz` as a reference index, the M5-01 inventory, M5-04 and M5-05 reports, generated figures, `docs/data/m5_05/STATUS.md`, and any summary cited by a conclusion. A generated file is not removable merely because its generator is proposed for merge/archive.
4. Only after each replacement is checked should old one-off recovery and inventory commands leave required gates. Preserve their sources and results in an archive with links.
5. Change `m2_m3_reconstruction_finalize.py` to stop writing `contracts/decisions.toml`. It may emit a proposed review artifact, but humans alone change decision status; do not archive the current writer until that behavior is replaced and reviewed.

Later update (2026-10-26): the draft `contracts/conclusions.toml` will supersede `docs/reproducibility/m5_05_conclusions.toml` on M5-00 freeze. Until then, the old registry remains the historical source; all 13 legacy `verbatim_conclusion` and `pass_criterion` values are preserved in the draft's active or `prior_*` fields.

### Frozen decisions to amend before a migration changes their behavior

| Decision | Why it must be reviewed first |
|---|---|
| `M5-00-D-3`, `M5-00-D-8` | They make every numeric claim, rather than every conclusion, the acceptance unit. Amend to keep the old ledger as a locator only. |
| `M5-00-A-3` | It freezes the data/claim workflow consumed by the old M5-05 run registry. Amend its source reference before archiving that consumer. |
| `M5-02-D-4` | It reserves `analysis/legacy` for inactive tools. Amend to name the archive destination and state that archiving removes gate ownership, not provenance. |
| `M5-02-D-6`, `M5-03-D-6` | They freeze the prose-extracted claim schema and ledger content. Amend before changing extraction or verdict meaning. |
| `M5-03-D-4` | It freezes the current `just` analysis interface. Amend before merging or retiring the per-report targets. |
| `M5-04-D-6` | It defines the old per-claim M4 status set. Replace its verdict wording before retiring that path. |
| `M5-05-D-2`, `M5-05-D-3`, `M5-05-A1-1`, `M5-05-A1-2`, `M5-05-A1-4` | They freeze predeclared M5-05 A-5 comparisons, `m5_05_conclusions.toml`, and its 13-run schedule. Replace the source registry and stage accounting before archiving their consumers. |
| `M5-05-M3-09-SCOPE-WITHDRAWAL`, `M5-05-M3-09-PROTOCOL-RECHECK` | They bind the M3-09 scope result to the old registry file. Preserve the result, then amend the reference to the new conclusion/evidence records. |
| `M5-05-D-6` | It requires cited harnesses, data, and generated M5-05 tables. Keep until the replacement report and its checks cover the same evidence. |

Decision IDs above are frozen in `contracts/decisions.toml`; update them through a separately reviewed decision change, not as a side effect of a generator.

## Gate implementation status

The Phase 1 gate now validates the conclusion/evidence registry and computes verdicts through a pure module. Remaining targets are still planned; each must be explicitly wired and receive a red test before its migration is complete. `analysis/simciv_analysis/conclusions.py` checks every Tier 1 record's ordered cells, exact predicate keys, numeric expected values where needed, registered host, HEAD harness paths, and verbatim text at the pinned report SHA then-path absent from its parent.

| Planned target | Function | Current state |
|---|---|---|
| `conclusions-check` | Validate conclusion/protocol schemas, verbatim anchors, evidence hashes and append-only history; check the generated verdict table. | Implemented in Phase 1 with red tests. |
| `evidence-check` | Separate evidence-only validation target for append-only records, content hashes, role/kind compatibility, and supersession graph. | Not separate; these checks run through `conclusions-check`. |
| `verdicts-check` | Compute each `(conclusion_id, role[, head SHA])` result order-independently from evidence and frozen criteria. | Implemented in `verdicts.py`; invoked by `conclusions-check`. |
| `report-m5-final-check` | Render conclusions and cited evidence from the registries and compare the committed report. | Not implemented. |

### Phase 2 gate migration record (2026-10-27)

`ledger-transition-check`, `reconstruction-inventory-check`, and `m3-05-pilot-report-check` are no longer dependencies of `just check`. The reconstruction inventory and M3-05 runtime artifacts use `m3-05-pilot-runtime-check` through `just check-local-data`; the cited M5-01, M5-04, and M5-05 report checks remain reachable until registry-backed replacements are reviewed. The underlying legacy sources and commands remain in the repository for provenance. No claim-ledger bytes/statuses or verdict rows were regenerated or changed.

## Import and gate graph

`analysis/simciv_analysis/__init__.py` exports no runtime API. The report entry points import shared I/O, table/figure, findings, provenance, and data-source helpers. M5-01 additionally imports ledger extraction and recovered-claim audit/triage; M5-04 imports claim overrides and source mapping; M5-05 imports its separate conclusion file and computes the historical-run report. The `just` recipes invoke these entry points directly. The intended convergence is one explicit registry/evidence/report path; shared byte-writing, hashing, tools, links, integrity, and delivery checks stay shared.

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
| Merge | `m3_05_pilot_results.py`, `m3_05_pilot_runtime.py`, `m5_05_dvc_status.py`, `recovered_claim_audit.py`, `recovered_claims.py`, `recovered_mismatch_triage.py` | Their outputs are still imported by report/claim modules or cited as evidence. Keep the cited M3-05 and M5-05 evidence checks; fold verdict output into the single verdict function and keep recovered artifacts as provenance. |
| Archive | `m2_m3_evidence_inventory.py`, `m2_m3_reconstruction_finalize.py`, `m2_m3_reconstruction.py`, `m3_05_pilot.py`, `m5_05_diagnosis_postprocess.py`, `m5_05_diagnosis.py`, `profile_m0_baseline.py`, `reconstruct_all.py`, `reconstruction.py` | One-off recovery, migration, pilot, and historical run procedures remain provenance, not the verdict engine. `m2_m3_reconstruction_finalize.py` must first lose its decision-status write. Its tests stay available until replacement checks are reviewed. |
| Delete | None proposed | No source is approved for destruction by this plan. |

## Proposed `just` recipe disposition

| Disposition | Current recipes | Reason |
|---|---|---|
| Keep | `action-refs-check`, `analysis-sync`, `analysis-test`, `check`, `check-autocrlf`, `check-quarto`, `check-tools`, `ci`, `ci-analysis`, `ci-clean`, `ci-linux`, `ci-rust`, `clippy-cross`, `commit-messages-check`, `data-integrity-check`, `data-integrity-range-check`, `data-integrity-update`, `decisions`, `decisions-check`, `dvc-freeze-check`, `gitleaks-range-check`, `hooks-install`, `large-file-range-check`, `links`, `lint-prose`, `m3-05-pilot-classify-check`, `m3-05-pilot-runtime-check`, `m5-05-dvc-status-check`, `recovered-claims-check`, `recovered-claim-audit-check`, `provenance SUMMARY`, `provenance-finalize SUMMARY`, `ship`, `tool-versions-check`, `check-local-data` | These report checks cover cited M3-05/M5-05 evidence and stay reachable; cross-cutting validation, local-data protection, and delivery checks also remain. |
| Conditional | `dvc-push REMOTE` | Do not retain as a public upload path until `M5-RECOVERED-REMOTE-PRIVACY` is final and the destination is safe for every listed artifact. |
| Merge | `claims`, `claims-check`, `claims-locate KEY`, `ledger-transition-check`, `generated-artifacts-check`, `report-m5-01`, `report-m5-01-check`, `report-m5-04`, `report-m5-04-check`, `report-m5-05`, `report-m5-05-check` | Replace parallel verdict/report paths with registry validation, verdict calculation, and one registered report target; preserve byte-check coverage for cited generated files. |
| Archive | `m3-05-pilot-classify`, `m3-05-pilot-report`, `m3-05-pilot-report-check`, `m3-05-pilot-runtime`, `m5-05-deterministic`, `m5-05-dvc-status`, `m5-05-plan`, `m5-05-timing ENTRY`, `reconstruct-all *ARGS`, `reconstruction-inventory`, `reconstruction-inventory-check`, `recovered-claim-audit`, `recovered-claims` | Preserve non-check procedures outside the routine gate graph; the checks invoked by `check-local-data` remain Keep. |
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

## Follow-up list

- **M5-07:** design a whole-runtime M2-versus-M0 experiment. The current M2-01-to-M2-12 allocation-phase comparison cannot represent that question as one report-role result. This follow-up is not yet registered as a conclusion.
