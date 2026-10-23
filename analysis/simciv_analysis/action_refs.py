"""Check that CI workflow action refs resolve to upstream Git tags."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIR = ROOT / ".github" / "workflows"
USES = re.compile(r"^\s*(?:-\s*)?uses:\s*([^\s#]+)")
EXACT_RELEASE_TAG = re.compile(r"v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\Z")


def workflow_paths(root: Path = ROOT) -> list[Path]:
    return sorted((root / ".github" / "workflows").glob("*.yml"))


def action_refs(workflow: str) -> list[tuple[str, str]]:
    refs = []
    for line in workflow.splitlines():
        match = USES.match(line)
        if not match:
            continue
        source = match.group(1)
        if source.startswith(("./", "docker://")):
            continue
        target, separator, ref = source.rpartition("@")
        parts = target.split("/")
        repository = "/".join(parts[:2])
        if len(parts) >= 2:
            refs.append((repository, ref))
    return refs


def invalid_action_refs(workflow: str) -> list[tuple[str, str]]:
    return [(repository, ref) for repository, ref in action_refs(workflow) if not EXACT_RELEASE_TAG.fullmatch(ref)]


def missing_action_refs(workflow: str) -> list[str]:
    missing = []
    for repository, ref in action_refs(workflow):
        if not EXACT_RELEASE_TAG.fullmatch(ref):
            continue
        result = subprocess.run(
            [
                "git",
                "ls-remote",
                "--exit-code",
                "--tags",
                "--refs",
                f"https://github.com/{repository}.git",
                f"refs/tags/{ref}",
            ],
            capture_output=True,
            check=False,
            text=True,
        )
        exact_tag = any(
            len(fields := line.split()) == 2 and fields[1] == f"refs/tags/{ref}"
            for line in result.stdout.splitlines()
        )
        if result.returncode != 0 or not exact_tag:
            missing.append(f"{repository}@{ref}")
    return missing


def main() -> int:
    refs = 0
    missing = []
    invalid = []
    workflows = workflow_paths()
    for path in workflows:
        try:
            workflow = path.read_text(encoding="utf-8")
        except OSError as error:
            print(f"cannot read {path}: {error}", file=sys.stderr)
            return 1
        refs += len(action_refs(workflow))
        invalid.extend((path.name, repository, ref) for repository, ref in invalid_action_refs(workflow))
        missing.extend((path.name, ref) for ref in missing_action_refs(workflow))
    if invalid:
        for workflow, repository, ref in invalid:
            print(f"{workflow} action ref must be a full vX.Y.Z release tag: {repository}@{ref}", file=sys.stderr)
        return 1
    if missing:
        for workflow, ref in missing:
            print(f"{workflow} action tag does not resolve: {ref}", file=sys.stderr)
        return 1
    print(f"All {refs} action refs across {len(workflows)} workflows use exact vX.Y.Z tags that resolve upstream.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
