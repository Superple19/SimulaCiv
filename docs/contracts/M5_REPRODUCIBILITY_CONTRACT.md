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

<!-- BEGIN GENERATED M5 STAGES -->
| Milestone | Stage | Status | Tag | Summary | Document |
|---|---|---|---|---|---|
| M5 | `M5-00` | frozen | — | Froze reproducibility definitions, provenance, and the initial data policy. | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| M5 | `M5-01` | complete | — | Inventoried numeric M0–M4 claims and their sources and reproducibility status. | [source](../reproducibility/M5_01_INVENTORY.md) |
| M5 | `M5-02` | complete | — | Reorganized repository documentation, reports, data, and analysis files. | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| M5 | `M5-03` | complete | — | Added the pinned analysis toolchain, deterministic report generation, and analysis CI. | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| M5 | `M5-04` | complete | — | Regenerated M4 deterministic outputs and conclusion-bearing timing claims. | [source](../reproducibility/M5_04_M4_REGENERATION.md) |
| M5 | `M5-05` | sessions 1–3 measured; M3-09 scope check pending | — | Measured sessions one through three; the M3-09 scope check remains pending. | [source](../reproducibility/M5_05_M0_M3_REGENERATION.md) |
| M5 | `M5-06` | planned | — | Add PyO3 bindings and a Python runner with content-digest, configuration, and host provenance instead of commit-hash provenance. | [source](M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-07` | planned | — | Run multi-seed, multi-workload robustness experiments with effect-size-first design. | [source](M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-08` | planned | — | Clean up project documentation, generate figures, and prepare newcomer-facing material. | [source](M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-09` | planned | — | Review related work and draft the technical report. | [source](M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
| M5 | `M5-AUDIT` | planned | — | Audit M5 reproducibility and research-hardening results. | [source](M5_REPRODUCIBILITY_CONTRACT.md#5-m5-stage-plan-planned-each-stage-needs-its-own-draft-review-frozen-contract) |
<!-- END GENERATED M5 STAGES -->

## 6. Known inputs

- [`docs/data/m4_inventory/README.md`](../data/m4_inventory/README.md) records recovered and missing M4 harness/data pairs.
- The [M4 final audit](../reports/M4_AUDIT_FINAL_AUDIT_AND_GRADUATION.md) records wiring debt, uncalibrated migration settings, the M4-05 boundary predicate, compaction money loss, newborn CRN loss, and single-host/workload limits.
- The M5-01 inventory must extend these inputs to every numeric claim in M0–M4 rather than treating this draft as a complete claim list.

## 7. Non-goals

- No semantic changes to existing profiles and no new simulation features as part of M5 reproducibility work.
- Inverse inference, distributed determinism, and learning agents remain later candidates.

## 8. M5-00 decision register (FROZEN)

<!-- BEGIN GENERATED DECISIONS: m5-00 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-00-D-1` | superseded | The original policy capped Git data files at 1 MiB and used manifest-backed release assets for larger outputs. | — | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-2` | frozen | Judge timing conclusions on the Ryzen 5 9600X reference host using A-5 and flag absolute timing ratios that change by more than 20 percent. | `docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md`<br>`docs/reproducibility/M5_04_M4_REGENERATION.md` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-3` | frozen | Deterministic claims must reproduce bit-exactly or be labeled unrecoverable without fabrication or approximation. | `analysis/simciv_analysis/claims.py`<br>`analysis/simciv_analysis/report_m5_04.py` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-4` | frozen | The planned sim-py interface uses maturin and PyO3 for Python 3.11 or newer while keeping the runner core on the standard library. | — | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-5` | frozen | The planned Python smoke matrix is Python 3.12 on three operating systems and any Python CI addition requires explicit approval. | — | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-6` | frozen | The robustness pilot starts from at least ten seeds across M3P sustained, M3 equilibrium, and calibrated migration workloads. | — | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-7` | frozen | The publication plan is an arXiv technical report first with JOSS considered after M5. | — | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
| `M5-00-D-8` | frozen | M5 acceptance requires every numeric claim to be classified and deterministic claims to be exact or explicitly unrecoverable. | `analysis/simciv_analysis/claims.py`<br>`just:claims-check`<br>`just:check` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#8-m5-00-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-00 -->

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

<!-- BEGIN GENERATED DECISIONS: m5-02 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-02-D-1` | frozen | Keep the M1 manifest at its existing root path and organize other repository content under the frozen docs and analysis layout. | `contracts/m1_contract.toml`<br>`docs/README.md` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| `M5-02-D-2` | frozen | Reports live under docs/reports with stage-based names and exactly one approved status header while historical prose is preserved. | `docs/reports/RENAMES.md`<br>`analysis/simciv_analysis/links.py` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| `M5-02-D-3` | frozen | Data is organized in flat per-stage docs/data folders with README files and superseded subfolders. | `docs/data/README.md`<br>`docs/data/SHA256SUMS`<br>`analysis/simciv_analysis/data_integrity.py` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| `M5-02-D-4` | frozen | Maintained analysis tools live in analysis and obsolete tools use analysis/legacy. | `analysis/README.md`<br>`analysis/simciv_analysis/links.py` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| `M5-02-D-5` | frozen | Cited harnesses, data, and historical report wording are preserved, and removals are recorded in the rename index. | `AGENTS.md`<br>`docs/reports/RENAMES.md` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
| `M5-02-D-6` | frozen | M5-03 commits the claim extractor, excludes input values, adds tiers, and rechecks the 29 M1 unrecoverable claims. | `analysis/simciv_analysis/claims.py`<br>`docs/reproducibility/claims.csv.gz`<br>`just:claims-check` | 2026-10-22 | [source](M5_REPRODUCIBILITY_CONTRACT.md#103-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-02 -->

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

Later update (2026-10-23): `git ls-remote --tags --refs` verified the pinned CI action, tool, and pre-commit refs. The workflow's previous `astral-sh/setup-uv@v10` ref did not exist, although the latest release tag `v10.3.2` did; CI now pins that exact tag. The verified workflow refs are `actions/checkout@v7`, `Swatinem/rust-cache@v2`, `astral-sh/setup-uv@v10.3.2`, `quarto-dev/quarto-actions@v2`, and `extractions/setup-just@v4`. Verified tool tags are Python `v3.15.1`, uv `0.13.5`, Quarto `v1.10.19`, just `1.58.0`, Vale `v3.25.1`, and prek `v0.5.7`. Verified pre-commit refs are `crate-ci/committed@v1.1.11`, `pre-commit/pre-commit-hooks@v6.0.0`, and `gitleaks/gitleaks@v8.30.1`. `just check` now verifies that every remote `uses:` ref in `ci.yml` resolves to an exact upstream tag.

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

<!-- BEGIN GENERATED DECISIONS: m5-03 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-03-D-1` | superseded | The original version policy pinned GitHub Actions by major tag while pinning Python, libraries, and Quarto more narrowly. | — | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| `M5-03-D-2` | frozen | Analysis is a uv project requiring Python 3.11 or newer with pandas, matplotlib, and pytest as its direct dependencies. | `analysis/pyproject.toml`<br>`analysis/uv.lock`<br>`.python-version` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| `M5-03-D-3` | frozen | Quarto reports and Matplotlib figures are deterministic and check mode compares temporary renders without rewriting tracked files. | `analysis/simciv_analysis/figures.py`<br>`analysis/simciv_analysis/report_m5_01.py`<br>`analysis/tests/test_figures.py`<br>`just:check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| `M5-03-D-4` | frozen | The analysis toolchain exposes the frozen just commands and check validates analysis outputs without rewriting them. | `justfile`<br>`just:check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| `M5-03-D-5` | frozen | The Ubuntu analysis CI job runs the pinned analysis toolchain separately from Rust validation. | `.github/workflows/ci.yml`<br>`test:analysis/tests/test_ci_targets.py::test_workflow_check_steps_are_single_just_targets` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
| `M5-03-D-6` | frozen | The committed claim ledger is tidy, excludes configuration inputs, carries tiers, and records explicit reasons for M1 claim classifications. | `analysis/simciv_analysis/claims.py`<br>`analysis/tests/test_claims.py`<br>`docs/reproducibility/claims.csv.gz`<br>`just:claims-check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#114-m5-03-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-03 -->

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

<!-- BEGIN GENERATED DECISIONS: m5-04 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-04-D-1` | frozen | Deterministic M4 reruns compare bytes or deterministic fields exactly and report mismatches without patching historical outputs. | `analysis/simciv_analysis/report_m5_04.py`<br>`just:report-m5-04-check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#125-m5-04-decision-register-frozen) |
| `M5-04-D-2` | frozen | Only final A-5 gates and conclusion-bearing M4 timings are rerun, with ratios and deviations over 20 percent recorded. | `analysis/simciv_analysis/report_m5_04.py`<br>`docs/reproducibility/M5_04_M4_REGENERATION.md` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#125-m5-04-decision-register-frozen) |
| `M5-04-D-3` | frozen | Bit-identical M4-07 parallel execution may be used for deterministic reruns when the mode is recorded. | `analysis/simciv_analysis/report_m5_04.py`<br>`crates/sim-model/examples/m4_07_many_branch.rs` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#125-m5-04-decision-register-frozen) |
| `M5-04-D-4` | frozen | Restored harness labels change only after a successful rerun and record that result. | `analysis/simciv_analysis/report_m5_04.py`<br>`crates/sim-model/examples/m4_02_a5_measure.rs` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#125-m5-04-decision-register-frozen) |
| `M5-04-D-5` | frozen | M4 outputs use the frozen size policy and the generated report reads committed harness summaries and data. | `analysis/simciv_analysis/report_m5_04.py`<br>`docs/data/m5_04/README.md`<br>`just:report-m5-04-check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#125-m5-04-decision-register-frozen) |
| `M5-04-D-6` | frozen | Every extracted M4 headline claim is classified as bit-exact, conclusion holds, mismatch, or unrecoverable. | `analysis/simciv_analysis/report_m5_04.py`<br>`analysis/simciv_analysis/claims.py`<br>`just:report-m5-04-check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#125-m5-04-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-04 -->

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

<!-- BEGIN GENERATED DECISIONS: m5-05 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-05-D-1` | superseded | The original plan scoped regeneration to M2 and M3 headline claims and included supporting values only when produced by selected runs. | — | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#136-m5-05-decision-register-frozen) |
| `M5-05-D-2` | frozen | Deterministic headline claims use cited tests and goldens without re-blessing expected outputs. | `crates/sim-model/tests`<br>`analysis/simciv_analysis/m5_05.py`<br>`just:m5-05-deterministic` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#136-m5-05-decision-register-frozen) |
| `M5-05-D-3` | frozen | Conclusion-bearing timings use A-5 with matched execution options and report old/new ratios for prelisted cells. | `crates/sim-model/examples/m5_05_conclusion_measure.rs`<br>`docs/data/m5_05/dvc.yaml`<br>`analysis/simciv_analysis/report_m5_05.py` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#136-m5-05-decision-register-frozen) |
| `M5-05-D-4` | frozen | The M3-08 and M3-09 equal-live drivers measure only the frozen 100k M3P/M3 comparison. | `crates/sim-model/examples/m5_05_conclusion_measure.rs`<br>`docs/data/m5_05/dvc.yaml` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#136-m5-05-decision-register-frozen) |
| `M5-05-D-5` | superseded | The original M5-05 timing plan stopped when the estimated total exceeded six hours. | — | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#136-m5-05-decision-register-frozen) |
| `M5-05-D-6` | frozen | M5-05 commits summaries and DVC data under the stage directory and cites harnesses and data for generated tables. | `docs/data/m5_05/dvc.yaml`<br>`analysis/simciv_analysis/report_m5_05.py`<br>`just:report-m5-05-check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#136-m5-05-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-05 -->

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

<!-- BEGIN GENERATED DECISIONS: m5-00-a1 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-00-A-1` | frozen | DVC tracks new raw outputs while Git retains small summaries and existing data, with manifest and cache handling defined by the amendment. | `docs/data/m5_05/dvc.yaml`<br>`docs/data/SHA256SUMS`<br>`analysis/simciv_analysis/data_integrity.py`<br>`just:data-integrity-check` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#144-m5-00-a-1a-2-decision-register-frozen) |
| `M5-00-A-2` | frozen | Source, spec, harness, output, and toolchain content digests are authoritative and run provenance does not use Git commit hashes. | `analysis/simciv_analysis/provenance.py`<br>`analysis/tests/test_provenance.py`<br>`just:provenance` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#144-m5-00-a-1a-2-decision-register-frozen) |
| `M5-00-A-3` | frozen | Timing DVC stages remain frozen unless a deliberate measurement run explicitly unfreezes and re-freezes them. | `justfile`<br>`just:m5-05-timing` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#144-m5-00-a-1a-2-decision-register-frozen) |
| `M5-00-A-4` | frozen | Clean-checkout analysis validation requires no DVC pull and reads only committed summaries. | `analysis/simciv_analysis/ci_clean.py`<br>`analysis/tests/test_m5_05_dvc_status.py`<br>`just:ci-clean` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#144-m5-00-a-1a-2-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-00-a1 -->

### 14.5 Freeze record

This amendment authorizes DVC setup, the public-read DagsHub remote URL in `.dvc/config`, local-only credentials, content-hash provenance tooling, and DVC-backed outputs for new work. It authorizes no DVC push, Git push, or migration of existing data.

### 14.6 Implementation record

The setup uses DVC 3.67.1, released 2026-03-31 ([PyPI release](https://pypi.org/project/dvc/3.67.1/), [upstream latest release](https://github.com/iterative/dvc/releases/latest)). Its published Python classifiers include 3.14 but not 3.15, so DVC was installed as an isolated uv tool under Python 3.14.9; the analysis project remains pinned to Python 3.15.1. The tracked remote configuration contains only the DagsHub URL; local credentials were not configured or committed.

## 15. M5-05 conclusion-level regeneration amendment A-1 (FROZEN)

This amendment replaces only the M5-05 scope and runtime plan in §13. It preserves §13's deterministic/timing classification, M5-00 A-1/A-2 data and provenance rules, and the requirement to report mismatches rather than repair historical values.

### 15.1 Conclusion registry

`docs/reproducibility/m5_05_conclusions.toml` is the pre-registered source for 13 conclusion-level rows. Each row records a verbatim report conclusion, its comparison, one to three representative original cells, the committed harness and cell arguments, the M5-00 pass criterion, deterministic test targets where applicable, and the estimated runtime. It covers M2 near-linear scaling, the M2-40 final tick, M2 Phase 4 crossover, M3-01 serial baseline, M3-02 parallel Phase 6B, M3-03 search reduction, M3-06 settlement indexing, M3-07 reversed-slot sensitivity, M3-08 full-rebuild cost, M3-09 incremental maintenance, and the three M3-11 propagation conclusions (Phase 9c speedup, whole-tick speedup, and observed crossover).

The registry is intentionally conclusion-level. A sampled result can support or challenge the selected comparison; it cannot be reported as a new full-grid aggregate when the original headline was computed over a wider grid. Such a row is **inconclusive** unless its pre-registered cells establish the stated conclusion.

### 15.2 Deterministic confirmations

Run the integration-test targets named in the registry without timing. Record exact pass/fail and test names. A test confirms only the deterministic behavior it exercises; no expected value is changed. Mismatches are retained as findings.

### 15.3 Timing stages and data

- Each timing conclusion has a separate DVC stage and raw output directory `docs/data/m5_05/<conclusion-id>/raw/`; the stage writes a Git summary `docs/data/m5_05/<conclusion-id>/summary.json` from that same run. The deterministic confirmation has its own stage and raw log.
- Every stage lists the conclusion spec, cell config and seeds, harness, `crates/sim-model/src`, `crates/sim-core/src`, root and crate manifests, `Cargo.lock`, and `rust-toolchain.toml` as dependencies. Timing stages are frozen in DVC after recording so `dvc repro` cannot replace measurements implicitly.
- Apply the M4 standard A-5 protocol: reference host, two warmups, 41 paired samples, fixed per-cell repetition count chosen during warmup so each sample is at least one second, per-operation time reported as sample time divided by that count, and both paired execution orders. On Windows, run at high priority with affinity `0x555`; print `TIMING RUN START` and `TIMING RUN END`, and run only while the PC is idle. Record the host and the per-cell repetition count.
- Every comparison asserts identical workload, starting checkpoint, execution mode, pool size, chunk size, horizon, enabled metrics/events, and snapshot options on both paths. This includes checks that are not a speed comparison.
- Store a content-hash summary with the exact source/spec/harness digests, toolchain, DVC output md5, output SHA-256, cell key, path medians/MADs, old/new values, ratio, and conclusion status. Report generators read summaries only.

### 15.4 Session plan

The focused set has 13 conclusions and at most three representative cells per conclusion. The registry estimates **7 hours 30 minutes** total: Session 1 is 2h45m, Session 2 is 2h45m, and Session 3 is 2h; estimates use historical cell costs, the 41-sample A-5 minimum, selected-cell driver setup, and paired-parity checks, and exclude deterministic tests. **Session 1, authorized now:** deterministic confirmations, then M2 near-linear/M2-40 final-tick/M2 Phase 4 crossover, and M3-01/M3-02/M3-03 selected cells. Sessions 2 and 3 are listed in the registry but are not authorized by this amendment execution. Commit the raw-data pointer/lock and summary after each completed session group.

### 15.5 Generated report and acceptance

`analysis/simciv_analysis/report_m5_05.py` generates `docs/reproducibility/M5_05_M0_M3_REGENERATION.qmd` and its rendered Markdown from the committed summaries. It reports one row per conclusion as **holds**, **does not hold**, or **inconclusive**, with old/new values, new/old ratios for absolute timings, and a flag when `|ratio - 1| > 20%`. Deterministic results, timing results, and unrun sessions are distinct. Update ledger status only from source mappings and recorded comparison results. `just check` and `just ci` must pass.

### 15.6 Frozen decision register

<!-- BEGIN GENERATED DECISIONS: m5-05-a1 -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-05-A1-1` | frozen | M5-05 uses the selected conclusion-level registry and does not infer unmeasured aggregates from representative cells. | `docs/reproducibility/m5_05_conclusions.toml`<br>`analysis/simciv_analysis/m5_05.py`<br>`docs/data/m5_05/dvc.yaml` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#156-frozen-decision-register) |
| `M5-05-A1-2` | frozen | Mapped deterministic tests run first and expected values are never re-blessed. | `analysis/simciv_analysis/m5_05.py`<br>`crates/sim-model/tests`<br>`just:m5-05-deterministic` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#156-frozen-decision-register) |
| `M5-05-A1-3` | frozen | Timing comparisons retain A-5, matched run options, conclusion-level reporting, and documented inconclusive outcomes. | `crates/sim-model/examples/m5_05_conclusion_measure.rs`<br>`analysis/simciv_analysis/report_m5_05.py`<br>`docs/data/m5_05/dvc.yaml` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#156-frozen-decision-register) |
| `M5-05-A1-4` | frozen | The 13 selected conclusions run in three recorded sessions, each bounded by six hours. | `docs/reproducibility/m5_05_conclusions.toml`<br>`analysis/simciv_analysis/m5_05.py`<br>`docs/data/m5_05/STATUS.md` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#156-frozen-decision-register) |
| `M5-05-A1-5` | frozen | Each selected conclusion has a DVC raw stage and a committed summary with content-hash provenance. | `docs/data/m5_05/dvc.yaml`<br>`analysis/simciv_analysis/provenance.py`<br>`just:provenance-finalize` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#156-frozen-decision-register) |
<!-- END GENERATED DECISIONS: m5-05-a1 -->

### 15.7 Freeze record

This amendment authorizes the conclusion registry and Session 1 work described in §15.1–§15.4. It does not authorize Sessions 2–3, any Git/DVC push, simulation semantic changes, or changes to existing tests, goldens, snapshots, or CI.

## 16. M5 infrastructure decision register (FROZEN)

This register records repository-wide M5 rules for checks, text/data integrity, and delivery. GitHub ruleset state is configured in repository settings and is not managed by this contract.

<!-- BEGIN GENERATED DECISIONS: m5-infrastructure -->
| ID | Status | Summary | Enforced by | Date | Document |
|---|---|---|---|---|---|
| `M5-INFRA-PREK-HOOKS` | frozen | Install and use the pinned prek hooks for repository checks and never bypass them with --no-verify. | `.pre-commit-config.yaml`<br>`AGENTS.md`<br>`just:hooks-install`<br>`prek:pre-push-clippy`<br>`prek:pre-push-ci-linux` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-TEXT-FORMAT` | frozen | Repository text uses UTF-8 with LF and no BOM, while data files remain byte-exact and Python text output goes through write_text_lf. | `.editorconfig`<br>`.gitattributes`<br>`analysis/simciv_analysis/io.py`<br>`analysis/tests/test_text_output_policy.py`<br>`prek:forbid-utf8-bom` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-DATA-INTEGRITY` | frozen | Non-README docs/data bytes match SHA256SUMS generated from Git blobs and integrity checks compare working-tree bytes with blobs. | `docs/data/SHA256SUMS`<br>`analysis/simciv_analysis/data_integrity.py`<br>`analysis/tests/test_data_integrity.py`<br>`just:data-integrity-check`<br>`prek:data-integrity` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-FINDINGS-GUARDRAILS` | frozen | Every generated Findings bullet has a passing paired predicate, a preregistered or exploratory tag, and scoped Vale checks. | `analysis/simciv_analysis/findings.py`<br>`analysis/simciv_analysis/report_m5_04.py`<br>`analysis/simciv_analysis/report_m5_05.py`<br>`analysis/tests/test_findings.py`<br>`just:lint-prose`<br>`prek:vale-changed-docs` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-TEST-OPT` | frozen | Development test builds optimize sim-model and sim-core at level two while retaining debug assertions and overflow checks. | `Cargo.toml`<br>`just:ci-rust` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-EXACT-ACTION-TAGS` | frozen | Every GitHub Actions reference uses an exact existing tag and action-ref validation checks both CI workflows. | `analysis/simciv_analysis/action_refs.py`<br>`test:analysis/tests/test_action_refs.py::test_ci_and_promotion_workflows_have_pinned_checkout_actions`<br>`just:action-refs-check` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-CI-JUST-TARGETS` | frozen | CI validation jobs call only ci-rust or ci-analysis and install every tool their target uses before running it. | `.github/workflows/ci.yml`<br>`test:analysis/tests/test_ci_targets.py::test_workflow_check_steps_are_single_just_targets`<br>`test:analysis/tests/test_ci_targets.py::test_just_jobs_install_all_target_tools_before_running_them`<br>`just:ci-rust`<br>`just:ci-analysis` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-CROSS-CLIPPY` | frozen | Local CI and pre-push validation run cross-target Clippy for Linux and macOS targets. | `justfile`<br>`just:clippy-cross`<br>`prek:pre-push-clippy-cross` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-CLEAN-CHECKOUT` | frozen | The analysis check is validated in a temporary clean Git worktree without DVC outputs or ignored files. | `analysis/simciv_analysis/ci_clean.py`<br>`justfile`<br>`just:ci-clean`<br>`prek:pre-push-ci-clean` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-WSL-LINUX` | frozen | The WSL Linux pre-push check refreshes a clean clone while retaining target and runs the same Rust and analysis targets as CI. | `analysis/simciv_analysis/ci_linux.py`<br>`analysis/README.md`<br>`just:ci-linux`<br>`prek:pre-push-ci-linux` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-DELIVERY` | frozen | just ship runs local checks, pushes the same commit to ci-check, and green CI promotes it to main by fast-forward before dispatching main CI. | `justfile`<br>`.github/workflows/promote.yml`<br>`test:analysis/tests/test_ci_targets.py::test_promotion_workflow_is_push_only_fast_forward_and_dispatches_main_ci`<br>`AGENTS.md`<br>`analysis/README.md` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-MAIN-RULESET` | frozen | The main branch ruleset requires passing checks, blocks force pushes and deletion, and has no bypass actors. | `docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-GZIP-CONTENT` | frozen | Generated gzip artifacts are compared by decompressed content rather than compressed container bytes. | `analysis/simciv_analysis/io.py`<br>`test:analysis/tests/test_io.py::test_gzip_content_check_ignores_container_bytes`<br>`analysis/simciv_analysis/claims.py`<br>`analysis/simciv_analysis/report_m5_04.py`<br>`analysis/simciv_analysis/report_m5_05.py` | 2026-10-23 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-DVC-CACHE` | frozen | DVC upload remains deferred and the local DVC cache must not be garbage-collected before dvc push --all-commits succeeds. | `AGENTS.md`<br>`docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-CI-TIME-SCOPE` | frozen | CI time reduction is in M5 scope and must preserve the full validation coverage. | `docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md` | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-M4-01-DEBT` | frozen | M4-01 state, codec, and context extensibility debt is an M6 candidate rather than M5 scope. | — | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-LARGE-FILE-REFACTOR` | frozen | Large-file refactors are deferred to M6 rather than included in M5. | — | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
| `M5-INFRA-COMPACTION-MONEY-LOSS` | frozen | M3P compaction money-loss semantics are an M6 candidate only as a new profile. | — | 2026-10-24 | [source](M5_REPRODUCIBILITY_CONTRACT.md#16-m5-infrastructure-decision-register-frozen) |
<!-- END GENERATED DECISIONS: m5-infrastructure -->
