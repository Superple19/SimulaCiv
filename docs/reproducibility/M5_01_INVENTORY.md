# M5-01 Reproducibility Inventory


Status: current

## Question

Which numeric statements in the existing M0–M4 performance reports and
contract evidence have a reproducible source, and which evidence gaps
remain?

## Method

The committed extractor
[claims.py](../../analysis/simciv_analysis/claims.py) reads the
report/contract source tree and assigns provenance from
[`sources.toml`](sources.toml). Its tidy output
[claims.csv.gz](claims.csv.gz) contains **32,465** rows; the prior
packed ledger [superseded/claims.csv](superseded/claims.csv) contains
**37,612** expanded scalar values. M5-04 statuses are applied only to
source-map entries assigned to that stage. Excluded inputs are table
columns or labeled assignments for population/workload sizes, group and
neighbor counts, seeds, replicates, horizons/days, thread/chunk
settings, and simulation parameter values. Free-text numbers are only
extracted from tables, report Summary/Findings/Status sections, selected
audit/graduation tables, and measured contract sections. The
machine-readable contracts/m1_contract.toml is a configuration/contract
input and is not scanned as a measured report.

## Results

Every table below is generated from [claims.csv.gz](claims.csv.gz) by
the report generator
[report_m5_01.py](../../analysis/simciv_analysis/report_m5_01.py).
Old-ledger matching is diagnostic only and does not populate source
paths, statuses, or M5 stages.

The tidy ledger contains **783** headline claims, **895** key claims,
and **30,787** supporting claims.

### Headline counts by milestone

| milestone | headline claims |
|-----------|-----------------|
| M2        | 288             |
| M3        | 412             |
| M4        | 83              |

Headline rows come from report Summary, Findings, and Status sections
plus audit/graduation tables; full gate tables are excluded. Other gate
verdicts remain in the `key` tier.

### Counts by milestone, class, status, and tier

