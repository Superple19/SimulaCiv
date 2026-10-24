import shutil
import subprocess
from pathlib import Path
import tomllib

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


def test_analysis_uv_python_pins_apply_to_project():
    project = tomllib.loads((ROOT / "analysis" / "pyproject.toml").read_text(encoding="utf-8"))
    assert project["tool"]["uv"]["required-version"] == f"=={tool_version('uv')}"
    assert project["project"]["requires-python"] == ">=3.11"
    assert {dependency.split("=")[0] for dependency in project["project"]["dependencies"]} == {
        "matplotlib", "pandas", "pytest"
    }
    assert (ROOT / "analysis" / ".python-version").read_text(encoding="utf-8").strip() == "3.15.1"

    result = subprocess.run(
        ["uv", "run", "--directory", "analysis", "--project", ".", "python", "-c", "import sys; print(sys.version)"],
        cwd=ROOT,
        capture_output=True,
        check=False,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.split()[0] == "3.15.1"


def test_uv_rejects_mismatched_required_version_from_analysis_project(tmp_path):
    project = tmp_path / "analysis"
    project.mkdir()
    write_text_lf(
        project / "pyproject.toml",
        '[project]\nname = "pin-discovery-test"\nversion = "0.1.0"\nrequires-python = ">=3.11"\n\n'
        '[tool.uv]\npackage = false\nrequired-version = "==0.0.1"\n',
    )
    write_text_lf(project / ".python-version", "3.15.1\n")

    result = subprocess.run(
        ["uv", "run", "--directory", str(project), "--project", ".", "--no-sync", "python", "-c", "print('ran')"],
        cwd=ROOT,
        capture_output=True,
        check=False,
        text=True,
    )

    assert result.returncode != 0
    assert "required-version" in result.stderr.lower() or "0.0.1" in result.stderr
    assert "ran" not in result.stdout


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
        "analysis/pyproject.toml",
        "analysis/.python-version",
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
    project_path = tmp_path / "analysis" / "pyproject.toml"
    project_text = project_path.read_text(encoding="utf-8").replace(
        f'required-version = "=={tool_version("uv")}"',
        'required-version = "==9.9.9"',
    )
    write_text_lf(project_path, project_text)
    monkeypatch.setattr("simciv_analysis.tool_versions._tracked_paths", lambda _: [])

    errors = check_tool_versions(tmp_path)
    assert any("analysis/pyproject.toml" in error and "9.9.9" in error and tool_version("uv") in error for error in errors)
