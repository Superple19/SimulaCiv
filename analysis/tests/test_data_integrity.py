import pytest

from simciv_analysis.data_integrity import (
    autocrlf_warning,
    check,
    data_files,
    parse_manifest,
    write,
)


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
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
    assert autocrlf_warning(tmp_path) is None
    subprocess.run(["git", "config", "--local", "core.autocrlf", "true"], cwd=tmp_path, check=True)
    assert autocrlf_warning(tmp_path)
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    data = tmp_path / "docs/data/stage"
    data.mkdir(parents=True)
    record = data / "records.json"
    record.write_bytes(b"{}\n")
    subprocess.run(["git", "add", "docs/data/stage/records.json"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "fixture"], cwd=tmp_path, check=True)

    write(tmp_path)
    assert check(tmp_path) == []

    record.write_bytes(b"{}\r\n")
    assert any("working-tree bytes differ from Git blob" in error for error in check(tmp_path))
    with pytest.raises(ValueError, match="working-tree bytes differ from Git blob"):
        write(tmp_path)

    record.write_bytes(b"{}\n")
    (data / "new.json").write_bytes(b"[]\n")
    assert any("unregistered data file" in error for error in check(tmp_path))
    write(tmp_path)
    assert check(tmp_path) == []
