# M5-08 report-revision A-5 runs

This stage stores the raw outputs for the passed-gate timing cells run at their registered report revisions. The committed summaries and pre-run manifests are the Git record; raw JSONL stays DVC-managed and is currently only in the maintainer's local DVC cache. After upload, use `dvc pull docs/data/m5_08/dvc.yaml` to materialize it.

Run `just a5-tonight-prepare` once from a clean tree, commit the generated manifests, then run the groups in order with `just a5-tonight-run m2`, `m3_09`, `m3_11`, and `m4`. The commands resume completed cells from `target/a5_tonight/raw/`. Ingest complete protocol groups with `just a5-tonight-ingest <group>`; incomplete or invalid cells remain visible in the generated summaries and are not hidden.

Each protocol summary identifies its manifest and raw JSONL by SHA-256 and reports completed cells, validity, recalibration, and group wall time.
