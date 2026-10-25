import hashlib
import subprocess

from simciv_analysis.data_integrity import base_range_violations, write
from simciv_analysis.io import write_text_lf


def _repository_with_base_data(root):
    subprocess.run(["git", "init", "--quiet"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=root, check=True)
    write_text_lf(root / "docs/data/stage/records.json", "{}\n")
    write(root)
    subprocess.run(["git", "add", "docs/data"], cwd=root, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "base"], cwd=root, check=True)
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()


def test_existing_data_bytes_and_checksums_are_immutable_in_a_pushed_range(tmp_path):
    base = _repository_with_base_data(tmp_path)
    path = tmp_path / "docs/data/stage/records.json"
    replacement = b'{"changed":true}\n'
    path.write_bytes(replacement)
    write_text_lf(
        tmp_path / "docs/data/SHA256SUMS",
        f"{hashlib.sha256(replacement).hexdigest()}  docs/data/stage/records.json\n",
    )
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "changed data"], cwd=tmp_path, check=True)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()

    errors = base_range_violations(tmp_path, base, head)

    assert any("existing data checksum changed" in error for error in errors)
    assert any("existing data bytes changed" in error for error in errors)


def test_new_data_path_can_be_registered_in_the_pushed_range(tmp_path):
    base = _repository_with_base_data(tmp_path)
    write_text_lf(tmp_path / "docs/data/new/summary.json", '{"ok":true}\n')
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "new data"], cwd=tmp_path, check=True)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()

    assert base_range_violations(tmp_path, base, head) == []


def test_checksum_rewrite_for_an_existing_data_path_is_rejected(tmp_path):
    base = _repository_with_base_data(tmp_path)
    write_text_lf(
        tmp_path / "docs/data/SHA256SUMS",
        f"{'0' * 64}  docs/data/stage/records.json\n",
    )
    subprocess.run(["git", "add", "docs/data/SHA256SUMS"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "rewritten checksum"], cwd=tmp_path, check=True)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()

    errors = base_range_violations(tmp_path, base, head)

    assert any("existing data checksum changed" in error for error in errors)
    assert not any("existing data bytes changed" in error for error in errors)
