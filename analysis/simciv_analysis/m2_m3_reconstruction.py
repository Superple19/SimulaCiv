"""Generate the M2/M3 reconstruction inventory from its committed inputs."""

from __future__ import annotations

import argparse
import sys
import tempfile
from pathlib import Path

from .io import content_matches, write_text_lf
from .provenance import ROOT
from . import m2_m3_evidence_inventory as source_inventory
from . import m3_05_pilot_runtime as pilot_runtime

CSV = Path("docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv")
REPORT = Path("docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.md")


def build_outputs(root: Path = ROOT) -> dict[Path, bytes]:
    from . import m2_m3_reconstruction_finalize as finalize

    temporary_root = root / "target"
    temporary_root.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="simciv-reconstruction-", dir=temporary_root) as temporary:
        workspace = Path(temporary)
        csv_path, report_path = workspace / CSV.name, workspace / REPORT.name
        source_outputs = source_inventory.build_outputs()
        write_text_lf(csv_path, source_outputs[source_inventory.OUT_CSV].decode("utf-8"))
        write_text_lf(report_path, source_outputs[source_inventory.OUT_MD].decode("utf-8"))
        original = (finalize.CSV_PATH, finalize.MD_PATH)
        try:
            finalize.CSV_PATH, finalize.MD_PATH = csv_path, report_path
            finalize.main()
            summary_outputs = pilot_runtime.build_summary_outputs(
                root,
                inventory_content=csv_path.read_bytes(),
                report_content=report_path.read_bytes(),
            )
            return {
                root / CSV: summary_outputs[root / pilot_runtime.INVENTORY.relative_to(ROOT)],
                root / REPORT: summary_outputs[root / pilot_runtime.INVENTORY_REPORT.relative_to(ROOT)],
                root / "docs/reproducibility/M2_M3_REMAINING_RUNTIME.csv": summary_outputs[root / pilot_runtime.PLAN_CSV.relative_to(ROOT)],
            }
        finally:
            finalize.CSV_PATH, finalize.MD_PATH = original


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
