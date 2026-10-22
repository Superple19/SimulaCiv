<a id="m3-02b-search-and-bookkeeping-reduction"></a>
# M3-03 (formerly M3-02b) Search and Bookkeeping Reduction
Status: current

M3-02b complete

- **Retained candidate:** Phase 4 global eligibility precheck, behind the explicit serial opt-in runner
- **Rejected candidates:** per-day bitsets, Phase 4 slot sidecar reuse, radix sort
- **Out of scope:** output-vector de-duplication, because it requires a public result-type change

## Executive summary

The retained precheck preserves exact Phase 4 intents and State, Metrics, and Events output. In the final Phase 4 profile, the `N=10,000`, M2-40 day-0 cells measured paired intent-stage savings of `6.0 ± 0.6 µs` at `p_local=0` and `10.1 ± 3.6 µs` at `p_local=1`. Other cells were noisier or slower. The sustained day-150 workload usually has eligible targets, so the precheck does not skip the neighbor scan there.

The paired whole-tick measurements did not show a positive saving larger than the paired MAD in any of the 12 requested cells. One cell, M2-40 at `N=100,000, p_local=0.5`, measured a paired slowdown larger than its MAD. These results support a narrow candidate-search optimization, not a general whole-tick speedup claim.

## Scope and correctness

The benchmark compares `run_m3_storage_days` with the explicit `run_m3_storage_days_with_search_reduction` runner. Each of the 12 cells uses the same checkpoint for both runners. Every timed sample asserts equality of final M3 State, cumulative Metrics and Events hashes, and the exact per-day Metrics and Events records. The focused Phase 4 profile also asserts identical intent vectors for every measured sample.

No existing test, fixture, golden, snapshot, canonical expected value, production source, or CI file changed for this benchmark and report. The unchanged M2 and M3 goldens and snapshots passed in `cargo test --workspace`.

## Host and method

- **CPU:** AMD Ryzen 5 9600X, 6 physical / 12 logical processors
- **OS:** Windows 11 Pro, build 26200
- **Rust:** `rustc 1.98.1 (48a229cea 2026-09-01)`
- **Workloads:** M2-40 at day 0; the M3 sustained configuration (`base_metabolic_cost=0.48`) at day 150
- **Cells:** `N ∈ {10,000, 100,000}`, `K=N/200`, `k=8`, `p_local ∈ {0, 0.5, 1}`
- **Seed / replicate:** `81985529216486895` / `7`
- **Whole-tick method:** 2 warmups, 11 timed paired samples, interleaved serial and search-reduction runs, three consecutive days per sample, reusable contexts and scratch, metrics and events enabled, snapshots disabled
- **Timer boundary:** world clone, graph, projection, and runner context were prepared before timing; the timer covers the runner call
- **Stage method:** 2 warmups and 11 timed samples on the same phase inputs. Phase 4 and Phase 6B use their direct stage paths; Phase 6B timing uses the opt-in resolver timing hook. The private Phase 4 eligibility helper is measured in a benchmark-local inclusion of the source module, while whole-tick timing calls the compiled `sim-model` library.
- **Command:** `cargo bench -p sim-model --bench m3_02b_search_reduction_bench`
- **Bench-reported wall time:** `90.160 s` (Cargo compilation is outside that measurement)
- **Per-day JSONL:** `target/m3_02b_search_reduction.jsonl` (generated, not committed)

