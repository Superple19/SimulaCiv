"""Enforce freeze records for locked timing stages and safe reconstruction targets."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from .provenance import ROOT


RECONSTRUCTION_TARGET = "docs/data/m2_m3_reconstruction/dvc.yaml:m3_05_protocol_sensitivity"
STAGE_LINE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")


def _stages(path: Path) -> dict[str, list[str]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    in_stages = False
    current: str | None = None
    result: dict[str, list[str]] = {}
    for line in lines:
        if line.strip() == "stages:":
            in_stages = True
            continue
        if not in_stages:
            continue
        match = STAGE_LINE.match(line)
        if match:
            current = match.group(1)
            result[current] = []
        elif current is not None and line.startswith("    "):
            result[current].append(line.strip())
        elif line.strip() and not line.startswith(" "):
            break
    return result


def _is_timing_stage(name: str, block: list[str]) -> bool:
    command = " ".join(block).casefold()
    return (
        " timing " in f" {command} "
        or "--mode a5" in command
        or "protocol-sensitivity" in command
        or "diagnosis" in name.casefold()
        or "a5" in name.casefold()
        or "validate_for" in name.casefold()
    )


def _locked_stage_names(path: Path) -> set[str]:
    return set(_stages(path)) if path.is_file() else set()


def frozen_timing_violations(root: Path = ROOT) -> list[str]:
    errors: list[str] = []
    for pipeline in sorted((root / "docs" / "data").rglob("dvc.yaml")):
        blocks = _stages(pipeline)
        locked = _locked_stage_names(pipeline.with_name("dvc.lock"))
        for name, block in blocks.items():
            if name in locked and _is_timing_stage(name, block) and "frozen: true" not in block:
                errors.append(f"{pipeline.relative_to(root).as_posix()}:{name} has a lock but is not frozen")
    return errors


def validate_repro_target(target: str, root: Path = ROOT) -> None:
    if target != RECONSTRUCTION_TARGET:
        raise ValueError(f"reconstruct-all may only run its registered stage: {RECONSTRUCTION_TARGET}")
    pipeline_text, separator, stage_name = target.partition(":")
    if not separator:
        raise ValueError("reconstruction target must include a DVC stage name")
    pipeline = root / pipeline_text
    block = _stages(pipeline).get(stage_name)
    if block is None:
        raise ValueError(f"registered reconstruction stage is missing: {target}")
    if stage_name in _locked_stage_names(pipeline.with_name("dvc.lock")) and (
        "frozen: true" in block
    ):
        raise ValueError(f"reconstruct-all refuses frozen DVC stage: {target}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.parse_args()
    errors = frozen_timing_violations()
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("all locked timing DVC stages are frozen")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
