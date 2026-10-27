# Research data

Specs, summaries, reports, generated tables/figures, golden files, and DVC pointer files live in Git. New full experiment and measurement outputs are tracked by DVC under `docs/data/<stage>/<entry>/raw/`; older raw files already committed to Git remain there unchanged. See the [M5 reproducibility contract](../contracts/M5_REPRODUCIBILITY_CONTRACT.md#14-m5-00-a-1a-2-data-and-provenance-amendment-frozen) for the storage and provenance rules.

## Pulling DVC data

DVC uploads are deferred. Raw outputs currently exist only in the maintainer's local DVC cache until `dvc push --all-commits` is run; the remote has no raw outputs yet. After upload, install the DVC version listed in [mise.toml](../../mise.toml) with `mise install --locked` and pull from the repository root with `dvc pull`. `just check` does not require DVC or `dvc pull`; reports read only Git-committed summary files.

## Maintainer authentication

Only maintainers who upload data need a token. The shared `.dvc/config` stores the remote URL; credentials are stored only in ignored `.dvc/config.local`. In PowerShell:

```powershell
$env:DAGSHUB_USER = Read-Host "DagsHub username"
$secure = Read-Host "DagsHub token" -AsSecureString
$env:DAGSHUB_TOKEN = [System.Net.NetworkCredential]::new("", $secure).Password
dvc remote modify dagshub --local auth basic
dvc remote modify dagshub --local user $env:DAGSHUB_USER
dvc remote modify dagshub --local password $env:DAGSHUB_TOKEN
Remove-Item Env:DAGSHUB_TOKEN, Env:DAGSHUB_USER
```

Never stage or commit `.dvc/config.local`. After the corresponding Git commit has been pushed, a maintainer can upload its DVC objects with `dvc push`.

## Existing Git data

| Folder | Contents |
|---|---|
| [`m4_02/`](m4_02/README.md) | M4-02 historical paired runs, host records, and worked-example traces. |
| [`m4_03/`](m4_03/README.md) | M4-03 divergence experiments and gate records. |
| [`m4_04/`](m4_04/README.md) | M4-04 CRN experiment outputs. |
| [`m4_05/`](m4_05/README.md) | M4-05 minimal-cause logs and probes. |
| [`m4_06/`](m4_06/README.md) | M4-06 migration gates and experiments. |
| [`m4_07/`](m4_07/README.md) | M4-07 branch, day-scan, and search outputs. |
| [`m4_viewer/`](m4_viewer/README.md) | Sample JSONL trace. |
| [`m4_inventory/`](m4_inventory/README.md) | M4 artifact recovery inventory. |

New DVC-backed stage directories are indexed here after their measurement summaries are committed.

The byte-exact recovered archive is indexed in [`m5_recovered/`](m5_recovered/README.md); its DVC payload remains in the maintainer’s local cache until upload.

The M5-05 pipeline definition and lockfile live together in [`m5_05/dvc.yaml`](m5_05/dvc.yaml) and [`m5_05/dvc.lock`](m5_05/dvc.lock). DVC targets use that pipeline path, for example `dvc status docs/data/m5_05/dvc.yaml`.

The report-revision A-5 runs are defined by [`m5_08/dvc.yaml`](m5_08/dvc.yaml); see [`m5_08/README.md`](m5_08/README.md) for the run and post-upload `dvc pull` procedure. The raw outputs are currently only in the maintainer's local DVC cache.
