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
[`sources.toml`](sources.toml). `claim_key` (report path, section
anchor, and in-section ordinal) is the stable identity; `claim_id`
remains the compatibility key for existing status maps. Source lines are
absent from the ledger; use `just claims-locate <claim_key>` for current
line lookup. Its tidy output [claims.csv.gz](claims.csv.gz) contains
**32,465** rows; the prior packed ledger
[superseded/claims.csv](superseded/claims.csv) contains **37,612**
expanded scalar values. M5-04 statuses are applied only to source-map
entries assigned to that stage. Excluded inputs are table columns or
labeled assignments for population/workload sizes, group and neighbor
counts, seeds, replicates, horizons/days, thread/chunk settings, and
simulation parameter values. Free-text numbers are only extracted from
tables, report Summary/Findings/Status sections, selected
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
| M2 | derived | analysis-only | supporting | 150 |
| M2 | derived | missing-data | headline | 92 |
| M2 | derived | missing-data | key | 5 |
| M2 | derived | missing-data | supporting | 686 |
| M2 | derived | unverifiable | headline | 36 |
| M2 | derived | unverifiable | key | 2 |
| M2 | derived | unverifiable | supporting | 2,499 |
| M2 | deterministic | analysis-only | headline | 7 |
| M2 | deterministic | analysis-only | supporting | 29 |
| M2 | deterministic | missing-source | headline | 2 |
| M2 | deterministic | missing-source | key | 5 |
| M2 | deterministic | missing-source | supporting | 57 |
| M2 | deterministic | reproducible-now | headline | 4 |
| M2 | deterministic | reproducible-now | supporting | 18 |
| M2 | deterministic | unverifiable | headline | 22 |
| M2 | deterministic | unverifiable | key | 3 |
| M2 | deterministic | unverifiable | supporting | 74 |
| M2 | timing | analysis-only | headline | 14 |
| M2 | timing | analysis-only | key | 9 |
| M2 | timing | analysis-only | supporting | 187 |
| M2 | timing | missing-data | headline | 18 |
| M2 | timing | missing-data | key | 1 |
| M2 | timing | missing-data | supporting | 397 |
| M2 | timing | unverifiable | headline | 26 |
| M2 | timing | unverifiable | supporting | 307 |
| M3 | derived | inconclusive (validity) | supporting | 26 |
| M3 | derived | missing-data | headline | 8 |
| M3 | derived | missing-data | key | 4 |
| M3 | derived | missing-data | supporting | 2,104 |
| M3 | derived | missing-source | headline | 7 |
| M3 | derived | missing-source | supporting | 170 |
| M3 | derived | not reproduced | headline | 10 |
| M3 | derived | recovered-original-output | headline | 106 |
| M3 | derived | recovered-original-output | supporting | 2,088 |
| M3 | derived | recovered-original-output (formatted) | supporting | 754 |
| M3 | derived | recovered-weak (composite field) | supporting | 8 |
| M3 | derived | reproduced | headline | 82 |
| M3 | derived | reproduced (loose) | headline | 7 |
| M3 | derived | unverifiable | headline | 86 |
| M3 | derived | unverifiable | key | 13 |
| M3 | derived | unverifiable | supporting | 8,414 |
| M3 | deterministic | missing-source | supporting | 14 |
| M3 | deterministic | reproducible-now | supporting | 1 |
| M3 | deterministic | unverifiable | headline | 6 |
| M3 | deterministic | unverifiable | supporting | 124 |
| M3 | timing | conclusion holds | headline | 2 |
| M3 | timing | inconclusive | headline | 4 |
| M3 | timing | missing-data | headline | 4 |
| M3 | timing | missing-data | key | 2 |
| M3 | timing | missing-data | supporting | 430 |
| M3 | timing | missing-source | headline | 9 |
| M3 | timing | missing-source | supporting | 17 |
| M3 | timing | recovered-original-output | supporting | 345 |
| M3 | timing | recovered-original-output (formatted) | supporting | 65 |
| M3 | timing | reproduced | headline | 1 |
| M3 | timing | unverifiable | headline | 80 |
| M3 | timing | unverifiable | key | 6 |
| M3 | timing | unverifiable | supporting | 8,299 |
| M4 | derived | bit-exact | headline | 19 |
| M4 | derived | bit-exact | supporting | 35 |
| M4 | derived | bit-exact after documented normalization | headline | 9 |
| M4 | derived | bit-exact after documented normalization | key | 1 |
| M4 | derived | bit-exact after documented normalization | supporting | 11 |
| M4 | derived | conclusion holds | key | 41 |
| M4 | derived | conclusion holds | supporting | 122 |
| M4 | derived | historical-protocol | key | 22 |
| M4 | derived | historical-protocol | supporting | 13 |
| M4 | derived | missing-source | key | 5 |
| M4 | derived | missing-source | supporting | 44 |
| M4 | derived | unrecoverable | supporting | 3 |
| M4 | derived | unverifiable | headline | 46 |
| M4 | derived | unverifiable | key | 288 |
| M4 | derived | unverifiable | supporting | 3,038 |
| M4 | deterministic | historical-protocol | key | 3 |
| M4 | deterministic | historical-protocol | supporting | 4 |
| M4 | deterministic | missing-source | supporting | 3 |
| M4 | deterministic | unverifiable | key | 21 |
| M4 | deterministic | unverifiable | supporting | 21 |
| M4 | timing | conclusion holds | key | 44 |
| M4 | timing | conclusion holds | supporting | 43 |
| M4 | timing | historical-protocol | key | 66 |
| M4 | timing | missing-source | key | 46 |
| M4 | timing | missing-source | supporting | 5 |
| M4 | timing | reproducible-now | supporting | 20 |
| M4 | timing | unverifiable | headline | 9 |
| M4 | timing | unverifiable | key | 290 |
| M4 | timing | unverifiable | supporting | 110 |
| M5 | derived | missing-source | key | 6 |
| M5 | derived | missing-source | supporting | 21 |

