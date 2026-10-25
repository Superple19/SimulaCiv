"""Enforce explicit DVC stage kinds, freeze records, and safe run targets."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from .provenance import ROOT


RECONSTRUCTION_TARGET = "docs/data/m2_m3_reconstruction/dvc.yaml:m3_05_protocol_sensitivity"
STAGE_LINE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")


def parse_stages(text: str) -> dict[str, dict[str, object]]:
    in_stages = False
    current: str | None = None
    in_meta = False
    result: dict[str, dict[str, object]] = {}
    for line in text.splitlines():
        if line.strip() == "stages:":
            in_stages = True
            continue
        if not in_stages:
            continue
        match = STAGE_LINE.match(line)
        if match:
            current = match.group(1)
            in_meta = False
            result[current] = {"kind": None, "frozen": False}
            continue
        if current is None:
            continue
        if line.startswith("    ") and line.strip() == "meta:":
            in_meta = True
        elif line.startswith("    ") and not line.startswith("      "):
            in_meta = False
            if line.strip() == "frozen: true":
                result[current]["frozen"] = True
        elif in_meta and line.startswith("      ") and line.strip().startswith("kind:"):
            result[current]["kind"] = line.strip().split(":", 1)[1].strip().strip("'\"")
        elif line.strip() and not line.startswith(" "):
            break
    return result


def _stages(path: Path) -> dict[str, dict[str, object]]:
    return parse_stages(path.read_text(encoding="utf-8")) if path.is_file() else {}


def _locked_stage_names(path: Path) -> set[str]:
    return set(_stages(path))


def frozen_timing_violations(root: Path = ROOT) -> list[str]:
    errors: list[str] = []
    for pipeline in sorted((root / "docs" / "data").rglob("dvc.yaml")):
        stages = _stages(pipeline)
        locked = _locked_stage_names(pipeline.with_name("dvc.lock"))
        for name, details in stages.items():
            kind = details["kind"]
            label = f"{pipeline.relative_to(root).as_posix()}:{name}"
            if kind not in {"timing", "deterministic"}:
                errors.append(f"{label} must declare meta.kind as timing or deterministic")
            elif kind == "timing" and name in locked and not details["frozen"]:
                errors.append(f"{label} has a lock but is not frozen")
    return errors


def lock_stage_outputs(text: str) -> dict[str, dict[str, str]]:
    """Parse DVC lock stage output path/hash pairs."""
    result: dict[str, dict[str, str]] = {}
    current_stage: str | None = None
    current_path: str | None = None
    in_stages = False
    in_outs = False
    for line in text.splitlines():
        if line.strip() == "stages:":
            in_stages = True
            continue
        if not in_stages:
            continue
        stage_match = STAGE_LINE.match(line)
        if stage_match:
            current_stage = stage_match.group(1)
            result.setdefault(current_stage, {})
            current_path = None
            in_outs = False
            continue
        if current_stage is None:
            continue
        if re.match(r"^    outs:\s*$", line):
            in_outs = True
            current_path = None
            continue
        if re.match(r"^    [A-Za-z_][A-Za-z0-9_-]*:", line):
            in_outs = False
            current_path = None
            continue
        if not in_outs:
            continue
        path_match = re.match(r"^    - path:\s*(.+?)\s*$", line)
        if path_match:
            current_path = path_match.group(1).strip("'\"").replace("\\", "/")
            result[current_stage][current_path] = ""
            continue
        md5_match = re.match(r"^      md5:\s*([0-9a-f]+(?:\.dir)?)\s*$", line)
        if md5_match and current_path is not None:
            result[current_stage][current_path] = md5_match.group(1)
            current_path = None
    return result


def frozen_output_hash_violations(
    pipeline_text: str,
    previous_lock_text: str,
    current_lock_text: str,
    label: str = "dvc.lock",
    previous_pipeline_text: str = "",
) -> list[str]:
    frozen = {
        name
        for source in (previous_pipeline_text, pipeline_text)
        for name, details in parse_stages(source).items()
        if details["frozen"]
    }
    previous = lock_stage_outputs(previous_lock_text)
    current = lock_stage_outputs(current_lock_text)
    return [
        f"{label}:{stage} frozen output hashes changed: {previous[stage]} -> {current.get(stage, {})}"
        for stage in sorted(frozen)
        if stage in previous and previous[stage] != current.get(stage, {})
    ]


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
    if block["kind"] != "timing":
        raise ValueError(f"reconstruct-all refuses non-timing DVC stage: {target}")
    if stage_name in _locked_stage_names(pipeline.with_name("dvc.lock")) and block["frozen"]:
        raise ValueError(f"reconstruct-all refuses frozen DVC stage: {target}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.parse_args()
    errors = frozen_timing_violations()
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("all DVC stages declare a kind; locked timing stages are frozen")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