| milestone | class | status | tier | claims |
|----|----|----|----|----|
| M0 | — | — | — | 0 |
| M1 | derived | historical-log | key | 3 |
| M1 | derived | historical-log | supporting | 29 |
| M1 | deterministic | historical-log | supporting | 2 |
| M2 | derived | analysis-only | headline | 67 |
| M2 | derived | analysis-only | key | 9 |
| M2 | derived | analysis-only | supporting | 204 |
| M2 | derived | missing-data | headline | 92 |
| M2 | derived | missing-data | key | 5 |
| M2 | derived | missing-data | supporting | 760 |
| M2 | derived | recovered-original-output | headline | 36 |
| M2 | derived | recovered-original-output | key | 2 |
| M2 | derived | recovered-original-output | supporting | 2,371 |
| M2 | deterministic | analysis-only | headline | 7 |
| M2 | deterministic | analysis-only | supporting | 32 |
| M2 | deterministic | missing-source | headline | 2 |
| M2 | deterministic | missing-source | key | 5 |
| M2 | deterministic | missing-source | supporting | 57 |
| M2 | deterministic | reproducible-now | headline | 26 |
| M2 | deterministic | reproducible-now | key | 3 |
| M2 | deterministic | reproducible-now | supporting | 89 |
| M2 | timing | analysis-only | headline | 14 |
| M2 | timing | analysis-only | key | 9 |
| M2 | timing | analysis-only | supporting | 205 |
| M2 | timing | missing-data | headline | 18 |
| M2 | timing | missing-data | key | 1 |
| M2 | timing | missing-data | supporting | 397 |
| M2 | timing | recovered-original-output | headline | 26 |
| M2 | timing | recovered-original-output | supporting | 289 |
| M3 | derived | missing-data | headline | 45 |
| M3 | derived | missing-data | key | 4 |
| M3 | derived | missing-data | supporting | 909 |
| M3 | derived | missing-source | headline | 8 |
| M3 | derived | missing-source | supporting | 152 |
| M3 | derived | recovered-original-output | headline | 254 |
| M3 | derived | recovered-original-output | key | 13 |
| M3 | derived | recovered-original-output | supporting | 12,503 |
| M3 | deterministic | missing-source | headline | 1 |
| M3 | deterministic | missing-source | supporting | 18 |
| M3 | deterministic | recovered-original-output | headline | 5 |
| M3 | deterministic | recovered-original-output | supporting | 113 |
| M3 | deterministic | reproducible-now | supporting | 8 |
| M3 | timing | conclusion holds | headline | 22 |
| M3 | timing | inconclusive | headline | 12 |
| M3 | timing | missing-data | headline | 5 |
| M3 | timing | missing-data | key | 5 |
| M3 | timing | missing-data | supporting | 599 |
| M3 | timing | missing-source | headline | 10 |
| M3 | timing | missing-source | supporting | 33 |
| M3 | timing | recovered-original-output | headline | 50 |
| M3 | timing | recovered-original-output | key | 3 |
| M3 | timing | recovered-original-output | supporting | 8,524 |
| M4 | derived | bit-exact | headline | 19 |
| M4 | derived | bit-exact | key | 3 |
| M4 | derived | bit-exact | supporting | 1,664 |
| M4 | derived | bit-exact after documented normalization | headline | 9 |
| M4 | derived | bit-exact after documented normalization | key | 2 |
| M4 | derived | bit-exact after documented normalization | supporting | 1,327 |
| M4 | derived | conclusion holds | key | 111 |
| M4 | derived | conclusion holds | supporting | 122 |
| M4 | derived | historical-protocol | headline | 46 |
| M4 | derived | historical-protocol | key | 216 |
| M4 | derived | historical-protocol | supporting | 102 |
| M4 | derived | missing-source | key | 5 |
| M4 | derived | missing-source | supporting | 44 |
| M4 | derived | recovered-original-output | key | 20 |
| M4 | derived | unrecoverable | supporting | 7 |
| M4 | deterministic | historical-protocol | key | 24 |
| M4 | deterministic | historical-protocol | supporting | 25 |
| M4 | deterministic | missing-source | supporting | 3 |
| M4 | timing | conclusion holds | key | 106 |
| M4 | timing | conclusion holds | supporting | 43 |
| M4 | timing | historical-protocol | headline | 9 |
| M4 | timing | historical-protocol | key | 266 |
| M4 | timing | historical-protocol | supporting | 110 |
| M4 | timing | missing-source | key | 47 |
| M4 | timing | missing-source | supporting | 5 |
| M4 | timing | recovered-original-output | key | 27 |
| M4 | timing | reproducible-now | supporting | 20 |
| M5 | derived | missing-source | key | 6 |
| M5 | derived | missing-source | supporting | 21 |

### Old and tidy ledger comparison

| Ledger view | rows | mapped before (exact name/line/value) | matched after (informational) | unmatched old rows | reason | matched to old ledger (informational) | unmatched old-ledger comparison |
|----|----|----|----|----|----|----|----|
| old packed records | 37,612 | 653 | 1,988 | 35,624 | RENAMES.md report mapping; normalized source text and value; line only breaks ties |  |  |
| new tidy records | 32,465 |  |  |  | sources.toml and regeneration results supply provenance; old matches are diagnostics only | 3,695 | 28,770 |

#### Match and unmapped reasons

| old record disposition | records | rule |
|----|----|----|
| old rows matched (informational) | 1,988 | RENAMES.md plus normalized source/value; line breaks ties; does not assign new provenance |
| old rows unmatched | 35,624 | includes excluded inputs, changed/unselected text, and ambiguous old matches |
| new rows ambiguous | 731 | current claims with conflicting metadata or tied candidates; these are new-row counts |
| historical-log subset of transferred rows | 29 | M1 aggregate runs are not regeneration gaps |
| previous exact-name/line/value baseline | 653 | old rows matched before the correction; diagnostic baseline |
| old report basenames in RENAMES.md | 29,669 | diagnostic count; this may overlap matched and unmatched rows |

