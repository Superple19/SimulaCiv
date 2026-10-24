"""Map ledger values to exact tokens in the recovered historical outputs."""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import io
import json
import re
import sys
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

from .io import write_text_lf


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "docs" / "reproducibility" / "recovered_claim_statuses.json.gz"
MISMATCH_OUTPUT = ROOT / "docs" / "reproducibility" / "recovered_claim_mismatches.csv.gz"
INPUT_HEADERS = {
    "n", "k", "seed", "master seed", "master_seed", "replicate", "replicate_id",
    "p_local", "relationship_degree_k", "neighbors", "max_hops", "horizon",
    "threads", "chunk", "chunk size", "workload", "configuration", "config",
    "input", "rank", "claim id", "groups", "group count", "settlements", "settlement_count",
    "initial_population", "master_seed", "initial_health", "initial_food",
    "initial_wealth", "initial_settlement_resource", "carrying_capacity", "regrowth_rate",
    "base_metabolic_cost", "health_decay_rate", "base_work_yield", "migration_probability",
    "migration_threshold", "migration_cost", "migration_cooldown_days", "birth_probability",
    "birth_food_threshold", "starvation_threshold", "action_biases", "base_weight_matrix",
}
NUMBER = re.compile(
    r"(?<![\w.])(?P<number>[+-]?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?|[+-]?\.\d+)"
    r"(?P<suffix>%|×|x|µs|μs|us|ms|ns|seconds?|secs?|s|days?|agents?|ticks?|k|M)?",
    re.IGNORECASE,
)
HASH = re.compile(r"(?<![0-9a-fA-F])[0-9a-fA-F]{64}(?![0-9a-fA-F])")
CONFIG_ASSIGNMENT = re.compile(
    r"\b(?:n|k|p_local|seed|master_seed|replicate(?:_id)?|threads?|chunk(?:_size)?|"
    r"groups?|settlements?|settlement_count|population|days?|horizon|relationship_degree_k)\s*[:=|]\s*$",
    re.IGNORECASE,
)
OUTPUT_SUFFIXES = {".csv", ".tsv", ".json", ".jsonl", ".log", ".txt", ".out", ".err", ".stdout", ".stderr"}
DIMENSION_KEYS = {
    "population_n", "settlement_count", "relationship_degree_k", "p_local", "seed", "master_seed",
    "replicate", "replicate_id", "day", "days", "horizon", "threads", "thread_count", "chunk",
    "chunk_size", "max_hops", "transmit", "transmit_probability", "phase", "stage", "profile",
    "runner", "path", "workload", "scenario", "mode", "action", "family", "layout", "case",
    "system", "schedule", "pool", "pool_size", "checkpoint", "interval", "from_n", "to_n",
    "frontiers", "levels_used", "max_frontier_size", "frontier_sizes_by_level", "levels_max",
    "grouping", "authority", "target", "cell", "candidate",
}
GENERIC_LABELS = {"the", "of", "and", "with", "by", "per", "at", "us", "ms", "ns", "ticks", "tick", "mean", "median", "mad", "value", "result"}


def _input_header(value: str) -> bool:
    normalized = re.sub(r"[`*_]", "", value).strip().lower()
    return normalized in INPUT_HEADERS


def _value_key(value: str, unit: str = "") -> str:
    if unit.casefold() == "sha256" or HASH.fullmatch(value):
        return value.lower()
    try:
        return str(Decimal(value.replace(",", "")).normalize())
    except InvalidOperation:
        return value.strip().lower()


def _add_text_values(values: set[str], text: str) -> None:
    for match in HASH.finditer(text):
        values.add(match.group(0).lower())
    for match in NUMBER.finditer(text):
        prefix = text[max(0, match.start() - 60) : match.start()]
        if CONFIG_ASSIGNMENT.search(prefix):
            continue
        try:
            values.add(str(Decimal(match.group("number").replace(",", "")).normalize()))
        except InvalidOperation:
            continue


def _json_values(value: Any, values: set[str]) -> None:
    if isinstance(value, dict):
        for child_key, child in value.items():
            if _input_header(str(child_key)):
                continue
            _json_values(child, values)
    elif isinstance(value, list):
        for child in value:
            _json_values(child, values)
    elif isinstance(value, bool) or value is None:
        return
    elif isinstance(value, int):
        values.add(str(Decimal(value).normalize()))
    elif isinstance(value, float):
        if value == value and value not in {float("inf"), float("-inf")}:
            values.add(str(Decimal(str(value)).normalize()))
    elif isinstance(value, str):
        _add_text_values(values, value)


