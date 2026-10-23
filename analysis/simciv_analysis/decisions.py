"""Validate the machine-readable M5 decision and stage registry."""

from __future__ import annotations

import argparse
import datetime as dt
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

from .io import write_text_lf


ROOT = Path(__file__).resolve().parents[2]
REGISTRY = ROOT / "contracts" / "decisions.toml"
CONTRACT = Path("docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md")
README = Path("README.md")
DECISION_GROUPS = {
    "m5-00": f"{CONTRACT}#8-m5-00-decision-register-frozen",
    "m5-02": f"{CONTRACT}#103-decision-register-frozen",
    "m5-03": f"{CONTRACT}#114-m5-03-decision-register-frozen",
    "m5-04": f"{CONTRACT}#125-m5-04-decision-register-frozen",
    "m5-05": f"{CONTRACT}#136-m5-05-decision-register-frozen",
    "m5-00-a1": f"{CONTRACT}#144-m5-00-a-1a-2-decision-register-frozen",
    "m5-05-a1": f"{CONTRACT}#156-frozen-decision-register",
    "m5-infrastructure": f"{CONTRACT}#16-m5-infrastructure-decision-register-frozen",
}
DECISION_MARKERS = {
    key: (f"<!-- BEGIN GENERATED DECISIONS: {key} -->", f"<!-- END GENERATED DECISIONS: {key} -->")
    for key in DECISION_GROUPS
}
CONTRACT_STAGE_MARKERS = (
    "<!-- BEGIN GENERATED M5 STAGES -->",
    "<!-- END GENERATED M5 STAGES -->",
)
README_STAGE_MARKERS = (
    "<!-- BEGIN GENERATED MILESTONE TABLE -->",
    "<!-- END GENERATED MILESTONE TABLE -->",
)


class RegistryError(ValueError):
    """Raised when registry records or their enforcement references are invalid."""


def _repo_path(root: Path, value: str) -> Path:
    return root.joinpath(*value.partition("#")[0].replace("\\", "/").split("/"))


def _collect_test_ids(root: Path) -> set[str]:
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "--collect-only", "-q"],
        cwd=root / "analysis",
        capture_output=True,
        check=False,
        text=True,
    )
    if result.returncode:
        raise RegistryError(f"pytest collection failed:\n{result.stdout}{result.stderr}")
    collected = set()
    for line in result.stdout.splitlines():
        node = line.strip().replace("\\", "/")
        if node.startswith("tests/") and "::" in node:
            collected.add(f"analysis/{node}")
    return collected


def _just_targets(root: Path) -> set[str]:
    recipe = re.compile(r"^([A-Za-z0-9_-]+)(?:\s+[A-Z][A-Z0-9_]*)?:")
    return {
        match.group(1)
        for line in (root / "justfile").read_text(encoding="utf-8").splitlines()
        if (match := recipe.match(line))
    }


def _prek_hooks(root: Path) -> set[str]:
    hook_id = re.compile(r"^\s*-\s+id:\s*([A-Za-z0-9_-]+)\s*$")
    return {
        match.group(1)
        for line in (root / ".pre-commit-config.yaml").read_text(encoding="utf-8").splitlines()
        if (match := hook_id.match(line))
    }


