# M5-05 M0–M3 Conclusion Regeneration


Status: in progress — Session 1 complete; Sessions 2–3 not run

## Question

Do the selected conclusion-bearing M2/M3 timing results and mapped
deterministic results still support the claims in the M0–M3 reports?

## Method

The frozen conclusion set is
[`m5_05_conclusions.toml`](m5_05_conclusions.toml). Each timing stage
runs the committed [measurement
example](../../crates/sim-model/examples/m5_05_conclusion_measure.rs)
with A-5 sampling; every timing DVC output has a Git summary and a raw
output directory. This report reads only Git summaries, not DVC raw
paths. Historical values are extracted from the named reports by
[m5_05.py](../../analysis/simciv_analysis/m5_05.py). Session 1 cell data
and timing are collected in the summaries under `docs/data/m5_05/`.

## Results

### Conclusion status

| milestone | conclusion | old values | new values | new/old ratios | derived | \>20% flag | status | notes | harness | measurement harness | summary | source report |
|----|----|----|----|----|----|----|----|----|----|----|----|----|
| M2 | m2_near_linear | B-N50000 rayon_median_us 8662.200 µs; B-N50000 serial_median_us 10081.030 µs; B-N100000 rayon_median_us 18210.730 µs; B-N100000 serial_median_us 21632.200 µs | B-N50000 M2Serial 10211.687 µs; M2Phase4Rayon 8202.295 µs; B-N100000 M2Serial 22061.410 µs; M2Phase4Rayon 17604.713 µs | B-N50000 serial 1.013×; B-N50000 rayon 0.947×; B-N100000 serial 1.020×; B-N100000 rayon 0.967× | 50k→100k α: serial 1.111; explicit Rayon 1.102 | false | inconclusive | Representative cells were remeasured; the headline covers a broader population/grid than this session. | [m0_baseline_bench.rs](../../crates/sim-model/benches/m0_baseline_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | [summary](../data/m5_05/m2_near_linear/summary.json) | [M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md](../../docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md) |
| M2 | m2_final_tick | A-N100000 rayon_median_us 16997.300 µs; A-N100000 serial_median_us 20371.400 µs; B-N100000 rayon_median_us 18210.730 µs; B-N100000 serial_median_us 21632.200 µs | A-N100000 M2Serial 21666.477 µs; M2Phase4Rayon 16672.317 µs; B-N100000 M2Serial 21304.590 µs; M2Phase4Rayon 16661.217 µs | A-N100000 serial 1.064×; A-N100000 rayon 0.981×; B-N100000 serial 0.985×; B-N100000 rayon 0.915× | — | false | does not hold | Selected 100k serial and explicit-Rayon medians compared with the recorded 17–22 ms range. | [m0_baseline_bench.rs](../../crates/sim-model/benches/m0_baseline_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | [summary](../data/m5_05/m2_final_tick/summary.json) | [M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md](../../docs/reports/M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md) |
| M2 | m2_phase4_crossover | B-N1000 reported_best_rayon_us 159.930 µs; B-N1000 reported_best_serial_us 153.370 µs; B-N5000 reported_best_rayon_us 760.270 µs; B-N5000 reported_best_serial_us 873.330 µs; B-N10000 reported_best_rayon_us 1481.230 µs; B-N10000 reported_best_serial_us 1759.570 µs | B-N1000-T6 M2Serial 166.838 µs; M2Phase4Rayon 175.732 µs; B-N5000-T6 M2Serial 961.954 µs; M2Phase4Rayon 826.381 µs; B-N10000-T1 M2Serial 1883.226 µs; M2Phase4Rayon 2117.734 µs | — | — | false | inconclusive | Full-tick ratios (serial/Rayon) were 1k=0.949, 5k=1.164, 10k one-thread=0.889; combined-P4 timing was not separately measured. | [m0_baseline_bench.rs](../../crates/sim-model/benches/m0_baseline_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | [summary](../data/m5_05/m2_phase4_crossover/summary.json) | [M2_38_DETERMINISTIC_RAYON_PHASE4_GATE.md](../../docs/reports/M2_38_DETERMINISTIC_RAYON_PHASE4_GATE.md) |
| M3 | m3_01_serial_baseline | N10000-k8-p0.5 storage_median_us 2099.000 µs; N50000-k8-p0.5 storage_median_us 12464.400 µs; N100000-k8-p0.5 storage_median_us 26744.900 µs | M3-storage-replay-N10000 Storage 1827.681 µs; Storage 1826.967 µs; M3-storage-replay-N50000 Storage 10232.533 µs; Storage 10178.590 µs; M3-storage-replay-N100000 Storage 22683.518 µs; Storage 22513.264 µs | M3-storage-replay-N10000 M3 storage 0.870×; M3-storage-replay-N50000 M3 storage 0.817×; M3-storage-replay-N100000 M3 storage 0.842× | serial storage α: 10k→50k 1.067; 50k→100k 1.145 | false | inconclusive | Representative cells were remeasured; the headline covers a broader population/grid than this session. | [m3_01_serial_baseline_bench.rs](../../crates/sim-model/benches/m3_01_serial_baseline_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | [summary](../data/m5_05/m3_01_serial_baseline/summary.json) | [M3_01_SERIAL_BASELINE.md](../../docs/reports/M3_01_SERIAL_BASELINE.md) |
| M3 | m3_02_parallel_phase6b | N10000-p0.5-pool6-chunk256 parallel_median_us 2337.400 µs; N10000-p0.5-pool6-chunk256 serial_median_us 2356.200 µs; N50000-p0.5-pool6-chunk256 parallel_median_us 11032.967 µs; N50000-p0.5-pool6-chunk256 serial_median_us 10583.533 µs; N100000-p0.5-pool6-chunk256 parallel_median_us 25183.700 µs; N100000-p0.5-pool6-chunk256 serial_median_us 24562.033 µs | M3-serial-parallel-N10000 Storage 1834.198 µs; Parallel 2127.688 µs; M3-serial-parallel-N50000 Storage 10446.350 µs; Parallel 10898.623 µs; M3-serial-parallel-N100000 Storage 23055.215 µs; Parallel 23405.779 µs | M3-serial-parallel-N10000 serial 0.778×; M3-serial-parallel-N10000 parallel 0.910×; M3-serial-parallel-N50000 serial 0.987×; M3-serial-parallel-N50000 parallel 0.988×; M3-serial-parallel-N100000 serial 0.939×; M3-serial-parallel-N100000 parallel 0.929× | — | true | holds | All selected full-tick cells show no parallel gain; the historical 180-cell aggregate is not recomputed. | [m3_02_parallel_phase6b_bench.rs](../../crates/sim-model/benches/m3_02_parallel_phase6b_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | [summary](../data/m5_05/m3_02_parallel_phase6b/summary.json) | [M3_02_PARALLEL_PHASE6B.md](../../docs/reports/M3_02_PARALLEL_PHASE6B.md) |
| M3 | m3_03_search_reduction | N10000-p0.5 paired_saving_us 28.967 µs; N10000-p0.5 search_reduction_median_us 2187.267 µs; N10000-p0.5 serial_median_us 2190.533 µs; N100000-p0.5 paired_saving_us 622.233 µs; N100000-p0.5 search_reduction_median_us 32223.167 µs; N100000-p0.5 serial_median_us 32049.700 µs | M3-serial-search-N10000 Storage 1827.543 µs; SearchReduction 1828.875 µs; M3-serial-search-N100000 Storage 22777.158 µs; SearchReduction 23016.361 µs | M3-serial-search-N10000 serial 0.834×; M3-serial-search-N10000 search reduction 0.836×; M3-serial-search-N100000 serial 0.711×; M3-serial-search-N100000 search reduction 0.714× | — | true | holds | The selected paired savings do not exceed their paired MADs. | [m3_02b_search_reduction_bench.rs](../../crates/sim-model/benches/m3_02b_search_reduction_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | [summary](../data/m5_05/m3_03_search_reduction/summary.json) | [M3_03_SEARCH_REDUCTION.md](../../docs/reports/M3_03_SEARCH_REDUCTION.md) |
| M3 | m3_06_settlement_index | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | [m3_scale_limit_bench.rs](../../crates/sim-model/benches/m3_scale_limit_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_06_SETTLEMENT_INDEX.md](../../docs/reports/M3_06_SETTLEMENT_INDEX.md) |
| M3 | m3_07_reversed_slots | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | [m3_scale_limit_bench.rs](../../crates/sim-model/benches/m3_scale_limit_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_07_SCALE_RESIDUAL.md](../../docs/reports/M3_07_SCALE_RESIDUAL.md) |
| M3 | m3_08_full_rebuild_equal_live | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | source unavailable: `crates/sim-model/examples/m5_05_equal_live.rs` | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_08_POPULATION_DYNAMICS.md](../../docs/reports/M3_08_POPULATION_DYNAMICS.md) |
| M3 | m3_09_incremental_equal_live | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | source unavailable: `crates/sim-model/examples/m5_05_equal_live.rs` | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_09_INCREMENTAL_TOPOLOGY.md](../../docs/reports/M3_09_INCREMENTAL_TOPOLOGY.md) |
| M3 | m3_11_phase9c_speedup | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | [m3_11_propagation_bench.rs](../../crates/sim-model/benches/m3_11_propagation_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_11_PROPAGATION.md](../../docs/reports/M3_11_PROPAGATION.md) |
| M3 | m3_11_whole_tick_speedup | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | [m3_11_propagation_bench.rs](../../crates/sim-model/benches/m3_11_propagation_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_11_PROPAGATION.md](../../docs/reports/M3_11_PROPAGATION.md) |
| M3 | m3_11_frontier_crossover | not exactly mapped | not run | — | — | false | inconclusive | No Session 1 summary; later session not run. | [m3_11_propagation_bench.rs](../../crates/sim-model/benches/m3_11_propagation_bench.rs) | [m5_05_conclusion_measure.rs](../../crates/sim-model/examples/m5_05_conclusion_measure.rs) | not recorded | [M3_11_PROPAGATION.md](../../docs/reports/M3_11_PROPAGATION.md) |

### Planned conclusion set

| conclusion                    | session | estimate_minutes | status   |
|-------------------------------|---------|------------------|----------|
| m2_near_linear                | 1       | 20               | recorded |
| m2_final_tick                 | 1       | 20               | recorded |
| m2_phase4_crossover           | 1       | 30               | recorded |
| m3_01_serial_baseline         | 1       | 25               | recorded |
| m3_02_parallel_phase6b        | 1       | 45               | recorded |
| m3_03_search_reduction        | 1       | 25               | recorded |
| m3_06_settlement_index        | 2       | 50               | not run  |
| m3_07_reversed_slots          | 2       | 35               | not run  |
| m3_08_full_rebuild_equal_live | 2       | 40               | not run  |
| m3_09_incremental_equal_live  | 2       | 40               | not run  |
| m3_11_phase9c_speedup         | 3       | 45               | not run  |
| m3_11_whole_tick_speedup      | 3       | 40               | not run  |
| m3_11_frontier_crossover      | 3       | 35               | not run  |

### Session 1 selected cells

| conclusion | cell | N | K | k | p_local | left µs/tick | left MAD | right µs/tick | right MAD | speedup | paired delta µs | paired delta MAD | r | minimum sample s |
|----|----|----|----|----|----|----|----|----|----|----|----|----|----|----|
| m2_near_linear | B-N50000 | 50,000 | 250 |  |  | 10,211.686667 | 69.141667 | 8,202.295 | 65.155 | 1.244979 | 1,990.955 | 74.97 | 20 | 1.083478 |
| m2_near_linear | B-N100000 | 100,000 | 500 |  |  | 22,061.41 | 87.906667 | 17,604.713333 | 145.88 | 1.253154 | 4,415.056667 | 171.78 | 10 | 1.173299 |
| m2_final_tick | A-N100000 | 100,000 | 2 |  |  | 21,666.476667 | 206.556667 | 16,672.316667 | 205.943333 | 1.299548 | 4,952.926667 | 158.513333 | 10 | 1.118902 |
| m2_final_tick | B-N100000 | 100,000 | 500 |  |  | 21,304.59 | 106.263333 | 16,661.216667 | 101.866667 | 1.278694 | 4,649.01 | 128.526667 | 10 | 1.128116 |
| m2_phase4_crossover | B-N1000-T6 | 1,000 | 5 |  |  | 166.837654 | 0.328002 | 175.732435 | 0.680527 | 0.949385 | -8.817593 | 0.606257 | 1,188 | 1.206202 |
| m2_phase4_crossover | B-N5000-T6 | 5,000 | 25 |  |  | 961.953998 | 11.380689 | 826.380689 | 4.32583 | 1.164057 | 137.709471 | 10.512177 | 271 | 1.421949 |
| m2_phase4_crossover | B-N10000-T1 | 10,000 | 50 |  |  | 1,883.2264 | 3.898933 | 2,117.734133 | 10.438933 | 0.889265 | -232.008533 | 9.496 | 125 | 1.473204 |
| m3_01_serial_baseline | M3-storage-replay-N10000 | 10,000 | 50 | 8 | 0.5 | 1,827.681111 | 6.019444 | 1,826.967222 | 4.336944 | 1.000391 | -0.2675 | 5.257778 | 120 | 1.308865 |
| m3_01_serial_baseline | M3-storage-replay-N50000 | 50,000 | 250 | 8 | 0.5 | 10,232.533333 | 85.825 | 10,178.590278 | 68.522222 | 1.0053 | 36.634722 | 53.679167 | 24 | 1.449536 |
| m3_01_serial_baseline | M3-storage-replay-N100000 | 100,000 | 500 | 8 | 0.5 | 22,683.518182 | 134.866667 | 22,513.263636 | 196.742424 | 1.007562 | 212.015152 | 169.251515 | 11 | 1.474312 |
| m3_02_parallel_phase6b | M3-serial-parallel-N10000 | 10,000 | 50 | 8 | 0.5 | 1,834.197576 | 4.398182 | 2,127.688485 | 13.075152 | 0.862061 | -294.623939 | 11.391818 | 110 | 1.296084 |
| m3_02_parallel_phase6b | M3-serial-parallel-N50000 | 50,000 | 250 | 8 | 0.5 | 10,446.35 | 71.921212 | 10,898.622727 | 85.374242 | 0.958502 | -417.756061 | 92.366667 | 22 | 1.382715 |
| m3_02_parallel_phase6b | M3-serial-parallel-N100000 | 100,000 | 500 | 8 | 0.5 | 23,055.215152 | 146.315152 | 23,405.778788 | 177.163636 | 0.985022 | -299.372727 | 218.636364 | 11 | 1.516092 |
| m3_03_search_reduction | M3-serial-search-N10000 | 10,000 | 50 | 8 | 0.5 | 1,827.543333 | 4.736389 | 1,828.875 | 8.990833 | 0.999272 | 0.589722 | 10.017222 | 120 | 1.306926 |
| m3_03_search_reduction | M3-serial-search-N100000 | 100,000 | 500 | 8 | 0.5 | 22,777.157576 | 247.242424 | 23,016.360606 | 239.754545 | 0.989607 | -195.881818 | 237.554545 | 11 | 1.484671 |

### Deterministic confirmations

| target | status | harness | summary |
|----|----|----|----|
| determinism_oracle_tests | pass | [determinism_oracle_tests.rs](../../crates/sim-model/tests/determinism_oracle_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |
| m3_parallel_phase6b_tests | pass | [m3_parallel_phase6b_tests.rs](../../crates/sim-model/tests/m3_parallel_phase6b_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |
| m3_parallel_runner_tests | pass | [m3_parallel_runner_tests.rs](../../crates/sim-model/tests/m3_parallel_runner_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |
| m3_reference_runner_tests | pass | [m3_reference_runner_tests.rs](../../crates/sim-model/tests/m3_reference_runner_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |
| m3_search_reduction_tests | pass | [m3_search_reduction_tests.rs](../../crates/sim-model/tests/m3_search_reduction_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |
| m3_storage_runner_tests | pass | [m3_storage_runner_tests.rs](../../crates/sim-model/tests/m3_storage_runner_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |
| phase4_rayon_tests | pass | [phase4_rayon_tests.rs](../../crates/sim-model/tests/phase4_rayon_tests.rs) | [deterministic summary](../data/m5_05/deterministic/summary.json) |

### Timing comparison figure

![Recorded and M5-05 selected timings](figures/m5_05_old_new_timing.svg)

### Sessions not run

Session 2 and Session 3 are **not run** in this execution. Their
conclusion rows remain inconclusive. This report does not substitute
selected cells for the broader historical grid or its aggregate
statistics.

## Findings

- Each conclusion status distinguishes direct representative-cell
  evidence from unsupported full-grid aggregates; see the generated
  status table.
- Exact selected-cell path ratios are flagged when `|ratio - 1| > 20%`;
  the flag does not itself decide whether the conclusion holds.
- Deterministic test targets ran without changing existing expected
  values; see the test result table.
- Unrun sessions remain inconclusive and are not counted as reproduced.

## Limits

- Session 1 uses representative cells; M2-02 and other historical grid
  aggregates remain inconclusive from samples.
- Timing is specific to the recorded reference host and A-5 sampling
  protocol.
- No raw DVC object is needed to run `just check`; summaries are the
  only report inputs.
