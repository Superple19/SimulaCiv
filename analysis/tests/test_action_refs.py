from simciv_analysis.action_refs import action_refs


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
