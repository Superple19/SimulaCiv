"""Extract tidy numeric claims from reports and measured contract sections."""

from __future__ import annotations

import argparse
import csv
import gzip
import io
import re
import sys
from collections.abc import Iterable, Iterator
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "docs" / "reproducibility" / "claims.csv.gz"
OLD_LEDGER = ROOT / "docs" / "reproducibility" / "superseded" / "claims.csv"
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
UNRECOVERABLE_M1_REASON = (
    "M1 recheck: the value is recorded in the frozen M1 contract and the associated current test/golden source was inspected; "
    "the M1 manifest contains no aggregate test-run output, so the exact historical passing-count record remains unrecoverable."
)


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


def _tier(section: str, line: str, *, audit: bool) -> str:
    context = f"{section} {line}".lower()
    if audit or any(term in context for term in ("summary", "findings", "gate verdict", "graduation")):
        return "key"
    if re.search(r"\b(?:pass|fail|inconclusive|complete|blocked)\b", context):
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
    if re.match(r"^\s*[-*+]\s+", line) and "finding" in section.lower():
        return True
    if contract and any(term in section.lower() for term in ("empirical", "measured", "validation", "benchmark", "performance")):
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
    audit = "audit" in path.stem.lower() or "graduation" in path.stem.lower()
    context = f"{section} {line}"
    base = {
        "report": path.name,
        "line": line_number,
        "section anchor": _slug(section),
        "claim text (short)": line.strip()[:240],
        "tier": _tier(section, line, audit=audit),
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
                row.update(value=value, unit=unit, **{"class": _class(value, unit, f"{header} {context}")})
                output.append(row)
        return output

    if _eligible_text(line, section, contract=contract):
        for value, unit in _tokens(line):
            row = dict(base)
            row.update(value=value, unit=unit, **{"class": _class(value, unit, context)})
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
            line_match = re.match(r"L(\d+):", aligned["claim text (short)"][0])
            line_number = int(line_match.group(1)) if line_match else 0
            for index, claim_id in enumerate(range(first, last + 1)):
                expanded.append(
                    {
                        "old_claim_id": claim_id,
                        "report": Path(aligned["report"][index]).name,
                        "line": line_number,
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


def _old_metadata(
    old_claims: Iterable[dict[str, str | int]],
) -> dict[tuple[str, int, str], dict[str, str | int] | None]:
    grouped: dict[tuple[str, int, str], list[dict[str, str | int]]] = {}
    for claim in old_claims:
        key = (str(claim["report"]), int(claim["line"]), str(claim["value"]))
        grouped.setdefault(key, []).append(claim)
    result: dict[tuple[str, int, str], dict[str, str | int] | None] = {}
    metadata_fields = ("unit", "class", "source harness", "raw data", "status", "M5 stage", "notes")
    for key, matches in grouped.items():
        signatures = {tuple(str(claim[field]) for field in metadata_fields) for claim in matches}
        result[key] = matches[0] if len(signatures) == 1 else None
    return result


def extract_claims(root: Path = ROOT, old_ledger: Path = OLD_LEDGER) -> list[dict[str, object]]:
    metadata = _old_metadata(expand_old_ledger(old_ledger))
    output: list[dict[str, object]] = []
    for claim in scan_documents(root):
        key = (str(claim["report"]), int(claim["line"]), str(claim["value"]))
        old = metadata.get(key)
        if old:
            status = str(old["status"] or "unmapped")
            note = str(old["notes"] or "")
            source_harness = str(old["source harness"] or "none")
            raw_data = str(old["raw data"] or "none")
            stage = str(old["M5 stage"] or "none")
            claim_class = str(old["class"] or claim["class"])
            unit = str(old["unit"] or claim["unit"])
            if str(claim["report"]) == "M1_CONTRACT_FREEZE.md" and status == "unrecoverable":
                note = "; ".join(filter(None, [note, UNRECOVERABLE_M1_REASON]))
        else:
            status = source_harness = raw_data = stage = "unmapped"
            ambiguous = key in metadata and metadata[key] is None
            note = (
                "ambiguous duplicate old mappings; provenance was not inferred"
                if ambiguous
                else "not matched by exact (report, line, value); prior provenance was not inferred"
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