The old ledger text-matches **3,695** new claims for comparison;
**28,770** do not match. These matches do not transfer status or
provenance. The extractor assigns harness, data, status, and M5 stage
from [`sources.toml`](sources.toml) and the committed regeneration
results.

### M1 unrecoverable-claim recheck

The **29** previously unrecoverable M1 aggregate claims were rechecked
against current tests, goldens, and the M1 manifest. All remain
`historical-log`: current tests and canonical artifacts revalidate
behavior, not the original aggregate test-run totals. The current
extractor also selects **5** additional numeric values from those
historical M1 sections, for **34** historical-log values total. Evidence
includes [golden PRNG
vectors](../../crates/sim-core/tests/golden_prng_tests.rs), [determinism
oracles](../../crates/sim-model/tests/determinism_oracle_tests.rs), [M0
graduation
gate](../../crates/sim-model/tests/m0_determinism_gate_tests.rs),
[snapshot
tests](../../crates/sim-model/tests/phase11_snapshot_tests.rs), and the
[M1 manifest](../../contracts/m1_contract.toml).

| claim_id | line | value | class | status | notes |
|----|----|----|----|----|----|
| 32,295 | 6 | 521 | derived | historical-log | The recorded M1 validation aggregate is a historical test-run log, not a regeneration target. |
| 32,296 | 6 | 521 | derived | historical-log | The recorded M1 validation aggregate is a historical test-run log, not a regeneration target. |
| 32,297 | 6 | 500 | derived | historical-log | The recorded M1 validation aggregate is a historical test-run log, not a regeneration target. |
| 32,299 | 37 | 0 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,300 | 38 | 7 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,301 | 39 | 1 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,302 | 40 | 15 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,303 | 41 | 8 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,304 | 42 | 12 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,307 | 43 | 10 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,308 | 44 | 12 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,309 | 45 | 13 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,310 | 46 | 12 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,311 | 47 | 11 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,313 | 48 | 27 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,315 | 49 | 36 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,316 | 50 | 37 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,317 | 51 | 30 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,318 | 52 | 41 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,319 | 53 | 50 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,320 | 54 | 47 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,321 | 55 | 66 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,322 | 55 | 256 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,323 | 56 | 50 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,324 | 56 | 1 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,325 | 56 | 11 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,326 | 57 | 36 | deterministic | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,327 | 57 | 500 | deterministic | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,328 | 58 | 521 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |

### Top evidence gaps

| milestone | report | status | claims |
|----|----|----|----|
| M3 | M3_01_SERIAL_BASELINE.md | missing-data | 384 |
| M3 | M3_02_PARALLEL_PHASE6B.md | missing-data | 340 |
| M3 | M3_11_PROPAGATION.md | missing-data | 304 |
| M2 | M2_31_POST_PREINDEX_HOT_PATH_PROFILE.md | missing-data | 275 |
| M2 | M2_X_REBASELINE_REPORT.md | missing-data | 243 |
| M2 | M2_06_BASELINE_REPORT.md | missing-data | 231 |
| M3 | M3_04_SUSTAINED_WORKLOAD.md | missing-data | 217 |
| M3 | M3_06_SETTLEMENT_INDEX.md | missing-data | 113 |
| M2 | M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | missing-data | 112 |
| M2 | M2_34_POST_ALGORITHM_REPROFILING.md | missing-data | 111 |

### Recovered original-output matches

The recovered-output generator
[recovered_claims.py](../../analysis/simciv_analysis/recovered_claims.py)
scanned **206** output logs and data files. It records **27,845** exact
report-scoped numeric or SHA-256 token matches; **24,236** claims with a
prior `missing-*` status moved to `recovered-original-output`. The
comparison excludes labeled input/configuration columns. A token match
is report-scoped evidence, not row-level semantic identification.

