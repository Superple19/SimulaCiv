from __future__ import annotations

import hashlib
from copy import deepcopy

from simciv_analysis.verdicts import _tie_verdicts, aggregate_verdict, compute_verdicts, results_relevant_digest


REPORT_SHA = "a" * 40
HEAD_SHA = "b" * 40
REF = {
    "id": "reference",
    "reference": True,
    "cpu_model": "Reference CPU",
    "physical_cores": 6,
    "logical_cores": 12,
    "rustc": "1.98.1",
    "rust_target": "x86_64-pc-windows-msvc",
    "a5": {
        "warmups": 2, "samples": 41, "sample_target_seconds": 1.25,
        "minimum_sample_seconds": 1.0, "max_path_mad_over_median": 0.05,
        "priority": "High", "affinity_mask": "0x555", "pair_order": "alternating",
    },
}
TOOLCHAIN = {
    "cpu_model": REF["cpu_model"],
    "physical_cores": REF["physical_cores"],
    "logical_cores": REF["logical_cores"],
    "rustc": "1.98.1 (test build)",
    "rust_target": REF["rust_target"],
    "a5_settings": REF["a5"],
}
A5_VALIDITY = {
    "warmups": 2,
    "sample_count": 41,
    "repetitions_per_sample": 1,
    "sample_seconds": [1.25] * 41,
    "path_mad_over_median": {"left": 0.01, "right": 0.02},
}


def _case(kind="ratio_band", *, conclusion_type="timing", role="report", expected=None, cell=None):
    expected = expected or {"cell-a": [2.0]}
    parameters = {
        "ratio_band": {"historical_values": [2.0], "ratio_direction": "test ratio", "deviation_bands_pct": [10, 20]},
        "threshold_side": {"metric": "metric", "historical_values": [1.5], "threshold": 1.0, "relation": ">", "noise_rule": "test"},
        "exact": {"fields": ["value"], "expected_values": expected, "comparison": "exact equality"},
        "completion": {"required_result": "completed", "error_policy": "errors fail"},
        "no_gain": {"paired_effect_field": "effect", "paired_mad_field": "mad", "relation": "abs(effect) <= MAD"},
    }[kind]
    conclusion = {
        "id": "C-1",
        "type": conclusion_type,
        "tier": 1,
        "experiment": "E-1",
        "report_revision": REPORT_SHA,
    }
    experiment = {
        "id": "E-1",
        "status": "complete",
        "ordered_cell_ids": ["cell-a"],
        "predicate_kind": kind,
        "predicate_parameters": parameters,
        "harness_overlays": [],
    }
    cell_result = dict(cell or {"cell_id": "cell-a", "observed_ratio": 2.0, "observed": 2.0})
    cell_result.setdefault("cell_id", "cell-a")
    cell_result.setdefault("valid", True)
    cell_result.setdefault("validity", deepcopy(A5_VALIDITY))
    record = {
        "id": "ev-1",
        "conclusion_id": "C-1",
        "role": role,
        "kind": "deterministic_exact" if conclusion_type == "deterministic" else ("a5_head" if role == "head" else "a5_report_revision"),
        "revision": REPORT_SHA if role == "report" else HEAD_SHA,
        "protocol_id": "E-1",
        "artifacts": [{"path": "some-output.json", "sha256": "c" * 64}],
        "toolchain": deepcopy(TOOLCHAIN),
        "cell_results": [cell_result],
    }
    return {"conclusion": [conclusion]}, {"experiment": [experiment]}, {"host": [REF]}, [record]


def _one(registry, experiments, hosts, evidence, **kwargs):
    return compute_verdicts(registry, experiments, hosts, evidence, **kwargs)[0]


def test_aggregate_verdict_obeys_each_precedence_branch():
    cases = [
        (["needs-decision", "not reproduced"], "needs-decision"),
        (["not reproduced", "inconclusive"], "not reproduced"),
        (["unverified", "unverified (stale)"], "unverified (stale)"),
        (["reproduced", "unverified"], "inconclusive"),
        (["reproduced (loose)", "inconclusive"], "inconclusive"),
        (["reproduced", "reproduced (loose)"], "reproduced (loose)"),
        (["reproduced", "reproduced"], "reproduced"),
        ([], "unverified"),
    ]
    for values, expected in cases:
        assert aggregate_verdict(values) == expected


def test_same_strength_conflicts_use_conservative_contract_tie_rule():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["cell_results"][0]["observed_ratio"] = 2.0
    loose = {**evidence[0], "id": "ev-2", "cell_results": [{"cell_id": "cell-a", "observed_ratio": 2.38, "validity": deepcopy(A5_VALIDITY)}]}
    assert _one(registry, experiments, hosts, [evidence[0], loose])["cells"][0]["verdict"] == "reproduced (loose)"
    fail = {**evidence[0], "id": "ev-3", "cell_results": [{"cell_id": "cell-a", "observed_ratio": 2.5, "validity": deepcopy(A5_VALIDITY)}]}
    assert _one(registry, experiments, hosts, [evidence[0], fail])["cells"][0]["verdict"] == "inconclusive"
    assert _tie_verdicts(["reproduced", "unverified"]) == "unverified"
    assert _tie_verdicts(["inconclusive", "unverified (stale)"]) == "unverified (stale)"


