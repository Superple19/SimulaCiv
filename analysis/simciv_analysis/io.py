"""Checked data readers and UTF-8/LF text output for the analysis tools."""

from __future__ import annotations

import gzip
import json
import re
import tomllib
import zlib
from pathlib import Path
from typing import Any

import pandas as pd


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
_FROZEN_DOCUMENT_STATUS = re.compile(r"(?m)^\s*(?:\*\*)?Status:\s*FROZEN\b")


def _decision_authority(text: str) -> tuple[tuple[str, str, tuple[tuple[str, str], ...]], ...]:
    document = tomllib.loads(text)
    records = [*document.get("decision", []), *document.get("stage", [])]
    result = []
    for row in records:
        approval = tuple(
            sorted((key, repr(value)) for key, value in row.items() if "approv" in key.lower())
        )
        result.append((str(row.get("id", "")), str(row.get("status", "")), approval))
    return tuple(sorted(result))


def _guard_generated_write(destination: Path, text: str) -> None:
    if not destination.exists():
        return
    try:
        relative = destination.resolve().relative_to(REPOSITORY_ROOT.resolve()).as_posix()
    except ValueError:
        return
    current = destination.read_text(encoding="utf-8")
    if relative == "contracts/decisions.toml":
        if _decision_authority(current) != _decision_authority(text):
            raise PermissionError("generators may not change decision or stage status/approval fields")
    if _FROZEN_DOCUMENT_STATUS.search(current):
        raise PermissionError(f"generators may not write frozen document: {relative}")


def content_matches(path: str | Path, expected: bytes) -> bool:
    """Compare generated content, ignoring gzip container metadata/encoding."""
    candidate = Path(path)
    actual = candidate.read_bytes()
    if candidate.suffix != ".gz":
        return actual == expected
    try:
        return gzip.decompress(actual) == gzip.decompress(expected)
    except (OSError, EOFError, zlib.error):
        return False


def write_text_lf(path: str | Path, text: str, *, append: bool = False, flush: bool = False) -> None:
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    normalized = text.replace("\r\n", "\n").replace("\r", "\n")
    _guard_generated_write(destination, normalized)
    with destination.open("a" if append else "w", encoding="utf-8", newline="\n") as stream:
        stream.write(normalized)
        if flush:
            stream.flush()


def _checked_path(path: str | Path, root: str | Path) -> Path:
    base = Path(root).resolve(strict=True)
    supplied = Path(path)
    candidate = supplied if supplied.is_absolute() else base / supplied
    resolved = candidate.resolve(strict=True)
    if not resolved.is_relative_to(base):
        raise ValueError(f"input path escapes allowed root: {path}")
    if not resolved.is_file():
        raise ValueError(f"input is not a file: {path}")
    return resolved


def _read_text(path: Path) -> str:
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8", newline="") as stream:
        return stream.read()


def load_data(path: str | Path, *, root: str | Path = REPOSITORY_ROOT) -> Any:
    """Read CSV, JSON, JSONL, or gzip-compressed variants under ``root``."""
    checked = _checked_path(path, root)
    suffixes = checked.suffixes
    suffix = suffixes[-2] if suffixes[-1:] == [".gz"] and len(suffixes) > 1 else checked.suffix
    if suffix == ".csv":
        return pd.read_csv(checked, compression="gzip" if checked.suffix == ".gz" else None)
    text = _read_text(checked)
    if suffix == ".json":
        return json.loads(text)
    if suffix == ".jsonl":
        return [json.loads(line) for line in text.splitlines() if line.strip()]
    raise ValueError(f"unsupported input format: {checked.name}")
