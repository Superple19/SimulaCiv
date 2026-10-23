"""Check that CI workflow action refs resolve to upstream Git tags."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
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
    try:
        workflow = WORKFLOW.read_text(encoding="utf-8")
    except OSError as error:
        print(f"cannot read {WORKFLOW}: {error}", file=sys.stderr)
        return 1
    missing = missing_action_refs(workflow)
    if missing:
        for ref in missing:
            print(f"CI action tag does not resolve: {ref}", file=sys.stderr)
        return 1
    print(f"All {len(action_refs(workflow))} CI action refs resolve to Git tags.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
