from __future__ import annotations

import json

import pytest

from simciv_analysis.a5_tonight import (
    GROUPS,
    ORDER,
    _available_protocol_ids,
    _cargo_env,
    _exceeds_invalid_limit,
    _overlay_failure_record,
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


def test_report_revisions_use_isolated_cargo_target_directories():
    left = _cargo_env("a" * 40)["CARGO_TARGET_DIR"]
    right = _cargo_env("b" * 40)["CARGO_TARGET_DIR"]
    assert left != right
    assert left.endswith("a" * 40)
    assert right.endswith("b" * 40)


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


def test_overlay_build_failure_maps_every_protocol_cell_to_the_build_gate():
    protocol = {
        "id": "m3_11",
        "conclusion_id": "M3-11",
        "report_revision": "a" * 40,
        "ordered_cell_ids": ["phase", "whole-a", "whole-b"],
        "harness_overlays": [{"path": "bench.rs", "sha256": "b" * 64, "origin_commit": "c" * 40}],
    }
    manifest = {
        "run_id": "build-failed",
        "revision": "a" * 40,
        "toolchain": {"rustc": "test"},
        "date": "2026-10-28",
        "commands": ["cargo bench --no-run"],
        "harness_overlays": protocol["harness_overlays"],
    }
    record = _overlay_failure_record(manifest, protocol, "diagnostic.json", "d" * 64)
    assert record["overlay_build_ok"] is False
    assert [row["cell_id"] for row in record["cell_results"]] == protocol["ordered_cell_ids"]
    assert all(row["overlay_build_ok"] is False for row in record["cell_results"])
