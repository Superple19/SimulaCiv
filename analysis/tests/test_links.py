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


def test_main_rejects_links_to_dvc_outputs_even_when_present(tmp_path, monkeypatch, capsys):
    source = tmp_path / "docs" / "reports" / "note.md"
    target = tmp_path / "docs" / "data" / "stage" / "raw.json"
    source.parent.mkdir(parents=True)
    target.parent.mkdir(parents=True)
    source.write_text("[raw output](../data/stage/raw.json)\n", encoding="utf-8")
    target.write_text("{}\n", encoding="utf-8")
    monkeypatch.setattr(links, "ROOT", tmp_path)
    monkeypatch.setattr(links, "dvc_output_paths", lambda _root: {"docs/data/stage/raw.json"})

    assert links.main(["docs/reports/note.md"]) == 1
    output = capsys.readouterr().out
    assert "DVC-tracked output is unavailable in a clean checkout" in output
    assert "link to the stage dvc.yaml/README entry" in output
