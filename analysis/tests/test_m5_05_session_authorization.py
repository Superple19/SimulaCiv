import pytest

from simciv_analysis.m5_05 import _timing_session_authorized


def test_timing_sessions_default_to_session_one(monkeypatch):
    monkeypatch.delenv("SIMCIV_M5_05_AUTHORIZED_SESSIONS", raising=False)
    assert _timing_session_authorized(1)
    assert not _timing_session_authorized(2)


def test_timing_sessions_require_an_explicit_allowlist(monkeypatch):
    monkeypatch.setenv("SIMCIV_M5_05_AUTHORIZED_SESSIONS", "2, 3")
    assert not _timing_session_authorized(1)
    assert _timing_session_authorized(2)
    assert _timing_session_authorized(3)


def test_timing_session_allowlist_must_be_valid(monkeypatch):
    monkeypatch.setenv("SIMCIV_M5_05_AUTHORIZED_SESSIONS", "2, nope")
    with pytest.raises(ValueError, match="comma-separated list"):
        _timing_session_authorized(2)
