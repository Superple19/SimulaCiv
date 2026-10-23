# SimulaCiv analysis

The analysis environment is pinned to Python 3.15.1, uv 0.13.5, pandas 3.0.6, matplotlib 3.11.2, pytest 9.1.1, Quarto CLI 1.10.19, and Vale 3.25.1 (released 2026-10-22). The Python package requires Python >=3.11; the pinned version is for development and CI.

## Setup

Install prek 0.5.7, uv 0.13.5, Quarto CLI 1.10.19, Vale 3.25.1, and just 1.58.0 using their official installation instructions. From the repository root, install the Git hooks:

```powershell
just hooks-install
```

Pre-commit checks run formatting, file hygiene, DVC-aware size checks, secret detection, and Vale/link checks for Markdown and Quarto files. The pre-push hooks run Clippy and `just check`; Rust tests remain in CI. Do not bypass the hooks with `--no-verify`.

Then install the pinned Python runtime and dependencies:

```sh
uv python install
```

`just check` runs the analysis tests and verifies links, claims, committed reports, and the Vale prose rules without rewriting them. `just lint-prose` runs the pinned Vale style and its rule tests. To reproduce the M5-01 report from the committed ledger, run the single command:

```sh
just report-m5-01
```

This regenerates the tidy claim ledger, `.qmd`, and rendered GitHub-flavored Markdown with the pinned tools. Use `just claims` alone when only the ledger needs regeneration.

## DVC experiment data

DVC 3.67.1 is the latest stable release checked for M5-05 (2026-03-31). Its published Python classifiers currently include Python 3.14 but not 3.15, so install it as an isolated uv tool with Python 3.14; this does not change the analysis project or its Python 3.15.1 pin:

```powershell
uv tool install --python 3.14 dvc==3.67.1
```

The shared DVC remote URL is in `.dvc/config`; the DagsHub DVC remote is public-read. Pull raw data when needed with:

```powershell
dvc pull docs/data/m5_05/dvc.yaml
```

Maintainers need a DagsHub token only to upload. Set it in the ignored local file and keep credentials out of Git:

```powershell
$env:DAGSHUB_USER = Read-Host "DagsHub username"
$secure = Read-Host "DagsHub token" -AsSecureString
$env:DAGSHUB_TOKEN = [System.Net.NetworkCredential]::new("", $secure).Password
dvc remote modify dagshub --local auth basic
dvc remote modify dagshub --local user $env:DAGSHUB_USER
dvc remote modify dagshub --local password $env:DAGSHUB_TOKEN
Remove-Item Env:DAGSHUB_TOKEN, Env:DAGSHUB_USER
```

Deterministic stages can be reproduced with `dvc repro docs/data/m5_05/dvc.yaml:<stage>`. Timing stages are frozen so ordinary `dvc repro` cannot silently replace samples. A deliberate re-measurement uses `dvc unfreeze docs/data/m5_05/dvc.yaml:<stage>`, `dvc repro docs/data/m5_05/dvc.yaml:<stage>`, then `dvc freeze docs/data/m5_05/dvc.yaml:<stage>`. The pipeline and lockfile are kept in `docs/data/m5_05/`; stage paths remain rooted at the repository root. Each committed summary is verified against its current source tree with:

```powershell
just provenance docs/data/m5_05/<entry>/summary.json
```

`just check` works without DVC installed and without `dvc pull`; report generators consume committed summaries, never raw DVC outputs.

After a harness creates a new file under `docs/data/`, run `just data-integrity-update` and commit the new checksum entry with the output. The command adds new paths but refuses to change or remove an existing data checksum; `just check` and the pre-commit hook verify the manifest.

## M5-05 focused regeneration

Validate the frozen conclusion registry and run its deterministic Session 1 confirmations with `just m5-05-plan` and `just m5-05-deterministic`. A selected timing conclusion is run with `just m5-05-timing <conclusion-id>`; the command records A-5 raw samples and verifies the summary's content provenance. Session 2 and Session 3 timing entries are intentionally rejected until separately authorized.
