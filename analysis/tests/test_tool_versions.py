from pathlib import Path
import shutil

from simciv_analysis.io import write_text_lf
from simciv_analysis.tool_versions import check_repository_pins, load_tool_versions


ROOT = Path(__file__).resolve().parents[2]


def test_tool_versions_are_canonical_and_all_install_pins_match():
    tools = load_tool_versions()
    assert set(tools) == {"uv", "python", "quarto", "just", "vale", "prek", "dvc"}
    assert check_repository_pins() == []


def test_version_check_reports_drifted_file_and_actual_value(tmp_path):
    files = (
        "tools.toml",
        "uv.toml",
        ".python-version",
        ".github/workflows/ci.yml",
        "analysis/README.md",
        "docs/data/README.md",
    )
    for relative in files:
        destination = tmp_path / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, destination)

    write_text_lf(tmp_path / "uv.toml", 'required-version = "==9.9.9"\n')
    errors = check_repository_pins(tmp_path)

    assert any("uv.toml" in error and "9.9.9" in error and "0.13.5" in error for error in errors)
