"""Recreate a fixed-seed precision sample of the 144af62 value-only map."""

from __future__ import annotations

import csv
import gzip
import io
import json
import random
import re
import subprocess
from pathlib import Path
from typing import Any

from .claims import extract_claims
from .io import write_text_lf
from .recovered_claims import (
    ROOT,
    _add_text_values,
    _manifest_outputs,
    _match_claim_context,
    _report_aliases,
    _value_key,
)


BASELINE_COMMIT = "144af62cd266764f6be1b3b32211f13d5d125804"
SAMPLE_SEED = 20261024
SAMPLE_PATH = ROOT / "docs/reproducibility/recovered_claim_precision_audit.csv"
SUMMARY_PATH = ROOT / "docs/reproducibility/recovered_claim_precision_audit.json"
FIELDS = [
    "claim_id",
    "tier",
    "value_type",
    "report",
    "claim_value",
    "claim_text",
    "claim_context",
    "matched_output",
    "context_corresponds",
    "review_reason",
]


def _value_type(claim: dict[str, Any]) -> str:
    try:
        context = json.loads(str(claim.get("context", "")))
    except json.JSONDecodeError:
        context = {}
    label = str(context.get("label", ""))
    config_assignment = re.search(
        r"\b(?:n|k|p_local|seed|master_seed|replicate(?:_id)?|days?|horizon|threads?|chunk(?:_size)?|"
        r"samples?|warmups?|max_hops|transmit_probability)\s*[:=|]\s*$",
        label,
        re.IGNORECASE,
    )
    if context.get("column_is_parameter") or config_assignment:
        return "config/parameter"
    value = str(claim.get("value", ""))
    measured_context = (
        str(context.get("subfield", "")).casefold() in {"median", "mad"}
        or bool(re.search(r"\b(?:ratio|speedup|median|mad)\b|±", str(claim.get("claim text (short)", "")), re.IGNORECASE))
        or str(claim.get("unit", "")).casefold() in {"%", "x", "ratio", "µs", "ms", "ns"}
    )
    if "." in value or measured_context:
        return "measured value"
    if re.fullmatch(r"[+-]?\d+", value) and abs(int(value)) <= 1000:
        return "small integer"
    return "other"


def _read_baseline_map(root: Path) -> dict[str, Any]:
    raw = subprocess.check_output(
        ["git", "show", f"{BASELINE_COMMIT}:docs/reproducibility/recovered_claim_statuses.json.gz"],
        cwd=root,
    )
    return json.loads(gzip.decompress(raw))


def _old_line_value(line: str, token: str, record: dict[str, Any] | None) -> bool:
    if record is not None:
        return token in {value for values in record["measures"].values() for value, _raw in values}
    values: set[str] = set()
    _add_text_values(values, line)
    return token in values


def _first_value_line(
    claim: dict[str, Any], outputs: list[dict[str, Any]], root: Path
) -> tuple[dict[str, Any], dict[str, Any] | None]:
    token = _value_key(str(claim.get("value", "")), str(claim.get("unit", "")))
    stage = root / "docs/data/m5_recovered"
    for output in sorted(outputs, key=lambda item: str(item["path"])):
        if token not in output["values"]:
            continue
        records = {int(record["line"]): record for record in output["records"]}
        path = stage / Path(str(output["path"]))
        for line_number, line in enumerate(path.read_text(encoding="utf-8-sig", errors="replace").splitlines(), 1):
            record = records.get(line_number)
            if _old_line_value(line, token, record):
                return output, record
    raise ValueError(f"baseline value token has no source line: claim {claim['claim_id']}")


