"""Classify recovered-output mismatches using recorded values and contexts only."""

from __future__ import annotations

import csv
import gzip
import io
import json
import subprocess
from collections import Counter, defaultdict
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP
from pathlib import Path
from typing import Any

from .io import write_text_lf
from .recovered_claims import ROOT, _claim_context, _match_claim_context


TRIAGE_OUTPUT = Path("docs/reproducibility/recovered_claim_mismatch_triage.csv.gz")
TRIAGE_SUMMARY = Path("docs/reproducibility/recovered_claim_mismatch_triage.json")
TRIAGE_COLUMNS = [
    "claim_id", "report", "tier", "cause", "subcategory", "status_after", "expected",
    "closest_or_matching_value", "relative_difference", "evidence", "rationale",
]
BASELINE_MISMATCH_COMMIT = "fddd9a91b81841a1a2b1d7ddb5e999de20838cb3"
STATUS_BY_CAUSE = {
    "context parser correction": "recovered-original-output",
    "rounding/formatting": "recovered-original-output (formatted)",
    "composite field position mismatch": "recovered-weak (composite field)",
    "unit conversion": "recovered-original-output (unit-converted)",
    "different run": "recovered-other-run",
    "derived": "recovered-derived",
    "genuine discrepancy": "recovered-output-mismatch",
}
MISMATCH_CAUSES = (
    "rounding/formatting",
    "composite field position mismatch",
    "unit conversion",
    "different run",
    "derived",
    "genuine discrepancy",
)


def _decimal(value: str) -> Decimal | None:
    value = value.replace(",", "").strip()
    if value.endswith("%"):
        value = value[:-1].strip()
    try:
        result = Decimal(value)
    except InvalidOperation:
        return None
    return result if result.is_finite() else None


def _printed_equal(value: str, expected: str) -> bool:
    actual_decimal = _decimal(value)
    expected_decimal = _decimal(expected)
    if actual_decimal is None or expected_decimal is None:
        return False
    places = max(0, -expected_decimal.as_tuple().exponent)
    quantum = Decimal(1).scaleb(-places)
    return actual_decimal.quantize(quantum, rounding=ROUND_HALF_UP) == expected_decimal.quantize(
        quantum, rounding=ROUND_HALF_UP
    )


def _same_record(left: dict[str, Any], right: dict[str, Any]) -> bool:
    return (left.get("path"), left.get("line"), left.get("output_field")) == (
        right.get("path"), right.get("line"), right.get("output_field")
    )


def _unit_for_claim(claim: dict[str, Any], context: dict[str, Any] | None) -> str:
    unit = str(claim.get("unit", "")).casefold()
    if unit:
        return unit.replace("μ", "µ")
    column = str((context or {}).get("column", "")).casefold().replace("μ", "µ")
    for unit in ("µs", "ms", "ns", "%", "seconds", "secs", "s"):
        if unit in column:
            return unit
    return ""


def _field_unit(field: str) -> str:
    lowered = field.casefold()
    for suffix, unit in (("_us", "µs"), ("_ms", "ms"), ("_ns", "ns"), ("_seconds", "s")):
        if lowered.endswith(suffix):
            return unit
    return ""


def _unit_converted(actual: str, expected: str, source_unit: str, field: str) -> tuple[bool, str]:
    actual_decimal = _decimal(actual)
    expected_decimal = _decimal(expected)
    target_unit = _field_unit(field)
    if actual_decimal is None or expected_decimal is None:
        return False, ""
    time_scale = {"ns": Decimal("1e-9"), "µs": Decimal("1e-6"), "us": Decimal("1e-6"), "ms": Decimal("1e-3"), "s": Decimal(1), "seconds": Decimal(1), "secs": Decimal(1)}
    source = source_unit.replace("µ", "µ")
    if source in time_scale and target_unit in time_scale and source != target_unit:
        converted = actual_decimal * time_scale[target_unit] / time_scale[source]
        if _printed_equal(str(converted), expected):
            return True, f"{actual} {target_unit} converts to {expected} {source}"
    if source_unit == "%" and ("fraction" in field.casefold() or "ratio" in field.casefold()):
        if _printed_equal(str(actual_decimal * 100), expected):
            return True, f"fraction {actual} converts to {expected}%"
    if source_unit in {"ratio", "fraction"} and ("pct" in field.casefold() or "percent" in field.casefold()):
        if _printed_equal(str(actual_decimal / 100), expected):
            return True, f"percent {actual}% converts to ratio {expected}"
    return False, ""


