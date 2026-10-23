"""Record source-level attribution for the committed M3-09 bisect result."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path
from typing import Any

from .provenance import ROOT


SUMMARY = Path("docs/data/m5_05/m3_09_diagnosis/bisect_summary.json")
THIS_FILE = "analysis/simciv_analysis/m5_05_diagnosis_postprocess.py"


def annotate(path: Path = ROOT / SUMMARY, root: Path = ROOT) -> dict[str, Any]:
    summary = json.loads(path.read_text(encoding="utf-8"))
    commit = summary.get("first_changed_commit")
    if not commit:
        raise ValueError("bisect summary has no first changed commit")
    name_status = subprocess.run(
        ["git", "show", "--format=%s", "--name-only", str(commit)],
        cwd=root, text=True, capture_output=True, check=True,
    ).stdout.splitlines()
    subject = next((line for line in name_status if line.strip()), "")
    changed_files = [line for line in name_status if line.strip() and line != subject]
    patch = subprocess.run(
        ["git", "show", "--format=", "--unified=0", str(commit), "--", "crates/sim-model/src/m3p.rs"],
        cwd=root, text=True, capture_output=True, check=True,
    ).stdout
    helper = "apply_phase9b_storage_incremental_append"
    phase = (
        "Phase 9B incremental non-compaction append path"
        if helper in patch
        else "not identified in the changed M3P source diff"
    )
    summary["change_evidence"] = {
        "commit": commit,
        "subject": subject,
        "changed_files": changed_files,
        "phase_attribution": phase,
        "phase_function": helper if helper in patch else None,
        "timer_scope": "the committed example timer brackets the public three-day runner call; no internal Phase 9B stopwatch",
    }
    summary["phase_attribution"] = f"{phase} (`{helper}`)" if helper in patch else phase
    summary["bisect_patch_has_phase9b_append"] = helper in patch
    summary["harness_files"] = sorted(set(summary.get("harness_files", [])) | {THIS_FILE})
    path.write_text(json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary", nargs="?", type=Path, default=SUMMARY)
    args = parser.parse_args()
    path = args.summary if args.summary.is_absolute() else ROOT / args.summary
    result = annotate(path)
    print(f"recorded {result['change_evidence']['phase_attribution']} for {result['first_changed_commit']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
