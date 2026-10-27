"""Pure M5 conclusion verdict calculation."""

from __future__ import annotations

import math
import re
from collections.abc import Iterable, Mapping, Sequence
from typing import Any


VERDICT_ORDER = (
    "reproduced",
    "reproduced (loose)",
    "not reproduced",
    "inconclusive",
    "unverified",
    "unverified (stale)",
    "needs-decision",
)
_VERDICT_RANK = {value: index for index, value in enumerate(VERDICT_ORDER)}
_STRENGTH = {
    "deterministic_exact": 6,
    "a5_report_revision": 5,
    "a5_head": 4,
    "structured_recovered_output": 3,
    "qualitative_source": 2,
    "historical_protocol": 1,
    "none": 0,
}
_ARCHIVAL = {"structured_recovered_output", "historical_protocol", "none"}
_SHA256 = re.compile(r"^[0-9a-f]{64}$")
_RUSTC_FIRST_LINE = re.compile(r"^rustc (?P<release>\S+) \((?P<short>[0-9a-f]+) (?P<date>\d{4}-\d{2}-\d{2})\)$")
_RUSTC_COMMIT = re.compile(r"^[0-9a-f]{40}$")
_THRESHOLD_NOISE_RULE = "Only valid cells are evaluated; comparisons within the frozen criteria noise bound are inconclusive."
_FROZEN_RATIO_NOISE_PCT = 20.0
_NORMALIZATIONS = {
    "m4_04_unbounded_ci_high",
    "m4_05_schema_v1_to_v2",
    "m4_05_guarantee_labels_v1_to_v2",
    "report_printed_precision",
}


def results_relevant_digest(files: Mapping[str, bytes]) -> str:
    """Hash a path-sorted set using the M5-00 NUL-delimited digest definition."""
    import hashlib

    payload = bytearray()
    for path in sorted(files, key=lambda value: value.encode("utf-8")):
        payload.extend(path.replace("\\", "/").encode("utf-8"))
        payload.append(0)
        payload.extend(hashlib.sha256(files[path]).hexdigest().encode("ascii"))
        payload.append(10)
    return hashlib.sha256(payload).hexdigest()


def _cells_by_id(record: Mapping[str, Any]) -> dict[str, Mapping[str, Any]]:
    raw = record.get("cell_results", [])
    if isinstance(raw, Mapping):
        return {str(key): value for key, value in raw.items() if isinstance(value, Mapping)}
    if isinstance(raw, list):
        return {
            str(item["cell_id"]): item
            for item in raw
            if isinstance(item, Mapping) and isinstance(item.get("cell_id"), str)
        }
    return {}


def _has_artifact_hashes(record: Mapping[str, Any]) -> bool:
    artifacts = record.get("artifacts")
    return isinstance(artifacts, list) and bool(artifacts) and all(
        isinstance(item, Mapping)
        and isinstance(item.get("path"), str)
        and bool(item["path"])
        and isinstance(item.get("sha256"), str)
        and _SHA256.fullmatch(item["sha256"]) is not None
        for item in artifacts
    )


def valid_repetitions_per_sample(value: Any) -> bool:
    if isinstance(value, int) and not isinstance(value, bool):
        return value >= 1
    return isinstance(value, Mapping) and bool(value) and all(
        isinstance(repetitions, int) and not isinstance(repetitions, bool) and repetitions >= 1
        for repetitions in value.values()
    )


def _compatible_kind(kind: str, conclusion_type: str, role: str) -> int | None:
    if kind in _ARCHIVAL:
        return None
    if kind == "deterministic_exact" and conclusion_type == "deterministic":
        return _STRENGTH[kind]
    if kind == "a5_report_revision" and conclusion_type == "timing" and role == "report":
        return _STRENGTH[kind]
    if kind == "a5_head" and conclusion_type == "timing" and role == "head":
        return _STRENGTH[kind]
    if kind == "qualitative_source" and conclusion_type == "qualitative":
        return _STRENGTH[kind]
    return -1


def _relation(value: float, threshold: float, relation: str) -> bool | None:
    return {
        ">": lambda: value > threshold,
        ">=": lambda: value >= threshold,
        "<": lambda: value < threshold,
        "<=": lambda: value <= threshold,
        "==": lambda: value == threshold,
    }.get(relation, lambda: None)()


