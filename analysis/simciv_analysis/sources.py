"""Resolve claim provenance from the reviewed source map."""

from __future__ import annotations

import re
import subprocess
import tomllib
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[2]
SOURCE_MAP = Path("docs") / "reproducibility" / "sources.toml"
HARNESS_CITATION = re.compile(r"crates/sim-model/(?:benches|examples)/[A-Za-z0-9_./-]+")
HARNESS_LINK = re.compile(r"\]\(([^)]+)\)")


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
        for source_report in entry.get("derived_from", []):
            if source_report not in reports:
                raise ValueError(f"reports.{report}.derived_from references unknown report {source_report}")
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
    tests = entry.get("tests", [])
    if not isinstance(tests, list) or any(not isinstance(command, str) for command in tests):
        raise ValueError(f"{label}.tests must be an array of test commands")
    derived_from = entry.get("derived_from", [])
    if not isinstance(derived_from, list) or any(not isinstance(report, str) for report in derived_from):
        raise ValueError(f"{label}.derived_from must be an array of report names")
    if not isinstance(entry.get("review"), bool):
        raise ValueError(f"{label}.review must be a boolean")
    if not isinstance(entry.get("status"), str) or not isinstance(entry.get("regeneration_stage"), str):
        raise ValueError(f"{label} must declare status and regeneration_stage")


def source_for_claim(
    sources: dict[str, Any], report: str, section: str, claim_class: str, _seen: frozenset[str] = frozenset()
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
        resolved = report_entry
    else:
        specificity = lambda entry: 2 * (entry["section"] != "*") + (entry.get("class", "*") != "*")
        matches.sort(key=specificity, reverse=True)
        if len(matches) > 1 and specificity(matches[0]) == specificity(matches[1]):
            raise ValueError(f"ambiguous section source mapping for {report}#{section} ({claim_class})")
        resolved = {**report_entry, **matches[0]}

    if resolved.get("status") != "derived":
        return resolved
    if report in _seen:
        raise ValueError(f"cyclic derived_from source mapping at {report}")
    sources_from = resolved.get("derived_from", [])
    inherited = [
        source_for_claim(sources, source_report, "*", claim_class, _seen | {report})
        for source_report in sources_from
    ]
    inherited = [entry for entry in inherited if entry is not None]
    if not inherited:
        return {**resolved, "status": "missing-source", "review": True}

    status_order = {
        "reproducible-now": 0,
        "bit-exact": 0,
        "bit-exact after documented normalization": 0,
        "conclusion holds": 0,
        "analysis-only": 1,
        "historical-log": 2,
        "historical-protocol": 2,
        "missing-data": 3,
        "missing-source": 4,
        "unrecoverable": 5,
        "unmapped": 6,
    }
    statuses = [str(entry.get("status", "missing-source")) for entry in inherited]
    inherited_status = max(statuses, key=lambda status: status_order.get(status, 6))
    inherited_paths = {
        field: sorted({value for entry in inherited for value in entry.get(field, [])})
        for field in ("harness", "data", "tests")
    }
    note = f"derived from {', '.join(sources_from)}; inherited status {inherited_status}"
    return {
        **resolved,
        **inherited_paths,
        "status": inherited_status,
        "review": any(bool(entry.get("review")) for entry in inherited),
        "notes": "; ".join(filter(None, [str(resolved.get("notes", "")), note])),
    }


def validate_source_paths(sources: dict[str, Any], root: Path = ROOT) -> None:
    """Fail closed when a committed mapping points at a missing artifact."""
    entries = list(sources.get("reports", {}).values()) + list(sources.get("sections", []))
    for entry in entries:
        for key in ("harness", "data"):
            for relative in entry.get(key, []):
                if not (root / relative).is_file():
                    raise ValueError(f"source map {key} path does not exist: {relative}")
        for command in entry.get("tests", []):
            match = re.search(r"--test\s+([A-Za-z0-9_]+)", command)
            if match and not (root / "crates" / "sim-model" / "tests" / f"{match.group(1)}.rs").is_file():
                raise ValueError(f"source map test target does not exist: {command}")
    validate_cited_harnesses(sources, root)


def validate_cited_harnesses(sources: dict[str, Any], root: Path = ROOT) -> None:
    """Require each committed bench/example cited by a report to appear in its mapping."""
    git = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"], capture_output=True, check=False
    )
    tracked = (
        {path.replace("\\", "/") for path in git.stdout.decode("utf-8").split("\0") if path}
        if git.returncode == 0
        else None
    )
    source_entries: dict[str, set[str]] = {}
    for report, entry in sources.get("reports", {}).items():
        source_entries.setdefault(report, set()).update(entry.get("harness", []))
    for entry in sources.get("sections", []):
        source_entries.setdefault(entry["report"], set()).update(entry.get("harness", []))

    for report_path in sorted((root / "docs" / "reports").glob("*.md")):
        if report_path.name.lower() == "renames.md":
            continue
        text = report_path.read_text(encoding="utf-8")
        cited = set(HARNESS_CITATION.findall(text))
        for link in HARNESS_LINK.findall(text):
            target = unquote(urlsplit(link).path)
            resolved = (report_path.parent / target).resolve()
            try:
                relative = resolved.relative_to(root.resolve()).as_posix()
            except ValueError:
                continue
            if relative.startswith(("crates/sim-model/benches/", "crates/sim-model/examples/")):
                cited.add(relative)
        if tracked is not None:
            cited = {path for path in cited if path in tracked}
        missing = sorted(cited - source_entries.get(report_path.name, set()))
        if missing:
            raise ValueError(
                f"{report_path.relative_to(root)} cites committed harnesses absent from sources.toml: {', '.join(missing)}"
            )
