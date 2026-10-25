"""Verify committed checksums for every non-README file under docs/data."""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

from simciv_analysis.dvc_freeze_guard import frozen_output_hash_violations, lock_stage_outputs, parse_stages
from simciv_analysis.git_hooks import dvc_descriptor_output_paths, dvc_output_paths
from simciv_analysis.io import write_text_lf


ROOT = Path(__file__).resolve().parents[2]
DATA_ROOT = ROOT / "docs" / "data"
MANIFEST = DATA_ROOT / "SHA256SUMS"
GENERATED_DATA_STATUS = "docs/data/m5_05/STATUS.md"


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


def _can_refresh_data_checksum(relative: str) -> bool:
    return _is_dvc_metadata(relative) or relative == GENERATED_DATA_STATUS


def _content_bytes(root: Path, relative: str, path: Path) -> bytes:
    blob = _git_blob_bytes(root, relative)
    worktree = path.read_bytes()
    if (relative == "docs/data/SHA256SUMS" or _can_refresh_data_checksum(relative)) and blob != worktree:
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


def _dvc_lock_manifest_violations(
    root: Path, manifest: dict[str, str], outputs: set[str]
) -> list[str]:
    errors = []
    for lock_path in sorted((root / "docs" / "data").rglob("dvc.lock")):
        lock_relative = lock_path.relative_to(root).as_posix()
        if lock_relative not in manifest:
            errors.append(f"DVC lock is missing from SHA256SUMS: {lock_relative}")
        lock_text = lock_path.read_text(encoding="utf-8")
        pipeline_path = lock_path.with_name("dvc.yaml")
        pipeline_outputs = dvc_descriptor_output_paths(pipeline_path, root)
        lock_outputs = lock_stage_outputs(lock_text)
        for output, dvc_hash in {
            path: digest
            for stage_outputs in lock_outputs.values()
            for path, digest in stage_outputs.items()
        }.items():
            if not dvc_hash:
                errors.append(f"DVC lock output is missing its content hash: {lock_relative}: {output}")
                continue
            if not _is_dvc_output(output, outputs):
                errors.append(f"DVC lock output is not declared by a DVC pipeline: {lock_relative}: {output}")
                continue
            if output not in pipeline_outputs:
                errors.append(f"DVC lock output is not declared by this pipeline: {lock_relative}: {output}")
                continue
            path = root / output
            listed = [name for name in manifest if name == output or name.startswith(output.rstrip("/") + "/")]
            is_directory = dvc_hash.endswith(".dir") or path.is_dir()
            if is_directory and not listed:
                errors.append(f"DVC lock directory output has no SHA256SUMS entries: {lock_relative}: {output}")
            elif not is_directory and output not in manifest:
                errors.append(f"DVC lock file output is missing from SHA256SUMS: {lock_relative}: {output}")
            if path.is_file() and output not in manifest:
                errors.append(f"materialized DVC lock output is missing from SHA256SUMS: {output}")
            elif path.is_dir():
                for materialized in path.rglob("*"):
                    if not materialized.is_file() or materialized.name.casefold() == "readme.md":
                        continue
                    relative = materialized.relative_to(root).as_posix()
                    if relative not in manifest:
                        errors.append(f"materialized DVC lock output is missing from SHA256SUMS: {relative}")
        declared_stages = parse_stages(pipeline_path.read_text(encoding="utf-8"))
        for stage_name in lock_outputs.keys() - declared_stages.keys():
            errors.append(f"DVC lock stage is not declared by its pipeline: {lock_relative}:{stage_name}")
    return errors


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


