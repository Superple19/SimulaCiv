from pathlib import Path
import json

from simciv_analysis.claims import apply_m5_05_results
from simciv_analysis.m5_05 import validate_registry
from simciv_analysis.report_m5_05 import _build


ROOT = Path(__file__).resolve().parents[2]


def test_conclusion_registry_and_sessions_are_complete():
    entries = validate_registry(ROOT)
    assert len(entries) == 13
    assert sum(entry["estimate_minutes"] for entry in entries) == 450
    assert {entry["session"] for entry in entries} == {1, 2, 3}
    assert {entry["id"] for entry in entries if entry["session"] == 1} == {
        "m2_near_linear",
        "m2_final_tick",
        "m2_phase4_crossover",
        "m3_01_serial_baseline",
        "m3_02_parallel_phase6b",
        "m3_03_search_reduction",
    }


def test_dvc_has_deterministic_and_per_conclusion_stages():
    entries = validate_registry(ROOT)
    pipeline = (ROOT / "dvc.yaml").read_text(encoding="utf-8")
    assert "m5_05_deterministic:" in pipeline
    for entry in entries:
        assert f"m5_05_{entry['id']}:" in pipeline
        assert f"docs/data/m5_05/{entry['id']}/raw" in pipeline


def test_report_generator_does_not_read_dvc_raw_paths():
    pipeline = (ROOT / "dvc.yaml").read_text(encoding="utf-8")
    report = (ROOT / "analysis" / "simciv_analysis" / "report_m5_05.py").read_text(encoding="utf-8")
    outputs = [
        line.strip()[2:]
        for line in pipeline.splitlines()
        if line.strip().startswith("- docs/data/m5_05/") and line.strip().endswith("/raw")
    ]
    assert outputs
    for output in outputs:
        assert output not in report


def test_m5_05_results_update_only_matching_timing_headlines(tmp_path):
    results_path = tmp_path / "docs" / "data" / "m5_05" / "results.json"
    results_path.parent.mkdir(parents=True)
    results_path.write_text(
        json.dumps({"conclusions": [{
            "report": "M3_03_SEARCH_REDUCTION.md",
            "section_anchor": "paired-whole-tick-results",
            "status": "holds",
            "notes": "selected cells passed",
        }]}),
        encoding="utf-8",
    )
    claims = [
        {
            "report": "M3_03_SEARCH_REDUCTION.md",
            "section anchor": "paired-whole-tick-results",
            "class": "timing",
            "tier": "headline",
            "M5 stage": "M5-05",
            "status": "reproducible-now",
            "notes": "source mapped",
        },
        {
            "report": "M3_03_SEARCH_REDUCTION.md",
            "section anchor": "paired-whole-tick-results",
            "class": "timing",
            "tier": "supporting",
            "M5 stage": "M5-05",
            "status": "reproducible-now",
            "notes": "source mapped",
        },
    ]

    updated = apply_m5_05_results(claims, tmp_path)
    assert updated[0]["status"] == "conclusion holds"
    assert updated[1]["status"] == "reproducible-now"


def test_m5_05_report_uses_one_git_summary_row_per_conclusion():
    generated = _build()
    assert len(generated["results"]["conclusions"]) == 13
    assert len(generated["results_bytes"]) <= 1024 * 1024
    assert "docs/data/m5_05/" in generated["qmd"].decode("utf-8")
    assert "DVC raw paths" in generated["qmd"].decode("utf-8")
