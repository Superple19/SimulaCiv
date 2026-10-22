# M4-01 System Registration Architecture
Status: current

M4-01 COMPLETE.


This report records the implementation through commits `5dc417f` and `861b663`, the noise-controlled §10.6 reruns, and the final inventory. The initial failed cells are retained as historical measurements; no thresholds or goldens were changed. M4-01 meets the frozen §10 success criteria: all measured cells pass, and per-combination day loops route through the two generic layout loops.

## Inventory before and after

The before count is the frozen §10.1 manual inventory at baseline `1b210a6497438f79f9f7eb79bcc1f191e21c4f62`: 14 Rust modules, 24 M3-family runner/dispatch/helper definitions in the runner inventory range, about 2,070 lines of contexts and orchestration, three reference entries, six serial/non-parallel storage entries, and three explicit parallel entries. The source count is a manual inventory, not a mechanically deduplicated LOC measurement.

After M4-01, `system_registry.rs` brings the involved module count to 15. The storage surface still has six serial/non-parallel mode families and three explicit parallel mode families; public names and signatures are preserved. The source now has two generic schedulers, `run_registered_world_day_loop` and `run_registered_storage_day_loop`. The manual function scan finds 22 M3-family/configured/registered definitions in `runner.rs`, including those two generic loops. The profile-specific public storage entrypoints dispatch through the generic storage loop; the M3 and M3P reference runners also use the generic WorldState loop.

The production-only §10.1 inventory uses these current `runner.rs` ranges: contexts `394–1176` (783 lines), generic layout loops `264–384` (121), M3 reference adapter `1571–1898` (328), public storage/dispatch entries `2056–2296` (241), configured dispatch and execution policies `2414–2589` (176), registered M3P storage adapter `2590–3180` (591), and registered M3 storage adapter `3496–4030` (535). This totals **2,775 production lines**, compared with the 2,070-line baseline. The broad inventory is a manual source-range estimate, not deduplicated LOC.

Legacy alternate day bodies are test-only: runner ranges `1899–2055`, `2297–2413`, `3181–3495`, and `4031–4298` total **857 test-only oracle lines** and are excluded from the production target. The M3PROP and M3PPROP legacy WorldState adapters are also `#[cfg(test)]` and used only by the new per-day parity tests. No superseded legacy day loop remains in production. The production increase is chiefly registered phase-state adapters and hook dispatch; the per-profile loops themselves now route through the two generic layout loops.

The M3PROP and M3PPROP WorldState entrypoints now run through the generic WorldState loop. New tests compare their per-day outcomes, logical state, graph/propagation state, and snapshot bytes against the former reference paths for 500 M3PROP days and 300 M3PPROP days. The exact bytes, phase order, and public entrypoint signatures were preserved.

`run_configured_storage_days` validates that the registry's feature-set map agrees with `SimulationSystems` and `runner_selection`, then preserves the existing typed context/result dispatch. M2 remains on its original runner. Adding the test-only no-op descriptor required no runner entrypoint and no change to either generic loop; the test toggles it in a test registry and verifies identical legacy M3 outcomes, state digest, metrics, events, and snapshot bytes.

## Initial performance gate (historical)

### Host and method

- Host: Windows 11 Pro, AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors.
- Measurement: release test profile; uninstrumented whole-tick calls, metrics/events enabled, snapshots disabled; each sample ran three consecutive days with context/scratch construction and initial state setup outside timing.
- Sampling: two warmups, 11 samples per path, paired in one process with execution order alternating by sample. Reported medians and MADs are microseconds per tick.
- Comparison: registered public storage entrypoint versus the retained test-only legacy day helper. Every pair also compared the returned day outcomes. Existing differential, digest, snapshot, and replay gates were run separately.
- Command: `cargo test --release -p sim-model --lib m4_01_perf_probe::m4_01_registry_performance_probe -- --nocapture`
- Probe wall time: 43.97 s total; the timed test body took 35.60 s. The temporary measurement probe was removed after collection.
- Host noise controls: no process priority or CPU affinity was set. A pre-run load/RAM sample could not be collected from the Windows counters in this environment. The immediate post-run WMI CPU load sample was 27%; a later idle sample was 6%, so the latter is not paired with the benchmark interval. Available RAM was not recorded at the measurement boundary.

The frozen gate is `abs(registry median - current median) <= max(2% of current median, registry MAD, current MAD)`.

| Profile / workload | Registry median ± MAD (µs/tick) | Current median ± MAD (µs/tick) | Delta (µs) | Allowed (µs) | Gate |
|---|---:|---:|---:|---:|---|
| M3, N=10,000 | 2,176.97 ± 45.87 | 2,221.60 ± 69.23 | 44.63 | 69.23 | PASS |
| M3, N=100,000 | 27,125.07 ± 1,196.63 | 26,426.60 ± 1,116.43 | 698.47 | 1,196.63 | PASS |
| M3P, N=10,000 | 9,683.33 ± 542.67 | 8,635.30 ± 584.47 | 1,048.03 | 584.47 | **FAIL** |
| M3P, N=100,000 | 146,334.93 ± 3,948.43 | 140,877.50 ± 1,366.53 | 5,457.43 | 3,948.43 | **FAIL** |
| M3 + propagation, N=100,000 | 81,769.23 ± 1,170.20 | 83,253.50 ± 1,180.87 | 1,484.27 | 1,665.07 | PASS |

The initial 11-sample run failed the M3P 10k and 100k cells. The gate did not include per-phase instrumentation, and the later controlled runs did not reproduce those failures. Step B was therefore not triggered; no M3P performance fix or root-cause claim is made.

