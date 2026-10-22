# M3-07 (formerly residual-scaling diagnosis) Scale Residual
Status: current

complete; measurement-only

- **Question:** do the post-M3-06 superlinear intervals reproduce, and do the remaining costs look like noise, memory hierarchy, access pattern, or algorithmic work?
- **Scope:** serial M3 storage path, equilibrium profile, `k=8`, `p_local=0.5`, day-0 starting state, `K=N/200`

## Measured facts

### Host, noise control, and method

- **Host:** AMD Ryzen 5 9600X, 6 physical cores / 12 logical processors; Windows 11 Pro; `rustc 1.98.1`; `x86_64-pc-windows-msvc`; 95.61 GiB RAM.
- **Load before / after:** other-process CPU averaged 1.67% / 1.22% over 5-second windows. Available RAM was 79,950,389,248 / 79,880,933,376 bytes. The busiest processes were Task Manager and short-lived Claude processes; no other process exceeded 0.6% machine CPU in either sample.
- **Controls:** the bench process was verified at Windows `High` priority and affinity mask `0x555`, selecting one logical processor from each of the six physical cores. Windows reported physical-core masks `0x3, 0xC, 0x30, 0xC0, 0x300, 0xC00`; the selected siblings were logical CPUs 0, 2, 4, 6, 8, and 10.
- The affinity constrains the process to those six physical cores but does not pin its serial thread to one specific core; Windows may migrate it among that set.
- **Cache hierarchy:** Windows WMI reported 480 KiB aggregate primary cache, 6,144 KiB aggregate secondary cache, and 32,768 KiB tertiary cache. CPUID identified these as 48 KiB L1d and 1 MiB L2 per core, with 32 MiB L3 shared by the single CCD.
- **4 KiB TLB reach:** CPUID reported 96 L1 DTLB entries (384 KiB). Its raw L2 DTLB field was 128 entries, but the Zen 5 guide documents 4,096 L2 DTLB entries for 4 KiB pages (16 MiB); the raw field is under-scaled for Zen 5. [AMD Software Optimization Guide for Zen 5](https://docs.amd.com/v/u/en-US/58455_1.00).
- **Configuration:** the scale bench's M2-40 equilibrium configuration, `N ∈ {25k, 50k, 100k, 150k, 200k, 300k, 400k, 500k, 750k, 1M}`, `K=N/200`, `k=8`, `p_local=0.5`, seed `81985529216486895`, replicate `7`.
- **Sampling:** two warmups and 15 timed samples per cell; each sample ran three consecutive days with scratch reuse. The reported profile medians and MADs use 45 timed day observations. Every sample restarts from the same day-0 checkpoint; initialization and graph/projection setup are outside phase timers.
- **Profile validity:** the bench-only `ManualProfile::step` was compared on every day to `run_m3_storage_days`: `M3DayOutcome` and canonical State hash matched. The reversed-slot checks separately compared State, Metrics, and Events digests.
- **Command:** `cargo bench -p sim-model --bench m3_scale_limit_bench -- --residual`.
- **Final run wall time:** 534.7 s (8 min 54.7 s), including Cargo startup. A preliminary run used the same sampling and controls before the report was changed to exclude unreliable alpha values; its 548.7 s results are not used below. Total bench process time across both runs was 1,083.4 s.

### Re-measured scaling alphas

`alpha = log(T₂/T₁) / log(N₂/N₁)`. The 250k anchor, 500k cell, and 1M cell all use this run's 15-sample method. An interval is reliable only when **both** endpoint MAD/median values are at most 5%; unreliable intervals are excluded rather than reported as alpha estimates.

| Phase | 250k→500k alpha (MAD/median at 250k / 500k) | 500k→1M alpha (MAD/median at 500k / 1M) |
|---|---:|---:|
| P1 regrowth | excluded (8.333% / 6.250%) | excluded (6.250% / 8.000%) |
| P2 degradation | 1.02841 (4.025% / 3.830%) | 0.96509 (3.830% / 2.759%) |
| P3 features | excluded (8.295% / 11.941%) | excluded (11.941% / 6.600%) |
| P4 action selection | excluded (1.828% / 8.643%) | excluded (8.643% / 1.889%) |
| P4 intent/projection | excluded (4.838% / 6.815%) | excluded (6.815% / 3.986%) |
| P5 partition | excluded (2.823% / 5.584%) | excluded (5.584% / 4.018%) |
| P6A work resolution | excluded (4.666% / 9.877%) | excluded (9.877% / 3.725%) |
| P6B targeted resolution | excluded (9.519% / 11.607%) | excluded (11.607% / 8.959%) |
| P7 market | excluded (7.561% / 8.510%) | excluded (8.510% / 4.818%) |
| P8 welfare | excluded (2.880% / 7.892%) | excluded (7.892% / 6.027%) |
| P9 mortality | 1.10825 (3.987% / 4.661%) | 1.06432 (4.661% / 4.134%) |
| P10 metrics | excluded (17.509% / 25.953%) | excluded (25.953% / 19.086%) |
| P11 events | excluded (2.489% / 8.574%) | excluded (8.574% / 3.859%) |

The earlier post-M3-06 `500k→1M` alphas above 2 were P4 intent/projection 2.77362, P5 2.12812, P6A 2.14141, P6B 2.37580, P7 2.36486, P8 2.87924, P9 2.79205, P10 2.34764, and P11 2.09474. None has a reliable alpha in this rerun: their 1M or 500k endpoint MAD exceeded 5%. The reliable intervals are P2 and P9, both near 1; thus the earlier `>2` values did **not** reproduce as reliable results, while the noisy phases remain inconclusive.

### Fine-N phase profile

Cells show median **ns per alive agent** and MAD/median percentage; `†` means MAD/median exceeds 5% and the cell is unreliable. P1 is divided by `N` for the requested per-agent scale; it actually visits `K` settlements. P6B's per-keyed values are shown separately below.

| Phase | N=25k | N=50k | N=100k | N=150k | N=200k | N=300k | N=400k | N=500k | N=750k | N=1M |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| P1 regrowth | 0.0080 (0.000%) | 0.0080 (25.000%)† | 0.0060 (16.667%)† | 0.0060 (11.111%)† | 0.0055 (9.091%)† | 0.0043 (7.692%)† | 0.0037 (6.667%)† | 0.0032 (6.250%)† | 0.0028 (4.762%) | 0.0025 (8.000%)† |
| P2 degradation | 0.8200 (0.976%) | 0.8300 (1.205%) | 0.8550 (1.988%) | 0.8480 (2.123%) | 0.8415 (1.604%) | 0.8440 (2.765%) | 0.8430 (2.046%) | 0.8616 (3.830%) | 0.8401 (1.206%) | 0.8410 (2.759%) |
| P3 features | 3.3600 (3.095%) | 3.6060 (7.876%)† | 3.6670 (10.008%)† | 3.5433 (6.679%)† | 3.4575 (4.845%) | 3.6483 (10.178%)† | 3.5217 (7.063%)† | 3.7600 (11.941%)† | 3.4945 (2.385%) | 3.6896 (6.600%)† |
| P4 action selection | 42.8920 (0.326%) | 43.2580 (0.902%) | 43.3710 (0.461%) | 43.5320 (0.652%) | 43.4965 (0.531%) | 43.9670 (1.862%) | 43.8765 (1.159%) | 47.6954 (8.643%)† | 45.2548 (4.026%) | 44.0869 (1.889%) |
| P4 intent/projection | 18.4520 (3.425%) | 20.1880 (5.003%)† | 26.9630 (8.215%)† | 30.3507 (5.810%)† | 32.0195 (4.605%) | 34.1360 (5.605%)† | 35.2885 (5.398%)† | 36.8890 (6.815%)† | 38.1045 (6.019%)† | 38.3117 (3.986%) |
| P5 partition | 13.0720 (13.953%)† | 13.5580 (15.814%)† | 12.2660 (4.101%) | 12.0527 (3.800%) | 12.0420 (2.873%) | 12.3777 (4.670%) | 12.9205 (3.984%) | 13.3804 (5.584%)† | 13.2743 (2.274%) | 14.0285 (4.018%) |
| P6A work resolution | 8.7640 (4.336%) | 10.2900 (7.658%)† | 10.4400 (1.849%) | 11.9200 (3.853%) | 11.3435 (3.954%) | 14.8657 (5.868%)† | 16.8700 (4.421%) | 25.0778 (9.877%)† | 23.5155 (3.192%) | 28.0152 (3.725%) |
| P6B targeted resolution | 44.2200 (18.544%)† | 45.6480 (13.810%)† | 50.1570 (15.739%)† | 51.8527 (11.758%)† | 55.7140 (6.786%)† | 55.1017 (5.955%)† | 58.8615 (9.791%)† | 73.2104 (11.607%)† | 66.9389 (9.648%)† | 75.7287 (8.959%)† |
| P7 market | 20.3880 (5.101%)† | 20.8680 (9.431%)† | 25.7000 (11.370%)† | 32.1667 (5.627%)† | 36.2640 (4.372%) | 45.0677 (3.960%) | 45.6235 (4.603%) | 58.1062 (8.510%)† | 55.5768 (6.206%)† | 65.6766 (4.818%) |
| P8 welfare | 11.9080 (2.150%) | 11.3240 (7.859%)† | 12.3970 (3.356%) | 13.2833 (2.479%) | 12.7970 (2.626%) | 15.0547 (6.800%)† | 14.1770 (2.941%) | 17.0710 (7.892%)† | 15.8213 (3.797%) | 17.8363 (6.027%)† |
| P9 mortality | 8.1120 (0.444%) | 8.3820 (1.098%) | 8.5970 (1.186%) | 11.8893 (3.140%) | 11.4280 (3.141%) | 12.6697 (3.423%) | 12.7545 (5.081%)† | 14.3178 (4.661%) | 13.4296 (3.804%) | 14.9706 (4.134%) |
| P10 metrics | 25.6880 (5.092%)† | 26.4280 (25.079%)† | 28.5020 (31.973%)† | 30.8140 (28.675%)† | 30.3610 (10.920%)† | 31.9730 (24.273%)† | 32.1912 (18.493%)† | 34.3732 (25.953%)† | 33.7933 (24.528%)† | 35.5912 (19.086%)† |
| P11 events | 21.9960 (1.109%) | 22.5900 (2.337%) | 23.8260 (2.132%) | 24.6113 (2.248%) | 24.8260 (1.766%) | 25.4013 (1.483%) | 26.1905 (1.999%) | 29.1630 (8.574%)† | 28.4841 (1.786%) | 29.8469 (3.859%) |

P6B total normalized by mean keyed entries/day:

| N | ns per keyed entry | MAD/median |
|---:|---:|---:|
| 25k | 179.1444 | 18.544%† |
| 50k | 186.2322 | 13.810%† |
| 100k | 204.0202 | 15.739%† |
| 150k | 211.0787 | 11.758%† |
| 200k | 227.0195 | 6.786%† |
| 300k | 224.6775 | 5.955%† |
| 400k | 240.1367 | 9.791%† |
| 500k | 298.5612 | 11.607%† |
| 750k | 272.9196 | 9.648%† |
| 1M | 308.7653 | 8.959%† |

Mean alive count was the full initialized population in every measured cell. Mean keyed entries/day were `6,171` at 25k, `12,256` at 50k, `24,584` at 100k, `36,848` at 150k, `49,083` at 200k, `73,574` at 300k, `98,047` at 400k, `122,605` at 500k, `183,952` at 750k, and `245,263` at 1M.

#### Observed point changes and working-set landmarks

- **Measured points:** P2 stays near `0.84 ns/alive` through 300k. P4 intent/projection rises from `20.188 ns/alive` at 50k to `26.963` at 100k, but the 100k cell has 8.215% MAD. P7 rises from `36.264` at 200k to `45.068` at 300k (both cells are within the 5% MAD gate). P9 rises from `11.428` to `12.670` across the same interval with both cells within the gate. P6A and P8 also have higher 300k point medians than 200k, but their 300k MAD ratios are 5.868% and 6.800% and are unreliable.
- **Measured capacity reference:** [M3 Scale Limit](M3_05_SCALE_LIMIT.md) measured about 47 MB SoA, 44 MB CSR, and 72 MB slot projection at 1M. Adding the per-agent feature (24 B), choice (about 8 B), intent (about 24 B), and the Phase 6B validated/keyed capacity buffers (about 48 B each) gives an approximate `315N` resident runtime set: about 31.5 MB at 100k, 47.3 MB at 150k, and 63 MB at 200k, before metrics indices, event storage, and other phase scratch. This layout-derived estimate is not a hardware counter.
- **Inference — memory hierarchy:** the estimated retained runtime set is near the host's 32 MiB L3 at 100k and above it by 150k, overlapping the P4/P7/P9 point changes. The 1 MiB L2 is exceeded by individual working vectors earlier, while sequential P2 remains flat. This is consistent with cache effects but does not prove a particular cache miss caused the changes.
- **Inference — TLB and gather locality:** the 72-byte-per-agent projection alone reaches the corrected 16 MiB L2 DTLB reach at about 233k agents. P4 intent cost keeps rising through 750k and is the phase with the clearest reversed-slot slowdown. That alignment is consistent with translation and gather locality pressure; no TLB/cache PMU counters were collected, and thread migration within the six-core affinity set weakens direct TLB attribution.

### Normal versus reversed slot layout

Each layout started from the same logical day-0 world, and State, Metrics, and Events digests matched exactly at both sizes. Values are median slowdown (`reversed / normal`); `†` marks a comparison where either layout's MAD/median exceeded 5%.

| Phase | N=100k slowdown | N=100k normal/reversed MAD% | N=1M slowdown | N=1M normal/reversed MAD% |
|---|---:|---:|---:|---:|
| P1 regrowth | 1.0000† | 16.667% / 16.667% | 0.9600† | 8.000% / 8.333% |
| P2 degradation | 0.9836 | 1.988% / 1.546% | 0.9923 | 2.759% / 1.809% |
| P3 features | 1.0458† | 10.008% / 4.641% | 1.0651† | 6.600% / 2.140% |
| P4 action selection | 1.5601 | 0.461% / 1.024% | 2.3734 | 1.889% / 3.752% |
| P4 intent/projection | 2.3131† | 8.215% / 8.601% | 3.6011† | 3.986% / 5.489% |
| P5 partition | 1.0180† | 4.101% / 7.039% | 1.0458† | 4.018% / 7.693% |
| P6A work resolution | 0.8412 | 1.849% / 1.822% | 1.0682 | 3.725% / 3.307% |
| P6B targeted resolution | 1.0025† | 15.739% / 7.532% | 1.0172† | 8.959% / 10.200% |
| P7 market | 1.0993† | 11.370% / 10.222% | 1.0412 | 4.818% / 4.669% |
| P8 welfare | 0.9858 | 3.356% / 3.985% | 0.9779 | 6.027% / 4.468% |
| P9 mortality | 0.9992 | 1.186% / 1.222% | 0.9922 | 4.134% / 3.234% |
| P10 metrics | 0.9948† | 31.973% / 31.351% | 1.0073† | 19.086% / 20.397% |
| P11 events | 1.0013 | 2.132% / 3.416% | 0.9944 | 3.859% / 2.564% |

P4 selection is a reliable 2.37× slower under reversed slots at 1M; P4 intent/projection's 3.60× point estimate is not reliable under the 5% MAD rule. Exact parity digests:

| N | State | Metrics | Events |
|---:|---|---|---|
| 100k | `93feb3d05e83b5d1e4f611788ff53082cd30af24e2067417466911a405624f99` | `5e838e669bc5d4d9cdaea1476208db5516a610667dd9597a955c1aae20fb478c` | `6811a5ba42c51b26d5ed0103f70827f3ad5c09f426354b743e186acacdb2b5c3` |
| 1M | `b87563bf78ac15dd75bc31ad7e072425ac4d193b7652facb0fa005265302d1fd` | `9eba08f49ff13b46900a636b37798f4cb91528d5546d7944e493b0b8603991f8` | `596b2546f8c7a2cd15936c846339dc92894642ed7596a98e3f2e155e225e6999` |

### Estimated bytes touched per phase/day

These are code-derived logical field and scratch estimates, not hardware memory-controller counters. At 1M, `N=1,000,000`, `K=5,000`, `k=8`; `A` is alive agents, `W` work intents, `T` targeted intents, `E` keyed Phase 6B entries, `D` deaths, and `R` staged events. Sorting, hash-table bucket traffic, cache-line overfetch, and repeated comparison loads add traffic beyond the compulsory-field estimates.

| Phase | Estimated compulsory field traffic | Index, projection, scratch, and variable work |
|---|---|---|
| P1 | `8K` B (resource `f32` read + write; about 40 KB) | Settlement vector only; no agent columns. |
| P2 | `17N` B (alive 1, health 4 and food 4 read; health and food 8 written; about 17 MB) | Sequential column scan; no per-agent allocation. |
| P3 | `25N` B read + `24A` B feature output (about 49 MB) | Dense GroupId lookup vector, duplicate flags, and scarcity values are `O(K)`; the measured 0..K−1 IDs use direct lookup. Sparse/non-dense IDs use the sorted fallback. `AgentFeatures` output sort remains. |
| P4 action | About `53A` B (feature 24, AgentId 4, alive/health 5, cooperation/aggression/risk 12, choice output about 8; about 53 MB) | `features` and `choices` vectors are `O(N)`; choices are sorted by AgentId. |
| P4 intent | About `25N` B base reads + `24N` B Intent output (about 49 MB) | For `T` targeted initiators, projected-row offsets add about `16T` B; each of `kT` neighbor visits reads an 8-byte slot plus AgentId/alive/health/food (about 21 bytes). At `k=8`, this is approximately `184T` B plus the persistent projection (`72N` B at 64-bit slots, about 72 MB) and a `k`-sized AgentId candidate scratch. |
| P5 | About `24N` B to read intents and `24N` B to move them into group buckets (about 48 MB) | Reusable GroupId→bucket HashMap and per-group vectors; `K` partition records sorted by GroupId. Per-group Vec storage is allocated as groups are drained and rebuilt. |
| P6A | About `24N` B intent traversal plus roughly `45W` B of worker validation/planning/commit fields | `slot_of` HashMap gathers; settlement index `O(K)`; per-group Work lists sort to AgentId order, with `Σ W_g log W_g ≤ W log W`. |
| P6B | About `24T` B intent reads | `slot_of` lookups; validated/keyed records are about 48 B each. Their reusable buffers are reserved to population capacity (`N`) in the M3 run context; live entries are `T`/`E`. Global `E log E` sort, then live-state gathers and food updates. |
| P7 | About `24N` B Intent traversal plus buyer/seller live-column reads and food/wealth writes | Settlement index `O(K)`, participant HashSet and per-group raw/planned vectors; buyers and sellers are sorted within each group, total sorting bounded by `O(N log N)`. |
| P8 | About `25N` B in the storage eligibility/validation pass (AgentId, alive, health, food, wealth, GroupId; about 25 MB) | Eligibility buckets store `(AgentId, slot)` entries, about 16 bytes per recipient; reusable `K` buckets/index; recipient and payout outputs are per-group vectors. |
| P9 | About `9N` B read (AgentId, health, alive; about 9 MB) plus `D` alive-byte writes | Per-day duplicate-ID HashSet; two `D` vectors for slots/AgentIds and a `D log D` death-ID sort. |
| P10 | About `17N` B read (AgentId, food, wealth, alive; about 17 MB) | Fresh `usize` index vector up to `8A` B, settlement index up to `8K` B, duplicate-ID HashSet, and wealth-Gini sort `O(A log A)`. `phase10_observe_storage` allocates its two index vectors on each call. |
| P11 | No authoritative agent-column scan | Event buffer reserves `2N+K+4` `EventRecord`s; per-day event vectors, GroupId counters/HashMap, and canonical event sort `O(R log R)`. |

The canonical graph CSR is a resident `44N`-byte allocation at `k=8` (source IDs, row offsets, and 8 `AgentId` neighbors); the transient slot projection is `72N` bytes (`usize` row offsets and 8 `usize` slots). These arrays persist, but not every phase reads them. The estimated runtime set and cache/TLB comparison above are inferences from capacities and field widths; no cache- or TLB-miss counters were collected.

### Code inspection and remaining complexity

| Phase | Production serial call site and access pattern | Remaining bound / allocation facts |
|---|---|---|
| P4 intent | [`intents.rs::generate_intents_m3_storage_into`](../../crates/sim-model/src/intents.rs), `generate_intents_storage_impl`, and `Phase4CandidateSource::target_agent`: ordered choice scan, direct cursor lookup for AgentId-sorted storage, otherwise `slot_of`; GiveFood/StealFood read only projected neighbor slots and gather live columns. Candidate scratch is reused; output is sorted by AgentId. | `O(Nk + N log N)` worst case including output sort; fixed `k=8` makes candidate visits linear. No per-initiator full-storage scan. Reversed slots use hash slot lookups and random candidate-column gathers. |
| P6A | [`resolution.rs::phase6a_work_resolution_storage`](../../crates/sim-model/src/resolution.rs): partitions and each partition's intent Vec are walked sequentially; Work agents are gathered through `slot_of`, checked against live group/alive/health, staged, sorted by AgentId, then food is updated. `SettlementIndex` replaces repeated settlement scans. | `O(N log N)` worst case from per-group Work sorts, bounded by `Σ W_g log W_g`; retained settlement indexing is `O(K)`. No partition-times-population scan. |
| P7 | [`resolution.rs::phase7_market_clearance_storage_with_config`](../../crates/sim-model/src/resolution.rs): partitions are sorted by GroupId; Buy/Sell intents gather participant columns by `slot_of`; raw buyer/seller and planned vectors are built per group and sorted by AgentId. | `O(N log N)` worst case from per-group participant sorts, bounded by `Σ B_g log B_g + Σ S_g log S_g`; transient per-group vectors and participant HashSet remain. No settlement-times-population scan. |
| P8 | [`resolution.rs::phase8_welfare_distribution_storage_with_config_and_scratch`](../../crates/sim-model/src/resolution.rs): one physical-slot validation/eligibility pass uses a GroupId→bucket HashMap, then each settlement reads its own recipient list. Reversed slots trigger a per-bucket AgentId sort. | `O(N log N)` worst case if reversed slots require bucket sorts; on sorted slots, eligibility collection is one `O(N)` scan. Scratch bucket capacity is reused; outputs are per-group vectors. No per-settlement full-storage scan on the production path. |
| P9 | [`phases.rs::phase9_mortality_commitment_storage`](../../crates/sim-model/src/phases.rs): one sequential slot scan validates AgentId/health/alive, records dead slots and IDs, sorts death IDs, then writes alive=false to dead slots. | `O(N + D log D)` plus expected `O(N)` HashSet validation; the two death vectors allocate per day. |

The inspected paths contain no remaining `O(NK)` or `O(N²)` settlement-by-population scan. The measured round-robin initialization gives dense GroupIds `0..K−1`, so P3 uses the `SettlementIndex` direct-vector path in `O(1)` per agent; sparse/non-dense inputs retain an `O(log K)` sorted fallback. Remaining explicit superlinear work is bounded by `O(N log N)`: phase output/partition sorts, P6B's global keyed sort (`O(E log E)`), per-group P6A/P7/P8 sorts, Phase 9 dead-ID sorting, P10 Gini sorting, and P11 event sorting. M3-K's GroupId index removed the repeated linear settlement scans; it did not remove these canonical ordering costs.

## Inferences by phase

| Phase | Classification | Inference and evidence |
|---|---|---|
| P1 | Noise | Timings are microseconds or smaller and many MAD ratios exceed 5%; its actual work is `O(K)`, so the per-agent quotient is not a useful cache indicator. |
| P2 | Memory hierarchy, mild | Its 17-byte sequential field pass stays near 0.82–0.86 ns/alive through 500k, with small later movement. No algorithmic superlinear operation is present in `SegmentedAgentStorage::phase2_degradation_with_config`. |
| P3 | Noise, mostly flat | Point estimates stay around 3.36–3.76 ns/alive, but most cells have MAD above 5%. Measured dense GroupIds use direct lookup; output sorting remains. No reliable alpha above 2 remains. |
| P4 action | Access pattern | Reversed slots are 2.37× slower at 1M with both MAD ratios below 5%. The sorted-storage cursor fast path is lost, causing `slot_of` fallback and personality-column gathers. |
| P4 intent | Access pattern, probable hierarchy effect | Per-agent cost rises from about 18.5 ns at 25k to 38.1 ns at 750k. Reversed-slot slowdown is 3.60× at 1M, but the reversed cell has 5.49% MAD and is just over the reliability limit. Projection/neighbor gathers are the main layout-sensitive code; attribution to cache or TLB is an inference, not a counter measurement. |
| P5 | Noise, possible hierarchy | Point estimates rise from about 12–14 ns/alive to 14 ns at 1M; several small-cell MADs are high. Group bucketing and per-group allocations fit `O(N+K log K)` and do not explain a reproducible quadratic curve. |
| P6A | Algorithmic `N log N` plus hierarchy | Both alpha intervals are excluded (9.877% MAD at 500k and 5.850% at 1M). Work-list sorts and `slot_of` gathers are remaining work, but there is no per-group full-population scan. Reversed layout is 1.07× at 1M with low MAD, so no large layout penalty was measured. |
| P6B | Algorithmic `E log E` plus noise | P6B costs about 179–309 ns/keyed entry in point estimates, with MAD above 5% in every cell. The resolver stages a global keyed stream and sorts it; rising per-entry cost is consistent with sort/cache growth, but these samples cannot identify an asymptotic alpha. No `K`-by-`E` scan remains. |
| P7 | Memory hierarchy / per-group ordering | Both alpha intervals are excluded by MAD above 5%; per-agent point cost rises from 20.9 ns at 50k to 65.7 ns at 1M. Reversed slots are only 1.04× at 1M with both MADs below 5%. Per-group participant sorts and temporary vectors are bounded by `O(N log N)`; no settlement-population scan remains. |
| P8 | Noise, possible hierarchy/allocation | Point cost rises to 17.8 ns/alive at 1M, but 1M MAD is 6.03%; reversed storage is 0.98× and the normal cell is also noisy. The production path groups recipients in one pass, so a remaining `O(NK)` eligibility scan is not supported by source inspection. |
| P9 | Mostly linear, with noise | Reliable alphas are 1.11 and 1.06. The 1M reversed result is 0.99× with MAD below 5%; the 1M normal per-agent time is about 15.0 ns. The hash validation and death sort remain, but no population-per-settlement rescan exists. |
| P10 | Measurement noise / allocation | MAD is 10.9–32.0% at most points, making its alpha unusable. The code allocates fresh N- and K-sized index vectors each day and sorts living-agent wealth indices; this is an `O(N log N)` path, not evidence for quadratic scaling. |
| P11 | Event staging and ordering | Per-agent point costs rise modestly from 22.0 ns at 25k to 29.8 ns at 1M; the 500k cell is noisy, while 750k and 1M are reliable. Event-buffer allocation and `O(R log R)` canonical sorting remain. Reversed layout is 0.99× at 1M. |

## Ranked bit-identical fix candidates — not implemented

1. **Compact the private Phase 4 slot projection.** Store neighbor slots as checked `u32` values while keeping `usize` row offsets. The M3 graph rejects populations at `2^24`, so slots fit exactly; this halves the 8-neighbor slot payload from 64 to 32 bytes per agent (about 32 MB saved at 1M). Preserve AgentId neighbor order and require the same digest and intent parity gates. No semantic contract change is indicated.
2. **Reuse Phase 10 index scratch in the M3 run context.** Replace the per-day convenience allocation with the existing `phase10_observe_storage_with_scratch` path using caller-owned `indices` and `settlement_indices`; retain identical ordering and arithmetic. P10's noisy measurements mean the gain must be measured with an uninstrumented whole-tick result. No semantic contract change is indicated.
3. **Prove and skip already-satisfied sorts.** For P4 output and per-group P6A/P7/P8 lists, detect strictly ascending AgentIds and bypass the sort only on that path, keeping the existing canonical sort fallback. This stays bit-identical if exact output order and duplicate/error behavior are preserved. No semantic contract change is indicated.
4. **Revisit P6B sort only after cleaner measurements.** A deterministic radix/key-only alternative could retain the full `(key, target, initiator, action)` comparator and stable tie rule, but the current P6B phase cells all fail the MAD gate. No algorithm change is supported by this run alone.

## Validation and scope

- `cargo fmt --check` — passed.
- `cargo clippy --all-targets -- -D warnings` — passed.
- `cargo test` was not run; this task changed only the opt-in benchmark harness and documentation, not production behavior or test code.
- The final measurement output and stderr are under ignored `target/`; no profiling counters, timers, or measurement code were added to production paths.
- No existing test, golden, snapshot, contract outside the requested stage-plan entry, or CI file changed.
