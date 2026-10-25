import json

from simciv_analysis.m5_05_dvc_status import _cache_object_state, status_report_matches


def test_dvc_directory_cache_manifest_keeps_dir_suffix(tmp_path, monkeypatch):
    from simciv_analysis import m5_05_dvc_status

    monkeypatch.setattr(m5_05_dvc_status, "CACHE", tmp_path)
    directory_md5 = "0123456789abcdef0123456789abcdef.dir"
    file_md5 = "abcdef0123456789abcdef0123456789"
    manifest = m5_05_dvc_status._cache_path(directory_md5)
    blob = m5_05_dvc_status._cache_path(file_md5)
    manifest.parent.mkdir(parents=True)
    blob.parent.mkdir(parents=True)
    manifest.write_text(json.dumps([{"md5": file_md5, "relpath": "samples.jsonl"}]), encoding="utf-8")
    blob.write_text("sample", encoding="utf-8")

    assert _cache_object_state(directory_md5) == f"complete (`{directory_md5}` and 1 file blob(s))"


def test_dvc_status_check_rejects_a_stale_generated_report(tmp_path):
    report = tmp_path / "STATUS.md"
    report.write_text("stale\n", encoding="utf-8")

    assert not status_report_matches(report, "fresh\n")
