# M3-05 A-5 reconstruction pilot

The DVC stage runs the committed driver at the M3-05 report revision with the frozen A-5 cells and writes JSONL into `raw/`. It writes a Git summary skeleton beside this README; after `dvc repro` updates the stage lock, finish the summary with `just provenance-finalize docs/data/m5_05/m3_05_pilot/summary.json`.

Full run:

```powershell
dvc repro docs/data/m5_05/m3_05_pilot/dvc.yaml:m3_05_a5_reconstruction
just provenance-finalize docs/data/m5_05/m3_05_pilot/summary.json
```

The smoke invocation writes only under ignored `target/`. It exercises the same calibration and retry code with a test-only 20 ms target, 10 ms floor, and one injected first-sample undershoot; these overrides are not used by full A-5 runs.

```powershell
uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot --output-dir target/m3_05_pilot_smoke --smoke
```
