# NNC2.6 Expired-Lease Cleanup Authority Proof

Date: 2026-07-24

Status: `passed`

Source commit before the item:
`e7c7656e0ad22fd7aed095818d29902faa9c64dd`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

Cluster segment creation authority and durable old-handle cleanup authority are
now separate capabilities:

```text
current committed, unexpired lease
  -> assign / acquire / grow

persisted super-net CIDR + NetworkLeaseEpoch + stable segment IDs
  -> inspect / quarantine / release / finalize / orphan quarantine
```

Lease expiry, lease-provider loss, and observation of a replacement epoch all
fence creation. None of them can strand cleanup for provider effects created
under the old durable epoch. Allocation/IPAM remains independent of cluster
membership, routing, mesh, and transport.

## Fail-Before Evidence

Before editing source, the preserved NNC0.5 regression was run exactly:

```text
timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::cluster::tests::expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold \
  -- --exact --ignored --nocapture
```

It exited `101`. The cluster adapter correctly rejected new acquire after the
clock reached the epoch-7 lease deadline, but the same live-lease gate also
rejected release of the already-durable old hold. The assertion failed with:

```text
expired create authority must still permit cleanup of its durable old hold
```

## Capability Split

`ClusterSegmentAllocator::live_inner` is the only adapter path that reads
`ClusterLeaseProvider`. It requires a committed, unexpired lease before
`segment_for`, `segments_for`, `acquire`, or `grow_block_if_current`.

`DurableSegmentCleanupAuthority` is a restricted sandbox-owned capability
reconstructed from the checksummed segment partition. Its public surface inside
the adapter has no assign, acquire, or grow method. It exposes only:

- non-creating `inspect_segments`;
- durable attachment `quarantine`;
- confirmed-detach hold `release`;
- identity/epoch-fenced `finalize_release`; and
- orphan inspection/quarantine reconciliation.

The portable `NetworkSegmentAllocator` contract now names
`inspect_segments` separately from its assign-capable lookup. This makes
inspection semantics testable without pretending `segments_for` is read-only.
The cleanup contract explicitly states that lease freshness cannot revoke
quarantine or release for an identity and epoch still present in durable
authority.

## Durable-Fence Semantics

Opening cleanup authority requires a complete persisted pair:

- the node super-net CIDR; and
- its typed `NetworkLeaseEpoch`.

A checksum-valid payload with only one field, or with tenant allocations but no
pair, fails closed without changing the authority file. The durable values are
then revalidated inside every state operation, so a concurrent epoch or
super-net change cannot turn a stale cleanup view into current authority.
Finalization still compares the tenant, complete ordered stable segment IDs,
and old epoch before any local slot becomes reusable.

An empty authority has no cleanup capability. A fully finalized authority
retains its super-net/epoch fence, so repeated finalization and repeated
attachment cleanup return their explicit idempotent `AlreadyReleased`
outcomes.

## Behavioral Proof

The former expected-red test is now an ordinary green test. It proves:

1. an epoch-7 hold is acquired while the lease is live;
2. after exact expiry, `inspect_segments` returns the durable realization
   without changing authority bytes;
3. `segment_for`, `segments_for`, `acquire`, and growth all reject;
4. quarantine succeeds under the expired old epoch;
5. a newly reported epoch-8/different-super-net lease neither hides the old
   handle nor overwrites its state;
6. restart after the provider reports no current lease still inspects and
   releases the old hold;
7. the cleanup token carries epoch 7;
8. finalization returns `Released` once and `AlreadyReleased` on replay;
9. subsequent inspect/quarantine/release are idempotently absent; and
10. no-current-lease creation remains rejected after cleanup.

`durable_cleanup_authority_rejects_incomplete_persisted_fencing` covers three
checksum-valid malformed shapes and verifies byte-for-byte non-mutation on
rejection.

## Behavioral And Quality Evidence

Final affected suites after the capability child-module extraction:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::cluster::tests::expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold \
  -- --exact --nocapture
# 1 passed; 0 failed; 0 ignored

timeout 300 cargo test -p nimbus-network --all-features
# 63 passed; 0 failed; 0 ignored

timeout 900 cargo test -p nimbus-sandbox
# library: 269 passed; 0 failed; 10 ignored
# helper binary: 2 passed; 0 failed; 0 ignored
# macOS Linux-only integration targets: 0 executable cases
```

The ten ignored library cases are named later-NNC expected-red tests, explicit
scale characterizations, and child-process roles. NNC2.6 removed the NNC0.5
ignore and added no ignored case.

Quality gates:

```text
cargo fmt --all --check
git diff --check

timeout 600 cargo check -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features

timeout 900 cargo clippy -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features -- -D warnings

timeout 300 cargo doc -p nimbus-network --no-deps
# all exit 0; only pre-existing vendored Brotli warnings are emitted
```

Dependency metadata still reports the exact allowed workspace edge:

```json
[{"name":"nimbus-core","kind":null}]
```

Static and documentation gates:

```text
bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 15 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
# only NNCV005, the deliberately later NNC3 port-authority migration

bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Modularity And Ownership

The restricted cleanup capability moved to the concept-owned
`oci/network/segment/cleanup.rs` child module when the allocator parent reached
the repository's 2,000-line decomposition threshold. The parent is now 1,918
lines and remains within its recorded 1,500–1,999 ownership exception; the
child is 96 lines. Provider effects remain in `nimbus-sandbox`, portable
contract/state remains in `nimbus-network`, and no transport, tenant, server,
system, proxy, Netavark, Iroh, or cloud dependency entered the portable crate.

## Worktree Isolation

The original checkout at `/Users/jack/src/github.com/nimbus/nimbus` still
contains exactly its four pre-existing user-owned paths:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No push or pull request was performed.

## Independent Review

The repository autoreview workflow reviewed the complete 44,798-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.77)
```

The reviewer independently traced operation routing, durable reconstruction,
`None` versus corruption semantics, revalidation under concurrent state change,
non-mutating inspection/rejection, trait substitution, restart/provider-loss,
idempotency, module ownership, and dependency invariants. It reported no
landing-blocking defect. Its bounded confidence note was that unchanged
single-node allocator internals were inferred from the supplied tests and
contract context; those internals were exercised directly by the named full
sandbox suite.