#### Claims moved from missing status

| Report | Tier | Prior status | Claims |
|----|----|----|----|
| M2_34_POST_ALGORITHM_REPROFILING.md | headline | missing-data | 6 |
| M2_34_POST_ALGORITHM_REPROFILING.md | key | missing-data | 2 |
| M2_34_POST_ALGORITHM_REPROFILING.md | supporting | missing-data | 213 |
| M2_37_POST_P5_P6B_REPROFILING.md | headline | missing-data | 17 |
| M2_37_POST_P5_P6B_REPROFILING.md | supporting | missing-data | 579 |
| M2_38_DETERMINISTIC_RAYON_PHASE4_GATE.md | supporting | missing-data | 517 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | missing-data | 39 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | supporting | missing-data | 1,351 |
| M3_01_SERIAL_BASELINE.md | headline | missing-source | 3 |
| M3_01_SERIAL_BASELINE.md | supporting | missing-data | 488 |
| M3_01_SERIAL_BASELINE.md | supporting | missing-source | 6 |
| M3_02_PARALLEL_PHASE6B.md | headline | missing-data | 3 |
| M3_02_PARALLEL_PHASE6B.md | key | missing-data | 6 |
| M3_02_PARALLEL_PHASE6B.md | supporting | missing-data | 10,561 |
| M3_02_PARALLEL_PHASE6B.md | supporting | missing-source | 24 |
| M3_03_SEARCH_REDUCTION.md | headline | missing-data | 9 |
| M3_03_SEARCH_REDUCTION.md | key | missing-data | 3 |
| M3_03_SEARCH_REDUCTION.md | supporting | missing-data | 470 |
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | missing-data | 486 |
| M3_04_SUSTAINED_WORKLOAD.md | headline | missing-data | 46 |
| M3_04_SUSTAINED_WORKLOAD.md | supporting | missing-data | 1,959 |
| M3_05_SCALE_DIAGNOSIS.md | supporting | missing-data | 230 |
| M3_05_SCALE_LIMIT.md | headline | missing-data | 181 |
| M3_05_SCALE_LIMIT.md | headline | missing-source | 1 |
| M3_05_SCALE_LIMIT.md | supporting | missing-data | 991 |
| M3_05_SCALE_LIMIT.md | supporting | missing-source | 64 |
| M3_06_SETTLEMENT_INDEX.md | supporting | missing-data | 295 |
| M3_07_SCALE_RESIDUAL.md | key | missing-data | 7 |
| M3_07_SCALE_RESIDUAL.md | supporting | missing-data | 458 |
| M3_07_SCALE_RESIDUAL.md | supporting | missing-source | 17 |
| M3_08_POPULATION_DYNAMICS.md | headline | missing-source | 25 |
| M3_08_POPULATION_DYNAMICS.md | supporting | missing-source | 290 |
| M3_09_INCREMENTAL_TOPOLOGY.md | headline | missing-source | 24 |
| M3_09_INCREMENTAL_TOPOLOGY.md | supporting | missing-source | 212 |
| M3_11_PROPAGATION.md | headline | missing-data | 13 |
| M3_11_PROPAGATION.md | supporting | missing-data | 4,584 |
| M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | missing-source | 4 |
| M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | supporting | missing-source | 5 |
| M4_01_SYSTEM_REGISTRY.md | key | missing-source | 47 |

#### Claims without an exact token match

These claims retain their previous status. A report-level output
association without a value match is not enough evidence to call the
result contradictory.

