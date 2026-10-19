# M4-02 Branching and Divergence Detection

**Status: implementation and correctness gates pass; §11.7 performance gates fail, so M4-02 is not complete.**

## Host and method

- Host: Windows, AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors.
- Noise controls: benchmark process priority `High`; processor affinity mask `0x555` (logical processor indices 0, 2, 4, 6, 8, 10, chosen as a physical-core-spread mask under Windows numbering; the logical-to-physical mapping was not independently verified); total CPU load averaged 25.1% before and 13.9% after. Free RAM was 60,432,472 KB before and 61,467,436 KB after.
- Build/run: a temporary, uncommitted `m4_02_measure` example was built with `cargo build --release -p sim-model --example m4_02_measure` and run from `target/release/examples`. The harness was removed after measurement; CSV output remains under ignored `target/`.
- Each cell used two warmups and 21 paired samples. Each sample compared two direct runs from the same snapshot against one `run_counterfactual_branch` call (base and branch), for 30 days per path, with metrics/events enabled and snapshots disabled. Sample order alternated direct-then-branch and branch-then-direct. M3 used the M3 fixture; M3P used the sustained M3P fixture; N was 10,000 or 100,000, K=N/200, k=8, and p_local=0.5.
- The paired timed paths both restored two states from the same snapshot. Restore/context setup was probed separately and treated as common cost. The restore probe includes graph extraction for the measurement object. Day-start cloning was measured for 60 clones per paired branch call. Branch daily digest cost was separately estimated from the two daily traces; direct digest cost was timed inside the two direct runs. These excluded costs are shown separately below.
- The complete measurement took **1,283.8 seconds (21m 23.8s)**. An earlier partial attempt started under 74.6% CPU load and was discarded; none of its samples are used here.

The report's gate calculation is:

```text
adjusted overhead = (branch pair median - direct pair median)
                  - day-start clone cost
                  - (branch daily digest cost - direct digest cost)
limit = max(5% of direct pair median, larger paired MAD)
```

Both paired paths perform two snapshot restores, so the separately measured restore/context setup cost is common and is not subtracted again. Direct and branch samples had MAD/median below 5% in each cell. Total CPU load was lower after the run than before it; intermediate host load was not sampled, so the MADs describe observed timing spread without attributing it to a specific process.

## Orchestration performance gate

Times are milliseconds per 30-day pair. Adjusted overhead excludes clone and differential digest costs as specified in §11.7.

| Profile / execution | N | Direct median ± MAD | Branch median ± MAD | Adjusted overhead | Limit | Result |
|---|---:|---:|---:|---:|---:|---|
| M3 serial | 10,000 | 172.968 ± 2.493 | 195.733 ± 4.261 | 29.411 | 8.648 | **FAIL** |
| M3 serial | 100,000 | 1,848.566 ± 23.858 | 2,386.782 ± 65.275 | 409.028 | 92.428 | **FAIL** |
| M3P serial | 10,000 | 624.509 ± 10.527 | 643.387 ± 9.911 | 20.407 | 31.225 | **PASS** |
| M3P incremental-topology | 10,000 | 250.696 ± 8.314 | 286.425 ± 10.469 | 42.008 | 12.535 | **FAIL** |
| M3P serial | 100,000 | 8,705.566 ± 264.917 | 9,241.216 ± 229.036 | 258.056 | 435.278 | **PASS** |
| M3P incremental-topology | 100,000 | 2,555.227 ± 8.555 | 3,153.967 ± 5.009 | 392.787 | 127.761 | **FAIL** |
| M3 + propagation serial | 100,000 | 3,648.666 ± 4.967 | 4,391.199 ± 10.617 | 360.643 | 182.433 | **FAIL** |

### Separately measured costs

All values are milliseconds per paired run. Restore/context setup is a probe through snapshot decode and context reconstruction; it also clones the graph to expose it to the measurement harness. Branch daily digest is an isolated estimate using the branch's per-day records and run-start state; direct digest is the median measured inside the direct runs.

| Profile / execution | N | Restore/context probe | Branch daily digests | Direct digests | Day-start clones |
|---|---:|---:|---:|---:|---:|
| M3 serial | 10,000 | 24.526 | 31.738 | 41.067 | 2.682 |
| M3 serial | 100,000 | 323.232 | 441.890 | 382.554 | 69.852 |
| M3P serial | 10,000 | 16.988 | 66.098 | 69.772 | 2.145 |
| M3P incremental-topology | 10,000 | 17.620 | 62.469 | 70.860 | 2.112 |
| M3P serial | 100,000 | 255.974 | 874.164 | 673.469 | 76.899 |
| M3P incremental-topology | 100,000 | 210.792 | 731.752 | 593.645 | 67.847 |
| M3 + propagation serial | 100,000 | 590.804 | 669.345 | 390.181 | 102.726 |

## Phase-recorder budget

For this separate one-day gate, the harness compared a no-op branch against a branch forcing `Idle` for `AgentId(0)`, which causes phase-location recording, and compared that added cost with two ordinary direct one-day passes. Samples were paired in both orders, with 21 samples after two warmups.

| Profile | N | No-op branch median ± MAD | Recorder overhead median ± MAD | Two direct days median ± MAD | Limit | Result |
|---|---:|---:|---:|---:|---:|---|
| M3P serial | 10,000 | 35.447 ± 0.198 | 125.019 ± 1.155 | 36.313 ± 0.173 | 3.631 | **FAIL** |
| M3P serial | 100,000 | 504.135 ± 2.283 | 10,464.374 ± 25.757 | 513.357 ± 2.422 | 51.336 | **FAIL** |

The measured recorder overhead exceeds the budget by about 34× at 10k and 204× at 100k. The current recorder calls `record_phase_components` after each core phase/hook and hashes canonical agent rows in AgentId order; this repeated per-agent hashing is the measured-path implementation. It explains the large recorder cost, but no recorder optimization was included in this task.

## Worked example: one-agent food override

Configuration: M3P sustained fixture, N=10,000, K=50, k=8, p_local=0.5, seed 81985529216486895, replicate 7, H=30. The intervention set `AgentId(0).food = 0.0` at the branch boundary.

- First divergence: day 0, slot `InterventionBoundary`, first differing agent `AgentId(0)`, component `agent_rows`.
- Daily divergence-set sizes `|D_d|`, for days 0 through 29: `[3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 18, 60, 119, 166, 198, 227, 255, 272, 308, 323, 339, 348, 359, 367, 372, 369, 383, 391]`.
- Newborn CRN loss: **0** matched births with differing AgentIds over the 30-day horizon.

## Correctness and completion status

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --workspace` passed. The existing M2/M3/M3P/propagation tests, goldens, and snapshot checks passed unchanged; the new M4 no-op identity test passed for all four supported profile combinations. No existing test, golden, snapshot, or CI file was changed.

M4-02 remains **not complete**: five of seven orchestration cells and both phase-recorder cells failed their frozen performance limits. The report does not update the M4 contract's §3 stage status or the README. Before M4-02 can be marked complete, the remaining orchestration and phase-recorder costs need to be reduced and all §11.7 cells rerun.
