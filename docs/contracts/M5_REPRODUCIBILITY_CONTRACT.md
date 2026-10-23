# M5 Reproducibility and Research Hardening Contract

**Status: M5-00, M5-02, M5-03, M5-04, and M5-05 FROZEN; M5-01 COMPLETE; M5-03 and M5-04 implementation COMPLETE**
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

The measured `.git` directory size was **12,877,787 bytes**. This is a point-in-time size; commits, packs, and garbage collection change it. The inventory and stage files are listed in [`docs/data/m4_inventory/README.md`](../data/m4_inventory/README.md) and [`docs/data/m4_06/README.md`](../data/m4_06/README.md).

Three M4-06 JSON files are individually large: `experiment.json` (11,297,013 bytes), `branch_experiment.json` (8,706,720 bytes), and `superseded/experiment_seed_set.json` (8,675,386 bytes). The last is superseded history, not a current report input. Other smaller superseded A-5 runs are also explicitly identified in the M4-06 data README. Already-pushed data stays in history; do not rewrite history to shrink it.

### Restored measurement sources

The following examples carry the header `restored from session history; not re-run`; their source compiles, but their results were not regenerated from those files:

- M4-02: `m4_02_a2_measure.rs`, `m4_02_a3_measure.rs`, `m4_02_a4_measure.rs`, `m4_02_a5_measure.rs`.
- M4-03: `m4_03_experiment.rs`, `m4_03_gate.rs`.
- M4-04: `m4_04_crn_experiment.rs`.
- M4-05: `m4_05_a5_gate.rs`, `m4_05_experiment.rs`.

The `_temp` names identify historical M4-03 drivers. The artifact inventory also records sources that were only partially recovered or not recovered, including M4-02 initial/A-1 and follow-up helpers and the M4-05 follow-up helper. A compiled binary is not a substitute for its source. See the [M4 artifact inventory](../data/m4_inventory/README.md).

### Removed probes and automation coverage

Several M2/M3 reports rely on temporary timing or allocation probes that were removed after measurement: [M2-31](../reports/M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md), [M2-37](../reports/M2_37_POST_P5_P6B_REPROFILING.md), [M2-40](../reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md), [M3-01](../reports/M3_01_SERIAL_BASELINE.md), [M3-08](../reports/M3_08_POPULATION_DYNAMICS.md), and [M3-09](../reports/M3_09_INCREMENTAL_TOPOLOGY.md). The reports preserve their summarized results and identify probe limitations; some raw measurements and original instrumentation cannot be regenerated exactly.

The repository has one tracked `AGENTS.md` at its root and no nested agent-rule files. The only workflow in `.github/workflows/` is `ci.yml`, which runs `cargo fmt --check`, workspace Clippy, and workspace tests on Linux, Windows, and macOS; it has no Python or benchmark step. There is one tracked Python script, [`analysis/profile_m0_baseline.py`](../../analysis/simciv_analysis/profile_m0_baseline.py). The temporary analysis scripts under `target/` are not tracked project tooling.

Later update (2026-10-23): the profiler was moved into `analysis/simciv_analysis/profile_m0_baseline.py`; its import path was checked without running the benchmark.

## 4. Data-size policy (FROZEN)

M5 selects Option A: every committed data file is at most **1 MiB**. Gzip-compressed files (for example, `.json.gz`) count by their committed compressed size. Larger raw outputs are published as GitHub release assets; the repository keeps a `docs/data/<stage>/MANIFEST.json` record with each asset's file name, byte count, SHA-256, producing harness path, exact command, and intended release tag. Agents prepare asset files and the manifest under `target/release-assets/<tag>/`; the user uploads the assets. Do not create a release or upload assets on the user's behalf. Already-pushed files stay in Git history; do not rewrite history.

The manifest is committed even when its referenced large asset is not. Any table using an external asset must cite both the harness and the manifest entry. Missing or unavailable assets are marked unrecoverable, not synthesized.

Later update (2026-10-23): M5-00 A-1 in §14 supersedes Option A for new experiment and measurement outputs. Existing committed data remains in Git; new raw outputs use DVC.

## 5. M5 stage plan (PLANNED — each stage needs its own DRAFT → review → FROZEN contract)

The order is structure → tooling → verification → new experiments → cleanup → publication.

