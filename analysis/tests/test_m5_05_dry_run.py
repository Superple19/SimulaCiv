import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_dry_run_covers_every_dvc_conclusion_without_timing():
    result = subprocess.run(
        [sys.executable, "-m", "simciv_analysis.m5_05", "--dry-run"],
        cwd=ROOT / "analysis",
        capture_output=True,
        text=True,
        check=True,
    )
    implemented = {
        line.removeprefix("IMPLEMENTED|")
        for line in result.stdout.splitlines()
        if line.startswith("IMPLEMENTED|")
    }
    assert len(implemented) == 13
    assert "m3_06_settlement_index" in implemented
    assert "m3_11_frontier_crossover" in implemented
    assert "TIMING RUN START" not in result.stdout
