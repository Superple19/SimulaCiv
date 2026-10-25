from simciv_analysis.reconstruct_all import RUN_ORDER, _print_dry_run, dry_run


def test_plan_lists_all_remaining_timed_groups_in_fixed_order(capsys):
    plan = dry_run()

    assert [group["run_id"] for group in plan["groups"]] == list(RUN_ORDER)
    assert plan["a5_cells"] == 114
    assert plan["timing_cells_total"] == 130
    assert abs(plan["total_estimated_seconds"] - 9439.1) < 0.1
    assert plan["remaining_group_rows"] == 76
    assert plan["zero_cell_group_rows"] == 58
    assert len(plan["zero_cell_details"]) == 58
    assert all(
        sum(row["cells"] for row in group["experiments"]) == group["cells"]
        for group in plan["groups"]
    )

    _print_dry_run(plan)
    output = capsys.readouterr().out
    for run_id in RUN_ORDER:
        assert run_id in output
    assert "N=100000/k=8/day=0" in output
    assert "M3-05 protocol-sensitivity add-on: 16 unique profile/N cells" in output


def test_m3_05_protocol_sensitivity_is_pre_registered_before_any_run():
    plan = dry_run()

    assert len(plan["protocol_cells"]) == 16
    assert plan["protocol_target_contrasts"] == 15
    assert plan["protocol_estimate_seconds"] is not None