def _normal(
    value: Any,
    normalization_id: str | None,
    fields: Any,
    key: str = "",
    *,
    search_complete: bool = False,
    apply_printed_precision: bool = True,
) -> Any:
    if normalization_id is None:
        return value
    if normalization_id not in _NORMALIZATIONS:
        return _Unknown
    if isinstance(value, Mapping):
        return {
            str(k): _normal(
                v,
                normalization_id,
                fields,
                str(k),
                search_complete=search_complete,
                apply_printed_precision=apply_printed_precision,
            )
            for k, v in value.items()
        }
    if isinstance(value, list):
        return [
            _normal(
                v,
                normalization_id,
                fields,
                key,
                search_complete=search_complete,
                apply_printed_precision=apply_printed_precision,
            )
            for v in value
        ]
    if normalization_id == "m4_04_unbounded_ci_high" and key in fields:
        if value is None or value == "inf" or value == "Infinity" or (
            isinstance(value, float) and math.isinf(value) and value > 0
        ):
            return "__positive_infinity__"
    if normalization_id == "m4_05_schema_v1_to_v2" and key in {"schema_version", "schema"}:
        if value in (1, "1", "v1", "version 1"):
            return 2
    if normalization_id == "m4_05_guarantee_labels_v1_to_v2":
        if value == "Minimum":
            return "minimum"
        if value == "OneMinimal":
            return "one_minimal"
        if value == "None":
            return "exhausted_no_cause" if search_complete else "incomplete"
    if normalization_id == "report_printed_precision" and apply_printed_precision and isinstance(fields, Mapping) and key in fields:
        places = fields[key]
        if not isinstance(places, int) or isinstance(places, bool) or places < 0:
            return _Unknown
        if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(float(value)):
            return round(float(value), places)
        return _Unknown
    return value


class _UnknownType:
    pass


_Unknown = _UnknownType()


def _observed_value(cell: Mapping[str, Any], name: str | None = None) -> Any:
    if name and name in cell:
        return cell[name]
    observed = cell.get("observed")
    if name and isinstance(observed, Mapping) and name in observed:
        return observed[name]
    return observed


