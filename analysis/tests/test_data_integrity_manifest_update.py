import subprocess

from simciv_analysis.data_integrity import check, write
from simciv_analysis.io import write_text_lf


def test_staged_checksum_manifest_can_register_a_new_data_file(tmp_path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)

    write_text_lf(tmp_path / "docs/data/base.json", "{}\n")
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    message = tmp_path / "commit-message.txt"
    write_text_lf(message, "test: base manifest\n\n- add base fixture\n")
    subprocess.run(["git", "commit", "--quiet", "-F", str(message)], cwd=tmp_path, check=True)

    write_text_lf(tmp_path / "docs/data/new_stage/dvc.yaml", "stages: {}\n")
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data/SHA256SUMS", "docs/data/new_stage/dvc.yaml"], cwd=tmp_path, check=True)

    assert check(tmp_path) == []
