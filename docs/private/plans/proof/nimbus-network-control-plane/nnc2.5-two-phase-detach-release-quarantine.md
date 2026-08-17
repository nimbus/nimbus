# NNC2.5 Two-Phase Detach/Release Quarantine Proof

Date: 2026-07-24

Status: `passed`

Source commit before the item:
`994c2d593851aae7b7fd680e681a2696c5553ed4`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

Segment location reuse is now separated from attachment/provider teardown by
two durable fences:

```text
quarantine attachment
  -> detach sandbox provider effects
    -> remove persistent netns
      -> release attachment hold
        -> remove every tenant bridge realization
          -> finalize exact segment IDs + lease epoch
            -> local slots become reusable
```

`nimbus-network` owns the portable lifecycle contract and opaque
identity-fenced cleanup token. `nimbus-sandbox` still owns Netavark, bridge,
namespace, egress PEP, machine proxy, and other provider effects. The portable
crate gained no provider, transport, server, tenant, sandbox, or cluster
dependency.

An incomplete or ambiguous step leaves the durable allocation present and
cleanup-pending. An IP address, CIDR, bridge name, or local slot is never
accepted as release identity.

## Fail-Before Evidence

Before implementation, the preserved NNC0.3 regression was run exactly:

```text
timeout 120 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::reaper::tests::failed_bridge_cleanup_must_fence_segment_from_reuse \
  -- --exact --ignored --nocapture
```

It exited `101`. The injected bridge deletion failed and left the old provider
effect alive, but the allocator had already deleted its tenant record; the
replacement tenant therefore received the same `10.0.0.0/24` location. This
was the unsafe reuse window NNC2.5 had to close.

## Portable Contract

`NetworkSegmentAllocator` now exposes three explicit teardown phases:

- `quarantine(tenant, attachment)` durably fences an attachment before the
  first external detach effect;
- `release(tenant, attachment)` is legal only after quarantine and confirmed
  attachment provider/netns removal; the last hold returns
  `NetworkSegmentCleanup<Segment>` without freeing the allocation; and
- `finalize_release(cleanup)` compares the tenant, complete ordered
  `NetworkSegmentId` set, and `NetworkLeaseEpoch` before freeing local slots.

`NetworkSegmentCleanup` carries adapter-owned realizations for effect cleanup,
but callers treat it as opaque and return the same token for finalization.
Allocation authority independently validates its durable identities and epoch.

Outcomes distinguish cleanup pending, still-live sibling holds, first
finalization, and idempotent already-released replay. Direct release without
quarantine fails before changing any authority byte.

## Durable Allocator State

Each tenant entry now records:

- the complete ordered stable segment/block set;
- live attachment identities;
- cleanup-pending attachment identities; and
- an allocation cleanup-pending fence.

The new fields are part of the current pre-launch schema, with no legacy
reader/default shim. Restart reopens the same quarantine state. `segment_for`,
`segments_for`, `acquire`, and growth reject an allocation whose last/all
remaining holds are cleanup-pending. A partially draining tenant may retain
service through confirmed live siblings, but the quarantined attachment itself
cannot reactivate.

Filesystem-only orphan discovery no longer releases holds. Missing netns
evidence durably quarantines the matching attachment and keeps every covered
segment location unavailable. NNC5.2a supplies exact association and
attempt-before-effect evidence; NNC5.2b classifies desired intent, provider
attempts/effects, generations, manifests, and unknown inspection; NNC8.3 owns
repeated detach/release convergence.

## Sandbox Effect Ordering

Container and krun final teardown now:

1. quarantine the attachment hold;
2. stop the workload-scoped PEP and applicable machine proxies;
3. tear down the sandbox Netavark/provider effect;
4. remove the persistent network namespace;
5. release the quarantined attachment hold only if every prior deletion was
   confirmed;
6. delete all tenant bridge realizations returned for the last hold; and
7. finalize the exact stable segment identity/epoch only after every bridge
   deletion succeeds.

Errors are collected without pretending a cross-provider transaction exists.
Any quarantine, proxy, provider, namespace, bridge, or finalization error
returns failure and retains the appropriate durable fence. Plan-only stop paths
now release their planned segment holds through the same identity-fenced
contract, closing a pre-existing reservation leak.

