from pathlib import Path
import re

from simciv_analysis.tool_versions import tool_version


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
JUSTFILE = ROOT / "justfile"


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
    assert "branches: [main, ci-check]" in workflow
    assert "pull_request:" in workflow
    assert "workflow_dispatch:" in workflow
    def checks(job: str) -> list[tuple[str | None, str]]:
        return [
        (name, command)
        for name, command in _run_steps(_steps(workflow, job))
        if not (name or "").startswith("Install pinned")
        ]

    assert checks("checks") == [("Run Rust checks", "just ci-rust")]
    assert checks("analysis") == [("Run analysis checks", "just ci-analysis")]
    check_target = next(
        line
        for line in (ROOT / "justfile").read_text(encoding="utf-8").splitlines()
        if line.startswith("check:")
    )
    assert check_target.startswith("check: check-quarto tool-versions-check analysis-test")
    assert "commit-messages-check" in check_target
    assert "fetch-depth: 0" in "\n".join(_steps(workflow, "analysis")[0])


def test_workflows_use_current_ga_runner_images():
    ci = WORKFLOW.read_text(encoding="utf-8")
    assert "- ubuntu-26.04" in ci
    assert "- windows-2025" in ci
    assert "- macos-26" in ci
    assert "runs-on: ubuntu-26.04" in ci
    assert all(label not in ci for label in ("ubuntu-latest", "windows-latest", "macos-latest"))


def test_quarto_preflight_precedes_checks_and_report_commands():
    justfile = JUSTFILE.read_text(encoding="utf-8")
    assert "check-quarto:" in justfile
    for target in (
        "report-m5-01:",
        "report-m5-01-check:",
        "report-m5-04:",
        "report-m5-04-check:",
        "report-m5-05:",
        "report-m5-05-check:",
        "check:",
        "ci:",
    ):
        line = next(line for line in justfile.splitlines() if line.startswith(target))
        assert "check-quarto" in line


def test_ship_only_checks_cleanliness_and_pushes_to_ci_check():
    justfile = JUSTFILE.read_text(encoding="utf-8")
    ship_line = next(line for line in justfile.splitlines() if line.startswith("ship:"))
    assert ship_line == "ship: analysis-sync"
    ship_recipe = justfile.split("ship: analysis-sync", 1)[1].split("\n\n", 1)[0]
    assert "python -m simciv_analysis.ship" in ship_recipe
    assert "just ci" not in ship_recipe
    assert "ship: ci" not in justfile


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
            "Install pinned Vale": f"vale_{tool_version('vale')}_Linux_64-bit.tar.gz",
            "Install pinned prek": f"releases/download/v{tool_version('prek')}/prek-x86_64-unknown-linux-gnu.tar.gz",
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
    assert f'just-version: "{tool_version("just")}"' in "\n".join(_step(steps, "Install pinned just"))

    analysis_steps = _steps(workflow, "analysis")
    assert f'version: "{tool_version("uv")}"' in "\n".join(_step(analysis_steps, "Install pinned uv"))
    assert f'version: "{tool_version("quarto")}"' in "\n".join(_step(analysis_steps, "Install pinned Quarto"))
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


def test_promotion_job_is_ci_check_push_only_fast_forward_and_needs_all_checks():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    assert _job_names(workflow) == ["checks", "analysis", "promote"]
    assert "permissions:\n  contents: read" in workflow
    assert "workflow_run:" not in workflow
    assert "workflow_dispatch:" in workflow
    assert "needs: [checks, analysis]" in workflow
    assert "github.event_name == 'push'" in workflow
    assert "github.ref == 'refs/heads/ci-check'" in workflow
    assert "needs.checks.result == 'success'" in workflow
    assert "needs.analysis.result == 'success'" in workflow

    steps = _steps(workflow, "promote")
    job_start = workflow.index("  promote:")
    job = workflow[job_start:]
    assert "    concurrency:\n      group: promote\n      cancel-in-progress: false" in job
    assert "    permissions:\n      contents: write" in job
    assert "actions: write" not in workflow
    checkout = "\n".join(_step(steps, "Check out full history at tested commit"))
    assert "uses: actions/checkout@v7" in checkout
    assert "ref: ${{ github.sha }}" in checkout
    assert "fetch-depth: 0" in checkout
    runs = _run_steps(steps)
    assert [name for name, _ in runs] == ["Fast-forward main"]
    commands = runs[0][1]
    assert 'git merge-base --is-ancestor "$main_sha" "$tested_sha"' in commands
    assert 'git push origin "$tested_sha:refs/heads/main"' in commands
    assert 'if [[ "$main_sha" == "$tested_sha" ]]' in commands
    assert commands.index("git merge-base") < commands.index("git push")
    assert commands.index('if [[ "$main_sha" == "$tested_sha" ]]') < commands.index("git push")
    assert "--force" not in commands
    assert "gh " not in commands
    assert re.findall(r"\b(git|gh)\s+([\w-]+)", commands) == [
        ("git", "fetch"),
        ("git", "rev-parse"),
        ("git", "rev-parse"),
        ("git", "merge-base"),
        ("git", "merge-base"),
        ("git", "push"),
    ]
    assert "gh workflow run" not in commands
    for job_name in ("checks", "analysis"):
        commands = [command for _, command in _run_steps(_steps(workflow, job_name))]
        assert all(not re.search(r"\b(?:git|gh)\s+", command) for command in commands)
    assert "gh workflow run" not in workflow


def test_promotion_job_skips_already_promoted_overlapping_runs():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    commands = _run_steps(_steps(workflow, "promote"))[0][1]
    already_promoted = 'if git merge-base --is-ancestor "$tested_sha" "$main_sha"; then'
    non_descendant = 'if ! git merge-base --is-ancestor "$main_sha" "$tested_sha"; then'
    assert already_promoted in commands
    assert 'skipping older overlapping run' in commands
    assert commands.index(already_promoted) < commands.index(non_descendant) < commands.index("git push")