def _output_values(path: Path) -> set[str]:
    values: set[str] = set()
    suffix = path.suffix.lower()
    text = path.read_text(encoding="utf-8-sig", errors="replace")
    if suffix in {".csv", ".tsv"}:
        delimiter = "\t" if suffix == ".tsv" else ","
        rows = list(csv.reader(text.splitlines(), delimiter=delimiter))
        header: list[str] | None = None
        for row in rows:
            if not row:
                continue
            if header is None:
                header = row
                continue
            for index, cell in enumerate(row):
                if index < len(header) and _input_header(header[index]):
                    continue
                _add_text_values(values, cell)
        return values
    if suffix in {".json", ".jsonl"}:
        for line in text.splitlines():
            if not line.strip():
                continue
            try:
                _json_values(json.loads(line), values)
            except json.JSONDecodeError:
                _add_text_values(values, line)
        return values

    pipe_header: list[str] | None = None
    for line in text.splitlines():
        if "|" in line and line.startswith(("ROW|", "TICK|", "ONETIME|", "VOLUME|", "RELEASE_DIFF|")):
            cells = line.split("|")
            if cells[0] == "ROW":
                pipe_header = cells[1:]
            elif pipe_header is not None:
                for index, cell in enumerate(cells[1:]):
                    if index < len(pipe_header) and _input_header(pipe_header[index]):
                        continue
                    _add_text_values(values, cell)
            else:
                _add_text_values(values, line)
        else:
            _add_text_values(values, line)
    return values


def _canonical_key(value: str) -> str:
    raw = value.strip()
    if raw == "K":
        return "settlement_count"
    if raw == "k":
        return "relationship_degree_k"
    if raw.casefold() == "n":
        return "population_n"
    normalized = re.sub(r"[^a-z0-9]+", "_", raw.casefold()).strip("_")
    aliases = {
        "groups": "settlement_count",
        "group_count": "settlement_count",
        "settlements": "settlement_count",
        "settlement_count": "settlement_count",
        "p": "p_local",
        "from_population": "from_n",
        "from_population_n": "from_n",
        "from_n": "from_n",
        "to_population": "to_n",
        "to_population_n": "to_n",
        "to_n": "to_n",
        "frontier_sizes_by_level": "frontiers",
        "frontiers": "frontiers",
        "level_count": "levels_used",
        "levels": "levels_used",
        "levels_used": "levels_used",
        "levels_max": "levels_max",
        "checkpoint": "horizon",
        "max_frontier": "max_frontier_size",
        "max_frontier_size": "max_frontier_size",
        "transmit_probability": "transmit_probability",
        "transmit": "transmit_probability",
        "day": "horizon",
    }
    return aliases.get(normalized, normalized)


def _label_tokens(value: str) -> set[str]:
    lowered = value.casefold().replace("µs", "us").replace("μs", "us")
    lowered = re.sub(r"\bpool\s*(\d+)\b", r"p\1", lowered)
    return {token for token in re.findall(r"[a-z0-9]+", lowered) if token not in GENERIC_LABELS}


def _dimension_value(value: str) -> str:
    raw = re.sub(r"^(?:day|days|horizon)\s*", "", value.strip(), flags=re.IGNORECASE).replace(",", "")
    if raw.startswith("[") and raw.endswith("]"):
        return "[" + ",".join(part.strip() for part in raw[1:-1].split(",")) + "]"
    scaled = re.fullmatch(r"([0-9]+(?:\.[0-9]+)?)([kKmM])", raw)
    if scaled:
        multiplier = Decimal(1_000 if scaled.group(2).lower() == "k" else 1_000_000)
        return str((Decimal(scaled.group(1)) * multiplier).normalize())
    try:
        return str(Decimal(raw).normalize())
    except InvalidOperation:
        return " ".join(sorted(_label_tokens(raw)))


def _numeric_tokens(text: str) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    for match in HASH.finditer(text):
        tokens.append((match.group(0).lower(), match.group(0)))
    for match in NUMBER.finditer(text):
        raw = match.group("number").replace(",", "")
        try:
            tokens.append((str(Decimal(raw).normalize()), match.group(0)))
        except InvalidOperation:
            continue
    return tokens


