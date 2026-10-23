"""Generate a read-only inventory of stale M5-05 DVC stages."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

from .m5_05 import _registry
from .provenance import ROOT, source_digest


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
    digest = md5.removesuffix(".dir")
    return CACHE / digest[:2] / digest[2:]


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


def _summary_info(stage: str) -> tuple[bool, str | None]:
    summary = ROOT / "docs" / "data" / "m5_05" / stage / "summary.json"
    tracked = subprocess.run(
        ["git", "ls-files", "--error-unmatch", summary.relative_to(ROOT).as_posix()],
        cwd=ROOT,
        text=True,
        capture_output=True,
    ).returncode == 0
    if not tracked or not summary.is_file():
        return False, None
    payload = json.loads(summary.read_text(encoding="utf-8"))
    return True, payload.get("source_digest")


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
    entries = {f"m5_05_{entry['id']}": entry for entry in registry["conclusion"]}
    statuses = _dvc_status()
    run_outputs = _run_cache_outputs()
    current_digest = source_digest()
    rows: list[list[str]] = []
    for stage_key, records in sorted(statuses.items()):
        stage = stage_key.split(":")[-1]
        entry_id = stage.removeprefix("m5_05_")
        entry = entries.get(stage)
        if entry is None:
            continue
        dependencies, outputs = _status_parts(records)
        summary_exists, recorded_digest = _summary_info(entry_id)
        cache_md5s = run_outputs.get(entry_id, [])
        cache_state = "; ".join(_cache_object_state(md5) for md5 in cache_md5s) or "no stage-specific run-cache record"
        if int(entry["session"]) in {2, 3} and not summary_exists and not cache_md5s:
            classification = f"(a) not run; planned for Session {entry['session']}"
        elif cache_md5s and dependencies != "none":
            classification = "(b) run; dependencies changed afterwards"
        elif cache_md5s and any("missing" in _cache_object_state(md5) for md5 in cache_md5s):
            classification = "(c) run; raw objects absent from local DVC cache"
        else:
            classification = "unresolved from available local records"
        digest_note = (
            f"{recorded_digest or 'missing'} / {current_digest}"
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
    return f"""# M5-05 DVC stage status

Status: generated read-only inventory

## Question

Why do the seven M5-05 stages for Sessions 2–3 appear stale, and are their raw outputs available locally?

## Method

Generated by [`m5_05_dvc_status.py`](../../../analysis/simciv_analysis/m5_05_dvc_status.py) using `dvc status {DVC_PIPELINE} --json`, the committed M5-05 conclusion registry, Git tracking state, the current `source_digest()` function, and stage-specific records under `.dvc/cache/runs` and `.dvc/cache/files/md5`. No stage was reproduced. A missing summary has no recorded digest to compare; the table shows that explicitly alongside the current digest.

## Results

{table}

## Status interpretation

The conclusion registry places these stages in Sessions 2–3. No Git summary or stage-specific DVC run-cache record exists for them, so the available evidence classifies them as not run.

DVC reports the declared raw output paths as deleted and lists changed dependencies. This is the current local pipeline status; it does not establish that a completed run was lost.

## Limits

- DVC run-cache inspection is local to the machine on which this report was generated.
- Because no stage-specific output hash is recorded for these planned stages, unreferenced content-addressed cache objects cannot be attributed to them.
- Without a summary or stage-specific run-cache record, there is no recorded source digest or output hash for these stages.
"""


def main() -> None:
    text = build_status()
    REPORT.write_text(text, encoding="utf-8", newline="\n")
    print(f"generated {REPORT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