## Failure, Restart, And Exactly-Once Proofs

Named tests prove:

- `release_without_durable_quarantine_fails_without_mutation`: an out-of-order
  hold release is rejected and authority bytes are unchanged;
- `cleanup_pending_survives_restart_and_reuses_only_after_fenced_finalize`:
  quarantine survives repeated store opens, prevents same-identity
  reactivation and location reuse, then permits reuse only after finalization;
- `wrong_or_stale_cleanup_fence_cannot_release_an_allocation`: a wrong segment
  ID and an old callback against a replacement fail without mutation;
- `concurrent_finalization_releases_one_identity_exactly_once`: two concurrent
  finalizers yield exactly one `Released` and one `AlreadyReleased`; subsequent
  allocations receive distinct locations and identities;
- `failed_bridge_cleanup_must_fence_segment_from_reuse`: failed provider cleanup
  retains the old location, a successful retry deletes/finalizes once, replay
  does not repeat provider deletion, and the location is handed out only once;
- orphan reconciliation preserves and quarantines a missing-netns hold instead
  of guessing that provider deletion completed; and
- container and krun recording substitutes observe
  `Quarantine -> Release -> FinalizeRelease` around confirmed teardown.

All existing refcount, multi-block, ABA, growth, disjointness, exhaustion,
orphan, stale-epoch, and thread-contention tests were converted or retained
without weakening assertions.

## Same-Process Authority-Lock Finding

The full sandbox suite exposed a real durability-store defect during this item:
separately opened store handles in one process could overlap atomic stage
replacement on platforms whose advisory file-lock semantics do not serialize
same-process descriptors. Under full-suite I/O load, legitimate fsync-backed
progress could also exceed the old two-second default wait budget.

The state store now composes:

- one canonical-path, process-shared mutex; and
- the existing cross-process OS file lock.

Symlink aliases resolve to the same process lock domain. The one deadline
covers both lock layers. The default remains bounded and fail-closed at 30
seconds; the explicit 50ms contention test still proves a held authority never
falls back to an unlocked read/mutation. Eight separately opened handles
concurrently preserve all eight updates, and the original allocator
acquire/quarantine/release/finalize contention test is stable in the full
suite.

This is one lock/transaction authority, not a second store. The
`nimbus-network -> nimbus-core` workspace dependency invariant is unchanged.

## Behavioral And Quality Evidence

Final affected suites:

```text
timeout 300 cargo test -p nimbus-network --all-features
# 63 passed; 0 failed; 0 ignored

timeout 900 cargo test -p nimbus-sandbox
# library: 267 passed; 0 failed; 11 ignored
# helper binary: 2 passed; 0 failed; 0 ignored
# macOS Linux-only integration targets: 0 executable cases
```

The 11 ignored library cases are named earlier/later NNC expected-red tests,
explicit scale characterization, and child-process roles. NNC2.5 removed the
NNC0.3 ignore and added no ignored test.

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

Dependency metadata reports the exact allowed workspace edge:

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

## Modularity

`nimbus-network/src/state_store.rs` (1,884 lines) and
`nimbus-sandbox/.../oci/network/segment.rs` (1,818 lines) remain inside the
repository's 1,500–1,999 explicit-justification band. The owning active plan
records the exception and the 2,000-line deletion gate. Both are deep,
concept-owned modules whose final sections are private invariant/failure-path
tests; neither is a composition root or generic helper bucket. Provider,
port, and orchestration behavior remain outside them.

## Worktree Isolation

The implementation remains in the dedicated owner worktree and branch. The
original checkout at `/Users/jack/src/github.com/nimbus/nimbus` still contains
exactly its four pre-existing user-owned paths:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No push or pull request was performed.

## Independent Review

The repository autoreview workflow reviewed the complete 103,720-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.7)
```

The reviewer independently traced multi-hold, double-release, restart,
concurrent-finalize, stale/ABA, backend failure, process/file-lock ordering,
poison recovery, canonical-path aliasing, dependency ownership, pre-launch
schema replacement, and the 30-second bounded lock budget. It reported no
landing-blocking defect.