### Old and tidy ledger comparison

| Ledger view | rows | mapped before (exact report/source/value) | matched after (informational) | unmatched old rows | reason | matched to old ledger (informational) | unmatched old-ledger comparison |
|----|----|----|----|----|----|----|----|
| old packed records | 37,612 | 3 | 1,988 | 35,624 | RENAMES.md report mapping; normalized source text and value |  |  |
| new tidy records | 32,465 |  |  |  | sources.toml and regeneration results supply provenance; old matches are diagnostics only | 3,695 | 28,770 |

#### Match and unmapped reasons

| old record disposition | records | rule |
|----|----|----|
| old rows matched (informational) | 1,988 | RENAMES.md plus normalized source/value; does not assign new provenance |
| old rows unmatched | 35,624 | includes excluded inputs, changed/unselected text, and ambiguous old matches |
| new rows ambiguous | 731 | current claims with conflicting metadata or tied candidates; these are new-row counts |
| historical-log subset of transferred rows | 29 | M1 aggregate runs are not regeneration gaps |
| previous exact-report/source/value baseline | 3 | old rows with exact normalized source text and value; line-independent diagnostic baseline |
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

| claim_id | claim_key | section | value | class | status | notes |
|----|----|----|----|----|----|----|
| 32,298 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:1 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 521 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,307 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:10 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 10 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,308 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:11 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 12 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,309 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:12 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 13 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,310 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:13 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 12 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,311 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:14 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 11 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,313 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:16 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 27 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,315 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:18 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 36 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,316 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:19 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 37 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,299 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:2 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 0 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,317 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:20 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 30 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,318 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:21 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 41 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,319 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:22 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 50 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,320 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:23 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 47 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,321 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:24 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 66 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,322 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:25 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 256 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,323 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:26 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 50 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,324 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:27 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 1 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,325 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:28 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 11 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,326 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:29 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 36 | deterministic | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,300 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:3 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 7 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,327 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:30 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 500 | deterministic | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,301 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:4 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 1 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,302 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:5 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 15 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,303 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:6 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 8 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,304 | docs/contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary:7 | [2-m0-empirical-validation-evidence-summary](../contracts/M1_CONTRACT_FREEZE.md#2-m0-empirical-validation-evidence-summary) | 12 | derived | historical-log | M1 aggregate test-run counts are preserved historical logs, not regeneration targets. |
| 32,295 | docs/contracts/M1_CONTRACT_FREEZE.md#m1-contract-freeze-specification-contracts-c01-c10:1 | [m1-contract-freeze-specification-contracts-c01-c10](../contracts/M1_CONTRACT_FREEZE.md#m1-contract-freeze-specification-contracts-c01-c10) | 521 | derived | historical-log | The recorded M1 validation aggregate is a historical test-run log, not a regeneration target. |
| 32,296 | docs/contracts/M1_CONTRACT_FREEZE.md#m1-contract-freeze-specification-contracts-c01-c10:2 | [m1-contract-freeze-specification-contracts-c01-c10](../contracts/M1_CONTRACT_FREEZE.md#m1-contract-freeze-specification-contracts-c01-c10) | 521 | derived | historical-log | The recorded M1 validation aggregate is a historical test-run log, not a regeneration target. |
| 32,297 | docs/contracts/M1_CONTRACT_FREEZE.md#m1-contract-freeze-specification-contracts-c01-c10:3 | [m1-contract-freeze-specification-contracts-c01-c10](../contracts/M1_CONTRACT_FREEZE.md#m1-contract-freeze-specification-contracts-c01-c10) | 500 | derived | historical-log | The recorded M1 validation aggregate is a historical test-run log, not a regeneration target. |

### Top evidence gaps

| milestone | report | status | claims |
|----|----|----|----|
| M3 | M3_02_PARALLEL_PHASE6B.md | unverifiable | 10,346 |
| M3 | M3_04_SUSTAINED_WORKLOAD.md | unverifiable | 1,865 |
| M4 | M4_03_DIVERGENCE.md | unverifiable | 1,759 |
| M2 | M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | unverifiable | 1,514 |
| M3 | M3_11_PROPAGATION.md | missing-data | 1,117 |
| M4 | M4_04_CRN_EFFICIENCY.md | unverifiable | 1,067 |
| M3 | M3_05_SCALE_LIMIT.md | unverifiable | 1,035 |
| M3 | M3_11_PROPAGATION.md | unverifiable | 718 |
| M4 | M4_02_BRANCHING.md | unverifiable | 691 |
| M3 | M3_01_SERIAL_BASELINE.md | unverifiable | 616 |

### Recovered original-output matches

The recovered-output matcher
[recovered_claims.py](../../analysis/simciv_analysis/recovered_claims.py)
scanned **206** files. A match requires a structured CSV/TSV field, JSON
key, or explicit `key=value` record with the same measurement field and
all recorded configuration dimensions. Composite values also require the
same component position; **8** same-field values at a different or
unknown position remain `recovered-weak`. It records **3,358** claims as
recovered evidence across exact, formatted, unit-converted, other-run,
and derived statuses; **23,902** value-only claims without structured
context remain `unverifiable`; **1,837** parameter values were excluded
from measurement matching.

The prior 144af62 rule matched value tokens without row, label, column,
or unit context. Among **24,236** claims it had moved, the recorded type
split was:

| Value type       | Claims |
|------------------|--------|
| config/parameter | 1,793  |
| measured value   | 20,253 |
| other            | 510    |
| small integer    | 1,680  |

A fixed-seed sample (20261024; 50 headline/key and 100 other measured
claims) found matching context for **37** of 150 value-only matches
(**24.7%**). The full review is in
[recovered_claim_precision_audit.csv](recovered_claim_precision_audit.csv).

#### Claims moved from missing status

| Report | Tier | Prior status | Resolution | Claims |
|----|----|----|----|----|
| M3_01_SERIAL_BASELINE.md | supporting | missing-data | recovered-original-output | 25 |
| M3_01_SERIAL_BASELINE.md | supporting | missing-data | recovered-original-output (formatted) | 65 |
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | missing-data | recovered-original-output | 8 |
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | missing-data | recovered-original-output (formatted) | 88 |
| M3_05_SCALE_LIMIT.md | headline | missing-data | recovered-original-output | 106 |
| M3_11_PROPAGATION.md | supporting | missing-data | recovered-original-output | 2,400 |
| M3_11_PROPAGATION.md | supporting | missing-data | recovered-original-output (formatted) | 666 |

#### Weak composite matches

These **8** values occur in the row and field but lack a same-position
match, so they remain evidence gaps:

| Report                        | Tier       | Claims |
|-------------------------------|------------|--------|
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | 8      |

#### Claims without an exact context match

These claims retain their previous status. A value found only in a
different row, configuration, or measure column—or only in unstructured
output—does not qualify as recovered evidence.

| Report | Tier | Unmatched claims |
|----|----|----|
| M2_34_POST_ALGORITHM_REPROFILING.md | headline | 2 |
| M2_34_POST_ALGORITHM_REPROFILING.md | supporting | 104 |
| M2_37_POST_P5_P6B_REPROFILING.md | headline | 1 |
| M2_37_POST_P5_P6B_REPROFILING.md | supporting | 48 |
| M2_38_DETERMINISTIC_RAYON_PHASE4_GATE.md | supporting | 37 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | 2 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | supporting | 73 |
| M3_01_SERIAL_BASELINE.md | headline | 4 |
| M3_01_SERIAL_BASELINE.md | supporting | 34 |
| M3_02_PARALLEL_PHASE6B.md | supporting | 131 |
| M3_03_SEARCH_REDUCTION.md | key | 4 |
| M3_03_SEARCH_REDUCTION.md | supporting | 9 |
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | 40 |
| M3_04_SUSTAINED_WORKLOAD.md | headline | 4 |
| M3_04_SUSTAINED_WORKLOAD.md | supporting | 134 |
| M3_05_SCALE_DIAGNOSIS.md | supporting | 1 |
| M3_05_SCALE_LIMIT.md | headline | 15 |
| M3_05_SCALE_LIMIT.md | supporting | 50 |
| M3_06_SETTLEMENT_INDEX.md | supporting | 113 |
| M3_07_SCALE_RESIDUAL.md | key | 1 |
| M3_07_SCALE_RESIDUAL.md | supporting | 5 |
| M3_08_POPULATION_DYNAMICS.md | headline | 1 |
| M3_08_POPULATION_DYNAMICS.md | supporting | 37 |
| M3_09_INCREMENTAL_TOPOLOGY.md | headline | 5 |
| M3_09_INCREMENTAL_TOPOLOGY.md | supporting | 88 |
| M3_11_PROPAGATION.md | headline | 2 |
| M3_11_PROPAGATION.md | supporting | 289 |
| M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | 3 |
| M4_01_SYSTEM_REGISTRY.md | key | 42 |
| M4_02_BRANCHING.md | key | 66 |
| M4_02_BRANCHING.md | supporting | 17 |
| M4_03_DIVERGENCE.md | headline | 6 |
| M4_03_DIVERGENCE.md | key | 31 |
| M4_03_DIVERGENCE.md | supporting | 9 |
| M4_05_MINIMAL_CAUSE.md | headline | 2 |
| M4_05_MINIMAL_CAUSE.md | key | 10 |
| M4_05_MINIMAL_CAUSE.md | supporting | 14 |
| M4_07_MANY_BRANCH.md | headline | 1 |
| M4_07_MANY_BRANCH.md | supporting | 46 |

#### Unverifiable value-only references

**23,902** claims have a numeric value in a related recovered output but
no structured record with matching measurement context. They receive
`unverifiable`; their values are not treated as recovered matches.

| Report | Tier | Unverifiable claims |
|----|----|----|
| M2_34_POST_ALGORITHM_REPROFILING.md | headline | 12 |
| M2_34_POST_ALGORITHM_REPROFILING.md | key | 2 |
| M2_34_POST_ALGORITHM_REPROFILING.md | supporting | 220 |
| M2_37_POST_P5_P6B_REPROFILING.md | headline | 17 |
| M2_37_POST_P5_P6B_REPROFILING.md | supporting | 597 |
| M2_38_DETERMINISTIC_RAYON_PHASE4_GATE.md | supporting | 532 |
| M2_39_RAYON_PRODUCTION_POLICY_GATE.md | supporting | 75 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | 55 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | key | 3 |
| M2_40_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | supporting | 1,456 |
| M3_01_SERIAL_BASELINE.md | headline | 12 |
| M3_01_SERIAL_BASELINE.md | supporting | 604 |
| M3_02_PARALLEL_PHASE6B.md | headline | 23 |
| M3_02_PARALLEL_PHASE6B.md | key | 6 |
| M3_02_PARALLEL_PHASE6B.md | supporting | 10,317 |
| M3_03_SEARCH_REDUCTION.md | headline | 9 |
| M3_03_SEARCH_REDUCTION.md | key | 2 |
| M3_03_SEARCH_REDUCTION.md | supporting | 425 |
| M3_04_EQUILIBRIUM_WORKLOAD.md | supporting | 394 |
| M3_04_SUSTAINED_WORKLOAD.md | headline | 52 |
| M3_04_SUSTAINED_WORKLOAD.md | supporting | 1,813 |
| M3_05_SCALE_DIAGNOSIS.md | supporting | 231 |
| M3_05_SCALE_LIMIT.md | headline | 43 |
| M3_05_SCALE_LIMIT.md | supporting | 1,055 |
| M3_06_SETTLEMENT_INDEX.md | supporting | 301 |
| M3_07_SCALE_RESIDUAL.md | key | 11 |
| M3_07_SCALE_RESIDUAL.md | supporting | 515 |
| M3_08_POPULATION_DYNAMICS.md | headline | 25 |
| M3_08_POPULATION_DYNAMICS.md | supporting | 294 |
| M3_09_INCREMENTAL_TOPOLOGY.md | headline | 25 |
| M3_09_INCREMENTAL_TOPOLOGY.md | supporting | 206 |
| M3_11_PROPAGATION.md | headline | 15 |
| M3_11_PROPAGATION.md | supporting | 703 |
| M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | headline | 5 |
| M3_AUDIT_FINAL_RUNTIME_PROFILE_AND_GRADUATION_GATE.md | supporting | 5 |
| M4_01_SYSTEM_REGISTRY.md | key | 48 |
| M4_02_BRANCHING.md | headline | 55 |
| M4_02_BRANCHING.md | key | 515 |
| M4_02_BRANCHING.md | supporting | 121 |
| M4_03_DIVERGENCE.md | headline | 12 |
| M4_03_DIVERGENCE.md | key | 35 |
| M4_03_DIVERGENCE.md | supporting | 1,724 |
| M4_04_CRN_EFFICIENCY.md | supporting | 1,067 |
| M4_05_MINIMAL_CAUSE.md | headline | 7 |
| M4_05_MINIMAL_CAUSE.md | key | 1 |
| M4_05_MINIMAL_CAUSE.md | supporting | 253 |
| M4_07_MANY_BRANCH.md | supporting | 4 |

#### Recovered-output mismatch triage

The baseline mismatch file recorded at commit `fddd9a9` contained
**1,087** claims and **4,371** output records. After correcting
graph/projection metric-field assignment, **0** claims (**0** records)
matched the intended output field exactly; these are a matcher
correction, not data discrepancies. Tier counts for that correction:
headline **0**, key **0**, supporting **0**.

The remaining **1,087** claims (**4,371** records) are classified in
precedence order: printed formatting/rounding, unit conversion, a
matching other run or aggregate, a derived value, then a genuine
discrepancy. Statuses for the first four are distinct; only the last
retains `recovered-output-mismatch`. The generated cause table uses
**claims** and **baseline output records**, split by
headline/key/supporting tier:

| Cause | claims | headline | key | Baseline records | supporting |
|----|----|----|----|----|----|
| rounding/formatting | 819 | 0 | 0 | 4,099 | 819 |
| composite field position mismatch | 0 | 0 | 0 | 0 | 0 |
| unit conversion | 0 | 0 | 0 | 0 | 0 |
| different run | 0 | 0 | 0 | 0 | 0 |
| derived | 0 | 0 | 0 | 0 | 0 |
| genuine discrepancy | 0 | 0 | 0 | 0 | 0 |
| unverifiable | 268 | 0 | 0 | 272 | 268 |

The detailed audit separates the earlier field-mapping correction from
the cause categories, including position-matched composite components,
weak composite matches, and printed-precision formatting:

| Cause and classification detail | claims | Baseline records |
|----|----|----|
| rounding/formatting: composite field component at matching position | 754 | 4,032 |
| rounding/formatting: printed-precision rounding | 65 | 67 |
| unverifiable: value found without a structured matching record | 268 | 272 |

Classification details are listed in
[recovered_claim_mismatch_triage.csv.gz](recovered_claim_mismatch_triage.csv.gz),
with its generated summary in
[recovered_claim_mismatch_triage.json](recovered_claim_mismatch_triage.json).

M3_11 accounts for **666** baseline mismatch claims and **3,944**
records. Recovered `frontiers` arrays and `levels_max` pairs are now
checked by component position: only matching positions count as
formatted evidence; sibling values in the same field are listed as weak
composite matches:

| M3_11 mismatch field | Candidate records |
|----------------------|-------------------|
| frontiers            | 3,776             |
| levels_max           | 168               |

#### Genuine discrepancies retained

Current contextual matching leaves **0** genuine mismatch claims. The
table lists all headline/key cases; **0** supporting cases remain. An
empty table means no headline/key discrepancies were found.

| Genuine discrepancies |
|-----------------------|
| none                  |

The contextual-difference evidence file is
[recovered_claim_mismatches.csv.gz](recovered_claim_mismatches.csv.gz);
it contains **4,097** candidate records before triage. These candidates
are not current mismatch statuses unless they remain after the ordered
checks above.

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
