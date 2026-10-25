import pytest

from simciv_analysis.data_integrity import (
    _dvc_lock_manifest_violations,
    autocrlf_warning,
    check,
    data_files,
    parse_manifest,
    write,
)
from simciv_analysis.io import write_text_lf


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


def test_integrity_check_allows_documentation_readme_edits(tmp_path):
    import subprocess

    data = tmp_path / "docs/data/stage"
    data.mkdir(parents=True)
    (data / "README.md").write_text("source note\n", encoding="utf-8")
    (data / "records.json").write_text("{}\n", encoding="utf-8")
    (tmp_path / "docs/data/SHA256SUMS").write_text("", encoding="utf-8")
    write(tmp_path)

    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(
        ["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True
    )
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(
        ["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True
    )
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "fixture"], cwd=tmp_path, check=True)

    (data / "README.md").write_text("updated documentation\n", encoding="utf-8")
    assert check(tmp_path) == []


def test_dvc_lock_output_must_be_covered_by_sha256_manifest(tmp_path: Path):
    import hashlib

    stage = tmp_path / "docs/data/stage"
    stage.mkdir(parents=True)
    pipeline = stage / "dvc.yaml"
    lock = stage / "dvc.lock"
    write_text_lf(
        pipeline,
        "stages:\n  recorded:\n    outs:\n    - raw\n    meta:\n      kind: timing\n",
    )
    write_text_lf(
        lock,
        "schema: '2.0'\nstages:\n  recorded:\n    outs:\n    - path: docs/data/stage/raw\n      hash: md5\n      md5: aaaa.dir\n",
    )
    manifest = {
        "docs/data/stage/dvc.yaml": hashlib.sha256(pipeline.read_bytes()).hexdigest(),
        "docs/data/stage/dvc.lock": hashlib.sha256(lock.read_bytes()).hexdigest(),
    }

    errors = _dvc_lock_manifest_violations(tmp_path, manifest, {"docs/data/stage/raw"})

    assert "DVC lock directory output has no SHA256SUMS entries: docs/data/stage/dvc.lock: docs/data/stage/raw" in errors


def test_dvc_lock_output_without_hash_is_rejected(tmp_path: Path):
    from simciv_analysis.dvc_freeze_guard import lock_stage_outputs

    parsed = lock_stage_outputs("stages:\n  recorded:\n    outs:\n    - path: docs/data/stage/raw\n")

    assert parsed == {"recorded": {"docs/data/stage/raw": ""}}


def test_dvc_lock_output_must_belong_to_its_own_pipeline(tmp_path: Path):
    stage = tmp_path / "docs/data/stage"
    stage.mkdir(parents=True)
    write_text_lf(stage / "dvc.yaml", "stages:\n  recorded:\n    outs:\n    - raw\n")
    write_text_lf(
        stage / "dvc.lock",
        "schema: '2.0'\nstages:\n  recorded:\n    outs:\n    - path: docs/data/other/raw\n      hash: md5\n      md5: aaaa.dir\n",
    )

    errors = _dvc_lock_manifest_violations(
        tmp_path, {}, {"docs/data/stage/raw", "docs/data/other/raw"}
    )

    assert "DVC lock output is not declared by this pipeline: docs/data/stage/dvc.lock: docs/data/other/raw" in errors


def test_base_range_rejects_changed_output_hash_for_frozen_stage(tmp_path: Path):
    import hashlib
    import subprocess

    from simciv_analysis.data_integrity import base_range_violations

    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
    stage = tmp_path / "docs/data/stage"
    raw = stage / "raw"
    raw.mkdir(parents=True)
    pipeline = stage / "dvc.yaml"
    lock = stage / "dvc.lock"
    raw_file = raw / "record.csv"
    write_text_lf(pipeline, "stages:\n  timed:\n    frozen: true\n    outs:\n    - raw\n    meta:\n      kind: timing\n")
    write_text_lf(lock, "schema: '2.0'\nstages:\n  timed:\n    outs:\n    - path: docs/data/stage/raw\n      hash: md5\n      md5: aaaa.dir\n")
    write_text_lf(raw_file, "value\n")
    manifest = tmp_path / "docs/data/SHA256SUMS"
    manifest.parent.mkdir(parents=True, exist_ok=True)

    def write_checksums():
        paths = [pipeline, lock, raw_file]
        write_text_lf(manifest, "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(tmp_path).as_posix()}\n"
            for path in paths
        ))

    write_checksums()
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "base"], cwd=tmp_path, check=True)
    base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()
    write_text_lf(lock, lock.read_text(encoding="utf-8").replace("aaaa.dir", "bbbb.dir"))
    write_checksums()
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "change locked output"], cwd=tmp_path, check=True)
    write_text_lf(lock, lock.read_text(encoding="utf-8").replace("bbbb.dir", "aaaa.dir"))
    write_checksums()
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "restore locked output"], cwd=tmp_path, check=True)

    assert subprocess.run(
        ["git", "diff", "--quiet", f"{base}..HEAD", "--", "docs/data/stage/dvc.lock"], cwd=tmp_path
    ).returncode == 0

    errors = base_range_violations(tmp_path, base)

    assert any("frozen output hashes changed" in error for error in errors)