| Stage | Planned scope |
|---|---|
| **M5-00 — Contract and data policy** | **COMPLETE.** Freeze reproducibility definitions, the selected data-size policy, provenance requirements, and acceptance gates. |
| **M5-01 — Reproducibility inventory** | **COMPLETE.** Inventory every numeric claim in M0–M4 reports and measured contract statements; map claim values to harnesses, raw data, status, class, and next stage. |
| **M5-02 — Repository layout** | **FROZEN; implementation in progress.** Organize contracts, reports, data, reproducibility outputs, and analysis tools under the frozen layout in §10. |
| **M5-03 — Analysis toolchain** | **FROZEN; implementation COMPLETE.** Add the pinned `uv` project, `pandas`, `matplotlib`, Quarto, `just`, `pytest`, deterministic report generation, link checking, and the approved Ubuntu analysis CI job. Regenerate M5-01 as a tidy one-row-per-claim gzip ledger, exclude configuration/input values, add key/supporting tiers, and recheck the 29 M1 claims currently labeled unrecoverable. |
| **M5-04 — M4 regeneration** | **FROZEN; implementation COMPLETE.** Re-run existing Rust harnesses against committed M4 data, verify deterministic outputs bit-exactly, remeasure conclusion-bearing M4 timings, and produce a generated Quarto report; see [M5-04 regeneration](../reproducibility/M5_04_M4_REGENERATION.md). |
| **M5-05 — M0–M3 regeneration and re-measurement** | **FROZEN.** Regenerate deterministic M2/M3 headline claims and re-measure conclusion-bearing timings only; the cell plan and budget are in §13. |
| **M5-06 — Python bindings** | Add PyO3/maturin bindings and an experiment runner with commit, configuration, and host provenance. |
| **M5-07 — Robustness experiments** | Run multi-seed and multi-workload, effect-size-first experiments, including migration calibration and a redesigned M4-05 predicate. Select the matrix from a pilot before execution. |
| **M5-08 — Documentation content** | Align naming, make reports summary-first, split contracts by stage, create a newcomer README, and generate figures from committed data. |
| **M5-09 — Literature and technical report** | Review related work, position SimulaCiv, and draft a Quarto technical report rendered to PDF. |
| **M5 audit** | Assess which M0–M4 findings reproduce across code, data, seeds, workloads, and timing tolerances; classify unresolved evidence. |

## 6. Known inputs

- [`docs/data/m4_inventory/README.md`](../data/m4_inventory/README.md) records recovered and missing M4 harness/data pairs.
- The [M4 final audit](../reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md) records wiring debt, uncalibrated migration settings, the M4-05 boundary predicate, compaction money loss, newborn CRN loss, and single-host/workload limits.
- The M5-01 inventory must extend these inputs to every numeric claim in M0–M4 rather than treating this draft as a complete claim list.

## 7. Non-goals

- No semantic changes to existing profiles and no new simulation features as part of M5 reproducibility work.
- Inverse inference, distributed determinism, and learning agents remain later candidates.

## 8. M5-00 decision register (FROZEN)

| ID | Decision | Frozen rule |
|---|---|---|
| D-1 | Data size and provenance | Select Option A in §4: committed data files are at most 1 MiB; larger outputs use manifest-backed release assets prepared under `target/release-assets/<tag>/` for user upload. Keep already-pushed files and history unchanged. |
| D-2 | Timing claims | Judge conclusions using the A-5 protocol on the reference host: the current Windows desktop with AMD Ryzen 5 9600X. Record new/old ratio for every absolute timing; flag `abs(ratio - 1) > 20%`. Gate PASS, ordering, and crossover-range conclusions are assessed on that host. Other hosts are supplementary evidence. |
| D-3 | Deterministic claims | Require bit-exact reproduction. If source, inputs, or data do not permit it, label the claim **unrecoverable**; do not fabricate or approximate a deterministic result. |
| D-4 | Python interface | Create a new `sim-py` crate using maturin and PyO3, supporting Python >=3.11. The experiment-runner core uses only the Python standard library; numpy, pandas, and matplotlib are optional analysis extras. The Rust core dependency set stays unchanged. |
| D-5 | Python CI boundary | The planned smoke matrix is Python 3.12 on Linux, Windows, and macOS, building the package and running a smoke test. Changing Python CI requires explicit approval in the M5-06 freeze; the analysis-check CI proposal requires separate explicit approval in the M5-03 freeze. |
| D-6 | Robustness matrix starting point | M5-07 begins from at least 10 seeds × 3 workloads: M3P sustained, M3 equilibrium, and calibrated migration. A pilot effect-size run at M5-07 freezes the actual matrix before the full run. |
| D-7 | Publication direction | Draft an arXiv technical report first; consider JOSS after M5. |
| D-8 | M5 acceptance | Classify every numeric claim. Deterministic claims must be bit-exact or explicitly unrecoverable. Timing conclusions must hold under D-2 or the change must be documented as a changed finding. |

The exact M5-07 seed/workload selection remains a blocking decision for that stage and is made from the pilot. Python CI changes remain unauthorized until explicitly approved in the M5-06 freeze. The proposed analysis-check CI job requires separate explicit approval in the M5-03 freeze.

## 9. M5-00 freeze record

This freeze selects the data policy, evidence-manifest requirements, reference-host timing comparison, deterministic-claim rule, Python interface plan, Python CI approval boundary, robustness-matrix starting point, publication direction, and M5 acceptance conditions. It authorizes M5-01 inventory work; it does not authorize implementation of Python bindings or changes to CI.

## 10. M5-02 repository layout (FROZEN)