def _predicate_result(
    conclusion: Mapping[str, Any],
    experiment: Mapping[str, Any],
    cell_id: str,
    cell_result: Mapping[str, Any],
) -> tuple[str, str]:
    kind = experiment.get("predicate_kind")
    params = experiment.get("predicate_parameters", {})
    if not isinstance(params, Mapping):
        return "needs-decision", "predicate parameters are not a mapping"

    if kind == "exact":
        expected_values = params.get("expected_values", {})
        if not isinstance(expected_values, Mapping) or cell_id not in expected_values:
            return "needs-decision", "exact predicate has no expected value for this cell"
        expected = expected_values[cell_id]
        if isinstance(expected, str):
            exact_match = cell_result.get("exact_match")
            if exact_match is True:
                return "reproduced", "evidence records an exact match to the named frozen oracle"
            if exact_match is False:
                return "not reproduced", "evidence records a mismatch to the named frozen oracle"
            return "needs-decision", "named-oracle evidence needs an exact_match result"
        observed = _observed_value(cell_result)
        normalization_id = cell_result.get("normalization_id")
        normalization_fields = cell_result.get("normalization_fields", [])
        if normalization_id is not None and normalization_id not in _NORMALIZATIONS:
            return "needs-decision", "normalization is not registered"
        search_complete = cell_result.get("search_complete") is True
        normalized_expected = _normal(
            expected,
            normalization_id,
            normalization_fields,
            search_complete=search_complete,
            apply_printed_precision=False,
        )
        normalized_observed = _normal(
            observed, normalization_id, normalization_fields, search_complete=search_complete
        )
        if normalized_expected is _Unknown or normalized_observed is _Unknown:
            return "needs-decision", "normalization is not registered"
        return (
            ("reproduced", "exact fields match")
            if normalized_expected == normalized_observed
            else ("not reproduced", "exact fields differ")
        )

    if kind == "ratio_band":
        values = params.get("historical_values", [])
        try:
            index = experiment.get("ordered_cell_ids", []).index(cell_id)
            old = float(values[index])
            observed = float(cell_result.get("observed_ratio", _observed_value(cell_result, "ratio")))
            lower, upper = (float(x) for x in params["deviation_bands_pct"])
        except (ValueError, TypeError, KeyError, IndexError):
            return "needs-decision", "ratio evidence or protocol parameters are incomplete"
        if old == 0 or observed <= 0 or lower < 0 or upper < lower:
            return "needs-decision", "ratio predicate is undefined for these values"
        deviation = abs(observed / old - 1.0) * 100.0
        if math.isclose(deviation, lower, rel_tol=1e-12, abs_tol=1e-12):
            deviation = lower
        elif math.isclose(deviation, upper, rel_tol=1e-12, abs_tol=1e-12):
            deviation = upper
        if deviation <= lower:
            return "reproduced", f"ratio deviation {deviation:.6g}% is within {lower:g}%"
        if deviation <= upper:
            return "reproduced (loose)", f"ratio deviation {deviation:.6g}% is within {upper:g}%"
        return "not reproduced", f"ratio deviation {deviation:.6g}% exceeds {upper:g}%"

    if kind == "threshold_side":
        values = params.get("historical_values", [])
        try:
            index = experiment.get("ordered_cell_ids", []).index(cell_id)
            old = float(values[index])
            observed = float(_observed_value(cell_result, "value"))
            threshold = float(params["threshold"])
            relation = str(params["relation"])
        except (ValueError, TypeError, KeyError, IndexError):
            return "needs-decision", "threshold evidence or protocol parameters are incomplete"
        noise = _threshold_noise_bound(params, threshold)
        if noise is None:
            return "needs-decision", "threshold noise rule is not a registered frozen rule"
        old_side = _relation(old, threshold, relation)
        new_side = _relation(observed, threshold, relation)
        if old_side is None or new_side is None or noise < 0:
            return "needs-decision", "threshold relation is not supported"
        if abs(old - threshold) <= noise:
            return "inconclusive", "historical value is within the recorded noise bound"
        if old_side == new_side:
            return "reproduced", "threshold side is preserved outside the noise bound"
        return "not reproduced", "threshold side changed outside the noise bound"

    if kind == "completion":
        completed = cell_result.get("completed", _observed_value(cell_result))
        error = cell_result.get("error")
        if completed is True and error in (None, False, ""):
            return "reproduced", "required completion occurred without runner error"
        if completed is False or error not in (None, False, ""):
            return "not reproduced", "runner failed or did not complete"
        return "needs-decision", "completion evidence is missing"

    if kind == "no_gain":
        try:
            effect = float(_observed_value(cell_result, str(params["paired_effect_field"])))
            mad = float(_observed_value(cell_result, str(params["paired_mad_field"])))
            relation = str(params["relation"])
        except (ValueError, TypeError, KeyError):
            return "needs-decision", "paired effect or MAD is missing"
        if mad < 0 or relation != "abs(effect) <= MAD":
            return "needs-decision", "no_gain uses only the frozen abs(effect) <= MAD predicate"
        if abs(effect) <= mad:
            return "reproduced", "paired effect is within its paired MAD"
        return "not reproduced", "paired effect exceeds its paired MAD"

    return "needs-decision", f"unknown predicate kind: {kind!r}"


def aggregate_verdict(cell_verdicts: Sequence[str]) -> str:
    """Apply the one frozen aggregate order for a conclusion's cells."""
    if not cell_verdicts:
        return "unverified"
    if any(v == "needs-decision" for v in cell_verdicts):
        return "needs-decision"
    if any(v == "not reproduced" for v in cell_verdicts):
        return "not reproduced"
    if all(v in {"unverified", "unverified (stale)"} for v in cell_verdicts):
        return "unverified (stale)" if "unverified (stale)" in cell_verdicts else "unverified"
    if any(v in {"inconclusive", "unverified", "unverified (stale)"} for v in cell_verdicts):
        return "inconclusive"
    if any(v == "reproduced (loose)" for v in cell_verdicts):
        return "reproduced (loose)"
    return "reproduced"


def _tie_verdicts(values: Sequence[str]) -> str:
    if not values:
        return "unverified"
    if "needs-decision" in values:
        return "needs-decision"
    passes = {"reproduced", "reproduced (loose)"}
    if passes.intersection(values) and "not reproduced" in values:
        return "inconclusive"
    if "reproduced" in values and "reproduced (loose)" in values:
        return "reproduced (loose)"
    return max(values, key=lambda value: _VERDICT_RANK.get(value, _VERDICT_RANK["needs-decision"]))


def _threshold_noise_bound(parameters: Mapping[str, Any], threshold: float) -> float | None:
    if parameters.get("noise_rule") != _THRESHOLD_NOISE_RULE:
        return None
    # The frozen criteria's outer ratio band supplies the threshold-side noise margin.
    return abs(threshold) * (_FROZEN_RATIO_NOISE_PCT / 100.0)