def _audit_row(claim: dict[str, Any], outputs: list[dict[str, Any]]) -> dict[str, Any]:
    exact, _mismatches = _match_claim_context(claim, outputs)
    paths = {str(output["path"]): output for output in outputs}
    if exact:
        evidence = sorted(exact, key=lambda item: (item["path"], item["line"]))[0]
        output = paths[str(evidence["path"])]
        record = next(record for record in output["records"] if int(record["line"]) == int(evidence["line"]))
        corresponds = True
        reason = "row identity, measure column, and value match"
        line = str(record["text"])
        line_number = int(record["line"])
    else:
        output, record = _first_value_line(claim, outputs, ROOT)
        line = ""
        line_number = 0
        source = ROOT / "docs/data/m5_recovered" / Path(str(output["path"]))
        line = source.read_text(encoding="utf-8-sig", errors="replace").splitlines()[0]
        # Find the displayed line again; the first line may only be a process header.
        token = _value_key(str(claim.get("value", "")), str(claim.get("unit", "")))
        by_line = {int(item["line"]): item for item in output["records"]}
        for line_number, candidate in enumerate(source.read_text(encoding="utf-8-sig", errors="replace").splitlines(), 1):
            if _old_line_value(candidate, token, by_line.get(line_number)):
                line = candidate
                record = by_line.get(line_number)
                break
        context = None
        try:
            context = json.loads(str(claim.get("context", "")))
        except json.JSONDecodeError:
            pass
        if not context or context.get("kind") != "table":
            reason = "claim has no structured table/config context"
        elif record is None:
            reason = "value-only output line is not a structured record"
        else:
            reason = "no output record matched all row/config dimensions and the measure column"
        corresponds = False
    return {
        "claim_id": int(claim["claim_id"]),
        "tier": str(claim.get("tier", "supporting")),
        "value_type": _value_type(claim),
        "report": Path(str(claim.get("report", ""))).name,
        "claim_value": str(claim.get("value", "")),
        "claim_text": str(claim.get("claim text (short)", "")),
        "claim_context": str(claim.get("context", "")),
        "matched_output": f"{output['path']}:{line_number}: {line}",
        "context_corresponds": "yes" if corresponds else "no",
        "review_reason": reason,
    }


def write_audit(root: Path = ROOT) -> dict[str, Any]:
    baseline = _read_baseline_map(root)
    recovered_ids = {
        int(row["claim_id"])
        for row in baseline["claims"]
        if row.get("status_after") == "recovered-original-output"
    }
    claims = extract_claims(root, apply_recovered=False)
    by_id = {int(claim["claim_id"]): claim for claim in claims}
    recovered = [by_id[claim_id] for claim_id in sorted(recovered_ids)]
    counts: dict[str, int] = {}
    for claim in recovered:
        kind = _value_type(claim)
        counts[kind] = counts.get(kind, 0) + 1
    measured = [claim for claim in recovered if _value_type(claim) == "measured value"]
    headline_key = [claim for claim in measured if claim.get("tier") in {"headline", "key"}]
    other = [claim for claim in measured if claim.get("tier") not in {"headline", "key"}]
    if len(headline_key) < 50 or len(other) < 100:
        raise ValueError("the recovered measured-claim pools are too small for the registered audit sample")
    rng = random.Random(SAMPLE_SEED)
    selected = rng.sample(sorted(headline_key, key=lambda row: int(row["claim_id"])), 50)
    selected += rng.sample(sorted(other, key=lambda row: int(row["claim_id"])), 100)

    _, outputs = _manifest_outputs(root)
    aliases = _report_aliases(root)
    by_report: dict[str, list[dict[str, Any]]] = {}
    for output in outputs:
        for report in output["reports_supported"]:
            name = Path(str(report)).name
            by_report.setdefault(aliases.get(name, name), []).append(output)
    rows = [
        _audit_row(claim, by_report.get(Path(str(claim["report"])).name, []))
        for claim in selected
    ]
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=FIELDS, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    write_text_lf(SAMPLE_PATH, buffer.getvalue())

    result = {
        "baseline_commit": BASELINE_COMMIT,
        "baseline_recovered_claims": len(recovered),
        "value_type_counts": counts,
        "measured_claims": len(measured),
        "headline_key_measured_pool": len(headline_key),
        "other_measured_pool": len(other),
        "seed": SAMPLE_SEED,
        "sample_headline_key": 50,
        "sample_other": 100,
        "sample_size": len(rows),
        "context_corresponds": sum(row["context_corresponds"] == "yes" for row in rows),
        "context_does_not_correspond": sum(row["context_corresponds"] == "no" for row in rows),
        "precision": sum(row["context_corresponds"] == "yes" for row in rows) / len(rows),
        "sample_path": SAMPLE_PATH.relative_to(root).as_posix(),
    }
    write_text_lf(SUMMARY_PATH, json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result


def main() -> int:
    print(json.dumps(write_audit(), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
