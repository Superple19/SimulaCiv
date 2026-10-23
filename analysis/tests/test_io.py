import gzip
import json

import pandas as pd
import pytest

from simciv_analysis.io import content_matches, load_data, write_text_lf


def test_write_text_lf_creates_parents_and_normalizes_newlines(tmp_path):
    target = tmp_path / "nested" / "output.txt"
    write_text_lf(target, "한글\r\ntwo\rthree\n")

    assert target.read_bytes() == "한글\ntwo\nthree\n".encode("utf-8")


def test_loads_csv_json_jsonl_and_gzip(tmp_path):
    (tmp_path / "table.csv").write_text("a,b\n1,x\n", encoding="utf-8")
    (tmp_path / "one.json").write_text('{"a": 1}', encoding="utf-8")
    (tmp_path / "lines.jsonl").write_text('{"a":1}\n\n{"a":2}\n', encoding="utf-8")
    with (tmp_path / "lines.jsonl.gz").open("wb") as raw:
        with gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as stream:
            stream.write(b'{"a":3}\n')

    assert load_data("table.csv", root=tmp_path).equals(pd.DataFrame({"a": [1], "b": ["x"]}))
    assert load_data("one.json", root=tmp_path) == {"a": 1}
    assert load_data("lines.jsonl", root=tmp_path) == [{"a": 1}, {"a": 2}]
    assert load_data("lines.jsonl.gz", root=tmp_path) == [{"a": 3}]


def test_gzip_content_check_ignores_container_bytes(tmp_path):
    payload = b"claim_id,value\n1,42\n"
    expected = gzip.compress(payload, compresslevel=9, mtime=0)
    differently_encoded = gzip.compress(payload, compresslevel=1, mtime=123)
    target = tmp_path / "claims.csv.gz"
    target.write_bytes(differently_encoded)

    assert differently_encoded != expected
    assert content_matches(target, expected)
    assert not content_matches(target, gzip.compress(payload + b"2,43\n", mtime=0))


def test_rejects_escape_and_unknown_format(tmp_path):
    outside = tmp_path.parent / "outside.json"
    outside.write_text(json.dumps({"ok": True}), encoding="utf-8")
    (tmp_path / "bad.bin").write_bytes(b"data")
    with pytest.raises(ValueError, match="escapes"):
        load_data(outside, root=tmp_path)
    with pytest.raises(ValueError, match="unsupported"):
        load_data("bad.bin", root=tmp_path)
