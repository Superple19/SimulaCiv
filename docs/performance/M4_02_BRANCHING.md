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

At the initial gate run, five of seven orchestration cells and both phase-recorder cells failed their frozen performance limits. M4-02 remains **not complete** after the follow-up: four of seven orchestration cells and both phase-recorder cells still fail. The report does not update the M4 contract's §3 stage status or the README.

## Follow-up after attribution and fixes

The historical failed run above is retained. Steps A–C were completed in commits `5a1b050` and `89c3587`; no M4-02 gate budget or runtime semantics were changed.

### Step A: cost attribution before the fixes

Temporary release timers split the 30-day branch scan at `N=100,000`. Values are milliseconds per paired branch call; the day-start clone column covers all 60 retained branch states.

| Profile / execution | Day runner | Day digests | `D_d` | Lineage | Component flags | Day-start clones | Start-state digest | Result assembly | Other | Total loop |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| M3 serial | 1,025.026 | 423.563 | 220.804 | 0.070 | 0.006 | 111.235 | 8.459 | 0.023 | 35.365 | 1,824.551 |
| M3P incremental topology | 1,810.434 | 882.348 | 218.580 | 6.845 | 0.005 | 103.863 | 9.309 | 0.018 | 31.764 | 3,063.166 |

For the forced-action M3P recorder case, the old WorldState reference locator took 10,462.159 ms. Two plain reference-day executions accounted for about 10,200 ms (5,100 ms/day median); canonical row fingerprints added 211.851 ms over 3,200,060 slot-hash operations, or 66.20 ns/slot; the remaining recorder overhead was about 50.308 ms. One plain M3P storage day took 119.15 ms median. The measured WorldState reference loop, rather than the fingerprint operation alone, dominated that recorder run.

### Steps B–C: changes and parity

- Equal State/Metrics/Events digest days now return `D_d = 0` and skip the agent-row, lineage, and component-difference scans. Matching, ascending AgentId slot maps use direct SoA row comparisons; other layouts use a canonical AgentId merge. Shared birth records are processed once when both branches remain equal.
- M3P and M3P+propagation phase-location now replays both states through the generic SegmentedAgentStorage loop in lockstep and stops at the first differing boundary. It compares rows in canonical AgentId order and uses the sorted fallback when physical slots differ. At the Phase 9B boundary it excludes newborn propagation rows because the reference path creates those rows at Phase 9C.
- The new storage-vs-reference locator test covers serial and incremental M3P/M3P+propagation paths, divergent births, and shuffled slots. `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` passed. Existing goldens, snapshots, and integration test files were unchanged.

### Step D: paired gate rerun

Host: AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors. The release measurement process ran at High priority with affinity `0x555`; as in the historical run, the logical-to-physical mapping was not independently verified. CPU load was 21.2% before and 16.1% after; available RAM was 61,150,852 KB before and 59,681,040 KB after. The harness used two warmups, 21 samples, alternating direct-first and branch-first order, `H=30`, metrics/events on, snapshots off, and scratch reuse. Total wall time was **939.4 seconds**. The direct-runner samples below time two ordinary storage passes without analysis hashing; direct digest medians are added from the existing isolated digest probe to show the comparable direct-pair reference. Clone and branch digest exclusions reuse the existing probe, except at `N=100,000`, where the current Step A clone/hash timings are used. The hashing and clone implementations were not changed by Steps B–C.

| Profile / execution | N | Direct runner pair ± MAD | Direct digest estimate | Comparable direct pair | Branch API ± MAD | Branch digest estimate | Clone estimate | Adjusted overhead | Limit | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| M3 serial | 10,000 | 112.959 ± 0.558 | 41.067 | 154.026 | 156.872 ± 2.252 | 31.738 | 2.682 | 9.493 | 7.701 | **FAIL** |
| M3 serial | 100,000 | 1,296.505 ± 6.153 | 382.554 | 1,679.059 | 1,930.721 ± 46.053 | 423.563 | 111.235 | 99.418 | 83.953 | **FAIL** |
| M3P serial | 10,000 | 483.532 ± 4.267 | 69.772 | 553.304 | 552.472 ± 4.447 | 66.098 | 2.145 | 0.697 | 27.665 | **PASS** |
| M3P incremental topology | 10,000 | 158.134 ± 2.783 | 70.860 | 228.994 | 241.435 ± 6.920 | 62.469 | 2.112 | 18.720 | 11.450 | **FAIL** |
| M3P serial | 100,000 | 8,225.486 ± 280.556 | 673.469 | 8,898.955 | 9,033.230 ± 289.305 | 882.348 | 103.863 | -178.467 | 444.948 | **PASS** |
| M3P incremental topology | 100,000 | 2,712.470 ± 324.297 | 593.645 | 3,306.115 | 4,133.385 ± 570.245 | 882.348 | 103.863 | 434.704 | 570.245 | **PASS** |
| M3 + propagation serial | 100,000 | 4,032.641 ± 32.378 | 390.181 | 4,422.822 | 4,985.860 ± 49.732 | 669.345 | 102.726 | 181.148 | 221.141 | **PASS** |

Adjusted overhead is `branch API − direct runner pair − branch digest − day-start clones`. The comparable direct pair adds the previously isolated direct-digest estimate and is used for the 5% limit; the directly timed runner pair intentionally excludes hashing. The limit is `max(5% of comparable direct pair, larger observed runner/branch MAD)`. The `N=100,000` M3P incremental samples have high spread (11.9% direct MAD/median and 13.8% branch MAD/median); their recorded limit therefore uses the larger MAD as §11.7 specifies.

The phase-recorder rerun used two ordinary one-day passes as its baseline and paired the no-op branch against a forced-Idle branch. It used the same 21-sample alternating order and noise controls.

| Profile | N | Two direct days ± MAD | No-op branch ± MAD | Recorder overhead ± MAD | Limit | Result |
|---|---:|---:|---:|---:|---:|---|
| M3P serial | 10,000 | 41.273 ± 2.949 | 41.160 ± 3.602 | 11.344 ± 2.757 | 4.127 | **FAIL** |
| M3P serial | 100,000 | 583.251 ± 14.088 | 578.451 ± 19.543 | 190.443 ± 26.899 | 58.325 | **FAIL** |

The storage lockstep recorder reduced the old overhead from 125.019 ms to 11.344 ms at 10k, and from 10,464.374 ms to 190.443 ms at 100k. Both still exceed the frozen 10% recorder budget.

### Proposed review amendment

Four of seven orchestration cells and both recorder cells still fail. M4-02 therefore remains **not complete**; §3 and the README were not changed. The measurements support review of the proposed, unfrozen **§11.7 A-1** appended to the contract. It separates required per-day digest/`D_d`/lineage work from runner overhead and measures the recorder's required replay-to-first-difference prefix separately from the comparisons performed during that replay. In this run, the N=100,000 M3P recorder added 190.443 ms against a 583.251 ms two-day baseline (32.7%); at N=10,000 it added 11.344 ms against 41.273 ms (27.5%). Those figures show that the current recorder budget also counts replaying the two states through the first differing boundary, even though that replay is required to locate the phase. No budget change is made without review.
