# SimulaCiv analysis

Standalone tools are pinned in [mise.toml](../mise.toml) and locked in [mise.lock](../mise.lock). The analysis project pins uv with `tool.uv.required-version` in `analysis/pyproject.toml` and Python with `analysis/.python-version`; Python package versions are in `uv.lock`. The package supports Python >=3.11.

## Setup

Install mise as described below, activate it in the current shell, then run `mise trust --yes` and `mise install --locked` from the repository root. Install the Git hooks with:

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
(cd analysis && uv python install)
```

`just check` runs the analysis tests and verifies links, claims, committed reports, and the Vale prose rules without rewriting them. `just ci-analysis` runs `just check` and `prek run --all-files` with `SKIP=cargo-fmt`; formatting is checked by `just ci-rust`, so the analysis job does not need to install Rust. The target sets `SKIP` only for the `prek` invocation. `just ci-rust` runs formatting, Clippy, and the Rust workspace tests. `just lint-prose` runs the pinned Vale style and its rule tests. To reproduce the M5-01 report from the committed ledger, run the single command:

```sh
just report-m5-01
```

This regenerates the tidy claim ledger, `.qmd`, and rendered GitHub-flavored Markdown with the pinned tools. Use `just claims` alone when only the ledger needs regeneration.

## Report-revision A-5 provenance

In the report-revision worktree, after registered harness overlays are in place and before starting a timed stage, run `just a5-manifest <protocol-id> <run-id> <manifest-path>` under `docs/reproducibility/evidence/`. It records the worktree revision, exact `rustc -Vv`, host identity, A-5 settings, and overlay hashes. After a completed raw run, `just a5-ingest <manifest-path> <raw-jsonl-path>` validates the ordered cells, 41 sample durations, path spreads, and hashes, then appends the report-role evidence record. M2/M4 adapter JSONL and M3-05 `SCALE_*` output are normalized by the ingester; smoke output is rejected. `just a5-dry-run` lists the 17 tonight cells and estimates each from report-revision timing rows, including warmups, calibration, samples, and setup.

## Delivery

Normal delivery is `just ship`: it runs local checks, then pushes the tested `HEAD` to `ci-check`. The same CI run promotes a successful commit to `main` automatically by fast-forward; no second CI run is dispatched. Never push directly to `main`.

## Windows setup

Install mise with WinGet, reopen PowerShell so the new command is on `PATH`, then update mise itself to the bootstrap pin in `mise.toml`, activate it, and install the locked tools:

```powershell
winget install --id jdx.mise --exact
# Reopen PowerShell before continuing.
$miseVersion = (Select-String -Path mise.toml -Pattern '^# mise-bootstrap-version = "([^\"]+)"$').Matches.Groups[1].Value
mise self-update $miseVersion --yes --no-plugins
mise activate pwsh | Out-String | Invoke-Expression
mise trust --yes
mise install --locked
Push-Location analysis
uv python install
Pop-Location
```

Run the activation command in each new PowerShell session, or add it to your PowerShell profile.

For non-interactive PowerShell sessions, add mise's shims directory to the **user** PATH (do not add individual install directories):

```powershell
mise reshim
$miseShims = Join-Path $env:LOCALAPPDATA 'mise\shims'
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $miseShims) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$miseShims", 'User')
}
# Open a new shell for the PATH change to take effect.
```

On Windows, disable Git's automatic EOL conversion for this checkout before working with repository data:

```powershell
git config --local core.autocrlf false
```

## WSL (Linux) checks

The default Ubuntu WSL distro runs Linux checks from `~/simciv-ci`. These user-home installs need no `sudo`; the distro must already have `build-essential`, `pkg-config`, `curl`, and `git`:

```bash
cd /mnt/c/AI/SimulaCiv
mise_version="$(sed -n 's/^# mise-bootstrap-version = "\([^"]*\)"$/\1/p' mise.toml)"
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
curl -fsSL https://mise.run | MISE_VERSION="v${mise_version}" sh
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
mise trust --yes
mise install --locked
eval "$(mise activate bash)"
(cd analysis && uv python install)
rustup toolchain install
```

For fresh non-interactive login shells, add mise's shims directory to `~/.profile`; this avoids requiring `mise activate` in each command:

```bash
mise reshim
grep -Fq 'mise/shims' ~/.profile || printf '%s\n' 'export PATH="$HOME/.local/share/mise/shims:$PATH"' >> ~/.profile
bash --login -c 'command -v mise just uv prek'
```

`just ci-linux` uses `mise exec` for its checks, while the shims make the same pinned tools available to other non-interactive login commands.

Rust remains pinned in `rust-toolchain.toml`; the standalone tool versions and platform artifacts come from `mise.toml` and `mise.lock`. From Windows, run `just ci-linux`; it refreshes a clean WSL clone, installs the locked mise tools there, and runs `mise exec -- just ci-rust` and `mise exec -- just ci-analysis`, matching the hosted CI commands. The WSL clone preserves only its `target/` build cache between runs. WSL currently uses Ubuntu 24.04.3 LTS, while hosted CI uses the `ubuntu-26.04` x64 image; `just ci-linux` matches CI's commands, not its runner OS version.

## DVC experiment data

DVC is pinned in `mise.toml` and installed in its own environment with a private Python runtime; this does not change `analysis/.python-version`.

The shared DVC remote URL is in `.dvc/config`, but uploads are deferred and the remote currently has no raw outputs. Raw outputs exist only in the maintainer's local DVC cache until `dvc push --all-commits` is run. After upload, pull raw data with:

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
