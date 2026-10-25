"""Extract tidy numeric claims from reports and measured contract sections."""

from __future__ import annotations

import argparse
import csv
import gzip
import io
import json
import re
import sys
from collections.abc import Iterator
from pathlib import Path

from .io import content_matches
from .recovered_claims import apply_recorded_statuses
from .sources import load_sources, source_for_claim, validate_source_paths


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "docs" / "reproducibility" / "claims.csv.gz"
OLD_LEDGER = ROOT / "docs" / "reproducibility" / "superseded" / "claims.csv"
RENAMES = ROOT / "docs" / "reports" / "RENAMES.md"
M5_04_OVERRIDES = Path("docs") / "data" / "m5_04" / "claim_statuses.json"
M5_05_RESULTS = Path("docs") / "reproducibility" / "M5_05_M0_M3_REGENERATION_RESULTS.json"
M5_05_LEGACY_RESULTS = Path("docs") / "data" / "m5_05" / "results.json"
M3_05_PILOT_RESULTS = Path("docs") / "reproducibility" / "M3_05_PILOT_RESULTS.json"
FIELDS = [
    "claim_id",
    "claim_key",
    "report",
    "section anchor",
    "claim text (short)",
    "context",
    "value",
    "unit",
    "class",
    "tier",
    "source harness",
    "raw data",
    "status",
    "M5 stage",
    "notes",
]

