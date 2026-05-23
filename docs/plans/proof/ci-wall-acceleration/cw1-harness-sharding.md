# CW1 — verification-harness corpus sharding

Adds `NIMBUS_HARNESS_SHARD=N/M` corpus sharding to the verification
harness so each surface can fan out into multiple CI jobs.

## Shape

`scripts/verification-harness.sh` accepts an optional third positional
arg of the form `N/M` (1 ≤ N ≤ M):

```bash
bash scripts/verification-harness.sh required server 1/4
```

The script validates the spec (rejects `0/M`, `N/0`, `N>M`, non-integer
forms) and exports `NIMBUS_HARNESS_SHARD` before invoking cargo. The
single-flight key includes the shard suffix so concurrent shards do not
collide on the same lock.

Three corpus selectors honor the env var:

- `crates/nimbus-storage/src/simulation/verification.rs` —
  `selected_generated_task_history_seed_corpus()` (the cross-surface
  generated-history corpus consumed by storage/engine/server harness
  tests).
- `crates/nimbus-server/src/tests/verification_harness.rs` —
  `selected_server_verification_cases()` (the server-specific
  transport-liveness campaigns: 7 cases).
- `crates/nimbus-runtime/src/runtime/tests/verification_harness.rs` —
  `selected_runtime_verification_cases()` (runtime liveness +
  integrity cases).

Each selector applies the existing case-id filter first, then applies
the shard filter (`index % M == N - 1`). A shard that selects zero
cases is a clean no-op — the harness loop iterates an empty `Vec` and
exits, which is correct under the sharding contract (across `N=1..M`,
every case is covered exactly once).

## Matrix expansion

`.github/workflows/ci.yml` `harness:` matrix expands from 4 to 9
entries:

| Surface | Pre-CW1 shards | CW1 shards | Reason |
|---------|----------------|------------|--------|
| Server  | 1 (12.7m)      | **4**      | Transport-liveness has 7 cases; 4 shards = ~2 cases per lane |
| Engine  | 1 (11.0m)      | **2**      | Generated-history corpus has 2 cases; M=2 is the cap |
| Storage | 1 (8.3m)       | 1          | Already below the wall pole; sharding would idle 1 shard |
| Runtime | 1 (1.4m)       | 1          | Already trivial |

Server sharding count (4) is intentionally chosen for the heaviest
corpus on the surface, not the generated-history one. The 2-case
generated-history corpus only populates server shards 1/4 and 2/4 (the
filter leaves shards 3/4 and 4/4 with zero generated-history cases),
but the dominant transport-liveness corpus fans out across all 4
shards, which is where the duration savings come from.

## Local validation

Compile check across the three crates that wired the shard helpers:

```
$ cargo check -p nimbus-storage -p nimbus-server -p nimbus-runtime --tests
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 35.08s
```

Running the server transport-liveness corpus under `1/2` then `2/2`
confirms the shards are complementary and together cover all 7 cases:

```
$ NIMBUS_HARNESS_SHARD=1/2 cargo test -p nimbus-server \
    verification_harness_required_transport_liveness_campaigns \
    -- --ignored --nocapture --test-threads=1
running case websocket-disconnect-cleanup ...
running case scheduled-job-history-failure-publication ...
running case runtime-tenant-fairness-websocket-rejection ...
running case mongodb-wire-handshake ...
test result: ok. 1 passed; 0 failed; 0 ignored; ...

$ NIMBUS_HARNESS_SHARD=2/2 cargo test -p nimbus-server \
    verification_harness_required_transport_liveness_campaigns \
    -- --ignored --nocapture --test-threads=1
running case websocket-auth-change-resubscribe ...
running case runtime-tenant-fairness-http-rejection ...
running case mongodb-wire-crud-roundtrip ...
test result: ok. 1 passed; 0 failed; 0 ignored; ...
```

4 + 3 = 7 cases total, no overlap, no drop. Shards 4/4 of a smaller
corpus (e.g. the 2-case generated-history corpus) are a no-op as
expected.

The CW0 verifier reports the matrix expansion + corpus-filter wiring as
PASS:

```
[4] Harness script accepts shard arg + corpus test honors NIMBUS_HARNESS_SHARD
  PASS  Harness script accepts shard arg, propagates NIMBUS_HARNESS_SHARD, corpus filter present
[5] harness job matrix includes per-surface shard expansion
  PASS  harness matrix includes per-surface shard expansion (8 surface entries)
```

## Expected wall delta

- **Server Verification Harness**: 12.7m → max shard ~4m (7 cases /
  4 shards × per-case cost), removing it as the Path A pole.
- **Engine Verification Harness**: 11.0m → ~6m per shard (boot cost
  per shard is non-trivial; ~half-of-monolith is the lower bound).
- **Path A pole post-CW1**: `warm-sccache (10.2m) + server-harness
  shard (~4m)` = ~14.2m, vs CW0's 22.9m.
- **Wall**: ~15-16m if the lateral poles (Rust Workspace Tests at
  15.7m, External Provider Integration Tests at 14.6m) become the
  new ceiling. CW2 and CW3 attack those.

Steady-state wall delta will be measured against this commit's CI run
and recorded in the Execution Log row for CW1.

## Repro the shard filter locally

```bash
# Run shard 1 of 4 of the server transport-liveness corpus
NIMBUS_HARNESS_SHARD=1/4 cargo test -p nimbus-server \
  verification_harness_required_transport_liveness_campaigns \
  -- --ignored --nocapture --test-threads=1

# Same thing via the harness wrapper (also wires single-flight)
bash scripts/verification-harness.sh required server 1/4
```

## Notes

- Sharding is opt-in: with no `NIMBUS_HARNESS_SHARD` env var, every
  corpus selector returns the full case list (i.e. existing
  developer-machine `cargo test` invocations are unchanged).
- The shard-arg surface on `scripts/verification-harness.sh` only
  applies to the `required` and `nightly` modes. The `repro` mode is
  case-id-targeted, so sharding is meaningless there.
- `harness-nightly` is intentionally NOT sharded in CW1: the nightly
  matrix runs the full nightly corpus (4 generated-history cases per
  surface) and is not on the PR critical path. Sharding the nightly
  matrix is straightforward if a future CW wave needs it; the corpus
  filter already supports it.
