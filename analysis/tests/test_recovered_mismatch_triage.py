import hashlib
import json

from simciv_analysis.io import write_text_lf
from simciv_analysis.recovered_claims import build_mapping
from simciv_analysis.recovered_claims import _claim_context
from simciv_analysis.recovered_mismatch_triage import _printed_equal


def _mapping(tmp_path, name, content, claim):
    stage = tmp_path / "docs" / "data" / "m5_recovered"
    recovered = stage / "recovered"
    recovered.mkdir(parents=True, exist_ok=True)
    output_path = recovered / name
    output_path.write_bytes(content)
    write_text_lf(
        stage / "MANIFEST.json",
        json.dumps(
            {
                "items": [
                    {
                        "path": f"recovered/{name}",
                        "type": "raw-output-copy",
                        "reports_supported": ["sample.md"],
                        "size_bytes": len(content),
                        "sha256": hashlib.sha256(content).hexdigest(),
                    }
                ]
            }
        ),
    )
    return build_mapping([claim], tmp_path)["claims"][0]


def _claim(value, column, dimensions, unit="", component_index=None):
    context = {"kind": "table", "column": column, "column_is_parameter": False, "dimensions": dimensions}
    if component_index is not None:
        context["component_index"] = component_index
    return {
        "claim_id": 1,
        "report": "sample.md",
        "value": value,
        "unit": unit,
        "tier": "supporting",
        "status": "missing-data",
        "context": json.dumps(
            context
        ),
    }


def test_rounding_and_composite_components_are_recovered_as_formatted(tmp_path):
    rounded = _mapping(
        tmp_path / "rounding",
        "out.csv",
        b"N,median_us_tick\n10000,1.234\n",
        _claim("1.2", "median_us_tick", {"population_n": "10000"}, "µs"),
    )
    assert rounded["status_after"] == "recovered-original-output (formatted)"
    assert rounded["triage_subcategory"] == "printed-precision rounding"

    component = _mapping(
        tmp_path / "component",
        "out.csv",
        b"N,gates\n10000,2/6\n",
        _claim("2", "gates", {"population_n": "10000"}, component_index=0),
    )
    assert component["status_after"] == "recovered-original-output (formatted)"
    assert component["triage_subcategory"] == "composite field component at matching position"
    wrong_position = _mapping(
        tmp_path / "wrong-position",
        "out.csv",
        b"N,gates\n10000,6/2\n",
        _claim("2", "gates", {"population_n": "10000"}, component_index=0),
    )
    assert wrong_position["status_after"] == "recovered-weak (composite field)"
    assert wrong_position["triage_subcategory"] == "composite field component at different or unknown position"
    missing_position = _mapping(
        tmp_path / "missing-position",
        "out.csv",
        b"N,gates\n10000,2\n",
        _claim("2", "gates", {"population_n": "10000"}, component_index=1),
    )
    assert missing_position["status_after"] == "recovered-weak (composite field)"
    assert _printed_equal("+1,234.00%", "1234.0")


def test_unit_conversion_and_other_run_are_classified_separately(tmp_path):
    converted = _mapping(
        tmp_path / "units",
        "out.csv",
        b"N,median_ms\n10000,0.001\n",
        _claim("0.000001", "median_ms", {"population_n": "10000"}, "s"),
    )
    assert converted["status_after"] == "recovered-original-output (unit-converted)"

    other_run = _mapping(
        tmp_path / "runs",
        "out.csv",
        b"N,median_us_tick\n10000,1.25\n10000,1.4\n",
        _claim("1.25", "median_us_tick", {"population_n": "10000"}, "µs"),
    )
    assert other_run["status_after"] == "recovered-other-run"

    aggregate = _mapping(
        tmp_path / "aggregate",
        "out.csv",
        b"N,median_us_tick\n10000,1\n10000,3\n",
        _claim("2", "median_us_tick", {"population_n": "10000"}, "µs"),
    )
    assert aggregate["status_after"] == "recovered-other-run"
    assert aggregate["triage_subcategory"] == "mean across same-context records"


def test_derived_value_and_genuine_discrepancy_are_distinguished(tmp_path):
    derived = _mapping(
        tmp_path / "derived",
        "out.jsonl",
        b'{"N":10,"serial":10,"parallel":5,"speedup":3}\n',
        _claim("2", "speedup", {"population_n": "10"}, "x"),
    )
    assert derived["status_after"] == "recovered-derived"

    genuine = _mapping(
        tmp_path / "genuine",
        "out.jsonl",
        b'{"N":10,"speedup":3}\n',
        _claim("2", "speedup", {"population_n": "10"}, "x"),
    )
    assert genuine["status_after"] == "recovered-output-mismatch"

    total = _mapping(
        tmp_path / "sum",
        "out.jsonl",
        b'{"N":10,"wealth_agents":4,"wealth_treasury":6,"total_wealth":15}\n',
        _claim("10", "total_wealth", {"population_n": "10"}),
    )
    assert total["status_after"] == "recovered-derived"
    assert total["triage_subcategory"] == "wealth_agents+wealth_treasury"

    difference = _mapping(
        tmp_path / "difference",
        "out.jsonl",
        b'{"N":10,"before_us":15,"after_us":10,"delta_us":7}\n',
        _claim("5", "delta_us", {"population_n": "10"}, "µs"),
    )
    assert difference["status_after"] == "recovered-derived"
    assert difference["triage_subcategory"] == "before_us-after_us"


def test_one_time_graph_and_projection_columns_keep_their_own_output_fields():
    graph = _claim_context({
        "unit": "µs",
        "context": json.dumps({
            "kind": "table", "column": "graph build median ± MAD (µs)",
            "subfield": "median", "dimensions": {"population_n": "10000"},
        }),
    })
    projection = _claim_context({
        "unit": "µs",
        "context": json.dumps({
            "kind": "table", "column": "projection build median ± MAD (µs)",
            "subfield": "MAD", "dimensions": {"population_n": "10000"},
        }),
    })

    assert graph["label"] == "graph_median_us"
    assert projection["label"] == "projection_mad_us"