NUMBER = re.compile(
    r"(?<![\w.])(?P<number>[+-]?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?|[+-]?\.\d+)"
    r"(?P<suffix>%|×|x|µs|μs|us|ms|ns|seconds?|secs?|s|days?|agents?|ticks?|k|M)?",
    re.IGNORECASE,
)
HASH = re.compile(r"(?<![0-9a-fA-F])[0-9a-fA-F]{64}(?![0-9a-fA-F])")
DATE = re.compile(r"\b\d{4}-\d{1,2}-\d{1,2}\b")
VERSION = re.compile(r"\bv\d+\.\d+(?:\.\d+)?(?:-[A-Za-z0-9.-]+)?\b", re.IGNORECASE)
MILESTONE = re.compile(r"\bM\d{1,2}(?:-\d{1,2}[A-Za-z]?)?\b", re.IGNORECASE)
GATE_ID = re.compile(r"\bA-\d{1,2}\b", re.IGNORECASE)
HEADING = re.compile(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$")
# Configuration/input fields are filtered at the column or label boundary; measured outcomes remain claims.
INPUT_ASSIGNMENT = re.compile(
    r"(?:\bN\b|\bK\b|\bk\b|seed|replicate(?:_id)?|master_seed|p_local|"
    r"relationship_degree_k|max_hops|chunk(?:_size)?|threads?|horizon|"
    r"days?|interval|migration_probability|migration_threshold|migration_cost|"
    r"birth_probability|birth_food_threshold|starvation_threshold|"
    r"initial_settlement_resource|carrying_capacity|regrowth_rate|"
    r"base_metabolic_cost|health_decay_rate|base_work_yield|"
    r"action_biases|base_weight_matrix)\s*(?:=|:|\bin\b\s*\{?)\s*$",
    re.IGNORECASE,
)
INPUT_HEADERS = {
    "n",
    "k",
    "seed",
    "master seed",
    "master_seed",
    "replicate",
    "replicate_id",
    "p_local",
    "relationship_degree_k",
    "neighbors",
    "max_hops",
    "horizon",
    "day",
    "days",
    "threads",
    "chunk",
    "chunk size",
    "workload",
    "configuration",
    "config",
    "input",
    "rank",
    "claim id",
}
CONTEXT_DIMENSION_HEADERS = {
    "n range", "population range", "interval", "checkpoint", "phase", "stage", "profile",
    "runner", "path", "workload", "scenario", "mode", "action", "family", "layout", "case",
    "system", "schedule", "target", "pool", "thread count", "threads", "chunk", "chunk size",
    "horizon", "max hops", "max_hops", "transmit", "transmit probability", "transmit_probability",
    "frontier sizes by level", "levels/max", "locality", "grouping", "authority", "candidate",
}
PARAMETER_HEADERS = {
    "checkpoint", "interval", "n range", "population range", "transmit", "transmit probability",
    "transmit_probability", "max hops", "max_hops", "sample count", "samples", "warmups",
    "repetitions", "thread count", "pool size", "pool sizes", "phase", "phase id", "candidate",
}
DESCRIPTIVE_HEADERS = {"test target / gate", "test target/gate", "claim text", "report"}
OLD_META_FIELDS = [
    "report",
    "section anchor",
    "claim text (short)",
    "value",
    "unit",
    "class",
    "source harness",
    "raw data",
    "status",
    "M5 stage",
    "notes",
]
def _slug(value: str) -> str:
    value = re.sub(r"[`*_]", "", value).lower()
    value = re.sub(r"[^a-z0-9]+", "-", value).strip("-")
    return value or "document"


def _split_cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def _is_separator(cells: list[str]) -> bool:
    return bool(cells) and all(re.fullmatch(r"\s*:?-{3,}:?\s*", cell) for cell in cells)


def _input_header(header: str) -> bool:
    normalized = re.sub(r"[`*_]", "", header).strip().lower()
    return normalized in INPUT_HEADERS


def _context_dimension_header(header: str) -> bool:
    normalized = re.sub(r"[`*]", "", header).strip().lower()
    if normalized.startswith("phase ("):
        return True
    normalized = re.sub(r"[_-]+", " ", normalized)
    return normalized in {_normalize_header(value) for value in INPUT_HEADERS | CONTEXT_DIMENSION_HEADERS}


def _parameter_header(header: str) -> bool:
    normalized = _normalize_header(header)
    return normalized in {_normalize_header(value) for value in INPUT_HEADERS | PARAMETER_HEADERS}


def _normalize_header(header: str) -> str:
    return re.sub(r"[_-]+", " ", re.sub(r"[`*]", "", header).strip().lower())


def _context_key(header: str) -> str:
    raw = re.sub(r"[`*]", "", header).strip()
    if raw.casefold().startswith("phase ("):
        return "phase"
    if raw == "K":
        return "settlement_count"
    if raw == "k":
        return "relationship_degree_k"
    if raw.casefold() == "n":
        return "population_n"
    return re.sub(r"[\s-]+", "_", _normalize_header(raw))


def _profile_column_dimensions(header: str) -> dict[str, str]:
    match = re.fullmatch(r"\s*([0-9][0-9,]*(?:\.[0-9]+)?\s*[kKmM]?)\s*/\s*([0-9.]+)\s*", header)
    if not match:
        return {}
    return {"population_n": match.group(1).replace(" ", ""), "p_local": match.group(2)}


def _input_context(text: str, start: int) -> bool:
    before = text[max(0, start - 100) : start]
    if INPUT_ASSIGNMENT.search(before):
        return True
    # Standalone configuration fields in prose are inputs, not measured claims.
    return bool(re.search(r"\b(?:N|K|k)\s*(?:∈|in)\s*\{[^}]*$", before, re.IGNORECASE))


def _unit_after(text: str, end: int) -> str:
    tail = text[end : end + 24]
    match = re.match(r"\s*(µs|μs|us|ms|ns|seconds?|secs?|days?|agents?|ticks?|%)\b", tail, re.IGNORECASE)
    return match.group(1) if match else ""


def _header_unit(header: str) -> str:
    match = re.search(r"[([]\s*(µs|μs|us|ms|ns|seconds?|secs?|days?|agents?|ticks?|%)\s*[)\]]", header, re.IGNORECASE)
    return match.group(1) if match else ""


def _tokens(text: str) -> Iterator[tuple[str, str]]:
    for value, unit, _ in _tokens_with_context(text):
        yield value, unit


def _tokens_with_context(text: str) -> Iterator[tuple[str, str, str]]:
    masked = DATE.sub(" ", text)
    masked = VERSION.sub(" ", masked)
    masked = MILESTONE.sub(" ", masked)
    masked = GATE_ID.sub(" ", masked)
    spans: list[tuple[int, int]] = []
    for match in HASH.finditer(masked):
        spans.append(match.span())
        yield match.group(0).lower(), "sha256", ""
    for match in NUMBER.finditer(masked):
        if any(match.start() < end and match.end() > start for start, end in spans):
            continue
        if _input_context(masked, match.start()):
            continue
        value = match.group("number").replace(",", "")
        unit = match.group("suffix") or _unit_after(masked, match.end())
        preceding = masked[: match.start()]
        local = re.split(r"[,;|]", preceding)[-1]
        local = re.sub(r"\b[+-]?(?:\d+(?:\.\d*)?|\.\d+)\b", " ", local)
        local = re.sub(r"[\s\[\]()*`]+$", "", local).strip()
        if "±" in local:
            local = "MAD"
        elif "±" in text[match.end() :]:
            local = "median"
        yield value, unit, local[-80:]


def _tier(section: str, line: str, *, header: str = "", table_row: bool = False, contract: bool = False) -> str:
    context = f"{section} {header} {line}".lower()
    section_name = section.lower()
    if table_row and "gate" in context:
        return "key" if re.search(r"\b(?:pass|fail|inconclusive)\b", context) else "supporting"
    if not contract and re.match(r"^\s*(?:[-*+]\s*)?(?:\*\*)?status(?:\*\*)?\s*:", line, re.IGNORECASE):
        return "headline"
    if not contract and any(name in section_name for name in ("summary", "findings", "status")):
        return "headline"
    table_context = f"{section} {header}".lower()
    if not contract and table_row and any(term in table_context for term in ("audit", "graduation", "milestone questions", "stage completion")):
        return "headline"
    if "gate verdict" in context:
        return "key"
    if re.match(r"^\s*(?:[-*+]\s*)?(?:\*\*)?(?:validation\s+)?status(?:\*\*)?\s*:", line, re.IGNORECASE) and re.search(
        r"\b(?:pass(?:ed)?|fail(?:ed)?|inconclusive|complete|blocked)\b", context, re.IGNORECASE
    ):
        return "key"
    if re.search(r"\b(?:pass(?:ed)?|fail(?:ed)?|inconclusive)\b", context, re.IGNORECASE):
        return "key"
    return "supporting"


def _class(value: str, unit: str, context: str) -> str:
    lowered = context.lower()
    if unit.lower() == "sha256" or any(term in lowered for term in ("digest", "hash", "snapshot bytes", "bit-exact", "replay")):
        return "deterministic"
    if unit.lower() in {"µs", "μs", "us", "ms", "ns", "s", "sec", "seconds"} or any(
        term in lowered for term in ("latency", "timing", "speedup", "benchmark", "median", "mad")
    ):
        return "timing"
    return "derived"


def _eligible_text(line: str, section: str, *, contract: bool) -> bool:
    if line.lstrip().startswith("|"):
        return False
    if re.search(r"\b(?:validation\s+)?status(?:\*\*)?\s*:", line, re.IGNORECASE):
        return True
    if line.startswith("#"):
        return False
    if not contract and any(term in section.lower() for term in ("summary", "findings", "status")):
        return bool(line.strip())
    if re.match(r"^\s*[-*+]\s+", line) and "finding" in section.lower():
        return True
    if contract and any(term in section.lower() for term in ("empirical", "measured", "validation", "benchmark", "performance", "current reproducibility state")):
        return bool(line.strip())
    return False


def _line_claims(
    *,
    path: Path,
    line_number: int,
    line: str,
    section: str,
    headers: list[str] | None,
    context_headers: list[str] | None,
    contract: bool,
) -> list[dict[str, object]]:
    measured_contract_section = any(
        term in section.lower()
        for term in ("empirical", "measured", "validation", "benchmark", "performance", "current reproducibility state")
    )
    contract_status = bool(re.search(r"\b(?:validation\s+)?status(?:\*\*)?\s*:", line, re.IGNORECASE))
    if contract and not (measured_contract_section or contract_status):
        return []
    context = f"{section} {line}"
    base = {
        "report": path.name,
        "line": line_number,
        "section anchor": _slug(section),
        "claim text (short)": line.strip()[:240],
        "context": "",
    }
    output: list[dict[str, object]] = []
    if line.lstrip().startswith("|") and headers is not None and not _is_separator(_split_cells(line)):
        cells = _split_cells(line)
        context_headers = context_headers or headers
        for index, cell in enumerate(cells):
            header = headers[index] if index < len(headers) else ""
            if _input_header(header) or header.strip().lower() in DESCRIPTIVE_HEADERS:
                continue
            cell_claims = list(_tokens_with_context(cell))
            for component_index, (value, unit, subfield) in enumerate(cell_claims):
                if unit == "" and not re.search(r"\d", value):
                    continue
                unit = unit or _header_unit(header)
                row = dict(base)
                dimensions = {
                    _context_key(context_headers[dimension_index]): cells[dimension_index].strip()
                    for dimension_index in range(min(len(context_headers), len(cells)))
                    if dimension_index != index
                    and _context_dimension_header(context_headers[dimension_index])
                }
                current_context_header = context_headers[index] if index < len(context_headers) else header
                dimensions.update(_profile_column_dimensions(current_context_header))
                row["context"] = json.dumps(
                    {
                        "kind": "table",
                        "section": section,
                        "column": current_context_header,
                        "column_is_parameter": _parameter_header(current_context_header),
                        "table_header": context_headers[0] if context_headers else "",
                        "subfield": subfield,
                        "dimensions": dimensions,
                        **(
                            {"component_index": component_index}
                            if len(cell_claims) > 1 and ("/" in cell or ("[" in cell and "]" in cell))
                            else {}
                        ),
                    },
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                )
                row.update(
                    value=value,
                    unit=unit,
                    tier=_tier(section, line, header=header, table_row=True, contract=contract),
                    **{"class": _class(value, unit, f"{header} {context}")},
                )
                output.append(row)
        return output

    if _eligible_text(line, section, contract=contract):
        for value, unit, label in _tokens_with_context(line):
            row = dict(base)
            row["context"] = json.dumps(
                {"kind": "prose", "section": section, "label": label},
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            )
            row.update(value=value, unit=unit, tier=_tier(section, line, contract=contract), **{"class": _class(value, unit, context)})
            output.append(row)
    return output


def scan_documents(root: Path = ROOT) -> list[dict[str, object]]:
    # Scan report prose and contract documents; the TOML manifest is configuration, not measured prose.
    candidates = sorted((root / "docs" / "reports").glob("*.md")) + sorted((root / "docs" / "contracts").glob("*.md"))
    candidates = [path for path in candidates if path.name.lower() not in {"readme.md", "renames.md"}]
    found: list[dict[str, object]] = []
    ordinals: dict[tuple[str, str], int] = {}
    for path in candidates:
        contract = "contracts" in path.parts
        section = "document"
        headers: list[str] | None = None
        context_headers: list[str] | None = None
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            heading = HEADING.match(line)
            if heading:
                section = heading.group(1)
                headers = None
                context_headers = None
            if line.lstrip().startswith("|"):
                cells = _split_cells(line)
                if context_headers is None and cells and not _is_separator(cells):
                    context_headers = cells
                if headers is None and cells and not _is_separator(cells):
                    headers = cells
                    continue
                if _is_separator(cells):
                    continue
            else:
                context_headers = None
            claims = _line_claims(
                path=path,
                line_number=line_number,
                line=line,
                section=section,
                headers=headers,
                context_headers=context_headers,
                contract=contract,
            )
            relative = path.relative_to(root).as_posix()
            for claim in claims:
                section_key = (relative, str(claim["section anchor"]))
                ordinal = ordinals.get(section_key, 0) + 1
                ordinals[section_key] = ordinal
                claim["claim_key"] = f"{relative}#{section_key[1]}:{ordinal}"
            found.extend(claims)
    return found


def locate_claim(claim_key: str, root: Path = ROOT) -> dict[str, object] | None:
    return next((claim for claim in scan_documents(root) if claim["claim_key"] == claim_key), None)


def _normalize_source(text: str) -> str:
    text = re.sub(r"^L\d+:\s*", "", text.strip())
    return "".join(char for char in text.casefold() if char.isalnum())


def _report_renames(root: Path = ROOT) -> dict[str, str]:
    path = root / "docs" / "reports" / "RENAMES.md"
    if not path.exists():
        return {}
    result: dict[str, str] = {}
    row = re.compile(r"^\|\s*`([^`]+)`\s*\|\s*`([^`]+)`\s*\|")
    for line in path.read_text(encoding="utf-8").splitlines():
        match = row.match(line)
        if match:
            old, new = match.groups()
            result[Path(old).name] = Path(new).name
    return result


def match_old_claims(
    old_claims: list[dict[str, str | int]], current_claims: list[dict[str, object]], root: Path = ROOT
) -> tuple[dict[int, dict[str, str | int]], set[int]]:
    """Match old rows for inventory diagnostics; do not use matches as current provenance."""
    renames = _report_renames(root)
    old_by_report_value: dict[tuple[str, str], list[tuple[dict[str, str | int], str]]] = {}
    historical_m1: list[tuple[dict[str, str | int], str]] = []
    for claim in old_claims:
        report = renames.get(str(claim["report"]), str(claim["report"]))
        source = _normalize_source(str(claim.get("source text", "")))
        if report == "M1_CONTRACT_FREEZE.md" and str(claim.get("status", "")) == "unrecoverable":
            historical_m1.append((claim, source))
            continue
        old_by_report_value.setdefault((report, str(claim["value"])), []).append((claim, source))

    matches: dict[int, dict[str, str | int]] = {}
    ambiguous: set[int] = set()
    metadata_fields = ("unit", "class", "source harness", "raw data", "status", "M5 stage", "notes")
    current_by_report_value: dict[tuple[str, str], list[tuple[int, dict[str, object], str]]] = {}
    for index, current in enumerate(current_claims):
        current_by_report_value.setdefault((str(current["report"]), str(current["value"])), []).append(
            (index, current, _normalize_source(str(current["claim text (short)"])))
        )

    used_current: set[int] = set()
    for old, old_source in sorted(historical_m1, key=lambda item: (int(item[0]["line"]), int(item[0]["old_claim_id"]))):
        report = renames.get(str(old["report"]), str(old["report"]))
        candidates = [
            (index, current)
            for index, current, current_source in current_by_report_value.get((report, str(old["value"])), [])
            if index not in used_current and (not old_source or current_source.startswith(old_source))
        ]
        fallback = not candidates
        if fallback:
            candidates = [
                (index, current)
                for index, current, _ in current_by_report_value.get((report, str(old["value"])), [])
                if index not in used_current
            ]
        if not candidates:
            continue
        old_line = int(old["line"])
        if old_line and any(int(current.get("line", 0)) > 0 for _, current in candidates):
            distance = min(abs(int(current.get("line", 0)) - old_line) for _, current in candidates if int(current.get("line", 0)) > 0)
            candidates = [(index, current) for index, current in candidates if int(current.get("line", 0)) > 0 and abs(int(current.get("line", 0)) - old_line) == distance]
        index, _ = min(candidates, key=lambda item: (int(item[1].get("line", 0)), item[0]))
        matched_old = dict(old)
        if fallback:
            matched_old["mapping_mode"] = "historical-report-value-fallback"
        matches[index] = matched_old
        used_current.add(index)

    for index, current in enumerate(current_claims):
        if index in matches:
            continue
        report = str(current["report"])
        value = str(current["value"])
        source = _normalize_source(str(current["claim text (short)"]))
        candidates = [
            (claim, old_source)
            for claim, old_source in old_by_report_value.get((report, value), [])
            if old_source and source.startswith(old_source)
        ]
        if not candidates:
            continue
        # Prefer the most specific old source prefix; line numbers only resolve equal-source matches.
        longest = max(len(old_source) for _, old_source in candidates)
        candidates = [(claim, old_source) for claim, old_source in candidates if len(old_source) == longest]
        current_line = int(current.get("line", 0))
        if current_line and any(int(claim["line"]) > 0 for claim, _ in candidates):
            distance = min(abs(current_line - int(claim["line"])) for claim, _ in candidates if int(claim["line"]) > 0)
            candidates = [(claim, old_source) for claim, old_source in candidates if int(claim["line"]) > 0 and abs(current_line - int(claim["line"])) == distance]
        signatures = {tuple(str(claim[field]) for field in metadata_fields) for claim, _ in candidates}
        if len(signatures) != 1:
            ambiguous.add(index)
        else:
            matches[index] = candidates[0][0]
    return matches, ambiguous


def _aligned(value: str, count: int) -> list[str]:
    parts = value.split(";") if value else [""]
    if len(parts) == count:
        return parts
    if len(parts) == 1:
        return parts * count
    return (parts + [""] * count)[:count]


def expand_old_ledger(path: Path = OLD_LEDGER) -> list[dict[str, str | int]]:
    previous = {field: "" for field in OLD_META_FIELDS}
    expanded: list[dict[str, str | int]] = []
    with path.open(encoding="utf-8-sig", newline="") as stream:
        for row in csv.DictReader(stream):
            for field in OLD_META_FIELDS:
                value = row.get(field, "") or ""
                if value:
                    previous[field] = "" if value.lower() == "none" else value
                else:
                    row[field] = previous[field]
            raw_ids = row.get("claim_id", "")
            match = re.fullmatch(r"(\d+)(?:-(\d+))?", raw_ids)
            if not match:
                continue
            first, last = int(match.group(1)), int(match.group(2) or match.group(1))
            count = last - first + 1
            aligned = {field: _aligned(row.get(field, "") or "", count) for field in OLD_META_FIELDS}
            for index, claim_id in enumerate(range(first, last + 1)):
                source_text = aligned["claim text (short)"][index]
                line_match = re.match(r"L(\d+):", source_text)
                line_number = int(line_match.group(1)) if line_match else 0
                expanded.append(
                    {
                        "old_claim_id": claim_id,
                        "report": Path(aligned["report"][index]).name,
                        "line": line_number,
                        "source text": source_text,
                        "value": aligned["value"][index].replace(",", ""),
                        "unit": aligned["unit"][index],
                        "class": aligned["class"][index],
                        "source harness": aligned["source harness"][index] or "none",
                        "raw data": aligned["raw data"][index] or "none",
                        "status": aligned["status"][index],
                        "M5 stage": aligned["M5 stage"][index],
                        "notes": aligned["notes"][index],
                    }
                )
    return expanded


def apply_m5_04_overrides(
    claims: list[dict[str, object]], root: Path = ROOT, overrides: dict[str, object] | None = None
) -> list[dict[str, object]]:
    if overrides is None:
        path = root / M5_04_OVERRIDES
        if not path.exists():
            return claims
        overrides = json.loads(path.read_text(encoding="utf-8"))
    records = overrides.get("claims", {})
    sources = load_sources(root)
    for claim in claims:
        if {"report", "section anchor", "class"}.issubset(claim):
            source = source_for_claim(
                sources,
                str(claim["report"]),
                str(claim["section anchor"]),
                str(claim["class"]),
            )
            if source is None or source.get("regeneration_stage") != "M5-04":
                continue
        override = records.get(str(claim["claim_id"]))
        if override is None:
            continue
        claim["status"] = str(override.get("ledger_status", override["result"]))
        notes = [
            note for note in str(claim["notes"]).split("; ")
            if not note.startswith("M5-04:")
        ]
        claim["notes"] = "; ".join(
            filter(None, [*notes, f"M5-04: {override['note']}"])
        )
    return claims


def apply_m5_05_results(claims: list[dict[str, object]], root: Path = ROOT) -> list[dict[str, object]]:
    """Apply recorded M5-05 conclusion outcomes to matching headline claims only."""
    path = root / M5_05_RESULTS
    if not path.is_file():
        path = root / M5_05_LEGACY_RESULTS
    if not path.is_file():
        return claims
    results = json.loads(path.read_text(encoding="utf-8"))
    statuses = {
        "holds": "conclusion holds",
        "does not hold": "conclusion changed",
        "inconclusive": "inconclusive",
    }
    by_source = {
        (str(row["report"]), str(row["section_anchor"])): row
        for row in results.get("conclusions", [])
    }
    for claim in claims:
        if str(claim.get("M5 stage")) != "M5-05" or str(claim.get("tier")) not in {"headline", "key"}:
            continue
        if str(claim.get("class")) != "timing":
            continue
        result = by_source.get((Path(str(claim.get("report", ""))).name, str(claim.get("section anchor", ""))))
        if result is None or str(claim.get("status")) == "missing-source":
            continue
        status = statuses.get(str(result.get("status")))
        if status is None:
            continue
        claim["status"] = status
        claim["notes"] = "; ".join(
            filter(None, [str(claim.get("notes", "")), f"M5-05: {result.get('notes', result.get('status'))}"])
        )
    return claims


def apply_m3_05_pilot_results(claims: list[dict[str, object]], root: Path = ROOT) -> list[dict[str, object]]:
    """Apply the recorded M3-05 pilot classification by stable claim key."""
    path = root / M3_05_PILOT_RESULTS
    if not path.is_file():
        return claims
    results = json.loads(path.read_text(encoding="utf-8"))
    by_key = {str(row["claim_key"]): row for row in results.get("claims", [])}
    allowed = {"reproduced", "reproduced (loose)", "not reproduced", "inconclusive (validity)"}
    for claim in claims:
        row = by_key.get(str(claim.get("claim_key", "")))
        if row is None or str(claim.get("status", "")) != str(row.get("status_before", "")):
            continue
        status = str(row.get("verdict", ""))
        if status not in allowed:
            raise ValueError(f"invalid M3-05 pilot status for {row['claim_key']}: {status}")
        claim["status"] = status
        class_override = row.get("class_override")
        if class_override:
            previous_class = str(claim.get("class", ""))
            claim["class"] = str(class_override)
            reason = str(row.get("class_reason", "method review changed the measurement classification"))
            claim["notes"] = "; ".join(
                filter(None, [str(claim.get("notes", "")), f"M3-05 class {previous_class}→{class_override}: {reason}"])
            )
        claim["notes"] = "; ".join(
            filter(None, [str(claim.get("notes", "")), f"M3-05 pilot: {row.get('note', status)}"])
        )
    return claims


def extract_claims(
    root: Path = ROOT,
    old_ledger: Path = OLD_LEDGER,
    *,
    apply_m5_04: bool = True,
    apply_recovered: bool = True,
    apply_m3_05_pilot: bool = True,
) -> list[dict[str, object]]:
    current = scan_documents(root)
    sources = load_sources(root)
    validate_source_paths(sources, root)
    source_map_present = (root / "docs" / "reproducibility" / "sources.toml").is_file()
    if source_map_present:
        old_matches: dict[int, dict[str, str | int]] = {}
        ambiguous_claims: set[int] = set()
    else:
        old_claims = expand_old_ledger(old_ledger)
        old_matches, ambiguous_claims = match_old_claims(old_claims, current, root)
    output: list[dict[str, object]] = []
    for index, claim in enumerate(current):
        mapped = source_for_claim(
            sources,
            str(claim["report"]),
            str(claim["section anchor"]),
            str(claim["class"]),
        )
        old = old_matches.get(index)
        if source_map_present and mapped is None:
            status, source_harness, raw_data, stage = "unmapped", "none", "none", "none"
            note = "report has no sources.toml entry; source attribution requires review"
        elif source_map_present:
            status = str(mapped["status"])
            source_commands = [*mapped.get("harness", []), *mapped.get("tests", [])]
            source_harness = "; ".join(source_commands) or "none"
            raw_data = "; ".join(mapped.get("data", [])) or "none"
            stage = str(mapped["regeneration_stage"])
            note = str(mapped.get("notes", ""))
            if mapped.get("review"):
                note = "; ".join(filter(None, ["review required", note]))
        else:
            # Standalone extraction roots without a source map keep the old diagnostic behavior.
            if old:
                status = str(old["status"] or "unmapped")
                source_harness = str(old["source harness"] or "none")
                raw_data = str(old["raw data"] or "none")
                stage = str(old["M5 stage"] or "none")
                note = str(old["notes"] or "")
                if str(claim["report"]) == "M1_CONTRACT_FREEZE.md" and status == "unrecoverable":
                    status = "historical-log"
                    note = "M1 aggregate test-run output is a historical log, not a regeneration target."
                match_note = (
                    "M1 historical-log carried by report/value and nearest line because source matching was unavailable"
                    if str(old.get("mapping_mode", "")) == "historical-report-value-fallback"
                    else "matched by renamed report, normalized source text, and value; line used only as a tie-breaker"
                )
                note = "; ".join(filter(None, [note, match_note]))
            else:
                status = source_harness = raw_data = stage = "unmapped"
                note = (
                    "ambiguous duplicate old mappings; provenance was not inferred"
                    if index in ambiguous_claims
                    else "not matched by normalized (report, source text, value); prior provenance was not inferred"
                )
        output.append(
            {
                "claim_id": len(output) + 1,
                **claim,
                "source harness": source_harness,
                "raw data": raw_data,
                "status": status,
                "M5 stage": stage,
                "notes": note,
            }
        )
    output = apply_m5_05_results(output, root)
    if apply_recovered:
        output = apply_recorded_statuses(output, root)
    if apply_m3_05_pilot:
        output = apply_m3_05_pilot_results(output, root)
    if apply_m5_04:
        # Final M4 regeneration verdicts take precedence over recovered-output evidence.
        output = apply_m5_04_overrides(output, root)
    return output


def ledger_bytes(claims: list[dict[str, object]]) -> bytes:
    text = io.StringIO(newline="")
    writer = csv.DictWriter(text, fieldnames=FIELDS, lineterminator="\n")
    writer.writeheader()
    for claim in claims:
        writer.writerow({field: claim.get(field, "") for field in FIELDS})
    raw = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as stream:
        stream.write(text.getvalue().encode("utf-8"))
    return raw.getvalue()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare regenerated bytes without writing")
    parser.add_argument("--locate", metavar="CLAIM_KEY", help="print the current source section and line for one stable claim key")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args(argv)
    if args.locate:
        claim = locate_claim(args.locate)
        if claim is None:
            print(f"claim key not found: {args.locate}", file=sys.stderr)
            return 1
        print(f"{claim['claim_key']}\t{claim['report']}#{claim['section anchor']}\tline {claim['line']}: {claim['claim text (short)']}")
        return 0
    claims = extract_claims()
    rendered = ledger_bytes(claims)
    if args.check:
        if not args.output.exists() or not content_matches(args.output, rendered):
            print(f"claim ledger differs from regenerated output: {args.output}", file=sys.stderr)
            return 1
        print(f"claim ledger matches: {args.output}")
        return 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(rendered)
    print(f"wrote {len(claims)} tidy claims ({len(rendered)} bytes): {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
