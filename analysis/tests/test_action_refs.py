import subprocess

import simciv_analysis.action_refs as action_refs_module
from simciv_analysis.action_refs import action_refs, invalid_action_refs, workflow_paths
from simciv_analysis.action_refs import _lookup_action_tag


def test_action_refs_extracts_pinned_remote_actions_only():
    workflow = """
steps:
  - uses: actions/checkout@v7.0.1
  - name: Install mise
    uses: jdx/mise-action@v5.2.2 # exact release tag
  - uses: ./local-action
  - uses: docker://example/image:latest
"""
    assert action_refs(workflow) == [
        ("actions/checkout", "v7.0.1"),
        ("jdx/mise-action", "v5.2.2"),
    ]


def test_only_full_semver_action_tags_are_accepted():
    workflow = """
steps:
  - uses: actions/checkout@v7
  - uses: quarto-dev/quarto-actions/setup@v2
  - uses: astral-sh/setup-uv@v10.3.2
"""
    assert invalid_action_refs(workflow) == [
        ("actions/checkout", "v7"),
        ("quarto-dev/quarto-actions", "v2"),
    ]
    assert invalid_action_refs("uses: astral-sh/setup-uv@v10.3.2") == []


def test_action_workflows_are_discovered_by_glob_and_use_exact_tags():
    workflows = workflow_paths()
    assert [path.name for path in workflows] == ["ci.yml"]
    for workflow_path in workflows:
        assert ("actions/checkout", "v7.0.1") in action_refs(
            workflow_path.read_text(encoding="utf-8")
        )
        assert ("jdx/mise-action", "v5.2.2") in action_refs(
            workflow_path.read_text(encoding="utf-8")
        )
        assert invalid_action_refs(workflow_path.read_text(encoding="utf-8")) == []


def test_action_tag_lookup_retries_network_errors_then_succeeds(monkeypatch):
    attempts = iter([
        subprocess.CompletedProcess([], 128, "", "network unavailable"),
        subprocess.CompletedProcess([], 128, "", "connection reset"),
        subprocess.CompletedProcess([], 0, "abc123\trefs/tags/v1.2.3\n", ""),
    ])
    sleeps = []
    monkeypatch.setattr(action_refs_module.subprocess, "run", lambda *args, **kwargs: next(attempts))
    monkeypatch.setattr(action_refs_module.time, "sleep", sleeps.append)

    assert _lookup_action_tag("actions/checkout", "v1.2.3") == ("found", "")
    assert sleeps == [0.25, 0.5]


def test_action_tag_lookup_reports_final_network_error_separately(monkeypatch):
    attempts = []
    sleeps = []

    def fail(*args, **kwargs):
        attempts.append(1)
        return subprocess.CompletedProcess([], 128, "", "network unavailable")

    monkeypatch.setattr(action_refs_module.subprocess, "run", fail)
    monkeypatch.setattr(action_refs_module.time, "sleep", sleeps.append)

    assert _lookup_action_tag("actions/checkout", "v1.2.3") == ("network error", "network unavailable")
    assert len(attempts) == 3
    assert sleeps == [0.25, 0.5]


def test_action_tag_lookup_distinguishes_a_missing_tag_without_retry(monkeypatch):
    attempts = []
    sleeps = []

    def missing(*args, **kwargs):
        attempts.append(1)
        return subprocess.CompletedProcess([], 2, "", "")

    monkeypatch.setattr(action_refs_module.subprocess, "run", missing)
    monkeypatch.setattr(action_refs_module.time, "sleep", sleeps.append)

    assert _lookup_action_tag("actions/checkout", "v1.2.3") == ("tag does not exist", "")
    assert len(attempts) == 1
    assert sleeps == []
