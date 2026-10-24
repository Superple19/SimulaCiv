"""Fail before report work if the pinned Quarto CLI is unavailable."""

from __future__ import annotations

import shutil
import subprocess
import sys

from simciv_analysis.tool_versions import tool_version


PINNED_VERSION = tool_version("quarto")


def main() -> int:
    executable = shutil.which("quarto")
    if executable is None:
        print(f"Quarto CLI {PINNED_VERSION} is required on PATH; install the pinned version from analysis/README.md", file=sys.stderr)
        return 1
    try:
        result = subprocess.run([executable, "--version"], check=True, capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as error:
        print(f"could not run Quarto at {executable}: {error}", file=sys.stderr)
        return 1
    actual = result.stdout.strip()
    if actual != PINNED_VERSION:
        print(f"Quarto version mismatch: expected {PINNED_VERSION}, found {actual or '<empty>'} at {executable}", file=sys.stderr)
        return 1
    print(f"Quarto {actual} is available at {executable}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