M5-02 freezes repository organization only. It does not change simulation semantics, code behavior, tests, goldens, snapshots, or CI.

### 10.1 Target layout

```text
contracts/                machine-readable manifest used by tests; stays at the repository root
crates/                   Rust code
docs/
├─ README.md               documentation map
├─ design/                 design specification
├─ contracts/              M1–M5 contracts and index
├─ reports/                milestone reports and index
├─ data/<stage>/           flat per-stage data, each with a README and superseded/ where needed
└─ reproducibility/        M5 inventory outputs
analysis/                  analysis tools; initially check_links.py and, if needed, legacy/
viewer/                    static viewer
```

### 10.2 Organization rules

- Keep `contracts/m1_contract.toml` at the repository root because tests consume that path.
- Use no global `legacy/` folder. Organize legacy files by milestone and stage.
- Keep superseded data, runs, and reports inside their stage folder under `superseded/`, with a README line identifying what superseded them.
- Every report begins with exactly one status line: `Status: current`, `Status: superseded by <link>`, or `Status: historical (<milestone>)`.
- Use `analysis/legacy/` only for tools that no longer run or are no longer maintained. Keep `analysis/profile_m0_baseline.py` in `analysis/` if it still runs; otherwise place it in `analysis/legacy/` with a note.
- Delete a file only if no report, test, document, or build references it. Never delete a cited harness or data file.
- Name numbered reports `M<m>_<nn>_<SLUG>.md`; use `M<m>_X_<SLUG>.md` for unnumbered analyses, diagnoses, and workloads, and `M<m>_AUDIT_<SLUG>.md` for interim/final audits. For M2 analyses, use the stage number in the report's own milestone line when present.

### 10.3 Decision register (FROZEN)

| ID | Decision | Frozen rule |
|---|---|---|
| D-1 | Repository roots | Keep `contracts/m1_contract.toml` at root; use the target tree in §10.1 for all other listed content. |
| D-2 | Report organization | Move reports to `docs/reports/`, use stage-based names, and give each report exactly one approved status header. Preserve historical body text except mechanical path/name updates and dated Later update notes. |
| D-3 | Data organization | Store data in flat `docs/data/<stage>/` folders, add a README to each, and put superseded items in that stage's `superseded/` folder with a README note. Follow the frozen M5-00 size policy. |
| D-4 | Analysis tools | Use `analysis/` for the link checker and maintained analysis tools. Reserve `analysis/legacy/` for tools that no longer run or are no longer maintained. The analysis toolchain itself is M5-03. |
| D-5 | Preservation | Keep cited harnesses/data and historical report wording. Remove no unreferenced item unless its references have been checked; record any deletion and reason in `docs/reports/RENAMES.md`. |
| D-6 | M5-03 claim work | Commit the extraction script, make a tidy claim ledger with configuration/input values excluded, and add key/supporting classification in M5-03. Recheck the M1 unrecoverable set there. |

### 10.4 Freeze record

This freeze authorizes the M5-02 repository moves, status headers, link repairs, and navigation indexes. It does not authorize Python bindings, an analysis CI change, simulation changes, or new experiments.

### 10.5 Validation record

M5-02 validation completed:

- `cargo fmt --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo test --workspace` — passed.
- `python analysis/check_links.py` — the Windows `python.exe` app-execution alias returned exit code 1 without starting Python. The same check passed with `py -3 analysis/check_links.py`: 719 relative Markdown links checked, 0 broken.
- `git diff --check` — passed.

## 11. M5-03 analysis toolchain (FROZEN)

M5-03 freezes the analysis environment, deterministic report-generation rules, command surface, and the analysis CI job. It does not change the Rust core dependency set or simulation behavior.

### 11.1 Version pins selected on 2026-10-23

At setup, the latest stable release was selected from each project's official release page or registry. The Python requirement remains `>=3.11` per §8 D-4; development and CI use the latest stable Python pinned below. Quarto prereleases are excluded.

