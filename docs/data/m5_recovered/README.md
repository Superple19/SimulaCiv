# M5 recovered historical evidence

This is an immutable, byte-exact archive of artifacts recovered from local build outputs, temporary outputs, DVC cache objects, and narrowly selected Codex session records. It is evidence of earlier work, not a rerun. Do not edit or normalize files under `recovered/`.

- Manifest: [`MANIFEST.json`](MANIFEST.json) lists original paths, mtimes, byte sizes, SHA-256 hashes, source locations, completeness notes, supported reports, and each session extract's source file, JSONL line, timestamp, and content digest.
- DVC output: [`recovered.dvc`](recovered.dvc) tracks the `recovered/` directory. The data is currently in the maintainer's local DVC cache; no remote upload has been performed. After upload, retrieve it with `dvc pull docs/data/m5_recovered/recovered.dvc`.
- Recovery source: `352 recovered files totaling 280,463,762 bytes, plus 208 session extracts (143 edit patches, 4 source bodies, 61 command outputs).`

## Artifact inventory

| Type | Files |
|---|---:|
| `captured-report-source` | 1 |
| `cargo-dependency-file` | 28 |
| `compiled-example-binary` | 18 |
| `debug-symbols` | 18 |
| `dvc-cache-raw-output` | 46 |
| `dvc-cache-unreferenced-object` | 23 |
| `dvc-directory-metadata` | 18 |
| `historical-bench-source-snapshot` | 1 |
| `historical-source-from-git` | 2 |
| `raw-output-copy` | 168 |
| `reconstructed-source` | 10 |
| `reconstructed-source-from-command` | 4 |
| `session-log-extracts` | 1 |
| `source-snapshot-initial-add` | 2 |
| `temporary-analysis-script-copy` | 13 |

## Immutability and reruns

Historical artifacts stay byte-exact. Never execute a recovered binary or write a rerun into `recovered/`. To attempt a reproduction, first check the artifact's completeness notes and the linked report, use a clean worktree at the report's stated revision, rebuild only from recovered source text after confirming the toolchain and dependencies, and write new results to a separate, newly registered DVC output. Record the harness, command, source revision, and checksums; do not overwrite the historical evidence.

## Exclusions

Whole Codex session logs and broad conversation transcripts are not stored. The JSONL extract file contains only relevant edit bodies, source bodies, and outputs from runs, each attributed to a session filename, source line, and timestamp. Session extracts may retain historical personal filesystem paths when those paths were printed by a command or embedded in a source diff; see the final audit of this stage before interpreting such paths.
