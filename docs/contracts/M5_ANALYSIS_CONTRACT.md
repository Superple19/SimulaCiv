# M5 Analysis Contract

Status: **DRAFT — NOT FROZEN**  
Baseline: `v0.5.0-m4`

Research question: **Can the project's reported research conclusions be reproduced from explicit, versioned evidence, and which conclusions survive the registered rechecks?**

The simulation used an M0 reference, froze semantics in M1, and optimized only after parity was established. M5's analysis layer grew without the same contract-first discipline: report prose was parsed into a large claim ledger and multiple generators assigned its statuses. The resulting repeated status regressions show that this design needs a smaller, explicit unit and a single verdict path.

## 1. Unit of reproducibility

The unit is a **conclusion and the experiment supporting it**, not each number extracted from a report. A conclusion is a scoped, falsifiable statement with a registered type, source, experiment, and applicable revision. A table row, estimate, or parameter remains evidence attached to that conclusion; it does not carry an independent verdict.

The existing claims ledger remains a reference index for locating historical numeric text. It has no verdict semantics and cannot override a registered conclusion. Existing report prose and frozen M0–M4 contracts remain historical sources; they are not rewritten to match a new result.

## 2. Conclusion registry

`contracts/conclusions.toml` is hand-written and reviewed. It contains dozens of conclusion records, not a scraped inventory. Required fields are:

| Field | Meaning |
|---|---|
| `id` | Stable, unique conclusion identifier. |
| `milestone` | M0 through M4. |
| `statement` | One sentence with explicit scope. |
| `type` | `deterministic`, `timing`, or `qualitative`. |
| `source` | Report path and section anchor. |
| `experiment` | Stable experiment or test-protocol identifier. |
| `revisions` | Exact revision(s) to which the conclusion applies; aliases such as `report_revision` must resolve to a commit before evidence is recorded. |

Optional `claim_keys` link to the historical index only; they do not supply verdicts. The draft registry also records the evidence already available, what is missing, an evidence-coverage assessment, and whether the conclusion belongs in the M5-09 paper's main results or limitations.

## 3. Evidence model

`contracts/evidence.jsonl` is the proposed append-only evidence log. Each record identifies a conclusion and contains an evidence id, kind, revision, toolchain, protocol id, date, artifact paths with SHA-256 content hashes, and optional `supersedes` ids. Artifact paths may point to Git summaries or DVC outputs; a path without a verified content hash is not a complete artifact reference. Evidence records are never edited or deleted. Corrections and reruns append a new record that supersedes the earlier record while retaining it for audit.

Evidence kinds and their default strength, strongest first:

| Rank | Kind | Use |
|---:|---|---|
| 6 | `deterministic_exact` | Rerun at the registered revision reproduces canonical values or bytes exactly. |
| 5 | `a5_report_revision` | A-5 timing rerun at the report revision tests historical reproduction. |
| 4 | `a5_head` | A-5 rerun at HEAD tests whether the conclusion still holds. |
| 3 | `structured_recovered_output` | Original structured output matches the registered context and content hash. |
| 2 | `qualitative_source` | A bounded qualitative statement is supported by cited source code, tests, or report evidence. |
| 1 | `historical_protocol` | Recorded evidence from a superseded or non-A-5 protocol; history only. |
| 0 | `none` | No supporting artifact. |

Strength ranks are defaults, not a license to compare unrelated outcomes. Deterministic evidence is exact; timing evidence is interpreted under the frozen reproduction criteria and its recorded scope; qualitative evidence cannot establish a numeric effect.

## 4. Verdicts and status ownership

One pure computation maps a registered conclusion, its append-only evidence records, and the frozen reproduction criteria to one verdict: `reproduced`, `reproduced (loose)`, `not reproduced`, `inconclusive`, or `unverified`. Deterministic conclusions require exact comparison. Timing conclusions use the registered A-5 protocol and ratio/ranking rules. Qualitative conclusions are `reproduced` only when their bounded statement is supported by the cited evidence; otherwise they are `inconclusive` or `unverified`.

A verdict can weaken only when new evidence of equal or higher strength contradicts the prior conclusion. Weaker evidence is retained but cannot override a stronger verdict. The computation is the sole owner of conclusion verdicts. No generator may write a verdict, a decision status in `contracts/decisions.toml`, or a frozen contract's status. Decision status remains a reviewed human decision, separate from evidence and computed verdicts.

## 5. What stays in gates

The required gates are the smallest set that validates the model and conclusion pipeline:

- conclusion registry schema, unique ids, report anchors, revisions, and evidence references;
- append-only evidence validation, content-hash verification, and the single verdict computation;
- report rendering from registered conclusions and evidence, with deterministic output checks;
- data immutability/integrity and tool/version checks;
- existing determinism, golden, snapshot, and parity tests for simulation behavior.

Recovery, token matching, mismatch triage, historical inventory reconstruction, and one-off migration tools are archived outside the gate graph. They remain available for provenance review but do not run from `just check`. The claims ledger is readable as a historical reference index only. This changes gate ownership; it does not authorize deletion of existing reports, data, harnesses, or history.

## 6. Gate-check requirements

