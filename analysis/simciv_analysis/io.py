"""Checked readers for repository-local tabular and JSON data."""

from __future__ import annotations

import gzip
import json
from pathlib import Path
from typing import Any

import pandas as pd


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


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
