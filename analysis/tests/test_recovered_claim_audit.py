import json

from simciv_analysis.recovered_claim_audit import _value_type


def test_recovered_value_type_prioritizes_config_then_measure_context():
    assert _value_type({"value": "8", "context": json.dumps({"kind": "table", "column": "k", "column_is_parameter": True})}) == "config/parameter"
    assert _value_type({"value": "1", "unit": "x", "context": json.dumps({"kind": "table", "column": "speedup", "column_is_parameter": False})}) == "measured value"
    assert _value_type({"value": "999", "context": json.dumps({"kind": "table", "column": "events", "column_is_parameter": False})}) == "small integer"
    assert _value_type({"value": "1.25", "context": json.dumps({"kind": "table", "column": "median", "column_is_parameter": False})}) == "measured value"
