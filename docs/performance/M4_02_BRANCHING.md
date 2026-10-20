# M4-02 Branching and Divergence Detection

**Current status: M4-02 remains not complete. A-3 timing data are recorded below, but the first pass returned a zero harness-CPU counter and the one retry lost its per-cell CPU trace during monitor shutdown, so no cell has a valid A-3 gate decision. A-1 and A-2 results remain historical.**

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

### Final follow-up: frozen §11.7 A-1 paired measurement

This run applies the user-approved, frozen [§11.7 A-1 protocol](../contracts/M4_COUNTERFACTUAL_CONTRACT.md#117-a-1--measurement-protocol-amendment-frozen). It supersedes the adjusted-cost calculations above for the gate decision; those earlier runs remain as historical measurements.

The release harness ran in one process on the AMD Ryzen 5 9600X (6 physical cores / 12 logical processors), at High priority with processor affinity `0x555` (logical-to-physical mapping was not independently verified). CPU load was 8% before and 100% after; free RAM changed from 59,256,460 KB to 51,412,508 KB. The run started at 21:21:29 and finished at 22:40:41 local time, for **79m 12.1s**. Intermediate host load was not sampled.

The harness used one Day 0 snapshot per cell, metrics and events enabled, snapshots disabled, and a 30-day horizon. Every direct-pair sample restored the same snapshot twice inside the timed region. It paired that direct pair with the analysis-disabled branch API and the ordinary full branch API, alternating their order. A same-sample instrumented full-analysis call supplied the digest, `D_d`, lineage, and clone breakdown. No cost estimate from a separate probe was subtracted. Each cell began with two warmups and 21 timed samples; because at least one measured series in each cell exceeded 5% MAD/median, each cell was rerun with 41 timed samples. The final table uses only those 41-sample reruns.

The paired-delta spread ratio below is `MAD / abs(median(delta))`; the absolute value makes cells with a negative overhead delta comparable. The gate limit remains `max(5% of direct-pair median, larger paired MAD)`. The analysis increment has no absolute limit under A-1.

#### Orchestration gate

Times are milliseconds per 30-day base-plus-branch pair. “Analysis-disabled” is the branch API with daily digests, `D_d`, lineage matching, component flags, and day-start clones disabled. “Full” is the normal `run_counterfactual_branch` API.

| Profile / execution | N | Direct pair ± MAD | Analysis-disabled API ± MAD | Full API ± MAD | Disabled − direct ± paired MAD | Limit | Delta MAD / \|median\| | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| M3 serial | 10,000 | 132.480 ± 3.911 | 129.669 ± 4.414 | 173.514 ± 4.151 | -3.441 ± 2.918 | 6.624 | 84.8% | **INCONCLUSIVE** |
| M3 serial | 100,000 | 1,319.605 ± 17.468 | 1,306.890 ± 5.844 | 1,870.942 ± 20.761 | -5.934 ± 10.037 | 65.980 | 169.1% | **INCONCLUSIVE** |
| M3P serial | 10,000 | 473.710 ± 2.567 | 472.065 ± 2.765 | 539.894 ± 2.787 | 0.004 ± 3.047 | 23.685 | 72,555% | **INCONCLUSIVE** |
| M3P incremental topology | 10,000 | 155.099 ± 0.603 | 152.849 ± 1.382 | 231.449 ± 1.685 | -2.548 ± 1.297 | 7.755 | 50.9% | **INCONCLUSIVE** |
| M3P serial | 100,000 | 7,626.565 ± 662.237 | 7,344.919 ± 479.093 | 8,667.315 ± 797.475 | 11.255 ± 202.801 | 662.237 | 1,802% | **INCONCLUSIVE** |
| M3P incremental topology | 100,000 | 2,223.660 ± 36.769 | 2,193.550 ± 39.109 | 3,231.525 ± 37.940 | -44.728 ± 23.627 | 111.183 | 52.8% | **INCONCLUSIVE** |
| M3 + propagation serial | 100,000 | 3,806.915 ± 455.620 | 3,710.369 ± 286.251 | 4,956.131 ± 639.222 | -27.331 ± 75.570 | 455.620 | 276.5% | **INCONCLUSIVE** |

All seven cells remain inconclusive because the paired disabled-minus-direct delta still had MAD/median above 5% after 41 samples. The 100k M3P serial and M3 + propagation cells also retained above-5% spread in their individual path timings after the rerun. No cell was omitted. The median disabled-minus-direct deltas are within the calculated limits, but A-1 does not allow those noisy cells to be reported as PASS.

#### Analysis cost record

The decomposition below reports the medians of instrumented full-analysis costs per alive agent-day. The agent-day denominator is the sum of daily live populations in both base and branch runs. The last column is the independently paired median of the ordinary full API minus the analysis-disabled API for the same cell.

| Profile / execution | N | Digest ns / agent-day | `D_d` + flags ns / agent-day | Lineage ns / agent-day | Day-start clones ns / agent-day | Full − disabled per pair |
|---|---:|---:|---:|---:|---:|---:|
| M3 serial | 10,000 | 57.894 | 0.0072 | 0.0080 | 8.828 | 46.174 ms |
| M3 serial | 100,000 | 69.540 | 0.0014 | 0.0026 | 16.447 | 557.582 ms |
| M3P serial | 10,000 | 106.258 | 0.0115 | 0.3026 | 4.666 | 67.340 ms |
| M3P incremental topology | 10,000 | 125.353 | 0.0078 | 0.2798 | 3.837 | 78.143 ms |
| M3P serial | 100,000 | 135.038 | 0.0013 | 0.4157 | 14.748 | 877.895 ms |
| M3P incremental topology | 100,000 | 163.426 | 0.0013 | 0.3883 | 14.715 | 1,062.624 ms |
| M3 + propagation serial | 100,000 | 131.926 | 0.0015 | 0.0027 | 29.709 | 1,161.911 ms |

The full-minus-disabled figures are a regression baseline only; A-1 sets no absolute limit for them. The instrumented component medians are measured independently inside the same sample sets and need not sum exactly to the externally timed delta.

#### Recorder gate

The replay baseline is the comparison-disabled lockstep replay to the same first-difference boundary. Replay and comparison-stage timings are reported separately from the full one-day branch API timings.

| Profile | N | Full locator API ± MAD | Comparison-disabled API ± MAD | Paired overhead ± MAD | Replay stage median | Comparison timer median | Limit | Delta MAD / \|median\| | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| M3P serial | 10,000 | 54.339 ± 3.779 | 53.018 ± 3.627 | 1.106 ± 1.894 ms | 2.111 ms | 0.465 ms | 3.779 ms | 171% | **INCONCLUSIVE** |
| M3P serial | 100,000 | 877.184 ± 147.298 | 893.782 ± 144.911 | -1.693 ± 27.008 ms | 35.798 ms | 5.865 ms | 147.298 ms | 1,595% | **INCONCLUSIVE** |

Both recorder cells remain inconclusive after 41 samples. Their paired comparison deltas were noisy relative to their near-zero medians; the 100k full and replay API paths also remained above 5% MAD/median. The paired overhead medians are below the frozen limits, but they cannot be called PASS under A-1.

#### Decision

No §11.7 A-1 cell is PASS: all seven orchestration cells and both recorder cells are inconclusive. M4-02 therefore remains **not complete**. The M4 contract §3 and README status are unchanged. No gate budget, simulation behavior, digest, snapshot, test, golden, or CI setting was changed by this measurement follow-up.

### Historical proposed review amendment (later frozen)

Four of seven orchestration cells and both recorder cells failed in that earlier run. M4-02 remained **not complete**; §3 and the README were not changed. At the time, those measurements supported review of the then-proposed **§11.7 A-1**. The user later approved and froze that measurement protocol; the controlled rerun above applies it. The earlier run measured the N=100,000 M3P recorder at 190.443 ms against a 583.251 ms two-day baseline (32.7%), and the N=10,000 recorder at 11.344 ms against 41.273 ms (27.5%). No budget was changed in that earlier run.

### Confirmatory paired gate under frozen §11.7 A-2

The A-1 results above are retained as history and are not re-judged. This section reports a fresh measurement under [§11.7 A-2](../contracts/M4_COUNTERFACTUAL_CONTRACT.md#117-a-2--decision-rule-amendment-frozen). Both passes used the AMD Ryzen 5 9600X host (6 physical cores / 12 logical processors), High process priority, and affinity mask `0x555`; the logical-to-physical mapping was not independently verified. The release harness used two warmups and 21 timed samples per cell, paired paths in the same process, and alternated their execution order. Orchestration used H=30, metrics/events enabled, snapshots off, and the existing M2-40-derived M3 and M3P fixtures. Recorder cells used the existing one-day forced-Idle `AgentId(0)` case. The first pass took **726.3 s**; its required one-time invalid-cell retry took **727.1 s** (**1,453.4 s** combined, excluding build and baseline sampling).

The host-validity threshold is `1.20 × median(pre-run idle CPU load)`. Total CPU load was sampled every 30 seconds and at cell boundaries. Pass 1 baseline samples were 27.14%, 20.68%, 21.95%, 18.93%, and 19.66% (median 20.68%, threshold 24.82%); load was 27.14% before and 26.35% after. Free RAM changed from 58,268,484 KB to 58,318,888 KB. Retry baseline samples were 18.63%, 20.50%, 21.18%, 19.58%, and 23.74% (median 20.50%, threshold 24.60%); load was 18.63% before and 18.97% after. Free RAM changed from 58,336,392 KB to 58,445,708 KB. Both runs applied High priority and affinity `0x555`. Retry harness process `PeakWorkingSet64` was **731,623,424 bytes (697.7 MiB)**. The first-pass peak was not captured: the post-exit process query returned zero.

#### Orchestration gate

For each sample, `delta = analysis-disabled branch API − direct pair`. `T = 5% × median(direct pair)`. The table gives the measured medians ± MAD and the numerical result under A-2 before host validity is applied.

| Profile / execution | N | Direct pair ms | Analysis-disabled API ms | Delta ms | T ms | Median(delta) + 2 MAD ms | Numerical result | Final validity |
|---|---:|---:|---:|---:|---:|---:|---|---|
| M3 serial | 10,000 | 115.454 ± 2.224 | 115.703 ± 1.330 | 0.292 ± 1.300 | 5.773 | 2.892 | PASS | **INVALID after retry** |
| M3P serial | 10,000 | 499.322 ± 8.376 | 499.269 ± 3.140 | 1.628 ± 5.663 | 24.966 | 12.955 | PASS | **INVALID after retry** |
| M3P incremental topology | 10,000 | 162.838 ± 1.180 | 164.154 ± 3.226 | -0.205 ± 3.195 | 8.142 | 6.185 | PASS | **INVALID after retry** |
| M3 serial | 100,000 | 1,349.032 ± 9.687 | 1,363.160 ± 7.647 | 24.422 ± 13.460 | 67.452 | 51.342 | PASS | **INVALID after retry** |
| M3P serial | 100,000 | 7,390.173 ± 30.987 | 7,386.248 ± 56.347 | -55.021 ± 70.081 | 369.509 | 85.141 | PASS | **INVALID after retry** |
| M3 + propagation serial | 100,000 | 3,352.237 ± 16.377 | 3,368.788 ± 13.536 | 11.721 ± 21.730 | 167.612 | 55.180 | PASS | **INVALID after retry** |
| M3P incremental topology | 100,000 | 2,071.618 ± 23.366 | 2,054.807 ± 22.512 | -27.507 ± 22.102 | 103.581 | 16.697 | PASS | **INVALID after retry** |

#### Recorder gate

For each sample, `delta = full one-day locator API − comparison-disabled one-day locator API`. `T = 5% × median(comparison-disabled API)`. Replay-prefix time is not the gate baseline; replay cost is shown separately.

| Profile | N | Full locator API ms | Comparison-disabled API ms | Delta ms | T ms | Median(delta) + 2 MAD ms | Replay cost ms | Numerical result | Final validity |
|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| M3P serial | 10,000 | 48.354 ± 0.741 | 47.517 ± 0.566 | 0.900 ± 0.999 | 2.376 | 2.898 | 2.009 ± 0.013 | INCONCLUSIVE | **INVALID after retry** |
| M3P serial | 100,000 | 699.842 ± 5.280 | 698.001 ± 9.530 | 6.587 ± 7.458 | 34.900 | 21.504 | 26.061 ± 0.281 | PASS | **INVALID after retry** |

#### Per-cell CPU-load trace and disposition

Each range summarizes the total-CPU readings assigned to that cell, including 30-second samples and start/end boundary samples. `over/total` counts readings above that run's threshold. Every first-pass cell was invalid and therefore retried once. Every retry cell also had at least one reading over threshold, so all nine final cells are **INVALID**; the numerical gate outcomes above are not valid PASS/FAIL/INCONCLUSIVE decisions.

| Cell | Pass 1 load range %, over/total | Retry load range %, over/total | Final host validity |
|---|---:|---:|---|
| M3 serial, 10k | 28.68–31.49, 2/2 | 28.50–28.87, 2/2 | **INVALID** |
| M3P serial, 10k | 30.77–30.81, 2/2 | 27.02–35.99, 2/2 | **INVALID** |
| M3P incremental, 10k | 30.03–35.61, 3/3 | 28.25–28.71, 3/3 | **INVALID** |
| M3 serial, 100k | 28.20–32.09, 4/4 | 27.59–37.54, 4/4 | **INVALID** |
| M3P serial, 100k | 26.43–30.51, 13/13 | 26.93–31.64, 13/13 | **INVALID** |
| M3 + propagation serial, 100k | 27.61–30.26, 7/7 | 27.16–33.73, 7/7 | **INVALID** |
| M3P incremental, 100k | 27.08–28.26, 5/5 | 27.78–32.20, 5/5 | **INVALID** |
| Recorder M3P serial, 10k | 27.94–28.70, 3/3 | 26.23–28.13, 3/3 | **INVALID** |
| Recorder M3P serial, 100k | 18.92–28.02, 2/3 | 19.88–39.54, 4/5 | **INVALID** |

The retry did not yield any host-valid gate cell, so no M4-02 completion decision can be made from this run. M4-02 remains **not complete**; §3 and the README are unchanged. The A-1 history is superseded for the decision rule only and is not reinterpreted here.

### Confirmatory measurement under frozen §11.7 A-3

The A-1 and A-2 measurements above remain historical and are not re-judged. This fresh release pass used all nine cells, two warmups, 21 timed samples, paired paths in one process, and alternating execution order. It retained High process priority and affinity `0x555` on the AMD Ryzen 5 9600X (6 physical / 12 logical processors). The first complete timing pass took 782.3 seconds. Its process counter used a single sample for a rate counter and returned zero harness CPU share; that pass is invalid under A-3 and was rerun once for all nine cells.

The one permitted rerun produced timing records for all nine cells in 782 seconds. Its CPU monitor mapped the harness PID to a Windows process-counter instance and sampled total and process CPU together. On the final post-run boundary, the process-counter path became unavailable after the harness exited; the monitor failed before it wrote the per-cell CPU trace or host summary. Therefore the rerun cannot establish the A-3 other-process CPU validity condition. Both complete passes are invalid for the gate; no further retry was made. The first pass's recorded total CPU and host summary are not used as an A-3 validity result because its harness share was zero.

#### A-3 numerical decision calculations

The table reports the retry's paired medians and MADs in milliseconds, deterministic bootstrap upper bound `U`, threshold `T`, and both path MAD/median ratios. These are numerical calculations only; the final validity column reflects the missing retry CPU trace. Orchestration path order is direct pair / analysis-disabled API; recorder path order is full locator / comparison-disabled API.

| Cell | Path MAD/median % | Median(delta) ± MAD ms | U ms | T ms | Numerical result | Final A-3 status |
|---|---:|---:|---:|---:|---|---|
| M3 serial, 10k | 1.81 / 3.56 | -1.022 ± 3.559 | 0.791 | 5.658 | PASS | **INVALID — CPU trace unavailable** |
| M3P serial, 10k | 4.25 / 3.89 | 6.397 ± 27.289 | 26.197 | 27.192 | PASS | **INVALID — CPU trace unavailable** |
| M3P incremental, 10k | 6.27 / 6.62 | -0.870 ± 16.805 | 9.207 | 9.205 | INCONCLUSIVE | **INVALID — CPU trace unavailable; path spread >5%** |
| M3 serial, 100k | 2.55 / 2.54 | 37.435 ± 42.948 | 59.625 | 71.022 | PASS | **INVALID — CPU trace unavailable** |
| M3P serial, 100k | 1.18 / 1.10 | 44.958 ± 106.156 | 96.881 | 406.807 | PASS | **INVALID — CPU trace unavailable** |
| M3 + propagation serial, 100k | 0.56 / 0.67 | 2.653 ± 35.718 | 26.400 | 173.334 | PASS | **INVALID — CPU trace unavailable** |
| M3P incremental, 100k | 1.05 / 1.98 | -20.006 ± 33.453 | 9.474 | 111.690 | PASS | **INVALID — CPU trace unavailable** |
| Recorder M3P serial, 10k | 3.39 / 5.92 | 1.991 ± 5.963 | 7.717 | 2.873 | INCONCLUSIVE | **INVALID — CPU trace unavailable; path spread >5%** |
| Recorder M3P serial, 100k | 3.64 / 3.16 | 11.480 ± 37.747 | 41.417 | 39.957 | INCONCLUSIVE | **INVALID — CPU trace unavailable** |

#### Host validity summary

The first pass recorded a pre-run total-CPU baseline median of 9.145% (range 8.338–9.874%), so its A-3 other-process threshold was 14.145 percentage points. The harness process was High priority with affinity `0x555`; the recorded peak working set was 746,364,928 bytes. However, its process rate-counter value was zero on every sample, so subtracting it would reproduce the known A-2 error. The retry's harness completed all timing cells, but its CPU trace and host summary were not saved after the process counter disappeared on the final boundary sample. Consequently, the retry's per-cell other-process CPU cannot be verified. The two 10k cells shown with path spread above 5% also remain invalid after the single retry.

| Cell | First pass total-CPU samples, range % (count) | Logged harness share | Retry other-process load |
|---|---:|---:|---|
| M3 serial, 10k | 17.597–25.246 (2) | 0% (invalid counter result) | unavailable |
| M3P serial, 10k | 17.332–18.687 (3) | 0% (invalid counter result) | unavailable |
| M3P incremental, 10k | 18.231–18.895 (2) | 0% (invalid counter result) | unavailable |
| M3 serial, 100k | 18.1–19.0 (4) | 0% (invalid counter result) | unavailable |
| M3P serial, 100k | 16.1–18.0 (14) | 0% (invalid counter result) | unavailable |
| M3 + propagation serial, 100k | 17.9–19.6 (8) | 0% (invalid counter result) | unavailable |
| M3P incremental, 100k | 17.3–22.4 (7) | 0% (invalid counter result) | unavailable |
| Recorder M3P serial, 10k | 17.8–18.3 (2) | 0% (invalid counter result) | unavailable |
| Recorder M3P serial, 100k | 9.1–17.6 (3) | 0% (invalid counter result) | unavailable |

M4-02 remains **not complete**. No §3 or README completion update was made, and M4-03 was not drafted because the A-3 gate did not produce nine valid PASS cells.
