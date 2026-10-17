# M3 Interaction Contract

**Status: FROZEN**

**Scope: Frozen M3-00 workload and single-threaded semantic reference**

This document freezes the M3-00 workload and single-threaded semantic reference. Later M3 stages (M3-01 onward) remain out of scope and will extend this document.

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

### M3-01 — Optimized path and serial baseline

Port M3 to the Segmented SoA path with index-based neighbor and target lookup. Do not add parallelism.

**Gate:** exact digest parity with the M3-00 reference, including larger differential runs with hundreds to thousands of agents beyond the small golden. Measure per-phase time across `p_local ∈ {0, 0.25, 0.5, 0.75, 1}` and `k ∈ {4, 8, 16}`; use `p_local = 1` as the baseline for the global stream's cost. Also record conflict density: the fraction of keyed Phase 6B entries that share an initiator or target with another entry on the same day, as a predictor for M3-02.

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
