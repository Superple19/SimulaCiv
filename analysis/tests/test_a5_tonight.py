from __future__ import annotations

import json

import pytest

from simciv_analysis.a5_tonight import (
    GROUPS,
    ORDER,
    _available_protocol_ids,
    _exceeds_invalid_limit,
    _protocols,
    _split_by_cell,
)


def test_tonight_plan_covers_the_registered_cells_once_in_execution_order():
    protocols = _protocols()
    actual = [cell for group in ORDER for protocol_id in GROUPS[group]["protocols"] for cell in protocols[protocol_id]["ordered_cell_ids"]]
    assert len(actual) == 16
    assert len(set(actual)) == len(actual)
    assert [GROUPS[group]["stage"] for group in ORDER] == [
        "a5_m2_explicit_rayon",
        "a5_m3_09_equal_live",
        "a5_m3_11_propagation",
        "a5_m4_search_parallel",
    ]


def test_m3_11_split_preserves_raw_record_bytes_and_cell_order():
    lines = [
        b'{"kind":"series","cell_id":"phase"}\r\n',
        b'{"kind":"series","cell_id":"whole-a"}\n',
        b'{"kind":"series","cell_id":"whole-b"}\r\n',
    ]
    split = _split_by_cell(
        b"".join(lines),
        {"phase-protocol": ["phase"], "whole-protocol": ["whole-a", "whole-b"]},
    )
    assert split == {"phase-protocol": lines[0], "whole-protocol": lines[1] + lines[2]}


def test_m3_11_split_rejects_unregistered_cell_records():
    with pytest.raises(ValueError, match="unexpected M3-11 raw cell"):
        _split_by_cell(
            (json.dumps({"cell_id": "wrong"}) + "\n").encode(),
            {"phase-protocol": ["phase"]},
        )


def test_stage_validity_limit_uses_strictly_more_than_ten_percent():
    assert not _exceeds_invalid_limit(0, 8)
    assert _exceeds_invalid_limit(1, 8)
    assert not _exceeds_invalid_limit(1, 10)
    assert not _exceeds_invalid_limit(0, 0)


def test_blocked_report_sha_group_is_excluded_from_prepared_protocols():
    available = _available_protocol_ids({"prepared_groups": ["m2", "m3_09", "m4"]})
    assert "m3_11_phase9c_speedup" not in available
    assert "m3_11_whole_tick_speedup" not in available
    assert available == {
        "m2_explicit_rayon_speedup",
        "m3_09_incremental_equal_live",
        "m4_07_case_a_search_parallel_speedup",
    }