def _record_from_fields(path: str, line_number: int, text: str, fields: dict[str, str]) -> dict[str, Any]:
    dimensions: dict[str, str] = {}
    measures: dict[str, list[tuple[str, str]]] = {}
    for raw_key, raw_value in fields.items():
        key = _canonical_key(raw_key)
        if key in DIMENSION_KEYS:
            dimensions[key] = raw_value.strip()
        else:
            values = _numeric_tokens(raw_value)
            if values:
                measures[key] = values
    if "frontiers" in dimensions:
        measures["frontiers"] = _numeric_tokens(dimensions["frontiers"])
    if "levels_used" in dimensions and "max_frontier_size" in dimensions:
        measures["levels_max"] = _numeric_tokens(
            f"{dimensions['levels_used']}/{dimensions['max_frontier_size']}"
        )
    return {
        "path": path,
        "line": line_number,
        "text": text,
        "dimensions": dimensions,
        "measures": measures,
    }


def _output_records(path: Path, relative: str) -> list[dict[str, Any]]:
    """Read labeled output records without treating the whole report's number set as evidence."""
    text = path.read_text(encoding="utf-8-sig", errors="replace")
    suffix = path.suffix.lower()
    records: list[dict[str, Any]] = []
    if suffix in {".csv", ".tsv"}:
        delimiter = "\t" if suffix == ".tsv" else ","
        rows = list(csv.reader(text.splitlines(), delimiter=delimiter))
        if not rows:
            return records
        header = rows[0]
        for line_number, row in enumerate(rows[1:], 2):
            fields = {header[i]: cell for i, cell in enumerate(row) if i < len(header)}
            records.append(_record_from_fields(relative, line_number, delimiter.join(row), fields))
        return records
    if suffix in {".json", ".jsonl"}:
        for line_number, line in enumerate(text.splitlines(), 1):
            if not line.strip():
                continue
            try:
                payload = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, list):
                objects = [row for row in payload if isinstance(row, dict)]
            elif isinstance(payload, dict):
                objects = [payload]
            else:
                continue
            for item in objects:
                flat: dict[str, str] = {}

                def visit(value: Any, prefix: str = "") -> None:
                    if isinstance(value, dict):
                        for child_key, child in value.items():
                            visit(child, f"{prefix}_{child_key}".strip("_"))
                    elif isinstance(value, list):
                        flat[prefix] = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
                    elif value is None or isinstance(value, bool):
                        return
                    else:
                        flat[prefix] = str(value)

                visit(item)
                records.append(_record_from_fields(relative, line_number, line, flat))
        return records

    pipe_header: list[str] | None = None
    pipe_kind: str | None = None
    for line_number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        if line.startswith(("ROW|", "PHASE_ROW|", "GRID_HEADER|")):
            pipe_kind = line.split("|", 1)[0]
            pipe_header = line.split("|")[1:]
            continue
        allowed_rows = {
            "ROW": ("TICK|", "ONETIME|", "VOLUME|", "RELEASE_DIFF|"),
            "PHASE_ROW": ("PHASE|",),
            "GRID_HEADER": ("GRID_ROW|",),
        }
        if pipe_header and not line.startswith(allowed_rows.get(pipe_kind or "", ())):
            pipe_header = None
            pipe_kind = None
        if line.startswith("ONETIME|"):
            fields = dict(zip(
                ("N", "K", "k", "p_local", "graph_median_us", "graph_mad_us", "projection_median_us", "projection_mad_us"),
                line.split("|")[1:],
            ))
            records.append(_record_from_fields(relative, line_number, line, fields))
            continue
        if line.startswith("VOLUME|"):
            fields = dict(zip(
                ("N", "K", "k", "p_local", "keyed_mean", "cross_partition_mean", "conflict_fraction", "mean_contention"),
                line.split("|")[1:],
            ))
            records.append(_record_from_fields(relative, line_number, line, fields))
            continue
        if line.startswith("SUSTAINED_PHASE|"):
            cells = line.split("|")
            fields = {}
            for part in cells[1:5]:
                match = re.fullmatch(r"([A-Za-z][A-Za-z0-9_-]*)=(.*)", part)
                if match:
                    fields[match.group(1)] = match.group(2)
            if len(cells) >= 8:
                fields.update(zip(("phase", "median_us", "mad_us"), cells[5:8]))
            records.append(_record_from_fields(relative, line_number, line, fields))
            continue
        if line.startswith("TICK|") and pipe_header is None and "=" not in line:
            fields = dict(zip(
                ("horizon", "N", "K", "p", "threads", "chunk", "serial_median_us", "serial_mad_us", "parallel_median_us", "parallel_mad_us", "speedup"),
                line.split("|")[1:],
            ))
            records.append(_record_from_fields(relative, line_number, line, fields))
            continue
        if pipe_header and line.startswith(("TICK|", "PHASE|", "ONETIME|", "VOLUME|", "RELEASE_DIFF|", "GRID_ROW|")):
            fields = {pipe_header[i]: cell for i, cell in enumerate(line.split("|")[1:]) if i < len(pipe_header)}
            records.append(_record_from_fields(relative, line_number, line, fields))
            continue
        parts = line.split("|")
        fields: dict[str, str] = {}
        for part in parts[1:] if len(parts) > 1 else parts:
            match = re.match(r"^\s*([A-Za-z][A-Za-z0-9_-]*)\s*=\s*(.*?)\s*$", part)
            if match:
                fields[match.group(1)] = match.group(2)
        if fields:
            records.append(_record_from_fields(relative, line_number, line, fields))
    return records


