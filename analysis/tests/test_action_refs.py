from simciv_analysis.action_refs import WORKFLOWS, action_refs


def test_action_refs_extracts_pinned_remote_actions_only():
    workflow = """
steps:
  - uses: actions/checkout@v7
  - name: Install uv
    uses: astral-sh/setup-uv@v10.3.2 # exact release tag
  - name: Install Quarto
    uses: quarto-dev/quarto-actions/setup@v2
  - uses: ./local-action
  - uses: docker://example/image:latest
"""
    assert action_refs(workflow) == [
        ("actions/checkout", "v7"),
        ("astral-sh/setup-uv", "v10.3.2"),
        ("quarto-dev/quarto-actions", "v2"),
    ]


def test_ci_and_promotion_workflows_have_pinned_checkout_actions():
    assert [path.name for path in WORKFLOWS] == ["ci.yml", "promote.yml"]
    for workflow_path in WORKFLOWS:
        assert ("actions/checkout", "v7") in action_refs(
            workflow_path.read_text(encoding="utf-8")
        )