def test_failed_validity_is_inconclusive_but_completion_ignores_timing_validity():
    registry, experiments, hosts, evidence = _case(cell={
        "cell_id": "cell-a", "observed_ratio": 2.0,
        "validity": {
            "warmups": 2, "sample_count": 41, "repetitions_per_sample": 1,
            "sample_seconds": [1.25] * 41,
            "path_mad_over_median": {"left": 0.06, "right": 0.02},
        },
    })
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "inconclusive"
    registry, experiments, hosts, evidence = _case("completion", cell={"cell_id": "cell-a", "completed": True, "valid": False})
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"


def test_a5_validity_checks_sample_count_duration_and_five_percent_boundary():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["cell_results"][0]["validity"]["path_mad_over_median"] = {"left": 0.05, "right": 0.01}
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["validity"]["sample_seconds"][0] = 0.99
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "inconclusive"
    evidence[0]["cell_results"][0]["validity"]["sample_seconds"] = [1.25] * 41
    evidence[0]["cell_results"][0]["validity"]["sample_count"] = 40
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "inconclusive"


def test_stale_head_digest_is_unverified_stale():
    registry, experiments, hosts, evidence = _case(role="head")
    result = _one(
        registry,
        experiments,
        hosts,
        evidence,
        roles=("head",),
        head_sha=HEAD_SHA,
        current_results_digests={"C-1": "d" * 64},
    )
    assert result["cells"][0]["verdict"] == "unverified (stale)"


def test_head_evidence_applies_when_results_relevant_digest_matches():
    registry, experiments, hosts, evidence = _case(role="head")
    evidence[0]["results_relevant_digest"] = "d" * 64
    result = _one(
        registry,
        experiments,
        hosts,
        evidence,
        roles=("head",),
        head_sha=HEAD_SHA,
        current_results_digests={"C-1": "d" * 64},
    )
    assert result["cells"][0]["verdict"] == "reproduced"


def test_report_overlay_build_failure_requires_decision():
    registry, experiments, hosts, evidence = _case()
    experiments["experiment"][0]["harness_overlays"] = [{"path": "h.rs", "sha256": "a" * 64, "origin_commit": REPORT_SHA}]
    evidence[0]["overlay_build_ok"] = False
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "needs-decision"


def test_unhashed_artifacts_are_ignored_not_promoted_or_failed():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["artifacts"] = [{"path": "unhashed.json"}]
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "unverified"


def test_exact_predicate_compares_registered_values():
    registry, experiments, hosts, evidence = _case("exact", conclusion_type="deterministic", expected={"cell-a": 7}, cell={"cell_id": "cell-a", "observed": 7})
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["observed"] = 8
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "not reproduced"


def test_deterministic_exact_evidence_outranks_archival_recovered_output():
    registry, experiments, hosts, evidence = _case(
        "exact", conclusion_type="deterministic", expected={"cell-a": 7},
        cell={"cell_id": "cell-a", "observed": 7},
    )
    archival = {**evidence[0], "id": "archive", "kind": "structured_recovered_output"}
    result = _one(registry, experiments, hosts, [archival, evidence[0]])
    assert result["cells"][0]["verdict"] == "reproduced"
    assert result["cells"][0]["evidence_ids"] == ["ev-1"]


def test_qualitative_source_kind_is_accepted_for_qualitative_conclusions():
    registry, experiments, hosts, evidence = _case(
        "exact", conclusion_type="qualitative", expected={"cell-a": "source result"},
        cell={"cell_id": "cell-a", "exact_match": True},
    )
    evidence[0]["kind"] = "qualitative_source"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"


def test_exact_predicate_accepts_only_registered_field_normalization():
    registry, experiments, hosts, evidence = _case(
        "exact",
        conclusion_type="deterministic",
        expected={"cell-a": {"schema_version": 2}},
        cell={
            "cell_id": "cell-a",
            "observed": {"schema_version": 1},
            "normalization_id": "m4_05_schema_v1_to_v2",
        },
    )
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["normalization_id"] = "unregistered-normalization"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "needs-decision"


def test_exact_predicate_supports_the_other_registered_normalizations():
    registry, experiments, hosts, evidence = _case(
        "exact",
        conclusion_type="deterministic",
        expected={"cell-a": {"ci95": {"upper": "__positive_infinity__"}}},
        cell={
            "cell_id": "cell-a", "observed": {"ci95": {"upper": "inf"}},
            "normalization_id": "m4_04_unbounded_ci_high", "normalization_fields": ["upper"],
        },
    )
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"

    registry, experiments, hosts, evidence = _case(
        "exact",
        conclusion_type="deterministic",
        expected={"cell-a": {"guarantee": "incomplete"}},
        cell={
            "cell_id": "cell-a", "observed": {"guarantee": "None"},
            "normalization_id": "m4_05_guarantee_labels_v1_to_v2", "search_complete": False,
        },
    )
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"


