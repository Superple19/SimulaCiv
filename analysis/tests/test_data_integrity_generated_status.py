import subprocess
from pathlib import Path

import pytest

from simciv_analysis.data_integrity import base_range_violations, check, write


def _commit(root: Path, message: str) -> str:
    subprocess.run(["git", "add", "docs/data"], cwd=root, check=True)
    subprocess.run(["git", "commit", "--quiet", "-m", message], cwd=root, check=True)
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()


def test_generated_status_must_be_staged_and_can_refresh_its_digest(tmp_path: Path):
    subprocess.run(["git", "init", "--quiet"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "--local", "core.autocrlf", "false"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=tmp_path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=tmp_path, check=True)
    status = tmp_path / "docs/data/m5_05/STATUS.md"
    status.parent.mkdir(parents=True)
    status.write_text("initial local status\n", encoding="utf-8")
    write(tmp_path)
    base = _commit(tmp_path, "base status")

    status.write_text("refreshed local status\n", encoding="utf-8")
    assert any("working-tree bytes differ from Git blob" in error for error in check(tmp_path))
    with pytest.raises(ValueError, match="working-tree bytes differ from Git blob"):
        write(tmp_path)

    subprocess.run(["git", "add", "docs/data/m5_05/STATUS.md"], cwd=tmp_path, check=True)
    write(tmp_path)
    head = _commit(tmp_path, "refresh generated status")

    assert check(tmp_path) == []
    assert base_range_violations(tmp_path, base, head) == []