The full-tick primary measurement uses the compiled library. The Phase 4 and Phase 6B stage profiles are secondary; the Phase 4 probe is compiled inside the benchmark to access the crate-private helper, so its code generation may differ from the library-linked whole-tick path. Phase 1–3, Phase 5, and Phase 6A values in JSON are one direct observation per checkpoint; Phase 7–11 are `null`. The existing [M3 sustained workload profile](M3_SUSTAINED_WORKLOAD.md#serial-per-phase-profile) contains the broader unchanged phase profile.

## Paired whole-tick results

All values are µs/tick, median ± MAD. `Paired saving` is the median of paired `(serial − search-reduction)` differences ± MAD; positive values favor the opt-in runner.

| Profile | N | p_local | Serial | Search reduction | Paired saving |
|---|---:|---:|---:|---:|---:|
| M2-40 day 0 | 10,000 | 0 | 2,334.067 ± 37.767 | 2,447.900 ± 193.900 | -36.933 ± 146.067 |
| M2-40 day 0 | 10,000 | 0.5 | 2,190.533 ± 157.533 | 2,187.267 ± 183.233 | 28.967 ± 181.400 |
| M2-40 day 0 | 10,000 | 1 | 2,590.400 ± 66.200 | 2,580.367 ± 107.067 | 13.733 ± 111.633 |
| M2-40 day 0 | 100,000 | 0 | 32,076.500 ± 372.100 | 32,854.467 ± 425.600 | -468.333 ± 788.900 |
| M2-40 day 0 | 100,000 | 0.5 | 32,049.700 ± 454.433 | 32,223.167 ± 526.600 | -622.233 ± 448.767 |
| M2-40 day 0 | 100,000 | 1 | 31,307.467 ± 805.567 | 31,769.067 ± 522.333 | 331.400 ± 947.300 |
| Sustained day 150 | 10,000 | 0 | 2,706.067 ± 79.800 | 2,819.933 ± 133.700 | -32.067 ± 159.733 |
| Sustained day 150 | 10,000 | 0.5 | 3,330.067 ± 42.033 | 3,182.800 ± 184.700 | 98.200 ± 199.033 |
| Sustained day 150 | 10,000 | 1 | 3,236.433 ± 101.300 | 3,217.067 ± 242.433 | 57.433 ± 204.367 |
| Sustained day 150 | 100,000 | 0 | 40,991.100 ± 742.100 | 40,927.433 ± 560.733 | -69.033 ± 1,083.500 |
| Sustained day 150 | 100,000 | 0.5 | 40,259.033 ± 418.133 | 40,391.700 ± 441.733 | -123.000 ± 889.633 |
| Sustained day 150 | 100,000 | 1 | 38,725.500 ± 1,343.500 | 38,404.733 ± 1,732.800 | 857.100 ± 2,019.233 |

No positive whole-tick paired saving exceeds its MAD. The `N=100,000`, M2-40 `p_local=0.5` row is the only paired median slowdown whose magnitude exceeds the paired MAD. This is evidence of variance and a possible workload-specific regression; it is not evidence for an across-the-board whole-tick improvement.

## Phase 4 target-search profile

Values are µs per Phase 4 intent generation, median ± MAD. The paired saving is baseline minus precheck path. A positive precheck value is the measured global eligibility scan cost.

| Profile | N | p_local | Reference intents | Precheck intents | Paired saving | Precheck |
|---|---:|---:|---:|---:|---:|---:|
| M2-40 day 0 | 10,000 | 0 | 147.2 ± 0.7 | 141.2 ± 0.8 | 6.0 ± 0.6 | 7.4 ± 0.0 |
| M2-40 day 0 | 10,000 | 0.5 | 209.1 ± 12.6 | 188.5 ± 2.9 | 8.9 ± 14.2 | 10.3 ± 0.3 |
| M2-40 day 0 | 10,000 | 1 | 155.6 ± 11.0 | 138.6 ± 2.4 | 10.1 ± 3.6 | 7.5 ± 0.1 |
| M2-40 day 0 | 100,000 | 0 | 2,372.7 ± 155.4 | 2,364.7 ± 371.1 | 85.4 ± 140.7 | 100.0 ± 1.9 |
| M2-40 day 0 | 100,000 | 0.5 | 2,287.9 ± 75.4 | 2,134.0 ± 114.2 | -149.0 ± 154.2 | 102.7 ± 2.3 |
| M2-40 day 0 | 100,000 | 1 | 2,486.6 ± 123.9 | 2,252.1 ± 83.3 | 238.4 ± 249.5 | 100.8 ± 1.0 |
| Sustained day 150 | 10,000 | 0 | 209.5 ± 7.1 | 211.0 ± 4.9 | 3.5 ± 9.4 | 0.0 ± 0.0 |
| Sustained day 150 | 10,000 | 0.5 | 263.4 ± 12.0 | 256.9 ± 9.3 | 8.7 ± 7.1 | 0.0 ± 0.0 |
| Sustained day 150 | 10,000 | 1 | 262.2 ± 22.1 | 271.6 ± 16.9 | -3.1 ± 9.2 | 0.0 ± 0.0 |
| Sustained day 150 | 100,000 | 0 | 3,414.8 ± 85.0 | 3,454.1 ± 114.6 | 48.6 ± 238.7 | 0.1 ± 0.0 |
| Sustained day 150 | 100,000 | 0.5 | 3,381.8 ± 153.7 | 3,463.6 ± 174.1 | 27.2 ± 68.5 | 0.1 ± 0.0 |
| Sustained day 150 | 100,000 | 1 | 3,527.4 ± 47.1 | 3,452.7 ± 92.0 | 23.1 ± 73.5 | 0.1 ± 0.0 |

The M2-40 day-0 `N=10,000` `p_local=0` and `p_local=1` stage savings exceed MAD. At day 150, the global target predicates are present in the sustained workload, so the precheck does not suppress neighbor traversal; the measured stage differences remain within MAD. The precheck behavior is therefore workload-conditional.

## Phase 6B stage profile

The resolver is unchanged between the two runners. These are the serial timing-hook medians ± MAD, in µs; they provide stage context for the same checkpoints.

| Profile | N | p_local | Validation / slot | Key generation | Sort | Commit / output |
|---|---:|---:|---:|---:|---:|---:|
| M2-40 day 0 | 10,000 | 0 | 65.3 ± 2.8 | 25.6 ± 0.3 | 55.0 ± 4.7 | 161.4 ± 4.9 |
| M2-40 day 0 | 10,000 | 0.5 | 83.9 ± 16.8 | 30.9 ± 3.0 | 59.9 ± 4.6 | 172.4 ± 9.2 |
| M2-40 day 0 | 10,000 | 1 | 62.3 ± 1.1 | 25.9 ± 0.4 | 44.2 ± 0.7 | 147.5 ± 2.6 |
| M2-40 day 0 | 100,000 | 0 | 2,297.8 ± 95.6 | 591.8 ± 6.8 | 1,082.1 ± 59.5 | 2,179.9 ± 230.3 |
| M2-40 day 0 | 100,000 | 0.5 | 2,397.1 ± 61.8 | 597.1 ± 10.4 | 1,082.0 ± 32.6 | 2,158.3 ± 179.7 |
| M2-40 day 0 | 100,000 | 1 | 2,570.5 ± 281.6 | 610.1 ± 11.9 | 1,145.9 ± 111.2 | 2,171.5 ± 200.0 |
| Sustained day 150 | 10,000 | 0 | 133.5 ± 2.0 | 34.3 ± 0.7 | 130.2 ± 3.6 | 295.5 ± 1.2 |
| Sustained day 150 | 10,000 | 0.5 | 118.2 ± 1.6 | 39.6 ± 0.1 | 112.8 ± 2.2 | 287.1 ± 8.4 |
| Sustained day 150 | 10,000 | 1 | 127.8 ± 4.0 | 40.3 ± 0.2 | 118.8 ± 4.9 | 294.4 ± 7.6 |
| Sustained day 150 | 100,000 | 0 | 2,652.6 ± 126.0 | 562.1 ± 7.1 | 2,195.6 ± 93.5 | 3,560.9 ± 154.5 |
| Sustained day 150 | 100,000 | 0.5 | 2,640.1 ± 152.8 | 429.4 ± 81.8 | 2,119.2 ± 63.8 | 3,545.6 ± 294.7 |
| Sustained day 150 | 100,000 | 1 | 2,518.3 ± 217.4 | 372.9 ± 25.3 | 2,045.8 ± 111.3 | 3,608.5 ± 282.2 |

## Checkpoint workload

Counts come from the corresponding checkpoint profile. GiveFood and StealFood cells are `Some / None`; `Applied+` counts positive Applied results. The full JSONL also records all action and Phase 6B outcome counts.

| Profile | N | p_local | Alive | Keyed | Cross-partition | GiveFood Some / None | StealFood Some / None | Applied+ | TheftFailed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| M2-40 day 0 | 10,000 | 0 | 10,000 | 2,446 | 2,446 | 0 / 2,189 | 2,446 / 0 | 1,230 | 1,216 |
| M2-40 day 0 | 10,000 | 0.5 | 10,000 | 2,446 | 1,193 | 0 / 2,189 | 2,446 / 0 | 1,230 | 1,216 |
| M2-40 day 0 | 10,000 | 1 | 10,000 | 2,446 | 0 | 0 / 2,189 | 2,446 / 0 | 1,230 | 1,216 |
| M2-40 day 0 | 100,000 | 0 | 100,000 | 24,405 | 24,405 | 0 / 22,057 | 24,405 / 0 | 12,317 | 12,088 |
| M2-40 day 0 | 100,000 | 0.5 | 100,000 | 24,405 | 12,218 | 0 / 22,057 | 24,405 / 0 | 12,317 | 12,088 |
| M2-40 day 0 | 100,000 | 1 | 100,000 | 24,405 | 0 | 0 / 22,057 | 24,405 / 0 | 12,317 | 12,088 |
| Sustained day 150 | 10,000 | 0 | 10,000 | 4,678 | 4,678 | 2,164 / 0 | 2,514 / 1 | 2,393 | 1,138 |
| Sustained day 150 | 10,000 | 0.5 | 10,000 | 4,678 | 2,328 | 2,164 / 0 | 2,514 / 1 | 2,410 | 1,133 |
| Sustained day 150 | 10,000 | 1 | 10,000 | 4,676 | 0 | 2,164 / 0 | 2,512 / 3 | 2,421 | 1,123 |
| Sustained day 150 | 100,000 | 0 | 100,000 | 46,773 | 46,773 | 22,250 / 0 | 24,523 / 12 | 23,545 | 11,244 |
| Sustained day 150 | 100,000 | 0.5 | 99,998 | 46,770 | 23,475 | 22,250 / 0 | 24,520 / 15 | 23,609 | 11,271 |
| Sustained day 150 | 100,000 | 1 | 99,997 | 46,762 | 0 | 22,250 / 0 | 24,512 / 23 | 23,481 | 11,231 |

At M2-40 day 0, GiveFood had no eligible targets, which makes the global GiveFood precheck useful. At sustained day 150, GiveFood found 2,164 targets at `N=10,000` and 22,250 at `N=100,000`; the optimization correctly leaves those neighbor searches enabled. This is an inference from the recorded counts and the frozen predicate.

## Candidate decisions

| Candidate | Result | Measurement / correctness basis |
|---|---|---|
| a. Global eligibility search skip | **Kept** | Exact intent and digest parity. Phase 4 paired stage improvement exceeded MAD in some M2-40 day-0 cells, including `N=10,000, p_local=0` and `p_local=1`; gains were not consistent at larger N or in the sustained workload. Whole-tick gains did not exceed MAD in the final matrix. |
| b. Per-day eligibility bitsets | **Rejected** | On the `N=10,000`, M2-40 day-0 probe, candidate-a Phase 4 was `135.90 µs`; with bitsets it was `137.90 µs`, paired change `-1.30 ± 1.30 µs`. It did not beat MAD. |
| c. Phase 4 slot sidecar reuse | **Rejected** | An isolated resolver profile saved `74.2 ± 31.7 µs`, but including sidecar preparation from Phase 4 through Phase 6B regressed by `18.4 ± 84.9 µs`; the combined candidate did not pass the stage gate. |
| d. Stable radix sort | **Rejected** | At 24,584 keyed entries, comparison sort measured `771.50 µs`, radix sort `915.40 µs`, paired saving `-163.90 ± 36.10 µs`. |
| e. Output-vector de-duplication | **Out of scope** | Complete de-duplication requires a public result-type change, excluded by frozen §11.2. |

## Validation and limitations

- `cargo fmt --check` — passed
- `cargo clippy --workspace --all-targets -- -D warnings` — passed
- `cargo test --workspace` — passed, including existing M2 goldens/snapshots and M3 differential tests
- `cargo test -p sim-model --test m3_search_reduction_tests -- --nocapture` — passed; target wall time `73.732 s` (test target reported `73.50 s`), within the frozen 2-minute debug target
- `cargo bench -p sim-model --bench m3_02b_search_reduction_bench` — passed; 12 cells measured, bench-reported 90.160 s
- JSONL under `target/` parsed as 12 records; it is not committed
- No post-push CI time was recorded because this task did not push

The measurements cover one Windows host and one checkpoint day per configuration, with three-day whole-tick samples. Stage instrumentation is secondary to the uninstrumented whole-tick runner. The Phase 4 private-helper probe is compiled as a local source module; the primary whole-tick comparison uses the linked library. Results do not establish a universal speedup or a cross-host performance claim.
