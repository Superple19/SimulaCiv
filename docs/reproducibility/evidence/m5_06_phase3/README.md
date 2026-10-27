# M5-06 Phase 3 report-revision rerun logs

Logs are stdout/stderr from the registered test targets and deterministic examples, run in detached worktrees at each conclusion's pinned report revision with the exact registered harness overlays. Rust toolchain: 1.98.1; no A-5 timing samples were run.

- `<report-sha>-<test-target>.log`: `cargo +1.98.1 test --locked -p sim-model --test <test-target>` output.
- `m4_03-example.log`: M4-03 deterministic example summary output; full CSV is DVC-tracked under the phase3 raw bundle.
- `m4_04-example.log`: M4-04 deterministic example progress/output at report revision 739004c; full CSV is DVC-tracked under the phase3 raw bundle.
- `m4_04-example-477bbc9.log`: M4-04 deterministic example progress/output at report revision 477bbc9; its full CSV outputs are byte-identical to the 739004c run.
- `m4_05-example.log`: M4-05 deterministic example output; evaluation JSONL is DVC-tracked under the phase3 raw bundle.
- `m4_06-example.log`: M4-06 deterministic example output; full JSON is DVC-tracked under the phase3 raw bundle.
- `477bbc9bb716a34a6942b4bd7032b2d8ad46a791-m4_04_crn_efficiency_tests.log`: M4-04 integration test rerun at the follow-up report revision.

Two overlay compile failures are retained as evidence; their affected cells are marked `needs-decision` in `contracts/evidence.jsonl` with the reason.
