from pathlib import Path

import simciv_analysis.links as links
from simciv_analysis.links import relative_target, targets


def test_link_targets_skip_fenced_code_and_external_urls(tmp_path):
    source = tmp_path / "doc.md"
    found = targets("[local](child.md) ![img](images/x.svg)\n```md\n[fake](no.md)\n```\n")
    assert found == [(1, "child.md"), (1, "images/x.svg")]
    assert relative_target(source, "child.md") == (tmp_path / "child.md").resolve()
    assert relative_target(source, "https://example.com") is None


def test_main_checks_only_supplied_qmd_files(tmp_path, monkeypatch, capsys):
    source = tmp_path / "note.qmd"
    source.write_text("[broken](missing.md)\n", encoding="utf-8")
    monkeypatch.setattr(links, "ROOT", tmp_path)

    assert links.main(["note.qmd"]) == 1
    assert "note.qmd:1: missing relative target: missing.md" in capsys.readouterr().out
