# NNC0.5 Secondary-Block Reuse And Expired-Cleanup Baselines

Status: `expected-red predicates reproduced`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `09c92c455`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, Rust test profile, deterministic injected
lease clock

## Result

Two explicitly ignored tests preserve the distinct NNC0.5 failure predicates.

### Existing secondary-block capacity is stranded

`placement_must_reuse_free_capacity_in_an_existing_secondary_block` uses
`/30` blocks so each block has exactly one container slot:

1. sandbox 1 fills primary `10.7.0.0/30`;
2. sandbox 2 forces growth and fills secondary `10.7.0.4/30`;
3. only sandbox 2's IPAM allocation is removed, leaving the primary full and
   the existing secondary block registered with one free slot; and
4. sandbox 3 enters the real `place_sandbox_on_block` loop.

Current placement checks the primary, observes exhaustion, and calls
`grow_block` without scanning existing secondary blocks. The fixture first
pins the exact unnecessary third block, `10.7.0.8/30`, then fails at the safe
reuse invariant:

```text
assertion `left == right` failed:
placement must reuse free capacity in an existing secondary block before growth
  left: "10.7.0.8/30"
 right: "10.7.0.4/30"
```

NNC2.3 owns one atomic operation across every existing block before growth.

### Lease expiry revokes cleanup authority

`expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold` uses an
injected `AtomicU64` clock:

1. epoch 7's lease is live at time 0;
2. the cluster allocator durably acquires the original tenant/sandbox hold;
3. the clock advances exactly to the 5,000 ms expiry;
4. a distinct new acquisition is rejected specifically as expired, preserving
   the create fence; and
5. release of the durable old hold is also rejected by the same expiry gate.

The test confirms the present release error is the lease-expiry error, then
fails only at:

```text
expired create authority must still permit cleanup of its durable old hold
```

NNC2.6 owns separating create authority from the cleanup authority carried by
a durable old handle.

## Commands and results

Both fail-before commands exited `101` at their exact terminal safety
assertions:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::placement::tests::placement_must_reuse_free_capacity_in_an_existing_secondary_block \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; current block 10.7.0.8/30 != reusable 10.7.0.4/30.

timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::cluster::tests::expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold \
  -- --exact --ignored --nocapture
# 0 passed; 1 failed; new create was fenced, old-hold release was also fenced.
```

The ordinary focused suites and static gates remained green:

```text
timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::placement::tests::
# 1 passed; 0 failed; 1 expected-red ignored.

timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::cluster::tests::
# 5 passed; 0 failed; 1 expected-red ignored.

timeout 300 cargo clippy -p nimbus-sandbox --all-targets -- -D warnings
# Exit 0; no warning from nimbus-sandbox. Existing vendored Brotli warnings
# remain outside the changed crate.

cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
# All exit 0.
```

No random seed, sleep, wall clock, provider privilege, KVM, live cluster,
cloud service, cross-target, or sovereignty-denial lane applies. The lease
provider is the existing cluster seam with a deterministic mutable clock, and
the placement proof uses the real allocator and tenant IPAM state.

## Independent closeout review

The test-only diff was reviewed with the repository autoreview skill and
independent Claude Opus 4.8 at maximum reasoning, with the NNC0.5/NNC2.3/NNC2.6
phase boundary supplied as context. The review exited `0`:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.82)
```

The reviewer independently verified the `0/4/8` `/30` block sequence, that the
fixture frees only the secondary slot while retaining its block, the
unnecessary-growth diagnostic and final safe polarity, the live-to-expired
clock transition, the continued new-create fence, the expiry-specific release
failure, and the final durable-cleanup polarity. No finding was reported.