def _parse_rustc_vv(value: Any) -> dict[str, str] | None:
    if not isinstance(value, str):
        return None
    lines = value.splitlines()
    if not lines:
        return None
    match = _RUSTC_FIRST_LINE.fullmatch(lines[0].strip())
    fields: dict[str, str] = {}
    for line in lines[1:]:
        key, separator, field_value = line.partition(":")
        if separator:
            fields[key.strip()] = field_value.strip()
    commit = fields.get("commit-hash", "")
    commit_date = fields.get("commit-date", "")
    host = fields.get("host", "")
    release = fields.get("release", "")
    if (
        match is None
        or not _RUSTC_COMMIT.fullmatch(commit)
        or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", commit_date)
        or not host
        or match.group("release") != release
        or not commit.startswith(match.group("short"))
        or match.group("date") != commit_date
    ):
        return None
    return {
        "release": release,
        "commit_hash": commit,
        "commit_date": commit_date,
        "host": host,
        "first_line": lines[0].strip().removeprefix("rustc "),
    }


def _overlay_provenance_matches(record: Mapping[str, Any], experiment: Mapping[str, Any]) -> bool:
    expected = [
        {key: overlay[key] for key in ("path", "sha256", "origin_commit")}
        for overlay in experiment.get("harness_overlays", [])
        if isinstance(overlay, Mapping) and {"path", "sha256", "origin_commit"} <= overlay.keys()
    ]
    actual = record.get("harness_overlays", [])
    if actual is None:
        actual = []
    if not isinstance(actual, list):
        return False
    normalized = []
    for overlay in actual:
        if not isinstance(overlay, Mapping) or not {"path", "sha256", "origin_commit"} <= overlay.keys():
            return False
        normalized.append({key: overlay[key] for key in ("path", "sha256", "origin_commit")})
    return normalized == expected


def _supersession_state(records: Sequence[Mapping[str, Any]]) -> tuple[set[str], dict[tuple[str, str], str]]:
    by_id = {str(record.get("id", "")): record for record in records}
    targets: set[str] = set()
    errors: dict[tuple[str, str], str] = {}
    graph: dict[str, list[str]] = {}
    for record in records:
        identifier = str(record.get("id", ""))
        refs = record.get("supersedes", [])
        if isinstance(refs, str):
            refs = [refs]
        if not isinstance(refs, list):
            errors[(str(record.get("conclusion_id", "")), str(record.get("role", "")))] = "supersedes must be a list of evidence IDs"
            continue
        graph[identifier] = [str(ref) for ref in refs]
        for ref in refs:
            target = by_id.get(ref)
            key = (str(record.get("conclusion_id", "")), str(record.get("role", "")))
            if target is None:
                errors[key] = f"supersedes target does not exist: {ref}"
                continue
            if target.get("conclusion_id") != record.get("conclusion_id") or target.get("role") != record.get("role"):
                errors[key] = f"supersedes crosses conclusion or role: {ref}"
            if _STRENGTH.get(str(record.get("kind")), -1) < _STRENGTH.get(str(target.get("kind")), -1):
                errors[key] = f"weaker evidence cannot supersede {ref}"
            targets.add(ref)
    done: set[str] = set()
    visiting: set[str] = set()

    def visit(identifier: str) -> None:
        if identifier in visiting:
            record = by_id[identifier]
            errors[(str(record.get("conclusion_id", "")), str(record.get("role", "")))] = "supersession graph contains a cycle"
            return
        if identifier in done:
            return
        visiting.add(identifier)
        for target_id in graph.get(identifier, []):
            if target_id in graph:
                visit(target_id)
        visiting.remove(identifier)
        done.add(identifier)

    for identifier in graph:
        visit(identifier)
    # Superseded records also remove their superseded ancestors from the active set.
    pending = list(targets)
    while pending:
        current = pending.pop()
        for ancestor in graph.get(current, []):
            if ancestor not in targets:
                targets.add(ancestor)
                pending.append(ancestor)
    return targets, errors


