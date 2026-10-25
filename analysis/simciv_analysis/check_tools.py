"""Require the repository's pinned mise-managed tools to be active on PATH."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys

from .tool_versions import ROOT, mise_version, rust_toolchain_version, tool_version


TOOLS = ("uv", "just", "prek", "vale", "quarto", "dvc")
VERSION = re.compile(r"(?<!\d)(\d+\.\d+\.\d+)(?!\d)")


def _run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, capture_output=True, text=True, check=False, timeout=15)


def _normalized(path: str) -> str:
    return os.path.normcase(os.path.abspath(path))


def check_tools() -> list[str]:
    errors: list[str] = []
    mise = shutil.which("mise")
    if mise is None:
        return ["mise is missing from PATH; install and activate the pinned mise version"]

    result = _run([mise, "--version"])
    actual = VERSION.search(result.stdout + result.stderr)
    expected_mise = mise_version()
    if result.returncode or actual is None or actual.group(1) != expected_mise:
        errors.append(f"mise: expected {expected_mise}, found {(actual.group(1) if actual else 'unknown')}")

    for name in TOOLS:
        executable = shutil.which(name)
        if executable is None:
            errors.append(f"{name}: missing from PATH; run mise install --locked and activate mise")
            continue
        expected_path = _run([mise, "which", name])
        managed = expected_path.stdout.strip().splitlines()[-1] if expected_path.stdout.strip() else ""
        if expected_path.returncode or not managed or _normalized(executable) != _normalized(managed):
            errors.append(f"{name}: PATH resolves to {executable}, not the mise-managed binary {managed or '<missing>'}")
            continue

        version_result = _run([executable, "--version"])
        version = VERSION.search(version_result.stdout + version_result.stderr)
        expected = tool_version(name)
        if version_result.returncode or version is None or version.group(1) != expected:
            errors.append(f"{name}: expected {expected}, found {(version.group(1) if version else 'unknown')} at {executable}")

    rustc = shutil.which("rustc")
    if rustc is None:
        errors.append("rustc is missing from PATH; install rust-toolchain.toml with rustup")
    else:
        result = _run([rustc, "--version"])
        actual = VERSION.search(result.stdout + result.stderr)
        expected = rust_toolchain_version(ROOT)
        if result.returncode or actual is None or actual.group(1) != expected:
            errors.append(f"rustc: expected toolchain {expected}, found {(actual.group(1) if actual else 'unknown')} at {rustc}")

    return errors


def main() -> int:
    errors = check_tools()
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("mise tools and the active Rust compiler match repository pins")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
