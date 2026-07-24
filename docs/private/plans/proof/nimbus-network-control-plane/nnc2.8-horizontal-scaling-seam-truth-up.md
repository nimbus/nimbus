# NNC2.8 Horizontal-Scaling Seam Truth-Up Proof

Date: 2026-07-24

Status: `passed`

Source commit:
`ff071dc81da3597010785286b25f21ea8124a9a2`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

The deferred horizontal-scaling owner now consumes the canonical
transport-free network control plane without taking over its authority:

| Concern | Canonical owner after truth-up |
| --- | --- |
| Stable segment, attachment, and lease identities | `nimbus-network` |
| Portable segment lifecycle contract | `nimbus-network::NetworkSegmentAllocator` |
| Durable local network-state contract | `nimbus-network::LocalNetworkStateStore` |
| OCI/Netavark/netns realization and local adapters | `nimbus-sandbox` |
| Raft-issued node super-net lease | future `nimbus-cluster` |
| Membership, node identity, routing, forwarding, mesh | future `nimbus-cluster` / `ClusterTransport` |

The allocator consumes a fenced node super-net lease. It does not absorb
cluster membership, routing, transport, or consensus. Cross-node forwarding
remains routed-not-overlay; no VXLAN/Geneve or Iroh/openraft edge enters
`nimbus-network`.

## Recovery And Fail-Before

At item start, `horizontal-scaling-plan.md` was absent from the isolated owner
worktree and branch history. Its only copy was an ignored private file in the
original checkout. That source had SHA-256:

```text
5081fe1a93d414446253763bd16583fd68b82d5b5f5cda21a2e6eb74d10ca587
```

Its HS2/HS5 execution row still called
`nimbus-sandbox::SingleNodeSegmentAllocator` the allocator seam, prescribed a
caller-local `install_supernet` path, and described the future Raft leg as
landing a new allocator rather than supplying a fenced lease to the canonical
contract.

The source was read completely, materialized into this owner worktree through
`apply_patch`, and hash-compared before modification:

```text
5081fe1a93d414446253763bd16583fd68b82d5b5f5cda21a2e6eb74d10ca587  original
5081fe1a93d414446253763bd16583fd68b82d5b5f5cda21a2e6eb74d10ca587  owner copy
```

The original file was never edited. The owner copy is force-tracked in this
checkpoint so its shared-seam ledger can be recovered from Git rather than an
ignored file in another checkout.

## Shared-Seam Corrections

The horizontal plan now:

1. links `nimbus-network-control-plane-plan.md` as the sole portable network
   lifecycle owner;
2. records `nimbus-network::NetworkSegmentAllocator` as the one portable
   allocation contract;
3. retains sandbox ownership of OCI realization plus the current single-node
   and cluster-shaped adapters;
4. retains future cluster ownership of the committed node super-net lease,
   membership, identity, routing, forwarding, mesh, and `ClusterTransport`;
5. requires the future cluster composition root to adapt committed state
   through a minimal dependency-safe lease-source seam rather than create a
   reverse dependency or second allocator;
6. preserves lease-expiry self-fencing, epoch-bumped reclamation, durable old
   handle cleanup, disjointness, and routed-not-overlay;
7. keeps HS deferred until its real multi-node activation gate fires; and
8. records the correction in the horizontal-scaling execution log and plan
   index without activating Iroh/openraft implementation.

The current sandbox-local `ClusterLeaseProvider` is explicitly a prototype
boundary, not a claim that a future sibling crate can implement a private
trait. HS must promote the smallest lease-source contract at a dependency-safe
composition boundary when it activates.

## Source And Dependency Proof

The historical stale claims are absent from the updated horizontal owner:

```text
rg -n \
  'nimbus[-_]sandbox::(SingleNodeSegmentAllocator|NetworkSegmentAllocator)|install_supernet' \
  docs/private/plans/horizontal-scaling-plan.md
# exit 1; no matches
```

The landed source has exactly one portable trait definition and keeps the
current provider adapter explicit:

```text
crates/nimbus-network/src/segment.rs:178:
pub trait NetworkSegmentAllocator: Send + Sync

crates/nimbus-sandbox/src/backends/oci/network/cluster.rs:77:
pub(crate) trait ClusterLeaseProvider: Send + Sync
```

Dependency metadata still reports the exact allowed workspace edge:

```json
[{"name":"nimbus-core","kind":null}]
```

The network source and manifest scan finds no Iroh, openraft, Axum, Pingora,
Netavark, sandbox, tenant, server, system, or cluster dependency/effect.

## Recovery-Ledger Fail-Before

The first aggregate verifier run after materializing the shared owner failed
two conditions:

```text
FAIL NNCV005 no-duplicate-port-allocation-authority
FAIL NNCV008 checkpoint-ledger-recoverable
     NNC2.8 recovery checkpoint lacks Last green
```

`NNCV005` is the expected later NNC3 gate. `NNCV008` identified a real
compaction-recovery omission: the new NNC2.8 checkpoint named its owned paths,
next action, and blocker but not its last green commit/evidence. The ledger now
records checkpoint `ff071dc81da3597010785286b25f21ea8124a9a2` plus the green
source, dependency, forbidden-effect, and documentation scans. The rerun
returns to exactly the one intended NNCV005 failure.

## Verification

```text
git diff --check

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

## Worktree Isolation

The original checkout remains untouched with exactly its four pre-existing
user-owned paths:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No implementation, push, or pull request was performed for the deferred
cluster substrate.

## Independent Review

The first review-bundle attempt failed closed before engine invocation because
the historical plan repeated a conceptual token-signing adjective that the
secret scanner classified as a credential-like value. Both occurrences were
rephrased to state the unchanged requirement directly: the current leader
signs the admission token and verification does not require contacting the
signer. The bundle preflight then reported zero secret-like fragments.

The repository autoreview workflow reviewed the complete 63,126-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.8)
```

The reviewer independently checked cross-document ledger consistency, the
source/checkpoint relationship, one-authority allocation, crate dependency
direction, cluster/network ownership, NNC2 completion, NNC3.1 activation, and
pre-launch policy. It found no actionable blocker.
