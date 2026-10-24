"""Small repository-local checks used by prek."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from os import walk
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MAX_FILE_BYTES = 1024 * 1024
UTF8_BOM = b"\xef\xbb\xbf"
MESSAGE_EXPECTATION = (
    "expected format: <type>: <subject> (<=72 characters)\n"
    "followed by one blank line and at least one contiguous '- <bullet>' line "
    "(each <=100 characters); no prose, blank bullet lines, or trailing whitespace"
)


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


def bom_violations(root: Path, paths: list[str]) -> list[str]:
    return [
        Path(raw_path).as_posix()
        for raw_path in paths
        if (path := root / raw_path).is_file() and path.read_bytes().startswith(UTF8_BOM)
    ]


def commit_message_error(message: str) -> str | None:
    """Validate the repository's complete commit-message allowlist."""
    lines = [line for line in message.replace("\r\n", "\n").replace("\r", "\n").splitlines() if not line.startswith("#")]
    if not lines:
        return "message is empty"
    if any(line.endswith((" ", "\t")) for line in lines):
        return "trailing whitespace is not allowed"
    subject = lines[0]
    if len(subject) > 72 or not re.fullmatch(r"(?:feat|fix|docs|perf|data|test|chore|ci): \S.*", subject):
        return "line 1 must be '<type>: <subject>' with an allowed type and at most 72 characters"
    if len(lines) == 1:
        return "a body with at least one bullet is required"
    if lines[1] != "":
        return "the body must start after exactly one blank line"
    body = lines[2:]
    if not body:
        return "a body must contain at least one bullet"
    for number, line in enumerate(body, start=3):
        if re.search(r"(?<!\S)-m(?!\S)", line):
            return f"body line {number} contains a stray '-m' token"
        if not re.fullmatch(r"- \S.*", line):
            return f"body line {number} must be a contiguous '- <bullet>' line"
        if len(line) > 100:
            return f"body line {number} exceeds 100 characters"
    return None


def _git_output(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=root, text=True, encoding="utf-8").strip()


def _commit_range(root: Path, environment: dict[str, str]) -> tuple[str, str] | None:
    event_name = environment.get("GITHUB_EVENT_NAME")
    event_path = environment.get("GITHUB_EVENT_PATH")
    if event_name == "workflow_dispatch":
        return None
    if event_name and event_path:
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        if event_name == "push":
            before, after = event.get("before", ""), event.get("after", "")
            if before and set(before) == {"0"}:
                before = _git_output(root, "merge-base", "origin/main", after)
            return before, after
        if event_name == "pull_request":
            return event["pull_request"]["base"]["sha"], event["pull_request"]["head"]["sha"]
    head = _git_output(root, "rev-parse", "HEAD")
    base = _git_output(root, "merge-base", "origin/main", head)
    return base, head


def commit_range_violations(root: Path, base: str, head: str) -> list[tuple[str, str]]:
    commits = _git_output(root, "rev-list", "--reverse", f"{base}..{head}").splitlines()
    violations = []
    for commit in commits:
        raw = subprocess.check_output(["git", "cat-file", "commit", commit], cwd=root)
        message = raw.split(b"\n\n", 1)[1].decode("utf-8")
        if error := commit_message_error(message):
            violations.append((commit, error))
    return violations


def main(argv: list[str] | None = None) -> int:
    paths = argv if argv is not None else sys.argv[1:]
    if paths[:1] == ["--check-commit-message"]:
        if len(paths) != 2:
            print("commit message path is required")
            return 2
        message_path = Path(paths[1])
        if not message_path.is_absolute():
            message_path = ROOT / message_path
        message = message_path.read_text(encoding="utf-8")
        if error := commit_message_error(message):
            print(f"invalid commit message: {error}\n{MESSAGE_EXPECTATION}")
            return 1
        return 0
    if paths[:1] == ["--check-commit-range"]:
        commit_range = _commit_range(ROOT, os.environ)
        if commit_range is None:
            print("no pushed commit range for this workflow event")
            return 0
        violations = commit_range_violations(ROOT, *commit_range)
        if violations:
            for commit, error in violations:
                print(f"{commit}: invalid commit message: {error}")
            print(MESSAGE_EXPECTATION)
            return 1
        count = len(_git_output(ROOT, "rev-list", f"{commit_range[0]}..{commit_range[1]}").splitlines())
        print(f"all {count} commits in range satisfy the allowlist")
        return 0
    if paths[:1] == ["--check-bom"]:
        violations = bom_violations(ROOT, paths[1:])
        if violations:
            for path in violations:
                print(f"{path}: UTF-8 BOM is not allowed")
            return 1
        return 0
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
