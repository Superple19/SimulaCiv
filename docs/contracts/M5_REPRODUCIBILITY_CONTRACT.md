# M5 Reproducibility and Research Hardening Contract

**Status: DRAFT - NOT FROZEN**
**Baseline: `v0.5.0-m4`**

## 1. Research question

> Can every result this project has reported be regenerated from committed code and data, and which findings survive multi-seed, multi-workload testing?

M5 is a research-hardening milestone. It audits the provenance and reproducibility of existing claims before extending the simulation model. The audit must distinguish exact replay from timing remeasurement and must record missing evidence rather than reconstructing it from memory.

## 2. Definitions

- **Reproducible:** the documented inputs, committed source, and declared environment are sufficient to run the stated procedure and recover the reported artifact or result.
- **Bit-exact:** deterministic outputs match exactly at the declared comparison boundary, including canonical values, bytes, hashes, or ordered records. It is a stronger property than merely reproducing a similar result.
- **Re-measured:** a timing or other noisy measurement is repeated with a declared protocol. It is not expected to be bit-identical; report the measurement distribution and compare against a tolerance declared before the run.
- **Unrecoverable:** required source, raw data, inputs, or environment evidence is unavailable and cannot be independently reconstructed. Mark the result and its limits; never fabricate missing values or claim an unperformed rerun.

## 3. Current reproducibility state (measured 2026-10-21)

The following sizes are current on-disk file sizes, measured from the checked-out repository. They are inventory facts, not a proposed data-size limit.

| `docs/data` directory | Files | Bytes | Approximate size |
|---|---:|---:|---:|
| `m4` | 37 | 2,029,282 | 1.94 MiB |
| `m4_06` | 14 | 28,770,278 | 27.44 MiB |
| `m4_07` | 10 | 201,323 | 196.6 KiB |
| `m4_viewer` | 1 | 130,570 | 127.5 KiB |
| **Total** | **62** | **31,131,453** | **29.69 MiB** |

The measured `.git` directory size was **12,877,787 bytes**. This is a point-in-time size; commits, packs, and garbage collection change it. The inventory and stage files are listed in [`docs/data/m4/INVENTORY.md`](../data/m4/INVENTORY.md) and [`docs/data/m4_06/README.md`](../data/m4_06/README.md).

Three M4-06 JSON files are individually large: `experiment.json` (11,297,013 bytes), `branch_experiment.json` (8,706,720 bytes), and `experiment_superseded_seed_set.json` (8,675,386 bytes). The last is superseded history, not a current report input. Other smaller superseded A-5 runs are also explicitly identified in the M4-06 data README. Already-pushed data stays in history; do not rewrite history to shrink it.

### Restored measurement sources

The following examples carry the header `restored from session history; not re-run`; their source compiles, but their results were not regenerated from those files:

- M4-02: `m4_02_a2_measure.rs`, `m4_02_a3_measure.rs`, `m4_02_a4_measure.rs`, `m4_02_a5_measure.rs`.
- M4-03: `m4_03_experiment_temp.rs`, `m4_03_gate_temp.rs`.
- M4-04: `m4_04_crn_experiment.rs`.
- M4-05: `m4_05_a5_gate.rs`, `m4_05_experiment.rs`.

The `_temp` names identify historical M4-03 drivers. The artifact inventory also records sources that were only partially recovered or not recovered, including M4-02 initial/A-1 and follow-up helpers and the M4-05 follow-up helper. A compiled binary is not a substitute for its source. See the [M4 artifact inventory](../data/m4/INVENTORY.md).

### Removed probes and automation coverage

Several M2/M3 reports rely on temporary timing or allocation probes that were removed after measurement: [M2-31](../performance/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md), [M2-37](../performance/M2_37_POST_P5_P6B_REPROFILING.md), [M2-40](../performance/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md), [M3-01](../performance/M3_01_SERIAL_BASELINE.md), [M3-08](../performance/M3_08_POPULATION_DYNAMICS.md), and [M3-09](../performance/M3_09_INCREMENTAL_TOPOLOGY.md). The reports preserve their summarized results and identify probe limitations; some raw measurements and original instrumentation cannot be regenerated exactly.

The repository has one tracked `AGENTS.md` at its root and no nested agent-rule files. The only workflow in `.github/workflows/` is `ci.yml`, which runs `cargo fmt --check`, workspace Clippy, and workspace tests on Linux, Windows, and macOS; it has no Python or benchmark step. There is one tracked Python script, [`scripts/profile_m0_baseline.py`](../../scripts/profile_m0_baseline.py). The temporary analysis scripts under `target/` are not tracked project tooling.

