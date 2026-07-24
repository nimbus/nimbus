# NNC2.2 Portable Allocator Contract Proof

Date: 2026-07-24

Status: `passed`

Source commit before the item:
`46eb0ff3d`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now owns the public, transport-free
`NetworkSegmentAllocator` lifecycle contract. Its attachment-facing operations
accept `NetworkAttachmentId`, never `SandboxId`, an address, a provider name,
or a workload-local type.

The low dependency boundary uses associated adapter types:

```rust
NetworkSegmentAllocator {
    type Segment;
    type Error;
}
```

`nimbus-sandbox` fixes those associated types to
`OciSegmentRealization` and `SandboxError`. Netavark bridge/interface names,
provider cleanup, netns inspection, IPAM effects, and the concrete single-node
and lease-gated cluster allocators therefore remain sandbox-owned. There is no
reverse dependency and no provider effect in `nimbus-network`.

## Expected-Red Baseline

The committed NNC2.1 checkpoint at `46eb0ff3d` recorded the exact pre-change
state:

- the aggregate static verifier passed 13 conditions and failed two;
- `NNCV011 single-portable-vocabulary-owner` named the private
  sandbox-owned allocator trait and concrete caller accessors; and
- `NNCV005` named the deliberately later port-allocation authorities.

That is the fail-before evidence for this extraction. The pass-after verifier
now reports 14 passes and only the later NNCV005 failure.

## Attachment Identity

`NetworkAttachmentId::for_workload_attachment` derives a stable identifier
from two independent inputs:

```text
domain("nimbus.network.attachment.v1")
  + length(workload incarnation key) + workload incarnation key
  + length(attachment name) + attachment name
  -> SHA-256 -> first 128 bits -> canonical prefixed ULID payload
```

The length framing prevents component-boundary ambiguity. Tests prove:

- identical incarnation key plus name is stable across replay;
- a different attachment name produces another identity;
- a replacement workload incarnation produces another identity;
- `("ab", "c")` cannot alias `("a", "bc")`; and
- the derived identifier round-trips through the canonical parser.

The OCI adapter currently names one attachment, `default`. It derives that
attachment from the sandbox incarnation ID at the sandbox/network boundary.
The portable contract never imports or mentions `SandboxId`.

The durable segment payload changed cleanly from `live_sandboxes` to
`live_attachments`. The old field's deserialize default was removed: this is a
pre-launch breaking schema replacement, not a compatibility shim. An old or
partially rewritten authority fails closed at the checksummed typed-payload
boundary.

## Injected Capability And Provider Ownership

Both OCI-family backends now retain:

```text
Arc<dyn NetworkSegmentAllocator<
    Segment = OciSegmentRealization,
    Error = SandboxError,
>>
```

Their default constructors inject a concept-owned
`ConfiguredSegmentAllocator`. That adapter opens the one network-owned local
authority for each operation and propagates store/configuration failures.
Container and krun no longer expose or call a concrete `segment_allocator()`
accessor and contain no downcast.

A shared behavior-recording substitute is injected independently into both
backends. Each proof asserts:

- startup invokes `Reconcile(empty)`;
- network resolution invokes `SegmentFor(the requested tenant)`;
- the returned provider-specific subnet, network name, and interface are the
  substitute's values; and
- no default/concrete allocator is reconstructed.

Placement and the reaper accept the fixed trait object. Provider realization,
real socket/network namespace work, IP address assignment, bridge deletion,
and cleanup-error collection remain in `nimbus-sandbox`.

## Hold, Release, And Reconciliation Equivalence

All production paths use the same pure attachment derivation:

- container acquire;
- krun acquire;
- shared release/reap;
- live-netns enumeration; and
- durable orphan reconciliation.

The persisted hold and the netns filename therefore converge on the same
`NetworkAttachmentId`. Reconciliation validates every durable tenant and
attachment ID before pruning it, then releases every block only when no live
attachment remains.

The typed filesystem scan first confirms that an entry owns a
`networks/netns` tree and only then parses its tenant ID. A stray non-tenant
sibling such as `.DS_Store` is ignored as before, while an invalid identity
that actually claims a live netns tree fails closed. The regression test proves
a foreign sibling cannot suppress reclamation of a real orphan.

## Cluster Seam

The documentation-only cluster allocator implements the same extracted trait:

- allocation, acquire, grow, and reconciliation continue through a live
  fenced lease;
- release preserves the existing lease-expiry behavior for the later NNC2.6
  expected-red proof;
- `requires_cluster_lease()` remains true; and
- cluster membership, routing, transport, and mesh remain outside this crate
  and this item.

No cluster transport type or effect moved into `nimbus-network`.

## Dependency And Source Proof

All-feature metadata reports exactly one outgoing workspace edge:

```json
[{"name":"nimbus-core","kind":"normal"}]
```

Source scans report:

```text
rg "SandboxId" crates/nimbus-network
# no matches

rg "trait NetworkSegmentAllocator" crates --glob '*.rs'
# crates/nimbus-network/src/segment.rs only

rg "fn segment_allocator|downcast" \
  crates/nimbus-sandbox/src/backends/container \
  crates/nimbus-sandbox/src/backends/krun
# no concrete allocator accessor or downcast
```

The static verifier:

```text
timeout 900 bash scripts/verify-nimbus-network-control-plane.sh --self-test
# self-test: 15 passed, 0 failed

timeout 900 bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed, 1 failed; exit 1 exactly as expected
```

`NNCV011` is green. The sole failure is `NNCV005`, whose old port allocators
remain visible for NNC3.

## Behavioral Verification

```text
cargo test -p nimbus-network
# 61 passed; 0 failed; 0 ignored

cargo test -p nimbus-sandbox
# library: 252 passed; 0 failed; 13 ignored
# helper binary: 2 passed; 0 failed; 0 ignored
# macOS Linux-only integration targets: 0 executable cases

cargo test -p nimbus-sandbox \
  container_backend_consumes_the_injected_portable_segment_allocator
# 1 passed; 0 failed

cargo test -p nimbus-sandbox \
  krun_backend_consumes_the_injected_portable_segment_allocator
# 1 passed; 0 failed

cargo test -p nimbus-sandbox \
  reconcile_ignores_non_tenant_siblings_without_a_netns_tree
# 1 passed; 0 failed

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

bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

The 13 ignored sandbox cases are named earlier/later NNC expected-red tests,
the allocation/port scale benchmarks, and child-process roles. This item added
no ignore and weakened no assertion.

## Independent Review And Disposition

The repository autoreview workflow ran Claude Opus 4.8 at maximum reasoning
over the complete local bundle.

The first review accepted one P2 finding: the newly typed live-netns scan
parsed every child under `tenants/` before checking whether it owned a netns
tree, so a foreign sibling could make best-effort startup reconciliation
silently no-op. The scan now checks the tree first, and a concrete regression
test proves the real orphan is still reclaimed in the presence of `.DS_Store`.

Focused tests and all-target/all-feature Clippy passed after that fix. The
required full-bundle rerun reported:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.8)
```

The reviewer explicitly confirmed attachment-key equivalence, object safety,
dependency direction, failure propagation, cluster behavior preservation,
provider-effect ownership, the breaking durable-field replacement, and the
filesystem-scan correction.

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
