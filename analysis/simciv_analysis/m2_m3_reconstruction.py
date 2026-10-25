"""Generate the M2/M3 reconstruction inventory from its committed inputs."""

from __future__ import annotations

import argparse
import csv
import shutil
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

from .io import content_matches, write_text_lf
from .provenance import ROOT
from . import m3_05_pilot_runtime as pilot_runtime

CSV = Path("docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv")
REPORT = Path("docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.md")


def build_outputs(root: Path = ROOT) -> dict[Path, bytes]:
    from . import m2_m3_reconstruction_finalize as finalize

    with tempfile.TemporaryDirectory(dir=root / "target") as temporary:
        workspace = Path(temporary)
        csv_path, report_path = workspace / CSV.name, workspace / REPORT.name
        shutil.copyfile(root / CSV, csv_path)
        shutil.copyfile(root / REPORT, report_path)
        criteria_path, decisions_path = workspace / "REPRODUCTION_CRITERIA.md", workspace / "decisions.toml"
        shutil.copyfile(root / "docs/reproducibility/REPRODUCTION_CRITERIA.md", criteria_path)
        shutil.copyfile(root / "contracts/decisions.toml", decisions_path)
        original = (
            finalize.CSV_PATH, finalize.MD_PATH, finalize.CRITERIA_PATH, finalize.DECISIONS_PATH,
            pilot_runtime.INVENTORY,
        )
        try:
            finalize.CSV_PATH, finalize.MD_PATH = csv_path, report_path
            finalize.CRITERIA_PATH = criteria_path
            finalize.DECISIONS_PATH = decisions_path
            finalize.main()
            series_path = root / pilot_runtime.SERIES_CSV.relative_to(ROOT)
            with series_path.open(encoding="utf-8-sig", newline="") as stream:
                series_rows = list(csv.DictReader(stream))
            by_n_kind: dict[tuple[int, str], list[float]] = defaultdict(list)
            for row in series_rows:
                by_n_kind[(int(row["N"]), row["series_type"])].append(float(row["known_elapsed_estimate_seconds"]))
            per_n = []
            for n in sorted({int(row["N"]) for row in series_rows}):
                total = [float(row["known_elapsed_estimate_seconds"]) for row in series_rows if int(row["N"]) == n]
                per_n.append({"N": n, "mean_per_series_seconds": sum(total) / len(total)})
            model = pilot_runtime._power_law([(float(row["N"]), float(row["mean_per_series_seconds"])) for row in per_n])
            for row in per_n:
                row["fitted_seconds"] = model(float(row["N"]))
            pilot_runtime.INVENTORY = csv_path
            inventory_csv, plan_csv, _, _ = pilot_runtime._inventory_outputs(root, per_n)
            return {
                root / CSV: inventory_csv,
                root / "docs/reproducibility/M2_M3_REMAINING_RUNTIME.csv": plan_csv,
            }
        finally:
            (
                finalize.CSV_PATH, finalize.MD_PATH, finalize.CRITERIA_PATH, finalize.DECISIONS_PATH,
                pilot_runtime.INVENTORY,
            ) = original


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    try:
        outputs = build_outputs()
        if args.check:
            different = [path.relative_to(ROOT).as_posix() for path, content in outputs.items() if not content_matches(path, content)]
            if different:
                print("reconstruction inventory differs: " + ", ".join(different), file=sys.stderr)
                return 1
            print("reconstruction inventory matches committed generator output")
            return 0
        for path, content in outputs.items():
            write_text_lf(path, content.decode("utf-8"))
    except (OSError, KeyError, ValueError, RuntimeError) as error:
        print(f"reconstruction inventory generation failed: {error}", file=sys.stderr)
        return 1
    print(f"generated {len(outputs)} reconstruction inventory files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
