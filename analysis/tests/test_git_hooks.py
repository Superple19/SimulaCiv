from pathlib import Path

from simciv_analysis.git_hooks import (
    commit_message_error,
    commit_range_violations,
    dvc_output_paths,
    large_file_range_violations,
    large_file_violations,
)


def test_commit_message_accepts_messages_with_bullet_bodies():
    known = """ci: promote green ci-check commits to main

- run CI on ci-check and fast-forward successful tested commits to main
- dispatch CI on main after promotion and guard against non-descendant SHAs
- validate action tags and promotion workflow behavior in analysis checks
- add just ship and document normal delivery and same-run promotion
"""
    assert commit_message_error(known) is None
    assert commit_message_error("docs: short subject\n\n- describe the documentation change\n") is None
    assert commit_message_error("docs: short subject\n# editor comment\n\n- describe the change\n") is None


def test_commit_message_rejects_everything_outside_the_allowlist():
    bad_messages = [
        "docs: subject only\n",
        "docs: subject with editor comment\n# comment\n",
        "docs: generated registry tables\n\n- generate contract and README tables\n\n- add generated-table checks\n",
        "fix: reject stray token\n\n- command used -m twice\n",
        "docs: explain result\n\nThis is prose, not a bullet.\n",
        "docs: wrapped bullet\n\n- first line\n  continuation\n",
        "docs: indented bullet\n\n - not allowed\n",
        "docs: star bullet\n\n* not allowed\n",
        "docs: blank gap\n\n\n- not allowed\n",
        "docs: trailing space\n\n- bad \n",
        "feat: " + "x" * 68,
        "docs: long body\n\n- " + "x" * 99,
        "docs: trailing blank\n\n- bullet\n\n",
    ]
    for message in bad_messages:
        assert commit_message_error(message) is not None, message


def test_commit_range_checker_reads_exact_committed_messages(tmp_path):
    import os
    import subprocess

    def git(*args, input_bytes=None):
        return subprocess.check_output(["git", *args], cwd=tmp_path, input=input_bytes).decode().strip()

    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
    environment = os.environ | {
        "GIT_AUTHOR_NAME": "Test",
        "GIT_AUTHOR_EMAIL": "test@example.com",
        "GIT_COMMITTER_NAME": "Test",
        "GIT_COMMITTER_EMAIL": "test@example.com",
    }
    tree = git("mktree", input_bytes=b"")
    base = subprocess.check_output(
        ["git", "commit-tree", tree], cwd=tmp_path, input=b"test: base\n", env=environment
    ).decode().strip()
    invalid = subprocess.check_output(
        ["git", "commit-tree", tree, "-p", base],
        cwd=tmp_path,
        input=b"docs: bad body\n\n- one\n\n- two\n",
        env=environment,
    ).decode().strip()

    assert commit_range_violations(tmp_path, base, invalid) == [
        (invalid, "body line 4 must be a contiguous '- <bullet>' line")
    ]


def test_commit_range_comes_from_the_github_push_event(tmp_path):
    import json

    from simciv_analysis.git_hooks import _commit_range

    event_path = tmp_path / "event.json"
    event_path.write_bytes(json.dumps({"before": "base-sha", "after": "head-sha"}).encode())
    environment = {"GITHUB_EVENT_NAME": "push", "GITHUB_EVENT_PATH": str(event_path)}

    assert _commit_range(tmp_path, environment) == ("base-sha", "head-sha")
    assert _commit_range(tmp_path, {"GITHUB_EVENT_NAME": "workflow_dispatch"}) is None


def test_ship_refuses_dirty_trees_and_pushes_clean_trees(monkeypatch, capsys):
    from types import SimpleNamespace

    from simciv_analysis import ship

    calls = []
    outputs = iter((SimpleNamespace(returncode=0, stdout=" M README.md\n", stderr=""),))
    monkeypatch.setattr(ship.subprocess, "run", lambda *args, **kwargs: (calls.append(args[0]), next(outputs))[1])
    assert ship.main() == 1
    assert calls == [["git", "status", "--porcelain", "--untracked-files=all"]]
    assert "working tree" in capsys.readouterr().err

    calls.clear()
    outputs = iter(
        (
            SimpleNamespace(returncode=0, stdout="", stderr=""),
            SimpleNamespace(returncode=0),
        )
    )
    monkeypatch.setattr(ship.subprocess, "run", lambda *args, **kwargs: (calls.append(args[0]), next(outputs))[1])
    assert ship.main() == 0
    assert calls == [
        ["git", "status", "--porcelain", "--untracked-files=all"],
        ["git", "push", "origin", "HEAD:ci-check"],
    ]


