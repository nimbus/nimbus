# NNC2.4 Stable Segment Identity And Lease-Epoch Proof

Date: 2026-07-24

Status: `passed`

Source commit before the item:
`414d009938a6b35c62624e01d1c0195be6c05c1e`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

The stable `NetworkSegmentId` and `NetworkLeaseEpoch` vocabulary introduced by
NNC1 now crosses the complete sandbox allocation and future cluster-lease seam.
`InstalledSuperNet`, durable `SegmentState`, and `SuperNetLease` no longer
represent a lease epoch as an interchangeable `u64`.

This item does not create cluster membership, lease issuance, routing, mesh, or
transport authority. The cluster adapter still consumes a fenced super-net
lease; the portable identity type remains in `nimbus-network`; OCI provider
realization and IPAM effects remain in `nimbus-sandbox`.

## Baseline And Scope

The NNC1.2 and NNC1.4 proof records already established:

- domain-separated, stable `NetworkSegmentId` wire identity;
- a distinct monotonic `NetworkLeaseEpoch`;
- portable `AllocatedSegment` attribution;
- fixed two-node same-local-slot non-collision and restart behavior; and
- a fixed stale-epoch load fence.

The pre-item source checkpoint nevertheless retained four raw-number seams:

```text
git show HEAD:crates/nimbus-sandbox/src/backends/oci/network/segment.rs |
  rg -n "epoch: u64|supernet_epoch: Option<u64>"
# InstalledSuperNet.epoch: u64
# SegmentState.supernet_epoch: Option<u64>

git show HEAD:crates/nimbus-sandbox/src/backends/oci/network/cluster.rs |
  rg -n "epoch: u64|supernet_epoch: Option<u64>|fn lease\\("
# SuperNetLease.epoch: u64
# test lease helper epoch: u64
```

This was a type-safety gap rather than a known behavior failure: the existing
fixed cross-node and stale-load tests were green before the item. NNC2.4 closes
the gap and adds the property/fail-closed coverage required by its acceptance
criterion instead of manufacturing a duplicate expected-red behavior test.

## Typed Fencing Seam

These fields now use `NetworkLeaseEpoch` directly:

```text
InstalledSuperNet.epoch
SegmentState.supernet_epoch
SuperNetLease.epoch
```

The default single-node epoch is constructed explicitly as epoch zero.
`AllocatedSegment` receives the installed typed epoch without a numeric
round-trip. Error text converts through `as_u64()` only at the operator-facing
format boundary.

`NetworkLeaseEpoch` has transparent numeric serialization, so the durable state
shape remains a number while Rust rejects accidental interchange with a port,
generation, time, count, or another numeric token. Nimbus is pre-launch and no
compatibility shim or parallel representation was introduced.

The closing source scan is empty:

```text
rg -n "epoch: u64|supernet_epoch: Option<u64>" \
  crates/nimbus-sandbox/src/backends/oci/network/segment.rs \
  crates/nimbus-sandbox/src/backends/oci/network/cluster.rs
# no matches
```

## Cross-Node Identity Property

`disjoint_node_supernets_never_alias_segment_identity_and_restart_stably` runs
16 generated cases. Each case chooses:

- two disjoint `/16` node super-nets;
- one through six identical tenant/local-slot positions on each node; and
- independent lease epochs.

For every allocation it proves:

- the provider-local interface name may be identical across nodes;
- the allocated CIDRs do not overlap;
- the stable segment IDs do not alias;
- every ID across both nodes is unique;
- each allocation carries its node's typed epoch; and
- reopening both durable authorities preserves every ID and epoch.

The property deliberately compares stable resource identity rather than
addresses or provider-local names. It therefore proves the routed,
non-overlay allocation seam without treating an IP address as workload
identity.

## Stale-Epoch Create/Growth Fence

`stale_epoch_rejects_every_create_and_growth_entrypoint_without_mutation`
creates durable allocation state under epoch 7, reopens the same authority
under epoch 8, then verifies all four allocation-authority entry points reject:

```text
segment_for
segments_for
acquire
grow_block_if_current
```

The test reads the authority bytes before and after every rejected attempt and
asserts exact equality. A stale epoch therefore cannot create, observe for
growth, reserve, or append a segment, and rejection cannot partially mutate
durable authority. Cleanup authority after expiry remains the separately
owned NNC2.6 task.

## Behavioral Proof

Focused proof:

```text
timeout 240 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::disjoint_node_supernets_never_alias_segment_identity_and_restart_stably \
  -- --exact --nocapture
# 1 passed; 0 failed; 0 ignored (16 property cases)

timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::segment::tests::stale_epoch_rejects_every_create_and_growth_entrypoint_without_mutation \
  -- --exact --nocapture
# 1 passed; 0 failed; 0 ignored

timeout 180 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::cluster::tests -- --nocapture
# 5 passed; 0 failed; 1 ignored
```

The ignored cluster test is the named NNC2.6 expected-red case proving that
expired create authority currently also blocks cleanup. NNC2.4 does not hide
or claim that later obligation.

Full affected-crate suites:

```text
timeout 300 cargo test -p nimbus-network --all-features
# 61 passed; 0 failed; 0 ignored

timeout 900 cargo test -p nimbus-sandbox
# library: 260 passed; 0 failed; 12 ignored
# helper binary: 2 passed; 0 failed; 0 ignored
# macOS Linux-only integration targets: 0 executable cases
```

The ignored sandbox cases are named earlier/later NNC expected-red tests,
explicit scale characterization, and child-process roles. NNC2.4 adds no
ignored test.

## Dependency, Source, And Quality Gates

Metadata still reports exactly one outgoing workspace dependency:

```json
[{"name":"nimbus-core","kind":null}]
```

Quality gates:

```text
cargo check -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features

cargo clippy -p nimbus-network -p nimbus-sandbox \
  --all-targets --all-features -- -D warnings

cargo doc -p nimbus-network --no-deps
cargo fmt --all --check
git diff --check
# all exit 0; Clippy reports only pre-existing non-fatal vendored Brotli warnings

bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 15 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
```

The only aggregate verifier failure is `NNCV005`, the deliberately later NNC3
port-allocation authority migration. Dependency, ownership, effect, duplicate
definition, address-as-identity, routing, and recovery-ledger conditions pass.

Documentation gates:

```text
bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Independent Review

The repository autoreview workflow reviewed the complete 26,328-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.82)
```

The reviewer independently checked the typed field/call-site migration,
transparent durable representation, epoch comparison and diagnostics,
fail-before-mutation byte proof, generated cross-node identity/restart
property, dependency invariants, security fence, modularity, and the explicit
NNC2.6 ignore. It reported no actionable defect.

## Worktree Isolation

The implementation remains in the dedicated owner worktree and branch. The
original checkout at `/Users/jack/src/github.com/nimbus/nimbus` retains exactly
its four pre-existing user-owned paths:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No push or pull request was performed.
