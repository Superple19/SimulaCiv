from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


def _run_steps(workflow: str, job: str) -> list[tuple[str | None, str]]:
    lines = workflow.splitlines()
    start = next(i for i, line in enumerate(lines) if line == f"  {job}:")
    end = next(
        (i for i in range(start + 1, len(lines)) if re.match(r"^  [\w-]+:$", lines[i])),
        len(lines),
    )
    steps: list[list[str]] = []
    current: list[str] = []
    for line in lines[start + 1 : end]:
        if line.startswith("      - "):
            if current:
                steps.append(current)
            current = [line]
        elif current:
            current.append(line)
    if current:
        steps.append(current)

    runs = []
    for step in steps:
        name_match = re.match(r"      - name: (.+)$", step[0])
        name = name_match.group(1) if name_match else None
        run_index = next((i for i, line in enumerate(step) if line.startswith("        run:")), None)
        if run_index is None:
            continue
        value = step[run_index].removeprefix("        run:").strip()
        if value in {"|", ">"}:
            value = "\n".join(line[10:] for line in step[run_index + 1 :] if line.startswith("          "))
        runs.append((name, value))
    return runs


def test_workflow_check_steps_are_single_just_targets():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    checks = lambda job: [
        (name, command)
        for name, command in _run_steps(workflow, job)
        if not (name or "").startswith("Install pinned")
    ]
    assert checks("checks") == [("Run Rust checks", "just ci-rust")]
    assert checks("analysis") == [("Run analysis checks", "just ci-analysis")]


def test_analysis_prek_helper_skips_only_cargo_format_hook(monkeypatch):
    from simciv_analysis import ci_analysis

    captured = {}

    def fake_run(command, *, env, check):
        captured.update(command=command, env=env, check=check)
        return type("Result", (), {"returncode": 0})()

    monkeypatch.setattr(ci_analysis.subprocess, "run", fake_run)
    monkeypatch.setenv("PATH", "test-path")

    assert ci_analysis.main() == 0
    assert captured["command"] == ["prek", "run", "--all-files"]
    assert captured["env"]["SKIP"] == "cargo-fmt"
    assert captured["env"]["PATH"] == "test-path"
    assert captured["check"] is False
