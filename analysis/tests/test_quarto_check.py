from simciv_analysis import quarto_check
from simciv_analysis.tool_versions import tool_version


PINNED_QUARTO = tool_version("quarto")


def test_quarto_preflight_rejects_missing_cli(monkeypatch, capsys):
    monkeypatch.setattr(quarto_check.shutil, "which", lambda _: None)

    assert quarto_check.main() == 1
    assert f"Quarto CLI {PINNED_QUARTO} is required on PATH" in capsys.readouterr().err


def test_quarto_preflight_rejects_wrong_version(monkeypatch, capsys):
    monkeypatch.setattr(quarto_check.shutil, "which", lambda _: "quarto")
    monkeypatch.setattr(
        quarto_check.subprocess,
        "run",
        lambda *args, **kwargs: type("Result", (), {"stdout": "1.9.0\n"})(),
    )

    assert quarto_check.main() == 1
    assert f"expected {PINNED_QUARTO}, found 1.9.0" in capsys.readouterr().err


def test_quarto_preflight_accepts_pinned_version(monkeypatch, capsys):
    monkeypatch.setattr(quarto_check.shutil, "which", lambda _: "quarto")
    monkeypatch.setattr(
        quarto_check.subprocess,
        "run",
        lambda *args, **kwargs: type("Result", (), {"stdout": f"{PINNED_QUARTO}\n"})(),
    )

    assert quarto_check.main() == 0
    assert f"Quarto {PINNED_QUARTO} is available" in capsys.readouterr().out
