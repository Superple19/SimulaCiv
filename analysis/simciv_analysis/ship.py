"""Push a clean working tree to ci-check; pre-push hooks run the local gates."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    status = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=all"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if status.returncode:
        print(status.stderr or "could not inspect the working tree", file=sys.stderr)
        return status.returncode
    if status.stdout:
        print(f"just ship requires a clean working tree:\n{status.stdout}", file=sys.stderr)
        return 1
    print("working tree clean; pushing HEAD to ci-check")
    return subprocess.run(["git", "push", "origin", "HEAD:ci-check"], cwd=ROOT, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