PROFILE_COLUMN = re.compile(r"^([0-9][0-9,]*(?:\.[0-9]+)?\s*[kKmM]?)\s*/\s*([0-9.]+)$")


def _phase_value(value: str) -> str:
    tokens = _label_tokens(value)
    suffixes = {"features", "projection", "market", "welfare", "mortality", "metrics", "events", "exchange", "lookup", "generation", "writeback", "output"}
    tokens = ["intent" if token == "intents" else token for token in tokens if token not in suffixes]
    return " ".join(tokens)


def _section_dimensions(section: str) -> dict[str, str]:
    aliases = {"N": "population_n", "K": "settlement_count", "k": "relationship_degree_k"}
    dimensions = {}
    for label, key in aliases.items():
        if match := re.search(rf"\b{label}\s*=\s*([\d,]+)", section):
            dimensions[key] = match.group(1)
    for label, key in (("p_local", "p_local"), ("seed", "seed"), ("replicate", "replicate"), ("threads", "threads"), ("chunk", "chunk_size")):
        if match := re.search(rf"\b{label}\s*=\s*([\d.]+)", section, re.IGNORECASE):
            dimensions[key] = match.group(1)
    if match := re.search(r"\bcheckpoint\s+day\s+(\d+)\b", section, re.IGNORECASE):
        dimensions["horizon"] = match.group(1)
    return dimensions