def validate_registry(
    registry: dict[str, Any],
    root: Path = ROOT,
    *,
    test_ids: set[str] | None = None,
    just_targets: set[str] | None = None,
    prek_hooks: set[str] | None = None,
) -> list[str]:
    required_decision = {"id", "milestone", "stage", "status", "summary", "enforced_by", "doc", "date"}
    required_stage = {"id", "milestone", "status", "summary", "doc"}
    decisions = registry.get("decision")
    stages = registry.get("stage")
    if not isinstance(decisions, list) or not isinstance(stages, list):
        raise RegistryError("registry requires [[decision]] and [[stage]] entries")

    decision_ids: dict[str, dict[str, Any]] = {}
    for item in decisions:
        if not isinstance(item, dict) or not required_decision <= item.keys():
            raise RegistryError(f"decision entry missing required fields: {item!r}")
        allowed = required_decision | {"superseded_by"}
        if item.keys() - allowed:
            raise RegistryError(f"unknown decision fields for {item.get('id')}: {item.keys() - allowed}")
        if not isinstance(item["id"], str) or not item["id"].strip():
            raise RegistryError("decision id must be a non-empty string")
        if item["id"] in decision_ids:
            raise RegistryError(f"duplicate decision id: {item['id']}")
        if item["status"] not in {"draft", "frozen", "superseded"}:
            raise RegistryError(f"invalid status for {item['id']}: {item['status']}")
        if not isinstance(item["summary"], str) or not item["summary"].strip():
            raise RegistryError(f"summary must be non-empty for {item['id']}")
        if not isinstance(item["enforced_by"], list) or not all(
            isinstance(ref, str) and ref.strip() for ref in item["enforced_by"]
        ):
            raise RegistryError(f"enforced_by must be a list of non-empty strings for {item['id']}")
        if not isinstance(item["date"], str):
            raise RegistryError(f"date must be an ISO date string for {item['id']}")
        try:
            dt.date.fromisoformat(item["date"])
        except ValueError as error:
            raise RegistryError(f"invalid ISO date for {item['id']}: {item['date']}") from error
        if not isinstance(item["doc"], str) or not _repo_path(root, item["doc"]).is_file():
            raise RegistryError(f"document does not exist for {item['id']}: {item.get('doc')}")
        decision_ids[item["id"]] = item

    stage_ids: dict[str, dict[str, Any]] = {}
    for item in stages:
        if not isinstance(item, dict) or not required_stage <= item.keys():
            raise RegistryError(f"stage entry missing required fields: {item!r}")
        allowed = required_stage | {"tag"}
        if item.keys() - allowed:
            raise RegistryError(f"unknown stage fields for {item.get('id')}: {item.keys() - allowed}")
        if not isinstance(item["id"], str) or not item["id"].strip():
            raise RegistryError("stage id must be a non-empty string")
        if item["id"] in stage_ids:
            raise RegistryError(f"duplicate stage id: {item['id']}")
        if not all(isinstance(item[key], str) and item[key].strip() for key in ("milestone", "status", "summary")):
            raise RegistryError(f"stage milestone, status, and summary must be non-empty strings for {item['id']}")
        if not isinstance(item["doc"], str) or not _repo_path(root, item["doc"]).is_file():
            raise RegistryError(f"document does not exist for stage {item['id']}: {item.get('doc')}")
        if "tag" in item and not isinstance(item["tag"], str):
            raise RegistryError(f"tag must be a string for {item['id']}")
        stage_ids[item["id"]] = item

    for item in decisions:
        stage = stage_ids.get(item["stage"])
        if stage is None:
            raise RegistryError(f"unknown stage {item['stage']} for {item['id']}")
        if stage["milestone"] != item["milestone"]:
            raise RegistryError(f"milestone does not match stage for {item['id']}")
        if item["status"] == "superseded":
            target = decision_ids.get(item.get("superseded_by"))
            if target is None or target["status"] != "frozen":
                raise RegistryError(f"{item['id']} must point to an existing frozen superseding decision")
        elif "superseded_by" in item:
            raise RegistryError(f"superseded_by is only valid for superseded decisions: {item['id']}")

    _assert_supersession_acyclic(decisions)
    test_ids = _collect_test_ids(root) if test_ids is None else test_ids
    just_targets = _just_targets(root) if just_targets is None else just_targets
    prek_hooks = _prek_hooks(root) if prek_hooks is None else prek_hooks
    for item in decisions:
        if item["status"] != "frozen":
            continue
        for reference in item["enforced_by"]:
            if reference.startswith("test:"):
                target = reference.removeprefix("test:").replace("\\", "/")
                if target not in test_ids:
                    raise RegistryError(f"collected test does not exist for {item['id']}: {target}")
            elif reference.startswith("just:"):
                target = reference.removeprefix("just:")
                if target not in just_targets:
                    raise RegistryError(f"just target does not exist for {item['id']}: {target}")
            elif reference.startswith("prek:"):
                target = reference.removeprefix("prek:")
                if target not in prek_hooks:
                    raise RegistryError(f"prek hook does not exist for {item['id']}: {target}")
            elif not _repo_path(root, reference).exists():
                raise RegistryError(f"enforcement path does not exist for {item['id']}: {reference}")

    return [item["id"] for item in decisions if item["status"] == "frozen" and not item["enforced_by"]]


def _assert_supersession_acyclic(decisions: list[dict[str, Any]]) -> None:
    graph = {item["id"]: item.get("superseded_by") for item in decisions}
    done: set[str] = set()
    for start in graph:
        path: set[str] = set()
        current: str | None = start
        while current is not None and current not in done:
            if current in path:
                raise RegistryError(f"supersession cycle includes {current}")
            path.add(current)
            current = graph.get(current)
        done.update(path)


def load_registry(root: Path = ROOT) -> tuple[dict[str, Any], list[str]]:
    registry = tomllib.loads((root / "contracts" / "decisions.toml").read_text(encoding="utf-8"))
    unenforced = validate_registry(registry, root)
    return registry, unenforced