## Noise-controlled §10.6 reruns

Both reruns used the Windows 11 Pro host with AMD Ryzen 5 9600X (6 physical cores / 12 logical processors), High process priority, and affinity mask `0x555` (logical processors 0, 2, 4, 6, 8, 10). This selects one logical processor from each presumed SMT sibling pair; the Windows topology mapping was not independently queried. Each cell used two warmups, 21 timed paired samples, both execution orders (11/10 starts), and three consecutive days per sample. Initial state, contexts, scratch, and pools were outside the timer. Metrics and events were enabled; snapshots were disabled. Results are µs/tick, median ± MAD. The allowed delta is `max(2% of legacy median, registry MAD, legacy MAD)`.

WMI host samples around the Step A runs were: M3P-only run, CPU load 5%→21%, free RAM 61,054→61,079 MB; remaining cells, 18%→12%, free RAM 61,056→61,120 MB. Around the final Step E run, CPU load was 4%→5%, free RAM 60,989→60,979 MB (total visible RAM 97,908 MB). The figures are instantaneous host samples, not per-process CPU utilization.

### Step A — before M3PROP/M3PPROP reference routing

| Cell | Registry median ± MAD | Legacy median ± MAD | Delta | Allowed | Gate |
|---|---:|---:|---:|---:|---|
| M3, N=10,000 | 2,125.40 ± 46.77 | 2,168.47 ± 44.13 | 43.07 | 46.77 | PASS |
| M3, N=100,000 | 28,552.07 ± 782.63 | 27,309.87 ± 1,253.27 | 1,242.20 | 1,253.27 | PASS |
| M3P, N=10,000 | 8,726.60 ± 478.17 | 8,761.93 ± 364.17 | 35.33 | 478.17 | PASS |
| M3P, N=100,000 | 152,342.10 ± 3,117.60 | 151,494.53 ± 3,640.33 | 847.57 | 3,640.33 | PASS |
| M3 + propagation, N=100,000 | 82,671.83 ± 802.97 | 81,676.87 ± 1,681.03 | 994.97 | 1,681.03 | PASS |

### Step E — after reference routing

| Cell | Registry median ± MAD | Legacy median ± MAD | Delta | Allowed | Gate |
|---|---:|---:|---:|---:|---|
| M3, N=10,000 | 2,064.83 ± 59.63 | 2,111.17 ± 32.97 | 46.33 | 59.63 | PASS |
| M3, N=100,000 | 23,351.00 ± 267.53 | 22,936.80 ± 299.10 | 414.20 | 458.74 | PASS |
| M3P, N=10,000 | 7,822.87 ± 64.50 | 7,849.07 ± 130.40 | 26.20 | 156.98 | PASS |
| M3P, N=100,000 | 120,497.10 ± 938.97 | 120,534.93 ± 429.00 | 37.83 | 2,410.70 | PASS |
| M3 + propagation, N=100,000 | 74,828.77 ± 789.53 | 74,741.10 ± 1,671.07 | 87.67 | 1,671.07 | PASS |

The final temporary probe test body took 65.09 s. It was removed after collection. For M3+propagation, the legacy timing used the retained M3 storage day helper followed by the same serial Phase 9c overlay; outcome and world state equality were asserted. The overlay runs after the base helper returns in this test-only measurement proxy, so its position relative to base Phase 10/event flush differs from the former wrapper; this is a cache-order limitation for that cell. No registry code was timed with instrumentation.

## Test-time change

The pre-implementation one-shot workspace run at baseline took 501.84 s. The post-Step-5 one-shot run took 571.34 s, a difference of 69.50 s (+13.8%). The post-Step C/D-validation `cargo test --workspace` run took about 369 s wall time; this is not directly comparable to the earlier runs because host load, cache state, and execution conditions differed. The suite includes the M3PROP and M3PPROP per-day WorldState parity tests added in Step C.

## Compatibility and scope record

- M2/M3/M3P/propagation goldens, snapshot checks, and existing error tests passed unchanged.
- No existing test, fixture, golden, snapshot, contract, or CI file was edited.
- M2 runners were not converted. Existing public entrypoint names and signatures remain.
- System hooks use existing subsystem IDs and frozen phase slots. Caller-owned pools remain explicit options; the registry does not create or resize pools.
- The generic WorldState loop is used by M3, M3P, M3PROP, and M3PPROP references. The generic storage loop serves all registered storage profiles; serial defaults and explicit caller-owned parallel options remain unchanged.
- Step A and Step E controlled reruns pass all five numeric §10.6 cells. The earlier M3P failures were not reproduced; no failure was attributed to a particular source-level cause.
- Step D inventory found no superseded day-loop body in production; retained legacy paths are `#[cfg(test)]` only.

## Known debt

- Production wiring grew from about 2,070 to 2,775 lines, mainly from the M3/M3P storage phase-state adapters. The below-2,070 comparison was a completion-task target, not a frozen §10 acceptance condition. Reducing the wiring is deferred; the M4-06 forward gate—adding a system with no runner function or generic-loop edits—is the meaningful architecture test.
- The initial M3P performance failures were not reproduced under the noise controls; their root cause is unknown.
- The M3+propagation legacy timing used a proxy whose Phase 9c placement has a different cache order from the former wrapper; this is recorded as a measurement limitation.

**Gate outcome: M4-01 COMPLETE.** All numeric §10.6 cells pass under the recorded controlled reruns, parity gates pass, and the registered generic loops replace per-combination day loops. The 2,775-line inventory is retained as known debt, not a frozen completion blocker.
