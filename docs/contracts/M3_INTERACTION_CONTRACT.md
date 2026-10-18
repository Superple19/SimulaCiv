# M3 Interaction Contract

**Status: §1–§7, §9–§13 FROZEN**

**Scope: M3-00 semantic reference (§1–§7), M3-01 optimized serial contract (§9), M3-02 deterministic parallel Phase 6B contract (§10), M3-03 (formerly M3-02b) search and bookkeeping reduction contract (§11), M3-06 (formerly M3-K) settlement lookup indexing contract (§12), and M3-08 (formerly M3-P) population dynamics contract (§13)**

This document freezes M3-00 (§1–§7), M3-01 (§9), M3-02 (§10), M3-03 (formerly M3-02b, §11), M3-06 (formerly M3-K, §12), and M3-08 (formerly M3-P, §13); other later M3 stages are planned in §8.

The research question is: **How much interaction complexity can a deterministic simulation runtime support before its execution model or data architecture must fundamentally change?** The M3-00 reference workload deliberately breaks the assumption that a settlement is an independent interaction partition.

## 1. Baseline and terminology

The frozen behavior is documented in [M1 Contract Freeze](M1_CONTRACT_FREEZE.md) and its machine-readable projection, [`contracts/m1_contract.toml`](../../contracts/m1_contract.toml). Relevant implementation references are [`intents.rs`](../../crates/sim-model/src/intents.rs) and [`resolution.rs`](../../crates/sim-model/src/resolution.rs). The corresponding design references are [§11](../../SIMULACIV_DESIGN_SPECIFICATION.md#11-command-intent-and-localized-conflict-resolution-architecture), [§15](../../SIMULACIV_DESIGN_SPECIFICATION.md#15-relationship--interaction-topologies-on2-avoidance), and [§17](../../SIMULACIV_DESIGN_SPECIFICATION.md#17-rng-logical-addressing--determinism-specification) of the design specification.

Here, `N` is the initialized population, `K` is the number of non-empty settlements, `n_g` is the population of settlement `g`, and an edge is a directed relationship from one source agent to one neighbor. `AgentId` is the logical graph key; `DenseSlot` is never a graph identity. Settlement membership is static in M3-00 because migration is out of scope.

M1 conflicts are listed in §5 with the frozen M3-profile resolution or amendment; the M2 profile remains unchanged.

## 2. Frozen contract items

### 2.1 Relationship graph

- Each initialized agent has exactly `k` directed outgoing edges.
- The logical graph is CSR keyed by ascending `AgentId`; stored source and neighbor identities are `AgentId`, never `DenseSlot`. Each row's neighbors are stored in ascending `AgentId` order.
- A source cannot point to itself and cannot contain the same neighbor more than once. Reciprocal edges are allowed and are independently sampled.
- The graph is generated once during initialization and remains unchanged throughout M3-00 and M3-01.
- For `profile = "m3"`, compute one canonical SHA-256 hash of the CSR at initialization using domain tag `SIMCIV_M3_RELATIONSHIP_CSR_V1`, a `u64` little-endian source count, then each ascending source `AgentId` (`u32` little-endian), row degree (`u32` little-endian), and ascending neighbor `AgentId`s (`u32` little-endian). Reuse this graph hash in every M3 State digest.
- The M3 State digest uses the M1 canonical State field order and encoding under domain tag `SIMCIV_M3_STATE_V1`, followed by the 32-byte CSR hash. The graph hash is recomputed from config and seed when the static graph is regenerated after restore; it is not added to the snapshot schema.
- If a neighbor later dies or becomes behaviorally ineligible, that edge remains in the graph. Candidate selection skips the neighbor; it does not rewire.
- The M3 contract amendment adds `RelationshipGraph` subsystem ID `6` for `profile = "m3"` only. For `profile = "m2"`, subsystem IDs `0`–`5` and the M1 manifest remain unchanged.
- Generation coordinates use `day = 0`, `phase = 0`, and the source `AgentId`. For edge slot `j` starting at zero, draw index `2j` selects locality and `2j + 1` selects a neighbor.

### 2.2 Locality dial, normative sampling, and required equivalent algorithm

`p_local` is the locality threshold for each directed edge. For each edge slot, compare its coordinate draw using `u < p_local`: true selects the source's own settlement, false selects all agents in other settlements. The target draw then selects an index from that candidate class.

The following pool-and-remove definition is the **normative sampling semantics**. It defines the exact graph regardless of the implementation algorithm:

- For a source in settlement `g`, the local pool is every other initialized agent in `g`, and the cross-settlement pool is every initialized agent whose settlement differs from `g`. Both pools are ordered by ascending `AgentId`.
- For each edge slot, draw locality first. In the selected pool, let `C` be its current size after earlier chosen neighbors from that same pool have been removed. Use the next coordinate draw and select index `min(floor(u * (C as f32)), C - 1)`. Remove the chosen agent from that pool before processing the next edge slot.
- Sampling is over agents, not settlements, so larger settlements contribute more cross-settlement candidate positions than smaller settlements. Choosing a destination settlement uniformly is not part of the frozen sampling rule.

The M1 coordinate conversion produces `u = m / 2^24` for integer `m` in `[0, 2^24)`. With the strict `u < p_local` comparison, the exact local-branch fraction over this coordinate lattice is `q_local = ceil(p_local * 2^24) / 2^24` (including `p_local = 0` and `p_local = 1`). Thus the analytically expected cross-edge fraction is `1 - q_local` under uniform coordinate outputs. For the exact quarter-step values in the table below, this equals `1 - p_local`.

For the normative index formula, `C` must be less than `2^24` so the integer candidate count is exactly representable as `f32`. Requiring population `N < 2^24` guarantees this bound for graph-generation pools; Phase 4 neighbor candidate counts are at most `k <= 16`.

The **required implementation** is the general non-contiguous rank-index algorithm below. It must not materialize a local or cross-settlement candidate pool for each source and must return the exact same `AgentId` as the normative pool-and-remove definition:

1. Maintain one global array of agents in ascending `AgentId` order. Build a `GroupId`-sorted directory and, for each group, its sorted list of global AgentId ranks. These lists contain `N` ranks total; a per-source-rank entry points to its group's list, so no dense `GroupId` assumption is made.
2. For local selection, the base order is the source group's rank list. For zero-based cross-pool rank `r`, binary-search for the smallest global rank `x` whose prefix count `x + 1 - upper_bound(group_ranks[g], x)` exceeds `r`; this maps the rank-th agent outside `g` without materializing the complement pool.
3. Preserve pool removal with rank adjustment. Keep the at-most-`k` previously selected base-pool ranks for the source and locality class in sorted order. For a local pool, also exclude the source's own rank. Given a draw rank `r` in the remaining pool, set `base_rank = r`; for each excluded rank `e` in ascending order, increment `base_rank` when `e <= base_rank`, stopping at the first `e > base_rank`. Map that base rank through step 1 or 2. After choosing all edges, sort the row by `AgentId` for CSR storage; this final sort does not affect edge choices.

The required algorithm uses the same coordinates, residual-pool size `C`, `f32` multiplication, floor, and upper clamp as the normative formula. Rank adjustment only maps the chosen residual-pool position into the original sorted pool; it does not change sampling order.

The general index costs `O(N + K)` space and `O(N log(K + 1))` construction time when input is already AgentId-ordered; sorting unsorted input adds `O(N log N)`. Complement rank selection costs `O(log^2 N)`, while exclusion adjustment costs `O(k)` per edge. Graph generation therefore costs `O(N log(K + 1) + Nk(log^2 N + k) + Nk log k)` time plus `O(N + K + Nk)` index and CSR storage, or `O(N log^2 N)` time for bounded `k` and `K <= N`. This required path avoids `O(N^2)` per-source pool materialization.

#### Optional round-robin arithmetic fast path

The current [`initialization.rs`](../../crates/sim-model/src/initialization.rs) assigns `agent_idx = i as u32`, stores `AgentId(agent_idx)`, and assigns `group_id = i % K` (where `K` is the configured settlement count). Thus AgentId rank `x` belongs to group `x mod K`; for `K > 1`, settlement members are interleaved rather than contiguous. The general index remains required for all layouts, but an implementation may use this `O(1)` arithmetic mapping while membership is exactly round-robin and static.

For a source group `g`, local base rank `q` maps to AgentId `g + qK`; exclude the source and previously selected local neighbors with the same rank adjustment above. For cross-settlement mapping let `Q = floor(N / K)`, `R = N mod K`, and `F = Q(K - 1)`. For `K > 1`, zero-based cross-pool rank `r` maps as:

```text
if r < F:
    block = floor(r / (K - 1))
    offset = r mod (K - 1)
    agent_rank = block*K + offset + (offset >= g ? 1 : 0)
else:
    tail_rank = r - F
    agent_rank = Q*K + tail_rank + (g < R && tail_rank >= g ? 1 : 0)
```

For a selected cross-settlement AgentId `x`, the inverse base-pool rank is `x - (x >= g ? floor((x - g) / K) + 1 : 0)`; a local AgentId `x` has base rank `(x - g) / K`. Because this initializer assigns `AgentId` equal to the global rank, the mapped rank is the AgentId value. The arithmetic path must use these inverse mappings to track chosen base-pool ranks and must be bit-identical to the required general algorithm and normative pool-and-remove semantics. It must be disabled as soon as membership stops being round-robin or static; it will not apply once M3-02 migration exists. Its rank mapping is `O(1)` per draw, with `O(k)` exclusion adjustment; for `k <= 16`, total graph generation is `O(N)` time and `O(Nk)` CSR output storage.

To ensure every possible locality-draw sequence can fill all `k` slots without fallback or duplicate edges, validate capacity before generation:

- If `k = 0`, no edge draws or pool-capacity checks are needed.
- If `k > 0` and `p_local = 1`, every source must have at least `k` other agents in its settlement: `n_g - 1 >= k`.
- If `k > 0` and `p_local = 0`, every source must have at least `k` agents outside its settlement: `N - n_g >= k`.
- If `k > 0` and `0 < p_local < 1`, both conditions must hold for every source.

With one settlement and `k > 0`, only `p_local = 1` is valid because no cross-settlement pool exists. The cross-partition edge ratio is zero when `k = 0` and there are no edges to count.

### 2.3 GiveFood and StealFood target candidates

The candidate set for each targeted action is the initiator's graph neighbors, filtered against live state at Phase 4 and ordered by ascending `AgentId`:

- Both actions require `alive == true`, `health > 0`, and `neighbor AgentId != initiator AgentId`.
- `GiveFood` additionally requires `neighbor.food < starvation_threshold`.
- `StealFood` additionally requires `neighbor.food > 0`.
- The same-settlement predicate is removed for the M3 interaction profile; cross-settlement neighbors are eligible.
- If no neighbor passes the filter, emit `target = None`.

Target choice retains the existing Phase 4 coordinate scheme and index formula: `GiveFood` uses `(current day, phase 4, MutualAidTarget subsystem 3, initiator AgentId, DrawIndex 1)`; `StealFood` uses `(current day, phase 4, TheftTarget subsystem 2, initiator AgentId, DrawIndex 1)`. Each selects `min(floor(u * C), C - 1)` from the ascending candidate list. The requested action amounts and Phase 4 action-selection semantics are otherwise unchanged.

This is the frozen candidate rule for `profile = "m3"`. The `profile = "m2"` path retains the same-settlement candidate rule in design specification §6.7–6.8 and M1 C05.

### 2.4 Global Phase 6B resolution reference

The M3 single-threaded reference collects every targeted intent with `target = Some(AgentId)` across all settlements into one global stream. `target = None` remains a zero-op classification before key evaluation: it consumes no resolution key or TheftSuccess draw, emits no command, and does not trigger target reselection.

The frozen M3 key retains the existing key packing and Mix64 folding, but clears the `group_id` field in `W2`. It uses the frozen M1 `K_PRIME = 0x9e3779b97f4a7c15` and Stafford Mix13 `Mix64`:

```text
W0 = MasterSeed
W1 = ((ReplicateId as u64) << 32) | Day
W2 = (6u64 << 48) | (5u64 << 32)       // no group_id bits
W3 = ((target_agent_id as u64) << 32) | initiator_agent_id
W4 = action_kind as u64                // GiveFood = 3, StealFood = 4

h0 = W0.wrapping_add(K_PRIME)
h1 = Mix64(h0 ^ W1)
h2 = Mix64(h1 ^ W2)
h3 = Mix64(h2 ^ W3)
h4 = Mix64(h3 ^ W4)
ResolutionKey = Mix64(h4)
```

Sort the complete stream by ascending `(ResolutionKey, target_agent_id, initiator_agent_id, action_kind)`. This is the frozen key for `profile = "m3"`; M1 `compute_resolution_key`, including `group_id`, remains the `profile = "m2"` behavior.

Resolve each entry in that order against authoritative live state and commit immediately. A later entry observes all earlier food changes:

- **GiveFood:** the initiator and target must be alive with `health > 0`; the initiator must have food; the target must remain below the starvation threshold. Cross-settlement targets are allowed. Transfer `min(requested_amount, live initiator food)` by debiting the initiator and crediting the target.
- **StealFood:** the initiator and target must be alive with `health > 0`; the target must have food. Cross-settlement targets are allowed. Preserve the existing TheftSuccess coordinate `(current day, phase 6, TheftSuccess subsystem 4, initiator AgentId, DrawIndex 0)` and `u < theft_success_probability` rule. On success transfer `min(requested_amount, live target food)` by debiting the target and crediting the initiator; on failure make no food change.
- An ineligible or missing target yields no state mutation. Phase 6B does not reselect a target.

The initiator's `group_id` remains the provenance group captured by the intent and must match the live initiator; it is not part of the frozen M3 key. The target's group is not required to match the initiator's.

The frozen event mapping retains the existing event schema and EventKey shape, with `partition_key` set to the initiator's `group_id` (as `u64`). Follow the existing Phase 6B structure: emit `zero_target` events first, ordered by ascending initiator `AgentId` within each group; then emit keyed events in that group's subsequence of the global keyed stream. Assign `local_sequence` continuously from that group's Phase 6A event count through both lists. The canonical EventKey sort therefore preserves the within-group order while grouping events by `partition_key` across groups.

### 2.5 M2 and M3 profile boundary

The `m2` profile uses the existing M2 code path unchanged. It must continue to produce these canonical digests and preserve existing snapshot behavior:

| Digest | Frozen value |
| --- | --- |
| State | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` |
| Metrics | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` |
| Events | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` |

The M3 resolver has no M2-equivalence mode. Its graph candidates and group-free key are M3 semantics; removing `group_id` changes key values and can reorder interactions within a settlement. Because Phase 6B commits immediately, a changed order can change later eligibility and food transfers. The `m2` path remains separate and unchanged.

The M3 profile has separate State, Metrics, and Events goldens. These do not replace or re-bless M2 values.

### 2.6 M3 metrics

- **Cross-partition edge ratio:** store `(cross_partition_edge_count: u64, total_directed_edge_count: u64)`; do not convert the ratio to float. Count stored directed edges, including edges whose target later dies. When there are no edges, store `(0, 0)` and define the reported ratio as zero.
- **Cross-partition interaction count per day:** count each Phase 4 targeted intent with `target = Some(...)` whose source and target have different `group_id`, whether Phase 6B later applies, fails, or rejects it after live-state validation. `target = None` is excluded. Store the count as `u64`.

These metrics belong to a separate M3 metrics profile and digest; frozen M2 `DailyMetrics` and its digest remain unchanged. Each M3 daily record encodes the frozen M1 metrics fields in their M1 order, followed by the edge-ratio numerator, edge-ratio denominator, and daily cross-partition interaction count as little-endian `u64`s. The static ratio pair is repeated in each daily record. Use the M1 canonical Metrics SHA-256 and day-ordering rules with distinct domain tag `SIMCIV_M3_METRICS_V1`.

### 2.7 Configuration and validation

The config field `profile` accepts `"m2"` or `"m3"`; if absent, it means `"m2"`. Existing configurations therefore stay on the unchanged M2 path and produce identical results. Explicit `profile = "m3"` selects the M3 semantics in this document.

M3 config fields are `interaction.relationship_degree_k` and `interaction.p_local`:

- `relationship_degree_k`: integer `k` in `[0, 16]`, following the fixed maximum degree in design specification §15.
- `p_local`: finite `f32` in `[0.0, 1.0]`; reject NaN, infinities, and values outside the closed interval.
- For `k > 0`, validate the local and cross-settlement pool capacities in §2.2 for every source before graph generation. Do not silently reduce `k`, replace an empty pool, or rewire an edge.
- No default values are defined for `relationship_degree_k` or `p_local`; an explicit M3 profile must provide both values.

### 2.8 M3-00 implementation acceptance gates

The implementation task should not be accepted until it demonstrates:

1. Replay determinism for identical configuration, seed, replicate, and execution horizon; graph generation and resolution must be invariant to physical `DenseSlot` layout.
2. Passing CI on Ubuntu, Windows, and macOS.
3. The existing M2 code path, selected by `profile = "m2"` or an absent field, remains unchanged and passes the current State/Metrics/Events digests and snapshot pause/resume checks above. No M2-equivalence mode is added inside the M3 resolver.
4. Separate, committed M3 golden State, Metrics, and Events outputs for `profile = "m3"`. M3 goldens must not replace or re-bless M2 values.

The `RelationshipGraph` PRNG test vector and separate M3 State, Metrics, and Events goldens are deliverables of the M3-00 implementation task, not of this freeze.

## 3. Expected cross-partition edge ratio

For `k > 0`, valid capacities, and `K >= 2`, the expected cross-partition edge ratio is `1 - q_local`, where `q_local = ceil(p_local * 2^24) / 2^24` reflects the M1 coordinate `f32` lattice. It is independent of `K`; `K` changes the available pool sizes and valid configurations, not this expectation. The table assumes every settlement is non-empty and lists quarter-step `p_local` values, for which `q_local = p_local`. For `K = 1`, a positive degree is valid only when `p_local = 1`.

| `p_local` | `K = 1` | `K = 2` | `K = 4` | `K = 8` |
| ---: | ---: | ---: | ---: | ---: |
| 0.00 | invalid¹ | 1.00 | 1.00 | 1.00 |
| 0.25 | invalid¹ | 0.75 | 0.75 | 0.75 |
| 0.50 | invalid¹ | 0.50 | 0.50 | 0.50 |
| 0.75 | invalid¹ | 0.25 | 0.25 | 0.25 |
| 1.00 | 0.00 | 0.00 | 0.00 | 0.00 |

¹ Invalid for `k > 0` because no cross-settlement candidate exists. For `k = 0`, the ratio is defined as zero for every `p_local` and `K`.

## 4. Explicit non-goals

M3-00 does not include:

- agent migration between settlements;
- dynamic topology, rewiring, or changing `k` after initialization;
- propagation workloads;
- parallel Phase 4 or Phase 6B execution.

## 5. Frozen-reference conflicts and resolutions

The following resolutions freeze the M3 profile while preserving the existing M1 behavior for `profile = "m2"`.

| Reference | Frozen behavior and M3 conflict | Frozen M3 resolution |
| --- | --- | --- |
| M1 C04 and design spec §17 | Subsystem IDs `0`–`5`, coordinate packing, and existing PRNG vectors are frozen. | Add `RelationshipGraph = 6` to `profile = "m3"` only. For `profile = "m2"`, IDs `0`–`5` and the M1 manifest remain unchanged. The new PRNG test vector is an M3-00 implementation deliverable. See §6 OQ-1. |
| M1 C05 and design spec §6.7–6.8 | GiveFood and StealFood candidates are same-settlement and captured in Phase 4. | Keep the M2 path unchanged. Apply the AgentId-keyed graph and cross-settlement candidate semantics only for `profile = "m3"`. See §6 OQ-2 and OQ-5. |
| M1 C06 and design spec §11.1 | Phase 6B uses settlement-local streams and a `ResolutionKey` containing `group_id`. | Keep the M2 resolver unchanged. The separate M3 resolver uses the group-free global key defined in §2.4; there is no M2-equivalence mode inside it. See §6 OQ-2. |
| M1 C07 and design spec §11.1 | Events use `(day, phase, partition_key, local_sequence)` and current Phase 6B events are gathered by settlement. | Use initiator `group_id` as `partition_key`; emit zero-target events in ascending initiator `AgentId` order before keyed events in each group's global-stream order, continuing after Phase 6A's event count. See §6 RQ-3. |
| M1 C08 and M1 canonical State contract | Snapshot schema version 1 is frozen and has no relationship graph field. | Regenerate the static graph deterministically from config and seed in M3-00/M3-01; keep the snapshot schema unchanged. Include the once-computed canonical CSR hash in every M3 State digest; M3-03 dynamic topology will require graph persistence in snapshots. See §6 OQ-3 and RQ-2. |
| M1 C09 and M1 canonical Metrics contract | M2 `DailyMetrics` fields, formulas, and digest are frozen. | Keep `DailyMetrics` unchanged. Store the M3 edge ratio as two `u64`s and use a separate M3 metrics profile and digest with a distinct domain tag. See §6 OQ-4 and RQ-1. |
| M1 C10 and canonical hash requirements | Existing M2 State, Metrics, and Events goldens are frozen. | Keep the existing M2 path passing those goldens; M3 has separate goldens, and its State digest includes the canonical CSR hash. M3 goldens are M3-00 implementation deliverables. See §6 OQ-2 and RQ-2. |

M1 §8 governs amendments to the frozen M1 profile. This M3 contract amendment adds subsystem ID `6` for `profile = "m3"`; the M1 manifest and IDs `0`–`5` remain unchanged for `profile = "m2"`.

## 6. Frozen decision register

The decisions in this register are frozen for the M3-00 workload and semantic reference.

| ID | Decision | Status | Frozen resolution |
| --- | --- | --- | --- |
| OQ-1 | RelationshipGraph subsystem ID | **FROZEN** | Assign ID `6` in the M3 amendment for `profile = "m3"` only. For `profile = "m2"`, IDs `0`–`5` and the M1 manifest are unchanged. |
| OQ-2 | M2 equivalence and M3 resolver boundary | **FROZEN** | No equivalence mode inside the M3 resolver. Keep the existing M2 code path unchanged and passing current M2 goldens; M3 is a separate profile with its own goldens. |
| OQ-3 | Static graph pause/resume handling | **FROZEN** | Regenerate the graph deterministically from config and seed during M3-00/M3-01; do not change the snapshot schema. M3-03 dynamic topology will require graph persistence in snapshots. |
| OQ-4 | M3 metrics and cross-partition Phase 6B event keys | **FROZEN** | Use a separate M3 metrics profile and digest; leave `DailyMetrics` unchanged. Set Phase 6B event `partition_key` to initiator `group_id`; within each group, put `zero_target` events in ascending initiator `AgentId` order before keyed events in global-stream order, with `local_sequence` continuing after Phase 6A events. |
| OQ-5 | Profile selection and legacy config behavior | **FROZEN** | `profile` is `"m2"` or `"m3"`; absent means `"m2"`, so existing configurations keep identical results. |
| RQ-1 | M3 edge-ratio metric representation and digest | **FROZEN** | Store numerator and denominator as `u64` with no float conversion. Encode each M3 daily record using M1 metric rules, then append numerator, denominator, and interaction count as little-endian `u64`s; use distinct domain tag `SIMCIV_M3_METRICS_V1`. |
| RQ-2 | Relationship graph in M3 State digests | **FROZEN** | Hash canonical CSR once at initialization in ascending source and neighbor `AgentId` order; include that hash in every M3 State digest under the M3 State domain tag. |
| RQ-3 | Ordering of zero-target Phase 6B events | **FROZEN** | Follow `zero_target` then `keyed_stream`; within each initiator group, emit zero-target events in ascending initiator `AgentId` order, then keyed events in that group's global-stream order. Continue `local_sequence` after Phase 6A event count. |

No open questions remain in the frozen M3-00 reference.

## 7. Freeze record

This freeze sets the contract status to FROZEN, records the M3 amendment that adds `RelationshipGraph` subsystem ID `6` for `profile = "m3"` only, and preserves IDs `0`–`5` and the M1 manifest for `profile = "m2"`. It freezes the M3-00 workload and single-threaded semantic reference without changing code, tests, goldens, snapshots, the M1 manifest, or CI.

This freeze authorizes the M3-00 implementation task to be requested; it does not start that task. The `RelationshipGraph` PRNG test vector and separate M3 State, Metrics, and Events goldens are deliverables of the M3-00 implementation task, not of this freeze. M3-01 onward remains out of scope.

## 8. M3 stage plan (PLANNED — not frozen)

This plan is non-frozen. Each stage has its own contract section and must pass through **DRAFT → review → FROZEN** before that stage's implementation begins.

### Stage-number mapping

The identifiers below put the stages in execution order. Existing filenames and document anchors remain unchanged.

| Previous identifier or name | Current identifier | Status / disposition |
|---|---|---|
| M3-00, M3-01, M3-02 | M3-00, M3-01, M3-02 | Unchanged |
| M3-02b — Search and bookkeeping reduction | M3-03 | Complete |
| M3-E — Equilibrium workload | M3-04 | Complete, benchmark-only stage |
| M3-S — Scale limit search | M3-05 | Complete, measurement-only stage |
| M3-K — Settlement lookup indexing | M3-06 | Complete |
| Residual-scaling diagnosis | M3-07 | Complete, measurement-only stage |
| M3-P — Population dynamics | M3-08 | Planned; dynamic topology is absorbed here |
| M3-05 — Propagation | M3-09 | Planned |
| M3-03 — Spatial migration | Deferred, unnumbered | Deferred follow-up |
| M3-04 — Dynamic topology | M3-08 | Absorbed into population dynamics |

### M3-00 — Reference workload — COMPLETE

Single-threaded reference with the relationship graph, neighbor targeting, global Phase 6B resolution, and separate M3 goldens.

### M3-01 — Optimized path and serial baseline — COMPLETE

Port M3 to the Segmented SoA path with index-based neighbor and target lookup, serially. The golden, §9.7 differential, snapshot-resume, shuffled-slot, and release-differential gates passed. The serial baseline and conflict-density results are recorded in [M3-01 Serial Baseline](../performance/M3_01_SERIAL_BASELINE.md).

### M3-02 — Deterministic parallel Phase 6B — COMPLETE

The frozen primary method, dependency-level scheduling, passed exact State, Metrics, and Events parity against M3-01 serial. Round-based Deterministic Reservations remains a comparison-only schedule. No settlement-partition fast path was added; M3-01 measured a conflict fraction of about `0.54` even at `p_local = 1`. Serial remains the default and parallel execution remains an explicit opt-in. The release benchmark, extended parity results, per-stage timings, level structure, and contention measurements are recorded in [M3-02 Parallel Phase 6B](../performance/M3_02_PARALLEL_PHASE6B.md).

### Optional stage after M3-02 — Contention dial

An optional hub-biased or otherwise contention-controlled graph mode may test scheduling across different endpoint-contention levels. It changes graph semantics and requires its own contract amendment before implementation.

### M3-03 (formerly M3-02b) — Search and bookkeeping reduction — COMPLETE

The explicit serial search-reduction path is opt-in; `run_m3_storage_days` remains the oracle. The frozen parity gates passed. The global Phase 4 eligibility skip was retained after stage measurement; bitsets, slot-sidecar reuse, and radix sorting were rejected by the measurement-first gate, and output-vector de-duplication remained out of scope. Results and limitations are recorded in [M3-02b Search and Bookkeeping Reduction](../performance/M3_02B_SEARCH_REDUCTION.md).

The post-M3-02 evidence is summarized in the [M3 Interim Audit](../performance/M3_INTERIM_AUDIT.md). It is an interim answer, not the M3 graduation gate.

### M3-04 (formerly M3-E) — Equilibrium workload — COMPLETE (benchmark-only)

Search for and measure a food-supply configuration that sustains interactions. This is a benchmark workload and does not change simulation semantics; results are recorded in [M3-04 Equilibrium Workload](../performance/M3_EQUILIBRIUM_WORKLOAD.md) and [M3-04 Sustained Workload](../performance/M3_SUSTAINED_WORKLOAD.md).

### M3-05 (formerly M3-S) — Scale limit search — COMPLETE (measurement-only)

Measure the static-world workload's scale limits using the equilibrium configuration. This stage adds measurement only; results are recorded in [M3-05 Scale Limit](../performance/M3_SCALE_LIMIT.md).

### M3-06 (formerly M3-K) — Settlement lookup indexing — COMPLETE

Index repeated GroupId lookups on the shared M2/M3 storage path without changing output or error semantics. The implementation contract is in [§12](#12-m3-k-settlement-lookup-indexing); before/after work counts, scaling, and whole-tick measurements are recorded in [M3 Settlement Index](../performance/M3_SETTLEMENT_INDEX.md).

### M3-07 — Residual scaling diagnosis — COMPLETE (measurement-only)

Determine whether post-M3-06 superlinear scaling is measurement noise, memory-hierarchy effects, or remaining algorithmic cost. Findings are recorded in [M3-07 Scale Residual](../performance/M3_07_SCALE_RESIDUAL.md).

### M3-08 (formerly M3-P) — Population dynamics — PLANNED

Add births and define newborn graph attachment, dead-agent cleanup and storage reuse, deterministic `AgentId` allocation, and graph persistence in snapshots. The frozen M3-08 contract is in [§13](#13-m3-08-formerly-m3-p-population-dynamics); its freeze authorizes the implementation task to be requested but does not start it. The current model has no births: initialization sets every `birth_day` to zero, and no simulation phase creates agents.

Spatial migration, previously listed as M3-03, remains a deferred follow-up and is not part of M3-08's planned scope.

### M3-09 (formerly M3-05) — Propagation — PLANNED

Model multi-hop spread over the graph within one tick and define intra-tick ordering.

### M3 audit and graduation

Report in the M2-40 format, answer the central research question, and decide whether a new architecture (M4) is needed.

M3-04 and M3-05 extend the measured answer for sustained workloads and scale limits. M3-08 and M3-09 deepen the research question with population dynamics and propagation; M3-07 diagnoses residual scaling after indexed settlement lookups.

## 9. M3-01 Optimized path and serial baseline

**Status: FROZEN**

### 9.1 Scope and reference authority

M3-01 runs the frozen M3 semantics on the Hybrid / `SegmentedAgentStorage` authority path, serially. Its public explicit opt-in entry point is `run_m3_storage_days`, not a new `Phase4ExecutionPolicy` variant or a mode on an M2 runner. It requires `profile = "m3"`; existing M2 runners continue to reject that profile. It does not change the M3-00 single-threaded `WorldState` reference; M3-00 remains the oracle for M3 in the same way that M0 remains the oracle for M2. The optimized M3 path must match M3-00 State, Metrics, and Events digests exactly for identical configuration, seed, replicate, execution options, and horizon. Existing M2 defaults remain unchanged.

### 9.2 Relationship graph execution projection

The canonical `RelationshipGraph` remains keyed by ascending `AgentId` and its CSR hash remains the M3 State-digest input from §2.1. The storage path may build a transient projection whose rows are indexed by the current SoA storage slot and whose neighbor entries are storage slots resolved with `SegmentedAgentStorage::slot_of(AgentId)`. These are column indices into the storage arrays; they must not be inferred from `AgentId`, the `DenseSlot` value, or physical vector order. Preserve each canonical row's ascending `AgentId` neighbor order when mapping its neighbors to slots.

M3-01 storage never adds, removes, or reorders agents. Build the projection once per storage instance after its `AgentId`-to-slot map is ready and before the first M3 Phase 4 intent generation. Rebuild it whenever storage is constructed, rebuilt, replaced, or restored from a snapshot. Restore reconstructs storage from the snapshot's `WorldState`; regenerate the relationship graph as required by §2.1, then build the slot projection against the restored storage map. In debug builds, verify projected source and neighbor slots against `slot_of` for a deterministic sample: the first, middle, and last source rows and the first, middle, and last neighbor of each sampled non-empty row. A slot-map version counter is deferred to M3-03, when membership changes begin.

The projection and its scratch are transient runtime data. They are excluded from `WorldState`, `SegmentedAgentStorage` serialization, State/Metric/Event digests, and snapshots. Only the canonical CSR graph hash enters the M3 State digest.

### 9.3 Phase 4 storage targeting

Use the existing serial storage Phase 4 action-selection path unchanged. For `GiveFood` and `StealFood`, use the slot projection to visit only the initiator's graph neighbors, reading live `alive`, `health`, `food`, and `AgentId` columns. Apply exactly §2.3's filters, exclude self, and retain ascending `AgentId` candidate order even when slots are shuffled:

- `GiveFood`: alive, health greater than zero, and food below `starvation_threshold`.
- `StealFood`: alive, health greater than zero, and food greater than zero.

Do not apply a same-settlement filter or fall back to another candidate source. Keep §2.3's coordinate tuple, subsystem, `DrawIndex = 1`, `floor(u * C)` selection, and upper clamp unchanged. Preserve all non-target intent fields and action-selection semantics from the M3-00 reference.

### 9.4 Phase 6B storage resolution

Resolve M3 interactions as one global keyed stream using §2.4's M3 `ResolutionKey` and tie-break order. During structural validation, resolve each initiator and non-null target through `storage.slot_of` once and retain those slots in transient reusable scratch; do not linearly scan all agent storage for each entry. Keep M3 zero-target classification and global keyed ordering, then re-read live storage fields immediately before each serial commit. Preserve M3's live eligibility rules, cross-settlement target allowance, theft RNG coordinate, and sequential-commit effects. In particular, the M2 StorageFast same-settlement target check does not apply to M3.

Reuse validation, global-key, and slot scratch across days, following the caller-owned Phase6B scratch lifetime in `run_hybrid_authority_days`. Return the same per-initiator-group resolution shape as M3-00: groups ascending, zero-target entries first in ascending initiator `AgentId`, then that group's subsequence of the global keyed stream. This keeps the existing event converter and Phase 6A local-sequence offset applicable without changing EventKey semantics.

### 9.5 Phases that retain the M2 storage path

The M3 storage runner retains the existing phase order. Only Phase 4 targeted-intent generation and Phase 6B resolution use M3-specific storage paths; Phase 4 action selection remains the existing serial storage implementation.

| Phase | M3-01 behavior | Why the existing settlement-scoped path remains valid |
|---|---|---|
| 1–3 | Existing resource regrowth, storage degradation, and storage feature extraction. | These phases do not select or resolve graph targets. Phase 3's scarcity input remains based on each agent's current group. |
| 5 | Existing partitioning by the intent's initiator `group_id`, with canonical group and per-group intent ordering. | A targeted intent retains its initiator group as provenance; its target group does not define its Phase 5 partition. |
| 6A | Existing storage work resolver. | It accepts only `Work` intents and allocates each group's own settlement resource; it has no target-agent operation. |
| 7 | Existing storage market resolver. | It selects only `BuyFood` and `SellFood` participants belonging to the partition's group. It runs after global Phase 6B and reads the resulting live food and wealth. |
| 8 | Existing one-pass storage welfare path and reusable eligibility buckets. | It groups recipients by their current agent `group_id`, tests current food after Phase 6B, and debits only that group's treasury. Cross-settlement transfers are already committed before this phase. |
| 9 | Existing storage mortality commitment. | It evaluates each agent's current health and alive flag without settlement-local interaction assumptions. |
| 10–11 | Existing storage metrics, event staging/flush, and snapshot encoding. | M3 appends its separate metrics fields and uses the M3 State digest specified below; the frozen event schema and snapshot schema remain unchanged. |

### 9.6 M3 metrics and canonical digests on storage

Use `SegmentedAgentStorage::phase10_metrics` for the frozen M1 `DailyMetrics` fields. Append the graph's fixed `(cross_partition_edge_count, total_directed_edge_count)` pair and that day's cross-partition interaction count exactly as §2.6 specifies. Count each Phase 4 targeted intent with a non-null target whose intent group differs from the target's live storage group; include entries regardless of their later Phase 6B outcome. The pair and count are digest data, not simulation state.

For M3 State hashing, encode the authoritative SoA values with the same M1 field order and validation as the existing storage canonical-state encoder, use the `SIMCIV_M3_STATE_V1` domain, and append the canonical CSR hash. Do not hash a stale `WorldState` copy between snapshot boundaries. Encode the M3 Metrics records under `SIMCIV_M3_METRICS_V1` using §2.6. Hash Events with the existing canonical Events encoding and domain: §2.4 freezes the event schema and adds no M3 event domain tag. Emit Phase 6B events through the existing `events_from_targeted_resolution` conversion and Phase 6A offset logic.

### 9.7 Parity gates

Accept M3-01 only after the serial storage path has exact State, Metrics, and Events digest equality with the M3-00 reference for:

- the committed M3 golden configuration;
- the following 100-day CI differential matrix against M3-00, using `replicate_id = 7` and master seeds `{0, 1, 81985529216486895}` for `N = 300`: `K ∈ {3, 10}`, `k ∈ {4, 8, 16}`, and `p_local ∈ {0, 0.5, 1}`. With round-robin initialization, group sizes are 100 for `K=3` and 30 for `K=10`; all cells satisfy §2.2, so no combinations are skipped;
- for `N ∈ {1,000, 3,000}`, a 100-day CI differential using master seed `81985529216486895`, `replicate_id=7`, `k=8`, `p_local ∈ {0, 0.5, 1}`, and `K = floor(N / 17)` (58 and 176 respectively), so every settlement has at least 17 agents;
- snapshot pause/resume, with the relationship graph regenerated and slot projection rebuilt after restore;
- shuffled or permuted physical `DenseSlot` / storage-slot layouts representing the same logical state.

The existing M2 path must continue to pass the frozen State, Metrics, and Events digests in §2.5 and existing snapshot checks unchanged. M3 goldens remain separate. Run one additional 100-day release-mode differential at `N=10,000`, `K=50`, master seed `81985529216486895`, `replicate_id=7`, `k=8`, and `p_local=0.5`; record it in the performance report, not CI.

### 9.8 Measurement protocol

Record host CPU, physical/logical core counts, OS, and Rust toolchain. Use the M2-40 fixed-group-size workload and benchmark configuration unchanged, except for `profile = "m3"`, `relationship_degree_k`, and `p_local`; preserve its action weights and biases without tuning. Measure populations `N ∈ {10k, 50k, 100k}` with approximately 200 agents per settlement (`K ≈ N / 200`), across the 15 combinations `p_local ∈ {0, 0.25, 0.5, 0.75, 1}` and `k ∈ {4, 8, 16}`. Use only configurations satisfying §2.2 capacity rules.

Follow the M2-40 sampling method: initialize outside the timed run; run two warmups and seven timed samples per cell; reuse runner scratch over three consecutive days per sample; keep metrics and events enabled and snapshots disabled. Report median ± MAD per day/tick and per-phase time for Phases 1–11; split Phase 4 into action selection and intent/projection work, and Phase 6B into validation/slot lookup, global key/sort, and sequential commit. Time graph generation and the initial slot-projection build separately so their one-time cost is visible. For each measured cell, record the mean daily count of keyed Phase 6B entries and cross-partition interactions over the three days of each timed sample. `p_local = 1` is the baseline for the global stream's cost. M2 Serial timings at the same `N` are context only, not a like-for-like comparison, because M2 uses same-settlement targeting and settlement-local Phase 6B ordering.

Report path: [`docs/performance/M3_01_SERIAL_BASELINE.md`](../performance/M3_01_SERIAL_BASELINE.md).

### 9.9 Conflict-density measurement

For a day `d`, let `E_d` be its keyed Phase 6B entries (`target = Some`). Each entry touches its initiator and target. The conflict fraction is the number of entries that share either endpoint with at least one *other* entry in `E_d`, divided by `|E_d|`. The mean contention is the arithmetic mean, over distinct agents touched by `E_d`, of the number of entries in `E_d` touching each agent. If `E_d` is empty, both values are not applicable for that day. Record these values by configuration as predictors for M3-02. They are measurement-only and are excluded from all digests.

### 9.10 Explicit non-goals

- Rayon or any other parallelism.
- Changing production-runner defaults or the existing M2 paths.
- Changing M3-00 reference semantics, event schema, snapshot schema, or canonical M2 behavior.

### 9.11 Frozen decision register

| ID | Decision | Status | Frozen resolution |
|---|---|---|---|
| Q1 | M3-01 optimized serial API | **FROZEN** | Expose `run_m3_storage_days` as a separate public opt-in for `profile = "m3"` on Hybrid / `SegmentedAgentStorage` authority. Do not add an M3 variant to `Phase4ExecutionPolicy` or route M3 through M2 runners. Existing M2 runners reject M3; M3-00 reference runner remains unchanged. |
| Q2 | Benchmark model and action mix | **FROZEN** | Keep the M2-40 fixed-group-size configuration and action weights/biases unchanged; add only the M3 profile fields and the requested `k` / `p_local` values. For each measured cell record the mean daily keyed Phase 6B count and cross-partition interaction count. |
| Q3 | Projection lifetime and invalidation | **FROZEN** | M3-01 storage never adds, removes, or reorders agents. Build the projection once per storage instance; rebuild on construction, rebuild, replacement, or snapshot restore. Debug builds verify the deterministic sample specified in §9.2 against `slot_of`. Defer a slot-map version counter to M3-03. |
| Q4 | Differential matrix | **FROZEN** | Run the §9.7 CI matrix: `N=300`, `replicate_id=7`, master seeds `{0, 1, 81985529216486895}`, `K ∈ {3,10}`, `k ∈ {4,8,16}`, `p_local ∈ {0,0.5,1}`, 100 days; and `N ∈ {1,000,3,000}`, `replicate_id=7`, seed `81985529216486895`, `K=floor(N/17)`, `k=8`, `p_local ∈ {0,0.5,1}`, 100 days. Skip and list any cell failing §2.2; none fail for the specified round-robin populations. Run the `N=10,000` release differential once and record it in the performance report, not CI. |

### 9.12 Freeze record

This freeze resolves the §9 API, benchmark workload, projection lifecycle, and differential-matrix questions. It freezes the M3-01 optimized serial-path contract and authorizes the M3-01 implementation task to be requested; it does not start that task.

## 10. M3-02 Deterministic parallel Phase 6B

**Status: FROZEN**

### 10.1 Scope and opt-in policy

M3-02 adds an explicit opt-in parallel Phase 6B path for the M3 Hybrid / `SegmentedAgentStorage` authority path. The existing `run_m3_storage_days` remains the unchanged serial default, and the M3-00 `WorldState` reference remains unchanged as the oracle. Phase 6B is the only parallelized phase in this stage; Phase 4 action selection and neighbor-based intent generation, and all other phases, continue to use their M3-01 serial paths.

Expose the parallel path through a separate public M3 storage entry point that receives a caller-owned, already-built Rayon `ThreadPool` and a nonzero chunk size. Calling that entry point is the explicit opt-in. The runner must not construct, resize, cap, or replace the pool, select a backend from timing data, or introduce an automatic threshold. This follows the caller-owned pool and explicit execution model in the M2-38 and M2-39 Rayon gates without adding M3 to `Phase4ExecutionPolicy` or changing M2 runners.

### 10.2 Commit reads, writes, and dependencies

The M3-01 storage resolver first validates the theft probability, partitions, settlement membership, intent group, requested amount, and initiator/target slots. For each keyed entry, commit then reads the initiator's live `alive`, `health`, `group_id`, and `food`; the target's live `alive`, `health`, and `food`; the immutable intent fields; and the relevant configuration values. GiveFood checks the starvation threshold and initiator food. StealFood uses the target food and the stateless TheftSuccess coordinate `(master_seed, replicate_id, current_day, phase 6, subsystem 4, initiator AgentId, draw 0)` and the configured probability. No shared RNG state is consumed.

A successful GiveFood entry debits initiator food and credits target food; a successful StealFood entry debits target food and credits initiator food. Phase 6B does not mutate `alive`, `health`, `group_id`, settlements, treasury, or other agents' fields. It has no shared semantic counter. Its `TargetedResolution` values and per-group output vectors are transient output scratch; Phase 6B events are generated later from the reconstructed result by the unchanged event conversion and Phase 6A offset logic.

Thus two keyed entries conflict exactly when their initiator/target endpoint sets overlap. Entry `j` depends on an earlier entry `i` in the frozen global keyed order iff they share an endpoint. Preserving the order of all entries touching each agent preserves that agent's sequence of live food reads and IEEE `f32` updates; entries with disjoint endpoints have no shared mutable simulation state and may be evaluated concurrently. If implementation inspection finds any additional shared mutable simulation state in this path, parallelization is blocked until its dependency semantics are specified.

The serial resolver appends each result to a per-group keyed output vector. Parallel workers must not append concurrently to a shared bucket. Stage each outcome at its deterministic global-stream position, then reconstruct each group's keyed subsequence in global order after commit scheduling completes.

### 10.3 Parallelizable stages

The M3-01 pipeline and M2 Rayon implementation support these stages:

1. **Structural validation and slot lookup:** validate entries and resolve their endpoint slots independently where possible. Preserve the serial error protocol exactly: invalid theft probability first; then partition checks and intent checks in caller traversal order, with each entry's group mismatch before requested amount and slot lookup. Parallel validation must retain source positions and return the earliest error the serial traversal would return.
2. **M3 key generation:** compute each keyed entry's `compute_m3_resolution_key` independently from its frozen coordinates and fields.
3. **Global sort:** order the complete keyed stream by the frozen `(ResolutionKey, target AgentId, initiator AgentId, action kind)` comparator. The normal Phase 4 pipeline emits at most one intent per agent, so its keyed entries have distinct initiators. The storage resolver currently uses stable `sort_by`; if a parallel resolver accepts externally supplied entries with equal four-field keys, it must preserve their serial staging order as the final tie behavior.
4. **Commit scheduling and evaluation:** build dependency levels as specified in §10.4, then evaluate entries within each level in parallel.

Validation and key generation should use fixed indexed input positions and deterministic error/result collection; Rayon completion order and worker identity must not select errors or determine semantic output order. Reuse caller-owned scratch across days, as in M3-01 and M2's production Rayon path.

### 10.4 Dependency-level schedule

The primary schedule is dependency levels over the globally sorted keyed stream. Build a `last_level` array indexed by storage slot, initialized to zero. Traverse keyed entries in global order. For an entry with endpoint slots `a` and `b`, assign:

```text
level(entry) = 1 + max(last_level[a], last_level[b])
last_level[a] = level(entry)
last_level[b] = level(entry)
```

Store the entry's original global-stream index in that level. This is one serial `O(E)` pass and uses `O(N + E)` transient scratch for `N` storage slots and `E` keyed entries. Entries assigned to one level have pairwise-disjoint endpoints. Process levels in ascending level number. At the start of each level, workers read the current live state and compute each entry's exact serial eligibility, RNG outcome, transfer amount, resolution record, and proposed endpoint food values. Apply that level's disjoint food writes only after all its evaluations finish; use safe Rust and no `unsafe` for disjoint writes. A deterministic serial scatter in the level's original global-stream order is a valid writeback implementation. Complete the writeback barrier before evaluating the next level.

The schedule can reorder only entries that touch disjoint agents. For every shared endpoint, its level order is the same as the global stream order, so later entries observe earlier writes exactly as in serial Phase 6B. Preserve the original global index separately from the level so that level order and Rayon completion order never become output order. Record the number of non-empty levels and the largest level size as measurements.

The per-slot `last_level` and level-membership scratch is caller-owned and reused across days. Resetting it for a new day must not allocate; clearing retained buffers or using a generation stamp are both acceptable. It remains transient runtime scratch and does not enter simulation state, snapshots, or digests.

An alternative round-based Deterministic Reservations schedule may be measured for comparison only. It must reserve endpoints in global-stream order within each round and may execute only endpoint-disjoint entries concurrently. Dependency levels remain the primary method for M3-02; the alternative does not change serial semantics or become an implicit fallback.

### 10.5 Output and event order

Reconstruct one `SettlementTargetedResolution` per initiator group in ascending `GroupId`. Each group contains `zero_target` entries first in ascending initiator `AgentId` order, followed by that group's subsequence of keyed results in the global keyed-stream order. `resolutions` is `zero_target` followed by `keyed_stream`, exactly as in §2.4 and §9.4. The reconstructed values must be independent of worker count, chunk size, completion order, and level assignment. Continue to use the existing `events_from_targeted_resolution` converter and Phase 6A local-sequence offsets so Phase 6B event order and payload remain unchanged.

### 10.6 Correctness and parity gates

Require bit-identical State, Metrics, and Events digest sequences against both the serial M3-01 path and the M3-00 reference. CI covers:

- the M3 golden configuration and the full §9.7 100-day matrix: all 60 differential cells (`N=300` with the three listed seeds, `K ∈ {3,10}`, `k ∈ {4,8,16}`, `p_local ∈ {0,0.5,1}`, plus `N ∈ {1,000,3,000}` with their §9.7 `K`, seed, `k`, and locality settings), using pool sizes `{2,4}` and chunk size `256`;
- a thread/chunk sweep on the M3 golden configuration and this subset: `N=300`, seed `81985529216486895`, `K=3`, `k=8`, `p_local ∈ {0,0.5,1}`, using pool sizes `{1,2,4}` × chunk sizes `{64,128,256,512,1024}`;
- snapshot pause/resume and shuffled-slot parity using pool size `4` and chunk size `256`, rebuilding the graph and slot projection after restore.

CI must construct and exercise both 2-thread and 4-thread pools regardless of the runner's reported core count. Keep all existing M2 goldens and snapshot checks unchanged. The serial M3-01 path remains available as the oracle and default.

Run the host-maximum pool and the full chunk-size sweep over the full §9.7 matrix once in release mode and record the results in the performance report; these extended runs are not CI gates. Record the host maximum used.

### 10.7 Measurement protocol

Record the same host fields as M3-01. Use the reference host's thread counts `{1, 2, 4, 6}` and `N ∈ {10k, 50k, 100k}`, `k=8`, and `p_local ∈ {0, 0.5, 1}`. Measure two state horizons: the initial day-0 state and a state after 100 simulated days. Build both states, graphs, projections, runner contexts, and pools outside timed regions; restore an identical checkpoint for each paired sample. At each horizon, use two warmups, interleave serial and parallel samples per cell, and collect at least 11 timed samples. Each timed sample covers three consecutive days with scratch reuse; report median ± MAD per tick.

The primary results are per-Phase-6B stage times: structural validation/slot lookup, key generation, global sort, dependency-level build, and commit/evaluation/writeback/output reconstruction. Whole-tick time is secondary. Cross parallel measurements with the §10.6 chunk sizes. At both horizons and for each configuration, report the number of levels, largest level size, §9.9 conflict fraction and mean contention, plus keyed-entry and cross-partition-interaction volume so timing and available concurrency have workload context. Record the exact thread and chunk settings for every result.

The M3-01 profile measured Phase 6B at about 15% of a 100k tick for representative `k=8` cells. Amdahl's model therefore bounds whole-tick speedup near `1 / (1 - 0.15) ≈ 1.17×` even if Phase 6B became infinitely fast; this is a model/upper bound, not a measured result or target. M3-01 combined sequential commit with output assembly. For its 100k, `k=8` cells, that combined stage is not smaller than validation at `p_local=0`; M3-02 must report its finer stage measurements rather than assume commit is always the smallest Phase 6B component.

Report path: [`docs/performance/M3_02_PARALLEL_PHASE6B.md`](../performance/M3_02_PARALLEL_PHASE6B.md).

### 10.8 Research framing

M3-01 measured a conflict fraction of about `0.54` across the tested `p_local` values, including `p_local = 1`, where cross-partition interaction count is zero. M3-02 tests the hypothesis that parallel Phase 6B scaling is governed by endpoint contention and dependency-level structure, not settlement locality. Settlement partitioning is not a valid fast path merely because `p_local = 1`; it is valid only if every keyed entry is also endpoint-disjoint from entries that would execute concurrently. M3-02 does not include a settlement-partition fast path.

### 10.9 Explicit non-goals

- Parallelizing any phase other than M3 storage Phase 6B.
- Changing `run_m3_storage_days`, any M2 path, or the serial default.
- Changing the relationship graph, target semantics, RNG coordinates, global key, transfer rules, event schema, snapshot schema, or digests.
- Introducing a settlement-partition fast path based only on `p_local`.

### 10.10 Frozen decision register

| ID | Decision | Status | Frozen resolution |
|---|---|---|---|
| Q1 | Contention dial | **FROZEN** | Out of M3-02 scope. Record it as the optional stage after M3-02 in §8; any hub-biased or other contention-controlled graph mode requires its own contract amendment before implementation. |
| Q2 | Parallel M3 entry point | **FROZEN** | Expose `run_m3_storage_days_with_parallel_phase6b`, taking a caller-owned `rayon::ThreadPool` and a nonzero chunk size. Calling it is the explicit opt-in; `run_m3_storage_days` remains serial and unchanged. |

### 10.11 Freeze record

This freeze resolves the M3-02 scope, parallel entry point, CI matrix, dependency-level scratch lifetime, and optional contention-dial boundary. It authorizes the M3-02 implementation task to be requested; it does not start that task.

<a id="11-m3-02b-search-and-bookkeeping-reduction"></a>

## 11. M3-02b (now M3-03) Search and bookkeeping reduction

**Status: FROZEN**

### 11.1 Goal and semantic boundary

Reduce Phase 4 neighbor-search work and Phase 6B resolver bookkeeping on the serial M3 storage path while preserving bit-identical State, Metrics, and Events digests, identical Phase 4 `Intent` values, identical Phase 6B `SettlementTargetedResolution` values, and identical error behavior, including error precedence and partial output where applicable. `run_m3_storage_days` remains the serial oracle. The separate, explicit opt-in entry point is `run_m3_storage_days_with_search_reduction`; its caller-owned `M3SearchReductionRunContext` wraps `M3StorageRunContext` and the reusable M3-02b scratch. It does not replace or alter the oracle, the M3-02 parallel entry point, or any M2 path. New public API is limited to this entry point and context; existing public types and signatures remain unchanged.

Do not change simulation semantics, Phase 4 RNG coordinates or target-index formula, the Phase 6B resolution key or comparator, event payload/schema/order, snapshot schema, M3 digests, or M2 behavior. Transient indexes, masks, and sidecars remain runtime scratch and are excluded from state, snapshots, and digests.

The current path is implemented by [`phase4_primary_action_selection_storage_into`](../../crates/sim-model/src/decision.rs), `generate_intents_m3_storage_into`, `generate_intents_storage_impl`, `Phase4CandidateSource::target_agent`, and `phase4_select_candidate` in [`intents.rs`](../../crates/sim-model/src/intents.rs); `run_m3_storage_day_with_phase6b` in [`runner.rs`](../../crates/sim-model/src/runner.rs) orders the phases. Phase 6B validation, `slot_of` lookups, key staging, sorting, sequential commits, and output assembly are in `phase6b_targeted_resolution_storage_m3_serial_impl` in [`resolution.rs`](../../crates/sim-model/src/resolution.rs). `SettlementTargetedResolution` currently exposes three owned vectors: `zero_target`, `keyed_stream`, and `resolutions`.

This work is motivated by measured search/setup cost and the difference between the short-lived and sustained workloads. See [M3-01 Serial Baseline](../performance/M3_01_SERIAL_BASELINE.md), especially its per-phase profile; [M3-02 Parallel Phase 6B](../performance/M3_02_PARALLEL_PHASE6B.md), especially “Search and setup vs applied interaction cost,” “Outcome counts over days 1–3,” and “Timing-hook overhead”; [M3 Sustained Workload](../performance/M3_SUSTAINED_WORKLOAD.md), especially its workload and search/setup tables; and [M3 Interim Audit](../performance/M3_INTERIM_AUDIT.md), especially “Evidence against the central research question” and “Candidate next stages.” The existing reports show that search/setup was substantially larger than Applied-positive work, that GiveFood produced no targets in the initial M2-40 window, and that the selected food-supply configuration retains active interactions later in the run. These observations motivate measurement; they do not authorize semantic shortcuts.

### 11.2 Candidate optimizations

Evaluate each candidate independently. “Order” below means candidate order and/or Phase 6B semantic commit order; every accepted implementation must preserve both where applicable.

#### a. Skip a target search when a global eligibility predicate is empty

Before Phase 4 intent generation, compute once per day whether any storage row satisfies the live target predicate:

- GiveFood: `alive && health > 0.0 && food < starvation_threshold`.
- StealFood: `alive && health > 0.0 && food > 0.0`.

If the corresponding population-wide predicate is false, no graph neighbor can satisfy its subset predicate, so the neighbor scan may be skipped and the target remains `None`. This is a sufficient condition; if the only matching agent is the initiator, the precheck may fail to skip, but the ordinary neighbor filter still returns the same result. The precheck must not bypass choice validation, change the selected action, or alter partial-output/error behavior.

In the current code, `Phase4CandidateSource::target_agent` visits projected M3 neighbors and `phase4_select_candidate` returns `None` when `candidate_count == 0` before constructing the `RngCoordinate` or calling `coordinate_prng_f32`. The PRNG is coordinate-based, not a mutable stream. Intent generation reads `&SegmentedAgentStorage`; Phase 4 selection also reads storage through a shared reference, so no earlier intent mutates the predicates observed by later intents. Reusing the exact comparisons therefore preserves target results. For nonempty candidate sets, preserve the existing candidate order, coordinate, and index calculation unchanged.

**RNG and ordering:** no draw is made today for an empty set; this optimization must keep that behavior. It must not change the draws or ordering for nonempty sets. **API/type change:** none required. **Bit-exact status:** provable from the subset predicate, subject to exact daily recomputation and the parity gates in §11.4.

#### b. Per-day eligibility bitsets

Build transient storage-slot bitsets once per day for `alive && health > 0.0`, `food < starvation_threshold`, and `food > 0.0`. During projected-neighbor traversal, test the corresponding bits instead of loading and comparing the separate columns for each neighbor. Recompute after all earlier phases that can change the relevant columns; do not cache masks across days.

The source predicates are the exact comparisons in `Phase4CandidateSource::target_agent`. Since `M3Phase4SlotProjection` stores each row in ascending `AgentId` order, continue traversing that row in its existing order and use the bitsets only as filters. Do not construct candidates by unordered bitset iteration.

**RNG and ordering:** unchanged if the same projected row and predicates are used. **API/type change:** none required; masks are transient scratch. **Bit-exact status:** the predicate argument is direct, but performance is unproven; an `O(N)` mask build may cost more than neighbor filtering and must be measured.

#### c. Reuse Phase 4-resolved slots in Phase 6B

Phase 4 already resolves each initiator to a slot, and `M3Phase4SlotProjection` supplies neighbor slots used to select a target. The current Phase 6B serial resolver then performs `slot_of` for each targeted initiator and each non-null target while validating partitions. A runner-private sidecar could retain the selected initiator and target slots and carry them through `phase5_partition_intents_from_vec_with_scratch`, avoiding those repeated lookups for intents generated by that run.

Do not remove structural validation. The existing public `phase6b_targeted_resolution_storage_m3_with_scratch` must continue to validate arbitrary caller-supplied partitions in its current order and return the same errors. Reuse is limited to partitions produced by the new runner itself; preserve live `alive`, `health`, `group_id`, `food`, and action-specific checks at commit time. If implementing this requires adding slot fields to public `Intent`, `SettlementIntentPartition`, or another frozen result type, it is out of scope.

**RNG and ordering:** slot access alone changes neither RNG coordinates nor ordering; the global key, comparator, and sequential commit order remain fixed. **API/type change:** avoidable only if the sidecar stays private and remains aligned through Phase 5. **Bit-exact status:** conditional on proving that the sidecar covers exactly the internally generated intents while the public resolver retains full validation; otherwise out of scope.

#### d. Deterministic global-sort replacement

The current serial resolver uses stable `sort_by` with `compare_keyed_interactions`, ordering by `(ResolutionKey, target AgentId, initiator AgentId, action kind)`; equal tuples retain serial staging order. A deterministic radix sort may be evaluated only if it implements that exact lexicographic order and preserves the stable staging-order tie-break. A key-only sort is not equivalent when keys collide and is out of scope unless it also reproduces every comparator tie-break.

**RNG and ordering:** no coordinate changes, but a changed commit order can change live-food outcomes. Exact order, including equal-tuple stability, is mandatory. **API/type change:** none required. **Bit-exact status:** only a proven comparator-equivalent stable radix implementation is in scope. Key-only sorting is excluded because it cannot reproduce the frozen tie-breaks.

#### e. Out of scope: remove duplicate output storage

The resolver currently builds `resolutions` as `zero_target` followed by `keyed_stream`, while `SettlementTargetedResolution` exposes all three as independently owned `Vec<TargetedResolution>` fields. Moving scratch vectors into the result could avoid some clones, but it gives up their retained capacity and may add allocations on later days. Eliminating duplicate record storage requires changing the public result representation, such as shared backing storage or ranges into one vector.

**Decision:** excluded from M3-02b. **RNG and ordering:** no RNG effect; vector contents and order would still need to remain exact. **API/type change:** required for full de-duplication, which is prohibited here. A move-only variant changes scratch lifetime and is not retained as a candidate in this stage.

### 11.3 Measurement-first rule

Implement, validate, and measure one candidate at a time. Do not combine candidates until each has an individual parity result. Retain a candidate only when all §11.4 equality gates pass and the paired median improvement exceeds the MAD of the paired timing differences; report median and MAD for both the stage and uninstrumented whole tick. Record every rejected candidate and the reason, including no improvement beyond MAD, regression, API/type impact, or inability to prove parity. Timing hooks are secondary evidence and their overhead must be reported.

### 11.4 Correctness and CI gates

For every retained candidate, require exact Phase 4 intent and Phase 6B resolution equality, exact daily State/Metrics/Events digests against both `run_m3_storage_days` and the M3-00 reference, and identical error variants, precedence, and partial-output behavior for the same inputs. Run these gates on the existing Linux, Windows, and macOS CI matrix:

- the M3 golden configuration and the complete §9.7 100-day matrix against both the serial M3-01 oracle and M3-00 reference;
- snapshot pause/resume parity with graph and slot projection rebuilt, and shuffled-DenseSlot parity;
- unchanged existing M2 goldens and snapshot checks;
- the selected M3 sustained food-supply configuration for 200 days at `N=300` and `N=3,000` in CI, comparing the same three semantic paths. Use `K=3` at `N=300` and `K=15` at `N=3,000`, seed `81985529216486895`, replicate `7`, `k=8`, `p_local=0.5`, and `base_metabolic_cost=0.48`; keep every other M2-40 setting unchanged. Round-robin groups have sizes 100 and 200 respectively. Under §2.2, local/cross capacities are 99/200 at `N=300` and 199/2,800 at `N=3,000`, so both exceed `k=8` for `p_local=0.5`.

**Expected added CI work (estimate, not a measurement):** for the single sustained `p_local=0.5` cell at each population, the added `N × days` is `(300 + 3,000) × 200 = 660,000` agent-days per path. The §9.7 matrix is `54 × 300 × 100 + 3 × 1,000 × 100 + 3 × 3,000 × 100 = 2,820,000` agent-days per path. Under a roughly linear population-cost assumption this is about a **23% increase to the §9.7 differential portion**, not 23% of the whole workspace suite. Actual CI wall time is unknown and must be recorded on the slowest supported OS; this estimate is not a substitute for that measurement.

The added sustained-gate test wall-time target is **at most two minutes in debug mode on the development host**. Record its actual test time in the report. After an authorized push, record the observed CI time there; this contract task does not authorize a push.

### 11.5 Measurement protocol

Use the M2-40 configuration at day 0 and the accepted M3 sustained configuration at day 150, for `N ∈ {10,000, 100,000}`, `p_local ∈ {0, 0.5, 1}`, and `k=8`. Record host details using the M3-01/M3-02 report format. The primary result is the uninstrumented whole-tick median ± MAD from paired samples with identical checkpoints and scratch reuse. Per-stage profiles are secondary; report hook overhead and do not treat hook-derived per-entry division as exact.

For each baseline and retained candidate, report search/setup versus Applied-positive work before and after: search/setup is Phase 4 neighbor filtering/target draw plus Phase 6B validation/slot lookup, key generation, and sort; work is the Phase 6B interval for entries ending `Applied` with `amount > 0`. Preserve keyed, Some/None, and outcome counts alongside the timings so changes are interpreted against the workload. Use the existing M3 measurement cadence where applicable: two warmups, at least eleven timed paired samples, three consecutive days per sample, and retained scratch. This follows the protocols and caveats recorded in the cited M3 reports.

### 11.6 Bench output for a future viewer

The new benchmark writes per-day JSON records under `target/` only; do not commit the generated files and do not include file output in timed regions. Each record includes configuration and day, alive count, action counts, GiveFood/StealFood `Some` and `None` counts, Phase 6B outcome counts, keyed-entry count, level count and maximum level size where a level schedule is measured (`null` for the serial schedule), and stage times. Collect measurements through the bench or result-neutral timing APIs; writing JSON must not mutate simulation inputs or outputs.

### 11.7 Report

Record methods, retained and rejected candidates, per-stage and whole-tick medians ± MAD, search/setup versus Applied-positive work, workload counts, hook overhead, parity results, and actual added CI wall time in [`docs/performance/M3_02B_SEARCH_REDUCTION.md`](../performance/M3_02B_SEARCH_REDUCTION.md). Label estimates and inferences separately from measured facts.

### 11.8 Remaining open questions

None. The debug runtime measurement and post-push CI timing are required implementation/report evidence, not unresolved contract decisions.

### 11.9 Frozen decision register

| ID | Decision | Status | Frozen resolution |
|---|---|---|---|
| OQ-1 | Sustained CI configuration | **FROZEN** | Use `N=300, K=3` and `N=3,000, K=15`, seed `81985529216486895`, replicate `7`, `k=8`, `p_local=0.5`, `base_metabolic_cost=0.48`, and 200 days. Both group layouts pass §2.2: local/cross capacities are 99/200 and 199/2,800, respectively, each at least `k=8`. |
| OQ-2 | Added test and CI runtime | **FROZEN** | The sustained-gate debug test target is at most two minutes on the development host. Record actual debug test time in the report and, after an authorized push, observed CI time. Runtime measurements do not relax any parity gate. |
| OQ-3 | Phase 4 slot sidecar | **FROZEN** | Candidate (c) is allowed only as a private sidecar created by a private Phase 5 wrapper that returns partitions and aligned slot vectors together. In debug builds, assert every non-null sidecar slot equals `storage.slot_of(agent_id)`. Keep the public M3 storage resolver's complete validation unchanged. If this cannot be implemented without changing an existing public type, reject candidate (c). |
| D-1 | Explicit opt-in API | **FROZEN** | Add `run_m3_storage_days_with_search_reduction` and `M3SearchReductionRunContext`, wrapping `M3StorageRunContext` plus caller-owned reusable scratch. Preserve existing public type layouts and signatures; the new entry point is serial and opt-in. |
| D-2 | Output-copy candidate | **FROZEN** | Candidate (e) is out of scope because complete de-duplication requires a public result-type change; do not implement the move-based scratch-capacity trade-off in M3-02b. |
| D-3 | Global sort candidate | **FROZEN** | Candidate (d) is limited to a stable radix sort proven equivalent to the full frozen comparator and serial staging-order tie-break. Key-only sorting is out of scope. |

### 11.10 Freeze record

This freeze resolves the sustained CI configuration, runtime target, private Phase 5 slot-sidecar constraints, explicit opt-in API, and candidate boundaries. It authorizes the M3-02b implementation task to proceed; it does not itself implement the optimizations.

<a id="12-m3-k-settlement-lookup-indexing"></a>

## 12. M3-K (now M3-06) Settlement lookup indexing

**Status: FROZEN**

### 12.1 Goal and measured baseline

Replace repeated GroupId-keyed scans on the shared M2/M3 storage path with a transient per-call settlement index. Preserve State, Metrics, and Events digests, intents, resolutions, error values and precedence, and all canonical ordering. Public function signatures and public types remain unchanged. The AoS reference implementations remain available as semantic oracles; this optimization applies only to storage execution.

[M3 Scale Diagnosis](../performance/M3_SCALE_DIAGNOSIS.md) records these day-0 M3 storage counts at `N=1,000,000`:

| Call site | K=500 | K=5,000 |
|---|---:|---:|
| P3 scarcity: per-agent binary-search comparisons | 9,980,000 | 13,362,000 |
| P6A settlement `.find`: calls / elements | 1,000 / 250,500 | 10,000 / 25,005,000 |
| P6B settlement `.any`: calls / elements | 500 / 125,250 | 5,000 / 12,502,500 |
| P7 settlement `.find`: calls / elements | 1,000 / 250,500 | 10,000 / 25,005,000 |
| P10 duplicate-settlement prefix `.any`: calls / elements | 500 / 124,750 | 5,000 / 12,497,500 |
| P11 event counter GroupId `.find`: calls / elements | 1,000 / 250,000 | 10,000 / 25,000,000 |

The P3 M3 storage path already binary-searches its source-order-preserving scarcity index; it is not a linear scan. The other listed resolver scans grow with partitions or settlements. P11 searches its event-counter vector, not the settlement slice, but is included because it performs the same repeated GroupId lookup pattern.

### 12.2 Required index behavior

Use a private, transient index built once per phase call from the supplied settlement slice. The index provides `GroupId -> source index` lookup and is not stored in `WorldState`, `SegmentedAgentStorage`, snapshots, hashes, or events. No public function signature or public result/configuration type changes.

- When the input IDs are unique and exactly dense in `0..settlements.len()`, use a direct-index table. Verify density and uniqueness from the actual input; never infer them from `K` or the GroupId type alone.
- For sparse IDs or duplicate IDs, use a sorted `(GroupId, source_index)` fallback and lower-bound lookup. The dense direct-index path accepts any source order. Order equal IDs by source index so fallback lookup returns the same first match as the old source-order `.find`.
- Index construction itself returns no semantic error. Each caller performs lookups at its existing validation point, in its existing traversal order, so index preparation cannot move an error ahead of an earlier validation.
- Do not use hash-table iteration to order settlements, participants, resolutions, or events. A hash table may be used only for keyed lookup when the output order continues to come from the existing ordered vectors.

Apply this lookup behavior at the following storage call sites:

1. **P3 scarcity** — in `Phase3ScarcityScratch::prepare` / `lookup` in `features.rs`, retain the current source-order scarcity calculation and first-match behavior for duplicate GroupIds, but resolve each per-agent GroupId through the transient index. Preserve every existing f32 expression and its source-order evaluation.
2. **P6A work** — in `phase6a_work_resolution_storage` in `resolution.rs`, replace the planning and commit settlement `.find` calls. The same lookup must address the same first source settlement.
3. **P6B validation** — in the M2 baseline and ordered storage resolvers and the M3 serial and parallel storage resolvers in `resolution.rs`, replace settlement-existence scans with index membership checks. Keep configuration validation, partition traversal, duplicate-partition checks, and intent validation in their current order.
4. **P7 market** — in `phase7_market_clearance_storage` in `resolution.rs`, replace both the planning lookup and commit lookup while preserving partition and participant ordering.
5. **P10 duplicate validation** — in `phase10_observe_storage_with_scratch` in `metrics.rs`, precompute which source positions are duplicates, then check each position inside the existing settlement loop. Return the duplicate at the same source position and retain the current order relative to negative-treasury validation.
6. **P11 event grouping** — in the M2 and M3 storage runner event assembly in `runner.rs`, replace repeated linear lookup in `phase6_counts` with a transient GroupId-to-counter-slot index. Continue emitting and updating the existing vector in the same Work-then-targeted resolution order; map iteration must not produce events. Leave M0 and M3-00 AoS reference event assembly unchanged.

### 12.3 Error and semantic parity

The index is an access optimization only. It must preserve source-order first-match behavior for duplicate settlement IDs and the exact point at which a missing GroupId is reported.

- P6A still checks a partition's settlement before its resource and intents, then commits settlements in the existing plan order.
- P6B still validates theft probability first, then partitions in caller order; duplicate-partition and missing-settlement precedence remains unchanged. Parallel structural validation must return the same earliest serial error.
- P7 still validates price/tax and duplicate partitions before processing settlement lookups in sorted partition order; it must select the same source settlement on duplicates.
- P10 still validates every agent before settlements, then visits settlements in source order and reports the same first duplicate or negative treasury error.
- P11 keeps the existing event vector order, payloads, partition keys, and local-sequence offsets.

Do not change M0 or M3-00 reference behavior. M2 and M3 storage paths must remain bit-identical, including M2 and M3 State/Metrics/Events goldens, exact intents and resolutions, snapshot bytes, and error variants/precedence.

### 12.4 Correctness gates

Run the full existing workspace suite unchanged, including M2 goldens and snapshot tests, M3 golden tests, the complete §9.7 differential matrix, M3 parallel Phase 6B tests, and M3 search-reduction tests.

Add a new randomized test for the private index that compares every indexed lookup with the old `settlements.iter().position(...)` result and duplicate-at-source result with the old prefix `.any(...)` rule. Include dense, sparse, shuffled, duplicate, and missing GroupIds. Also exercise public phase errors against the old linear lookup oracle, including multiple failures in one input, and assert exact error value and precedence. Keep test randomness deterministic and do not weaken or edit existing tests.

### 12.5 Measurement gate

Rerun the M3 Scale Diagnosis sampling method on the same host and equilibrium configuration before and after indexing:

- fixed `N=250,000`, `K ∈ {250, 500, 1,250, 2,500, 5,000}`;
- fixed `K=500` and `K=N/200` for `N ∈ {100,000, 250,000, 500,000, 1,000,000}`;
- `k=8`, `p_local=0.5`, day 0, serial, two warmups, seven samples, three consecutive days with scratch reuse.

Report per-phase median ± MAD, per-phase alpha vs N for the fixed-K and `K=N/200` series, per-phase time vs K for the fixed-N series, lookup work before/after, and a paired whole-tick comparison. Report results in [`docs/performance/M3_SETTLEMENT_INDEX.md`](../performance/M3_SETTLEMENT_INDEX.md). Keep the original M3 scale-limit and diagnosis measurements unchanged.

### 12.6 Frozen decision register

| ID | Decision | Status | Frozen resolution |
|---|---|---|---|
| Q1 | Index construction and fast path | **FROZEN** | Build a private transient index once per phase call. Use a direct-index table only after proving GroupIds are unique and exactly dense in `0..K`; otherwise use a sorted `(GroupId, source_index)` index with lower-bound lookup. Duplicate IDs resolve to the smallest source index. |
| Q2 | Phase coverage and public surface | **FROZEN** | Apply the index to P3 scarcity, P6A, every M2/M3 storage P6B resolver, P7, P10 duplicate validation, and M2/M3 storage P11 event grouping as listed in §12.2. Keep all public signatures and types unchanged. Do not change M0 or M3-00 AoS reference paths. |
| Q3 | Error and output parity | **FROZEN** | Index construction raises no semantic error. Perform each lookup at the existing validation point and keep source traversal, first-match behavior, error values/precedence, event sequence, resolution order, and f32 calculation order unchanged. P10 checks duplicate markers within its existing source-order loop. |
| Q4 | Correctness gates | **FROZEN** | Run the unchanged full workspace suite and add randomized tests comparing indexed lookups and public phase errors with the old linear oracle across dense, sparse, shuffled, duplicate, and missing IDs. Preserve M2/M3 goldens, snapshots, and all existing differential gates. |
| Q5 | Measurement gate | **FROZEN** | Rerun the fixed-N, fixed-K, and `K=N/200` series in §12.5, reporting per-phase median ± MAD, alpha vs N, lookup work, and paired whole-tick timing in `docs/performance/M3_SETTLEMENT_INDEX.md`. |

### 12.7 Freeze record

This freeze resolves the index representation, storage call-site scope, error-parity requirements, test matrix, and measurement protocol. It authorizes the requested M3-K implementation steps; it does not itself change simulation code.

## 13. M3-08 (formerly M3-P) Population dynamics

**Status: FROZEN**

This section freezes the population-dynamics semantics for profile `m3p`. It leaves M2 and the existing M3 profile unchanged. The implementation-stage plan below is not implementation authorization; this freeze authorizes the M3-08 implementation task to be requested, but does not start it.

### 13.1 Verified current-code baseline

- `initialize_world` in `crates/sim-model/src/initialization.rs` assigns initial IDs as `AgentId(i as u32)`, groups round-robin as `GroupId(i % settlement_count)`, and `birth_day = 0`. `AgentId` is a `u32` newtype in `crates/sim-core/src/primitives.rs`.
- `SimulationProfile` and `SimConfig::validate` in `crates/sim-model/src/config.rs` currently support `m2` and `m3`; `m3p` and `[population]` are additions specified by this section.
- `phase9_mortality_commitment` in `crates/sim-model/src/phases.rs` and `phase9_mortality_commitment_storage` only change `alive` from true to false; neither removes agents or compacts storage. The storage resolver leaves `AgentId`, `DenseSlot`, and physical slot positions untouched.
- Current simulation phases do not append to or remove from an existing `WorldState.agents` vector; initialization and snapshot decoding construct the agent vectors. `SegmentedAgentStorage::push` in `crates/sim-model/src/storage.rs` appends every column and updates the `AgentId -> slot` map; `rebuild_slot_map` reconstructs that map after storage rebuilding.
- `RelationshipGraph::generate` in `crates/sim-model/src/relationship_graph.rs` builds a canonical fixed-degree CSR from membership input. `M3StorageRunContext::new` in `crates/sim-model/src/runner.rs` builds it once from current storage memberships and builds the Phase 4 slot projection; `generate_intents_m3_into` and `generate_intents_m3_storage_into` in `crates/sim-model/src/intents.rs` filter dead or unhealthy neighbors at use time. The current M3 runner does not add nodes or mutate graph edges during a run.
- `encode_snapshot` and `decode_snapshot` in `crates/sim-model/src/snapshot.rs` use snapshot schema v1, canonicalize agents by `AgentId`, and store `alive` and `birth_day`. The schema stores neither a graph nor an AgentId allocation cursor. M3 recreates its static graph when constructing the run context; that cannot reconstruct a graph that has changed through births or edge cleanup.
- `canonical_m3_state_bytes` and `canonical_m3_state_bytes_from_storage` in `crates/sim-model/src/hashing.rs` append the current CSR hash to the canonical M1 State fields. Therefore a graph update changes the M3 State digest even if all agent fields are unchanged.
- The current graph generator rejects population counts `N >= 2^24` in `relationship_graph.rs::validate_population_count`, because its rank-selection formula uses an exactly representable `f32` population bound. Births must retain a checked bound for the population represented in the graph.

### 13.2 Birth timing

Phase 9b runs immediately after Phase 9 mortality on day `D`. It creates births on day `D`; newborns are included in that day's Phase 10 metrics, Phase 11 events, and any day-D snapshot, but do not act until day `D+1`. Birth candidates and commits follow ascending parent `AgentId`; allocated child IDs and Birth event order are therefore ascending as well. The end-of-day placement preserves the existing order of Phases 1–9 for the current population.

### 13.3 Birth eligibility, probability, resources, and traits

The parent is eligible exactly when `alive == true`, `health >= birth_min_health`, and `food >= birth_food_threshold`. Evaluate exactly one stateless coordinate draw for every eligible parent:

```text
(master_seed, replicate_id, day, phase = 9, subsystem = 7, parent AgentId, draw = 0)
```

The parent produces at most one child on that day iff `u < birth_probability`. Evaluate draws for all eligible parents before applying the cap. `births_max_per_day = 0` means unlimited; otherwise admit successful parents in ascending `AgentId` order up to the cap.

For each admitted birth, debit `birth_food_cost` from the parent and assign that amount to the child; set child wealth to `0`, health to `world.initial_health`, `birth_day` to `D`, and `alive` to true. Copy the parent's traits exactly. Plan all births and validate identifiers, live-population capacity, and parent resources before committing any debit, child, graph row, event, or metric. Any validation failure rejects the birth batch atomically.

The `profile` field must explicitly be `m3p`; if absent it retains the existing `m2` default. The `[population]` fields are all required for `m3p` and have no defaults: `birth_min_health: f32`, `birth_food_threshold: f32`, `birth_probability: f32`, `birth_food_cost: f32`, `births_max_per_day: u32`, and `compaction_interval_days: u32`. Validate `birth_min_health` as finite and in `[0, 1]`; `birth_food_threshold` and `birth_food_cost` as finite and nonnegative; `birth_food_threshold >= birth_food_cost`; and `birth_probability` as finite and in `[0, 1]`. Zero `births_max_per_day` means unlimited. `compaction_interval_days` must be >= 1. The existing M3 graph settings `relationship_degree_k` and `p_local` remain required and retain §2.7 ranges.

### 13.4 AgentId allocation and population bound

`next_agent_id: u64` is part of `m3p` state and is initialized to the initial population `N`. A child receives the current cursor value, then the cursor increments. IDs are never reused. The cursor may represent the exhausted sentinel `u32::MAX + 1`; a birth requiring an ID when `next_agent_id > u32::MAX` returns a checked error before commit. Invalid cursors above that sentinel are rejected.

Before committing a birth batch, checked arithmetic must prove that live population after births remains strictly below `2^24`; otherwise reject the entire batch atomically. The strict initial-graph capacity validation in §2.2 remains unchanged. This live-population limit also bounds the candidate pools used by m3p dynamic edge sampling.

### 13.5 Newborn group assignment

Each newborn joins its parent's current `GroupId`. This uses no extra RNG draw, is independent of physical slot layout, and makes the group's identity part of the child's canonical state and snapshot.

### 13.6 Graph attachment and dead endpoints

Newborn rows are directed. When `k > 0`, a newborn has exactly `k` requested outgoing slots: edge 0 targets its parent; edge slots 1 through `k-1` use the §2.2 locality sampler over live agents, excluding the child, parent, and already chosen neighbors. The first locality/target draws are `2` and `3`; each subsequent requested edge advances by two draw indices. For `k = 0`, the row is empty. Existing rows do not change at birth.

If a selected locality pool has no remaining candidate, omit that requested edge and continue to the next requested edge with its assigned coordinates. Do not switch to the other locality pool or duplicate an edge. The resulting row may therefore be shorter than `k`.

At compaction, process surviving source rows in ascending source `AgentId`, then their pre-update edges in ascending neighbor `AgentId` order. For each edge targeting a dead agent being removed, attempt a replacement using the §2.2 sampler over live agents, excluding the source and all other already chosen row neighbors. Use `day`, phase `9`, subsystem `7`, source `AgentId`, and draw indices starting at `1,000,000 + 2 * position`, where `position` is the zero-based edge position in the pre-update canonical row. If the selected pool has no eligible replacement, delete the edge. Do not fall back to the other pool. Sort each updated row by `AgentId` after processing. Existing edges not targeting a removed agent are retained.

Short rows are permitted only for m3p dynamic updates: newborn rows and rows shortened during RQ-7 compaction. Initial graph generation remains subject to §2.2's strict capacity validation and never reduces `k`.

Every m3p CSR edge target must exist in canonical m3p state, whether alive or a not-yet-compacted dead record. Compaction replaces or deletes every incoming edge to a removed agent before removing that agent and its source row, so no dangling target remains. Snapshot v2 restore validates and rejects any edge target absent from canonical state.

Dropping an edge to a dead neighbor is behavior-neutral for Phase 4 targeting compared with retaining it and filtering it as dead: `generate_intents_m3_into` and `generate_intents_m3_storage_into` in `crates/sim-model/src/intents.rs` exclude dead neighbors, so the eligible candidate lists are identical. This equivalence concerns candidate/intent behavior; CSR content and its hash, and thus the m3p State digest, change when compaction drops an edge.

Recompute the CSR hash after any birth or topology update, including replacement, edge deletion, and source-row removal. The hash remains canonical over ascending source IDs and ascending neighbors.

### 13.7 Death cleanup and storage layout

Compaction runs inside Phase 9b after births on each day `D` for which `D % compaction_interval_days == 0`. Remove all dead agent records and their graph source rows. Process incoming dead-target edges by §13.6: replace when possible, otherwise drop. `Cleanup.removed` counts the removed agents; `replaced_edges` and `dropped_edges` count incoming edges to those agents that were respectively replaced or deleted. Edges in source rows removed with dead agents are not counted as incoming replacements or drops.

Rebuild storage in ascending `AgentId` order, then rebuild the `AgentId -> slot` map and Phase 4 graph projection. Physical slot order remains non-semantic. Monotonic IDs, appending children in ascending parent/child ID order, and AgentId-order compaction keep storage AgentId-sorted. This preserves the sequential cursor lookup path in `phase4_primary_action_selection_storage_into` (`crates/sim-model/src/decision.rs`) when Phase 4 inputs are AgentId-sorted.

**M3-07 locality evidence:** `docs/performance/M3_07_SCALE_RESIDUAL.md`, “Normal versus reversed slot layout,” reports a P4 action-selection reversed-slot slowdown of `2.3734x` at `N=1M` (reliable under its MAD rule) and a P4 intent/projection point estimate of `3.6011x` (unreliable under the MAD rule). Its “Observed point changes and working-set landmarks” section attributes gather/translation pressure only as an inference. The chosen append/compaction order preserves sorted slots; measured M3-07 results motivate this choice but do not guarantee a particular speedup for m3p.

### 13.8 Snapshot and restore

Profile `m3p` uses snapshot schema v2. Store the canonical CSR and `next_agent_id` along with the canonical agent and settlement state. Restore must validate sorted unique AgentIds; CSR source IDs matching the canonical state AgentIds exactly; row offsets; sorted neighbor rows; every edge target's existence in canonical state; the recomputed CSR hash; and the allocator cursor invariant. Allow `next_agent_id == u32::MAX + 1` only as the exhausted sentinel; reject larger values. Reject malformed v2 data, including dangling edge targets; do not regenerate a mutated graph from the seed. M2 and `m3` continue to use the existing schema v1 and retain byte-identical snapshot behavior.

### 13.9 Profile, metrics, and events compatibility

Population dynamics is selected only by `profile = "m3p"` with the required `[population]` fields in §13.3. Existing `m2` and `m3` configurations, paths, State/Metrics/Events digests, goldens, and v1 snapshot bytes remain unchanged. The `m3p` State digest uses domain `SIMCIV_M3P_STATE_V1`, the canonical M1 State fields, the canonical CSR hash, then `next_agent_id` as little-endian `u64`. The m3p Metrics digest uses domain `SIMCIV_M3P_METRICS_V1`, the existing M3 metrics field order followed by `births`, `removed`, `replaced_edges`, `live_population`, and `next_agent_id`, each as little-endian `u64`. The m3p Events digest uses domain `SIMCIV_M3P_EVENTS_V1` and the canonical event encoding.

After the Phase 9 mortality events, stage Birth events in ascending child `AgentId` order as `Birth { parent, child, group }`. On each scheduled compaction day, follow those Birth events with one `Cleanup { removed, replaced_edges, dropped_edges }` event, including when all three counts are zero. The three Cleanup values and the daily births count are `u64`. The M3P daily metrics append the listed fields in that order; no M2 or current M3 metric/event schema is changed.

### 13.10 Implementation-stage plan

M3-08 proceeds in three implementation stages, mirroring the M3-00 reference-first and M3-01 optimized-path sequence:

1. **WorldState reference path.** Add the m3p population lifecycle to a separate single-threaded reference entry point. Freeze and validate Phase 9b ordering, birth/cleanup events, graph mutations, allocator state, snapshot v2, and M3P digests. Keep M3-00 and all M2/M3 paths unchanged.
2. **Segmented SoA storage path.** Port the frozen m3p reference to a separate storage-authority entry point. Compare exact per-day intents, resolutions, errors, State/Metrics/Events digests, and snapshots against the WorldState reference, including slot permutations and compaction rebuilds.
3. **Measurement.** Find and commit the sustained m3p configuration, run the 1,000-day acceptance workload, and measure birth, edge repair, and compaction costs. Performance results do not change semantics or the frozen reference.

### 13.11 Acceptance gates

1. Repeated m3p runs produce exact State, Metrics, Events, intent, resolution, and error parity.
2. Snapshot pause/resume parity holds at several boundaries, including a compaction day; v2 restore validates allocator and CSR invariants. Shuffled physical slots produce identical canonical outputs.
3. ID exhaustion, live-population capacity, resource debit, edge capacity, and atomic birth-batch failure are checked. Dynamic short rows, edge replacement/drop, and cleanup event counts have direct tests.
4. Existing M2 and `m3` goldens and snapshot bytes remain unchanged. m3p has separate committed State, Metrics, and Events goldens under the M3P domains.
5. Commit a sustained m3p configuration with `N=10,000` whose live population remains within ±10% of its initial value on every day from 100 through 1,000. CI runs that configuration at `N=1,000` for 1,000 days with the same tolerance.

### 13.12 Frozen decision register

| ID | Decision | Status | Frozen resolution |
|---|---|---|---|
| RQ-1 | Birth timing | **FROZEN** | Phase 9b runs immediately after Phase 9 on day D. Newborns appear in day-D Phase 10 metrics, Phase 11 events, and snapshots; they act starting day D+1. |
| RQ-2 | Birth eligibility, probability, and cap | **FROZEN** | Eligible iff `alive && health >= birth_min_health && food >= birth_food_threshold`. Draw once per eligible parent at `(seed, replicate, day, phase 9, subsystem 7, parent AgentId, draw 0)`; birth iff `u < birth_probability`. At most one child per parent/day. A nonzero `births_max_per_day` admits successes by ascending parent ID; zero is unlimited. All m3p population config fields are required without defaults and validated as in §13.3. |
| RQ-3 | Child resources and initial state | **FROZEN** | Debit `birth_food_cost` from the parent and assign it to child food; child wealth is zero, health is `world.initial_health`, birth day is D, and alive is true. Require `birth_food_threshold >= birth_food_cost`. Plan and validate the full birth batch before commit; errors do not partially mutate state. |
| RQ-4 | Trait inheritance | **FROZEN** | Copy all parent traits exactly; no trait perturbation or trait RNG draw. |
| RQ-5 | AgentId allocation and cap | **FROZEN** | Store `next_agent_id: u64` in m3p state, initialize it to N, allocate then increment, and never reuse IDs. The exhausted sentinel is `u32::MAX + 1`; no ID is allocated from it. Reject invalid cursor values and atomically reject a batch whose live population would reach `2^24`. |
| RQ-6 | Newborn group and graph row | **FROZEN** | Join the parent's current group. For k>0, make edge 0 the child-to-parent edge and sample up to k-1 remaining edges from live agents with §2.2 locality and exclusions using phase 9/subsystem 7 and draws starting at 2. For k=0, create no edges. If a selected pool is empty, omit that edge slot without fallback; m3p dynamic rows may be shorter than k. Existing rows are unchanged at birth. Initial graph generation keeps strict §2.2 capacity rules. |
| RQ-7 | Dead-endpoint repair | **FROZEN** | Repair only during compaction, in ascending source AgentId and pre-update row edge order. Sample with phase 9/subsystem 7 and draws starting at `1,000,000 + 2 * position`. Replace dead targets when possible; otherwise delete the edge so the row becomes shorter. Sort rows after updates. |
| RQ-8 | Cleanup and graph closure | **FROZEN** | Dead agents remain as `alive=false` state and graph records until compaction. Compaction removes them and their source rows; incoming edges are replaced or dropped. Every CSR edge target must exist in canonical state. Dropping an edge to a dead target preserves Phase 4 candidate/intent behavior because dead neighbors were already filtered, although the CSR hash and m3p State digest change. Recompute the CSR hash on births and every graph mutation. |
| RQ-9 | Compaction and slots | **FROZEN** | Require `compaction_interval_days >= 1`; compact inside Phase 9b after births whenever `day % C == 0`. Rebuild storage in ascending AgentId order, then the slot map and Phase 4 projection. Physical order is non-semantic. Monotonic append and sorted compaction retain AgentId-sorted slots and the Phase 4 cursor path. |
| RQ-10 | Profile, snapshot, and digest domains | **FROZEN** | Select population dynamics with `profile = "m3p"` and required `[population]` fields. Use snapshot schema v2 only for m3p, storing canonical CSR and `next_agent_id`; reject dangling graph targets on restore. Use `SIMCIV_M3P_STATE_V1`, `SIMCIV_M3P_METRICS_V1`, and `SIMCIV_M3P_EVENTS_V1`. Keep m2/m3 goldens and v1 snapshot bytes unchanged. |
| RQ-11 | Population events and metrics | **FROZEN** | After mortality events, emit Birth events by ascending child ID, then one Cleanup event on each compaction day with `{ removed, replaced_edges, dropped_edges }`. The m3p metric appends `births`, `removed`, `replaced_edges`, `live_population`, and `next_agent_id` in that order as `u64` fields. |
| RQ-12 | Sustained-population gate | **FROZEN** | Commit an m3p configuration whose N=10,000 live population remains within ±10% of its initial value on every day 100–1,000. CI runs N=1,000 for 1,000 days with the same tolerance, plus replay and snapshot resume at multiple boundaries including compaction. |
| RQ-13 | Compaction performance tuning | **FROZEN — NON-BLOCKING** | Measure compaction cadence and storage locality after implementation. This measurement does not block semantic implementation and cannot change the fixed `day % C == 0` rule without a contract amendment. |

### 13.13 Freeze record

This freeze records the m3p birth schedule and rule, checked monotonic identity allocation, graph attachment and cleanup behavior, storage compaction order, snapshot v2 validation, separate digest/event/metric schemas, implementation stages, and acceptance gates. It resolves the §13 open questions. The freeze authorizes the M3-08 implementation task to be requested; it does not start implementation.
