# M2-38 Deterministic Rayon Phase4 Gate

- **Baseline:** `master` at `55754b45f292a75bb0a81846c50137d0e8db2b4e`
- **Decision entering this gate:** `RAYON-FIRST`
- **Implemented scope:** experimental Rayon Phase4 Selection and Phase4 Indexed Intent
- **Default execution:** existing serial production runner
- **Acceptance:** **PASS-A — PRODUCTION-CANDIDATE**; production integration remains a separate gate

## 1. Result

The experimental path is bit-exact against the serial path for selection, indexed intent generation, runner outputs, state, metrics, events, and snapshots. The full canonical hashes also match the frozen values at 1, 6, and 12 threads.

On the measured AMD Ryzen 5 9600X (6 physical cores, 12 logical processors), multi-thread Rayon showed full-tick improvement from N=5,000 in both workload families. At N=10,000 through 50,000, every tested 2/4/6/12-thread and chunk-size combination improved the paired full-tick median. One-thread Rayon remained slower than serial through N=50,000. The best observed full-tick improvements were 1.15–1.24× from N=5,000 to 50,000.

`PASS-A` means this backend is a production candidate for a later integration and crossover-policy gate. `run_hybrid_authority_days` remains serial in this change.

## 2. Scope and dependency

Rayon 1.12.0 is a direct dependency of `sim-model`, matching the repository's direct-dependency convention. Pools are caller-owned. The experimental runner receives a prebuilt `ThreadPool` and nonzero chunk size; it does not create a pool per day or per sample.

The new APIs are:

- `phase4_primary_action_selection_storage_into_rayon`
- `generate_intents_storage_with_candidate_index_rayon`
- `run_hybrid_authority_days_with_rayon_phase4`

The serial selection and indexed-intent APIs remain unchanged as callable oracles. Existing runner entry points explicitly dispatch to serial Phase4. The experimental runner requires Hybrid Storage Authority and reuses transient Phase4 candidate-index scratch across its days.

## 3. Independence and scalar semantics

Selection reads one agent's feature and storage fields, config, and coordinate-based random value, then writes one choice. It has no cross-agent read/write dependency. Intent generation reads one choice and the immutable, serially built group/action candidate index, then writes one intent. There are no reservations, shared RNG state, or writes to other agents.

The existing per-agent scalar calculation remains in its original operation order: utility accumulation, trait adjustment, `stable_softmax`, coordinate construction, RNG draw, and CDF traversal. Intent action formulas, candidate predicates, candidate order, self-exclusion, `floor(u*C)`, requested amounts, and target RNG coordinates/draw index are unchanged. No SIMD, reassociation, FMA rewrite, or alternative math implementation was added.

The candidate index is built serially once per tick. Workers share it immutably. Candidate buckets remain AgentId ascending, including when physical storage is reversed.

## 4. Error precedence and partial output

Selection returns the first error in feature input order. The Rayon path tags each fixed chunk by its original sequence position, completes each chunk only through its first local error, then merges chunks in increasing chunk index. This returns the same earliest feature error and preserves the serial partial `out` contents on failure.

Indexed Intent keeps the serial error protocol before parallel construction:

1. Build the candidate index, clear output, and perform the existing sorted-choice duplicate precheck.
2. Visit choices in input order, preserving slot lookup, eligibility, and then unsorted-input duplicate validation precedence. Record validated slots in a transient vector.
3. If validation fails, generate only the already-valid prefix in parallel and merge it in input order before returning the same error.
4. After all intents are generated, run the existing missing-choice coverage validation in the same storage order. Only a successful result receives the canonical AgentId sort.

The current indexed storage API has no group-mismatch or action-specific validation errors; no new validation cases were introduced. Candidate index, validated-slot vector, chunk-local intent vectors, and the caller's output vector are transient runtime data. None enters `WorldState`, `SegmentedAgentStorage`, snapshots, hashes, or events.

## 5. Deterministic chunking and merge

Both parallel kernels divide canonical contiguous feature/choice sequences with fixed `par_chunks(C)`. Each result carries a deterministic chunk index. Chunk-local outputs are sorted/merged by that index; entries within each chunk keep source order. Successful APIs then apply the same stable AgentId output ordering as their serial counterparts. Rayon completion order and worker identity do not determine semantic order.

