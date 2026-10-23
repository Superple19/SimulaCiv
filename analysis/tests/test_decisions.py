from datetime import date

import pytest

from simciv_analysis.decisions import (
    ROOT,
    RegistryError,
    _assert_supersession_acyclic,
    load_registry,
    validate_registry,
)


def _decision(**overrides):
    value = {
        "id": "M5-00-D-1",
        "milestone": "M5",
        "stage": "M5-00",
        "status": "frozen",
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
        if item["status"] == "frozen" and not item["enforced_by"]
    }


def test_validator_rejects_duplicate_decision_ids():
    item = _decision()
    with pytest.raises(RegistryError, match="duplicate decision id"):
        _validate([item, item.copy()])


def test_validator_checks_paths_tests_just_targets_and_prek_hooks():
    item = _decision(
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
            _validate([_decision(enforced_by=[bad])])


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
