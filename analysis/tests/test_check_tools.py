from pathlib import Path
import subprocess

from simciv_analysis import check_tools
from simciv_analysis.tool_versions import mise_version, rust_toolchain_version, tool_version


def test_tools_must_resolve_to_the_pinned_mise_installations(monkeypatch, tmp_path: Path):
    mise = str(tmp_path / "mise")
    managed = {name: str(tmp_path / name) for name in check_tools.TOOLS}
    rustc = str(tmp_path / "rustc")
    on_path = {"mise": mise, "rustc": rustc, **managed}
    version_overrides: dict[str, str] = {}
    monkeypatch.setattr(check_tools.shutil, "which", lambda name: on_path.get(name))

    def run(command: list[str]) -> subprocess.CompletedProcess[str]:
        if command[0] == mise and command[1] == "--version":
            output = f"mise {mise_version()} windows-x64"
        elif command[0] == mise and command[1] == "which":
            output = managed[command[2]]
        elif command[0] == rustc:
            output = f"rustc {version_overrides.get('rustc', rust_toolchain_version())} (fixture)"
        else:
            name = Path(command[0]).name.removesuffix(".exe")
            output = f"{name} {version_overrides.get(name, tool_version(name))}"
        return subprocess.CompletedProcess(command, 0, output, "")

    monkeypatch.setattr(check_tools, "_run", run)
    assert check_tools.check_tools() == []

    on_path["just"] = str(tmp_path / "standalone-just")
    assert any("not the mise-managed binary" in error for error in check_tools.check_tools())
    on_path["just"] = managed["just"]
    version_overrides["just"] = "0.0.0"
    assert any("just: expected" in error and "found 0.0.0" in error for error in check_tools.check_tools())
    version_overrides["just"] = tool_version("just")
    version_overrides["rustc"] = "0.0.0"
    assert any("rustc: expected toolchain" in error for error in check_tools.check_tools())