def _relative_difference(expected: str, actual: str) -> str:
    expected_decimal = _decimal(expected)
    actual_decimal = _decimal(actual)
    if expected_decimal is None or actual_decimal is None:
        return ""
    difference = abs(actual_decimal - expected_decimal)
    if expected_decimal == 0:
        return "" if difference == 0 else "infinite (expected zero)"
    return str((difference / abs(expected_decimal)).normalize())


def _record_index(outputs: list[dict[str, Any]]) -> dict[tuple[str, int], dict[str, Any]]:
    return {
        (str(output["path"]), int(record["line"])): record
        for output in outputs
        for record in output["records"]
    }


def _baseline_mismatches(root: Path, mapping: dict[str, Any]) -> list[dict[str, str]]:
    if root.resolve() != ROOT.resolve():
        return list(mapping["confirmed_mismatches"])
    raw = subprocess.check_output(
        ["git", "show", f"{BASELINE_MISMATCH_COMMIT}:docs/reproducibility/recovered_claim_mismatches.csv.gz"],
        cwd=root,
    )
    return list(csv.DictReader(io.StringIO(gzip.decompress(raw).decode("utf-8"))))


def _derived_value(record: dict[str, Any], output_field: str) -> tuple[Decimal | None, str]:
    field = output_field.casefold()
    measures = {
        name: _decimal(values[0][1])
        for name, values in record["measures"].items()
        if values and _decimal(values[0][1]) is not None
    }
    if any(word in field for word in ("speedup", "ratio", "alpha")):
        numerator_fields = [name for name in measures if any(key in name for key in ("serial", "base", "baseline")) and not any(key in name for key in ("mad", "ratio", "speedup"))]
        denominator_fields = [name for name in measures if any(key in name for key in ("parallel", "fast", "optimized")) and not any(key in name for key in ("mad", "ratio", "speedup"))]
        for numerator_name in numerator_fields:
            for denominator_name in denominator_fields:
                numerator, denominator = measures[numerator_name], measures[denominator_name]
                if denominator:
                    return numerator / denominator, f"{numerator_name}/{denominator_name}"
    if any(word in field for word in ("delta", "difference")):
        names = list(measures)
        pairs = [(a, b) for a in names for b in names if a != b and
                 (("registered" in a and "legacy" in b) or ("before" in a and "after" in b))]
        for left, right in pairs:
            return measures[left] - measures[right], f"{left}-{right}"
    if field.startswith("total_") or field.endswith("_sum"):
        prefix = field.removeprefix("total_").removesuffix("_sum") + "_"
        components = {
            name: value
            for name, value in measures.items()
            if name.startswith(prefix) and not any(word in name for word in ("total", "sum", "mad", "ratio", "speedup"))
        }
        if len(components) >= 2:
            return sum(components.values(), Decimal(0)), "+".join(sorted(components))
    return None, ""