def _claim_context(claim: dict[str, Any]) -> dict[str, Any] | None:
    try:
        context = json.loads(str(claim.get("context", "")))
    except json.JSONDecodeError:
        return None
    if context.get("kind") != "table" or context.get("column_is_parameter"):
        return None
    column = str(context.get("column", "")).replace("μ", "µ")
    column_key = column.strip().casefold().replace("μ", "µ")
    subfield = str(context.get("subfield", "")).strip()
    section = str(context.get("section", ""))
    dimensions = {
        _canonical_key(str(key)): str(value)
        for key, value in context.get("dimensions", {}).items()
    }
    for key, value in _section_dimensions(section).items():
        dimensions.setdefault(key, value)
    profile = PROFILE_COLUMN.fullmatch(column.strip())
    table_header = str(context.get("table_header", ""))
    subfield_key = subfield.casefold()
    compact_section = section.casefold().replace("-", "")
    noninterleaved = "noninterleaved" in compact_section
    interleaved = "interleaved" in compact_section and not noninterleaved
    if match := re.search(r"\bpool\s*(\d+)\b", section, re.IGNORECASE):
        dimensions["threads"] = match.group(1)
    if match := re.search(r"\bchunk\s*(\d+)\b", section, re.IGNORECASE):
        dimensions["chunk_size"] = match.group(1)
    if profile:
        dimensions["population_n"] = profile.group(1).replace(" ", "")
        dimensions["p_local"] = profile.group(2)
        label = "mad_us_tick" if subfield_key == "mad" else "median_us_tick"
    elif setting := re.fullmatch(r"\s*p_local\s*=\s*([0-9.]+)\s*", column, re.IGNORECASE):
        dimensions["p_local"] = setting.group(1)
        label = "mad_us" if subfield_key == "mad" else "median_us"
    elif "alpha" in _label_tokens(column) and subfield:
        dimensions["phase"] = subfield
        label = "alpha"
    else:
        label = _canonical_key(column)
        if re.fullmatch(r"\s*P\d+[A-Z]?\s*(?:[a-zA-Z /+-]+)?\s*", column) and subfield_key in {"median", "mad"}:
            dimensions["phase"] = column
            label = "mad_us" if subfield_key == "mad" else "median_us"
        elif subfield_key in {"median", "mad"} and "graph build" in column.casefold():
            label = "graph_mad_us" if subfield_key == "mad" else "graph_median_us"
        elif subfield_key in {"median", "mad"} and "projection build" in column.casefold():
            label = "projection_mad_us" if subfield_key == "mad" else "projection_median_us"
        elif subfield_key in {"median", "mad"} and re.search(r"median\s*±\s*MAD|median\s*±\s*mad", column, re.IGNORECASE):
            label = "mad_us_tick" if subfield_key == "mad" else "median_us_tick"
        elif pool := re.fullmatch(r"\s*pool\s*(\d+)\s*µ?s\s*", column, re.IGNORECASE):
            variant = "ni" if noninterleaved else "i"
            measure = "mad" if subfield_key == "mad" else "us"
            label = f"p{pool.group(1)}_{variant}_{measure}"
        elif re.fullmatch(r"\s*serial\s*µ?s\s*", column, re.IGNORECASE):
            variant = "ni" if noninterleaved else "i"
            measure = "mad" if subfield_key == "mad" else "us"
            label = f"serial_{variant}_{measure}"
        elif column_key in {"serial µs/tick", "parallel µs/tick"}:
            side = column_key.split()[0]
            if noninterleaved or interleaved:
                variant = "noninterleaved" if noninterleaved else "interleaved"
                suffix = "_mad" if subfield_key == "mad" else ""
                label = f"{variant}_{side}{suffix}"
            else:
                suffix = "mad_us" if subfield_key == "mad" else "median_us"
                label = f"{side}_{suffix}"
        elif column_key == "speedup":
            label = "noninterleaved_speedup" if noninterleaved else "interleaved_speedup" if interleaved else "speedup"
        elif column_key == "frontier sizes by level":
            label = "frontiers"
        elif column_key == "levels/max":
            label = "levels_max"
    identity_dimensions = {
        "population_n", "settlement_count", "relationship_degree_k", "p_local", "seed", "master_seed",
        "replicate", "replicate_id", "horizon", "days", "day", "phase", "stage", "system",
        "max_hops", "transmit_probability", "threads", "thread_count", "chunk", "chunk_size",
        "profile", "scenario", "workload", "runner", "mode", "case", "action",
        "candidate",
    }
    if not dimensions or not identity_dimensions.intersection(dimensions):
        return None
    return {
        "column": column,
        "subfield": subfield,
        "label": label,
        "dimensions": dimensions,
        "section": section,
        **({"component_index": int(context["component_index"])} if "component_index" in context else {}),
    }


def _dimension_matches(expected: dict[str, str], actual: dict[str, str]) -> bool:
    expected = {_canonical_key(key): value for key, value in expected.items()}
    actual = {_canonical_key(key): value for key, value in actual.items()}
    identity_dimensions = {
        "population_n", "settlement_count", "relationship_degree_k", "p_local", "seed", "master_seed",
        "replicate", "replicate_id", "horizon", "days", "day", "phase", "stage", "system",
        "from_n", "to_n", "max_hops", "transmit_probability", "threads", "thread_count", "chunk",
        "chunk_size", "profile", "scenario", "workload", "runner", "mode", "case", "action",
        "candidate",
    }
    expected_identity = dict(expected)
    if "interval" in expected_identity:
        bounds = re.split(r"→|->", expected_identity.pop("interval"))
        if len(bounds) != 2:
            return False
        expected_identity["from_n"], expected_identity["to_n"] = bounds
    if identity_dimensions.intersection(actual) != identity_dimensions.intersection(expected_identity):
        return False
    for key, expected_value in expected.items():
        if key == "interval":
            bounds = re.split(r"→|->", expected_value)
            if len(bounds) != 2:
                return False
            if _dimension_value(actual.get("from_n", "")) != _dimension_value(bounds[0]):
                return False
            if _dimension_value(actual.get("to_n", "")) != _dimension_value(bounds[1]):
                return False
        elif key == "levels_max":
            parts = re.split(r"/|:", expected_value)
            if len(parts) != 2:
                return False
            if _dimension_value(actual.get("levels_used", "")) != _dimension_value(parts[0]):
                return False
            if _dimension_value(actual.get("max_frontier_size", "")) != _dimension_value(parts[1]):
                return False
        else:
            actual_value = actual.get(key)
            if actual_value is None:
                return False
            left = _dimension_value(expected_value)
            right = _dimension_value(actual_value)
            if key == "phase" and _phase_value(left) == _phase_value(right):
                continue
            if left == right:
                continue
            left_tokens = _label_tokens(left)
            right_tokens = _label_tokens(right)
            if not left_tokens or left_tokens != right_tokens:
                return False
    return True


