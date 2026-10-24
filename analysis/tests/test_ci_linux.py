from simciv_analysis import ci_linux


def test_wsl_check_installs_locked_mise_tools_and_uv_python_before_ci_targets(monkeypatch):
    scripts = []
    monkeypatch.setattr(ci_linux, "_wsl_path", lambda: "/mnt/c/project")
    monkeypatch.setattr(ci_linux.subprocess, "check_output", lambda *args, **kwargs: "abc123\n")
    monkeypatch.setattr(ci_linux, "_run_wsl", lambda script: scripts.append(script) or 0)

    assert ci_linux.main() == 0
    assert any("mise trust --yes && mise install --locked" in script for script in scripts)
    assert any("mise exec -- uv python install" in script for script in scripts)
    assert any("mise exec -- just ci-rust" in script for script in scripts)
    assert any("mise exec -- just ci-analysis" in script for script in scripts)
