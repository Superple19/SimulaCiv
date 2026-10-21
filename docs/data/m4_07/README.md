# M4-07 raw measurements

All files here are emitted by [`m4_07_many_branch.rs`](../../../crates/sim-model/examples/m4_07_many_branch.rs). They are raw JSONL and are not simulation inputs.

| Report table / section | Data file | Command |
|---|---|---|
| M4-04 CRN serial/parallel comparison | `crn_parallel.jsonl` | `cargo run --release -p sim-model --example m4_07_many_branch -- --only=crn` |
| SHA versus direct-column day scan | `day_scan.jsonl` | `cargo run --release -p sim-model --example m4_07_many_branch -- --only=scan` |
| M4-05 case (a) serial/parallel search | `minimal_cause_final.jsonl`, `m4_05_search_serial_numeric8.jsonl`, `m4_05_search_parallel_numeric8.jsonl` | `cargo run --release -p sim-model --example m4_07_many_branch -- --only=search` |
| Restore/clone share for the copy-on-write decision | `clone_share.jsonl` | `cargo run --release -p sim-model --example m4_07_many_branch -- --only=cow` |

The two `*_invalid_candidate_selection.jsonl` files preserve an early search attempt that was not used: candidate truncation occurred after string ordering, and a chunk size of 256 serialized its ten replicates. The corrected case (a) run selects the eight lowest numeric AgentIds and uses chunk size 1. The invalid attempt is retained for provenance and is not cited as a result.
