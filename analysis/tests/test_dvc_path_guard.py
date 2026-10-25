from pathlib import Path

from simciv_analysis.dvc_path_guard import analysis_dvc_read_violations, find_dvc_opened_paths


ROOT = Path(__file__).resolve().parents[2]


def test_analysis_tests_and_report_generators_do_not_open_dvc_outputs():
    assert analysis_dvc_read_violations(ROOT) == []


def test_dvc_open_guard_detects_direct_and_pathlib_reads():
    source = '''
from pathlib import Path
ROOT = Path(__file__).parents[2]
RAW = ROOT / "docs" / "data" / "m5_05" / "case" / "raw"
first = RAW.read_text()
second = open(RAW / "samples.jsonl", encoding="utf-8")
'''

    assert find_dvc_opened_paths(source, {"docs/data/m5_05/case/raw"}) == [
        "docs/data/m5_05/case/raw",
        "docs/data/m5_05/case/raw/samples.jsonl",
    ]