def _classify_one(
    claim: dict[str, Any],
    mismatch_rows: list[dict[str, Any]],
    outputs: list[dict[str, Any]],
    record_by_path_line: dict[tuple[str, int], dict[str, Any]],
) -> dict[str, Any]:
    expected = str(claim.get("value", ""))
    context = _claim_context(claim)
    exact, differing = _match_claim_context(claim, outputs)
    same_record_exact = [
        good for good in exact
        if any(_same_record(good, bad) for bad in differing)
    ]
    expected_component = context.get("component_index") if context else None
    position_matched = [
        good for good in same_record_exact
        if expected_component is not None and int(good.get("component_index", -1)) == expected_component
    ]

    rounded = [row for row in differing if _printed_equal(str(row["actual"]), expected)]
    if rounded:
        evidence = rounded[0]
        actual = str(evidence["actual"])
        return _result(claim, "rounding/formatting", "printed-precision rounding", expected, actual,
                       f"{evidence['path']}:{evidence['line']}",
                       f"{actual} rounds to the report's {max(0, -(_decimal(expected) or Decimal(0)).as_tuple().exponent)} decimal places as {expected}.")

    if position_matched:
        evidence = position_matched[0]
        return _result(claim, "rounding/formatting", "composite field component at matching position", expected, expected,
                       f"{evidence['path']}:{evidence['line']}",
                       f"The expected scalar matches component {expected_component} in the same output field and row.")

    if same_record_exact:
        evidence = same_record_exact[0]
        if expected_component is not None:
            return _result(claim, "composite field position mismatch", "composite field component at different or unknown position", expected, expected,
                           f"{evidence['path']}:{evidence['line']}",
                           f"The expected scalar occurs in the same output field and row, but not at the claimed component position {expected_component}.")
        return _result(claim, "rounding/formatting", "composite field component", expected, expected,
                       f"{evidence['path']}:{evidence['line']}",
                       "The expected scalar is present in the same output field and row.")

    unit = _unit_for_claim(claim, context)
    for row in differing:
        converted, rationale = _unit_converted(str(row["actual"]), expected, unit, str(row["output_field"]))
        if converted:
            return _result(claim, "unit conversion", "unit-normalized value", expected, str(row["actual"]),
                           f"{row['path']}:{row['line']}", rationale)

    mismatch_records = {(str(row["path"]), int(row["line"]), str(row["output_field"])) for row in differing}
    other_run_exact = [row for row in exact if (str(row["path"]), int(row["line"]), str(row["output_field"])) not in mismatch_records]
    if other_run_exact:
        row = sorted(other_run_exact, key=lambda item: (item["path"], item["line"]))[0]
        return _result(claim, "different run", "exact value in another recovered record", expected, str(row["actual"]),
                       f"{row['path']}:{row['line']}",
                       "The report value occurs in an additional same-context recovered record; the other differing records are repeats/follow-up runs.")

    numeric_records: list[tuple[Decimal, dict[str, Any]]] = []
    for row in differing:
        value = _decimal(str(row["actual"]))
        if value is not None:
            numeric_records.append((value, row))
    unique_records = {(row["path"], row["line"]) for _value, row in numeric_records}
    if len(unique_records) > 1 and numeric_records:
        numbers = sorted(value for value, _row in numeric_records)
        middle = len(numbers) // 2
        aggregates = {
            "minimum": numbers[0],
            "maximum": numbers[-1],
            "mean": sum(numbers) / Decimal(len(numbers)),
            "median": numbers[middle] if len(numbers) % 2 else (numbers[middle - 1] + numbers[middle]) / 2,
        }
        for name, value in aggregates.items():
            if _printed_equal(str(value), expected):
                sources = sorted({f"{row['path']}:{row['line']}" for _value, row in numeric_records})
                return _result(claim, "different run", f"{name} across same-context records", expected, str(value),
                               "; ".join(sources),
                               f"The report value equals the {name} of the recorded same-context values across runs.")

    derived_matches = []
    for row in differing:
        record = record_by_path_line.get((str(row["path"]), int(row["line"])))
        if not record:
            continue
        derived, formula = _derived_value(record, str(row["output_field"]))
        if derived is not None and _printed_equal(str(derived), expected):
            derived_matches.append((derived, formula, row))
    if derived_matches:
        derived, formula, row = derived_matches[0]
        return _result(claim, "derived", formula, expected, str(derived), f"{row['path']}:{row['line']}",
                       f"The report value is computed from recovered fields using {formula}.")

    closest = min(
        differing,
        key=lambda row: abs((_decimal(str(row["actual"])) or Decimal("Infinity")) - (_decimal(expected) or Decimal(0))),
    )
    actual = str(closest["actual"])
    return _result(claim, "genuine discrepancy", "no normalization or same-context derivation matched", expected, actual,
                   f"{closest['path']}:{closest['line']}",
                   f"Closest recorded value is {actual}; relative difference is {_relative_difference(expected, actual)}.")


