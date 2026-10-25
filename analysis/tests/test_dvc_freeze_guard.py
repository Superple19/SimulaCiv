from pathlib import Path

import pytest

from simciv_analysis.dvc_freeze_guard import (
    RECONSTRUCTION_TARGET,
    frozen_timing_violations,
    validate_repro_target,
)
from simciv_analysis.io import write_text_lf


def test_locked_timing_stages_must_be_frozen(tmp_path: Path):
    data = tmp_path / "docs" / "data" / "stage"
    data.mkdir(parents=True)
    write_text_lf(data / "dvc.yaml", "stages:\n  timed:\n    cmd: runner timing cell\n    outs:\n    - raw\n")
    write_text_lf(data / "dvc.lock", "schema: '2.0'\nstages:\n  timed:\n    cmd: runner timing cell\n")

    assert frozen_timing_violations(tmp_path) == ["docs/data/stage/dvc.yaml:timed has a lock but is not frozen"]

    write_text_lf(
        data / "dvc.yaml",
        "stages:\n  timed:\n    cmd: runner timing cell\n    outs:\n    - raw\n    frozen: true\n",
    )
    assert frozen_timing_violations(tmp_path) == []


def test_reconstruct_all_allows_only_its_unfrozen_registered_target():
    validate_repro_target(RECONSTRUCTION_TARGET)
    with pytest.raises(ValueError, match="registered stage"):
        validate_repro_target("docs/data/m2_m3_reconstruction/dvc.yaml:missing")
    with pytest.raises(ValueError, match="only run its registered stage"):
        validate_repro_target("docs/data/m5_05/dvc.yaml:m5_05_m3_06_settlement_index")
