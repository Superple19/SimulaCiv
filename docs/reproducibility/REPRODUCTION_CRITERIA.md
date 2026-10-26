# M2/M3 Reproduction Criteria

Status: FROZEN — approved 2026-10-25

Freeze date: 2026-10-25

## Scope and change reason

These frozen criteria govern the M2/M3 evidence-reconstruction inventory in [M2_M3_EVIDENCE_RECONSTRUCTION.md](M2_M3_EVIDENCE_RECONSTRUCTION.md). They were approved on 2026-10-25 before any timing run. The prior draft's unmeasured band justification, strict treatment of noisy rankings, and unspecified report-revision versus HEAD question are resolved below.

## Revision selection by class

| Class | What is measured | Default revision | Question answered |
|---|---|---|---|
| (a) deterministic replay | Hashes, exact fields, ordering, errors, and test outcomes | Original report-creation commit; use that commit's `rust-toolchain.toml` | Do the historical deterministic values reproduce? |
| (b) committed harness | The cited bench or example and its reported ratios/rankings | Original report-creation commit; use its pinned toolchain and harness | Do the historical timing numbers reproduce? |
| (c) complete recovered source | The recovered driver against the historical API, without running recovered binaries | Original report-creation commit; use its pinned toolchain; place the driver under `crates/sim-model/examples/` and copy it as untracked into the historical worktree | Do the historical timing numbers reproduce? |
| (d) reconstructed protocol | A new committed driver calling APIs available at the historical revision | Original report-creation commit and its pinned toolchain; no edits to historical tracked source | Do the historical timing numbers reproduce? |
| (e) underspecified | No run until missing configuration, timer boundary, or cell set is resolved | None | Not evaluated until the protocol is decided |
| (f) M5-05 conclusion covered | The already recorded M5-05 conclusion rerun | Revision and toolchain recorded in the M5-05 summary/provenance | Does the conclusion still hold? This does not establish historical-number reproduction. |

The CSV records each report-creation commit and the `rust-toolchain.toml` channel read from that commit. All report revisions currently listed in this inventory pin Rust `1.98.1`. A HEAD rerun is a separate result answering only whether the conclusion still holds; it does not replace the default historical-reproduction run. If the original commit has no toolchain pin, use the compiler version recorded in the report or mark the row `needs-decision` before running.

## Deterministic evidence

- Canonical digests must match exactly. Other deterministic outputs must match their recorded field values exactly, including ordering and errors when those are the claim.
- Do not change a golden, snapshot, fixture, or historical value to make a rerun pass.

## Timing evidence

- Record absolute medians and MADs for context, but do not compare absolute elapsed times for pass/fail.
- Predeclare the dimensionless effect that supports the historical conclusion: a speedup/ratio, scaling alpha, phase share, crossover, or ranking. If no historical denominator or ordering exists, mark the claim `needs-decision` before running. A zero historical denominator or undefined ratio also requires `needs-decision`.
- For a historical ratio `q_old` and rerun ratio `q_new`, record both ratios and the relative deviation `abs(q_new / q_old - 1)`. Classify deviations of **≤10%** as **reproduced**, **>10% and ≤20%** as **reproduced (loose)**, and **>20%** as **not reproduced**. Do not compare absolute elapsed times for pass/fail.
- Enforce a historical ordering only when the original pair was distinguishable: either its absolute gap exceeded `2 * max(MAD_a, MAD_b)`, or its relative ratio gap exceeded 20%. If neither condition holds, record the original and rerun ordering but do not make it pass/fail. For significant original orderings, require the same order. Crossover endpoints and gate verdicts remain exact when the original endpoints/verdict were outside the noise band.
- Use the reference Ryzen 5 9600X Windows host and its A-5 protocol: two warmups, 41 paired samples, and a per-cell repetition factor `r` chosen in warmup and fixed so every timed sample is at least one second; preserve pair alternation. A cell failing A-5 validity is inconclusive, not a pass.