def _result(
    claim: dict[str, Any], cause: str, subcategory: str, expected: str, actual: str,
    evidence: str, rationale: str,
) -> dict[str, Any]:
    return {
        "claim_id": int(claim["claim_id"]),
        "report": Path(str(claim.get("report", ""))).name,
        "tier": str(claim.get("tier", "supporting")),
        "cause": cause,
        "subcategory": subcategory,
        "status_after": STATUS_BY_CAUSE[cause],
        "expected": expected,
        "closest_or_matching_value": actual,
        "relative_difference": _relative_difference(expected, actual),
        "evidence": evidence,
        "rationale": rationale,
    }


def classify_mapping(
    mapping: dict[str, Any], claims: list[dict[str, Any]], by_report: dict[str, list[dict[str, Any]]], root: Path,
    *, write_outputs: bool = True,
) -> dict[str, Any]:
    by_id = {int(claim["claim_id"]): claim for claim in claims}
    baseline_rows = _baseline_mismatches(root, mapping)
    baseline_by_claim: dict[int, list[dict[str, str]]] = defaultdict(list)
    for row in baseline_rows:
        baseline_by_claim[int(row["claim_id"])].append(row)
    mismatch_by_claim: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for row in mapping["confirmed_mismatches"]:
        mismatch_by_claim[int(row["claim_id"])].append(row)
    current_claims = {int(row["claim_id"]): row for row in mapping["claims"]}
    if not set(mismatch_by_claim).issubset(baseline_by_claim):
        raise ValueError("the current contextual matcher introduced mismatch claims outside the pre-triage set")
    outputs_by_path = {
        str(output["path"]): output
        for report_outputs in by_report.values()
        for output in report_outputs
    }
    outputs = list(outputs_by_path.values())
    record_by_path_line = {
        (str(output["path"]), int(record["line"])): record
        for output in outputs for record in output["records"]
    }
    triage_rows = []
    by_cause_claims: Counter[str] = Counter()
    by_cause_records: Counter[str] = Counter()
    by_cause_tiers: dict[str, Counter[str]] = defaultdict(Counter)
    by_cause_subcategory_claims: Counter[tuple[str, str]] = Counter()
    by_cause_subcategory_records: Counter[tuple[str, str]] = Counter()
    by_report_cause_claims: Counter[tuple[str, str]] = Counter()
    by_report_cause_records: Counter[tuple[str, str]] = Counter()
    claims_by_id = current_claims
    for claim_id, baseline_evidence in sorted(baseline_by_claim.items()):
        claim = by_id[claim_id]
        report = Path(str(claim.get("report", ""))).name
        current = claims_by_id.get(claim_id)
        if current is None:
            raise ValueError(f"pre-triage mismatch claim disappeared from the current map: {claim_id}")
        if claim_id not in mismatch_by_claim:
            exact = current.get("output_matches", [])
            if current.get("status_after") == "recovered-weak (composite field)" and exact:
                evidence = exact[0]
                result = _result(
                    claim,
                    "composite field position mismatch",
                    "composite field component at different or unknown position",
                    str(claim.get("value", "")),
                    str(evidence["actual"]),
                    f"{evidence['path']}:{evidence['line']}",
                    f"The expected scalar occurs at component {evidence.get('component_index', 'unknown')}, not claimed position {json.loads(str(claim.get('context', '{}'))).get('component_index', 'unknown')}.",
                )
            elif current.get("status_after") == "recovered-original-output" and exact:
                evidence = exact[0]
                result = _result(
                    claim,
                    "context parser correction",
                    "correct measurement field now matches",
                    str(claim.get("value", "")),
                    str(evidence["actual"]),
                    f"{evidence['path']}:{evidence['line']}",
                    "The earlier matcher assigned this table column to the wrong output field; the corrected field map finds the exact value.",
                )
            else:
                raise ValueError(f"pre-triage mismatch was not explained by a current exact match: {claim_id}")
        else:
            result = _classify_one(claim, mismatch_by_claim[claim_id], by_report.get(report, []), record_by_path_line)
        result["mismatch_record_count"] = len(baseline_evidence)
        triage_rows.append(result)
        by_cause_claims[result["cause"]] += 1
        by_cause_records[result["cause"]] += len(baseline_evidence)
        by_cause_tiers[result["cause"]][result["tier"]] += 1
        cause_subcategory = (result["cause"], result["subcategory"])
        by_cause_subcategory_claims[cause_subcategory] += 1
        by_cause_subcategory_records[cause_subcategory] += len(baseline_evidence)
        report_cause = (result["report"], result["cause"])
        by_report_cause_claims[report_cause] += 1
        by_report_cause_records[report_cause] += len(baseline_evidence)
        claims_by_id[claim_id]["status_after"] = result["status_after"]
        claims_by_id[claim_id]["triage_cause"] = result["cause"]
        claims_by_id[claim_id]["triage_subcategory"] = result["subcategory"]
        claims_by_id[claim_id]["triage_rationale"] = result["rationale"]

    summary = {
        "classified_claims": len(triage_rows),
        "mismatch_records": len(baseline_rows),
        "mismatch_baseline_commit": BASELINE_MISMATCH_COMMIT,
        "causes": {
            cause: {
                "claims": by_cause_claims[cause],
                "mismatch_records": by_cause_records[cause],
                "headline": by_cause_tiers[cause]["headline"],
                "key": by_cause_tiers[cause]["key"],
                "supporting": by_cause_tiers[cause]["supporting"],
            }
            for cause in MISMATCH_CAUSES
        },
        "field_mapping_correction": {
            "claims": by_cause_claims["context parser correction"],
            "mismatch_records": by_cause_records["context parser correction"],
            "headline": by_cause_tiers["context parser correction"]["headline"],
            "key": by_cause_tiers["context parser correction"]["key"],
            "supporting": by_cause_tiers["context parser correction"]["supporting"],
        },
        "cause_subcategories": {
            f"{cause}: {subcategory}": {
                "claims": by_cause_subcategory_claims[(cause, subcategory)],
                "mismatch_records": by_cause_subcategory_records[(cause, subcategory)],
            }
            for cause, subcategory in sorted(by_cause_subcategory_claims)
        },
        "reports_by_cause": {
            f"{report}: {cause}": {
                "claims": by_report_cause_claims[(report, cause)],
                "mismatch_records": by_report_cause_records[(report, cause)],
            }
            for report, cause in sorted(by_report_cause_claims)
        },
        "m3_11": {
            "claims": sum(row["report"] == "M3_11_PROPAGATION.md" for row in triage_rows),
            "mismatch_records": sum(row["mismatch_record_count"] for row in triage_rows if row["report"] == "M3_11_PROPAGATION.md"),
            "claims_by_cause": dict(Counter(row["cause"] for row in triage_rows if row["report"] == "M3_11_PROPAGATION.md")),
            "records_by_field": dict(Counter(row["output_field"] for row in mapping["confirmed_mismatches"] if row["report"] == "M3_11_PROPAGATION.md")),
        },
        "context_parser_correction_records": sum(
            row["mismatch_record_count"] for row in triage_rows if row["cause"] == "context parser correction"
        ),
        "genuine_headline_key": [
            row for row in triage_rows if row["cause"] == "genuine discrepancy" and row["tier"] in {"headline", "key"}
        ],
        "genuine_other_count": sum(
            row["cause"] == "genuine discrepancy" and row["tier"] not in {"headline", "key"}
            for row in triage_rows
        ),
    }
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=[*TRIAGE_COLUMNS, "mismatch_record_count"], lineterminator="\n")
    writer.writeheader()
    writer.writerows(triage_rows)
    compressed = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=compressed, mtime=0, compresslevel=9) as stream:
        stream.write(buffer.getvalue().encode("utf-8"))
    generated_outputs = {
        root / TRIAGE_OUTPUT: compressed.getvalue(),
        root / TRIAGE_SUMMARY: (json.dumps(summary, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    }
    if write_outputs:
        for path, content in generated_outputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            if path.suffix == ".gz":
                path.write_bytes(content)
            else:
                write_text_lf(path, content.decode("utf-8"))
    mapping["mismatch_triage"] = summary
    if not write_outputs:
        mapping["_triage_outputs"] = generated_outputs
    return mapping