## 4. Data-size policy — blocking decision

Choose and freeze a data policy in M5-00 before adding substantial new raw output. Options:

| Option | Policy | Costs and trade-offs |
|---|---|---|
| **A. Compressed cap plus external full data** | Commit summaries and per-file compressed raw data up to a declared cap (for example, 1 MiB); publish larger full outputs as GitHub release assets with checksums. | Keeps normal repository fetches smaller and retains access to full measurements, but requires asset/checksum maintenance and an approved release-asset workflow. |
| **B. Git LFS** | Keep large outputs as versioned LFS objects. | Preserves file-oriented workflows, but adds LFS setup, bandwidth/storage quotas, CI configuration, and clone behavior that contributors must understand. Existing Git history is not rewritten. |
| **C. Regenerate on demand** | Commit experiment specifications and checksums, then regenerate data when needed. | Avoids storing most raw output, but timing results are not bit-exact; it cannot recover missing historical inputs or probes, and an unavailable result remains unrecoverable. |

**Recommendation (proposal, not frozen): Option A.** It preserves full measurement artifacts while bounding repository growth. Its main cost is a dependable release-asset and checksum process; M5-00 must decide whether that process is acceptable. Keep already-pushed large files and their history unchanged under every option.

## 5. M5 stage plan (PLANNED — each stage needs its own DRAFT → review → FROZEN contract)

| Stage | Planned scope |
|---|---|
| **M5-00 — Contract and data policy** | Freeze reproducibility definitions, the selected data-size policy, provenance requirements, and acceptance gates. |
| **M5-01 — Reproducibility inventory** | Inventory every numeric claim in M0–M4 reports; map each to its harness, raw data, status, and classification as deterministic or timing/noisy. Mark unrecoverable evidence explicitly. |
| **M5-02 — Python experiment interface** | Add PyO3 bindings and a Python experiment runner with commit hash, configuration hash, and host provenance. Freeze Python package/version policy first. |
| **M5-03 — Restore and regenerate M4 evidence** | Re-run the restored M4 harnesses and frozen experiment specifications; compare bit-exact outputs where deterministic and rename the restored `*_temp` examples without deleting them. |
| **M5-04 — M0–M3 tables** | Regenerate or re-measure the key M0–M3 tables, preserving old values as history and labeling re-measurements with their protocol and tolerance. |
| **M5-05 — Robustness experiments** | Run multi-seed and multi-workload, effect-size-first experiments, including migration calibration and a redesigned M4-05 predicate. Select the matrix before execution. |
| **M5-06 — Documentation and generated figures** | Align naming, make reports summary-first, split contracts by stage, create a newcomer README, and generate figures from committed data. |
| **M5-07 — Literature and technical report** | Review related work, position SimulaCiv, and draft a technical report from reproducible evidence. |
| **M5 audit** | Assess which M0–M4 findings reproduce across code, data, seeds, workloads, and timing tolerances; classify unresolved evidence. |

## 6. Known inputs

- [`docs/data/m4/INVENTORY.md`](../data/m4/INVENTORY.md) records recovered and missing M4 harness/data pairs.
- The [M4 final audit](../performance/M4_FINAL_AUDIT_AND_GRADUATION.md) records wiring debt, uncalibrated migration settings, the M4-05 boundary predicate, compaction money loss, newborn CRN loss, and single-host/workload limits.
- The M5-01 inventory must extend these inputs to every numeric claim in M0–M4 rather than treating this draft as a complete claim list.

## 7. Non-goals

- No semantic changes to existing profiles and no new simulation features as part of M5 reproducibility work.
- Inverse inference, distributed determinism, and learning agents remain later candidates.

## 8. Open questions

These are blocking for the corresponding freeze or gate; this DRAFT does not decide them:

- **Python packaging and version policy:** package layout, supported Python versions, and compatibility window (blocking for M5-02).
- **Python CI scope:** which bindings and experiment-runner checks are required on all three operating systems (blocking for M5-02 freeze).
- **Timing tolerance:** comparison statistic, host matching, and threshold for re-measured claims (blocking for M5-00/M5-01).
- **Seed/workload matrix size:** number and selection method for seed and workload variation in M5-05 (blocking for M5-05).
- **Publication target:** JOSS, an arXiv technical report, or artifact evaluation (non-blocking for M5-00; decide before M5-07).
