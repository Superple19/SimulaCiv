"""Verify committed checksums for every non-README file under docs/data."""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

from simciv_analysis.git_hooks import dvc_output_paths
from simciv_analysis.io import write_text_lf


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


def _git_blob_bytes(root: Path, relative: str) -> bytes | None:
    for revision in (f"HEAD:{relative}", f":{relative}"):
        result = subprocess.run(
            ["git", "show", revision],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if result.returncode == 0:
            return result.stdout
    return None


def _git_index_blob_bytes(root: Path, relative: str) -> bytes | None:
    result = subprocess.run(
        ["git", "show", f":{relative}"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return result.stdout if result.returncode == 0 else None


def _is_dvc_metadata(relative: str) -> bool:
    return relative.startswith("docs/data/") and Path(relative).name in {"dvc.yaml", "dvc.lock"}


def _content_bytes(root: Path, relative: str, path: Path) -> bytes:
    blob = _git_blob_bytes(root, relative)
    worktree = path.read_bytes()
    if (relative == "docs/data/SHA256SUMS" or _is_dvc_metadata(relative)) and blob != worktree:
        staged = _git_index_blob_bytes(root, relative)
        if staged == worktree:
            return staged
    if blob is not None and blob != worktree:
        raise ValueError(f"working-tree bytes differ from Git blob: {relative}")
    return blob if blob is not None else worktree


def _is_dvc_output(relative: str, outputs: set[str]) -> bool:
    return any(
        relative == output or relative.startswith(output.rstrip("/") + "/")
        for output in outputs
    )


def _digest(root: Path, relative: str, path: Path) -> str:
    return hashlib.sha256(_content_bytes(root, relative, path)).hexdigest()


def _tracked_data_paths(root: Path) -> set[str]:
    paths = subprocess.check_output(
        ["git", "ls-files", "-z", "docs/data"], cwd=root
    ).split(b"\0")
    return {
        path.decode("utf-8")
        for path in paths
        if path and Path(path.decode("utf-8")).name.casefold() != "readme.md"
    }


def autocrlf_warning(root: Path = ROOT) -> str | None:
    result = subprocess.run(
        ["git", "config", "--local", "--get", "core.autocrlf"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
        text=True,
    )
    if result.stdout.strip().casefold() == "true":
        return "WARNING: repository-local core.autocrlf=true can convert checkout bytes."
    return None


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
            if relative not in expected or not _is_dvc_metadata(relative):
                return [f"committed checksum entry changed or removed: {relative}"]
            staged = _git_index_blob_bytes(root, relative)
            path = root / relative
            if staged is None or not path.is_file() or staged != path.read_bytes():
                return [f"DVC metadata checksum can change only when staged bytes match: {relative}"]

    files = data_files(root)
    tracked = _tracked_data_paths(root)
    outputs = dvc_output_paths(root)
    errors = [
        f"unregistered data file: {path}"
        for path in sorted(files.keys() - expected.keys())
    ]
    for relative in sorted(tracked):
        path = root / relative
        if not path.is_file():
            errors.append(f"tracked data file is missing: {relative}")
            continue
        try:
            _content_bytes(root, relative, path)
        except ValueError as error:
            errors.append(str(error))
    for relative in sorted(expected.keys() - files.keys()):
        if relative in tracked or not _is_dvc_output(relative, outputs):
            errors.append(f"manifest file is missing: {relative}")
    for relative, path in sorted(files.items()):
        if relative not in expected:
            continue
        try:
            actual = _digest(root, relative, path)
        except ValueError:
            continue
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
                actual = _digest(root, relative, files[relative])
                if actual != digest:
                    if not _is_dvc_metadata(relative):
                        raise ValueError(f"refusing to change existing data checksum: {relative}")
                    staged = _git_index_blob_bytes(root, relative)
                    if staged is None or staged != files[relative].read_bytes():
                        raise ValueError(f"DVC metadata update must be staged byte-exactly: {relative}")
                    digest = hashlib.sha256(staged).hexdigest()
                entries[relative] = digest
    for relative, path in sorted(files.items()):
        if relative not in entries:
            entries[relative] = _digest(root, relative, path)
    write_text_lf(
        manifest,
        "".join(f"{digest}  {relative}\n" for relative, digest in sorted(entries.items())),
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify data files against SHA256SUMS")
    mode.add_argument("--write", action="store_true", help="update SHA256SUMS after producing data")
    mode.add_argument("--check-autocrlf", action="store_true", help="warn if core.autocrlf is true")
    args = parser.parse_args(argv)
    if args.check_autocrlf:
        warning = autocrlf_warning()
        if warning:
            print(warning, file=sys.stderr)
        else:
            result = subprocess.run(
                ["git", "config", "--local", "--get", "core.autocrlf"],
                cwd=ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                check=False,
                text=True,
            )
            print(f"repository-local core.autocrlf={result.stdout.strip() or 'unset'}")
        return 0
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
