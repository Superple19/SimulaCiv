"""Check that CI workflow action refs resolve to upstream Git tags."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = (
    ROOT / ".github" / "workflows" / "ci.yml",
    ROOT / ".github" / "workflows" / "promote.yml",
)
USES = re.compile(r"^\s*(?:-\s*)?uses:\s*([^\s#]+)")


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
        if separator and len(parts) >= 2 and ref:
            refs.append((repository, ref))
    return refs


def missing_action_refs(workflow: str) -> list[str]:
    missing = []
    for repository, ref in action_refs(workflow):
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
    for path in WORKFLOWS:
        try:
            workflow = path.read_text(encoding="utf-8")
        except OSError as error:
            print(f"cannot read {path}: {error}", file=sys.stderr)
            return 1
        refs += len(action_refs(workflow))
        missing.extend((path.name, ref) for ref in missing_action_refs(workflow))
    if missing:
        for workflow, ref in missing:
            print(f"{workflow} action tag does not resolve: {ref}", file=sys.stderr)
        return 1
    print(f"All {refs} action refs across {len(WORKFLOWS)} workflows resolve to Git tags.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
