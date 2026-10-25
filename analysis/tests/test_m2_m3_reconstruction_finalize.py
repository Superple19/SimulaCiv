import shutil

from simciv_analysis import m2_m3_reconstruction_finalize as finalize
from simciv_analysis.provenance import ROOT


def test_running_inventory_finalizer_leaves_decisions_unchanged(tmp_path, monkeypatch):
    decisions = ROOT / "contracts/decisions.toml"
    before = decisions.read_bytes()
    csv_path = tmp_path / "M2_M3_EVIDENCE_RECONSTRUCTION.csv"
    report_path = tmp_path / "M2_M3_EVIDENCE_RECONSTRUCTION.md"
    shutil.copyfile(ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv", csv_path)
    shutil.copyfile(ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.md", report_path)
    monkeypatch.setattr(finalize, "CSV_PATH", csv_path)
    monkeypatch.setattr(finalize, "MD_PATH", report_path)

    finalize.main()
    assert decisions.read_bytes() == before
