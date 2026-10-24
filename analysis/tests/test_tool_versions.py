import shutil
from pathlib import Path

from simciv_analysis.tool_versions import (
    check_tool_versions,
    hardcoded_tool_versions,
    load_tool_versions,
    mise_version,
    tool_version,
)
from simciv_analysis.io import write_text_lf


ROOT = Path(__file__).resolve().parents[2]


def test_mise_config_and_lock_are_canonical_for_all_platforms():
    tools = load_tool_versions()
    assert set(tools) == {"uv", "python", "just", "vale", "prek", "quarto", "dvc"}
    assert tools["python"]["version"] == tool_version("python")
    assert tools["dvc"]["python"]
    assert len(mise_version().split(".")) == 3
    assert check_tool_versions() == []


def test_scan_rejects_another_hardcoded_pin_but_ignores_requirements_and_actions():
    bad_version = ".".join(("0", "12", "10"))
    findings = hardcoded_tool_versions(f"Install uv {bad_version}", "README.md")
    assert len(findings) == 1
    assert "README.md" in findings[0] and bad_version in findings[0]
    assert hardcoded_tool_versions(f"Python >={'.'.join(('3', '11'))}", "README.md") == []
    assert hardcoded_tool_versions("uses: astral-sh/setup-uv@v10.3.2", ".github/workflows/ci.yml") == []


def test_tool_version_check_reports_uv_drift_in_its_source_file(tmp_path, monkeypatch):
    files = (
        "mise.toml",
        "mise.lock",
        "uv.toml",
        ".python-version",
        ".github/workflows/ci.yml",
        "analysis/README.md",
        "docs/data/README.md",
        "docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md",
    )
    for relative in files:
        destination = tmp_path / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, destination)
    shutil.copytree(ROOT / ".mise/locks", tmp_path / ".mise/locks")
    write_text_lf(tmp_path / "uv.toml", 'required-version = "==9.9.9"\n')
    monkeypatch.setattr("simciv_analysis.tool_versions._tracked_paths", lambda _: [])

    errors = check_tool_versions(tmp_path)
    assert any("uv.toml" in error and "9.9.9" in error and tool_version("uv") in error for error in errors)