def _field_matches(context: dict[str, Any], field: str) -> bool:
    return _canonical_key(context["label"]) == _canonical_key(field)


def _context_records(claim_context: dict[str, Any], output: dict[str, Any]) -> list[dict[str, Any]]:
    signature = _identity_signature(claim_context["dimensions"])
    records = output.get("record_index", {}).get(signature, [])
    return [record for record in records if _dimension_matches(claim_context["dimensions"], record["dimensions"])]


def _identity_signature(dimensions: dict[str, str]) -> tuple[tuple[str, str], ...]:
    identity = {
        "population_n", "settlement_count", "relationship_degree_k", "p_local", "seed", "master_seed",
        "replicate", "replicate_id", "horizon", "days", "day", "phase", "stage", "system",
        "from_n", "to_n", "max_hops", "transmit_probability", "threads", "thread_count", "chunk",
        "chunk_size", "profile", "scenario", "workload", "runner", "mode", "case", "action", "candidate",
    }
    canonical = {_canonical_key(key): value for key, value in dimensions.items()}
    if "interval" in canonical:
        bounds = re.split(r"→|->", canonical.pop("interval"))
        if len(bounds) == 2:
            canonical["from_n"], canonical["to_n"] = bounds
    return tuple(sorted(
        (key, _phase_value(value) if key == "phase" else _dimension_value(value))
        for key, value in canonical.items() if key in identity
    ))


def _context_values(claim_context: dict[str, Any], record: dict[str, Any]) -> list[tuple[str, str, str, int]]:
    return [
        (field, value, raw, component_index)
        for field, values in record["measures"].items()
        if _field_matches(claim_context, field)
        for component_index, (value, raw) in enumerate(values)
    ]