def _blob_at(root: Path, revision: str, relative: str) -> bytes | None:
    result = subprocess.run(
        ["git", "show", f"{revision}:{relative}"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return result.stdout if result.returncode == 0 else None


def _manifest_at(root: Path, revision: str) -> dict[str, str]:
    content = _blob_at(root, revision, "docs/data/SHA256SUMS")
    return {} if content is None else parse_manifest(content.decode("utf-8"))


def _frozen_lock_range_violations(root: Path, base: str, head: str) -> list[str]:
    commits = subprocess.run(
        ["git", "rev-list", "--reverse", f"{base}..{head}"],
        cwd=root, capture_output=True, text=True, check=False,
    )
    if commits.returncode:
        return [commits.stderr.strip() or f"could not inspect commits in {base}..{head}"]
    errors = []
    for commit in commits.stdout.splitlines():
        parent_line = subprocess.check_output(
            ["git", "rev-list", "--parents", "-n", "1", commit], cwd=root, text=True
        ).split()
        parent = parent_line[1] if len(parent_line) > 1 else base
        changed = subprocess.run(
            ["git", "diff", "--name-only", "-z", parent, commit, "--", "docs/data"],
            cwd=root, capture_output=True, check=False,
        )
        if changed.returncode:
            errors.append(f"could not inspect DVC metadata in {parent}..{commit}")
            continue
        for raw_path in changed.stdout.split(b"\0"):
            if not raw_path or Path(raw_path.decode("utf-8")).name != "dvc.lock":
                continue
            lock_path = Path(raw_path.decode("utf-8")).as_posix()
            pipeline_path = str(Path(lock_path).with_name("dvc.yaml")).replace("\\", "/")
            old_lock = _blob_at(root, parent, lock_path)
            new_lock = _blob_at(root, commit, lock_path)
            old_pipeline = _blob_at(root, parent, pipeline_path)
            new_pipeline = _blob_at(root, commit, pipeline_path)
            if None in (old_lock, new_lock, old_pipeline, new_pipeline):
                continue
            errors.extend(
                frozen_output_hash_violations(
                    new_pipeline.decode("utf-8"),
                    old_lock.decode("utf-8"),
                    new_lock.decode("utf-8"),
                    lock_path,
                    previous_pipeline_text=old_pipeline.decode("utf-8"),
                )
            )
    return errors


def base_range_violations(root: Path, base: str, head: str = "HEAD") -> list[str]:
    """Reject changes to existing data blobs or their checksums in a pushed range.

    DVC pipeline YAML/lock files and the generated M5-05 cache-status summary are
    control metadata; raw data and their manifest entries remain protected. A
    new DVC output path may be registered normally.
    """
    try:
        base_manifest = _manifest_at(root, base)
        head_manifest = _manifest_at(root, head)
    except (subprocess.SubprocessError, UnicodeDecodeError, ValueError) as error:
        return [f"could not read data manifests for {base}..{head}: {error}"]
    outputs = dvc_output_paths(root)
    errors = []
    errors.extend(_frozen_lock_range_violations(root, base, head))
    for relative, old_digest in sorted(base_manifest.items()):
        new_digest = head_manifest.get(relative)
        if new_digest is None:
            if not _can_refresh_data_checksum(relative):
                errors.append(f"existing data checksum removed in {base}..{head}: {relative}")
        elif new_digest != old_digest and not _can_refresh_data_checksum(relative):
            errors.append(f"existing data checksum changed in {base}..{head}: {relative}")

    for relative, digest in sorted(head_manifest.items()):
        if relative in base_manifest or _can_refresh_data_checksum(relative):
            continue
        blob = _blob_at(root, head, relative)
        if blob is None:
            if not _is_dvc_output(relative, outputs):
                errors.append(f"new checksum has no tracked file or DVC output in {head}: {relative}")
        elif hashlib.sha256(blob).hexdigest() != digest:
            errors.append(f"new data checksum does not match {head} blob: {relative}")

    changed = subprocess.run(
        ["git", "diff", "--name-only", "-z", f"{base}..{head}", "--", "docs/data"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if changed.returncode:
        errors.append(changed.stderr.decode("utf-8", errors="replace").strip() or "could not inspect changed data paths")
        return errors
    paths = {Path(path.decode("utf-8")).as_posix() for path in changed.stdout.split(b"\0") if path}
    for relative in sorted(paths):
        if relative.casefold().endswith("/readme.md") or relative.casefold() == "docs/data/readme.md":
            continue
        if relative == "docs/data/SHA256SUMS" or _can_refresh_data_checksum(relative):
            continue
        before = _blob_at(root, base, relative)
        after = _blob_at(root, head, relative)
        if before is not None and before != after:
            errors.append(f"existing data bytes changed in {base}..{head}: {relative}")
        elif before is None and after is not None:
            if head_manifest.get(relative) != hashlib.sha256(after).hexdigest():
                errors.append(f"new data file must be registered in docs/data/SHA256SUMS: {relative}")
    return errors


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
            if relative not in expected or not _can_refresh_data_checksum(relative):
                return [f"committed checksum entry changed or removed: {relative}"]
            staged = _git_index_blob_bytes(root, relative)
            path = root / relative
            if staged is None or not path.is_file() or staged != path.read_bytes():
                return [f"generated data checksum can change only when staged bytes match: {relative}"]

    files = data_files(root)
    tracked = _tracked_data_paths(root)
    outputs = dvc_output_paths(root)
    errors = _dvc_lock_manifest_violations(root, expected, outputs) + [
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
                    if not _can_refresh_data_checksum(relative):
                        raise ValueError(f"refusing to change existing data checksum: {relative}")
                    staged = _git_index_blob_bytes(root, relative)
                    if staged is None or staged != files[relative].read_bytes():
                        raise ValueError(f"generated data update must be staged byte-exactly: {relative}")
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
    mode.add_argument("--check-base-range", nargs=2, metavar=("BASE", "HEAD"), help="protect existing data in a Git range")
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
    if args.check_base_range:
        errors = base_range_violations(ROOT, *args.check_base_range)
        if errors:
            print("\n".join(errors), file=sys.stderr)
            return 1
        print(f"existing raw data bytes and checksums are unchanged in {args.check_base_range[0]}..{args.check_base_range[1]}")
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
