# NNC2.3 Atomic Existing-Block Allocation Proof

Date: 2026-07-24

Status: `passed`

Source commit before the item:
`c0e52f180`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

OCI placement now chooses and reserves an address across the tenant's complete
ordered segment set in one tenant-IPAM transaction under the shared
`nimbus-network` authority lock. It grows only after that atomic scan reports
every observed block exhausted.

The portable allocator contract now exposes:

```rust
fn segments_for(&self, tenant: &TenantId) -> Result<Vec<Self::Segment>, Self::Error>;

fn grow_block_if_current(
    &self,
    tenant: &TenantId,
    observed_segments: &[Self::Segment],
) -> Result<NetworkSegmentGrowth<Self::Segment>, Self::Error>;
```

`segments_for` returns every block in durable allocation order.
`grow_block_if_current` is a compare-and-swap operation over that complete
ordered identity set. A stale caller receives `ObservationStale`, re-observes,
and retries reservation instead of appending a redundant block.

## Expected-Red Baseline

The committed NNC0.5 regression was run unchanged against source checkpoint
`c0e52f180` before implementation:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::placement::tests::placement_must_reuse_free_capacity_in_an_existing_secondary_block \
  -- --exact --ignored --nocapture

left:  "10.7.0.8/30"
right: "10.7.0.4/30"
NNC2.3_EXPECTED_RED_EXIT=101
```

The original path filled the primary `/30`, grew a secondary `/30`, freed that
secondary's sole workload address, then checked only the primary and grew a
third block. The test is now an ordinary non-ignored test with no conditional
escape.

## Atomic Selection And Reservation

`allocate_container_ips_on_first_available` accepts the full ordered
`OciNetworkConfig` set. While holding the existing tenant-IPAM transaction it:

1. recovers an idempotent reservation by mapping its durable address back to
   exactly one current block;
2. otherwise scans every block in allocation order;
3. reserves the first available workload address in the same transaction; or
4. returns typed exhaustion only after every current block is full.

The durable mutation occurs only after a concrete address is found. Any corrupt
address, invalid subnet, or reservation outside the current block set rejects
the transaction and leaves the previous authority unchanged. The single-config
`allocate_container_ips` path delegates to the same operation, so later
Netavark setup recovers the reservation made during placement rather than
creating another allocation path.

IPAM state and provider configuration remain in `nimbus-sandbox`. No bridge,
namespace, firewall, Netavark, gvproxy, socket, or forwarding effect moved into
`nimbus-network`.

## Growth Fencing

Two placers may both observe the same full set before either requests growth.
The segment authority therefore compares the persisted ordered
`NetworkSegmentId` sequence to the caller's complete observation while holding
the segment transaction. Exactly one caller may append. Every other caller
receives `ObservationStale` and retries against the newly grown block.

The fence deliberately does not compare:

- block count alone;
- CIDR/address location;
- local allocation slot;
- bridge/interface name; or
- Netavark network ID.

Stable segment identity also closes same-count ABA replacement. A test removes
the observed allocation, recreates the tenant on the same reusable local slot,
and proves the old observation cannot grow the replacement because its
`NetworkSegmentId` differs.

The lease-gated cluster adapter delegates the same full-set contract. A lease
or epoch change still fails closed through the existing installed-super-net
check; membership, transport, routing, and mesh remain outside this item.

## Behavioral Proof

Focused proof:

```text
cargo test -p nimbus-sandbox --lib \
  backends::oci::network::placement::tests -- --nocapture
# 5 passed; 0 failed; 0 ignored

cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::growth_fence_rejects_same_count_remove_and_recreate_aba \
  -- --exact --nocapture
# 1 passed; 0 failed; 0 ignored

cargo test -p nimbus-network --lib segment::tests -- --nocapture
# 3 passed; 0 failed; 0 ignored
```

The placement group proves:

- the first exhausted block grows exactly one sibling;
- a freed secondary block is reused without growing a third;
- a retry recovers its already reserved secondary block;
- a 16-case property test frees arbitrary positions across two through six
  `/30` blocks and always reuses the free block without growth; and
- six concurrent placers receive six unique one-workload `/30` blocks with no
  redundant seventh block.

Segment tests additionally prove two growers carrying one observation append
exactly one block, and same-count remove/recreate ABA is stale.

Full affected-crate suites:

```text
timeout 300 cargo test -p nimbus-network
# 61 passed; 0 failed; 0 ignored

timeout 600 cargo test -p nimbus-sandbox
# library: 258 passed; 0 failed; 12 ignored
# helper binary: 2 passed; 0 failed; 0 ignored
# macOS Linux-only integration targets: 0 executable cases
```

The ignored sandbox cases are named earlier/later NNC expected-red tests,
explicit scale characterization, and child-process roles. NNC2.3 removed one
expected-red ignore and introduced none.

## Dependency, Source, And Quality Gates

The public trait remains object-safe with adapter-owned `Segment` and `Error`
types. The object-safety test invokes both complete-set observation and fenced
growth through a trait object. Source scan reports exactly one trait owner and
no old `grow_block` method:

```text
rg "trait NetworkSegmentAllocator" crates --glob '*.rs'
# crates/nimbus-network/src/segment.rs only

rg "fn grow_block\\(" crates --glob '*.rs'
# no matches
```

Metadata remains exactly one outgoing workspace dependency. Cargo renders an
unspecified normal dependency kind as `null` in the raw no-deps JSON:

```json
[{"name":"nimbus-core","kind":null}]
```

Quality gates:

```text
cargo check -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features
# exit 0

cargo clippy -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features -- -D warnings
# exit 0; only pre-existing vendored Brotli warnings remained non-fatal

cargo doc -p nimbus-network --no-deps
cargo fmt --all --check
git diff --check
# exit 0

bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 15 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
```

The sole static-verifier failure is `NNCV005`, which names the deliberately
later NNC3 port-allocation authorities. The checkpoint ledger, dependency
graph, portable owner, provider-effect boundary, duplicate-definition guard,
and address-as-identity guard are green.

Documentation gates:

```text
bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Independent Review

The repository autoreview workflow reviewed the complete 53,600-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
autoreview --mode local --engine claude \
  --model claude-opus-4-8 --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.78)
```

The reviewer independently traced termination, all implementors, lock
boundaries, the six-placer interleaving, idempotent recovery, range matching,
stable-identity ABA fencing, provider ownership, and test strength. It reported
no actionable defect. Its only noted style nit was the direct test dependency
version for `proptest`; there is no workspace dependency entry to reuse, so no
change was warranted.

## Worktree Isolation

The implementation remained in the dedicated owner worktree and branch. The
original checkout at `/Users/jack/src/github.com/nimbus/nimbus` retained its
four pre-existing user-owned paths unchanged:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No push or pull request was performed.
