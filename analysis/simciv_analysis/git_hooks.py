"""Small repository-local checks used by prek."""

from __future__ import annotations

import subprocess
import sys
from os import walk
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MAX_FILE_BYTES = 1024 * 1024


def dvc_output_paths(root: Path = ROOT) -> set[str]:
    """Return paths declared as DVC outputs by tracked pipeline/pointer files."""
    descriptors = []
    for directory, subdirs, filenames in walk(root):
        subdirs[:] = [name for name in subdirs if name not in {".git", "target", ".venv"}]
        descriptors.extend(
            Path(directory) / name
            for name in filenames
            if name == "dvc.yaml" or name.endswith(".dvc")
        )
    outputs: set[str] = set()
    for descriptor in descriptors:
        workdir = descriptor.parent
        in_outs = False
        outs_indent = -1
        for line in descriptor.read_text(encoding="utf-8").splitlines():
            stripped = line.strip()
            indent = len(line) - len(line.lstrip())
            if descriptor.name == "dvc.yaml" and stripped.startswith("wdir:"):
                value = stripped.split(":", 1)[1].strip().strip("'\"")
                workdir = (descriptor.parent / value).resolve()
            if stripped == "outs:":
                in_outs, outs_indent = True, indent
                continue
            if not in_outs:
                continue
            if indent < outs_indent or (indent == outs_indent and not stripped.startswith("-")):
                in_outs = False
                continue
            value = ""
            if stripped.startswith("path:"):
                value = stripped.split(":", 1)[1].strip().strip("'\"")
            elif stripped.startswith("-"):
                item = stripped[1:].strip()
                if item and ":" not in item:
                    value = item.strip("'\"")
                elif item.startswith("path:"):
                    value = item.split(":", 1)[1].strip().strip("'\"")
            if value:
                resolved = (workdir / value).resolve()
                try:
                    outputs.add(resolved.relative_to(root.resolve()).as_posix())
                except ValueError:
                    continue
    return outputs


def _is_dvc_output(path: str, outputs: set[str]) -> bool:
    return any(path == output or path.startswith(output.rstrip("/") + "/") for output in outputs)


def large_file_violations(
    root: Path,
    paths: list[str],
    staged_paths: set[str],
    outputs: set[str],
) -> list[tuple[str, int]]:
    violations = []
    for raw_path in paths:
        path = Path(raw_path).as_posix()
        if path not in staged_paths or _is_dvc_output(path, outputs):
            continue
        file_path = root / path
        if file_path.is_file() and file_path.stat().st_size > MAX_FILE_BYTES:
            violations.append((path, file_path.stat().st_size))
    return violations


def main(argv: list[str] | None = None) -> int:
    paths = argv if argv is not None else sys.argv[1:]
    staged = subprocess.check_output(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMRT", "-z"],
        cwd=ROOT,
    ).decode("utf-8").split("\0")
    staged_paths = {Path(path).as_posix() for path in staged if path}
    violations = large_file_violations(ROOT, paths, staged_paths, dvc_output_paths())
    if violations:
        for path, size in violations:
            print(f"{path}: {size} bytes exceeds the 1 MiB Git limit; track it with DVC")
        return 1
    print("Staged files satisfy the 1 MiB Git limit or are DVC outputs.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
