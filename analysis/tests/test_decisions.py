from datetime import date
from pathlib import Path

import pytest

from simciv_analysis.decisions import (
    ROOT,
    RegistryError,
    _assert_supersession_acyclic,
    load_registry,
    render_decision_table,
    validate_registry,
)


def _decision(**overrides):
    value = {
        "id": "M5-00-D-1",
        "milestone": "M5",
        "stage": "M5-00",
        "status": "frozen",
        "enforcement": "none",
        "summary": "A frozen decision is represented by one sentence.",
        "enforced_by": [],
        "doc": "README.md",
        "date": date.today().isoformat(),
    }
    value.update(overrides)
    return value


def _stage(**overrides):
    value = {
        "id": "M5-00",
        "milestone": "M5",
        "status": "frozen",
        "summary": "The stage has a recorded status.",
        "doc": "README.md",
    }
    value.update(overrides)
    return value


def _validate(decisions):
    return validate_registry(
        {"decision": decisions, "stage": [_stage()]},
        ROOT,
        test_ids={"analysis/tests/test_decisions.py::test_registry_validates_unique_ids_and_real_enforcement"},
        just_targets={"check"},
        prek_hooks={"cargo-fmt"},
    )


def test_registry_validates_unique_ids_and_real_enforcement():
    registry, unenforced = load_registry()
    ids = [item["id"] for item in registry["decision"]]
    assert len(ids) == len(set(ids))
    assert registry["stage"]
    assert set(unenforced) == {
        item["id"]
        for item in registry["decision"]
        if item["status"] == "frozen" and item["enforcement"] == "none"
    }


def test_validator_rejects_duplicate_decision_ids():
    item = _decision()
    with pytest.raises(RegistryError, match="duplicate decision id"):
        _validate([item, item.copy()])


def test_validator_checks_paths_tests_just_targets_and_prek_hooks():
    item = _decision(
        enforcement="verified",
        enforced_by=[
            "AGENTS.md",
            "test:analysis/tests/test_decisions.py::test_registry_validates_unique_ids_and_real_enforcement",
            "just:check",
            "prek:cargo-fmt",
        ]
    )
    assert _validate([item]) == []
    for bad in ("missing.txt", "test:analysis/tests/no_such_test.py::test_missing", "just:no-such-target", "prek:no-such-hook"):
        with pytest.raises(RegistryError):
            _validate([_decision(enforcement="verified", enforced_by=[bad, item["enforced_by"][1]])])


def test_enforcement_level_validation_and_generated_decision_table():
    test_ref = "test:analysis/tests/test_decisions.py::test_registry_validates_unique_ids_and_real_enforcement"
    assert _validate([_decision(enforcement="documented", enforced_by=["README.md"])]) == []
    assert _validate([_decision(enforcement="exists", enforced_by=["just:check"])]) == []
    verified = _decision(enforcement="verified", enforced_by=[test_ref])
    assert _validate([verified]) == []
    table = render_decision_table([verified], Path("docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md"))
    assert "| ID | Status | Enforcement | Summary | Enforced by | Date | Document |" in table
    assert "| `M5-00-D-1` | frozen | verified |" in table

    for invalid in (
        _decision(enforcement="strong"),
        _decision(enforcement="verified", enforced_by=["README.md"]),
        _decision(enforcement="none", enforced_by=["README.md"]),
        _decision(enforcement="documented", enforced_by=["just:check"]),
    ):
        with pytest.raises(RegistryError):
            _validate([invalid])


def test_superseded_decisions_must_point_to_an_existing_frozen_decision():
    old = _decision(id="M5-03-D-1", status="superseded", superseded_by="M5-INFRA-ACTION-TAGS")
    new = _decision(id="M5-INFRA-ACTION-TAGS", stage="M5-00")
    assert _validate([old, new]) == ["M5-INFRA-ACTION-TAGS"]
    with pytest.raises(RegistryError, match="existing frozen"):
        _validate([old])
    with pytest.raises(RegistryError, match="existing frozen"):
        _validate([old, _decision(id="M5-INFRA-ACTION-TAGS", stage="M5-00", status="draft")])


def test_supersession_graph_rejects_cycles():
    with pytest.raises(RegistryError, match="cycle"):
        _assert_supersession_acyclic(
            [
                {"id": "a", "superseded_by": "b"},
                {"id": "b", "superseded_by": "a"},
            ]
        )