def _timing_host_matches(record: Mapping[str, Any], reference: Mapping[str, Any]) -> bool:
    toolchain = record.get("toolchain")
    if not isinstance(toolchain, Mapping):
        return False
    if toolchain.get("cpu_model") != reference.get("cpu_model"):
        return False
    if toolchain.get("physical_cores") != reference.get("physical_cores"):
        return False
    if toolchain.get("logical_cores") != reference.get("logical_cores"):
        return False
    rustc = _parse_rustc_vv(toolchain.get("rustc_vv"))
    if rustc is None:
        return False
    if (
        rustc["release"] != reference.get("rustc")
        or rustc["commit_hash"] != reference.get("rustc_commit_hash")
        or rustc["commit_date"] != reference.get("rustc_commit_date")
        or rustc["host"] != reference.get("rustc_host")
        or toolchain.get("rustc") != rustc["first_line"]
    ):
        return False
    if toolchain.get("rust_target", toolchain.get("target")) != reference.get("rust_target"):
        return False
    if toolchain.get("a5_settings", toolchain.get("a5")) != reference.get("a5"):
        return False
    return True


def _a5_cell_validity(cell: Mapping[str, Any], settings: Mapping[str, Any]) -> tuple[bool, str]:
    if cell.get("valid") is False:
        return False, "recorded cell validity is false"
    validity = cell.get("validity")
    if not isinstance(validity, Mapping):
        return False, "A-5 validity measurements are missing"
    warmups = settings.get("warmups")
    sample_count = settings.get("samples")
    if validity.get("warmups") != warmups or validity.get("sample_count") != sample_count:
        return False, "A-5 warmup or sample count differs from the registered host protocol"
    repetitions = validity.get("repetitions_per_sample")
    if not valid_repetitions_per_sample(repetitions):
        return False, "A-5 repetition factor is missing or invalid"
    sample_seconds = validity.get("sample_seconds")
    minimum = settings.get("minimum_sample_seconds")
    if (
        not isinstance(sample_seconds, list)
        or len(sample_seconds) != sample_count
        or not isinstance(minimum, (int, float))
        or not all(isinstance(value, (int, float)) and math.isfinite(value) for value in sample_seconds)
        or any(value < float(minimum) for value in sample_seconds)
    ):
        return False, "one or more A-5 samples are below the registered minimum duration"
    spreads = validity.get("path_mad_over_median")
    maximum = settings.get("max_path_mad_over_median")
    if (
        not isinstance(spreads, Mapping)
        or not spreads
        or not isinstance(maximum, (int, float))
        or not all(isinstance(value, (int, float)) and math.isfinite(value) and value >= 0 for value in spreads.values())
    ):
        return False, "A-5 per-path MAD/median measurements are missing or invalid"
    if any(float(value) > float(maximum) for value in spreads.values()):
        return False, "an A-5 path MAD/median exceeds the registered validity limit"
    return True, "A-5 sample count, duration floor, and per-path MAD/median are valid"


def _record_is_stale(
    record: Mapping[str, Any],
    current_head_sha: str,
    current_results_digest: str | None,
) -> bool:
    if record.get("role") != "head":
        return False
    recorded_digest = record.get("results_relevant_digest")
    if not isinstance(recorded_digest, str) or current_results_digest is None:
        return True
    # A result recorded at current_head_sha still has to carry the frozen digest.
    return recorded_digest != current_results_digest