def test_ratio_band_uses_frozen_ten_and_twenty_percent_edges():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["cell_results"][0]["observed_ratio"] = 2.2
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["observed_ratio"] = 2.4
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced (loose)"
    evidence[0]["cell_results"][0]["observed_ratio"] = 2.5
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "not reproduced"


def test_threshold_side_respects_old_noise_margin():
    registry, experiments, hosts, evidence = _case("threshold_side", cell={"cell_id": "cell-a", "value": 1.6, "noise_bound": 0.1, "valid": True})
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["value"] = 0.9
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "not reproduced"
    experiments["experiment"][0]["predicate_parameters"]["historical_values"] = [1.05]
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "inconclusive"


def test_completion_predicate_passes_only_completed_without_error():
    registry, experiments, hosts, evidence = _case("completion", cell={"cell_id": "cell-a", "completed": True})
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["error"] = "runner failed"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "not reproduced"


def test_no_gain_predicate_uses_abs_effect_and_paired_mad():
    registry, experiments, hosts, evidence = _case("no_gain", cell={"cell_id": "cell-a", "effect": -0.1, "mad": 0.2, "valid": True})
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"
    evidence[0]["cell_results"][0]["effect"] = 0.3
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "not reproduced"


def test_timing_host_mismatch_is_inconclusive():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["toolchain"]["cpu_model"] = "Other CPU"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "inconclusive"


def test_each_disqualifying_timing_host_field_is_inconclusive():
    changes = [
        lambda tc: tc.update(cpu_model="Other CPU"),
        lambda tc: tc.update(physical_cores=8),
        lambda tc: tc.update(logical_cores=16),
        lambda tc: tc.update(rustc="1.99.0"),
        lambda tc: tc.update(rust_target="aarch64-unknown-linux-gnu"),
        lambda tc: tc["a5_settings"].update(samples=40),
    ]
    for change in changes:
        registry, experiments, hosts, evidence = _case()
        change(evidence[0]["toolchain"])
        assert _one(registry, experiments, hosts, evidence)["verdict"] == "inconclusive"


def test_os_build_difference_does_not_disqualify_timing_and_determinism_is_cross_host():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["toolchain"]["os_family"] = "Other Windows"
    evidence[0]["toolchain"]["os_build"] = "different build"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"

    registry, experiments, hosts, evidence = _case("exact", conclusion_type="deterministic", expected={"cell-a": 7}, cell={"cell_id": "cell-a", "observed": 7})
    evidence[0]["toolchain"]["cpu_model"] = "Other CPU"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "reproduced"


def test_unexpected_evidence_kind_type_pair_requires_decision():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["kind"] = "deterministic_exact"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "needs-decision"


def test_structured_recovered_output_is_archival_only():
    registry, experiments, hosts, evidence = _case()
    evidence[0]["kind"] = "structured_recovered_output"
    assert _one(registry, experiments, hosts, evidence)["verdict"] == "unverified"


def test_valid_supersession_removes_the_superseded_record():
    registry, experiments, hosts, evidence = _case()
    old = {**evidence[0], "id": "old", "cell_results": [{"cell_id": "cell-a", "observed_ratio": 2.0, "validity": deepcopy(A5_VALIDITY)}]}
    new = {**evidence[0], "id": "new", "supersedes": ["old"], "cell_results": [{"cell_id": "cell-a", "observed_ratio": 3.0, "validity": deepcopy(A5_VALIDITY)}]}
    result = _one(registry, experiments, hosts, [old, new])
    assert result["cells"][0]["verdict"] == "not reproduced"
    assert result["cells"][0]["evidence_ids"] == ["new"]


def test_invalid_supersession_graph_is_needs_decision():
    registry, experiments, hosts, evidence = _case()
    second = {**evidence[0], "id": "ev-2", "supersedes": ["ev-1"]}
    evidence[0]["supersedes"] = ["ev-2"]
    assert _one(registry, experiments, hosts, [evidence[0], second])["verdict"] == "needs-decision"


def test_cross_conclusion_and_weaker_supersession_are_needs_decision():
    registry, experiments, hosts, evidence = _case()
    cross = {**evidence[0], "id": "ev-2", "conclusion_id": "C-2"}
    evidence[0]["supersedes"] = ["ev-2"]
    assert _one(registry, experiments, hosts, [evidence[0], cross])["verdict"] == "needs-decision"

    registry, experiments, hosts, evidence = _case()
    recovered = {
        **evidence[0], "id": "ev-2", "kind": "structured_recovered_output", "supersedes": ["ev-1"],
    }
    assert _one(registry, experiments, hosts, [evidence[0], recovered])["verdict"] == "needs-decision"


def test_results_relevant_digest_sorts_paths_and_hashes_bytes():
    files = {"crates/sim-core/src/lib.rs": b"core", "Cargo.lock": b"lock"}
    expected = (
        b"Cargo.lock\0" + hashlib.sha256(b"lock").hexdigest().encode() + b"\n"
        + b"crates/sim-core/src/lib.rs\0" + hashlib.sha256(b"core").hexdigest().encode() + b"\n"
    )
    assert results_relevant_digest(files) == hashlib.sha256(expected).hexdigest()
