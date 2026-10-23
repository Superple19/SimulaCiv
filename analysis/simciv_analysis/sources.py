"""Resolve claim provenance from the reviewed source map."""

from __future__ import annotations

import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SOURCE_MAP = Path("docs") / "reproducibility" / "sources.toml"


def load_sources(root: Path = ROOT) -> dict[str, Any]:
    path = root / SOURCE_MAP
    if not path.is_file():
        return {"version": 1, "reports": {}, "sections": []}
    document = tomllib.loads(path.read_text(encoding="utf-8"))
    if document.get("version") != 1:
        raise ValueError(f"unsupported source map version in {path}")
    reports = document.get("reports", {})
    for report, entry in reports.items():
        _validate_entry(entry, f"reports.{report}")
    sections = document.get("sections", [])
    seen: set[tuple[str, str, str]] = set()
    for entry in sections:
        _validate_entry(entry, "sections[]")
        key = (entry["report"], entry["section"], entry.get("class", "*"))
        if key in seen:
            raise ValueError(f"duplicate source-map section key {key}")
        seen.add(key)
    return document


def _validate_entry(entry: dict[str, Any], label: str) -> None:
    for key in ("harness", "data"):
        value = entry.get(key, [])
        if not isinstance(value, list) or any(not isinstance(path, str) for path in value):
            raise ValueError(f"{label}.{key} must be an array of paths")
    if not isinstance(entry.get("review"), bool):
        raise ValueError(f"{label}.review must be a boolean")
    if not isinstance(entry.get("status"), str) or not isinstance(entry.get("regeneration_stage"), str):
        raise ValueError(f"{label} must declare status and regeneration_stage")


def source_for_claim(
    sources: dict[str, Any], report: str, section: str, claim_class: str
) -> dict[str, Any] | None:
    """Return the most-specific section mapping, then its report default."""
    report_entry = sources.get("reports", {}).get(report)
    if report_entry is None:
        return None
    matches = [
        entry
        for entry in sources.get("sections", [])
        if entry["report"] == report
        and entry["section"] in {section, "*"}
        and entry.get("class", "*") in {claim_class, "*"}
    ]
    if not matches:
        return report_entry
    specificity = lambda entry: 2 * (entry["section"] != "*") + (entry.get("class", "*") != "*")
    matches.sort(key=specificity, reverse=True)
    if len(matches) > 1 and specificity(matches[0]) == specificity(matches[1]):
        raise ValueError(f"ambiguous section source mapping for {report}#{section} ({claim_class})")
    return {**report_entry, **matches[0]}


def validate_source_paths(sources: dict[str, Any], root: Path = ROOT) -> None:
    """Fail closed when a committed mapping points at a missing artifact."""
    entries = list(sources.get("reports", {}).values()) + list(sources.get("sections", []))
    for entry in entries:
        for key in ("harness", "data"):
            for relative in entry.get(key, []):
                if not (root / relative).is_file():
                    raise ValueError(f"source map {key} path does not exist: {relative}")
