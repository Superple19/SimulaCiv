# M3 Interaction Contract

**Status: FROZEN**

**Scope: M3-00 semantic reference (§1–§7) and M3-01 optimized serial contract (§9)**

This document freezes M3-00 (§1–§7) and M3-01 (§9); later M3 stages are planned in §8.

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

### M3-00 — Reference workload — COMPLETE

Single-threaded reference with the relationship graph, neighbor targeting, global Phase 6B resolution, and separate M3 goldens.

### M3-01 — Optimized path and serial baseline — COMPLETE

Port M3 to the Segmented SoA path with index-based neighbor and target lookup, serially. The golden, §9.7 differential, snapshot-resume, shuffled-slot, and release-differential gates passed. The serial baseline and conflict-density results are recorded in [M3-01 Serial Baseline](../performance/M3_01_SERIAL_BASELINE.md).

### M3-02 — Deterministic parallel Phase 6B

The primary approach is conflict-based scheduling (the Deterministic Reservations family): entries touching disjoint agents may run in parallel; entries sharing an agent retain global-stream order. A settlement-partition fast path is valid only when every interaction is settlement-local (`p_local = 1`). Serial remains the default; parallel execution is an explicit opt-in.

**Gate:** bit-identical digests to the reference. Report speedup versus `p_local` and where partition-based parallelism stops paying off, including the cause.

### M3-03 — Spatial migration

Settlement membership changes and partitions change daily. The round-robin fast path no longer applies.

### M3-04 — Dynamic topology

Edges are created and removed. The graph must be persisted in snapshots starting in this stage.

### M3-05 — Propagation

Model multi-hop spread over the graph within one tick and define intra-tick ordering.

### M3 audit and graduation

Report in the M2-40 format, answer the central research question, and decide whether a new architecture (M4) is needed.

Stopping after M3-02 still provides a complete answer to the central research question. M3-03 onward deepens that answer.

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

**Status: DRAFT — NOT FROZEN**

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

An alternative round-based Deterministic Reservations schedule may be measured for comparison only. It must reserve endpoints in global-stream order within each round and may execute only endpoint-disjoint entries concurrently. Dependency levels remain the primary method for M3-02; the alternative does not change serial semantics or become an implicit fallback.

### 10.5 Output and event order

Reconstruct one `SettlementTargetedResolution` per initiator group in ascending `GroupId`. Each group contains `zero_target` entries first in ascending initiator `AgentId` order, followed by that group's subsequence of keyed results in the global keyed-stream order. `resolutions` is `zero_target` followed by `keyed_stream`, exactly as in §2.4 and §9.4. The reconstructed values must be independent of worker count, chunk size, completion order, and level assignment. Continue to use the existing `events_from_targeted_resolution` converter and Phase 6A local-sequence offsets so Phase 6B event order and payload remain unchanged.

### 10.6 Correctness and parity gates

Require bit-identical State, Metrics, and Events digest sequences against both the serial M3-01 path and the M3-00 reference for:

- the committed M3 golden configuration;
- every configuration in the full §9.7 100-day differential matrix (`N=300`, the three listed seeds, `K ∈ {3,10}`, `k ∈ {4,8,16}`, `p_local ∈ {0,0.5,1}`, plus `N ∈ {1,000,3,000}` with their §9.7 `K`, seed, `k`, and locality settings);
- snapshot pause/resume, rebuilding the graph and slot projection after restore;
- equivalent logical states with shuffled or permuted storage slots.

Run parity with Rayon pool sizes `{1, 2, 4}` and the host maximum, and with chunk sizes `{64, 128, 256, 512, 1024}`. Record the host maximum used. CI must explicitly construct and exercise at least 2-thread and 4-thread pools; it must not skip multi-thread coverage because the runner reports a low core count. Keep all existing M2 goldens and snapshot checks unchanged. The serial M3-01 path remains available as the oracle and default.

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

### 10.10 Open questions

| ID | Question | Status |
|---|---|---|
| Q1 | Should a later M3 stage add a contention dial, such as a hub-biased graph mode, to test scheduling across different endpoint-contention levels? This would change graph semantics and requires its own contract amendment before implementation. | **NON-BLOCKING** |
| Q2 | What exact Rust identifier should the separate public opt-in parallel runner use? The required boundary is fixed here (M3 storage Phase 6B only, caller-owned pool, explicit call); the spelling can be selected during implementation without changing semantics. | **NON-BLOCKING** |
