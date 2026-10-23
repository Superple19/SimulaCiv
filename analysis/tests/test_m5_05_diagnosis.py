from simciv_analysis.m5_05_diagnosis import historical_decomposition, repetitions_for_target


def test_diagnosis_decomposition_reads_reported_absolute_paths():
    result = historical_decomposition()

    assert result["comparison_reference"] == "M3-09 follow-up paired"
    assert result["rows"][0]["numerator_m3p_incremental_us"] == 28802
    assert result["rows"][0]["denominator_m3_us"] == 11145
    assert result["dominant_component"] == "both, dominated by numerator"
    assert result["numerator_change_percent"] > 40
    assert -10 < result["denominator_change_percent"] < 0


def test_a5_repetitions_use_the_slowest_calibration_probe():
    assert repetitions_for_target([0.25, 0.3, 0.28], 1.5) == 6