def test_large_file_check_allows_dvc_output_and_ignores_unstaged(tmp_path):
    output = tmp_path / "docs/data/sample/raw"
    output.mkdir(parents=True)
    (output / "large.bin").write_bytes(b"x" * (1024 * 1024 + 1))
    outside = tmp_path / "large.bin"
    outside.write_bytes(b"x" * (1024 * 1024 + 1))

    violations = large_file_violations(
        tmp_path,
        ["docs/data/sample/raw/large.bin", "large.bin"],
        {"docs/data/sample/raw/large.bin", "large.bin"},
        {"docs/data/sample/raw"},
    )

    assert violations == [("large.bin", 1024 * 1024 + 1)]
    assert large_file_violations(tmp_path, ["large.bin"], set(), set()) == []


def test_large_file_check_uses_committed_pushed_range(tmp_path):
    import subprocess

    def git(*args):
        return subprocess.check_output(["git", *args], cwd=tmp_path).decode().strip()

    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
    (tmp_path / "small.txt").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "add", "small.txt"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "base"], cwd=tmp_path, check=True)
    base = git("rev-parse", "HEAD")
    (tmp_path / "large.bin").write_bytes(b"x" * (1024 * 1024 + 1))
    subprocess.run(["git", "add", "large.bin"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "large"], cwd=tmp_path, check=True)
    head = git("rev-parse", "HEAD")

    assert large_file_range_violations(tmp_path, base, head) == [
        ("large.bin", 1024 * 1024 + 1)
    ]


def test_large_file_range_checks_intermediate_commits_not_only_net_diff(tmp_path):
    import subprocess

    def git(*args):
        return subprocess.check_output(["git", *args], cwd=tmp_path).decode().strip()

    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
    (tmp_path / "small.txt").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "add", "small.txt"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "base"], cwd=tmp_path, check=True)
    base = git("rev-parse", "HEAD")

    (tmp_path / "large.bin").write_bytes(b"x" * (1024 * 1024 + 1))
    subprocess.run(["git", "add", "large.bin"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "add large file"], cwd=tmp_path, check=True)
    (tmp_path / "large.bin").unlink()
    subprocess.run(["git", "commit", "--quiet", "-am", "remove large file"], cwd=tmp_path, check=True)
    head = git("rev-parse", "HEAD")

    assert subprocess.run(
        ["git", "diff", "--quiet", f"{base}..{head}", "--", "large.bin"], cwd=tmp_path
    ).returncode == 0
    assert large_file_range_violations(tmp_path, base, head) == [
        ("large.bin", 1024 * 1024 + 1)
    ]


def test_pushed_range_guards_are_wired_into_check_and_pre_push():
    root = Path(__file__).resolve().parents[2]
    justfile = (root / "justfile").read_text(encoding="utf-8")
    hooks = (root / ".pre-commit-config.yaml").read_text(encoding="utf-8")
    check = next(line for line in justfile.splitlines() if line.startswith("check:"))

    assert all(target in check for target in (
        "large-file-range-check", "data-integrity-range-check", "gitleaks-range-check"
    ))
    assert all(hook in hooks for hook in (
        "gitleaks-pushed-range", "pre-push-large-file-range", "pre-push-data-integrity-range"
    ))


def test_ci_check_includes_gitleaks_and_pushed_range_guards():
    from pathlib import Path

    root = Path(__file__).resolve().parents[2]
    justfile = (root / "justfile").read_text(encoding="utf-8")
    config = (root / ".pre-commit-config.yaml").read_text(encoding="utf-8")
    check_line = next(line for line in justfile.splitlines() if line.startswith("check:"))

    assert "large-file-range-check" in check_line
    assert "data-integrity-range-check" in check_line
    assert "gitleaks-range-check" in check_line
    assert "gitleaks-pushed-range" in config
    assert "origin/main..HEAD" in config
    assert "stages: [pre-push]" in config


def test_dvc_output_paths_read_pipeline_wdir(tmp_path):
    pipeline = tmp_path / "docs/data/stage/dvc.yaml"
    pipeline.parent.mkdir(parents=True)
    pipeline.write_text(
        "stages:\n  sample:\n    wdir: ../../../\n    outs:\n    - docs/data/stage/raw\n",
        encoding="utf-8",
    )

    assert "docs/data/stage/raw" in dvc_output_paths(tmp_path)
