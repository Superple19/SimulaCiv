from simciv_analysis.action_refs import action_refs, invalid_action_refs, workflow_paths


def test_action_refs_extracts_pinned_remote_actions_only():
    workflow = """
steps:
  - uses: actions/checkout@v7.0.1
  - name: Install uv
    uses: astral-sh/setup-uv@v10.3.2 # exact release tag
  - name: Install Quarto
    uses: quarto-dev/quarto-actions/setup@v2.2.0
  - uses: ./local-action
  - uses: docker://example/image:latest
"""
    assert action_refs(workflow) == [
        ("actions/checkout", "v7.0.1"),
        ("astral-sh/setup-uv", "v10.3.2"),
        ("quarto-dev/quarto-actions", "v2.2.0"),
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
    assert [path.name for path in workflows] == ["ci.yml", "promote.yml"]
    for workflow_path in workflows:
        assert ("actions/checkout", "v7.0.1") in action_refs(
            workflow_path.read_text(encoding="utf-8")
        )
        assert invalid_action_refs(workflow_path.read_text(encoding="utf-8")) == []