## Measured basis for the ratio bands

The between-session M3-09 protocol-matched result is 1.322346887 at follow-up commit `c857d51517bf8df2ff5f50c6cb035585fc4b549e`, compared with the historical 1.49 ratio in [M3-09](../reports/M3_09_INCREMENTAL_TOPOLOGY.md). The deviation is **11.25%**, so this example falls in the reproduced (loose) band. The recorded result and protocol are in `docs/data/m5_05/m3_09_scope_check/summary.json`.

Another exact same-configuration comparison across separate M5-05 stage runs is M2-40 workload B at N=100,000/K=500, Serial versus Explicit Rayon 6/256 over three days: `m2_near_linear` recorded 1.253153606 and `m2_final_tick` recorded 1.278693533, a **2.04%** max/min spread. Both summaries identify the same config digest `sha256:ad4426ef75da87d2c809acd1cbee364d3981f796b0776f2c97e5c542e79ab691` (`docs/data/m5_05/m2_near_linear/summary.json`, `docs/data/m5_05/m2_final_tick/summary.json`). The protocol-matched M3-09 comparison between follow-up and HEAD is supplementary cross-revision evidence: 1.322346887/1.352036586 at 100k (2.25%) and 1.242627486/1.232961366 at 10k (0.78%). The 10% and 20% bands are decision grades, not estimates of measurement noise; the 20% upper band is retained as a loose-reproduction category based on the observed 11.25% historical-to-follow-up change. The available repeat evidence remains limited to these workloads and configurations.

## Failure record

For each failed or inconclusive cell, preserve the old and new value, derived ratio/rank, A-5 median/MAD/validity, configuration, command, revision/toolchain, and raw-output checksum. Mark it `not reproduced` or `inconclusive`; do not silently widen the tolerance, remove a cell, or replace an expected value. Any later criteria change requires a separate reviewed amendment before rerunning, and the earlier result remains in the record.

## M3-05 pilot procedure

The pilot is the merged M3-05 first-break group. Its historical report was written at commit `5df006c150179f67078588dfaf83a355a81d5f9f`, whose `rust-toolchain.toml` pins Rust `1.98.1`. The committed DVC wrapper creates a detached worktree at that commit and copies the separate driver into it as an untracked file; the driver includes that worktree's unchanged scale bench source. The historical tree is not edited.

```powershell
dvc repro docs/data/m5_05/m3_05_pilot/dvc.yaml:m3_05_a5_reconstruction
just provenance-finalize docs/data/m5_05/m3_05_pilot/summary.json
```

The driver uses the historical public API, emits the existing scale record families plus `r`, A-5 medians/MADs and parity digests, and writes raw JSONL into the DVC stage. Its output covers 16 paired full-tick configurations, 16 phase-profile configurations, and 16 one-time graph/projection series (eight configurations times two construction measurements): 48 A-5 series. The lower-bound runtime is **2,064 seconds (34 minutes 24 seconds)**; compilation, initialization, checkpoint construction, memory probes, and `r` above the one-second floor add time.

The historical `cargo bench -p sim-model --bench m3_scale_limit_bench` command uses seven samples, not A-5; it remains unchanged. The separate driver runs the frozen 2-warmup/41-sample protocol at the report commit. The criteria are frozen, but this does not authorize the A-5 pilot in this task.

## Later update (2026-10-26)

The M3-09 protocol-matched follow-up revision is `681bf296ea07d3ddc4391f5f1dfd49255876300c`, not `c857d51517bf8df2ff5f50c6cb035585fc4b549e` as stated in the historical paragraph above. The old `docs/reproducibility/m5_05_conclusions.toml` is superseded by the draft `contracts/conclusions.toml`; its 13 `verbatim_conclusion` and `pass_criterion` values are carried into the new registry's `verbatim` and `pass_criterion` fields. The old file remains historical evidence and is not edited.
