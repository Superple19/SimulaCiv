"""Generate a read-only inventory of stale M5-05 DVC stages."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

from .m5_05 import _registry
from .provenance import ROOT, source_digest
from .io import write_text_lf


REPORT = ROOT / "docs" / "data" / "m5_05" / "STATUS.md"
DVC_PIPELINE = "docs/data/m5_05/dvc.yaml"
CACHE = ROOT / ".dvc" / "cache" / "files" / "md5"
RUN_CACHE = ROOT / ".dvc" / "cache" / "runs"


def _dvc_status() -> dict[str, list[dict[str, object]]]:
    result = subprocess.run(
        ["dvc", "status", DVC_PIPELINE, "--json"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    )
    return json.loads(result.stdout)


def _cache_path(md5: str) -> Path:
    return CACHE / md5[:2] / md5[2:]


def _cache_object_state(md5: str | None) -> str:
    if not md5:
        return "no stage-specific run-cache record"
    obj = _cache_path(md5)
    if not obj.is_file():
        return f"missing object `{md5}`"
    if md5.endswith(".dir"):
        try:
            entries = json.loads(obj.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return f"directory manifest `{md5}` unreadable"
        missing = [entry["md5"] for entry in entries if not _cache_path(str(entry["md5"])).is_file()]
        if missing:
            return f"manifest `{md5}` present; {len(missing)} file blob(s) missing"
        return f"complete (`{md5}` and {len(entries)} file blob(s))"
    return f"present (`{md5}`)"


def _run_cache_outputs() -> dict[str, list[str]]:
    outputs: dict[str, list[str]] = {}
    path_re = re.compile(r"^\s*-\s+path:\s*docs/data/m5_05/([^/\s]+)/raw\s*$")
    md5_re = re.compile(r"^\s*md5:\s*([0-9a-f]+(?:\.dir)?)\s*$")
    for record in RUN_CACHE.rglob("*"):
        if not record.is_file():
            continue
        try:
            lines = record.read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeDecodeError):
            continue
        output_stage: str | None = None
        output_md5: str | None = None
        in_outs = False
        for line in lines:
            if line.strip() == "outs:":
                in_outs = True
                continue
            if not in_outs:
                continue
            match = path_re.match(line)
            if match:
                output_stage = match.group(1)
                continue
            if output_stage:
                match = md5_re.match(line)
                if match:
                    output_md5 = match.group(1)
                    break
        if output_stage and output_md5:
            outputs.setdefault(output_stage, []).append(output_md5)
    return outputs


def _summary_info(stage: str) -> tuple[bool, str | None, list[str]]:
    summary = ROOT / "docs" / "data" / "m5_05" / stage / "summary.json"
    tracked = subprocess.run(
        ["git", "ls-files", "--error-unmatch", summary.relative_to(ROOT).as_posix()],
        cwd=ROOT,
        text=True,
        capture_output=True,
    ).returncode == 0
    if not tracked or not summary.is_file():
        return False, None, []
    payload = json.loads(summary.read_text(encoding="utf-8"))
    outputs = [
        str(record["dvc_md5"])
        for record in payload.get("outputs", [])
        if isinstance(record, dict) and record.get("dvc_md5")
    ]
    return True, payload.get("source_digest"), outputs


def _status_parts(records: list[dict[str, object]]) -> tuple[str, str]:
    dependencies: list[str] = []
    outputs: list[str] = []
    for record in records:
        changed = record.get("changed deps", {})
        if isinstance(changed, dict):
            dependencies.extend(f"{path} ({state})" for path, state in changed.items())
        changed_outputs = record.get("changed outs", {})
        if isinstance(changed_outputs, dict):
            outputs.extend(f"{path} ({state})" for path, state in changed_outputs.items())
    return "; ".join(dependencies) or "none", "; ".join(outputs) or "none"


def build_status() -> str:
    registry = _registry()
    statuses = _dvc_status()
    status_by_stage = {key.split(":")[-1]: records for key, records in statuses.items()}
    run_outputs = _run_cache_outputs()
    current_digest = source_digest()
    rows: list[list[str]] = []
    session_entries = [entry for entry in registry["conclusion"] if int(entry["session"]) in {2, 3}]
    changed_stage_count = 0
    tracked_summary_count = 0
    complete_cache_count = 0
    for entry in session_entries:
        entry_id = str(entry["id"])
        stage = f"m5_05_{entry_id}"
        records = status_by_stage.get(stage, [])
        dependencies, outputs = _status_parts(records)
        if dependencies != "none" or outputs != "none":
            changed_stage_count += 1
        summary_exists, recorded_digest, summary_md5s = _summary_info(entry_id)
        tracked_summary_count += int(summary_exists)
        cache_md5s = summary_md5s or run_outputs.get(entry_id, [])
        cache_state = "; ".join(_cache_object_state(md5) for md5 in cache_md5s) or "no stage-specific run-cache record"
        cache_complete = bool(cache_md5s) and not any("missing" in _cache_object_state(md5) for md5 in cache_md5s)
        complete_cache_count += int(cache_complete)
        if summary_exists and dependencies != "none":
            classification = "(b) run; dependencies changed afterwards"
        elif summary_exists and not cache_complete:
            classification = "(c) run; raw objects absent from local DVC cache"
        elif summary_exists and recorded_digest != current_digest:
            classification = "summary source digest differs from current sources"
        elif summary_exists:
            classification = "run and committed; DVC up to date"
        elif cache_md5s and dependencies != "none":
            classification = "(b) run; dependencies changed afterwards"
        elif cache_md5s and not cache_complete:
            classification = "(c) run; raw objects absent from local DVC cache"
        elif cache_md5s:
            classification = "raw cache exists; Git summary is missing"
        else:
            classification = f"(a) not run; planned for Session {entry['session']}"
        digest_note = (
            f"{recorded_digest or 'missing'} / {current_digest} ({'match' if recorded_digest == current_digest else 'diff'})"
            if summary_exists else f"no Git summary / {current_digest}"
        )
        rows.append([
            entry_id,
            str(entry["session"]),
            dependencies,
            outputs,
            "yes" if summary_exists else "no",
            digest_note,
            cache_state,
            classification,
        ])

    headers = [
        "Stage", "Planned session", "DVC changed dependencies", "DVC changed outputs",
        "Summary in Git", "Recorded/current source_digest", "Local DVC raw cache", "Classification",
    ]
    table = "| " + " | ".join(headers) + " |\n|" + "|".join("---" for _ in headers) + "|\n"
    table += "\n".join("| " + " | ".join(value.replace("|", "\\|") for value in row) + " |" for row in rows)
    stage_count = len(session_entries)
    status_line = (
        f"Status: {tracked_summary_count}/{stage_count} summaries tracked; "
        f"{complete_cache_count}/{stage_count} raw caches complete; "
        f"{changed_stage_count} DVC stages changed"
    )
    interpretation = (
        f"All {stage_count} Session 2–3 summaries are tracked, their recorded source digests match current Rust sources, "
        "their raw outputs are present in the local DVC cache, and DVC reports no stale stages."
        if tracked_summary_count == stage_count and complete_cache_count == stage_count and changed_stage_count == 0
        else "The table reports which summaries, source digests, DVC dependencies, and local cache objects need follow-up."
    )
    return f"""# M5-05 DVC stage status

{status_line}

## Question

Are the M5-05 Session 2–3 records committed, current, and backed by local DVC raw outputs?

## Method

Generated by [`m5_05_dvc_status.py`](../../../analysis/simciv_analysis/m5_05_dvc_status.py) using `dvc status {DVC_PIPELINE} --json`, the committed M5-05 conclusion registry, Git tracking state, the current `source_digest()` function, summary output hashes, and stage-specific records under `.dvc/cache/runs` and `.dvc/cache/files/md5`. This inventory is read-only; it does not run stages.

## Results

{table}

## Status interpretation

{interpretation}

## Limits

- DVC run-cache inspection is local to the machine on which this report was generated.
- Raw DVC objects are local cache evidence; they are not Git-tracked or uploaded by this task.
- DVC cache availability is specific to the machine on which this report was generated.
"""


def main() -> None:
    text = build_status()
    write_text_lf(REPORT, text)
    print(f"generated {REPORT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