def compute_verdicts(
    registry: Mapping[str, Any],
    experiments: Mapping[str, Any],
    hosts: Mapping[str, Any],
    evidence_records: Sequence[Mapping[str, Any]],
    *,
    roles: Iterable[str] = ("report",),
    head_sha: str | None = None,
    current_results_digests: Mapping[str, str] | None = None,
) -> list[dict[str, Any]]:
    """Compute report-role and requested SHA-keyed head verdicts without I/O."""
    conclusions = registry.get("conclusion", [])
    experiment_by_id = {str(item["id"]): item for item in experiments.get("experiment", [])}
    host_records = hosts.get("host", [])
    reference_hosts = [item for item in host_records if item.get("reference") is True]
    reference = reference_hosts[0] if len(reference_hosts) == 1 else {}
    requested_roles = tuple(roles)
    head_digests = current_results_digests or {}
    superseded, supersession_errors = _supersession_state(evidence_records)
    output: list[dict[str, Any]] = []

    for conclusion in conclusions:
        conclusion_id = str(conclusion.get("id", ""))
        experiment = experiment_by_id.get(str(conclusion.get("experiment", "")), {})
        cells = [str(value) for value in experiment.get("ordered_cell_ids", [])]
        if experiment.get("status") == "incomplete" or not cells:
            cells = []
        for role in requested_roles:
            if role not in {"report", "head"} or (role == "head" and head_sha is None):
                continue
            revision = str(conclusion.get("report_revision", "")) if role == "report" else str(head_sha)
            cell_rows: list[dict[str, Any]] = []
            supersession_error = supersession_errors.get((conclusion_id, role))
            if not cells:
                verdict = "unverified"
            elif supersession_error:
                cell_rows = [
                    {"cell_id": cell, "verdict": "needs-decision", "evidence_ids": [], "reason": supersession_error}
                    for cell in cells
                ]
                verdict = "needs-decision"
            else:
                for cell_id in cells:
                    candidates: list[tuple[int, str, str, Any]] = []
                    validity_failures = 0
                    stale_seen = False
                    hard_needs: list[tuple[int, str, str]] = []
                    for record in evidence_records:
                        if record.get("id") in superseded:
                            continue
                        if record.get("conclusion_id") != conclusion_id or record.get("role") != role:
                            continue
                        kind = str(record.get("kind", ""))
                        if kind in _ARCHIVAL:
                            continue
                        result = _cells_by_id(record).get(cell_id)
                        if result is None:
                            continue
                        if not _has_artifact_hashes(record):
                            continue
                        strength = _compatible_kind(kind, str(conclusion.get("type", "")), role)
                        if strength is None:
                            continue
                        if strength < 0:
                            hard_needs.append((100, "needs-decision", f"evidence kind {kind!r} is incompatible with {conclusion.get('type')} / {role}"))
                            continue
                        if record.get("protocol_id") != experiment.get("id"):
                            hard_needs.append((100, "needs-decision", "evidence protocol_id does not match the registered experiment"))
                            continue
                        if role == "report" and record.get("revision") != conclusion.get("report_revision"):
                            hard_needs.append((100, "needs-decision", "report-role evidence is recorded at the wrong report revision"))
                            continue
                        if role == "head" and _record_is_stale(record, revision, head_digests.get(conclusion_id)):
                            stale_seen = True
                            continue
                        if not _overlay_provenance_matches(record, experiment):
                            hard_needs.append((100, "needs-decision", "evidence overlay hashes differ from the registered protocol"))
                            continue
                        if record.get("overlay_build_ok") is False:
                            hard_needs.append((100, "needs-decision", "registered harness overlay does not build against the report-revision API"))
                            continue
                        if experiment.get("harness_overlays") and record.get("overlay_build_ok") is not True:
                            hard_needs.append((100, "needs-decision", "overlay build result is missing"))
                            continue
                        if kind in {"a5_report_revision", "a5_head"} and experiment.get("predicate_kind") != "completion":
                            if not _timing_host_matches(record, reference):
                                validity_failures += 1
                                continue
                        if result.get("overlay_build_ok") is False:
                            hard_needs.append((100, "needs-decision", "registered harness overlay does not build against the report-revision API"))
                            continue
                        if kind in {"a5_report_revision", "a5_head"} and experiment.get("predicate_kind") != "completion":
                            is_valid, validity_reason = _a5_cell_validity(result, reference.get("a5", {}))
                            if not is_valid:
                                validity_failures += 1
                                continue
                        cell_verdict, reason = _predicate_result(conclusion, experiment, cell_id, result)
                        candidates.append((strength, cell_verdict, str(record.get("id", "")), reason))
                    if hard_needs:
                        selected = _tie_verdicts([item[1] for item in hard_needs])
                        reason = "; ".join(item[2] for item in hard_needs)
                        cell_rows.append({"cell_id": cell_id, "verdict": selected, "evidence_ids": [], "reason": reason})
                    elif not candidates:
                        if validity_failures:
                            cell_verdict, reason = "inconclusive", "all covering records failed protocol validity or reference-host checks"
                        elif stale_seen:
                            cell_verdict, reason = "unverified (stale)", "head evidence results-relevant digest differs from this head SHA"
                        else:
                            cell_verdict, reason = "unverified", "no accepted evidence covers this cell"
                        cell_rows.append({"cell_id": cell_id, "verdict": cell_verdict, "evidence_ids": [], "reason": reason})
                    else:
                        best_strength = max(item[0] for item in candidates)
                        best = [item for item in candidates if item[0] == best_strength]
                        cell_verdict = _tie_verdicts([item[1] for item in best])
                        cell_rows.append({
                            "cell_id": cell_id,
                            "verdict": cell_verdict,
                            "evidence_ids": sorted(item[2] for item in best),
                            "reason": "; ".join(sorted({str(item[3]) for item in best})),
                        })
                verdict = aggregate_verdict([item["verdict"] for item in cell_rows])
            output.append({
                "conclusion_id": conclusion_id,
                "role": role,
                "revision": revision,
                "verdict": verdict,
                "cells": cell_rows,
            })
    return output
