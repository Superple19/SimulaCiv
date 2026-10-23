"""Run the analysis checks in a fresh Git worktree without ignored files."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    result_code = 1
    with tempfile.TemporaryDirectory(prefix="simciv-ci-clean-") as temporary:
        worktree = Path(temporary) / "checkout"
        added = subprocess.run(
            ["git", "worktree", "add", "--detach", str(worktree), "HEAD"],
            cwd=ROOT,
            check=False,
        )
        if added.returncode != 0:
            return added.returncode
        try:
            print(f"Running just check in clean worktree: {worktree}", flush=True)
            result_code = subprocess.run(["just", "check"], cwd=worktree, check=False).returncode
        finally:
            removed = subprocess.run(
                ["git", "worktree", "remove", "--force", str(worktree)],
                cwd=ROOT,
                check=False,
            )
            if removed.returncode != 0:
                print(f"failed to remove temporary worktree: {worktree}", file=sys.stderr)
                result_code = removed.returncode
    return result_code


if __name__ == "__main__":
    raise SystemExit(main())
