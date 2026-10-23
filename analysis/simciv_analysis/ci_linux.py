"""Run the committed CI checks in the default WSL Ubuntu checkout."""

from __future__ import annotations

import shlex
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DISTRO = "Ubuntu"


def _wsl_path() -> str:
    result = subprocess.run(
        ["wsl.exe", "-d", DISTRO, "--", "wslpath", "-u", ROOT.as_posix()],
        capture_output=True,
        check=False,
        text=True,
    )
    if result.returncode != 0:
        print(result.stderr, file=sys.stderr, end="")
        raise RuntimeError("could not resolve the repository path inside WSL")
    return result.stdout.strip()


def _run_wsl(script: str) -> int:
    return subprocess.run(
        ["wsl.exe", "-d", DISTRO, "--", "bash", "-l"],
        cwd=ROOT,
        input=script.encode("utf-8"),
        check=False,
    ).returncode


def main() -> int:
    source = shlex.quote(_wsl_path())
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    revision = shlex.quote(revision)
    sync = f"""set -euo pipefail
src={source}
expected={revision}
dest="$HOME/simciv-ci"
if [ -d "$dest/.git" ]; then
    git -C "$dest" fetch origin
else
    if [ -e "$dest" ]; then
        echo "Refusing to replace non-Git path: $dest" >&2
        exit 2
    fi
    git clone "$src" "$dest"
fi
git -C "$dest" checkout --force "$expected"
git -C "$dest" reset --hard "$expected"
git -C "$dest" clean -fdx -e target/
"""

    results: list[tuple[str, int, float]] = []

    def step(name: str, command: str) -> None:
        started = time.perf_counter()
        code = _run_wsl(command)
        elapsed = time.perf_counter() - started
        results.append((name, code, elapsed))
        status = "PASS" if code == 0 else "FAIL"
        print(f"[{status}] {name}: {elapsed:.1f}s", flush=True)

    step("clone/fetch, checkout, and clean", sync)
    if results[-1][1] == 0:
        environment = (
            'export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"; '
            'cd "$HOME/simciv-ci"; '
        )
        for name, command in (
            ("just check", "just check"),
            ("cargo clippy", "cargo clippy --workspace --all-targets -- -D warnings"),
            ("cargo test", "cargo test --workspace"),
        ):
            step(name, environment + command)
    else:
        for name in ("just check", "cargo clippy", "cargo test"):
            results.append((name, 125, 0.0))
            print(f"[SKIP] {name}: checkout preparation failed", flush=True)

    print("ci-linux summary:")
    for name, code, elapsed in results:
        status = "PASS" if code == 0 else "SKIP" if code == 125 else "FAIL"
        print(f"{status:4} {name}: {elapsed:.1f}s")
    return int(any(code != 0 for _, code, _ in results))


if __name__ == "__main__":
    raise SystemExit(main())
