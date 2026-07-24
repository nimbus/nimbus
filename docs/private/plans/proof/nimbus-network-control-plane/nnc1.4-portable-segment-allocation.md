# NNC1.4 Portable Segment Allocation Proof

Date: 2026-07-23

Status: `passed`

Source commit before the item:
`20ee91c353231f5ab5e9ae3bfe5e36a0ccbad420`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

Portable segment allocation and OCI provider realization are now distinct
concepts with distinct owners:

```text
nimbus-core::Cidr
        |
        v
nimbus-network::AllocatedSegment
  { NetworkSegmentId, TenantId, Cidr, NetworkLeaseEpoch }
        |
        v
nimbus-sandbox::OciSegmentRealization
  { portable allocation, host-local slot-derived provider names }
```

`nimbus-core` retains only the zero-I/O `Cidr` value and its address math. The
former `NetworkSegment` and `NetworkId` types are deleted. They were not
portable: they embedded provider network/interface naming and derived their
claimed identity solely from a host-local allocation index.

`nimbus-network::AllocatedSegment` now carries:

- a domain-separated, globally minted `NetworkSegmentId`;
- explicit `TenantId` attribution;
- the assigned `Cidr` as location rather than identity; and
- the issuing node authority's `NetworkLeaseEpoch`.

Its source contains no bridge, interface, network-name, network-ID, or
Netavark realization vocabulary.

The sandbox-owned `OciSegmentRealization` composes that allocation with the
host-local provider network name, interface name, and 64-hex Netavark ID. A
local slot remains appropriate for those same-host handles, but it is no
longer represented as global segment identity.

## Durable Identity Shape

The current sandbox allocator state replaces a bare `Vec<u32>` with explicit
block records:

```text
SegmentBlock {
    local_slot: u32,
    segment_id: NetworkSegmentId,
}
```

The ID is minted while the allocator lock is held and persisted with the block.
Reads, holds, growth, release, orphan reconciliation, and restart reconstruct
the same portable allocation from that record. Reuse of a fully released local
slot mints a new segment ID; the old address and provider names are never
treated as the new allocation's identity.

This is a deliberate pre-launch breaking state-shape replacement. There is no
`indices` compatibility field, alias, migration shim, or fallback. An old or
malformed shape fails closed at deserialization. NNC2.1 still owns replacing
the current non-atomic sandbox JSON authority with the versioned, checksummed,
crash-safe network store; this item does not claim that durability proof.

## Cross-Node And Stability Proof

The focused allocator test constructs two independent node roots with disjoint
`/16` super-nets. Both allocate local slot zero for the same tenant, producing
the expected host-local provider interface `nb-0`, but:

- their CIDRs are `10.10.0.0/24` and `10.20.0.0/24`;
- their `NetworkSegmentId` values are distinct despite the identical local
  slot; and
- fresh allocators over the same state roots recover each original ID, tenant
  attribution, and epoch.

The existing refcount/reuse test also proves that after the last hold releases,
the next tenant may reuse the cleaned CIDR and local slot but receives a new
global segment identity.

Two network-owned unit tests separately prove that:

- construction preserves the explicit ID, tenant, CIDR, and epoch; and
- two allocations can share an address while retaining different identities,
  so an IP range cannot become identity accidentally.

## Provider Boundary And Preserved Behavior

All OCI naming behavior remains in `nimbus-sandbox`:

- `nimbus-t-<local-slot>` provider network names;
- `nb-<local-slot>` interface names, including the existing IFNAMSIZ bound;
- the provider's 64-hex network ID;
- Netavark request/teardown behavior;
- namespace, IPAM, firewall, forwarding, and egress effects; and
- container and krun config materialization.

The existing allocator behavior remains green for disjoint tenant subnets,
idempotent assignment, refcounts, exhaustion, stale epochs, concurrent
threaded use, orphan reconciliation, multi-block growth/drain, block-aware
placement, and both OCI backend consumers. The cluster test seam still only
supplies a fenced super-net; transport, node membership, routing, mesh, and
openraft remain outside `nimbus-network`.

The private `NetworkSegmentAllocator`, its `SandboxId` holds, concrete
`SingleNodeSegmentAllocator` placement consumer, and sandbox JSON authority
deliberately remain for their ordered NNC2.1/NNC2.2 extraction. This prevents
NNC1.4 from hiding provider realization inside a prematurely promoted
interface.

## Ownership And Dependency Proof

Static source scans prove:

