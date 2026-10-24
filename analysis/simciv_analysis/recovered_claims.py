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
        outputs.append({"path": relative, "reports_supported": item["reports_supported"], "values": _output_values(path)})
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

    exact: list[dict[str, Any]] = []
    unmatched: list[int] = []
    unsupported: list[int] = []
    for claim in claims:
        report = Path(str(claim.get("report", ""))).name
        candidates = by_report.get(report, [])
        if not candidates:
            unsupported.append(int(claim["claim_id"]))
            continue
        token = _value_key(str(claim.get("value", "")), str(claim.get("unit", "")))
        paths = sorted({str(output["path"]) for output in candidates if token in output["values"]})
        if not paths:
            unmatched.append(int(claim["claim_id"]))
            continue
        before = str(claim.get("status", ""))
        after = "recovered-original-output" if before.startswith("missing-") else before
        exact.append(
            {
                "claim_id": int(claim["claim_id"]),
                "report": report,
                "tier": str(claim.get("tier", "supporting")),
                "status_before": before,
                "status_after": after,
            }
        )
    report_outputs = {
        report: sorted({str(output["path"]) for output in report_items})
        for report, report_items in sorted(by_report.items())
    }
    return {
        "schema_version": 1,
        "matching_rule": "Exact decimal or SHA-256 token membership in a recovered output associated with the same report; input columns and labeled config assignments are excluded. This is report-scoped evidence, not a claim that the source output row was semantically identified.",
        "stage_manifest_sha256": manifest_sha,
        "outputs_scanned": len(outputs),
        "report_outputs": report_outputs,
        "claims": exact,
        "unmatched_claim_ids": unmatched,
        "claims_without_associated_output_ids": unsupported,
        "confirmed_mismatches": [],
    }


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
        if after == "recovered-original-output":
            claim["status"] = after
            paths = ", ".join(mapping.get("report_outputs", {}).get(str(row.get("report", "")), []))
            claim["notes"] = "; ".join(
                filter(None, [str(claim.get("notes", "")), f"M5 recovered: exact value token in report-scoped output: {paths}"])
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
    return {
        "outputs_scanned": int(mapping["outputs_scanned"]),
        "exact_claim_matches": len(rows),
        "moved_from_missing": sum(row["status_after"] == "recovered-original-output" for row in rows),
        "unmatched_claims": len(mapping["unmatched_claim_ids"]),
        "claims_without_output": len(mapping["claims_without_associated_output_ids"]),
        "confirmed_mismatches": len(mapping["confirmed_mismatches"]),
    }


if __name__ == "__main__":
    raise SystemExit(main())
