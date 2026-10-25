"""Block unsafe DVC pushes while recovered artifacts lack a final privacy decision."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path, PurePosixPath

from .git_hooks import dvc_output_paths
from .provenance import ROOT


PRIVACY_DECISION = "M5-RECOVERED-REMOTE-PRIVACY"
PUBLIC_REMOTE_NAMES = {"dagshub"}
PRIVATE_DRAFT = Path("docs/reproducibility/M5_RECOVERED_PRIVATE_REMOTE_DRAFT.md")
DECISIONS = Path("contracts/decisions.toml")
DVC_CONFIG = Path(".dvc/config")
REMOTE_SECTION = re.compile(r"^\s*\[\s*'?remote\s+\"([^\"]+)\"'?\s*\]\s*$")
CORE_SECTION = re.compile(r"^\s*\[core\]\s*$")


def _dvc_config(text: str) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    core: dict[str, str] = {}
    remotes: dict[str, dict[str, str]] = {}
    section: str | None = None
    for line in text.splitlines():
        if CORE_SECTION.match(line):
            section = "core"
            continue
        remote = REMOTE_SECTION.match(line)
        if remote:
            section = remote.group(1)
            remotes.setdefault(section, {})
            continue
        if line.lstrip().startswith("["):
            section = None
            continue
        if section and "=" in line:
            key, value = (piece.strip() for piece in line.split("=", 1))
            value = value.strip("\"'")
            (core if section == "core" else remotes[section])[key] = value
    return core, remotes


def _listed_paths(draft: str) -> list[PurePosixPath]:
    section = draft.split("## Exact manifest paths under review", 1)
    if len(section) != 2:
        raise ValueError("privacy draft is missing its exact-path section")
    body = section[1].split("\n## ", 1)[0]
    paths = []
    for value in re.findall(r"`([^`]+)`", body):
        if value.startswith("recovered/"):
            paths.append(PurePosixPath("docs/data/m5_recovered") / value.rstrip("/"))
        elif value.startswith("docs/data/"):
            paths.append(PurePosixPath(value.rstrip("/")))
    if not paths:
        raise ValueError("privacy draft contains no recovered artifact paths")
    return paths


def _intersects_output(path: PurePosixPath, outputs: set[str]) -> bool:
    return any(
        path == PurePosixPath(output)
        or path in PurePosixPath(output).parents
        or PurePosixPath(output) in path.parents
        for output in outputs
    )


def dvc_push_violations(remote_name: str, root: Path = ROOT) -> list[str]:
    root = root.resolve()
    errors = []
    try:
        decisions = tomllib.loads((root / DECISIONS).read_text(encoding="utf-8"))
        rows = [row for row in decisions.get("decision", []) if row.get("id") == PRIVACY_DECISION]
        if len(rows) != 1:
            return [f"privacy decision must have exactly one registry entry: {PRIVACY_DECISION}"]
        if rows[0].get("status") != "frozen":
            errors.append(f"DVC push blocked: {PRIVACY_DECISION} is not final (status={rows[0].get('status')})")

        core, remotes = _dvc_config((root / DVC_CONFIG).read_text(encoding="utf-8"))
        if core.get("remote"):
            errors.append("DVC push blocked: remove the default remote; use an explicit -r remote")
        if remote_name not in remotes:
            errors.append(f"DVC push blocked: named remote is not configured: {remote_name}")
        if errors:
            return errors

        draft_paths = _listed_paths((root / PRIVATE_DRAFT).read_text(encoding="utf-8"))
        outputs = dvc_output_paths(root)
        configured_sensitive = [str(path) for path in draft_paths if _intersects_output(path, outputs)]
        if remote_name in PUBLIC_REMOTE_NAMES and configured_sensitive:
            errors.append(
                f"DVC push blocked: {len(configured_sensitive)} privacy-listed paths are covered by DVC outputs for public remote {remote_name}"
            )
    except (OSError, KeyError, ValueError, tomllib.TOMLDecodeError) as error:
        errors.append(f"DVC push blocked: privacy/configuration check failed: {error}")
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("remote", help="explicit named DVC remote; no default remote is used")
    args = parser.parse_args(argv)
    errors = dvc_push_violations(args.remote)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"DVC push allowed to named remote {args.remote}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
