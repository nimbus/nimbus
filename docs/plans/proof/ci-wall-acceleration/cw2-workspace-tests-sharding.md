# CW2 — workspace tests sharding via nextest --partition

Splits `Rust Workspace Tests` (the 15.7m lateral pole on CW0 baseline)
into a 3-shard matrix backed by `cargo-nextest`'s deterministic
`--partition hash:N/M`.

## Shape

`Makefile`'s `test-rust-workspace` target reads a
`NIMBUS_NEXTEST_PARTITION` env var of the form `N/M` and forwards it
as `--partition hash:N/M`. The single-flight key includes the
partition suffix so concurrent shards in the same workspace do not
collide on the lock:

```makefile
NIMBUS_NEXTEST_PARTITION ?=
ifeq ($(strip $(NIMBUS_NEXTEST_PARTITION)),)
NEXTEST_PARTITION_ARGS :=
NEXTEST_SINGLE_FLIGHT_SUFFIX :=
else
NEXTEST_PARTITION_ARGS := --partition hash:$(NIMBUS_NEXTEST_PARTITION)
NEXTEST_SINGLE_FLIGHT_SUFFIX := -$(subst /,-of-,$(NIMBUS_NEXTEST_PARTITION))
endif

test-rust-workspace: $(UI_DIST_INDEX)
    NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
      $(SINGLE_FLIGHT) --key cargo-nextest-workspace-ci$(NEXTEST_SINGLE_FLIGHT_SUFFIX) \
      -- cargo nextest run --workspace --exclude nimbus-runtime $(NEXTEST_PARTITION_ARGS)
```

CI matrix sets the env var per shard:

```yaml
strategy:
  matrix:
    include:
      - partition: "1/3"
        run-doctests: "true"
      - partition: "2/3"
        run-doctests: "false"
      - partition: "3/3"
        run-doctests: "false"
```

`nextest --partition hash:N/M` hashes test paths to compute the shard
assignment, so the partition is stable across runs (cache reuse +
retry-on-failure both behave) but unpredictable enough to balance
load across shards.

## Doctest placement

`cargo-nextest` does not run doc tests; the workflow still requires
them via `make test-rust-docs` (cargo's libtest doctest pass). Doctests
are ~30s on warm caches — too small to merit their own job, too small
to matter which shard runs them. Shard 1 is pinned via
`if: matrix.run-doctests == 'true'` so they run exactly once across
the matrix without adding a new lane.

## Dry-run validation

The Makefile forwarding produces the expected nextest invocation:

```
$ make -n test-rust-workspace NIMBUS_NEXTEST_PARTITION=1/3
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  bash scripts/single-flight.sh \
    --key cargo-nextest-workspace-ci-1-of-3 \
    -- cargo nextest run --workspace --exclude nimbus-runtime --partition hash:1/3

$ make -n test-rust-workspace
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  bash scripts/single-flight.sh \
    --key cargo-nextest-workspace-ci \
    -- cargo nextest run --workspace --exclude nimbus-runtime
```

When the env var is unset, the invocation is byte-identical to the
pre-CW2 command, so developer-machine `make test-rust-workspace`
behavior is unchanged.

## Expected wall delta

- **Pre-CW2 (CW0 baseline)**: `Rust Workspace Tests` ran 15.7m as a
  single un-sharded job.
- **CW2 with 3 partitions**: each shard runs ~1/3 of the partitioned
  tests. With nextest's per-test runner overhead included, max shard
  is projected to be ~6-7m (not exactly 5.2m because boot cost,
  workspace compile + UI dist, and shared linking dominate the small
  surface).

The lateral pole at 15.7m drops below the `warm-sccache + 1 server
harness shard` Path A pole, so the wall stops gating on it.

## Verifier evidence

```
[6] rust-workspace-tests uses nextest --partition with matrix
  PASS  rust-workspace-tests uses nextest --partition and matrix shard axis
```

The verifier (`scripts/verify-ci-wall-acceleration.sh` condition 6)
treats either inline `--partition hash:N/M` in ci.yml OR
`NIMBUS_NEXTEST_PARTITION` env-var forwarding through the Makefile
(coupled with `--partition hash:$(NIMBUS_NEXTEST_PARTITION)` in
Makefile) as evidence. CW2 chose the Makefile-forwarded form because
it keeps the nextest contract co-located with the rest of the target's
flags rather than scattered between ci.yml and the Makefile.

## Repro the partition locally

```bash
# Run partition 2 of 3
NIMBUS_NEXTEST_PARTITION=2/3 make test-rust-workspace

# Listing the cases assigned to partition 1 of 3
cargo nextest list --workspace --exclude nimbus-runtime --partition hash:1/3
```

## Notes

- CW2 does NOT shard `test-rust-runtime` (the runtime crate's tests),
  which lives in a separate lane and is already faster (9.9m on the
  CW0 baseline). Sharding the runtime crate would require careful
  V8-state isolation per shard; deferred until it becomes the wall
  pole.
- `external-provider-tests` is sharded in CW3 (per-provider matrix
  split), not via nextest partitioning. Per-provider is a more natural
  axis because tests within a provider need its database service
  running.
- `Rust Clippy` (11.7m on CW0) is intentionally not sharded in this
  CW: clippy runs serially via `cargo clippy --workspace` and there
  is no equivalent of nextest partitioning for the lint pass. CW4 may
  revisit this if clippy becomes the wall pole post-CW1-3.
