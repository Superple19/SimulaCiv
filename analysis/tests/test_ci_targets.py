from pathlib import Path
import re
import os
import shutil
import subprocess
import sys



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
            if not (name or "").startswith(("Install pinned", "Show analysis Python version"))
        ]

    assert checks("checks") == [("Run Rust checks", "just ci-rust")]
    assert checks("analysis") == [("Run analysis checks", "just ci-analysis")]
    check_target = next(
        line
        for line in (ROOT / "justfile").read_text(encoding="utf-8").splitlines()
        if line.startswith("check:")
    )
    assert check_target.startswith("check: check-quarto tool-versions-check check-tools analysis-test")
    assert "commit-messages-check" in check_target
    assert "fetch-depth: 0" in "\n".join(_steps(workflow, "analysis")[0])


def test_workflows_use_current_ga_runner_images():
    ci = WORKFLOW.read_text(encoding="utf-8")
    runner_labels = re.findall(r"^\s+- (ubuntu-\S+|windows-\S+|macos-\S+)$", ci, re.MULTILINE)
    assert runner_labels == ["ubuntu-26.04", "windows-2025", "macos-26"]
    assert "pinned GA labels are x64 for Ubuntu/Windows and arm64 for macOS" in ci
    assert "runs-on: ubuntu-26.04" in ci
    assert all(label not in ci for label in ("ubuntu-latest", "windows-latest", "macos-latest"))


def test_workflow_promotion_shell_is_fast_forward_only_in_a_temporary_repo(tmp_path):
    bash = shutil.which("bash")
    if sys.platform == "win32":
        git_bash = Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Git" / "bin" / "bash.exe"
        if git_bash.is_file():
            bash = str(git_bash)
    assert bash is not None, "the hosted runner images provide Git Bash/Bash for the promotion test"

    origin = tmp_path / "origin.git"
    checkout = tmp_path / "checkout"
    subprocess.run(["git", "init", "--quiet", "--bare", "--object-format=sha1", "--initial-branch=main", str(origin)], check=True)
    subprocess.run(["git", "init", "--quiet", "--object-format=sha1", "--initial-branch=main", str(checkout)], check=True)
    for name, value in (("user.name", "Promotion Test"), ("user.email", "promotion@example.invalid")):
        subprocess.run(["git", "config", name, value], cwd=checkout, check=True)
    (checkout / "state.txt").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "add", "state.txt"], cwd=checkout, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "base"], cwd=checkout, check=True)
    base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    subprocess.run(["git", "remote", "add", "origin", str(origin)], cwd=checkout, check=True)
    subprocess.run(["git", "push", "--quiet", "-u", "origin", "main"], cwd=checkout, check=True)

    (checkout / "state.txt").write_text("tested\n", encoding="utf-8")
    subprocess.run(["git", "commit", "--quiet", "-am", "tested"], cwd=checkout, check=True)
    tested = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    subprocess.run(["git", "push", "--quiet", "origin", "HEAD:ci-check"], cwd=checkout, check=True)

    workflow = WORKFLOW.read_text(encoding="utf-8")
    script = _run_steps(_steps(workflow, "promote"))[0][1]

    def promote(sha: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [bash, "-e", "-c", script],
            cwd=checkout,
            env=os.environ | {"TESTED_SHA": sha},
            capture_output=True,
            text=True,
            check=False,
        )

    successful = promote(tested)
    assert successful.returncode == 0, f"tested SHA length={len(tested)} {tested}: {successful.stderr}"
    main_after = subprocess.check_output(["git", "--git-dir", str(origin), "rev-parse", "refs/heads/main"], text=True).strip()
    assert main_after == tested

    overlapping = promote(base)
    assert overlapping.returncode == 0, overlapping.stderr
    assert "already an ancestor" in overlapping.stdout

    subprocess.run(["git", "checkout", "--quiet", "--detach", base], cwd=checkout, check=True)
    (checkout / "state.txt").write_text("sibling\n", encoding="utf-8")
    subprocess.run(["git", "commit", "--quiet", "-am", "sibling"], cwd=checkout, check=True)
    sibling = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    subprocess.run(["git", "push", "--quiet", "--force", "origin", "HEAD:ci-check"], cwd=checkout, check=True)
    rejected = promote(sibling)
    assert rejected.returncode != 0
    assert "not a descendant of origin/main" in rejected.stderr
    assert subprocess.check_output(["git", "--git-dir", str(origin), "rev-parse", "refs/heads/main"], text=True).strip() == tested


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
    checks_start = workflow.index("  checks:")
    analysis_start = workflow.index("  analysis:")
    checks_job = workflow[checks_start:analysis_start]
    analysis_job = workflow[analysis_start:]
    assert 'MISE_ENABLE_TOOLS: "aqua:casey/just"' in checks_job
    assert "MISE_ENABLE_TOOLS" not in analysis_job
    jobs_with_just = {}
    for job in _job_names(workflow):
        steps = _steps(workflow, job)
        targets = [command for _, command in _run_steps(steps) if command.startswith("just ")]
        if targets:
            jobs_with_just[job] = targets
    assert jobs_with_just == {"checks": ["just ci-rust"], "analysis": ["just ci-analysis"]}

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
        mise_index = names.index("Install mise-managed tools")
        assert "uses: jdx/mise-action@v5.2.2" in "\n".join(_step(steps, "Install mise-managed tools"))
        assert mise_index < target_index
        assert "version: ${{ env.MISE_VERSION }}" in "\n".join(_step(steps, "Install mise-managed tools"))
        if job == "analysis":
            assert names.index("Install pinned Rust toolchain") < target_index

    analysis_steps = _steps(workflow, "analysis")
    analysis_block = "\n".join("\n".join(step) for step in analysis_steps)
    assert "rustup toolchain install" in "\n".join(_step(analysis_steps, "Install pinned Rust toolchain"))
    assert "uv python install --directory analysis" in analysis_block
    python_step = "uv run --directory analysis --project . python -c \"import sys; print(sys.version)\""
    assert python_step in analysis_block
    analysis_names = [step[0] for step in analysis_steps]
    assert analysis_names.index("      - name: Show analysis Python version") < analysis_names.index("      - name: Run analysis checks")
    assert "setup-uv" not in workflow
    assert "quarto-actions" not in workflow
    assert "curl -fsSL" not in workflow

    checks = _steps(workflow, "checks")
    rust_setup = "\n".join(_step(checks, "Install pinned Rust toolchain"))
    assert "rustup toolchain install" in rust_setup
    assert "component add" not in workflow
    assert "target add" not in workflow
    rust_toolchain = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    assert 'components = ["rustfmt", "clippy"]' in rust_toolchain
    assert 'targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]' in rust_toolchain


def test_rust_checks_use_the_locked_dependency_graph():
    justfile = JUSTFILE.read_text(encoding="utf-8")
    hooks = (ROOT / ".pre-commit-config.yaml").read_text(encoding="utf-8")
    ci_rust = justfile.split("ci-rust:", 1)[1].split("\n\n", 1)[0]
    cross = justfile.split("clippy-cross:", 1)[1].split("\n\n", 1)[0]
    assert "cargo clippy --locked" in ci_rust
    assert "cargo test --locked" in ci_rust
    assert cross.count("cargo clippy --locked") == 2
    assert "entry: cargo clippy --locked --workspace" in hooks
    assert "entry: cargo test --locked --workspace" in hooks


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
    assert "${#TESTED_SHA} -ne 40" in commands
    assert "*[!0-9a-f]*)" in commands
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
