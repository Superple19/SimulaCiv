import json
import hashlib

import pytest

from simciv_analysis.m3_05_pilot import _read_resume, _series_plan
from simciv_analysis.io import write_text_lf


def test_resume_accepts_only_a_completed_series_with_matching_config_hash(tmp_path):
    expected = _series_plan(smoke=True)
    series_id = sorted(expected)[0]
    path = tmp_path / "resume.jsonl"
    marker = {
        "record": "SCALE_SERIES_COMPLETE",
        "series_id": series_id,
        "config_hash": expected[series_id],
        "elapsed_seconds": 1.25,
    }
    write_text_lf(path, json.dumps(marker) + "\n")

    records, completed = _read_resume(path, expected)

    assert records == [marker]
    assert completed == {series_id: 1.25}


def test_resume_rejects_a_completed_series_for_a_different_config(tmp_path):
    expected = _series_plan(smoke=True)
    series_id = sorted(expected)[0]
    marker = {
        "record": "SCALE_SERIES_COMPLETE",
        "series_id": series_id,
        "config_hash": hashlib.sha256(b"different config").hexdigest(),
        "elapsed_seconds": 1.25,
    }
    path = tmp_path / "resume.jsonl"
    write_text_lf(path, json.dumps(marker) + "\n")

    with pytest.raises(ValueError, match="config hash"):
        _read_resume(path, expected)
