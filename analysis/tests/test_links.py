from pathlib import Path

from simciv_analysis.links import relative_target, targets


def test_link_targets_skip_fenced_code_and_external_urls(tmp_path):
    source = tmp_path / "doc.md"
    found = targets("[local](child.md) ![img](images/x.svg)\n```md\n[fake](no.md)\n```\n")
    assert found == [(1, "child.md"), (1, "images/x.svg")]
    assert relative_target(source, "child.md") == (tmp_path / "child.md").resolve()
    assert relative_target(source, "https://example.com") is None
