# M2/M3 reconstruction data

The DVC pipeline is [dvc.yaml](dvc.yaml). Raw outputs are produced into each
stage's `raw/` directory; summaries remain in Git and are finalized against
`dvc.lock` after a run. DVC uploads are deferred, so raw outputs remain only in
the maintainer's local cache until `dvc push --all-commits` is run. After upload,
retrieve them with `dvc pull` from this stage directory.

The currently prepared stages are the M2-06 deterministic manifest checks
and the M3-05 historical-protocol sensitivity add-on. Sensitivity conditions are registered in
[M3_05_PROTOCOL_SENSITIVITY.toml](../../reproducibility/M3_05_PROTOCOL_SENSITIVITY.toml).
The 15 remaining historical A-5 groups are listed by `just reconstruct-all
--dry-run`; the full run refuses to start until each has a streaming A-5
driver. No timing run has been performed for the sensitivity stage.
