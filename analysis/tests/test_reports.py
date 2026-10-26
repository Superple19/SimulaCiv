from simciv_analysis import reports


def test_aggregate_report_check_runs_all_preserved_checks_and_fails_closed(monkeypatch, capsys):
    calls = []

    def result(name, passed):
        def check():
            calls.append(name)
            return passed

        return check

    monkeypatch.setattr(
        reports,
        "REPORT_CHECKS",
        (("M5-01", result("M5-01", True)), ("M5-04", result("M5-04", False)), ("M5-05", result("M5-05", True))),
    )

    assert reports.check_reports() is False
    assert calls == ["M5-01", "M5-04", "M5-05"]
    assert "M5-04" in capsys.readouterr().err
