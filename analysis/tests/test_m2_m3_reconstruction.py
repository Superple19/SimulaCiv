from pathlib import Path

from simciv_analysis import m2_m3_reconstruction


def test_inventory_check_rejects_output_that_differs_from_source_build(tmp_path: Path, monkeypatch, capsys):
    output = tmp_path / "inventory.csv"
    output.write_bytes(b"stale\n")
    monkeypatch.setattr(m2_m3_reconstruction, "ROOT", tmp_path)
    monkeypatch.setattr(m2_m3_reconstruction, "build_outputs", lambda: {output: b"rebuilt from sources\n"})

    assert m2_m3_reconstruction.main(["--check"]) == 1
    assert "inventory.csv" in capsys.readouterr().err