| Tool | Exact pin or action major | Latest stable release checked | Release date | Official source |
|---|---|---|---|---|
| Python | `3.15.1` in `.python-version` | `3.15.1` | 2026-10-21 | [Python.org](https://www.python.org/downloads/release/python-3151/) |
| uv | `0.13.5` in `uv.toml` | `0.13.5` | 2026-10-21 | [uv releases](https://github.com/astral-sh/uv/releases/tag/0.13.5) |
| pandas | `3.0.6` in `uv.lock` | `3.0.6` | 2026-09-17 | [PyPI](https://pypi.org/project/pandas/3.0.6/) |
| matplotlib | `3.11.2` in `uv.lock` | `3.11.2` | 2026-09-11 | [PyPI](https://pypi.org/project/matplotlib/3.11.2/) |
| pytest | `9.1.1` in `uv.lock` | `9.1.1` | 2026-06-19 | [PyPI](https://pypi.org/project/pytest/9.1.1/) |
| Quarto CLI | `1.10.19` | `1.10.19` stable; `1.11.6` is prerelease | 2026-10-06 | [Quarto CLI releases](https://github.com/quarto-dev/quarto-cli/releases/tag/v1.10.19) |
| `actions/checkout` | `@v7` | `v7.0.1` | 2026-07-20 | [checkout releases](https://github.com/actions/checkout/releases/tag/v7.0.1) |
| `astral-sh/setup-uv` | `@v10` | `v10.3.2` | 2026-10-22 | [setup-uv releases](https://github.com/astral-sh/setup-uv/releases) |
| `quarto-dev/quarto-actions/setup` | `@v2` | `v2.2.0` | 2025-11-25 | [Quarto Actions releases](https://github.com/quarto-dev/quarto-actions/releases/tag/v2.2.0) |
| `extractions/setup-just` | `@v4` | `v4` / `4.0.0` | 2026-04-05 | [setup-just releases](https://github.com/extractions/setup-just/releases) |
| just CLI | `1.58.0` | `1.58.0` | 2026-08-03 | [just releases](https://github.com/casey/just/releases/tag/1.58.0) |

All selected versions are latest stable as checked on the date above; no compatibility downgrade was needed. Later upgrades require a separate pin-update commit and a successful `just check`.

### 11.2 Frozen toolchain and commands

- `analysis/` is a uv project with committed `pyproject.toml` and `uv.lock`; `requires-python = ">=3.11"`; direct dependencies are limited to pandas, matplotlib, and pytest. Additional direct dependencies require a recorded reason in the change that adds them.
- `.python-version` pins development and CI to Python `3.15.1`. `uv.lock` pins Python libraries and their transitive dependencies. The Quarto CLI is pinned to `1.10.19` and documented in `analysis/README.md` and CI.
- The repository-root `justfile` provides `analysis-sync`, `analysis-test`, `links`, `claims`, `report-m5-01`, and `check`. `check` performs all analysis checks without rewriting committed outputs: it checks links and claim/report regeneration, runs pytest, and renders report sources to a temporary directory before byte-comparing generated files with the committed `.qmd`/`.md` outputs.
- Quarto renders `.qmd` as GitHub-flavored Markdown. Both source and rendered Markdown are committed. Reports contain no render timestamps. Matplotlib uses a fixed style and the Agg backend; SVG output sets `svg.hashsalt` and omits Date metadata. Repeated renders of the same figure must be byte-identical.
- The approved analysis CI job runs on `ubuntu-latest`, installs the pinned uv and Quarto versions plus just, then runs `just check`. The existing Rust job remains unchanged. CI actions are pinned by major version as listed in §11.1.

### 11.3 Frozen claim and report workflow

- `analysis/simciv_analysis/claims.py` is the committed M5-01 extraction source. It scans report documents and contracts and emits a tidy one-row-per-claim `docs/reproducibility/claims.csv.gz`; the old packed ledger is retained under `docs/reproducibility/superseded/`.
- Input/configuration values (including population lists, seeds, and parameter settings) are excluded by the extractor and documented in its generated report. Claims carry a `key` or `supporting` tier. Old status/harness metadata is carried only on exact `(report, line, value)` matches; unmatched claims are `unmapped` and are not guessed.
- The M1 set of 29 claims formerly labeled unrecoverable is rechecked against the frozen M1 contract, tests, and goldens. The report records the reason for each classification.
- `docs/reproducibility/M5_01_INVENTORY.qmd` and its rendered `.md` are generated from the tidy ledger. Their counts, comparisons, key-claim list, M1 recheck table, and gap list are computed from the ledger, not hand-entered.

### 11.4 M5-03 decision register (FROZEN)

| ID | Decision | Frozen rule |
|---|---|---|
| D-1 | Version policy | Use the latest stable versions listed in §11.1, pin Python via `.python-version`, libraries via `uv.lock`, Quarto exactly, and GitHub Actions by major. Record upgrades in a separate commit and rerun `just check`. |
| D-2 | Python project | Use `analysis/` as a uv project, `requires-python >=3.11`, and only pandas, matplotlib, and pytest as direct dependencies. |
| D-3 | Deterministic reports | Commit `.qmd` and rendered GFM `.md`; suppress timestamps and stabilize Matplotlib SVG metadata/style/hash salt. `check` rerenders to temporary storage and fails on byte differences without rewriting tracked files. |
| D-4 | Task interface | Provide the six named just targets. `check` covers sync/lock validation, tests, link checks, claim regeneration checks, and report-render comparisons without modifying committed artifacts. |
| D-5 | Analysis CI | Add a separate `analysis` job on `ubuntu-latest` using uv, pinned Quarto, and `just check`; leave the Rust job unchanged. |
| D-6 | Claim ledger | Generate a tidy gzip ledger; exclude input/config values; add tiers; transfer old metadata only by exact report/line/value match; classify all nonmatches as `unmapped`. Recheck the 29 M1 unrecoverable claims with explicit reasons. |

### 11.5 Freeze record

This freeze authorizes the M5-03 analysis environment, report generator, claim-ledger regeneration, and the separate Ubuntu analysis CI job described above. It does not authorize simulation runs, Python bindings, or changes to simulation semantics.

### 11.6 Implementation and validation record

M5-03 implementation completed with the pins in §11.1. The M5-01 inventory is generated from the committed tidy ledger; the old packed ledger and hand-written report remain under `docs/reproducibility/superseded/` for comparison. The new CI workflow has a separate Ubuntu analysis job; hosted CI has not yet run for this commit.

- `just check` — passed: 9 analysis tests; 736 relative Markdown links; claim ledger byte check; temporary Quarto render byte comparison.
- `cargo fmt --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo test --workspace` — passed.
- `git diff --check` — passed.
- No benchmark or experiment harness was run.

Later update (2026-10-23): the M5-01 carry-over now maps renamed report basenames through `docs/reports/RENAMES.md`, matches normalized source text and value with line number only as a tie-breaker, adds a `headline` tier, and classifies the 29 M1 aggregate run records as `historical-log`. See the [M5-01 inventory](../reproducibility/M5_01_INVENTORY.md). The M5-03 rules above record the original freeze-era implementation.

## 12. M5-04 M4 regeneration (FROZEN)

M5-04 reproduces the committed M4 evidence without changing simulation semantics, existing tests, goldens, snapshots, or CI. A mismatch is recorded as a finding and is never patched to match historical output.

### 12.1 Deterministic outputs

- Re-run the committed or restored M4 harnesses against the same declared inputs and compare deterministic outputs byte-for-byte. Where the artifact mixes deterministic and timing fields, compare deterministic fields exactly and exclude only the explicitly noisy timing fields.
- Run in this order: M4-06 and M4-07 harness/data pairs; M4-03, M4-04, and M4-05 harnesses against their committed raw records; the M4-02, M4-05, and M4-06 A-5 timing gates; then the viewer exporter sample, compared bit-exactly as JSONL.
- The bit-identical M4-07 parallel mode may be used to shorten deterministic reruns. Each result records whether serial or parallel execution was used.
- A mismatch, missing source, or missing historical raw artifact is reported with a focused diff and classified; no expected value is re-blessed.

### 12.2 Timing claims

- Re-measure only the final A-5 gates and conclusion-bearing M4 timings on the M5-00 reference host: the Windows desktop with AMD Ryzen 5 9600X. A-1 through A-4 remain historical and are not rerun.
- Re-run the nine-cell M4-02 A-5 gate with fresh samples, plus the M4-05 and M4-06 A-5 gates. Follow the frozen A-5 protocol and record the exact command, execution mode, host conditions, and raw samples.
- For every absolute timing, report the new/old ratio and flag `abs(ratio - 1) > 20%`. Judge the claim by whether its stated conclusion still holds; report any changed conclusion instead of silently retaining the old wording.
- Emit `TIMING RUN START` and `TIMING RUN END` markers in the run log. Keep the reference PC idle between timing runs.

### 12.3 Provenance and output handling

- After a successful rerun, replace that harness's `restored from session history; not re-run` header with `re-run in M5-04: <result>`. Failed, partial, or not-yet-run harnesses keep their historical label.
- Store new outputs under `docs/data/m5_04/<stage>/`. Follow M5-00 §4: committed files are at most 1 MiB; for larger outputs, prepare the asset under `target/release-assets/m5-04/` and commit a manifest entry with the filename, bytes, SHA-256, harness, command, and intended release tag. Do not upload assets.
- Commit each stage's regenerated data before starting the next stage run.
- The generated report is `docs/reproducibility/M5_04_M4_REGENERATION.qmd` and its rendered GFM `.md`. `analysis/simciv_analysis/report_m5_04.py` computes the claim-result table, timing ratios, mismatch diffs, figures, and total compute time from the ledger, prior reports, harness outputs, and committed data. Hand-entered measured values are not allowed.

### 12.4 Acceptance gate

Every M4 headline claim is classified as one of: **bit-exact**, **conclusion holds**, **mismatch**, or **unrecoverable**. Deterministic claims require exact equality or an explicit mismatch/unrecoverable finding. Timing claims require a conclusion-holds or documented changed-conclusion result under the reference-host A-5 protocol. The report links every result to its harness and raw data or manifest.

### 12.5 M5-04 decision register (FROZEN)

| ID | Decision | Frozen rule |
|---|---|---|
| D-1 | Deterministic reruns | Compare bytes or deterministic fields exactly; any mismatch is reported, never patched to match. |
| D-2 | Timing reruns | Re-measure only final A-5 gates and conclusion-bearing timings on the reference host; report new/old ratios and flag deviations over 20%; A-1 through A-4 stay historical. |
| D-3 | Execution mode | M4-07 parallel execution is allowed only for deterministic reruns already proven bit-identical; record the mode used. |
| D-4 | Harness provenance | Change the restored/not-rerun header only after a successful rerun and record its result. |
| D-5 | Data and report | Keep outputs under `docs/data/m5_04/`; apply the 1 MiB policy and manifest-backed release-asset process; generate the report from committed harnesses and data. |
| D-6 | Acceptance | Classify every M4 headline claim as bit-exact, conclusion holds, mismatch, or unrecoverable. |

### 12.6 Freeze record

This freeze authorizes M5-04 reruns of the committed M4 harnesses, fresh A-5 timing samples, data/manifest commits, harness-header updates after successful reruns, and the generated M5-04 report. It authorizes no simulation semantic changes, edits to existing tests or expected outputs, or CI changes.

### 12.7 Implementation completion record

M5-04 is **COMPLETE**. The generated [M4 headline result table](../reproducibility/M5_04_M4_REGENERATION.md#m4-headline-claims) classifies every extracted M4 headline claim. Deterministic mismatches and unrecoverable artifacts are recorded without changing historical outputs. The final A-5 rerun passed for M4-03, M4-05, and M4-06; M4-02 incremental M3P cells changed from PASS to FAIL and are documented. The committed M5-04 data satisfies the size policy, so no release assets or manifest were prepared.

- `just report-m5-04` — generated the comparison data, M5-04 report, updated claim ledger, and regenerated M5-01 inventory.
- `just check` — passed; see the final task record for the exact validation output.

## 13. M5-05 M0–M3 regeneration (FROZEN)

M5-05 covers M2 and M3 headline claims in [`claims.csv.gz`](../reproducibility/claims.csv.gz). It does not make semantic changes or alter existing expected outputs. Supporting claims are re-measured only when the same selected run produces them without additional cells.

### 13.1 Deterministic headline claims

- Confirm exact State, Metrics, Events, intent, resolution, error, and snapshot claims through the integration tests and goldens named by the report/source map. Record the exact test target names and the result for each claim.
- The source map currently names these CI integration targets for deterministic claims: `determinism_oracle_tests`, `m2_soa_gate_tests`, `phase3_features_tests`, `phase4_decision_tests`, `phase4_intent_tests`, `phase4_rayon_tests`, `phase5_partition_tests`, `phase6a_work_tests`, `phase6b_targeted_tests`, `m3_search_reduction_tests`, and `m3_settlement_index_tests`.
- Use the committed report's golden or expected value unchanged. A mismatch is recorded as a finding; never update the expected value to match.
- If a report does not name a runnable integration target or committed golden for a deterministic headline claim, classify it as **unrecoverable** rather than inferring a test.

### 13.2 Timing headline claims

- Re-run only cited benchmark cells that support M2/M3 headline timing claims. Supporting measurements are retained when naturally produced by those selected cells; no unrelated sweep rows are run.
- Apply the M5-00 reference host and A-5 sampling protocol. Each paired comparison must start from the same checkpoint and assert matching workload, execution mode, pool size, chunk size, and all other options. The M4-02/M5-04 mode mismatch must not recur.
- For each conclusion-bearing comparison, record the old and new values, new/old ratio for absolute timings, MADs, and whether the conclusion still holds. Flag every absolute timing with `|ratio - 1| > 20%`.
- Preserve old values and reports as historical evidence. A changed conclusion is reported, never rewritten to agree with the earlier result.

### 13.3 Planned cell set

The exact cell identity includes the benchmark, complete workload/configuration key, checkpoint, runner or comparison paths, and parallel options. The M2 rows are deduplicated by that full key across reports. Before any run, commit a cell manifest containing the expanded M2 keys copied from the headline source rows; do not infer missing parameters. The planned cells are:

| Report family | Planned headline cells |
|---|---|
| M2 baseline and optimization reports | `m0_baseline_bench.rs` rows tied to headline timing claims in M2-06, M2-08, M2-09, M2-12, M2-16.1, M2-18, M2-31, M2-34, M2-37, M2-38, M2-40, and M2-X; exact row keys are copied from those reports and deduplicated before execution. M2-07, M2-13, M2-14, M2-17, M2-18.1, and M2-39 are analysis-only and excluded. |
| M3-01 serial baseline | 45 cells: `N ∈ {10k,50k,100k}`, `K=N/200`, `k ∈ {4,8,16}`, `p_local ∈ {0,0.25,0.5,0.75,1}`, day 0. |
| M3-02 parallel Phase 6B | 180 active-workload cells behind the `0.967×` headline median: `N ∈ {10k,50k,100k}`, `K=N/200`, `k=8`, `p_local ∈ {0,0.5,1}`, day 0, pool sizes `{1,2,4,6}`, chunks `{64,128,256,512,1024}`. The day-100 cells are not run for the headline. |
| M3-03 search reduction | 12 whole-tick pairs: `N ∈ {10k,100k}`, `K=N/200`, `k=8`, `p_local ∈ {0,0.5,1}`, checkpoints day 0 and day 150; compare serial M3 with the explicit search-reduction path. |
| M3-04 sustained workload | 36 whole-tick pairs: the same `N`, `K`, and `p_local` values as M3-03, checkpoints day 0/day 150, and pool sizes `{1,4,6}` at chunk 256 against serial. Re-run the 52 N=10k equilibrium candidates plus the three N=100k locality checks because the selected equilibrium point is the report's headline result. |
| M3-05 scale limit | 16 whole-tick pairs: `N ∈ {100k,250k,500k,1M}`, `K=N/200`, `k ∈ {8,16}`, `p_local=0.5`, checkpoints day 0/day 50; serial versus pool 6/chunk 256. |
| M3-06 settlement index | 11 scaling cells: fixed `K=500`, `N ∈ {100k,250k,500k,1M}`; fixed `N=250k`, `K ∈ {250,500,1250,2500,5000}`; proportional `K=N/200`, `N ∈ {100k,250k,500k,1M}`, deduplicated by full key. Also include Phase 3 before/after subphase comparisons at `N ∈ {250k,1M}` and the reported N=1M whole-tick serial/parallel pair. |
| M3-07 residual scaling | `N ∈ {25k,50k,100k,150k,200k,300k,400k,500k,750k,1M}`, `K=N/200`, `k=8`, `p_local=0.5`, day 0; plus normal/reversed slot pairs at N=100k and N=1M and the reported N=1M, k=16 Phase 6B comparison. |
| M3-08 / M3-09 equal-live headline | Two 100k equal-live comparisons from the frozen M3P sustained configurations: full-rebuild M3P/M3 for M3-08, and incremental M3P/M3 for M3-09. New committed drivers measure only these headline ratios. |
| M3-11 propagation | 20 crossover cells: `N=10k`, `K=50`, `k=8`, `p_local=0.2`, `max_hops ∈ {1,2,4,8,16}`, pools `{1,2,4,6}`, chunk 256; plus the large-frontier ratio cell `N=100k`, `K=500`, `k=16`, `p_local=0.2`, `max_hops=16`, pool 6/chunk 256 versus serial. |

### 13.4 A-5 runtime estimate and stop rule

The estimate uses the historical benchmark logs as a planning basis, then applies A-5's 41 samples and one-second minimum paired sample. M3-02 alone requires at least 2.05 hours for 180 active cells. Scaling the M3-05 25.5-minute seven-sample run to 41 samples gives about 2.5 hours; scaling the M3-06 11.55-minute seven-sample run gives about 1.1 hours. These are planning estimates, not new measurements. M3-01/03/04/07/11, the M2 headline cells, the two equal-live comparisons, warmups, and the 55 equilibrium candidates add further work. The total planned work is estimated at **8–10 hours** on the reference host, before any rerun caused by A-5 validity.

Because the estimate exceeds the six-hour limit in M5-00 §8, no M5-05 deterministic or timing run is authorized in this execution. The cell plan remains frozen for a later split or explicit re-planning; do not start a partial run under this task.

### 13.5 Provenance and generated report

- Use the committed benches listed in `sources.toml`; the two M3-08/M3-09 headline replacements must be committed drivers under `crates/sim-model/examples/` before use.
- Store run data under `docs/data/m5_05/<report>/`. Follow §4's 1 MiB policy and manifest-backed release assets for larger outputs. Commit each report group before starting the next run group.
- Generate `docs/reproducibility/M5_05_M0_M3_REGENERATION.qmd` and its rendered GFM `.md` with `analysis/simciv_analysis/report_m5_05.py`. The report records headline results by milestone, new/old ratios and >20% flags, figures for conclusion-bearing comparisons, and historical-protocol/analysis-only exclusions. Hand-entered measured values are not allowed.
- Update claim statuses in the analysis code from the comparison results. `just check` and the full Rust validation are required after the regeneration report is complete.

### 13.6 M5-05 decision register (FROZEN)

| ID | Decision | Frozen rule |
|---|---|---|
| D-1 | Scope | M2/M3 headline claims only; supporting claims are included only when produced by the same selected run. |
| D-2 | Deterministic evidence | Use only cited tests and committed goldens; require bit-exact results or classify the claim as unrecoverable. Never re-bless expected outputs. |
| D-3 | Timing evidence | Use A-5 on the reference host, same-mode paired paths, report new/old ratios and flag absolute timing changes over 20%; only prelisted headline cells are run. |
| D-4 | M3-08/M3-09 drivers | Commit new drivers for only the 100k equal-live M3P/M3 ratios using the frozen configs; detailed historical tables remain historical-protocol. |
| D-5 | Runtime budget | Commit the plan and estimate before runs. If the estimate exceeds six hours, stop without running. |
| D-6 | Provenance | Commit outputs under `docs/data/m5_05/`, apply the M5-00 size policy, and cite harness plus data/manifest for every generated table. |

### 13.7 Freeze record

This freeze authorizes only the M5-05 headline regeneration described in §13.1–§13.5. The preregistered estimate exceeds six hours, so execution stops before tests or benchmarks; no M5-05 result is claimed. It does not authorize any simulation semantic change, expected-value update, CI change, or later M5 stage.

Later update (2026-10-23): M5-05 A-1 in §15 supersedes the broad cell scope and six-hour stop for the prior plan with a preselected conclusion-level set executed in sessions of at most six hours. The earlier plan and estimate above remain as the record of the original freeze.

## 14. M5-00 A-1/A-2 data and provenance amendment (FROZEN)

This amendment supersedes M5-00 §4 only for new experiment and measurement outputs and adds content-hash provenance rules. It does not move or rewrite existing committed data.

### 14.1 DVC data policy (A-1)

- Track new full raw experiment and measurement outputs with DVC. The default DVC remote is the public-read DagsHub remote `https://dagshub.com/Superple19/SimulaCiv.dvc`.
- Keep specs/configs, harnesses, reports, generated summaries/tables/figures, viewer samples, golden files, `dvc.yaml`, `dvc.lock`, and `*.dvc` pointers in Git. Full raw outputs are DVC data, not Git files.
- Each run has a small committed summary at or below 1 MiB. Report generators read only these summaries and must not read DVC-tracked raw paths. Raw outputs and their small summaries are produced from the same run.
- The tracked `.dvc/config` contains the URL only. Authentication is local-only in `.dvc/config.local`, which must remain ignored. Never commit tokens, usernames, or passwords.
- Existing data already committed to Git remains there. Do not rewrite history or migrate old files to DVC as part of this amendment.
- Deterministic stages may be reproduced normally. Timing stages are frozen in DVC so `dvc repro` does not silently repeat or replace measurements. Re-measurement requires an explicit `dvc unfreeze <stage>`, `dvc repro <stage>`, then `dvc freeze <stage>` sequence, followed by review of new raw outputs and summary.

### 14.2 Content-hash provenance (A-2)

Content digests are the only provenance authority in new run records and reports; do not write Git commit hashes into them. For a source set, normalize repository paths to forward-slash UTF-8 paths and sort by their UTF-8 bytes. For each path, append `path`, one NUL byte, the lowercase SHA-256 hex digest of the file bytes, and one LF byte. `source_digest` is SHA-256 over that concatenation for `crates/**/*.rs`, `crates/**/Cargo.toml`, root `Cargo.toml`, `Cargo.lock`, and `rust-toolchain.toml`. `spec_digest` and `harness_digest` use the same algorithm over their explicitly listed input paths.

Each run records `source_digest`, `spec_digest`, `harness_digest`, `rustc -V`, and the output digests. Output provenance records the DVC md5 value from `dvc.lock` and a SHA-256 digest in the run metadata. Run metadata also records the exact command, spec/config paths and values, and host information. It contains no Git commit hash.

### 14.3 DVC execution and validation boundary

- Each `dvc.yaml` stage explicitly lists its harness, spec/config inputs including seeds, `crates/sim-model/src`, `crates/sim-core/src`, root and crate Cargo manifests, `Cargo.lock`, and `rust-toolchain.toml` as dependencies.
- `just check` succeeds on a clean Git checkout without `dvc pull`; report generation consumes committed summaries only. An analysis test rejects report generators that read DVC-tracked raw paths.
- `just provenance` prints and verifies the run's content digests against the current checkout. A mismatch makes the recorded run stale; it does not update the expected digest automatically.
- Do not run `dvc push` or Git push as part of this task.

### 14.4 M5-00 A-1/A-2 decision register (FROZEN)

| ID | Decision | Frozen rule |
|---|---|---|
| A-1 | New-data storage | DVC tracks new raw outputs on the DagsHub remote; small summaries and source artifacts stay in Git. Existing Git data is retained unchanged. This supersedes the release-asset policy in §4 for new work. |
| A-2 | Provenance | Source/spec/harness/output content digests and toolchain version are authoritative. No Git commit hash is recorded in run data or generated reports. |
| A-3 | Timing stages | Timing DVC stages are frozen by default. Re-measurement explicitly unfreezes, runs, reviews, and re-freezes the stage. |
| A-4 | Clean-checkout behavior | `just check` requires no DVC pull and report generators read committed summaries only; credentials are never committed. |

### 14.5 Freeze record

This amendment authorizes DVC setup, the public-read DagsHub remote URL in `.dvc/config`, local-only credentials, content-hash provenance tooling, and DVC-backed outputs for new work. It authorizes no DVC push, Git push, or migration of existing data.

### 14.6 Implementation record

The setup uses DVC 3.67.1, released 2026-03-31 ([PyPI release](https://pypi.org/project/dvc/3.67.1/), [upstream latest release](https://github.com/iterative/dvc/releases/latest)). Its published Python classifiers include 3.14 but not 3.15, so DVC was installed as an isolated uv tool under Python 3.14.9; the analysis project remains pinned to Python 3.15.1. The tracked remote configuration contains only the DagsHub URL; local credentials were not configured or committed.
