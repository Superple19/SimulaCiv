import csv

from simciv_analysis.claims import extract_claims
from simciv_analysis.sources import load_sources, source_for_claim, validate_source_paths


def test_sources_are_explicit_and_existing_artifacts_resolve():
    sources = load_sources()
    validate_source_paths(sources)
    assert len(sources["reports"]) == 45


def test_section_sources_override_report_defaults():
    sources = load_sources()
    a5 = source_for_claim(
        sources,
        "M4_02_BRANCHING.md",
        "final-confirmatory-measurement-under-frozen-11-7-a-5",
        "timing",
    )
    historical = source_for_claim(sources, "M4_02_BRANCHING.md", "host-validity-summary", "timing")
    assert a5 is not None and a5["regeneration_stage"] == "M5-04" and a5["status"] == "conclusion holds"
    assert historical is not None and historical["status"] == "historical-protocol"

    m1 = source_for_claim(
        sources, "M1_CONTRACT_FREEZE.md", "2-m0-empirical-validation-evidence-summary", "deterministic"
    )
    seed_followup = source_for_claim(sources, "M4_05_MINIMAL_CAUSE.md", "seed-variation", "derived")
    assert m1 is not None and m1["status"] == "historical-log"
    assert seed_followup is not None and seed_followup["status"] == "unrecoverable" and seed_followup["review"]


def test_claim_extraction_uses_source_map_instead_of_old_metadata(tmp_path):
    reports = tmp_path / "docs" / "reports"
    reproducibility = tmp_path / "docs" / "reproducibility"
    reports.mkdir(parents=True)
    reproducibility.mkdir(parents=True)
    (reports / "sample.md").write_text("# Sample\n\n## Findings\n\n- Result is 42.\n", encoding="utf-8")
    (reproducibility / "sources.toml").write_text(
        'version = 1\n\n[reports."sample.md"]\nclass = "*"\nharness = []\ndata = []\n'
        'regeneration_stage = "M5-04"\nstatus = "reproducible-now"\nreview = false\nnotes = "mapped source"\n',
        encoding="utf-8",
    )
    old = tmp_path / "old.csv"
    with old.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow([
            "claim_id", "report", "section anchor", "claim text (short)", "value", "unit", "class",
            "source harness", "raw data", "status", "M5 stage", "notes",
        ])
        writer.writerow(["1", "sample.md", "findings", "L5: - Result is 42.", "42", "", "derived", "legacy.rs", "old.csv", "unrecoverable", "none", "old value"])

    claim = extract_claims(tmp_path, old)[0]
    assert claim["status"] == "reproducible-now"
    assert claim["source harness"] == "none"
    assert claim["notes"] == "mapped source"
