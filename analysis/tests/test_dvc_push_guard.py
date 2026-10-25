from pathlib import Path

from simciv_analysis.dvc_push_guard import dvc_push_violations


def _fixture(root: Path, *, status: str = "draft", default: bool = False) -> None:
    (root / "contracts").mkdir(parents=True)
    (root / "contracts/decisions.toml").write_text(
        f'[[decision]]\nid = "M5-RECOVERED-REMOTE-PRIVACY"\nstatus = "{status}"\n',
        encoding="utf-8",
    )
    (root / "docs/reproducibility").mkdir(parents=True)
    (root / "docs/reproducibility/M5_RECOVERED_PRIVATE_REMOTE_DRAFT.md").write_text(
        "## Exact manifest paths under review\n\n- `recovered/windows/target/`\n",
        encoding="utf-8",
    )
    (root / "docs/data/m5_recovered").mkdir(parents=True)
    (root / "docs/data/m5_recovered/recovered.dvc").write_text(
        "outs:\n- md5: deadbeef.dir\n  path: recovered\n",
        encoding="utf-8",
    )
    (root / ".dvc").mkdir()
    default_setting = "[core]\n    remote = dagshub\n\n" if default else "[core]\n\n"
    (root / ".dvc/config").write_text(
        default_setting
        + "['remote \"dagshub\"']\n    url = https://dagshub.com/example/project.dvc\n"
        + "['remote \"private\"']\n    url = s3://private-bucket/project\n",
        encoding="utf-8",
    )


def test_push_refuses_until_privacy_decision_is_frozen(tmp_path: Path):
    _fixture(tmp_path)
    errors = dvc_push_violations("private", tmp_path)
    assert any("status=draft" in error for error in errors)


def test_push_refuses_sensitive_paths_on_public_remote(tmp_path: Path):
    _fixture(tmp_path, status="frozen")
    errors = dvc_push_violations("dagshub", tmp_path)
    assert any("privacy-listed paths" in error for error in errors)


def test_push_allows_private_named_remote_after_final_decision(tmp_path: Path):
    _fixture(tmp_path, status="frozen")
    assert dvc_push_violations("private", tmp_path) == []


def test_push_refuses_a_default_remote(tmp_path: Path):
    _fixture(tmp_path, status="frozen", default=True)
    errors = dvc_push_violations("private", tmp_path)
    assert any("remove the default remote" in error for error in errors)
