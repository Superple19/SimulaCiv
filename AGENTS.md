# AGENTS.md

These rules apply to all agents working in this repository.

## 1. Scope

Do exactly the requested task and no more. Interpret task wording strictly:

- **Review / audit / inspect / analyze** → read and report only; do not modify files.
- **Edit / revise / fix** → modify only the explicitly requested files and scope.
- **Implement** → code only the explicitly authorized implementation scope.
- **Validate / test** → perform validation only; do not fix unrelated findings unless instructed.
- Never convert a review into an edit, an edit into an implementation, or validation into a repair.
- Do not start the next milestone unless explicitly instructed. `Next Step`, `Next Milestone`, roadmap entries, TODOs, design documents, and obvious follow-ups do not authorize it.
- Do not proactively implement, scaffold, refactor, create files, add dependencies, run benchmarks, or change architecture unless the task explicitly requires it.
- Preserve existing architecture, terminology, file layout, and accepted decisions. Prefer targeted edits; do not add speculative abstractions, solve deferred problems, broaden scope, or modify unrelated files for convenience.
- Stop after the requested deliverable. Do not begin follow-up work or fix unrelated findings; wait for explicit instruction.

## 2. Commands

From the repository root:

- `just hooks-install` — install the repository Git hooks before committing.
- `mise install` — install the pinned repository tools from `mise.toml`.
- `just check-tools` — verify that the active tools match the repository pins.
- `just ci` — run the local CI checks.
- `just ci-linux` — run the CI Rust and analysis targets in the WSL Linux checkout.
- `just ship` — run the standard delivery path to `ci-check`.
- `just data-integrity-update` — register checksums for newly produced data files.
- `just decisions` / `just decisions-check` — regenerate or verify registry tables.
- `just report-m5-01`, `just report-m5-04`, `just report-m5-05` — regenerate the corresponding reports; matching `*-check` targets verify committed output.

## 3. Versioning and Git

Unless explicitly requested:

- Do not create commits.
- Do not create tags.
- Do not bump project versions.
- Do not invent document versions, revision numbers, or release labels.
- Do not rename files, milestones, branches, or architectural stages.
- Never amend, rebase, reset, or force-push a commit that exists on `origin`; correct mistakes with a new commit.
- Track new experiment raw outputs through DVC stages; keep summaries, specs, harnesses, reports, generated figures, and DVC pointers in Git. Never commit DVC credentials.
- Install the repository Git hooks with `just hooks-install` before committing; never bypass them with `--no-verify`.
- Write a UTF-8/LF commit-message file with `write_text_lf` or an editor and use `git commit -F <file>` only; never use `-m`. The `allowlist-commit-message` hook requires a nonempty contiguous bullet body and enforces message syntax.
- When writing repository files from PowerShell, use PowerShell 7 or `-Encoding utf8NoBOM`; never rely on Windows PowerShell 5.1 `Out-File` or `Set-Content` defaults.
- Normal delivery is `just ship`, which pushes to `ci-check`; pre-push hooks run local checks, and the same successful CI run promotes the commit to `main` by fast-forward only. Never push directly to `main`.

## 4. SimulaCiv development stages

Semantics and contracts come before optimization.

- **M0 — Reference Model:** complete, `v0.1.0-m0`.
- **M1 — Contract Freeze:** complete, `v0.2.0-m1`.
- **M2 — Optimized Runtime:** complete, `v0.3.0-m2`.
- **M3 — Interaction Complexity:** complete, `v0.4.0-m3`.
- **M4 — Counterfactual Research:** complete, `v0.5.0-m4`.
- **M5 — Reproducibility and Research Hardening:** in progress; stage status is in [contracts/decisions.toml](contracts/decisions.toml).

Rules:

- M0 exists to validate semantics.
- M1 freezes contracts after M0 validation.
- M2 implements the frozen contracts with performance optimizations.
- Do not introduce M2 implementation details into M0 unless explicitly required.
- Keep semantic contracts separate from performance and storage optimizations.
- A documented next milestone is not authorization to begin it.

## 5. Do not silently resolve open decisions

If the task exposes an unresolved architectural or semantic decision:

- identify it clearly;
- state whether it blocks the current task;
- do not make a permanent decision unless explicitly authorized.

When uncertainty affects correctness, stop at the current task boundary instead of guessing forward.

## 6. Test integrity is part of the specification

Tests, fixtures, golden outputs, canonical hashes, snapshots, and CI validation rules are part of the project contract.

Unless explicitly instructed:

- Do not modify, delete, weaken, skip, disable, replace, or bypass existing tests.
- Do not change expected values merely to make an implementation pass.
- Do not regenerate or overwrite golden files, fixtures, snapshots, or canonical hashes.
- Do not widen numerical tolerances or weaken assertions.
- Do not mark failing tests as ignored, skipped, expected-to-fail, or conditionally disabled.
- Do not remove test cases or reduce validation coverage.
- Do not modify CI configuration, test configuration, feature flags, build profiles, or environment settings to hide failures.
- Do not add production-code branches that detect tests or special-case known test inputs.
- Do not mock, stub, hard-code, or bypass production behavior solely to satisfy tests.
- Do not catch, suppress, downgrade, or discard errors that should cause a test failure.
- Do not replace a failing correctness test with a weaker test.
- Do not alter the reference oracle or canonical expected output to match a new implementation.

If an existing test appears incorrect or conflicts with the specification:

1. stop changing that test;
2. report the exact conflict;
3. identify the relevant contract or specification;
4. wait for explicit authorization before modifying the test.

Tests are not implementation suggestions. They are external acceptance constraints.

## 7. Validation must use real production paths

When validating an implementation:

- Exercise the same production code paths used by normal execution.
- Do not introduce test-only shortcuts into production logic.
- Do not bypass invariants, resolvers, canonicalization, persistence, or state-transition logic.
- Differential tests must compare independently produced logical results, not shared cached values or copied outputs.
- M2 validation must compare against the M0 reference oracle without modifying M0 to accommodate M2.

A passing test is not sufficient if the implementation violates the documented contract.

## 8. Verify before reporting completion

Before claiming completion:

- reread the modified or reviewed area;
- verify that all explicit constraints were followed;
- verify that no work outside the requested scope was performed;
- review the final diff or equivalent changed-file list;
- verify whether any test, fixture, golden, snapshot, hash, CI, or configuration file changed;
- report only actions actually performed;
- never claim tests, commands, files, commits, benchmarks, or results that did not occur.

If tests fail, report the failure accurately. Do not hide or work around it unless explicitly instructed to investigate or fix it.

## 9. Reporting

Keep completion reports concise.

Report only:

1. what was inspected or changed;
2. validation performed and exact commands executed;
3. all modified files;
4. whether any test, fixture, golden, snapshot, canonical hash, or CI file changed;
5. remaining blockers, failures, or open questions;
6. nothing beyond the requested scope.

## 10. Standing research rules

- Commit each experiment or measurement harness under `crates/sim-model/examples/` or `crates/sim-model/benches/` and keep it after measuring; temporary library instrumentation may be removed, but its driver stays committed.
- Track full raw outputs under `docs/data/<stage>/` through DVC; keep specs, summaries (at most 1 MiB), pointers, and reports in Git. `prek:dvc-large-files` enforces the Git size limit. Cite the harness and summary/raw data entry for every report table; existing Git data stays in place.
- Do not garbage-collect the local DVC cache until `dvc push --all-commits` has succeeded; DVC uploads remain deferred unless explicitly requested.
- `.dvc/config` may contain the shared remote URL; keep credentials only in ignored `.dvc/config.local`, which `prek:forbid-dvc-local-config` blocks.
- Choose thresholds and parameters from a measured effect distribution; record the choices before running the experiment.
- Preserve historical documents, including frozen contracts and past audits. Add a dated **Later update** note instead of rewriting their historical wording.

## 11. Documentation rules

- Generate report numbers, tables, and figures from committed code and data. Do not hand-type measured or computed values into prose; cite the generated table or figure.
- Keep experiment conditions in spec files and link to them from documentation instead of restating them.
- Record new decisions in [contracts/decisions.toml](contracts/decisions.toml), one row per decision; preserve historical contract registers.
- Put process history (attempts, reruns, and rule changes) in commit messages, not report bodies. Reports show the current result and link to superseded material.
- Use this template for new reports: Status; Question; Method (links); Results (generated); Findings (at most five bullets, each citing a generated item); Limits (at most five bullets). Target one page of prose.
- Do not duplicate content across the README, audits, and contracts; link to the single source.
- Preserve existing historical reports. M5-08 may add summary-first headers.
- Follow `.editorconfig` and `.gitattributes`: text is UTF-8 without BOM and LF except `.bat`/`.cmd` CRLF, while data bytes are exact. `prek:forbid-utf8-bom`, `prek:trailing-whitespace`, `prek:end-of-file-fixer`, `prek:mixed-line-ending`, and `prek:data-integrity` enforce this; Python analysis writes go through `simciv_analysis.io.write_text_lf`.

### Findings integrity

Generated Findings require passing named predicates, inline Quarto values, `[pre-registered: <id>]` or `[exploratory]` tags, and a predicate appendix. Report tests, `just check`, `just lint-prose`, and `prek:vale-changed-docs` enforce these and Vale's overclaim exception rule. Keep the style under `analysis/vale/`, invoke it with `--config analysis/vale/.vale.ini`, and use the pinned version in CI.