def _match_claim_context(claim: dict[str, Any], candidates: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    context = _claim_context(claim)
    if context is None:
        return [], []
    expected = _value_key(str(claim.get("value", "")), str(claim.get("unit", "")))
    exact: list[dict[str, Any]] = []
    mismatches: list[dict[str, Any]] = []
    for output in candidates:
        for record in _context_records(context, output):
            for field, actual, raw, component_index in _context_values(context, record):
                result = {
                    "path": output["path"],
                    "line": record["line"],
                    "column": context["column"],
                    "output_field": field,
                    "component_index": component_index,
                    "expected": str(claim.get("value", "")),
                    "actual": raw,
                    "line_text": record["text"],
                }
                if actual == expected:
                    exact.append(result)
                else:
                    mismatches.append(result)
    return exact, mismatches


def _report_aliases(root: Path) -> dict[str, str]:
    aliases: dict[str, str] = {}
    path = root / "docs" / "reports" / "RENAMES.md"
    if path.is_file():
        for line in path.read_text(encoding="utf-8").splitlines():
            match = re.match(r"^\|\s*`([^`]+)`\s*\|\s*`([^`]+)`\s*\|", line)
            if match:
                aliases[Path(match.group(1)).name] = Path(match.group(2)).name
    return aliases


def _manifest_outputs(root: Path) -> tuple[str, list[dict[str, Any]]]:
    stage_root = root / "docs" / "data" / "m5_recovered"
    stage_manifest = stage_root / "MANIFEST.json"
    if not stage_manifest.is_file():
        raise FileNotFoundError(f"recovered evidence manifest is missing: {stage_manifest}")
    manifest_bytes = stage_manifest.read_bytes()
    manifest = json.loads(manifest_bytes)
    outputs: list[dict[str, Any]] = []
    for item in manifest["items"]:
        if item.get("type") not in {"raw-output-copy", "dvc-cache-raw-output"}:
            continue
        relative = str(item["path"])
        path = stage_root / Path(relative)
        if path.suffix.lower() not in OUTPUT_SUFFIXES or not item.get("reports_supported"):
            continue
        if not path.is_file():
            raise FileNotFoundError(f"recovered output is not materialized: {relative}; run dvc checkout after retrieval")
        content = path.read_bytes()
        if len(content) != int(item["size_bytes"]) or hashlib.sha256(content).hexdigest() != item["sha256"]:
            raise ValueError(f"recovered output differs from its manifest: {relative}")
        records = _output_records(path, relative)
        record_index: dict[tuple[tuple[str, str], ...], list[dict[str, Any]]] = {}
        for record in records:
            record_index.setdefault(_identity_signature(record["dimensions"]), []).append(record)
        outputs.append({
            "path": relative,
            "reports_supported": item["reports_supported"],
            "values": _output_values(path),
            "records": records,
            "record_index": record_index,
        })
    return hashlib.sha256(manifest_bytes).hexdigest(), outputs


def build_mapping(claims: list[dict[str, Any]], root: Path = ROOT) -> dict[str, Any]:
    manifest_sha, outputs = _manifest_outputs(root)
    aliases = _report_aliases(root)
    by_report: dict[str, list[dict[str, Any]]] = {}
    for output in outputs:
        for report in output["reports_supported"]:
            name = Path(str(report)).name
            name = aliases.get(name, name)
            by_report.setdefault(name, []).append(output)

    matched: list[dict[str, Any]] = []
    mismatches: list[dict[str, Any]] = []
    unmatched: list[int] = []
    unsupported: list[int] = []
    config_values: list[int] = []
    for claim in claims:
        report = Path(str(claim.get("report", ""))).name
        candidates = by_report.get(report, [])
        if not candidates:
            unsupported.append(int(claim["claim_id"]))
            continue
        try:
            source_context = json.loads(str(claim.get("context", "")))
        except json.JSONDecodeError:
            source_context = {}
        if source_context.get("column_is_parameter"):
            config_values.append(int(claim["claim_id"]))
            continue
        exact, differing = _match_claim_context(claim, candidates)
        # Keep one evidence row per distinct output line/field; timing runs may differ by sample.
        exact = list({(r["path"], r["line"], r["output_field"], r["actual"]): r for r in exact}.values())
        differing = list({(r["path"], r["line"], r["output_field"], r["actual"]): r for r in differing}.values())
        if not exact and not differing:
            unmatched.append(int(claim["claim_id"]))
            continue
        before = str(claim.get("status", ""))
        after = before
        if differing:
            after = "recovered-output-mismatch"
        elif before.startswith("missing-"):
            after = "recovered-original-output"
        component_index = source_context.get("component_index")
        if component_index is not None and exact and not any(
            int(row.get("component_index", -1)) == int(component_index) for row in exact
        ):
            after = "recovered-weak (composite field)"
        claim_row = {
            "claim_id": int(claim["claim_id"]),
            "report": report,
            "tier": str(claim.get("tier", "supporting")),
            "status_before": before,
            "status_after": after,
            "output_matches": exact[:1],
            "mismatches": differing[:1],
        }
        matched.append(claim_row)
        mismatches.extend(
            {"claim_id": int(claim["claim_id"]), "report": report, "context": str(claim.get("context", "")), **r}
            for r in differing
        )
    report_outputs = {
        report: sorted({str(output["path"]) for output in report_items})
        for report, report_items in sorted(by_report.items())
    }
    mapping = {
        "schema_version": 1,
        "matching_rule": "A recovered measurement requires an exact decimal or SHA-256 value in the same output row, exact normalized measure field, and every row/config identity dimension present on either side. Prose-only claims and numeric configuration/input columns are not recovered measurements; a different value in the same context is a mismatch.",
        "stage_manifest_sha256": manifest_sha,
        "outputs_scanned": len(outputs),
        "report_outputs": report_outputs,
        "claims": matched,
        "unmatched_claim_ids": unmatched,
        "claims_without_associated_output_ids": unsupported,
        "config_value_claim_ids": config_values,
        "confirmed_mismatches": mismatches,
    }
    from .recovered_mismatch_triage import classify_mapping

    return classify_mapping(mapping, claims, by_report, root)


def apply_recorded_statuses(claims: list[dict[str, Any]], root: Path = ROOT) -> list[dict[str, Any]]:
    path = root / "docs" / "reproducibility" / "recovered_claim_statuses.json"
    compressed_path = root / "docs" / "reproducibility" / "recovered_claim_statuses.json.gz"
    if compressed_path.is_file():
        with gzip.open(compressed_path, "rt", encoding="utf-8") as stream:
            mapping = json.load(stream)
    elif path.is_file():
        mapping = json.loads(path.read_text(encoding="utf-8"))
    else:
        return claims
    by_id = {int(row["claim_id"]): row for row in mapping.get("claims", [])}
    for claim in claims:
        row = by_id.get(int(claim["claim_id"]))
        if row is None or str(claim.get("status", "")) != str(row.get("status_before", "")):
            continue
        after = str(row.get("status_after", row["status_before"]))
        if after in {
            "recovered-original-output",
            "recovered-original-output (formatted)",
            "recovered-original-output (unit-converted)",
            "recovered-weak (composite field)",
            "recovered-other-run",
            "recovered-derived",
            "recovered-output-mismatch",
        }:
            claim["status"] = after
            evidence = row.get("mismatches") or row.get("output_matches") or []
            first = evidence[0] if evidence else {}
            reference = (
                f"{first.get('path', '')}:{first.get('line', '')}"
                if first
                else ", ".join(mapping.get("report_outputs", {}).get(str(row.get("report", "")), []))
            )
            fallback = (
                f"expected {first.get('expected')} but output recorded {first.get('actual')}"
                if after == "recovered-output-mismatch"
                else "exact value and context matched"
            )
            detail = str(row.get("triage_rationale", fallback))
            claim["notes"] = "; ".join(
                filter(
                    None,
                    [
                        str(claim.get("notes", "")),
                        f"M5 recovered ({row.get('triage_cause', 'exact context')}): {detail} at {reference}",
                    ],
                )
            )
    return claims


def write_mapping(path: Path = OUTPUT, root: Path = ROOT) -> dict[str, Any]:
    from .claims import extract_claims

    claims = extract_claims(root, apply_recovered=False)
    mapping = build_mapping(claims, root)
    payload = json.dumps(mapping, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    compressed = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=compressed, mtime=0, compresslevel=9) as stream:
        stream.write(payload)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(compressed.getvalue())
    mismatch_text = io.StringIO(newline="")
    mismatch_fields = ["claim_id", "report", "context", "expected", "actual", "output_path", "line", "column", "output_field", "component_index"]
    writer = csv.DictWriter(mismatch_text, fieldnames=mismatch_fields, lineterminator="\n")
    writer.writeheader()
    for row in mapping["confirmed_mismatches"]:
        writer.writerow({
            field: row.get("path", "") if field == "output_path" else row.get(field, "")
            for field in mismatch_fields
        })
    mismatch_bytes = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=mismatch_bytes, mtime=0, compresslevel=9) as stream:
        stream.write(mismatch_text.getvalue().encode("utf-8"))
    write_path = root / MISMATCH_OUTPUT.relative_to(ROOT)
    write_path.parent.mkdir(parents=True, exist_ok=True)
    write_path.write_bytes(mismatch_bytes.getvalue())
    return mapping


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)
    try:
        mapping = write_mapping()
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"recovered claim mapping failed: {error}", file=sys.stderr)
        return 1
    summary = collections_summary(mapping)
    print(json.dumps(summary, sort_keys=True))
    return 0


def collections_summary(mapping: dict[str, Any]) -> dict[str, int]:
    rows = mapping["claims"]
    recovered_statuses = {
        "recovered-original-output",
        "recovered-original-output (formatted)",
        "recovered-original-output (unit-converted)",
        "recovered-other-run",
        "recovered-derived",
    }
    return {
        "outputs_scanned": int(mapping["outputs_scanned"]),
        "exact_claim_matches": sum(row["status_after"] in recovered_statuses for row in rows),
        "moved_from_missing": sum(row["status_after"] in recovered_statuses for row in rows),
        "recovered_weak": sum(row["status_after"] == "recovered-weak (composite field)" for row in rows),
        "recovered_output_mismatches": sum(row["status_after"] == "recovered-output-mismatch" for row in rows),
        "config_values_unchanged": len(mapping["config_value_claim_ids"]),
        "unmatched_claims": len(mapping["unmatched_claim_ids"]),
        "claims_without_output": len(mapping["claims_without_associated_output_ids"]),
        "triage_candidate_records": len(mapping["confirmed_mismatches"]),
        "genuine_discrepancy_claims": sum(row["status_after"] == "recovered-output-mismatch" for row in rows),
    }


if __name__ == "__main__":
    raise SystemExit(main())
