"""Verify committed checksums for every non-README file under docs/data."""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

from simciv_analysis.git_hooks import dvc_output_paths


ROOT = Path(__file__).resolve().parents[2]
DATA_ROOT = ROOT / "docs" / "data"
MANIFEST = DATA_ROOT / "SHA256SUMS"


def data_files(root: Path) -> dict[str, Path]:
    data_root = root / "docs" / "data"
    return {
        path.relative_to(root).as_posix(): path
        for path in data_root.rglob("*")
        if path.is_file() and path.name.casefold() not in {"readme.md", "sha256sums"}
    }


def parse_manifest(text: str) -> dict[str, str]:
    entries: dict[str, str] = {}
    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line:
            continue
        digest, separator, relative = line.partition("  ")
        if (
            len(digest) != 64
            or any(character not in "0123456789abcdef" for character in digest)
            or not separator
            or not relative.startswith("docs/data/")
            or "\\" in relative
            or "\n" in relative
            or Path(relative).is_absolute()
            or ".." in Path(relative).parts
            or relative.casefold().endswith("/readme.md")
            or relative.casefold() == "docs/data/sha256sums"
        ):
            raise ValueError(f"invalid SHA256SUMS entry on line {line_number}: {line}")
        if relative in entries:
            raise ValueError(f"duplicate SHA256SUMS path on line {line_number}: {relative}")
        entries[relative] = digest
    return entries


def canonical_git_bytes(root: Path, relative: str, path: Path) -> bytes:
    """Read Git-clean bytes so core.autocrlf does not create false failures."""
    object_id = subprocess.check_output(
        ["git", "hash-object", "-w", f"--path={relative}", "--", str(path)],
        cwd=root,
        text=True,
        stderr=subprocess.DEVNULL,
    ).strip()
    return subprocess.check_output(
        ["git", "cat-file", "blob", object_id], cwd=root
    )


def _is_dvc_output(relative: str, outputs: set[str]) -> bool:
    return any(
        relative == output or relative.startswith(output.rstrip("/") + "/")
        for output in outputs
    )


def _digest(root: Path, relative: str, path: Path, outputs: set[str]) -> str:
    raw = (
        path.read_bytes()
        if _is_dvc_output(relative, outputs)
        else canonical_git_bytes(root, relative, path)
    )
    return hashlib.sha256(raw).hexdigest()


def _committed_manifest(root: Path) -> dict[str, str]:
    try:
        text = subprocess.check_output(
            ["git", "show", "HEAD:docs/data/SHA256SUMS"],
            cwd=root,
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        return {}
    return parse_manifest(text)


def check(root: Path = ROOT) -> list[str]:
    manifest = root / "docs" / "data" / "SHA256SUMS"
    if not manifest.is_file():
        return ["docs/data/SHA256SUMS is missing"]
    try:
        expected = parse_manifest(manifest.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return [str(error)]

    committed = _committed_manifest(root)
    for relative, digest in committed.items():
        if expected.get(relative) != digest:
            return [f"committed checksum entry changed or removed: {relative}"]

    files = data_files(root)
    outputs = dvc_output_paths(root)
    errors = [
        f"unregistered data file: {path}"
        for path in sorted(files.keys() - expected.keys())
    ]
    for relative in sorted(expected.keys() - files.keys()):
        if not _is_dvc_output(relative, outputs):
            errors.append(f"manifest file is missing: {relative}")
    for relative, path in sorted(files.items()):
        if relative not in expected:
            continue
        actual = _digest(root, relative, path, outputs)
        if actual != expected[relative]:
            errors.append(
                f"checksum mismatch: {relative}: expected {expected[relative]}, got {actual}"
            )
    return errors


def write(root: Path = ROOT) -> None:
    manifest = root / "docs" / "data" / "SHA256SUMS"
    files = data_files(root)
    outputs = dvc_output_paths(root)
    entries: dict[str, str] = {}
    if manifest.exists():
        old_entries = parse_manifest(manifest.read_text(encoding="utf-8"))
        for relative, digest in old_entries.items():
            if relative not in files and _is_dvc_output(relative, outputs):
                entries[relative] = digest
            elif relative not in files:
                raise FileNotFoundError(f"refusing to drop missing non-DVC data file: {relative}")
            else:
                actual = _digest(root, relative, files[relative], outputs)
                if actual != digest:
                    raise ValueError(f"refusing to change existing data checksum: {relative}")
                entries[relative] = digest
    for relative, path in sorted(files.items()):
        if relative not in entries:
            entries[relative] = _digest(root, relative, path, outputs)
    manifest.write_text(
        "".join(f"{digest}  {relative}\n" for relative, digest in sorted(entries.items())),
        encoding="utf-8",
        newline="\n",
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify data files against SHA256SUMS")
    mode.add_argument("--write", action="store_true", help="update SHA256SUMS after producing data")
    args = parser.parse_args(argv)
    if args.write:
        write()
        print("Updated docs/data/SHA256SUMS.")
        return 0
    errors = check()
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("All present docs/data files match docs/data/SHA256SUMS.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
