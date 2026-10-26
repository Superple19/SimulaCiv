from __future__ import annotations

import csv
import gzip
import io
import json
import argparse
import re
import subprocess
import sys
import tomllib
from collections import Counter, defaultdict
from pathlib import Path

from .claims import load_sources, scan_documents, source_for_claim
from .io import content_matches, write_text_lf
from .provenance import ROOT

LEDGER = ROOT / "docs/reproducibility/claims.csv.gz"
MANIFEST = ROOT / "docs/data/m5_recovered/MANIFEST.json"
SOURCES = ROOT / "docs/reproducibility/sources.toml"
M5_RESULTS = ROOT / "docs/reproducibility/M5_05_M0_M3_REGENERATION_RESULTS.json"
OUT_CSV = ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv"
OUT_MD = ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.md"
A5_SPEC = ROOT / "docs/reproducibility/m5_05_configs/m3_05_a5_pilot.toml"

FILTER_REPORT = re.compile(r"^M[23]_")
PROTOCOL_CUE = re.compile(
    r"config|configuration|\bN\b|\bK\b|p_local|\bk\b|day|horizon|warm.?up|sample|paired|interleav|"
    r"timer|timed|measurement|benchmark|\bcall\b|\bseed\b|replicate|outside|inside|per tick",
    re.IGNORECASE,
)
METHOD_HEADING = re.compile(r"method|protocol|measurement|benchmark|configuration|sampling|host and", re.I)
TOKEN_PATTERNS = {
    "days": re.compile(r"(?:\b\d[\d,]*\s*(?:consecutive\s+)?days?\b|\bdays?\s+\d[\d,]*\s*[-–]\s*\d[\d,]*|\bH\s*=\s*\d[\d,]*|\bday\s*\d[\d,]*\s*(?:to|[-–])\s*\d[\d,]*)", re.I),
    "warmups": re.compile(r"\b\d[\d,]*\s+warm.?ups?\b", re.I),
    "samples": re.compile(r"\b(?:\d[\d,]*\s+)?(?:timed\s+)?samples?\b", re.I),
    "pairing": re.compile(r"paired|interleav|alternat|rotation|same (?:state|snapshot|checkpoint)|cloned, identical", re.I),
    "timer_scope": re.compile(r"timer|timed|inside|outside|per tick|per public call|call(?:ed|s)? .*days?|excludes|includes", re.I),
    "configuration": re.compile(r"config(?:uration)?|\bp_local\b|\bK\s*=|\bk\s*=|\bN\s*=", re.I),
}


def git(*args: str, check: bool = True) -> str:
    result = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    return result.stdout.strip()


def split_cells(line: str) -> list[str]:
    return [part.strip() for part in line.strip().strip("|").split("|")]


def separator(line: str) -> bool:
    cells = split_cells(line)
    return bool(cells) and all(re.fullmatch(r":?-{3,}:?", item) for item in cells)


def slug(text: str) -> str:
    text = re.sub(r"[`*_]", "", text).lower()
    return re.sub(r"[^a-z0-9]+", "-", text).strip("-") or "document"


