# SimulaCiv analysis

The analysis environment is pinned to Python 3.15.1, uv 0.13.5, pandas 3.0.6, matplotlib 3.11.2, pytest 9.1.1, Quarto CLI 1.10.19, and Vale 3.25.1 (released 2026-10-22). The Python package requires Python >=3.11; the pinned version is for development and CI.

## Setup

Install prek 0.5.7, uv 0.13.5, Quarto CLI 1.10.19, Vale 3.25.1, and just 1.58.0 using their official installation instructions. From the repository root, install the Git hooks:

On Windows, disable Git's automatic EOL conversion for this checkout before working with repository data:

```powershell
git config --local core.autocrlf false
```

`just check` warns if the repository-local setting is `true`.

```powershell
just hooks-install
```

Pre-commit checks run formatting, file hygiene, DVC-aware size checks, secret detection, and Vale/link checks for Markdown and Quarto files. The pre-push hooks run Clippy and `just check`; Rust tests remain in CI. Do not bypass the hooks with `--no-verify`.

Then install the pinned Python runtime and dependencies:

```sh
uv python install
```

`just check` runs the analysis tests and verifies links, claims, committed reports, and the Vale prose rules without rewriting them. `just ci-analysis` runs `just check` and `prek run --all-files` with `SKIP=cargo-fmt`; formatting is checked by `just ci-rust`, so the analysis job does not need to install Rust. The target sets `SKIP` only for the `prek` invocation. `just ci-rust` runs formatting, Clippy, and the Rust workspace tests. `just lint-prose` runs the pinned Vale style and its rule tests. To reproduce the M5-01 report from the committed ledger, run the single command:

```sh
just report-m5-01
```

This regenerates the tidy claim ledger, `.qmd`, and rendered GitHub-flavored Markdown with the pinned tools. Use `just claims` alone when only the ledger needs regeneration.

## Delivery

Normal delivery is `just ship`: it runs local checks, then pushes the tested `HEAD` to `ci-check`. A successful CI run is promoted to `main` automatically by a fast-forward-only workflow; a promoted commit then gets its own CI run on `main`. Never push directly to `main`.

One-time bootstrap only: push the initial CI branch-trigger and promotion-workflow commits directly to `main`, because GitHub runs the `workflow_run` promotion workflow from the version on the default branch.

## WSL (Linux) checks

The default Ubuntu WSL distro runs Linux checks from `~/simciv-ci`. These user-home installs need no `sudo`; the distro must already have `build-essential`, `pkg-config`, `curl`, and `git`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
cd /mnt/c/AI/SimulaCiv
rustup toolchain install
rustup component add rustfmt clippy

mkdir -p "$HOME/.local/bin"
curl -LsSf https://astral.sh/uv/0.13.5/install.sh | env UV_INSTALL_DIR="$HOME/.local/bin" UV_NO_MODIFY_PATH=1 sh

curl -fSL https://github.com/quarto-dev/quarto-cli/releases/download/v1.10.19/quarto-1.10.19-linux-amd64.tar.gz -o /tmp/quarto-1.10.19-linux-amd64.tar.gz
tar -xzf /tmp/quarto-1.10.19-linux-amd64.tar.gz -C "$HOME/.local"
ln -sfn "$HOME/.local/quarto-1.10.19/bin/quarto" "$HOME/.local/bin/quarto"

cargo +1.98.1 install just --version 1.58.0 --locked

curl -fsSL https://github.com/vale-cli/vale/releases/download/v3.25.1/vale_3.25.1_Linux_64-bit.tar.gz -o /tmp/vale.tar.gz
echo "4bf5c5176b682b1da0830f471584d3494f1c2e27ee3029e1ad6f6661d4962182  /tmp/vale.tar.gz" | sha256sum --check
tar -xzf /tmp/vale.tar.gz -C /tmp
install -m 0755 /tmp/vale "$HOME/.local/bin/vale"

curl -fsSL https://github.com/j178/prek/releases/download/v0.5.7/prek-x86_64-unknown-linux-gnu.tar.gz -o /tmp/prek.tar.gz
echo "e238e5a6b4e384e8d18f33d3c3b1a329be0e1237f77ac6e021e641ddd3a2bab6  /tmp/prek.tar.gz" | sha256sum --check
tar -xzf /tmp/prek.tar.gz -C /tmp
install -m 0755 /tmp/prek-x86_64-unknown-linux-gnu/prek "$HOME/.local/bin/prek"
```

This pins Rust to the repository's `rust-toolchain.toml` (`1.98.1`), uv `0.13.5`, Quarto `1.10.19`, just `1.58.0`, Vale `3.25.1`, and prek `0.5.7`. From Windows, run `just ci-linux`; it clones the current commit once, then fetches and checks it out cleanly on later runs. The WSL clone preserves only its `target/` build cache between runs.

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
