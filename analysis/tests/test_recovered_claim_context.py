import json

from simciv_analysis.claims import scan_documents
from simciv_analysis.io import write_text_lf
from simciv_analysis.recovered_claims import _dimension_matches, _phase_value


def test_claim_context_keeps_measure_column_and_row_configuration(tmp_path):
    report = tmp_path / "docs" / "reports" / "sample.md"
    report.parent.mkdir(parents=True)
    write_text_lf(
        report,
        "# Sample\n\n## Results\n\n"
        "| N | p_local | phase | Median (µs) |\n|---:|---:|---|---:|\n"
        "| 10000 | 0.5 | 4 | 1.250 |\n",
    )

    claims = scan_documents(tmp_path)
    metric = next(claim for claim in claims if claim["value"] == "1.250")
    phase_label = next(claim for claim in claims if claim["value"] == "4")
    metric_context = json.loads(metric["context"])
    phase_context = json.loads(phase_label["context"])

    assert metric_context["column"] == "Median (µs)"
    assert metric_context["dimensions"] == {"population_n": "10000", "p_local": "0.5", "phase": "4"}
    assert phase_context["column"] == "phase"
    assert phase_context["column_is_parameter"] is True


def test_alpha_dimensions_match_the_same_interval_and_phase_alias():
    expected = {
        "relationship_degree_k": "8",
        "checkpoint": "50",
        "interval": "100k→250k",
        "phase": "P3",
    }
    actual = {
        "k": "8",
        "horizon": "50",
        "phase": "P3_features",
        "from_N": "100000",
        "to_N": "250000",
    }

    assert _dimension_matches(expected, actual)
    assert not _dimension_matches(expected, {**actual, "to_N": "500000"})


def test_phase_normalization_has_stable_token_order():
    assert _phase_value("P6B key_generation") == "key p6b"
    assert _phase_value("key generation P6B") == "key p6b"


def test_candidate_parameter_is_recorded_as_a_config_dimension(tmp_path):
    report = tmp_path / "docs" / "reports" / "sample.md"
    report.parent.mkdir(parents=True)
    write_text_lf(
        report,
        "# Sample\n\n## Results\n\n"
        "| Candidate | Mean alive (agents) |\n|---|---:|\n"
        "| health_decay_rate=0.01 | 1234 |\n",
    )

    claims = scan_documents(tmp_path)
    mean = next(claim for claim in claims if claim["value"] == "1234")
    mean_context = json.loads(mean["context"])

    assert mean_context["dimensions"]["candidate"] == "health_decay_rate=0.01"
    assert mean_context["column_is_parameter"] is False
    assert not any(claim["value"] == "0.01" for claim in claims)
