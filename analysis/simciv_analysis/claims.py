"""Extract tidy numeric claims from reports and measured contract sections."""

from __future__ import annotations

import argparse
import csv
import gzip
import io
import re
import sys
from collections.abc import Iterator
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "docs" / "reproducibility" / "claims.csv.gz"
OLD_LEDGER = ROOT / "docs" / "reproducibility" / "superseded" / "claims.csv"
RENAMES = ROOT / "docs" / "reports" / "RENAMES.md"
FIELDS = [
    "claim_id",
    "report",
    "line",
    "section anchor",
    "claim text (short)",
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
    masked = DATE.sub(" ", text)
    masked = VERSION.sub(" ", masked)
    masked = MILESTONE.sub(" ", masked)
    spans: list[tuple[int, int]] = []
    for match in HASH.finditer(masked):
        spans.append(match.span())
        yield match.group(0).lower(), "sha256"
    for match in NUMBER.finditer(masked):
        if any(match.start() < end and match.end() > start for start, end in spans):
            continue
        if _input_context(masked, match.start()):
            continue
        value = match.group("number").replace(",", "")
        unit = match.group("suffix") or _unit_after(masked, match.end())
        yield value, unit


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
    }
    output: list[dict[str, object]] = []
    if line.lstrip().startswith("|") and headers is not None and not _is_separator(_split_cells(line)):
        cells = _split_cells(line)
        for index, cell in enumerate(cells):
            header = headers[index] if index < len(headers) else ""
            if _input_header(header) or header.strip().lower() in DESCRIPTIVE_HEADERS:
                continue
            for value, unit in _tokens(cell):
                if unit == "" and not re.search(r"\d", value):
                    continue
                unit = unit or _header_unit(header)
                row = dict(base)
                row.update(
                    value=value,
                    unit=unit,
                    tier=_tier(section, line, header=header, table_row=True, contract=contract),
                    **{"class": _class(value, unit, f"{header} {context}")},
                )
                output.append(row)
        return output

    if _eligible_text(line, section, contract=contract):
        for value, unit in _tokens(line):
            row = dict(base)
            row.update(value=value, unit=unit, tier=_tier(section, line, contract=contract), **{"class": _class(value, unit, context)})
            output.append(row)
    return output


def scan_documents(root: Path = ROOT) -> list[dict[str, object]]:
    # Scan report prose and contract documents; the TOML manifest is configuration, not measured prose.
    candidates = sorted((root / "docs" / "reports").glob("*.md")) + sorted((root / "docs" / "contracts").glob("*.md"))
    candidates = [path for path in candidates if path.name.lower() not in {"readme.md", "renames.md"}]
    found: list[dict[str, object]] = []
    for path in candidates:
        contract = "contracts" in path.parts
        section = "document"
        headers: list[str] | None = None
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            heading = HEADING.match(line)
            if heading:
                section = heading.group(1)
                headers = None
            if line.lstrip().startswith("|"):
                cells = _split_cells(line)
                if headers is None and cells and not _is_separator(cells):
                    headers = cells
                    continue
                if _is_separator(cells):
                    continue
            found.extend(
                _line_claims(
                    path=path,
                    line_number=line_number,
                    line=line,
                    section=section,
                    headers=headers,
                    contract=contract,
                )
            )
    return found


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
    """Carry old provenance by renamed report, normalized short source, and value; line is only a tie-breaker."""
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
        if old_line:
            distance = min(abs(int(current["line"]) - old_line) for _, current in candidates)
            candidates = [(index, current) for index, current in candidates if abs(int(current["line"]) - old_line) == distance]
        index, _ = min(candidates, key=lambda item: (int(item[1]["line"]), item[0]))
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
        current_line = int(current["line"])
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


def extract_claims(root: Path = ROOT, old_ledger: Path = OLD_LEDGER) -> list[dict[str, object]]:
    current = scan_documents(root)
    old_claims = expand_old_ledger(old_ledger)
    metadata, ambiguous_claims = match_old_claims(old_claims, current, root)
    output: list[dict[str, object]] = []
    for index, claim in enumerate(current):
        old = metadata.get(index)
        if old:
            status = str(old["status"] or "unmapped")
            note = str(old["notes"] or "")
            source_harness = str(old["source harness"] or "none")
            raw_data = str(old["raw data"] or "none")
            stage = str(old["M5 stage"] or "none")
            claim_class = str(old["class"] or claim["class"])
            unit = str(old["unit"] or claim["unit"])
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
            ambiguous = index in ambiguous_claims
            note = (
                "ambiguous duplicate old mappings; provenance was not inferred"
                if ambiguous
                else "not matched by normalized (report, source text, value); prior provenance was not inferred"
            )
            claim_class = str(claim["class"])
            unit = str(claim["unit"])
        output.append(
            {
                "claim_id": len(output) + 1,
                **claim,
                "unit": unit,
                "class": claim_class,
                "source harness": source_harness,
                "raw data": raw_data,
                "status": status,
                "M5 stage": stage,
                "notes": note,
            }
        )
    return output


def ledger_bytes(claims: list[dict[str, object]]) -> bytes:
    text = io.StringIO(newline="")
    writer = csv.DictWriter(text, fieldnames=FIELDS, lineterminator="\n")
    writer.writeheader()
    for claim in claims:
        writer.writerow(claim)
    raw = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as stream:
        stream.write(text.getvalue().encode("utf-8"))
    return raw.getvalue()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare regenerated bytes without writing")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args(argv)
    claims = extract_claims()
    rendered = ledger_bytes(claims)
    if args.check:
        if not args.output.exists() or args.output.read_bytes() != rendered:
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
