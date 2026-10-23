from simciv_analysis.provenance import _summary_lock_path


def test_provenance_selects_the_nearest_nested_dvc_lock(tmp_path):
    data = tmp_path / "docs" / "data" / "m5_05"
    diagnosis = data / "m3_09_diagnosis"
    diagnosis.mkdir(parents=True)
    summary = diagnosis / "summary.json"
    summary.write_text("{}\n", encoding="utf-8")
    (data / "dvc.lock").write_text("stages: {}\n", encoding="utf-8")
    expected = diagnosis / "dvc.lock"
    expected.write_text("stages: {}\n", encoding="utf-8")

    assert _summary_lock_path(summary, tmp_path) == expected
