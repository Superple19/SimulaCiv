"""Run repository hooks from the analysis CI target without Rust setup."""

from __future__ import annotations

import os
import subprocess


def main() -> int:
    environment = os.environ.copy()
    environment["SKIP"] = "cargo-fmt"
    return subprocess.run(["prek", "run", "--all-files"], env=environment, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