Every check reachable from `just check`, `just check-local-data`, CI, or a pre-push hook must be declared in the gate manifest, have a red test that feeds it a deliberate invalid input, and be reachable from a named gate target. Check logic must consume explicit registry fields or manifests; it must not infer claim classes, evidence strength, report coverage, or generated-file ownership from prose heuristics. A failed or missing check is reported, not silently converted into a verdict.

## 7. M5-09 paper scope

The M5-09 report will state only conclusions present in `contracts/conclusions.toml` and will render their verdicts from validated evidence. Its scope is:

- M0/M1: deterministic reference behavior and the frozen semantic contract;
- M2: performance improvements under canonical parity, including observed tradeoffs and unresolved timing conclusions;
- M3: the scale and interaction workloads that exposed lookup, locality, dynamic-topology, and propagation behavior;
- M4: the tested limits of branching, divergence, CRN estimates, minimal-cause search, migration, and cross-branch execution.

Every main-text claim must cite its conclusion id and evidence records. Deterministic claims need exact artifacts at the applicable revision; timing claims need report-revision and HEAD evidence under A-5 or an explicit `inconclusive`/`unverified` verdict; qualitative claims need bounded statements and direct citations. Partial coverage, missing historical harnesses, single-host results, and unavailable DVC raw artifacts appear as limitations, not as implied confirmation. Unregistered numbers may appear only as source quotations needed to describe a protocol, never as independent findings.

## Appendix A. Current analysis surface: proposed disposition

Disposition is a migration proposal only. “Archive” means preserve outside the required gate graph; “merge” means replace the separate path with the named registry/evidence path; no move or deletion is authorized by this draft.

### Analysis modules

| Disposition | Current modules/files | Reason |
|---|---|---|
| Keep | `__init__.py`, `io.py`, `tables.py`, `figures.py`, `findings.py`, `links.py`, `provenance.py`, `tool_versions.py`, `check_tools.py`, `quarto_check.py`, `action_refs.py`, `git_hooks.py`, `data_integrity.py`, `dvc_push_guard.py`, `dvc_path_guard.py`, `dvc_freeze_guard.py`, `decisions.py`, `ci_analysis.py`, `ci_clean.py`, `ci_linux.py`, `ship.py` | Shared file, evidence-hash, decision, toolchain, and delivery infrastructure remains useful to the contract. |
| Merge | `claims.py`, `report_m5_01.py`, `report_m5_04.py`, `report_m5_05.py`, `generated_artifacts.py`, `generated_artifacts.toml`, `ledger_transition_guard.py` | Retain a non-verdict reference index and one conclusion/evidence report path; remove parallel status layers. |
| Archive | `sources.py`, `recovered_claims.py`, `recovered_mismatch_triage.py`, `recovered_claim_audit.py`, `m2_m3_evidence_inventory.py`, `m2_m3_reconstruction.py`, `m2_m3_reconstruction_finalize.py`, `reconstruction.py`, `reconstruct_all.py`, `profile_m0_baseline.py`, `m3_05_pilot.py`, `m3_05_pilot_results.py`, `m3_05_pilot_runtime.py`, `m5_05.py`, `m5_05_diagnosis.py`, `m5_05_diagnosis_postprocess.py`, `m5_05_dvc_status.py` | These recover, classify, inventory, or post-process historical experiments; preserve them for provenance without making them required checks. |
| Delete | None proposed | Preserve experiment history until a separate reviewed archival decision identifies genuinely redundant files. |

### `just` recipes

| Disposition | Current recipes | Reason |
|---|---|---|
| Keep | `action-refs-check`, `analysis-sync`, `analysis-test`, `check`, `check-autocrlf`, `check-quarto`, `check-tools`, `ci`, `ci-analysis`, `ci-clean`, `ci-linux`, `ci-rust`, `clippy-cross`, `commit-messages-check`, `data-integrity-check`, `data-integrity-range-check`, `data-integrity-update`, `decisions`, `decisions-check`, `dvc-freeze-check`, `dvc-push REMOTE`, `gitleaks-range-check`, `hooks-install`, `large-file-range-check`, `links`, `lint-prose`, `provenance SUMMARY`, `provenance-finalize SUMMARY`, `ship`, `tool-versions-check` | Keep reproducible CI, policy, toolchain, delivery, and artifact-integrity operations. |
| Merge | `claims`, `claims-check`, `claims-locate KEY`, `generated-artifacts-check`, `ledger-transition-check`, `report-m5-01`, `report-m5-01-check`, `report-m5-04`, `report-m5-04-check`, `report-m5-05`, `report-m5-05-check` | Fold index, transition, generated-output, and per-stage report recipes into the explicit conclusion/evidence validator and report target. |
| Archive | `check-local-data`, `m3-05-pilot-classify`, `m3-05-pilot-classify-check`, `m3-05-pilot-report`, `m3-05-pilot-report-check`, `m3-05-pilot-runtime`, `m3-05-pilot-runtime-check`, `m5-05-deterministic`, `m5-05-dvc-status`, `m5-05-dvc-status-check`, `m5-05-plan`, `m5-05-timing ENTRY`, `reconstruct-all *ARGS`, `reconstruction-inventory`, `reconstruction-inventory-check`, `recovered-claim-audit`, `recovered-claim-audit-check`, `recovered-claims`, `recovered-claims-check` | Preserve one-off reconstruction and local raw-data procedures without running them in required gates. |
| Delete | None proposed | No recipe deletion is justified before the archived workflows and evidence are reviewed. |
