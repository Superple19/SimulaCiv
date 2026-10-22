# M5 Reproducibility and Research Hardening Contract

**Status: M5-00, M5-02, and M5-03 FROZEN; M5-01 COMPLETE**
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

## 5. M5 stage plan (PLANNED — each stage needs its own DRAFT → review → FROZEN contract)

The order is structure → tooling → verification → new experiments → cleanup → publication.

| Stage | Planned scope |
|---|---|
| **M5-00 — Contract and data policy** | **COMPLETE.** Freeze reproducibility definitions, the selected data-size policy, provenance requirements, and acceptance gates. |
| **M5-01 — Reproducibility inventory** | **COMPLETE.** Inventory every numeric claim in M0–M4 reports and measured contract statements; map claim values to harnesses, raw data, status, class, and next stage. |
| **M5-02 — Repository layout** | **FROZEN; implementation in progress.** Organize contracts, reports, data, reproducibility outputs, and analysis tools under the frozen layout in §10. |
| **M5-03 — Analysis toolchain** | **FROZEN.** Add the pinned `uv` project, `pandas`, `matplotlib`, Quarto, `just`, `pytest`, deterministic report generation, link checking, and the approved Ubuntu analysis CI job. Regenerate M5-01 as a tidy one-row-per-claim gzip ledger, exclude configuration/input values, add key/supporting tiers, and recheck the 29 M1 claims currently labeled unrecoverable. |
| **M5-04 — M4 regeneration** | Re-run existing Rust harnesses against committed M4 data, verify deterministic outputs bit-exactly, and produce a Quarto report. |
| **M5-05 — M0–M3 regeneration and re-measurement** | Regenerate or re-measure key M0–M3 tables, preserving old values as history and applying the frozen timing comparison rule. |
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