def _cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def _markdown_link(doc: str, source: Path) -> str:
    path, _, anchor = doc.partition("#")
    absolute = ROOT / path
    relative = Path(__import__("os").path.relpath(absolute, (ROOT / source).parent)).as_posix()
    if anchor:
        relative += f"#{anchor}"
    return f"[source]({relative})"


def render_decision_table(decisions: list[dict[str, Any]], source: Path) -> str:
    lines = [
        "| ID | Status | Summary | Enforced by | Date | Document |",
        "|---|---|---|---|---|---|",
    ]
    for item in decisions:
        enforced = "<br>".join(f"`{_cell(ref)}`" for ref in item["enforced_by"]) or "—"
        lines.append(
            "| "
            + " | ".join(
                (
                    f"`{_cell(item['id'])}`",
                    item["status"],
                    _cell(item["summary"]),
                    enforced,
                    item["date"],
                    _markdown_link(item["doc"], source),
                )
            )
            + " |"
        )
    return "\n".join(lines)


def render_stage_table(stages: list[dict[str, Any]], source: Path, *, milestone: str | None = None) -> str:
    selected = [item for item in stages if milestone is None or item["milestone"] == milestone]
    lines = [
        "| Milestone | Stage | Status | Tag | Summary | Document |",
        "|---|---|---|---|---|---|",
    ]
    for item in selected:
        lines.append(
            "| "
            + " | ".join(
                (
                    item["milestone"],
                    f"`{_cell(item['id'])}`",
                    _cell(item["status"]),
                    f"`{_cell(item.get('tag', ''))}`" if item.get("tag") else "—",
                    _cell(item["summary"]),
                    _markdown_link(item["doc"], source),
                )
            )
            + " |"
        )
    return "\n".join(lines)


def _replace_marked(text: str, start: str, end: str, contents: str) -> str:
    pattern = re.compile(re.escape(start) + r"\n.*?\n" + re.escape(end), re.DOTALL)
    updated, count = pattern.subn(f"{start}\n{contents}\n{end}", text)
    if count != 1:
        raise RegistryError(f"expected exactly one generated-table marker block: {start}")
    return updated


def generated_files(registry: dict[str, Any]) -> dict[Path, str]:
    result: dict[Path, str] = {}
    decisions = registry["decision"]
    contract_text = (ROOT / CONTRACT).read_text(encoding="utf-8")
    for key, doc in DECISION_GROUPS.items():
        rows = [item for item in decisions if item["doc"] == doc]
        if not rows:
            raise RegistryError(f"no decisions mapped to register {key}")
        start, end = DECISION_MARKERS[key]
        contract_text = _replace_marked(
            contract_text,
            start,
            end,
            render_decision_table(rows, CONTRACT),
        )
    start, end = CONTRACT_STAGE_MARKERS
    contract_text = _replace_marked(
        contract_text,
        start,
        end,
        render_stage_table(registry["stage"], CONTRACT, milestone="M5"),
    )
    result[ROOT / CONTRACT] = contract_text

    readme_text = (ROOT / README).read_text(encoding="utf-8")
    start, end = README_STAGE_MARKERS
    result[ROOT / README] = _replace_marked(
        readme_text,
        start,
        end,
        render_stage_table(registry["stage"], README),
    )
    return result


def update_generated(*, check: bool) -> bool:
    registry, unenforced = load_registry()
    generated = generated_files(registry)
    mismatched = []
    for path, expected in generated.items():
        current = path.read_text(encoding="utf-8")
        if current == expected:
            continue
        if check:
            mismatched.append(path.relative_to(ROOT).as_posix())
        else:
            write_text_lf(path, expected)
    if mismatched:
        print(f"generated decision/stage tables differ: {', '.join(mismatched)}", file=sys.stderr)
        return False
    if unenforced:
        print("Frozen decisions without enforcement: " + ", ".join(unenforced))
    print("decision registry and generated tables validated")
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--check", action="store_true", help="validate and compare generated tables")
    action.add_argument("--write", action="store_true", help="validate and update generated tables")
    action.add_argument("--validate", action="store_true", help="validate registry entries only")
    args = parser.parse_args(argv)
    try:
        if args.validate:
            _, unenforced = load_registry()
            if unenforced:
                print("Frozen decisions without enforcement: " + ", ".join(unenforced))
            print("decision registry validated")
            return 0
        return 0 if update_generated(check=args.check) else 1
    except (OSError, RegistryError, tomllib.TOMLDecodeError) as error:
        print(f"decision registry error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