| Report | Tier | Unmatched claims |
|----|----|----|
| M2_34_POST_ALGORITHM_REPROFILING.md | headline | 2 |
| M2_34_POST_ALGORITHM_REPROFILING.md | supporting | 111 |
| M2_37_POST_P5_P6B_REPROFILING.md | headline | 1 |
| M2_37_POST_P5_P6B_REPROFILING.md | supporting | 66 |
| M2_38_DETERMINISTIC_RAYON_PHASE4_GATE.md | supporting | 49 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | 2 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | supporting | 110 |
| M3_01_SERIAL_BASELINE.md | headline | 10 |
| M3_01_SERIAL_BASELINE.md | supporting | 384 |
| M3_02_PARALLEL_PHASE6B.md | headline | 5 |
| M3_02_PARALLEL_PHASE6B.md | supporting | 340 |
| M3_03_SEARCH_REDUCTION.md | key | 4 |
| M3_03_SEARCH_REDUCTION.md | supporting | 10 |
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | 53 |
| M3_04_SUSTAINED_WORKLOAD.md | headline | 10 |
| M3_04_SUSTAINED_WORKLOAD.md | supporting | 207 |
| M3_05_SCALE_DIAGNOSIS.md | supporting | 2 |
| M3_05_SCALE_LIMIT.md | headline | 38 |
| M3_05_SCALE_LIMIT.md | supporting | 50 |
| M3_06_SETTLEMENT_INDEX.md | supporting | 115 |
| M3_07_SCALE_RESIDUAL.md | key | 5 |
| M3_07_SCALE_RESIDUAL.md | supporting | 57 |
| M3_08_POPULATION_DYNAMICS.md | headline | 1 |
| M3_08_POPULATION_DYNAMICS.md | supporting | 61 |
| M3_09_INCREMENTAL_TOPOLOGY.md | headline | 6 |
| M3_09_INCREMENTAL_TOPOLOGY.md | supporting | 96 |
| M3_11_PROPAGATION.md | headline | 4 |
| M3_11_PROPAGATION.md | supporting | 302 |
| M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | 4 |
| M4_01_SYSTEM_REGISTRY.md | key | 43 |
| M4_02_BRANCHING.md | headline | 13 |
| M4_02_BRANCHING.md | key | 353 |
| M4_02_BRANCHING.md | supporting | 55 |
| M4_03_DIVERGENCE.md | headline | 8 |
| M4_03_DIVERGENCE.md | key | 36 |
| M4_03_DIVERGENCE.md | supporting | 63 |
| M4_05_MINIMAL_CAUSE.md | headline | 2 |
| M4_05_MINIMAL_CAUSE.md | key | 10 |
| M4_05_MINIMAL_CAUSE.md | supporting | 14 |
| M4_07_MANY_BRANCH.md | headline | 1 |
| M4_07_MANY_BRANCH.md | supporting | 46 |

#### Confirmed output mismatches

| Confirmed output mismatches |
|-----------------------------|
| none                        |

Confirmed mismatches: **0**. No mismatch is inferred solely because a
value was absent from a report-scoped output.

## Findings

- \[exploratory\] The previously unrecoverable M1 aggregate rows carry
  the `historical-log` status.
- \[exploratory\] Historical-log and historical-protocol rows are
  excluded from regeneration gap totals.
- \[exploratory\] M2 analysis-only rows do not appear in the
  evidence-gap table.
- \[exploratory\] Top evidence-gap rows are ordered by claim count
  descending.

## Limits

- A `reproducible-now` status means a source and data mapping is
  present; this inventory did not run the harness.
- Values excluded as input/configuration or outside the selected claim
  contexts are not assigned new provenance.
- This report compares source evidence only and does not remeasure
  timings or run simulations.

## Appendix: paired findings checks

| Finding tag     | Paired predicate                              | Value  |
|-----------------|-----------------------------------------------|--------|
| \[exploratory\] | `finding_m1_rows_are_historical_log`          | `true` |
| \[exploratory\] | `finding_historical_rows_excluded_from_gaps`  | `true` |
| \[exploratory\] | `finding_m2_analysis_only_excluded_from_gaps` | `true` |
| \[exploratory\] | `finding_top_gaps_sorted_by_count`            | `true` |
