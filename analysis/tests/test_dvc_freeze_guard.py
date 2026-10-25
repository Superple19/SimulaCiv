from pathlib import Path

import pytest

from simciv_analysis.dvc_freeze_guard import (
    RECONSTRUCTION_TARGET,
    frozen_output_hash_violations,
    frozen_timing_violations,
    validate_repro_target,
)
from simciv_analysis.io import write_text_lf


def test_locked_timing_stages_must_be_frozen(tmp_path: Path):
    data = tmp_path / "docs" / "data" / "stage"
    data.mkdir(parents=True)
    write_text_lf(data / "dvc.yaml", "stages:\n  timed:\n    cmd: runner timing cell\n    outs:\n    - raw\n    meta:\n      kind: timing\n")
    write_text_lf(data / "dvc.lock", "schema: '2.0'\nstages:\n  timed:\n    cmd: runner timing cell\n")

    assert frozen_timing_violations(tmp_path) == ["docs/data/stage/dvc.yaml:timed has a lock but is not frozen"]

    write_text_lf(
        data / "dvc.yaml",
        "stages:\n  timed:\n    cmd: runner timing cell\n    outs:\n    - raw\n    frozen: true\n    meta:\n      kind: timing\n",
    )
    assert frozen_timing_violations(tmp_path) == []


def test_every_stage_requires_an_explicit_kind(tmp_path: Path):
    data = tmp_path / "docs" / "data" / "stage"
    data.mkdir(parents=True)
    write_text_lf(data / "dvc.yaml", "stages:\n  named_deterministic:\n    cmd: deterministic\n")

    assert frozen_timing_violations(tmp_path) == [
        "docs/data/stage/dvc.yaml:named_deterministic must declare meta.kind as timing or deterministic"
    ]


def test_frozen_lock_output_hash_change_is_rejected():
    pipeline = "stages:\n  timing_stage:\n    frozen: true\n    meta:\n      kind: timing\n"
    before = "stages:\n  timing_stage:\n    outs:\n    - path: docs/data/raw\n      md5: aaaa.dir\n"
    after = "stages:\n  timing_stage:\n    outs:\n    - path: docs/data/raw\n      md5: bbbb.dir\n"

    assert frozen_output_hash_violations(pipeline, before, after) == [
        "dvc.lock:timing_stage frozen output hashes changed: {'docs/data/raw': 'aaaa.dir'} -> {'docs/data/raw': 'bbbb.dir'}"
    ]


def test_reconstruct_all_allows_only_its_unfrozen_registered_target():
    validate_repro_target(RECONSTRUCTION_TARGET)
    with pytest.raises(ValueError, match="registered stage"):
        validate_repro_target("docs/data/m2_m3_reconstruction/dvc.yaml:missing")
    with pytest.raises(ValueError, match="only run its registered stage"):
        validate_repro_target("docs/data/m5_05/dvc.yaml:m5_05_m3_06_settlement_index")
