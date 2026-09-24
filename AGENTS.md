# AGENTS.md

These rules apply to all agents working in this repository.

## 1. Task scope is a hard boundary

Do exactly the requested task and no more.

- Do not start the next milestone unless explicitly instructed.
- Do not infer permission from `Next Step`, `Next Milestone`, roadmap entries, TODOs, design documents, or obvious follow-up work.
- Do not proactively implement, scaffold, refactor, create files, add dependencies, run benchmarks, or change architecture unless the current task explicitly requires it.
- Prefer stopping after the requested deliverable over continuing helpfully.

## 2. Respect task mode

Interpret task wording strictly.

- **Review / audit / inspect / analyze** → read and report only. Do not modify files.
- **Edit / revise / fix** → modify only the explicitly requested files and scope.
- **Implement** → code only the explicitly authorized implementation scope.
- **Validate / test** → perform validation only. Do not fix unrelated findings unless instructed.

Never convert:

- a review task into an edit task;
- an edit task into an implementation task;
- a validation task into a repair task.

## 3. Make minimal changes

- Preserve existing architecture, terminology, file layout, and accepted decisions unless explicitly asked to change them.
- Prefer targeted edits over whole-file rewrites.
- Do not introduce speculative abstractions or future-facing infrastructure.
- Do not solve deferred problems early.
- Do not broaden the current scope because a future requirement appears obvious.
- Do not modify unrelated files merely to make the requested change easier.

## 4. Versioning and Git

Unless explicitly requested:

- Do not create commits.
- Do not create tags.
- Do not bump project versions.
- Do not invent document versions, revision numbers, or release labels.
- Do not rename files, milestones, branches, or architectural stages.

## 5. SimulaCiv development stages

Semantics and contracts come before optimization.

- **M0** — Reference Model
- **M1** — Contract Freeze
- **M2** — Optimized Runtime

Rules:

- M0 exists to validate semantics.
- M1 freezes contracts after M0 validation.
- M2 implements the frozen contracts with performance optimizations.
- Do not introduce M2 implementation details into M0 unless explicitly required.
- Keep semantic contracts separate from performance and storage optimizations.
- A documented next milestone is not authorization to begin it.

## 6. Do not silently resolve open decisions

If the task exposes an unresolved architectural or semantic decision:

- identify it clearly;
- state whether it blocks the current task;
- do not make a permanent decision unless explicitly authorized.

When uncertainty affects correctness, stop at the current task boundary instead of guessing forward.

## 7. Test integrity is part of the specification

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

## 8. Validation must use real production paths

When validating an implementation:

- Exercise the same production code paths used by normal execution.
- Do not introduce test-only shortcuts into production logic.
- Do not bypass invariants, resolvers, canonicalization, persistence, or state-transition logic.
- Differential tests must compare independently produced logical results, not shared cached values or copied outputs.
- M2 validation must compare against the M0 reference oracle without modifying M0 to accommodate M2.

A passing test is not sufficient if the implementation violates the documented contract.

## 9. Verify before reporting completion

Before claiming completion:

- reread the modified or reviewed area;
- verify that all explicit constraints were followed;
- verify that no work outside the requested scope was performed;
- review the final diff or equivalent changed-file list;
- verify whether any test, fixture, golden, snapshot, hash, CI, or configuration file changed;
- report only actions actually performed;
- never claim tests, commands, files, commits, benchmarks, or results that did not occur.

If tests fail, report the failure accurately. Do not hide or work around it unless explicitly instructed to investigate or fix it.

## 10. Reporting

Keep completion reports concise.

Report only:

1. what was inspected or changed;
2. validation performed and exact commands executed;
3. all modified files;
4. whether any test, fixture, golden, snapshot, canonical hash, or CI file changed;
5. remaining blockers, failures, or open questions;
6. nothing beyond the requested scope.

## 11. Stop condition

After completing the explicitly requested deliverable, stop.

Do not begin follow-up work even if the next step appears obvious.

Do not fix newly discovered unrelated issues unless explicitly instructed.

Wait for explicit user instruction before proceeding.