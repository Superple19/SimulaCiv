"""Content-hash provenance for M5 measurement summaries."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import re
import subprocess
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]


def _relative_path(path: str | Path, root: Path) -> tuple[str, Path]:
    candidate = Path(path)
    resolved = (candidate if candidate.is_absolute() else root / candidate).resolve(strict=True)
    try:
        relative = resolved.relative_to(root.resolve(strict=True)).as_posix()
    except ValueError as error:
        raise ValueError(f"provenance path escapes repository: {path}") from error
    return relative, resolved


def _relative_file(path: str | Path, root: Path) -> tuple[str, Path]:
    relative, resolved = _relative_path(path, root)
    if not resolved.is_file():
        raise ValueError(f"provenance input is not a file: {path}")
    return relative, resolved


def digest_files(paths: list[str | Path], root: Path = ROOT) -> str:
    """Hash a sorted path-and-content list using the M5-00 A-2 framing."""
    normalized: list[tuple[bytes, str, Path]] = []
    seen: set[str] = set()
    for path in paths:
        relative, resolved = _relative_file(path, root)
        if relative in seen:
            raise ValueError(f"duplicate provenance path: {relative}")
        seen.add(relative)
        normalized.append((relative.encode("utf-8"), relative, resolved))
    digest = hashlib.sha256()
    for encoded_path, _, resolved in sorted(normalized, key=lambda item: item[0]):
        content_hash = hashlib.sha256(resolved.read_bytes()).hexdigest().encode("ascii")
        digest.update(encoded_path)
        digest.update(b"\0")
        digest.update(content_hash)
        digest.update(b"\n")
    return digest.hexdigest()


def source_files(root: Path = ROOT) -> list[Path]:
    paths = list((root / "crates").rglob("*.rs"))
    paths.extend((root / "crates").rglob("Cargo.toml"))
    paths.extend(root / name for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"))
    return sorted(set(paths), key=lambda path: path.relative_to(root).as_posix().encode("utf-8"))


def source_digest(root: Path = ROOT) -> str:
    return digest_files(source_files(root), root)


def _dvc_lock_hashes(lock_path: Path) -> dict[str, str]:
    """Read output path/md5 pairs from DVC's generated lock YAML without a YAML dependency."""
    if not lock_path.exists():
        return {}
    result: dict[str, str] = {}
    current_path: str | None = None
    current_md5: str | None = None
    item_line = re.compile(r"^\s*-\s+")
    path_line = re.compile(r"^\s*(?:-\s+)?path:\s*(.+?)\s*$")
    md5_line = re.compile(r"^\s*(?:-\s+)?md5:\s*([0-9a-f]+(?:\.dir)?)\s*$")

    def save() -> None:
        if current_path is not None and current_md5 is not None:
            result[current_path.replace("\\", "/")] = current_md5

    for line in lock_path.read_text(encoding="utf-8").splitlines():
        if item_line.match(line):
            save()
            current_path = None
            current_md5 = None
        path_match = path_line.match(line)
        if path_match:
            current_path = path_match.group(1).strip("'\"")
        md5_match = md5_line.match(line)
        if md5_match:
            current_md5 = md5_match.group(1)
        save()
    save()
    return result


def finalize_summary(summary_path: str | Path, root: Path = ROOT) -> dict[str, Any]:
    path = Path(summary_path)
    resolved = path if path.is_absolute() else root / path
    summary = json.loads(resolved.read_text(encoding="utf-8"))
    spec_files = [str(item) for item in summary.get("spec_files", [])]
    harness_files = [str(item) for item in summary.get("harness_files", [])]
    raw_outputs = [str(item) for item in summary.get("raw_outputs", [])]
    lock_hashes = _dvc_lock_hashes(root / "dvc.lock")
    output_records = []
    for output in raw_outputs:
        relative, output_path = _relative_path(output, root)
        if output_path.is_dir():
            files = sorted(
                (file for file in output_path.rglob("*") if file.is_file()),
                key=lambda file: file.relative_to(root).as_posix().encode("utf-8"),
            )
            output_hash = digest_files(files, root)
        else:
            output_hash = hashlib.sha256(output_path.read_bytes()).hexdigest()
        dvc_md5 = lock_hashes.get(relative)
        if dvc_md5 is None:
            raise ValueError(f"DVC lock has no md5 for raw output {relative}")
        output_records.append({"path": relative, "dvc_md5": dvc_md5, "sha256": output_hash})
    rustc = subprocess.run(["rustc", "-V"], cwd=root, capture_output=True, text=True, check=True).stdout.strip()
    summary.update(
        {
            "source_digest": source_digest(root),
            "spec_digest": digest_files(spec_files, root),
            "harness_digest": digest_files(harness_files, root),
            "toolchain": rustc,
            "outputs": output_records,
            "host": {
                "system": platform.system(),
                "release": platform.release(),
                "machine": platform.machine(),
                "processor": platform.processor(),
            },
        }
    )
    resolved.write_text(json.dumps(summary, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
    return summary


def verify_summary(summary_path: str | Path, root: Path = ROOT) -> None:
    path = Path(summary_path)
    resolved = path if path.is_absolute() else root / path
    summary = json.loads(resolved.read_text(encoding="utf-8"))
    actual_source = source_digest(root)
    if summary.get("source_digest") != actual_source:
        raise ValueError(
            f"source_digest mismatch for {resolved.relative_to(root)}: "
            f"recorded={summary.get('source_digest')} current={actual_source}"
        )
    for label in ("spec_files", "harness_files"):
        recorded = summary.get(f"{label.removesuffix('_files')}_digest")
        actual = digest_files([str(item) for item in summary.get(label, [])], root)
        if recorded != actual:
            raise ValueError(f"{label.removesuffix('_files')}_digest mismatch for {resolved.relative_to(root)}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary", type=Path, help="M5 run summary JSON to verify")
    parser.add_argument(
        "--finalize",
        action="store_true",
        help="after a completed DVC stage, add content digests and output hashes before verifying",
    )
    args = parser.parse_args(argv)
    if args.finalize:
        finalize_summary(args.summary)
    verify_summary(args.summary)
    print(f"provenance matches current source: {args.summary}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
