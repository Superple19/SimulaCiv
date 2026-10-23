from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


def _job_names(workflow: str) -> list[str]:
    lines = workflow.splitlines()
    jobs = lines.index("jobs:")
    return [
        match.group(1)
        for line in lines[jobs + 1 :]
        if (match := re.match(r"^  ([\w-]+):$", line))
    ]


def _steps(workflow: str, job: str) -> list[list[str]]:
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

    return steps


def _step(steps: list[list[str]], name: str) -> list[str]:
    return next(step for step in steps if step[0] == f"      - name: {name}")


def _run_steps(steps: list[list[str]]) -> list[tuple[str | None, str]]:
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
    def checks(job: str) -> list[tuple[str | None, str]]:
        return [
        (name, command)
        for name, command in _run_steps(_steps(workflow, job))
        if not (name or "").startswith("Install pinned")
        ]

    assert checks("checks") == [("Run Rust checks", "just ci-rust")]
    assert checks("analysis") == [("Run analysis checks", "just ci-analysis")]


def test_just_jobs_install_all_target_tools_before_running_them():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    jobs_with_just = {}
    for job in _job_names(workflow):
        steps = _steps(workflow, job)
        targets = [command for _, command in _run_steps(steps) if command.startswith("just ")]
        if targets:
            jobs_with_just[job] = targets
    assert jobs_with_just == {"checks": ["just ci-rust"], "analysis": ["just ci-analysis"]}

    required = {
        "checks": {
            "Install pinned Rust toolchain": "rustup component add rustfmt clippy",
        },
        "analysis": {
            "Install pinned uv": "uses: astral-sh/setup-uv@v10.3.2",
            "Install pinned Python": "uv python install",
            "Install pinned Quarto": "quarto-dev/quarto-actions/setup@v2",
            "Install pinned Vale": "vale_3.25.1_Linux_64-bit.tar.gz",
            "Install pinned prek": "releases/download/v0.5.7/prek-x86_64-unknown-linux-gnu.tar.gz",
        },
    }
    for job in jobs_with_just:
        steps = _steps(workflow, job)
        names = [
            match.group(1)
            for step in steps
            if (match := re.match(r"      - name: (.+)$", step[0]))
        ]
        target_name = next(
            name for name, command in _run_steps(steps) if command.startswith("just ci-")
        )
        target_index = names.index(target_name)
        expected_steps = {"Install pinned just": "uses: extractions/setup-just@v4", **required[job]}
        for name, expected in expected_steps.items():
            index = names.index(name)
            block = "\n".join(_step(steps, name))
            assert expected in block
            assert index < target_index
        assert 'just-version: "1.58.0"' in "\n".join(_step(steps, "Install pinned just"))

    analysis_steps = _steps(workflow, "analysis")
    assert 'version: "0.13.5"' in "\n".join(_step(analysis_steps, "Install pinned uv"))
    assert 'version: "1.10.19"' in "\n".join(_step(analysis_steps, "Install pinned Quarto"))
    assert "vale --version" in "\n".join(_step(analysis_steps, "Install pinned Vale"))
    assert "prek --version" in "\n".join(_step(analysis_steps, "Install pinned prek"))

    rust_block = "\n".join(_step(_steps(workflow, "checks"), "Install pinned Rust toolchain"))
    assert "rustup toolchain install" in rust_block


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
