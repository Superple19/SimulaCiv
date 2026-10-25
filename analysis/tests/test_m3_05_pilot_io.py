from simciv_analysis.io import write_text_lf


def test_completed_series_can_be_appended_and_flushed(tmp_path):
    target = tmp_path / "series.jsonl"
    write_text_lf(target, '{"series":1}\n')
    write_text_lf(target, '{"series":2}\n', append=True, flush=True)

    assert target.read_text(encoding="utf-8") == '{"series":1}\n{"series":2}\n'