- no `NetworkSegment` or `NetworkId` definition remains in `nimbus-core`;
- no Rust source imports either deleted core type;
- `AllocatedSegment` has exactly one definition in `nimbus-network`;
- the portable segment module contains none of the provider realization terms;
- provider names occur only in the sandbox-owned realization and effect code;
  and
- the existing NNCV011 failure now names only the private sandbox allocator
  trait.

A fresh six-profile dependency capture at source HEAD `20ee91c35` reports zero
cycles:

| Profile | Workspace edges | Cycles |
| --- | ---: | ---: |
| normal default host | 222 | 0 |
| dev/test/build default host | 247 | 0 |
| all-feature macOS arm64 | 253 | 0 |
| all-feature Linux x86_64 | 253 | 0 |
| all-feature Linux arm64 | 253 | 0 |
| all-feature Windows x86_64 | 253 | 0 |

No manifest changed. `nimbus-network` retains exactly one outgoing workspace
edge:

```text
nimbus-network -> nimbus-core
```

## Behavioral Verification

```text
timeout 900 cargo test \
  -p nimbus-core -p nimbus-network -p nimbus-sandbox \
  --all-targets -- --test-threads=1
```

| Target | Passed | Ignored | Notes |
| --- | ---: | ---: | --- |
| `nimbus-core` library | 191 | 0 | Pure CIDR and all existing core behavior. |
| `nimbus-network` library | 17 | 0 | Includes two portable-allocation tests. |
| `nimbus-sandbox` library | 246 | 16 | Includes realization, cross-node ID, restart, reuse, placement, cluster, and orphan tests. |
| sandbox helper binary | 2 | 0 | Existing user-switch tests. |
| Linux-only integration targets on macOS | 0 | 0 | Compiled, but target cfg supplied no executable cases; not claimed as Linux provider evidence. |

The 16 sandbox ignores are the existing, named NNC expected-red or explicit
benchmark/child-role cases. No ignore was added or removed by this item.

Additional gates:

```text
timeout 600 cargo check \
  -p nimbus-core -p nimbus-network -p nimbus-sandbox --all-targets
# exit 0

timeout 900 cargo clippy \
  -p nimbus-core -p nimbus-network -p nimbus-sandbox \
  --all-targets -- -D warnings
# exit 0; only pre-existing vendored Brotli warnings outside workspace targets

timeout 180 cargo doc -p nimbus-network --no-deps
# exit 0

cargo fmt --all --check
git diff --check
# exit 0
```

## Static Verifier State

The verifier remains intentionally red at ten passes and two later-band
failures:

- `NNCV005` still identifies the pre-NNC3 duplicate port allocators; and
- `NNCV011` now identifies only the sandbox-owned
  `NetworkSegmentAllocator`, which NNC2.2 moves and generalizes.

The former core `NetworkSegment` failure disappeared without suppressing the
remaining allocator obligation.

## Independent Review

The repository `autoreview` workflow reviewed the complete staged NNC1.4 diff
with Claude Opus 4.8 at maximum reasoning. It reported no accepted or
actionable findings and assessed the patch as correct with 0.80 confidence.
The review independently confirmed:

- `Cidr` remains zero-I/O core vocabulary and both provider-coupled core types
  are fully deleted;
- `AllocatedSegment` is provider-neutral while `OciSegmentRealization` alone
  owns the host-local provider names;
- ULID minting under the allocator lock is appropriate global identity here,
  and every live read, hold, release, orphan, and restart path reconstructs the
  persisted ID rather than regenerating it;
- same-slot cross-node distinction, restart stability, and new identity after
  cleaned-slot reuse are non-vacuously tested;
- the state-shape replacement fails closed without an alias, default, shim, or
  fallback;
- existing behavior, dependency direction, and effect ownership are
  preserved; and
- the later crash-safety, cleanup quarantine, neutral attachment, concrete
  allocator, and cluster-transport obligations are truthfully deferred.

## Scope Guard

NNC1.4 does not:

- move allocation persistence or the allocator trait ahead of NNC2;
- claim the current JSON write is crash-safe or checksummed;
- generalize `SandboxId` ahead of `NetworkAttachmentId` substitution;
- fix cleanup-before-reuse, secondary-block selection, or expired-lease cleanup
  ahead of their NNC2 owners;
- move provider naming or effects into `nimbus-network`; or
- add cluster transport, node identity, membership, routing, mesh, or overlay
  behavior.
