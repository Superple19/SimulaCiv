# M4-01 System Registration Architecture

**Status: implementation in place; §10.6 performance gate failed. M4-01 is not complete.**

This report records the implementation through commit `834fe87` and the local release gate run below. The failed cells are retained as measured; no thresholds or goldens were changed. The M4 contract and README completion status remain unchanged.

## Inventory before and after

The before count is the frozen §10.1 manual inventory at baseline `1b210a6497438f79f9f7eb79bcc1f191e21c4f62`: 14 Rust modules, 24 M3-family runner/dispatch/helper definitions in the runner inventory range, about 2,070 lines of contexts and orchestration, three reference entries, six serial/non-parallel storage entries, and three explicit parallel entries. The source count is a manual inventory, not a mechanically deduplicated LOC measurement.

After M4-01, `system_registry.rs` brings the involved module count to 15. The storage surface still has six serial/non-parallel mode families and three explicit parallel mode families; public names and signatures are preserved. The source now has two generic schedulers, `run_registered_world_day_loop` and `run_registered_storage_day_loop`. The manual function scan finds 22 M3-family/configured/registered definitions in `runner.rs`, including those two generic loops. The profile-specific public storage entrypoints dispatch through the generic storage loop; the M3 and M3P reference runners also use the generic WorldState loop.

The same broad manual line-range method gives about 3,500 lines across M3-family contexts and execution orchestration in `runner.rs`, versus about 2,070 before. The increase reflects the new phase-state adapters and hook dispatch. It does not represent 1,430 lines of duplicated phase bodies: the count includes the generic scheduler, profile-specific day state, retained oracle helpers, and compatibility wrappers. M3PROP and M3PPROP WorldState reference entrypoints remain their existing reference implementations; their storage paths use the registered storage loop.

`run_configured_storage_days` validates that the registry's feature-set map agrees with `SimulationSystems` and `runner_selection`, then preserves the existing typed context/result dispatch. M2 remains on its original runner. Adding the test-only no-op descriptor required no runner entrypoint and no change to either generic loop; the test toggles it in a test registry and verifies identical legacy M3 outcomes, state digest, metrics, events, and snapshot bytes.

## Performance gate

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

The M3P 10k and 100k cells fail the frozen tolerance. No inference is made about the precise source of their regression; the gate did not include per-phase instrumentation. These two cells must be investigated and pass a rerun before M4-01 can graduate.

## Test-time change

The pre-implementation one-shot workspace run at baseline took 501.84 s. The post-Step-5 one-shot run took 571.34 s, a difference of 69.50 s (+13.8%). Intermediate full-suite runs ranged from 480.60 s to 557.88 s, so this is a rough before/after observation rather than a controlled test-time benchmark. All required commands passed after each implementation commit. The suite now includes a generic-loop phase-order test, a legacy M3 golden comparison, a 60-cell day-by-day M3 storage legacy matrix, and a 10-day M3P propagation legacy comparison.

## Compatibility and scope record

- M2/M3/M3P/propagation goldens, snapshot checks, and existing error tests passed unchanged.
- No existing test, fixture, golden, snapshot, contract, or CI file was edited.
- M2 runners were not converted. Existing public entrypoint names and signatures remain.
- System hooks use existing subsystem IDs and frozen phase slots. Caller-owned pools remain explicit options; the registry does not create or resize pools.
- The generic WorldState loop is used by the M3 and M3P base reference entrypoints. M3PROP and M3PPROP reference implementations remain the reference-oracle paths; their registered storage paths preserve existing serial/parallel options.

**Gate outcome: M4-01 NOT COMPLETE.** The M3P performance failures prevent the §10.6 success condition. This report does not change §3 of the M4 contract or the README status.
