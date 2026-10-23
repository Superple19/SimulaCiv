import pytest

from simciv_analysis.data_integrity import check, data_files, parse_manifest, write


def test_manifest_parser_and_data_file_scope(tmp_path):
    data = tmp_path / "docs/data/stage"
    data.mkdir(parents=True)
    (data / "records.json").write_text("{}\n", encoding="utf-8")
    (data / "README.md").write_text("readme\n", encoding="utf-8")
    (tmp_path / "docs/data/SHA256SUMS").write_text("", encoding="utf-8")

    assert set(data_files(tmp_path)) == {"docs/data/stage/records.json"}
    assert parse_manifest("a" * 64 + "  docs/data/stage/records.json\n") == {
        "docs/data/stage/records.json": "a" * 64
    }


def test_integrity_check_detects_changed_and_unregistered_files(tmp_path):
    import subprocess

    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    data = tmp_path / "docs/data/stage"
    data.mkdir(parents=True)
    record = data / "records.json"
    record.write_text("{}\n", encoding="utf-8")

    write(tmp_path)
    assert check(tmp_path) == []

    record.write_text('{"changed": true}\n', encoding="utf-8")
    assert any("checksum mismatch" in error for error in check(tmp_path))
    with pytest.raises(ValueError, match="refusing to change existing data checksum"):
        write(tmp_path)

    record.write_text("{}\n", encoding="utf-8")
    (data / "new.json").write_text("[]\n", encoding="utf-8")
    assert any("unregistered data file" in error for error in check(tmp_path))
