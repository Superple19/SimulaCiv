# M2-39 Production Rayon Integration & Runtime Crossover Policy Gate

- **Baseline:** `master` at `7e856dfded5f52e92851a4337ec085918ad355e8`
- **Implemented scope:** caller-configured serial, explicit Rayon, and Auto Phase4 execution policies
- **Default execution:** existing production runner remains serial
- **Decision:** **PASS-B — EXPLICIT OPT-IN; Auto remains experimental**

## 1. Result

The configurable runtime policy is implemented and passes exact differential and trajectory checks. Existing serial runner entry points continue to select serial Phase4. Callers may opt into the experimental policy runner and provide a prebuilt Rayon pool, chunk size, and (for Auto) a nonzero threshold. The runner does not create a pool or change its thread count.

The measured host was an AMD Ryzen 5 9600X with 6 physical cores and 12 logical processors. One final paired run found first median gains between N=1,500 and N=6,000, depending on workload, thread count, and chunk size. For its six-thread/128-item-chunk profile, first one-MAD-separated gains were N=2,000 for fixed K=2 and N=2,500 for fixed group size. Three complete paired benchmark runs did not agree: the all-population one-MAD rule returned no threshold, then 3,500, then no threshold. The selected chunk also varied (128, 128, then 256). This run-to-run change means neither value is a stable production setting.

The final production-policy measurement used 7,500 eligible items as a trial point because that run had no qualifying all-population threshold. Auto's median was faster in all eight measured cells at or above the trial point, though the one-MAD intervals overlapped at B/7,500. An earlier complete run also had large Auto-vs-serial differences at some below-threshold cells even though both selected serial. There is not enough repeatable evidence for a production Auto threshold or default chunk. PASS-B keeps Auto available for controlled experiments and explicit caller opt-in; it does not change the default runner.

## 2. Runtime policy and compatibility

The public policy surface is `Phase4ExecutionPolicy::{Serial, Rayon, Auto}` with `Phase4Backend` and `run_hybrid_authority_days_with_phase4_policy`.

- `Serial` selects the existing serial path.
- `Rayon` always uses the supplied caller-owned pool and nonzero chunk size.
- `Auto` reads the current tick's actual eligible Phase4 feature count after Phase3. It selects Rayon when the count is at least the configured nonzero threshold and the supplied pool has more than one worker; otherwise it selects serial.
- The selected pool is never built, replaced, capped, or resized by the runner. Threshold and chunk size are supplied by the caller; no machine-independent defaults are embedded.
- Existing production entry points continue to request `Serial`. The existing explicit Rayon runner remains as a convenience wrapper.

The policy and Rayon scratch are transient execution data. They are not added to `WorldState`, storage, snapshots, hashes, or events. No Phase5+ semantics, RNG mapping, event schema, or frozen expected value changed.

## 3. Crossover and policy measurements

The crossover refinement tested two workloads: A with fixed K=2 and B with approximately 200 agents per group. It sampled N=1,000 through 50,000 at 14 population points, thread pools of 2/4/6/12, and chunks of 128/256/512: 336 paired full-tick cells. Each cell used two warm-ups and seven timed samples; every sample averaged three days. Pool and initial-world construction were outside timers. A fixed five-step permutation spread cell order, and serial/Rayon order alternated per sample. The reliable-gain rule was `Rayon median + MAD < serial median - MAD`.

| Profile | First median gain | First one-MAD-separated gain |
|---|---:|---:|
| A, 6 threads, chunk 128 | 2,000 | 2,000 |
| B, 6 threads, chunk 128 | 2,500 | 2,500 |
| Range across all tested profiles in the final run | 1,500–6,000 | 2,000–6,000 |

In the final run, the conservative all-population rule returned no threshold for the six-thread/chunk-128 profile. At the 7,500-item trial point, the same-profile chunk comparison covered eight cells (two workloads × four populations). Chunk 256 had 5.08% mean and 12.12% maximum regret relative to the best chunk at each cell; chunks 128 and 512 had 5.27%/18.60% and 6.70%/16.20%. Earlier complete runs selected chunk 128 instead. No chunk is endorsed as a stable default.

The production-policy matrix compared direct serial, explicit six-thread Rayon, Auto using the same pool, and a paired per-cell best Rayon configuration. It used two warm-ups and seven samples of three-day full ticks. Auto's selected backend was based on the initial eligible count shown below; subsequent days still evaluated the live count.

| Workload | N | Initial eligible items | Auto backend | Serial µs/tick | Auto µs/tick | Auto speedup |
|---|---:|---:|---|---:|---:|---:|
| A | 1,000 | 1,000 | Serial | 152.60 ± 3.20 | 149.07 ± 1.27 | 1.024× |
| A | 7,500 | 7,500 | Rayon | 1,271.40 ± 7.53 | 1,081.97 ± 11.00 | 1.175× |
| A | 10,000 | 10,000 | Rayon | 1,686.23 ± 43.90 | 1,416.30 ± 10.70 | 1.191× |
| A | 20,000 | 20,000 | Rayon | 3,360.50 ± 9.33 | 3,165.00 ± 124.40 | 1.062× |
| A | 50,000 | 50,000 | Rayon | 9,175.70 ± 94.03 | 8,213.97 ± 379.40 | 1.117× |
| B | 1,000 | 1,000 | Serial | 158.00 ± 3.67 | 164.67 ± 2.00 | 0.960× |
| B | 7,500 | 7,500 | Rayon | 1,372.70 ± 17.80 | 1,346.93 ± 49.40 | 1.019× |
| B | 10,000 | 10,000 | Rayon | 1,808.27 ± 19.77 | 1,564.27 ± 3.67 | 1.156× |
| B | 20,000 | 20,000 | Rayon | 3,683.37 ± 16.53 | 3,493.73 ± 88.20 | 1.054× |
| B | 50,000 | 50,000 | Rayon | 9,844.87 ± 87.40 | 8,959.27 ± 204.23 | 1.099× |

The final 7,500-item trial run improved median full-tick time at all eight sampled cells at or above the threshold; the one-MAD separation was absent at B/7,500. Below the threshold, both policies select serial. An earlier complete run measured Auto-vs-serial ratios from 0.666× to 1.441× across low-population cells, far beyond plausible policy-dispatch overhead. Combined with the changing threshold/chunk selection across runs, this variance prevents selecting a production policy from these measurements.

## 4. Correctness and deterministic parity

The tests cover inclusive threshold boundaries, one-thread serial selection, unchanged caller pool size, both directions of eligible-count threshold crossing, policy-independent errors, and parity among serial, explicit Rayon, and Auto. Snapshot/resume tests compare bytes at Days 100/250/500 and continue from a restored snapshot. The 500-day canonical trajectory is checked against the serial path.

Frozen canonical values remain unchanged and match exactly:

| Canonical output | Expected and observed |
|---|---|
| State | `5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9` |
| Metrics | `ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b` |
| Events | `2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac` |

Day 100/250/500 snapshot bytes match serial exactly. The policy does not change per-agent scalar order, candidate ordering, RNG coordinates, or deterministic Rayon merge. No golden, fixture, expected hash, snapshot, event, or CI file was changed.

## 5. Gate conclusion

**PASS-B — explicit opt-in only.** Deterministic Rayon remains available behind caller-owned execution policy. The serial runner stays the production default. Auto is experimental and requires caller-supplied threshold, pool, and chunk; this gate does not endorse a universal runtime threshold or silently switch the default. Revisit a production Auto default only after a repeatable threshold and chunk policy are supported by stable measurements across both workload families and the intended host profiles.