def source_blocks(report: str, lines: list[str]) -> list[dict[str, object]]:
    headings: list[tuple[int, str]] = [(0, "document")]
    for n, line in enumerate(lines, 1):
        match = re.match(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        if match:
            headings.append((n, match.group(1)))
    blocks = []
    for i, line in enumerate(lines):
        if not line.lstrip().startswith("|") or i + 1 >= len(lines) or not separator(lines[i + 1]):
            continue
        start = i + 1
        end = start + 1
        while end < len(lines) and lines[end].lstrip().startswith("|"):
            end += 1
        heading = max((item for item in headings if item[0] <= start), default=(0, "document"), key=lambda x: x[0])[1]
        blocks.append({
            "start": start,
            "end": end,
            "section_anchor": slug(heading),
            "header": lines[i],
            "data_rows": [lines[j] for j in range(i + 2, end) if lines[j].lstrip().startswith("|") and not separator(lines[j])],
        })
    return blocks


def prose_blocks(lines: list[str]) -> list[dict[str, object]]:
    """Keep one paragraph or list item together; split separate findings bullets."""
    blocks: list[dict[str, object]] = []
    active: dict[str, object] | None = None
    for index, line in enumerate(lines, 1):
        stripped = line.strip()
        if not stripped or re.match(r"^\s{0,3}#{1,6}\s", line) or line.lstrip().startswith("|"):
            active = None
            continue
        is_list_item = bool(re.match(r"^\s{0,3}(?:[-*+]\s+|\d+[.)]\s+)", line))
        if is_list_item or active is None:
            active = {"start": index, "end": index, "lines": [line]}
            blocks.append(active)
        else:
            active["end"] = index
            active["lines"].append(line)
    return blocks


def report_creation(report: str) -> tuple[str, str]:
    candidates = git("log", "--follow", "--diff-filter=A", "--format=%H", "--", f"docs/reports/{report}", check=False).splitlines()
    commit = candidates[-1] if candidates else ""
    if not commit:
        return "not found", ""
    name_status = git("show", "--format=", "--name-status", commit).splitlines()
    paths = []
    for line in name_status:
        parts = line.split("\t")
        if len(parts) >= 2 and parts[0].startswith("A"):
            paths.append(parts[-1])
        elif len(parts) >= 3 and parts[0].startswith("R"):
            paths.append(parts[-1])
    path = next((p for p in paths if Path(p).name == report), "")
    if not path:
        tree_paths = git("ls-tree", "-r", "--name-only", commit).splitlines()
        path = next((p for p in tree_paths if Path(p).name == report), "")
    return commit, path


def path_exists_at(commit: str, path: str) -> bool:
    if not commit or not path:
        return False
    return subprocess.run(["git", "-C", str(ROOT), "cat-file", "-e", f"{commit}:{path}"], capture_output=True).returncode == 0


def text_from_sections(lines: list[str], section_title: str) -> tuple[list[tuple[int, str]], list[tuple[int, str]]]:
    current = "document"
    relevant_method: list[tuple[int, str]] = []
    relevant_section: list[tuple[int, str]] = []
    for n, line in enumerate(lines, 1):
        match = re.match(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        if match:
            current = match.group(1)
        if not PROTOCOL_CUE.search(line):
            continue
        entry = (n, line)
        if current == section_title:
            relevant_section.append(entry)
        if METHOD_HEADING.search(current):
            relevant_method.append(entry)
    return relevant_method, relevant_section


def compact_exact(lines: list[tuple[int, str]], limit: int = 10) -> str:
    selected = []
    seen = set()
    for n, line in lines:
        value = f"L{n}: {line.rstrip()}"
        if value not in seen:
            selected.append(value)
            seen.add(value)
        if len(selected) >= limit:
            break
    return "\n".join(selected)


def data_row_count(block: dict[str, object] | None) -> int:
    return len(block.get("data_rows", [])) if block else 0


def exact_matches(pattern: re.Pattern[str], text: str) -> list[str]:
    return list(dict.fromkeys(match.group(0) for match in pattern.finditer(text)))


def a5_cell_floor_seconds(root: Path = ROOT) -> int:
    spec = tomllib.loads((root / A5_SPEC.relative_to(ROOT)).read_text(encoding="utf-8"))
    seconds = (int(spec["warmups"]) + int(spec["samples"])) * float(spec["sample_floor_seconds"])
    if not seconds.is_integer():
        raise ValueError("A-5 cell floor must resolve to a whole number of seconds")
    return int(seconds)


def check_command_test_exists(command: str, commit: str = "") -> tuple[bool, bool]:
    test = re.search(r"--test\s+([A-Za-z0-9_]+)", command)
    if not test:
        return False, False
    path = f"crates/sim-model/tests/{test.group(1)}.rs"
    return path_exists_at(commit, path), (ROOT / path).is_file()


def build_outputs() -> dict[Path, bytes]:
    cell_floor = a5_cell_floor_seconds()
    with gzip.open(LEDGER, "rt", encoding="utf-8", newline="") as stream:
        ledger = list(csv.DictReader(stream))
    selected = [r for r in ledger if FILTER_REPORT.match(r["report"]) and r["tier"] in {"headline", "key"} and r["status"] in {"missing-data", "missing-source"}]
    scanned = {str(r["claim_key"]): r for r in scan_documents(ROOT)}
    missing_keys = sorted({r["claim_key"] for r in selected} - set(scanned))
    if missing_keys:
        raise RuntimeError(f"current source scan did not locate {len(missing_keys)} ledger keys: {missing_keys[:5]}")

    # Tables stay grouped by their source table; prose is grouped by paragraph/list item.
    by_report_lines: dict[str, list[str]] = {}
    blocks_by_report: dict[str, list[dict[str, object]]] = {}
    prose_by_report: dict[str, list[dict[str, object]]] = {}
    for report in sorted({r["report"] for r in selected}):
        path = ROOT / "docs" / "reports" / report
        lines = path.read_text(encoding="utf-8").splitlines()
        by_report_lines[report] = lines
        blocks_by_report[report] = source_blocks(report, lines)
        prose_by_report[report] = prose_blocks(lines)

    groups: dict[tuple[object, ...], dict[str, object]] = {}
    for record in selected:
        source = scanned[record["claim_key"]]
        report = record["report"]
        context = json.loads(str(source["context"]))
        anchor = record["section anchor"]
        kind = context.get("kind", "prose")
        block = None
        if kind == "table":
            line = int(source["line"])
            block = next((b for b in blocks_by_report[report] if b["section_anchor"] == anchor and int(b["start"]) <= line <= int(b["end"])), None)
        if block is not None:
            key = (report, anchor, "table", int(block["start"]))
        elif kind == "table":
            key = (report, anchor, "table-unlocated", str(context.get("table_header", "")))
        else:
            line = int(source["line"])
            paragraph = next((p for p in prose_by_report[report] if int(p["start"]) <= line <= int(p["end"])), None)
            key = (report, anchor, "prose", int(paragraph["start"]) if paragraph else line)
        paragraph = None
        if kind != "table":
            source_line = int(source["line"])
            paragraph = next((p for p in prose_by_report[report] if int(p["start"]) <= source_line <= int(p["end"])), None)
        group = groups.setdefault(key, {"report": report, "anchor": anchor, "kind": str(key[2]), "block": block, "paragraph": paragraph, "claims": []})
        group["claims"].append(record)

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    evidence_types = {
        "reconstructed-source", "reconstructed-source-from-command", "compiled-example-binary",
        "historical-bench-source-snapshot", "raw-output-copy", "dvc-cache-raw-output",
        "session-log-extracts", "captured-report-source", "temporary-analysis-script-copy",
    }
    source_map = load_sources(ROOT)
    m5 = json.loads(M5_RESULTS.read_text(encoding="utf-8"))
    conclusions: dict[tuple[str, str], list[dict[str, object]]] = defaultdict(list)
    for entry in m5.get("conclusions", []):
        conclusions[(str(entry.get("report", "")), str(entry.get("section_anchor", "")))].append(entry)

    output_rows: list[dict[str, str]] = []
    groups_ordered = sorted(groups.values(), key=lambda g: (str(g["report"]), str(g["anchor"]), str(g["kind"]), int(g["block"]["start"]) if g["block"] else 0))
    for number, group in enumerate(groups_ordered, 1):
        report = str(group["report"])
        anchor = str(group["anchor"])
        claims = list(group["claims"])
        classes = Counter(r["class"] for r in claims)
        tiers = Counter(r["tier"] for r in claims)
        source_claim = scanned[claims[0]["claim_key"]]
        context0 = json.loads(str(source_claim["context"]))
        section_title = str(context0.get("section", ""))
        block = group["block"]
        report_path = ROOT / "docs" / "reports" / report
        lines = by_report_lines[report]
        method_lines, section_lines = text_from_sections(lines, section_title)
        exact_lines: list[tuple[int, str]] = []
        # Method material is shared by the report; section lines are nearest to this experiment.
        exact_lines.extend(method_lines[:10])
        paragraph = group.get("paragraph")
        if paragraph:
            exact_lines.extend((int(paragraph["start"]) + offset, text) for offset, text in enumerate(paragraph["lines"]))
        else:
            exact_lines.extend(section_lines[:8])
        if block:
            start = int(block["start"])
            exact_lines.append((start, str(block["header"])))
            first_data = list(block.get("data_rows", []))[:2]
            exact_lines.extend((start + 2 + offset, str(text)) for offset, text in enumerate(first_data))
        exact_lines = sorted(set(exact_lines), key=lambda item: item[0])
        quote = compact_exact(exact_lines, 18)
        quote_text = "\n".join(line.split(": ", 1)[-1] for line in quote.splitlines())
        evidence_text = "\n".join(line for _, line in exact_lines)
        dimensions: dict[str, set[str]] = defaultdict(set)
        for claim in claims:
            context = json.loads(claim["context"])
            for dim, value in context.get("dimensions", {}).items():
                dimensions[str(dim)].add(str(value))
        dimension_text = "; ".join(f"{k}={','.join(sorted(v))}" for k, v in sorted(dimensions.items())) or "not captured in claim context"
        n_values_set = set(dimensions.get("population_n", set()))
        for value in re.findall(r"\bN\s*=\s*([\d,]+(?:\s*[kKmM])?)", evidence_text):
            n_values_set.add(value.replace(" ", ""))
        n_values = ", ".join(sorted(n_values_set)) or "not stated/captured"
        k_values_set = set(dimensions.get("settlement_count", set()))
        for value in re.findall(r"\bK\s*=\s*([\d,]+(?:\s*[kKmM])?)", evidence_text):
            k_values_set.add(value.replace(" ", ""))
        k_values = ", ".join(sorted(k_values_set)) or "not stated/captured"
        rel_k_values_set = set(dimensions.get("relationship_degree_k", set()))
        for value in re.findall(r"(?<![A-Za-z])k\s*=\s*([\d,]+(?:\s*[kKmM])?)", evidence_text):
            rel_k_values_set.add(value.replace(" ", ""))
        rel_k_values = ", ".join(sorted(rel_k_values_set)) or "not stated/captured"
        config_lines = [line for _, line in exact_lines if re.search(r"config|configuration|p_local|\bK\s*=|\bk\s*=|\bN\s*=", line, re.I)]
        config_text = " || ".join(dict.fromkeys(line.strip() for line in config_lines)) or f"not directly stated; captured dimensions: {dimension_text}"
        fields = {}
        for name, pattern in TOKEN_PATTERNS.items():
            values = [f"L{line_no}: {line.strip()}" for line_no, line in exact_lines if pattern.search(line)]
            fields[name] = " || ".join(dict.fromkeys(values)) if values else "not stated in the report excerpt"
        if fields["configuration"] == "not stated in the report excerpt" and dimensions:
            fields["configuration"] = f"dimension values only: {dimension_text}"
        row_count = data_row_count(block)

        creation, historical_report_path = report_creation(report)
        mapped_harnesses: set[str] = set()
        mapped_tests: set[str] = set()
        mapped_data: set[str] = set()
        for claim in claims:
            context = json.loads(claim["context"])
            resolved = source_for_claim(source_map, report, str(context.get("section", "*")), str(claim["class"])) or {}
            mapped_harnesses.update(resolved.get("harness", []))
            mapped_tests.update(resolved.get("tests", []))
            mapped_data.update(resolved.get("data", []))
        if report == "M2_06_BASELINE_REPORT.md" and anchor == "1-executive-summary" and classes == {"deterministic": len(claims)}:
            # These are the M1 oracle checks named by the report's frozen-manifest correctness statement.
            mapped_tests.update({
                "cargo test -p sim-model --test m1_contract_manifest_tests test_22_500_day_graduation_state_hash_matches_manifest",
                "cargo test -p sim-model --test m1_contract_manifest_tests test_23_500_day_graduation_metrics_hash_matches_manifest",
                "cargo test -p sim-model --test m1_contract_manifest_tests test_24_500_day_graduation_event_hash_matches_manifest",
            })
        harness_at_commit = sorted(p for p in mapped_harnesses if path_exists_at(creation, p))
        harness_at_head = sorted(p for p in mapped_harnesses if (ROOT / p).is_file())
        tests_at_commit = sorted(cmd for cmd in mapped_tests if check_command_test_exists(cmd, creation)[0])
        tests_at_head = sorted(cmd for cmd in mapped_tests if check_command_test_exists(cmd)[1])

        report_entries = [(i, item) for i, item in enumerate(manifest.get("items", [])) if report in item.get("reports_supported", []) and item.get("type") in evidence_types]
        manifest_refs = []
        for idx, item in report_entries:
            note = str(item.get("completeness_notes", "")).replace(";", ",")
            manifest_refs.append(f"MANIFEST.json#/items/{idx} {item.get('path')} [{item.get('type')}; {note}]")
        if not manifest_refs:
            manifest_text = "none linked by this report in the recovered manifest"
        else:
            manifest_text = " || ".join(manifest_refs)

        m5_entries = conclusions.get((report, anchor), [])
        # Conclusion reruns cover prose conclusions at their exact anchor, not full historical tables.
        m5_summary = "none at this exact report/anchor"
        if m5_entries:
            m5_summary = " || ".join(f"{e.get('conclusion')}={e.get('status')}; notes={e.get('notes','')}; values={e.get('new values','')}" for e in m5_entries)
        m5_covers_group = bool(m5_entries) and str(group["kind"]) == "prose"

        complete_recovered_sources = [
            item for _, item in report_entries
            if item.get("type") in {"reconstructed-source", "reconstructed-source-from-command"}
            and re.search(r"complete|complete edit chain", str(item.get("completeness_notes", "")), re.I)
        ]
        only_deterministic = set(classes) == {"deterministic"}
        has_timing = bool(set(classes) & {"timing", "derived"})
        paragraph_text = "\n".join(str(line) for line in (group.get("paragraph") or {}).get("lines", []))
        table_text = "\n".join([str(block.get("header", "")), *[str(v) for v in block.get("data_rows", [])]]) if block else ""
        local_text = paragraph_text + "\n" + table_text
        local_n = set(re.findall(r"\bN\s*=\s*([\d,]+(?:\s*[kKmM])?)", local_text))
        local_n.update(dimensions.get("population_n", set()))
        if m5_covers_group:
            classification = "f"
            basis = "M5-05 has a conclusion-level result at this exact report anchor; that rerun covers the summary conclusion, not a separate historical detail table."
        elif only_deterministic and (tests_at_commit or tests_at_head):
            classification = "a"
            basis = "Deterministic claim group maps to named integration tests; replaying the original revision is sufficient to re-establish this deterministic result."
        elif harness_at_commit or harness_at_head:
            classification = "b"
            basis = "A source-mapped committed harness/bench exists at the report revision or HEAD; historical timing still needs a new measurement."
        elif complete_recovered_sources:
            classification = "c"
            basis = "The recovered manifest marks a recovered harness source complete; it has not been executed or rebuilt here."
        else:
            has_config = bool(config_lines) or dimension_text != "not captured in claim context"
            has_days = fields["days"] != "not stated in the report excerpt"
            has_timer = fields["timer_scope"] != "not stated in the report excerpt"
            comparative = bool(re.search(r"\b(?:vs\.?|versus|compare|comparison|paired|interleav|serial and parallel)\b", local_text, re.I))
            pairing_ok = (not comparative) or fields["pairing"] != "not stated in the report excerpt"
            missing_fields = []
            if not has_config:
                missing_fields.append("configuration")
            if not local_n and not n_values_set:
                missing_fields.append("N")
            if not has_days:
                missing_fields.append("days/horizon")
            if not has_timer:
                missing_fields.append("timer scope")
            if not pairing_ok:
                missing_fields.append("pairing for a comparison")
            if has_timing and not missing_fields and (row_count > 0 or bool(paragraph_text)):
                classification = "d"
                omissions = [name for name in ("warmups", "samples") if fields[name] == "not stated in the report excerpt"]
                omission_note = f" Original report omits {', '.join(omissions)}; the A-5 rerun protocol supplies its fixed 2+41 sampling." if omissions else ""
                basis = "No executable harness was found, but the report states configuration, population, horizon, timer scope, and comparison mode sufficiently to reconstruct a driver." + omission_note
            else:
                classification = "e"
                basis = "Protocol is too incomplete to reconstruct without assumptions; unresolved fields: " + ", ".join(missing_fields or ["N/cells or runnable source"]) + "."
        # Each table row is one inferred result cell; prose uses distinct explicitly stated N values.
        inferred_cells = row_count if row_count else max(len(local_n), len(n_values_set))
        estimate_applicable = classification in {"b", "c", "d"} and "timing" in classes
        cells = str(inferred_cells) if estimate_applicable and inferred_cells else "not estimable"
        estimate_seconds = str(inferred_cells * cell_floor) if estimate_applicable and inferred_cells else ""
        if classification == "a":
            estimate_note = "A-5 is a timing protocol and does not apply to deterministic test replay; runtime not estimated from report data."
        elif classification == "f":
            estimate_note = "No further run for the conclusion; supporting/detail claims remain outside that coverage."
        elif classification in {"b", "c", "d"} and "timing" not in classes:
            estimate_note = "No direct timing-class claim in this group; any derived values should be regenerated from their source experiment, not counted as a separate A-5 cell."
        elif estimate_seconds:
            estimate_note = f"Lower bound = {cell_floor} s per inferred table row/cell from the committed A-5 pilot protocol; excludes setup/build, retries, and any multi-setting rows."
        else:
            estimate_note = "Not estimable from the report because table-cell count/protocol is missing; no timing was run."

        group_id = f"E{number:03d}"
        claim_ids = ";".join(str(c["claim_id"]) for c in claims)
        values = "; ".join(f"{c['claim_id']}={c['value']}{c['unit']}" for c in claims)
        output_rows.append({
            "experiment_id": group_id,
            "report": report,
            "section_anchor": anchor,
            "report_section": section_title,
            "claim_source_group": str(group["kind"]),
            "table_header": str(block["header"]).strip() if block else "",
            "claim_count": str(len(claims)),
            "headline_count": str(tiers.get("headline", 0)),
            "key_count": str(tiers.get("key", 0)),
            "class_mix": "; ".join(f"{k}={v}" for k, v in sorted(classes.items())),
            "claim_ids": claim_ids,
            "claim_values": values,
            "report_written_commit": creation,
            "report_path_at_creation": historical_report_path,
            "protocol_quote_with_report_line_numbers": quote,
            "configuration_as_reported": config_text,
            "N_values": n_values,
            "K_values": k_values,
            "relationship_k_values": rel_k_values,
            "days_as_reported": fields["days"],
            "warmups_as_reported": fields["warmups"],
            "samples_as_reported": fields["samples"],
            "pairing_as_reported": fields["pairing"],
            "timer_scope_as_reported": fields["timer_scope"],
            "timer_contents_as_reported": " || ".join(
                f"L{line_no}: {line.strip()}" for line_no, line in exact_lines
                if re.search(r"includes?|inside|outside|enabled|disabled|metrics|events|snapshots|initialization|graph creation|context construction|timer", line, re.I)
            ) or "not stated in the report excerpt",
            "mapped_harnesses": "; ".join(sorted(mapped_harnesses)) or "none",
            "harnesses_present_at_report_commit": "; ".join(harness_at_commit) or "none",
            "harnesses_present_at_HEAD": "; ".join(harness_at_head) or "none",
            "mapped_test_targets": "; ".join(sorted(mapped_tests)) or "none",
            "tests_present_at_report_commit": "; ".join(tests_at_commit) or "none",
            "tests_present_at_HEAD": "; ".join(tests_at_head) or "none",
            "mapped_data_paths": "; ".join(sorted(mapped_data)) or "none",
            "recovered_manifest_entries": manifest_text,
            "M5_05_conclusion_level_coverage": m5_summary,
            "classification": classification,
            "classification_basis": basis,
            "A5_cell_estimate": cells,
            "A5_lower_bound_seconds": estimate_seconds,
            "A5_estimate_basis": estimate_note,
        })

    csv_buffer = io.StringIO(newline="")
    fields = list(output_rows[0]) if output_rows else []
    writer = csv.DictWriter(csv_buffer, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    writer.writerows(output_rows)
    csv_text = csv_buffer.getvalue()
    if len(csv_text.encode("utf-8")) >= 1024 * 1024:
        raise RuntimeError(f"inventory CSV exceeds 1 MiB: {len(csv_text.encode('utf-8'))} bytes")

    claim_by_class = Counter()
    group_by_class = Counter()
    for row in output_rows:
        group_by_class[row["classification"]] += 1
        claim_by_class[row["classification"]] += int(row["claim_count"])
    claims_by_report = Counter()
    for record in selected:
        claims_by_report[record["report"]] += 1
    top = sorted(output_rows, key=lambda r: (-int(r["headline_count"]) - int(r["key_count"]), r["report"], r["section_anchor"], r["experiment_id"]))[:10]
    timing_seconds = sum(int(row["A5_lower_bound_seconds"]) for row in output_rows if row["classification"] in {"b", "c", "d"} and row["A5_lower_bound_seconds"])
    classes = ["a", "b", "c", "d", "e", "f"]
    md = [
        "# M2/M3 Evidence Reconstruction Inventory",
        "",
        "**Status:** generated from committed claim and evidence sources; no new timing or simulation harness was run during inventory generation.",
        "",
        "## Scope and method",
        "",
        f"The source ledger filter selected **{len(selected)}** M2/M3 headline/key claims across **{len(claims_by_report)}** reports with status `missing-data` or `missing-source`. Table claims are grouped by source table within a section; prose claims are grouped by paragraph or list item. This inventory is a reconstruction plan, not a verification of the historical measurements.",
        "",
        "Report creation commits are the earliest additions found with `git log --follow --diff-filter=A`; report-era harness presence is checked in that commit tree and again at current HEAD. Recovered evidence cites exact `MANIFEST.json#/items/<index>` entries. M5-05 coverage is assigned only to a prose group at the exact report/anchor; representative M5-05 cells do not count as coverage for a full historical results table.",
        "",
        "Class labels: (a) deterministic replay at original revision; (b) timing with committed harness; (c) timing with complete recovered harness source; (d) reconstructed timing harness from a sufficiently specified report protocol; (e) protocol underspecified; (f) conclusion already covered by M5-05, with historical details excluded from that coverage.",
        "",
        f"A-5 lower bounds use the committed pilot protocol's {cell_floor}-second per-cell floor. They exclude build/setup, repetitions within each sample beyond the minimum, noise retries, and cells not enumerated as table rows; they are workload floors, not elapsed-time predictions. Deterministic replay is not an A-5 timing run.",
        "",
        "## Counts",
        "",
        "| Class | Experiments | Claims |",
        "|---|---:|---:|",
    ]
    class_names = {"a": "(a) deterministic replay", "b": "(b) committed harness", "c": "(c) complete recovered source", "d": "(d) reconstructable protocol", "e": "(e) underspecified", "f": "(f) M5-05 conclusion covered"}
    for cls in classes:
        md.append(f"| {class_names[cls]} | {group_by_class[cls]} | {claim_by_class[cls]} |")
    md.extend([
        "",
        f"Timing-class A-5 minimum for estimable (b)–(d) groups: **{timing_seconds:,} seconds ({timing_seconds / 3600:.2f} hours)**. This lower bound covers groups with an enumerated table-row or explicit-N cell count; **{sum(r['classification'] in {'b','c','d'} and not r['A5_lower_bound_seconds'] for r in output_rows)}** additional (b)–(d) groups have no defensible cell count and are unestimated. Class (e) is unestimated because the protocol is incomplete; class (a) deterministic replay has no A-5 duration estimate.",
        "",
        "## Ten largest experiment groups",
        "",
        "| # | Report and anchor | Claims | Class |",
        "|---:|---|---:|---|",
    ])
    for idx, row in enumerate(top, 1):
        md.append(f"| {idx} | `{row['report']}#{row['section_anchor']}` | {int(row['headline_count']) + int(row['key_count'])} | {row['classification']} |")
    md.extend([
        "",
        "## Evidence notes",
        "",
        "- The CSV records the exact protocol excerpts with report line references, the report-creation commit, harness/test availability at that commit and HEAD, recovered manifest entries, and any M5-05 conclusion result.",
        "- A blank A-5 estimate means the group is not an A-5 timing task or is not estimable from the recorded protocol; it does not mean zero compute cost.",
        "- The recovered archive is evidence only. No recovered binary was executed and no recovered source was compiled in this inventory task.",
        "",
        "## M3-05 end-to-end pilot proposal",
        "",
        "The pilot protocol and settings are in [m3_05_a5_pilot.toml](m5_05_configs/m3_05_a5_pilot.toml); the result section is rendered from committed pilot records.",
        "",
        "## Criteria and status",
        "",
        "The frozen reproduction criteria are in [REPRODUCTION_CRITERIA.md](REPRODUCTION_CRITERIA.md).",
        "",
    ])
    md_text = "\n".join(md)
    return {OUT_CSV: csv_text.encode("utf-8"), OUT_MD: md_text.encode("utf-8")}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    outputs = build_outputs()
    if args.check:
        different = [path.relative_to(ROOT).as_posix() for path, content in outputs.items() if not content_matches(path, content)]
        if different:
            print("evidence inventory differs: " + ", ".join(different), file=sys.stderr)
            return 1
        print("evidence inventory matches source inputs")
        return 0
    for path, content in outputs.items():
        write_text_lf(path, content.decode("utf-8"))
    print("generated evidence inventory from committed source inputs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
