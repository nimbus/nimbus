# NNC0.3 Segment Cleanup-Failure/Reuse Baseline

Status: `expected-red predicate reproduced`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `d2bedefc5`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, Rust test profile; no privileged
Netavark or bridge operation required

## Result

The explicitly ignored
`failed_bridge_cleanup_must_fence_segment_from_reuse` test preserves the exact
NNC0.3 failure predicate:

1. the real `SingleNodeSegmentAllocator` allocates `10.0.0.0/24`;
2. the production-faithful release helper durably drops the final sandbox
   hold before invoking provider cleanup;
3. an injected bridge reaper returns the named provider failure while
   recording that the bridge effect survives;
4. a different tenant acquires from the same durable allocator root; and
5. the allocator returns the still-live `10.0.0.0/24` segment again.

The expected-red test asserts the safe invariant, so it fails only at:

```text
assertion `left != right` failed:
a segment with a surviving provider effect must remain fenced from reuse
  left: Cidr { base: 10.0.0.0, prefix: 24 }
 right: Cidr { base: 10.0.0.0, prefix: 24 }
```

This is intentionally a fail-before baseline, not the quarantine
implementation. NNC2.5 owns the two-phase `CleanupPending` fix, conversion of
this test to green, and removal of its ignore marker. Making the current
unsafe reuse a passing ordinary test would invert the regression contract.

## Production-order fidelity

Container and krun previously duplicated this exact teardown sequence:

```text
allocator.release(final hold)
  -> receive TenantDrained segments
  -> reap each provider bridge
  -> report every cleanup error
```

NNC0.3 extracts that unchanged sequence into
`release_network_segment_hold`, used by both backends. Its private
`release_network_segment_hold_with` seam substitutes only the bridge effect,
allowing deterministic failure without host privileges. The three existing
outcomes remain equivalent:

- `TenantDrained` attempts every returned bridge and collects every error;
- `StillLive` performs no provider cleanup; and
- allocator failure returns one teardown error.

No Cargo manifest, dependency edge, portable network type, provider behavior,
or allocation behavior changed.

## Commands and results

The exact fail-before command exited `101` only at the final safety assertion:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::reaper::tests::failed_bridge_cleanup_must_fence_segment_from_reuse \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; 244 filtered out.
# Failure: replacement 10.0.0.0/24 == surviving-effect 10.0.0.0/24.
```

The ordinary focused suites and static gates remained green:

```text
timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::reaper::tests::
# 1 passed; 0 failed; 1 expected-red ignored.

timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::
# 11 passed; 0 failed; 0 ignored.

timeout 240 cargo check -p nimbus-sandbox --all-targets
# Exit 0.

timeout 300 cargo clippy -p nimbus-sandbox --all-targets -- -D warnings
# Exit 0; no warning from nimbus-sandbox. Existing vendored Brotli warnings
# remain outside the changed crate.

cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
# All exit 0.
```

No KVM, live Netavark, gvproxy, cloud provider, cross-target, or
sovereignty-denial lane applies. The test uses the real durable allocator
state and a deterministic injected provider failure; it uses no random seed,
sleep, or address as workload identity.

## Independent closeout review

The diff was reviewed with the repository autoreview skill and independent
Claude Opus 4.8 at maximum reasoning.

The first pass raised one P2 asking NNC0.3 to implement quarantine and enable
the test. The underlying isolation defect is correct, but the disposition is
`rejected as phase-inapplicable`: NNC0.3's explicit success criterion is to
demonstrate the current unsafe reuse, while NNC2.5 owns the pass-after fix.
Implementing NNC2.5 here would destroy the required fail-before checkpoint.
The code comment now records that boundary beside the ignore marker.

The second pass received that canonical plan boundary as review context and
exited `0`:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.77)
```

It independently verified the release/reap extraction is behavior-preserving,
both OCI-family backends use it, the injected failure reaches the real
allocator release ordering, pre-final assertions cannot mask the defect, and
the final `assert_ne!` polarity can turn green only when NNC2.5 fences reuse.
