import subprocess

from simciv_analysis.data_integrity import check, write
from simciv_analysis.io import write_text_lf


def test_staged_dvc_metadata_can_be_updated_without_changing_artifact_policy(tmp_path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)

    pipeline = tmp_path / "docs/data/stage/dvc.yaml"
    lock = tmp_path / "docs/data/stage/dvc.lock"
    write_text_lf(pipeline, "stages: {}\n")
    write_text_lf(lock, "schema: '2.0'\n")
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    message = tmp_path / "commit-message.txt"
    write_text_lf(message, "test: initial DVC metadata\n\n- add pipeline files\n")
    subprocess.run(["git", "commit", "--quiet", "-F", str(message)], cwd=tmp_path, check=True)

    write_text_lf(pipeline, "stages:\n  measure:\n    cmd: sample\n")
    write_text_lf(lock, "schema: '2.0'\nstages: {}\n")
    subprocess.run(["git", "add", "docs/data/stage/dvc.yaml", "docs/data/stage/dvc.lock"], cwd=tmp_path, check=True)
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data/SHA256SUMS"], cwd=tmp_path, check=True)

    assert check(tmp_path) == []
