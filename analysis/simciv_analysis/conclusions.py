"""Validate M5 conclusion/evidence records and render the verdict table."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from datetime import date
from pathlib import Path, PurePosixPath
from typing import Any

from .git_hooks import dvc_output_paths
from .io import write_text_lf
from .verdicts import (
    _STRENGTH,
    _parse_rustc_vv,
    _supersession_state,
    compute_verdicts,
    valid_repetitions_per_sample,
)


ROOT = Path(__file__).resolve().parents[2]
CONCLUSIONS = ROOT / "contracts" / "conclusions.toml"
EXPERIMENTS = ROOT / "contracts" / "experiments.toml"
HOSTS = ROOT / "contracts" / "hosts.toml"
EVIDENCE = ROOT / "contracts" / "evidence.jsonl"
OUTPUT = ROOT / "docs" / "reproducibility" / "VERDICTS.md"
HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
FULL_SHA1 = re.compile(r"^[0-9a-f]{40}$")
PREDICATE_KEYS = {
    "exact": {"fields", "expected_values", "comparison"},
    "ratio_band": {"historical_values", "ratio_direction", "deviation_bands_pct"},
    "threshold_side": {"metric", "historical_values", "threshold", "relation", "noise_rule"},
    "completion": {"required_result", "error_policy"},
    "no_gain": {"paired_effect_field", "paired_mad_field", "relation"},
}
NORMALIZATIONS = {
    "m4_04_unbounded_ci_high",
    "m4_05_schema_v1_to_v2",
    "m4_05_guarantee_labels_v1_to_v2",
    "report_printed_precision",
}
EVIDENCE_KINDS = set(_STRENGTH)
TOOLCHAIN_FIELDS = {
    "rustc",
    "rust_target",
    "cargo_lock_sha256",
    "python",
    "uv",
    "analysis_lock_sha256",
    "os_family",
    "os_build",
    "cpu_model",
    "physical_cores",
    "logical_cores",
    "external_tools",
    "a5_settings",
}


def _slug(heading: str) -> str:
    heading = re.sub(r"<[^>]+>", "", heading).strip().lower().replace("`", "")
    heading = re.sub(r"[^a-z0-9 _-]", "", heading)
    return re.sub(r"\s+", "-", heading).strip("-")


def anchor_section(text: str, anchor: str) -> str | None:
    """Return a heading section including nested subsections."""
    lines = text.splitlines()
    start: int | None = None
    level: int | None = None
    for index, line in enumerate(lines):
        match = re.match(r"^(#{1,6})\s+(.*)$", line)
        if match and _slug(match.group(2)) == anchor:
            start, level = index, len(match.group(1))
            break
    if start is None:
        explicit = re.search(rf'<a\s+id=["\']{re.escape(anchor)}["\']\s*></a>', text, re.I)
        if explicit:
            first_heading = text[: explicit.start()].count("\n") + 1
            for index in range(first_heading, len(lines)):
                match = re.match(r"^(#{1,6})\s+", lines[index])
                if match:
                    start, level = index, len(match.group(1))
                    break
    if start is None or level is None:
        return None
    end = len(lines)
    for index in range(start + 1, len(lines)):
        match = re.match(r"^(#{1,6})\s+", lines[index])
        if match and len(match.group(1)) <= level:
            end = index
            break
    return "\n".join(lines[start:end])


def validate_anchor_content(text: str, parent_text: str | None, anchor: str, verbatim: str) -> list[str]:
    errors: list[str] = []
    section = anchor_section(text, anchor)
    if section is None:
        errors.append(f"anchor not found: {anchor}")
    elif verbatim not in section:
        errors.append(f"verbatim is outside anchor section: {anchor}")
    if verbatim not in text:
        errors.append("verbatim not found in report revision")
    if parent_text is not None and verbatim in parent_text:
        errors.append("verbatim already exists in the report revision's parent")
    return errors


def validate_evidence_append_only(base: bytes, current: bytes) -> None:
    if not base:
        return
    if not base.endswith(b"\n"):
        raise ValueError("base evidence file does not end at a JSONL record boundary")
    if not current.startswith(base):
        raise ValueError("contracts/evidence.jsonl rewrites or removes an existing base line")


def _git(root: Path, *args: str, input_bytes: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        ["git", *args], cwd=root, input=input_bytes, capture_output=True, check=False
    )


def _git_text(root: Path, *args: str) -> str | None:
    result = _git(root, *args)
    if result.returncode:
        return None
    return result.stdout.decode("utf-8")


def _valid_commit(root: Path, revision: str) -> bool:
    return bool(FULL_SHA1.fullmatch(revision)) and _git(root, "cat-file", "-e", f"{revision}^{{commit}}").returncode == 0


def _repo_file(root: Path, value: str) -> Path | None:
    path = PurePosixPath(value.replace("\\", "/"))
    if path.is_absolute() or ".." in path.parts:
        return None
    candidate = root.joinpath(*path.parts).resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError:
        return None
    return candidate


def _read_jsonl(path: Path) -> tuple[list[dict[str, Any]], list[str]]:
    if not path.exists():
        return [], [f"required evidence file is missing: {path.as_posix()}"]
    records: list[dict[str, Any]] = []
    errors: list[str] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            errors.append(f"evidence line {line_number} is blank; JSONL records must be contiguous")
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            errors.append(f"evidence line {line_number} is invalid JSON: {error}")
            continue
        if not isinstance(record, dict):
            errors.append(f"evidence line {line_number} must be a JSON object")
            continue
        records.append(record)
    return records, errors


def load_evidence(path: Path = EVIDENCE) -> tuple[list[dict[str, Any]], list[str]]:
    records, errors = _read_jsonl(path)
    identifiers: set[str] = set()
    for index, record in enumerate(records, start=1):
        identifier = record.get("id")
        if not isinstance(identifier, str) or not identifier:
            errors.append(f"evidence record {index} has no non-empty id")
        elif identifier in identifiers:
            errors.append(f"duplicate evidence id: {identifier}")
        else:
            identifiers.add(identifier)
    return records, errors


def _protocol_errors(conclusion: dict[str, Any], experiment: dict[str, Any], root: Path) -> list[str]:
    errors: list[str] = []
    label = str(conclusion.get("id", "<unknown>"))
    tier = conclusion.get("tier")
    status = experiment.get("status")
    if tier == 1 and (status != "complete"):
        errors.append(f"Tier 1 protocol must be complete: {experiment.get('id')}")
    if status == "incomplete":
        return errors

    kind = experiment.get("predicate_kind")
    parameters = experiment.get("predicate_parameters")
    if kind not in PREDICATE_KEYS:
        errors.append(f"unknown predicate kind for {label}: {kind!r}")
    elif not isinstance(parameters, dict) or set(parameters) != PREDICATE_KEYS[kind]:
        errors.append(f"predicate parameters for {label} must contain exactly {sorted(PREDICATE_KEYS.get(kind, set()))}")

    cells = experiment.get("ordered_cell_ids")
    if not isinstance(cells, list) or not cells or not all(isinstance(cell, str) and cell for cell in cells):
        errors.append(f"protocol has no ordered cell IDs: {experiment.get('id')}")
        cells = []
    elif len(cells) != len(set(cells)):
        errors.append(f"protocol has duplicate ordered cell IDs: {experiment.get('id')}")
    if tier == 1 and not isinstance(experiment.get("scope"), str):
        errors.append(f"Tier 1 protocol has no scope: {experiment.get('id')}")
    if tier == 1 and not isinstance(experiment.get("host_id"), str):
        errors.append(f"Tier 1 protocol has no host_id: {experiment.get('id')}")

    if kind in {"ratio_band", "threshold_side"} and isinstance(parameters, dict) and cells:
        values = parameters.get("historical_values")
        if not isinstance(values, list) or len(values) != len(cells) or not all(
            isinstance(value, (int, float)) and not isinstance(value, bool) for value in values
        ):
            errors.append(f"numeric historical_values must cover ordered cells for {experiment.get('id')}")
    if kind == "ratio_band" and isinstance(parameters, dict):
        bands = parameters.get("deviation_bands_pct")
        if bands != [10, 20]:
            errors.append(f"ratio_band deviation bands must be [10, 20]: {experiment.get('id')}")
    if kind == "threshold_side" and isinstance(parameters, dict):
        if not isinstance(parameters.get("threshold"), (int, float)) or parameters.get("relation") not in {"<", "<=", ">", ">=", "=="}:
            errors.append(f"threshold_side requires a numeric threshold and supported relation: {experiment.get('id')}")
    if kind == "exact" and isinstance(parameters, dict):
        expected = parameters.get("expected_values")
        if not isinstance(expected, dict) or set(expected) != set(cells):
            errors.append(f"exact expected_values must cover exactly the ordered cells: {experiment.get('id')}")
    if kind == "completion" and isinstance(parameters, dict):
        if not all(isinstance(parameters.get(key), str) and parameters[key] for key in ("required_result", "error_policy")):
            errors.append(f"completion protocol needs required_result and error_policy: {experiment.get('id')}")
    if kind == "no_gain" and isinstance(parameters, dict):
        if parameters.get("relation") != "abs(effect) <= MAD":
            errors.append(f"no_gain relation must be abs(effect) <= MAD: {experiment.get('id')}")

    harnesses = experiment.get("harness_paths")
    if not isinstance(harnesses, list) or not harnesses:
        errors.append(f"protocol has no harness_paths: {experiment.get('id')}")
        harnesses = []
    for harness in harnesses:
        path = _repo_file(root, str(harness))
        if path is None or not path.is_file():
            errors.append(f"harness path is missing or escapes repository for {experiment.get('id')}: {harness}")

    overlays = experiment.get("harness_overlays", [])
    if not isinstance(overlays, list):
        errors.append(f"harness_overlays must be an array for {experiment.get('id')}")
        overlays = []
    for overlay in overlays:
        if not isinstance(overlay, dict) or not {"path", "sha256", "origin_commit"} <= overlay.keys():
            errors.append(f"overlay needs path, sha256, and origin_commit: {experiment.get('id')}")
            continue
        path = _repo_file(root, str(overlay["path"]))
        if path is None:
            errors.append(f"overlay path is missing: {overlay['path']}")
            continue
        origin = str(overlay["origin_commit"])
        blob = _git(root, "show", f"{origin}:{overlay['path']}")
        if blob.returncode or hashlib.sha256(blob.stdout).hexdigest() != overlay["sha256"]:
            errors.append(f"overlay origin blob SHA mismatch: {overlay['path']} at {origin}")

    sampling = experiment.get("sampling")
    if conclusion.get("type") == "timing" and tier == 1:
        if not isinstance(sampling, dict):
            errors.append(f"Tier 1 timing protocol lacks A-5 sampling: {experiment.get('id')}")
        elif sampling.get("reference_host_id") != experiment.get("host_id"):
            errors.append(f"Tier 1 timing protocol host differs from A-5 host: {experiment.get('id')}")
    return errors


def _toolchain_errors(record: dict[str, Any], label: str) -> list[str]:
    toolchain = record.get("toolchain")
    if not isinstance(toolchain, dict):
        return [f"evidence toolchain must be an object: {label}"]
    fields = {
        "rustc", "rust_target", "cargo_lock_sha256", "python", "uv",
        "analysis_lock_sha256", "os_family", "os_build", "cpu_model",
        "physical_cores", "logical_cores", "external_tools", "a5_settings",
    }
    missing = fields - toolchain.keys()
    if missing:
        return [f"evidence toolchain omits explicit fields {sorted(missing)}: {label}"]
    errors = []
    for key in ("cargo_lock_sha256", "analysis_lock_sha256"):
        value = toolchain.get(key)
        if value is not None and (not isinstance(value, str) or not HEX_SHA256.fullmatch(value)):
            errors.append(f"evidence toolchain {key} is not a SHA-256 or null: {label}")
    return errors


def validate_artifact_hashes(record: dict[str, Any], root: Path = ROOT) -> list[str]:
    errors: list[str] = []
    artifacts = record.get("artifacts")
    label = str(record.get("id", "<missing>"))
    if not isinstance(artifacts, list) or not artifacts:
        return [f"evidence needs at least one hashed artifact: {label}"]
    for artifact in artifacts:
        if not isinstance(artifact, dict) or not isinstance(artifact.get("path"), str) or not HEX_SHA256.fullmatch(str(artifact.get("sha256", ""))):
            errors.append(f"evidence artifact needs a repository path and SHA-256: {label}")
            continue
        path = _repo_file(root, artifact["path"])
        if path is None:
            errors.append(f"evidence artifact is missing: {label}: {artifact['path']}")
            continue
        if path.is_file():
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
        elif artifact["path"] in dvc_output_paths(root):
            manifest = root / "docs" / "data" / "SHA256SUMS"
            entries = {}
            if manifest.is_file():
                for line in manifest.read_text(encoding="utf-8").splitlines():
                    digest, separator, relative = line.partition("  ")
                    if separator and len(digest) == 64:
                        entries[relative] = digest
            actual = entries.get(artifact["path"])
            if actual is None and artifact["path"] in dvc_output_paths(root):
                prefix = artifact["path"].rstrip("/") + "/"
                members = sorted((name, digest) for name, digest in entries.items() if name.startswith(prefix))
                if members:
                    digest = hashlib.sha256()
                    for name, content_hash in members:
                        digest.update(name.encode("utf-8"))
                        digest.update(b"\0")
                        digest.update(content_hash.encode("ascii"))
                        digest.update(b"\n")
                    actual = digest.hexdigest()
            if actual is None:
                errors.append(f"DVC artifact is absent from docs/data/SHA256SUMS: {label}: {artifact['path']}")
                continue
        else:
            errors.append(f"evidence artifact is missing: {label}: {artifact['path']}")
            continue
        if actual != artifact["sha256"]:
            prefix = "DVC SHA256SUMS mismatch" if not path.is_file() else "SHA-256 mismatch"
            errors.append(f"evidence artifact {prefix}: {label}: {artifact['path']}")
    return errors


def _protocol_for_evidence(experiment: dict[str, Any], evidence_id: str) -> dict[str, Any]:
    for history in experiment.get("protocol_history", []):
        if evidence_id in history.get("superseded_evidence_ids", []):
            return {
                **experiment,
                "ordered_cell_ids": history["prior_ordered_cell_ids"],
                "harness_paths": history.get("prior_harness_paths", experiment.get("harness_paths", [])),
                "harness_overlays": history.get("prior_harness_overlays", []),
            }
    return experiment


def _evidence_errors(
    records: list[dict[str, Any]],
    conclusions: dict[str, dict[str, Any]],
    experiments: dict[str, dict[str, Any]],
    hosts_doc: dict[str, Any],
    root: Path,
) -> list[str]:
    errors: list[str] = []
    ids = {str(record.get("id", "")): record for record in records}
    host_by_id = {str(host.get("id", "")): host for host in hosts_doc.get("host", []) if isinstance(host, dict)}
    for record in records:
        identifier = str(record.get("id", "<missing>"))
        cid = str(record.get("conclusion_id", ""))
        conclusion = conclusions.get(cid)
        if conclusion is None:
            errors.append(f"evidence references unknown conclusion: {identifier} -> {cid}")
            continue
        role = record.get("role")
        if role not in {"report", "head"}:
            errors.append(f"evidence role must be report or head: {identifier}")
        kind = record.get("kind")
        if kind not in EVIDENCE_KINDS:
            errors.append(f"unknown evidence kind: {identifier}")
        elif kind not in {"none", "historical_protocol", "structured_recovered_output"}:
            compatible = (
                (kind == "deterministic_exact" and conclusion.get("type") == "deterministic")
                or (kind == "a5_report_revision" and conclusion.get("type") == "timing" and role == "report")
                or (kind == "a5_head" and conclusion.get("type") == "timing" and role == "head")
                or (kind == "qualitative_source" and conclusion.get("type") == "qualitative")
            )
            if not compatible:
                errors.append(f"evidence kind/type/role combination is not accepted: {identifier}")
        if not FULL_SHA1.fullmatch(str(record.get("revision", ""))) or not _valid_commit(root, str(record.get("revision", ""))):
            errors.append(f"evidence revision is not an available full commit SHA: {identifier}")
        if not isinstance(record.get("protocol_id"), str) or record.get("protocol_id") != conclusion.get("experiment"):
            errors.append(f"evidence protocol_id does not match conclusion experiment: {identifier}")
        if not isinstance(record.get("date"), str):
            errors.append(f"evidence date is required: {identifier}")
        else:
            try:
                date.fromisoformat(record["date"])
            except ValueError:
                errors.append(f"evidence date must be ISO format: {identifier}")
        errors.extend(_toolchain_errors(record, identifier))
        if record.get("kind") in {"a5_report_revision", "a5_head"}:
            identity = _parse_rustc_vv(record.get("toolchain", {}).get("rustc_vv") if isinstance(record.get("toolchain"), dict) else None)
            if identity is None:
                errors.append(f"timing evidence requires a valid rustc -Vv capture: {identifier}")
            elif record.get("toolchain", {}).get("rustc") != identity["first_line"]:
                errors.append(f"timing evidence rustc label differs from rustc -Vv: {identifier}")
        if role == "report" and record.get("revision") != conclusion.get("report_revision"):
            errors.append(f"report-role evidence revision must equal report_revision: {identifier}")
        if role == "head" and not HEX_SHA256.fullmatch(str(record.get("results_relevant_digest", ""))):
            errors.append(f"head evidence requires a results_relevant_digest: {identifier}")
        errors.extend(validate_artifact_hashes(record, root))

        experiment = experiments.get(str(conclusion.get("experiment", "")), {})
        evidence_protocol = _protocol_for_evidence(experiment, identifier)
        results = record.get("cell_results", [])
        if not isinstance(results, list):
            errors.append(f"cell_results must be an array: {identifier}")
            continue
        seen_cells: set[str] = set()
        allowed_cells = set(evidence_protocol.get("ordered_cell_ids", []))
        for result in results:
            if not isinstance(result, dict) or result.get("cell_id") not in allowed_cells:
                errors.append(f"evidence has unknown cell result: {identifier}")
                continue
            cell_id = str(result["cell_id"])
            if cell_id in seen_cells:
                errors.append(f"evidence repeats a cell result: {identifier}: {cell_id}")
            seen_cells.add(cell_id)
            if result.get("normalization_id") is not None and result.get("normalization_id") not in NORMALIZATIONS:
                errors.append(f"evidence uses an unregistered normalization: {identifier}: {cell_id}")
            if result.get("normalization_id") == "report_printed_precision":
                precision = result.get("normalization_fields")
                if (
                    not isinstance(precision, dict)
                    or not precision
                    or any(not isinstance(value, int) or isinstance(value, bool) or value < 0 for value in precision.values())
                ):
                    errors.append(f"report printed-precision normalization needs nonnegative decimal places: {identifier}: {cell_id}")
            if (
                record.get("kind") in {"a5_report_revision", "a5_head"}
                and experiment.get("predicate_kind") != "completion"
                and record.get("overlay_build_ok") is not False
            ):
                validity = result.get("validity")
                if not isinstance(validity, dict):
                    errors.append(f"timing evidence cell needs A-5 validity measurements: {identifier}: {cell_id}")
                else:
                    if not isinstance(validity.get("warmups"), int) or not isinstance(validity.get("sample_count"), int):
                        errors.append(f"A-5 validity needs warmup and sample counts: {identifier}: {cell_id}")
                    if not valid_repetitions_per_sample(validity.get("repetitions_per_sample")):
                        errors.append(f"A-5 validity needs a positive repetition factor per measured path: {identifier}: {cell_id}")
                    seconds = validity.get("sample_seconds")
                    if not isinstance(seconds, list) or not all(
                        isinstance(value, (int, float)) and not isinstance(value, bool) for value in seconds
                    ):
                        errors.append(f"A-5 validity needs per-sample durations: {identifier}: {cell_id}")
                    spreads = validity.get("path_mad_over_median")
                    if not isinstance(spreads, dict) or len(spreads) != 2 or not all(
                        isinstance(value, (int, float)) and not isinstance(value, bool) for value in spreads.values()
                    ):
                        errors.append(f"A-5 validity needs two numeric path MAD/median values: {identifier}: {cell_id}")
        if evidence_protocol.get("harness_overlays") or record.get("harness_overlays"):
            expected = [
                {key: overlay[key] for key in ("path", "sha256", "origin_commit")}
                for overlay in evidence_protocol.get("harness_overlays", [])
            ]
            actual = record.get("harness_overlays", [])
            if actual != expected:
                errors.append(f"evidence overlay provenance differs from protocol: {identifier}")
            if evidence_protocol.get("harness_overlays") and not isinstance(record.get("overlay_build_ok"), bool):
                errors.append(f"report evidence must record overlay_build_ok: {identifier}")

    # A duplicate identifier is reported by load_evidence; preserving this map here
    # makes supersession validation deterministic even for malformed input.
    _, graph_errors = _supersession_state(records)
    errors.extend(f"invalid supersession graph for {cid}/{role}: {message}" for (cid, role), message in sorted(graph_errors.items()))
    missing_targets = set()
    for record in records:
        refs = record.get("supersedes", [])
        if isinstance(refs, str):
            refs = [refs]
        if isinstance(refs, list):
            missing_targets.update(str(ref) for ref in refs if str(ref) not in ids)
    errors.extend(f"supersession target does not exist: {target}" for target in sorted(missing_targets))
    return errors


def _conclusion_errors(
    registry: dict[str, Any], experiments_doc: dict[str, Any], hosts_doc: dict[str, Any], root: Path
) -> tuple[list[str], dict[str, dict[str, Any]], dict[str, dict[str, Any]]]:
    errors: list[str] = []
    conclusions = registry.get("conclusion")
    experiment_rows = experiments_doc.get("experiment")
    host_rows = hosts_doc.get("host")
    if registry.get("status") != "frozen" or experiments_doc.get("status") != "frozen" or hosts_doc.get("status") != "frozen":
        errors.append("conclusions, experiments, and hosts registries must be frozen")
    if not isinstance(conclusions, list) or not isinstance(experiment_rows, list) or not isinstance(host_rows, list):
        return errors + ["registries need [[conclusion]], [[experiment]], and [[host]] rows"], {}, {}
    required_conclusion = {"id", "milestone", "statement", "verbatim", "type", "source", "experiment", "report_revision", "report_path_at_revision", "tier", "coverage"}
    conclusion_by_id: dict[str, dict[str, Any]] = {}
    experiments_by_id: dict[str, dict[str, Any]] = {}
    for row in conclusions:
        cid = str(row.get("id", "<missing>"))
        if not required_conclusion <= row.keys():
            errors.append(f"conclusion omits required fields: {cid}")
        if cid in conclusion_by_id:
            errors.append(f"duplicate conclusion id: {cid}")
        conclusion_by_id[cid] = row
        if row.get("type") not in {"deterministic", "timing", "qualitative"}:
            errors.append(f"invalid conclusion type: {cid}")
        if row.get("tier") not in {1, 2}:
            errors.append(f"invalid conclusion tier: {cid}")
        coverage = row.get("coverage")
        if not isinstance(coverage, dict) or set(coverage) != {"report", "head"} or any(
            value not in {"sufficient", "partial", "rerunnable", "missing"}
            for value in coverage.values()
        ):
            errors.append(f"invalid per-role coverage for {cid}")
        if not FULL_SHA1.fullmatch(str(row.get("report_revision", ""))) or not _valid_commit(root, str(row.get("report_revision", ""))):
            errors.append(f"report_revision must be an available full commit SHA: {cid}")
            continue
        report_path = str(row.get("report_path_at_revision", ""))
        report_text = _git_text(root, "show", f"{row['report_revision']}:{report_path}")
        if report_text is None:
            errors.append(f"report then-path missing at report_revision: {cid}: {report_path}")
            continue
        source = str(row.get("source", ""))
        anchor = source.partition("#")[2]
        parent_text = _git_text(root, "show", f"{row['report_revision']}^:{report_path}")
        errors.extend(f"{cid}: {error}" for error in validate_anchor_content(report_text, parent_text, anchor, str(row.get("verbatim", ""))))

    for row in experiment_rows:
        eid = str(row.get("id", "<missing>"))
        if eid in experiments_by_id:
            errors.append(f"duplicate experiment id: {eid}")
        experiments_by_id[eid] = row
    referenced: dict[str, int] = {}
    for row in conclusions:
        eid = str(row.get("experiment", ""))
        referenced[eid] = referenced.get(eid, 0) + 1
        exp = experiments_by_id.get(eid)
        if exp is None:
            errors.append(f"unknown experiment for {row.get('id')}: {eid}")
            continue
        if exp.get("conclusion_id") != row.get("id"):
            errors.append(f"experiment conclusion_id mismatch: {eid}")
        if exp.get("tier") != row.get("tier"):
            errors.append(f"experiment tier mismatch: {eid}")
        if exp.get("status") != "incomplete" and (
            exp.get("report_revision") != row.get("report_revision")
            or exp.get("report_path_at_revision") != row.get("report_path_at_revision")
        ):
            errors.append(f"experiment report pin mismatch: {eid}")
        errors.extend(_protocol_errors(row, exp, root))
    for eid, count in referenced.items():
        if count != 1:
            errors.append(f"experiment id must resolve from exactly one conclusion: {eid}")

    errors.extend(validate_hosts(hosts_doc))
    host_by_id = {str(host.get("id", "")): host for host in host_rows}
    for exp in experiment_rows:
        if exp.get("status") == "incomplete":
            continue
        host_id = exp.get("host_id")
        if not isinstance(host_id, str) or host_id not in host_by_id:
            errors.append(f"experiment host_id is not registered: {exp.get('id')}")
    return errors, conclusion_by_id, experiments_by_id


def validate_hosts(hosts_doc: dict[str, Any]) -> list[str]:
    hosts = hosts_doc.get("host")
    if not isinstance(hosts, list) or not hosts:
        return ["hosts registry must contain [[host]] entries"]
    errors: list[str] = []
    ids: set[str] = set()
    for host in hosts:
        identifier = host.get("id") if isinstance(host, dict) else None
        if not isinstance(identifier, str) or not identifier:
            errors.append("each host needs a non-empty id")
            continue
        if identifier in ids:
            errors.append(f"duplicate host id: {identifier}")
        ids.add(identifier)
        required = {"cpu_model", "physical_cores", "logical_cores", "ram_bytes", "os_family", "rustc", "rustc_commit_hash", "rustc_commit_date", "rustc_host", "rust_target", "reference", "a5"}
        if not required <= host.keys():
            errors.append(f"host {identifier} omits required fields: {sorted(required - host.keys())}")
        if "rustc_commit_hash" in host and not FULL_SHA1.fullmatch(str(host["rustc_commit_hash"])):
            errors.append(f"host {identifier} rustc_commit_hash must be a full SHA-1")
        if "rustc_commit_date" in host:
            try:
                date.fromisoformat(str(host["rustc_commit_date"]))
            except ValueError:
                errors.append(f"host {identifier} rustc_commit_date must be ISO format")
        if "rustc_host" in host and (not isinstance(host["rustc_host"], str) or not host["rustc_host"]):
            errors.append(f"host {identifier} rustc_host must be non-empty")
        if not isinstance(host.get("a5"), dict):
            errors.append(f"host {identifier} must declare A-5 settings")
    if sum(host.get("reference") is True for host in hosts if isinstance(host, dict)) != 1:
        errors.append("hosts registry must have exactly one reference host")
    return errors


def validate_append_only_at_ref(root: Path, base_ref: str, evidence_path: Path = EVIDENCE) -> list[str]:
    relative = evidence_path.relative_to(root).as_posix()
    current = evidence_path.read_bytes() if evidence_path.exists() else b""
    base = _git(root, "show", f"{base_ref}:{relative}")
    if base.returncode:
        # A valid base revision without this file means the entire current file is appended.
        exists = _git(root, "cat-file", "-e", f"{base_ref}^{{commit}}").returncode == 0
        if not exists:
            return [f"base revision does not exist: {base_ref}"]
        base_bytes = b""
    else:
        base_bytes = base.stdout
    try:
        validate_evidence_append_only(base_bytes, current)
    except ValueError as error:
        return [str(error)]
    return []


def _tree_blobs(root: Path, revision: str) -> dict[str, tuple[str, str]]:
    listing = _git(root, "ls-tree", "-rz", revision)
    if listing.returncode:
        raise ValueError(listing.stderr.decode("utf-8", errors="replace"))
    entries: dict[str, tuple[str, str]] = {}
    for raw in listing.stdout.split(b"\0"):
        if not raw:
            continue
        metadata, raw_path = raw.split(b"\t", 1)
        mode, kind, oid = metadata.decode("ascii").split(" ")
        if kind == "blob":
            entries[raw_path.decode("utf-8")] = (oid, mode)
    return entries


def _blob_contents(root: Path, entries: dict[str, tuple[str, str]], paths: set[str]) -> dict[str, bytes]:
    wanted = [(path, entries[path][0]) for path in sorted(paths) if path in entries]
    if not wanted:
        return {}
    output = _git(root, "cat-file", "--batch", input_bytes=("\n".join(oid for _, oid in wanted) + "\n").encode("ascii"))
    if output.returncode:
        raise ValueError(output.stderr.decode("utf-8", errors="replace"))
    data = output.stdout
    offset = 0
    result: dict[str, bytes] = {}
    for path, oid in wanted:
        header_end = data.find(b"\n", offset)
        header = data[offset:header_end].decode("ascii").split()
        if len(header) != 3 or header[0] != oid or header[1] != "blob":
            raise ValueError(f"unexpected git cat-file response for {path}")
        size = int(header[2])
        start = header_end + 1
        result[path] = data[start : start + size]
        offset = start + size + 1
    return result


def compute_head_digest(root: Path, revision: str, harness_paths: list[str]) -> str:
    entries = _tree_blobs(root, revision)
    paths = {
        path
        for path in entries
        if (path.startswith("crates/") and "/src/" in path)
        or PurePosixPath(path).name == "Cargo.toml"
        or path in {"Cargo.lock", "rust-toolchain.toml"}
    }
    paths.update(str(path).replace("\\", "/") for path in harness_paths)
    files = _blob_contents(root, entries, paths)
    missing = set(harness_paths) - files.keys()
    if missing:
        raise ValueError(f"head revision misses protocol harness paths: {sorted(missing)}")
    from .verdicts import results_relevant_digest

    return results_relevant_digest(files)


def _current_head_digests(
    root: Path,
    head_sha: str,
    conclusions: dict[str, dict[str, Any]],
    experiments: dict[str, dict[str, Any]],
    evidence: list[dict[str, Any]],
) -> dict[str, str]:
    ids = {str(record.get("conclusion_id")) for record in evidence if record.get("role") == "head"}
    result = {}
    for cid in sorted(ids):
        conclusion = conclusions.get(cid)
        if conclusion is None:
            continue
        experiment = experiments.get(str(conclusion.get("experiment")), {})
        result[cid] = compute_head_digest(root, head_sha, list(experiment.get("harness_paths", [])))
    return result


def _markdown(rows: list[dict[str, Any]]) -> str:
    lines = [
        "<!-- Generated by [conclusions.py](../../analysis/simciv_analysis/conclusions.py) -->",
        "# M5 conclusion verdicts",
        "",
        "Each conclusion-role revision expands to ordered per-cell results; a conclusion without cells has one summary row.",
        "",
        "| Conclusion | Role | Revision | Verdict | Cell | Cell verdict | Evidence |",
        "|---|---|---|---|---|---|---|",
    ]
    for row in rows:
        cells = row["cells"] or [{"cell_id": "—", "verdict": row["verdict"], "evidence_ids": []}]
        for cell in cells:
            values = (
                f"`{row['conclusion_id']}`",
                str(row["role"]),
                str(row["revision"]),
                str(row["verdict"]),
                str(cell["cell_id"]),
                str(cell["verdict"]),
                ", ".join(cell.get("evidence_ids", [])) or "—",
            )
            lines.append("| " + " | ".join(value.replace("|", "\\|").replace("\n", " ") for value in values) + " |")
    return "\n".join(lines) + "\n"


def _load_toml(path: Path, label: str) -> tuple[dict[str, Any], list[str]]:
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        return {}, [f"could not parse {label}: {error}"]
    return value, []


def build_verdict_document(root: Path = ROOT, *, base_ref: str | None = None) -> tuple[str, list[str]]:
    registry, errors = _load_toml(root / "contracts" / "conclusions.toml", "conclusions registry")
    experiments_doc, e2 = _load_toml(root / "contracts" / "experiments.toml", "experiment registry")
    hosts_doc, e3 = _load_toml(root / "contracts" / "hosts.toml", "hosts registry")
    errors.extend(e2)
    errors.extend(e3)
    evidence, evidence_errors = load_evidence(root / "contracts" / "evidence.jsonl")
    errors.extend(evidence_errors)
    errors.extend(validate_append_only_at_ref(root, base_ref or _default_base_ref(root), root / "contracts" / "evidence.jsonl"))
    if errors:
        return "", errors
    registry_errors, conclusions_by_id, experiments_by_id = _conclusion_errors(registry, experiments_doc, hosts_doc, root)
    errors.extend(registry_errors)
    errors.extend(_evidence_errors(evidence, conclusions_by_id, experiments_by_id, hosts_doc, root))
    if errors:
        return "", errors

    reports = compute_verdicts(registry, experiments_doc, hosts_doc, evidence, roles=("report",))
    head_revisions = sorted({str(item["revision"]) for item in evidence if item.get("role") == "head"})
    rows = list(reports)
    for revision in head_revisions:
        digests = _current_head_digests(root, revision, conclusions_by_id, experiments_by_id, evidence)
        rows.extend(
            compute_verdicts(
                registry,
                experiments_doc,
                hosts_doc,
                evidence,
                roles=("head",),
                head_sha=revision,
                current_results_digests=digests,
            )
        )
    return _markdown(rows), []


def _default_base_ref(root: Path) -> str:
    for candidate in ("origin/main", "HEAD"):
        if _git(root, "cat-file", "-e", f"{candidate}^{{commit}}").returncode == 0:
            return candidate
    return "HEAD"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    parser.add_argument("--base", default=None, help="base revision for evidence append-only validation")
    args = parser.parse_args(argv)
    output, errors = build_verdict_document(ROOT, base_ref=args.base)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    if args.check:
        if not OUTPUT.is_file() or OUTPUT.read_text(encoding="utf-8") != output:
            print(f"generated verdict table differs: {OUTPUT.relative_to(ROOT)}", file=sys.stderr)
            return 1
        print("conclusions, evidence, and generated verdict table validated")
        return 0
    write_text_lf(OUTPUT, output)
    print(f"generated {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
