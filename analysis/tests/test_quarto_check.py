from simciv_analysis import quarto_check


def test_quarto_preflight_rejects_missing_cli(monkeypatch, capsys):
    monkeypatch.setattr(quarto_check.shutil, "which", lambda _: None)

    assert quarto_check.main() == 1
    assert "Quarto CLI 1.10.19 is required on PATH" in capsys.readouterr().err


def test_quarto_preflight_rejects_wrong_version(monkeypatch, capsys):
    monkeypatch.setattr(quarto_check.shutil, "which", lambda _: "quarto")
    monkeypatch.setattr(
        quarto_check.subprocess,
        "run",
        lambda *args, **kwargs: type("Result", (), {"stdout": "1.9.0\n"})(),
    )

    assert quarto_check.main() == 1
    assert "expected 1.10.19, found 1.9.0" in capsys.readouterr().err


def test_quarto_preflight_accepts_pinned_version(monkeypatch, capsys):
    monkeypatch.setattr(quarto_check.shutil, "which", lambda _: "quarto")
    monkeypatch.setattr(
        quarto_check.subprocess,
        "run",
        lambda *args, **kwargs: type("Result", (), {"stdout": "1.10.19\n"})(),
    )

    assert quarto_check.main() == 0
    assert "Quarto 1.10.19 is available" in capsys.readouterr().out
