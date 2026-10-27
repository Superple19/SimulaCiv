import subprocess
from pathlib import Path

import pytest

from simciv_analysis.data_integrity import (
    DATA_DELETION_ALLOWLIST,
    _data_deletion_reason,
    base_range_violations,
    check,
    write,
)


def test_superseded_status_deletion_is_explicitly_allowlisted(tmp_path: Path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
    status = tmp_path / "docs/data/m5_05/STATUS.md"
    status.parent.mkdir(parents=True)
    status.write_text("superseded cache status\n", encoding="utf-8")
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data"], cwd=tmp_path, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", "base status"], cwd=tmp_path, check=True)
    base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()

    status.unlink()
    subprocess.run(["git", "add", "-u", "docs/data/m5_05/STATUS.md"], cwd=tmp_path, check=True)
    write(tmp_path)
    subprocess.run(["git", "add", "docs/data/SHA256SUMS"], cwd=tmp_path, check=True)
    assert check(tmp_path) == []
    subprocess.run(["git", "commit", "--quiet", "-m", "remove superseded status"], cwd=tmp_path, check=True)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp_path, text=True).strip()
    assert base_range_violations(tmp_path, base, head) == []
    assert _data_deletion_reason("docs/data/m5_05/STATUS.md") == DATA_DELETION_ALLOWLIST[
        "docs/data/m5_05/STATUS.md"
    ]
    assert _data_deletion_reason("docs/data/m4_03/m4_03_a1_experiments.csv") is None


def test_nonallowlisted_data_deletion_still_fails(tmp_path: Path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    path = tmp_path / "docs/data/records.json"
    path.parent.mkdir(parents=True)
    path.write_text("{}\n", encoding="utf-8")
    write(tmp_path)
    path.unlink()
    with pytest.raises(FileNotFoundError, match="refusing to drop missing non-DVC data file"):
        write(tmp_path)