No task is created per agent. Intent workers have separate candidate-source wrappers and chunk-local output vectors; candidate contents are shared read-only.

## 6. Differential and trajectory validation

`phase4_rayon_tests.rs` covers:

- Selection input sizes 0, 1, below/equal/above chunk boundaries, and multiple chunks; all six actions; randomized features; reversed physical storage.
- Indexed Intent with no targeted action, GiveFood-only and StealFood-only inputs, mixed actions, zero/one/multiple candidates, self included/excluded from candidate buckets, sparse groups, and reversed storage.
- Selection errors at input indices 17/70/1300 and intent errors at 64/1300, spanning separate chunks at sizes 64/256/1024; duplicate-precheck precedence, missing-choice coverage errors, and exact partial-output parity.
- A seeded pseudo-random Intent dataset with shuffled physical storage, sparse groups, varied food quantities, and mixed actions.
- Direct Selection → Indexed Intent differential runs.
- Three-day and 40-day runner parity across all 5 thread counts × all 5 chunk sizes.
- Snapshot-byte parity at resume days 100, 250, and 500 across all thread/chunk combinations.
- The 500-day canonical trajectory at 1, 6, and 12 threads.

All differential results match the serial oracle exactly.

Frozen canonical values matched:

| Canonical output | Expected and observed |
|---|---|
| State | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` |
| Metrics | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` |
| Events | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` |

Day 100/250/500 snapshot bytes were equal to serial. No snapshot, event, hash, fixture, or golden schema/value was changed.

## 7. Benchmark method

The final benchmark run used the AMD Ryzen 5 9600X host (6 physical cores / 12 logical processors). It tested thread counts `1, 2, 4, 6, 12`, chunk sizes `64, 128, 256, 512, 1024`, populations `1k, 5k, 10k, 20k, 50k`, and:

- **A:** fixed K=2 settlements.
- **B:** approximately 200 agents per settlement, K=`ceil(N/200)`.

That is 250 workload/population/thread/chunk cells. The cell order used a fixed 73-step permutation. Within each cell the serial/Rayon order alternated deterministically. Each cell had two warm-ups and seven timed samples; the report uses median and median absolute deviation (MAD). Inputs, initial state, and thread pools were built outside their timers. Component timings measured Selection, indexed Intent including serial index build, and combined P4. Full-tick samples ran three days through the serial production runner or the experimental Rayon runner, with metrics and events enabled and snapshots disabled; scratch was reused across the three days. Full-tick results are shown as µs/tick.

The tables below select the lowest Rayon median independently for each component. A cell's paired serial measurement is shown beside it; the best thread/chunk may differ by component. These host-specific minima are observations, not a universal tuning policy.

## 8. Best observed component and full-tick results

Each cell is `serial → Rayon µs ± MAD (speedup; Rayon threads/chunk)`. For full tick, the table also reports throughput for the selected Rayon cell.

| Workload | N / K | Selection | Indexed Intent | Combined P4 | Full tick; ticks/s |
|---|---:|---|---|---|---|
| A | 1k / 2 | 29.70±0.30 → 25.60±0.10 (1.16×; 4/128) | 26.80±0.00 → 30.80±0.20 (0.87×; 4/128) | 55.20±0.10 → 56.00±0.20 (0.99×; 4/64) | 143.50±0.73 → 152.23±0.73 (0.94×; 4/256); 6,569 |
| A | 5k / 2 | 159.60±0.90 → 67.90±10.40 (2.35×; 6/128) | 138.20±0.70 → 93.40±0.50 (1.48×; 6/128) | 293.20±0.50 → 152.20±1.80 (1.93×; 6/128) | 814.60±2.17 → 705.57±13.10 (1.15×; 6/64); 1,417 |
| A | 10k / 2 | 318.00±0.80 → 104.30±6.90 (3.05×; 12/512) | 278.00±0.70 → 165.80±1.10 (1.68×; 6/256) | 594.10±1.40 → 264.60±5.70 (2.25×; 12/128) | 1,644.77±4.50 → 1,330.23±13.83 (1.24×; 12/128); 752 |
| A | 20k / 2 | 635.50±6.40 → 214.20±8.80 (2.97×; 12/64) | 674.80±112.90 → 310.00±7.10 (2.18×; 12/128) | 1,194.90±3.50 → 473.90±9.70 (2.52×; 12/64) | 3,270.87±3.53 → 2,737.10±61.10 (1.20×; 6/512); 365 |
| A | 50k / 2 | 1,569.20±6.60 → 524.60±30.00 (2.99×; 12/1024) | 1,528.10±88.20 → 813.70±23.40 (1.88×; 12/512) | 3,353.60±309.20 → 1,160.60±117.00 (2.89×; 12/512) | 8,785.70±43.37 → 7,319.00±79.27 (1.20×; 6/128); 137 |
| B | 1k / 5 | 30.50±0.30 → 24.80±0.20 (1.23×; 4/64) | 27.60±0.10 → 31.20±0.40 (0.89×; 4/64) | 55.90±0.40 → 55.50±1.00 (1.01×; 4/64) | 153.37±2.47 → 159.93±2.07 (0.96×; 4/64); 6,253 |
| B | 5k / 25 | 159.20±1.00 → 62.00±1.10 (2.57×; 6/1024) | 136.90±2.60 → 96.90±1.00 (1.41×; 6/1024) | 290.00±1.00 → 160.80±5.20 (1.80×; 6/1024) | 873.33±8.70 → 760.27±3.53 (1.15×; 6/256); 1,315 |
| B | 10k / 50 | 318.80±1.30 → 92.90±4.00 (3.43×; 12/64) | 282.50±13.20 → 180.40±3.20 (1.57×; 12/1024) | 589.50±3.00 → 275.20±0.90 (2.14×; 6/128) | 1,759.57±3.73 → 1,481.23±23.37 (1.19×; 6/128); 675 |
| B | 20k / 100 | 628.00±1.10 → 210.80±8.70 (2.98×; 12/64) | 554.60±10.40 → 329.00±5.40 (1.69×; 12/512) | 1,176.60±2.10 → 490.70±8.10 (2.40×; 12/128) | 3,609.93±12.20 → 3,067.40±57.30 (1.18×; 12/256); 326 |
| B | 50k / 250 | 1,603.60±23.20 → 577.40±27.60 (2.78×; 12/128) | 1,498.30±92.30 → 861.00±54.80 (1.74×; 12/1024) | 3,008.70±36.80 → 1,259.00±17.50 (2.39×; 12/64) | 9,408.33±23.13 → 7,935.67±8.67 (1.19×; 6/1024); 126 |

## 9. Full-tick thread/chunk matrix

Each cell is paired serial median divided by Rayon median; values above 1.0 are faster with Rayon. Chunk columns are 64/128/256/512/1024.

### Workload A, N=10,000

| Threads \ Chunk | 64 | 128 | 256 | 512 | 1024 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0.914 | 0.910 | 0.923 | 0.890 | 0.912 |
| 2 | 1.075 | 1.069 | 1.084 | 1.072 | 1.099 |
| 4 | 1.250 | 1.203 | 1.159 | 1.263 | 1.254 |
| 6 | 1.183 | 1.169 | 1.224 | 1.207 | 1.083 |
| 12 | 1.235 | 1.236 | 1.228 | 1.167 | 1.210 |

### Workload A, N=50,000

| Threads \ Chunk | 64 | 128 | 256 | 512 | 1024 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0.894 | 0.923 | 0.918 | 0.909 | 0.917 |
| 2 | 1.025 | 1.049 | 1.026 | 1.068 | 1.047 |
| 4 | 1.091 | 1.162 | 1.087 | 1.096 | 1.149 |
| 6 | 1.187 | 1.200 | 1.213 | 1.213 | 1.192 |
| 12 | 1.169 | 1.172 | 1.199 | 1.197 | 1.180 |

### Workload B, N=10,000

| Threads \ Chunk | 64 | 128 | 256 | 512 | 1024 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0.923 | 0.910 | 0.925 | 0.923 | 0.920 |
| 2 | 1.048 | 1.059 | 1.072 | 1.054 | 1.056 |
| 4 | 1.130 | 1.059 | 1.150 | 1.086 | 1.190 |
| 6 | 1.184 | 1.188 | 1.171 | 1.180 | 1.144 |
| 12 | 1.161 | 1.241 | 1.177 | 1.118 | 1.180 |

### Workload B, N=50,000

| Threads \ Chunk | 64 | 128 | 256 | 512 | 1024 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0.896 | 0.909 | 0.897 | 0.919 | 0.911 |
| 2 | 1.043 | 1.022 | 1.066 | 1.051 | 1.031 |
| 4 | 1.099 | 1.078 | 1.132 | 1.116 | 1.084 |
| 6 | 1.138 | 1.186 | 1.186 | 1.191 | 1.186 |
| 12 | 1.178 | 1.189 | 1.188 | 1.172 | 1.183 |

## 10. One-thread overhead and crossover

One-thread Rayon was slower across the measured full-tick grid. By N=5k, the best observed one-thread chunk still added about 7–11% to full-tick time; at N=10k–50k the observed one-thread full-tick overhead was about 7–9%. The best one-thread combined-P4 cell added about 16–23% at N=5k–50k. Selection had the largest scheduler/chunk overhead; the best measured one-thread Selection cells were about 28–31% slower at N=5k–10k and about 82–83% slower at N=50k.

The four one-thread deltas were measured separately for Selection, Indexed Intent, combined P4, and full tick. The table reports the best one-thread chunk for each component independently. These deltas aggregate pool dispatch, chunk-local output allocation, validation/index work, and ordered merge as applicable; those subcosts were not individually timed.

| Workload | N | Selection overhead | Intent overhead | Combined P4 overhead | Full-tick overhead |
|---|---:|---:|---:|---:|---:|
| A | 1k | +46.96% @64 | +34.96% @64 | +42.59% @64 | +22.72% @128 |
| A | 5k | +30.24% @256 | +12.78% @256 | +18.66% @1024 | +8.91% @256 |
| A | 10k | +28.45% @1024 | +8.01% @512 | +18.74% @1024 | +8.31% @256 |
| A | 20k | +44.52% @64 | +5.75% @256 | +19.25% @512 | +7.32% @512 |
| A | 50k | +83.30% @1024 | −18.75% @1024* | +23.05% @1024 | +8.33% @128 |
| B | 1k | +45.12% @256 | +36.70% @256 | +42.75% @256 | +22.24% @256 |
| B | 5k | +28.61% @512 | +13.20% @1024 | +18.78% @512 | +10.52% @512 |
| B | 10k | +30.74% @128 | +7.13% @1024 | +16.48% @256 | +8.29% @512 |
| B | 20k | +45.75% @512 | +4.34% @512 | +19.13% @1024 | +7.26% @1024 |
| B | 50k | +81.71% @1024 | +4.27% @256 | +22.01% @512 | +8.83% @512 |

`@C` is the chunk size. `*` is one unstable A/50k Intent cell: its serial median had MAD 254.30 µs, versus 25.70 µs for one-thread Rayon. It is not treated as evidence of a one-thread Intent speedup.

Outside that noisy cell, the one-thread Intent probe was slower by about 4–13% at N≥5k. The anomaly does not change the combined-P4 or full-tick result.

For every tested multi-thread setting (2/4/6/12 threads and all five chunks), both workloads first showed combined-P4 and full-tick median improvement at N=5,000. No one-thread setting showed both improvements through N=50,000. This places the observed crossover between 1k and 5k on this host; it does not establish a portable threshold.

## 11. Best configurations and SMT

The lowest full-tick medians by population used:

| Workload | 1k | 5k | 10k | 20k | 50k |
|---|---|---|---|---|---|
| A | 4 threads / 256 | 6 / 64 | 12 / 128 | 6 / 512 | 6 / 128 |
| B | 4 threads / 64 | 6 / 256 | 6 / 128 | 12 / 256 | 6 / 1024 |

These independently selected minima show no single best thread/chunk pair. Six threads often matched or beat twelve for full ticks at N=20k–50k. Twelve threads reduced some P4 component medians, but its whole-tick benefit was inconsistent; there is no general SMT gain on this host.

At N=50k, the best 6-thread vs 12-thread full-tick medians were 7,319 vs 7,828 µs/tick for A and 7,936 vs 7,947 µs/tick for B. At N=10k they were 1,332 vs 1,330 for A and 1,481 vs 1,508 for B. The differences are small enough that chunk-specific results should guide a later policy rather than a hard-coded 12-thread default.

## 12. Amdahl: model and measured result

M2-37's 33–37% candidate share and 1.29–1.41× six-thread scenario covered **P2 + P3 + P4 Selection + P4 Indexed Intent + P9**. That is a model for a broader candidate set, not a prediction that this gate's P4-only implementation would achieve the same result. M2-37's estimates did not subtract scheduling, merge, validation, or memory costs.

This gate measured P4-only share and speedup using the paired serial P4/full-tick medians. The measured P4-only Amdahl value uses `1 / ((1-p) + p/s)`, where `p` is that cell's serial combined-P4/full-tick ratio and `s` is its measured combined-P4 speedup.

For this table, each row uses the same thread/chunk cell selected by the lowest full-tick median. Its P4 share and P4 speedup are measured in that paired cell; they are not the independently best P4 cell from section 8.

| Workload / N | Measured P4 share | Combined P4 speedup | P4-only Amdahl model | Measured full-tick speedup | Gap |
|---|---:|---:|---:|---:|---:|
| A / 10k | 36.1% | 2.245× | 1.251× | 1.236× | −0.014× |
| A / 20k | 36.4% | 2.367× | 1.266× | 1.195× | −0.071× |
| A / 50k | 34.1% | 2.147× | 1.223× | 1.200× | −0.023× |
| B / 10k | 33.5% | 2.142× | 1.217× | 1.188× | −0.030× |
| B / 20k | 32.6% | 2.373× | 1.232× | 1.177× | −0.055× |
| B / 50k | 31.7% | 2.057× | 1.194× | 1.186× | −0.009× |

Measured full-tick gains were below the P4-only Amdahl model in all listed cases. Scheduling, chunk-local allocation/merge, serial candidate-index construction and validation, memory/cache effects, and other phase timing are plausible contributors; this run did not isolate those costs, so these remain explanations to test rather than measured causes.

## 13. Remaining bottlenecks and limits

- The candidate index build and error/coverage validation remain serial.
- Each chunk allocates a local output vector; allocation counts were not instrumented.
- P2, P3, P9, P5, P6B, P7, P8, event staging, and Phase10 remain serial in this path.
- Best thread/chunk settings vary by workload, population, component, and observed sample. The measurements justify an optional experimental backend and a later runtime-policy gate, not a universal default.
- No 100k case, allocator profile, cache counter, or bandwidth counter was collected in this gate.

## 14. Decision and next adoption step

**PASS-A — PRODUCTION-CANDIDATE.** Correctness, determinism, canonical hashes, snapshots, and error precedence passed. At N≥5k, the measured multi-thread paths delivered meaningful P4 and full-tick speedups in both workloads. The next gate may evaluate production integration and a runtime crossover policy using representative deployment hardware. Keep the current serial default until that gate chooses an integration policy.

## 15. Validation and repository scope

The following validation commands passed:

- `cargo test --workspace`
- `cargo test -p sim-model --test phase4_intent_tests -- --nocapture`
- `cargo test -p sim-model --test phase4_decision_tests -- --nocapture`
- `cargo test -p sim-model --test phase4_rayon_tests -- --nocapture`
- `cargo test -p sim-model --test determinism_oracle_tests -- --nocapture`
- `cargo test -p sim-model --test m2_soa_gate_tests -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench --bench m0_baseline_bench`
- `git diff --check`

Only Rayon dependency/lockfile changes, Phase4 source and experimental runner APIs, Phase4 Rayon tests, the benchmark gate, and this report are in scope. No Phase5+ source, RNG implementation, fixture, golden, canonical expected hash, snapshot/event schema, or CI file was changed. No commit was created.
