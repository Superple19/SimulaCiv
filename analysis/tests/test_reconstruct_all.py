import csv

from simciv_analysis.provenance import ROOT
from simciv_analysis.reconstruct_all import RUN_ORDER, _print_dry_run, dry_run


def test_plan_lists_all_remaining_timed_groups_in_fixed_order(capsys):
    plan = dry_run()

    run_ids = [group["run_id"] for group in plan["groups"]]
    assert run_ids == [run_id for run_id in RUN_ORDER if run_id in set(run_ids)]
    with (ROOT / "docs/reproducibility/M2_M3_EVIDENCE_RECONSTRUCTION.csv").open(
        encoding="utf-8-sig", newline=""
    ) as stream:
        source_rows = list(csv.DictReader(stream))
    scheduled_rows = [
        row for row in source_rows
        if row.get("classification") in {"a", "b", "c", "d"}
        and row.get("experiment_id") != "M3-05-FIRST-BREAK"
    ]
    source_cells = sum(int(row.get("A5_cells_to_schedule") or 0) for row in scheduled_rows)
    source_zero_cell_rows = sum(int(row.get("A5_cells_to_schedule") or 0) <= 0 for row in scheduled_rows)
    assert plan["a5_cells"] == source_cells
    assert plan["timing_cells_total"] == source_cells + len(plan["protocol_cells"])
    assert plan["total_estimated_seconds"] == plan["a5_model_seconds"] + plan["protocol_estimate_seconds"]
    assert plan["remaining_group_rows"] == len(scheduled_rows)
    assert plan["zero_cell_group_rows"] == source_zero_cell_rows
    assert len(plan["zero_cell_details"]) == source_zero_cell_rows
    assert all(
        sum(row["cells"] for row in group["experiments"]) == group["cells"]
        for group in plan["groups"]
    )

    _print_dry_run(plan)
    output = capsys.readouterr().out
    for run_id in run_ids:
        assert run_id in output
    assert "N=100000/k=8/day=0" in output
    assert "M3-05 protocol-sensitivity add-on: 16 unique profile/N cells" in output


def test_m3_05_protocol_sensitivity_is_pre_registered_before_any_run():
    plan = dry_run()

    assert len(plan["protocol_cells"]) == 16
    assert plan["protocol_target_contrasts"] == 15
    assert plan["protocol_estimate_seconds"] is not None
