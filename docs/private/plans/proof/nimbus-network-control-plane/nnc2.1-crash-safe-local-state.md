# NNC2.1 Crash-Safe Local Network State Proof

Date: 2026-07-23

Status: `passed`

Source commit before the item:
`a16bd88e8083cd4fb77abf9aa93283c8a35e4738`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now owns the one node-local persistence and lock boundary for
portable network-resource authority:

```text
<state-root>/networks/control-plane/
  authority.lock  # one bounded cross-process lock domain
  state.json      # one versioned, checksummed latest-state envelope
```

The envelope has typed partitions, currently:

- `segment-allocations`; and
- `tenant-ipam/<TenantId>`.

Partition keys are JSON map keys, not filesystem paths. Segment allocation and
per-tenant IPAM therefore share one revision, checksum, lock, and durable
replacement boundary without allowing tenant input to select a path.

The old sandbox-owned `segments.json`, IPAM state-file, and IPAM lock-file
authorities are deleted. `nimbus-sandbox` no longer depends on `fs2`; it calls
the network-owned store while retaining Netavark, netns, nftables, gvproxy, and
all provider effects.

## Atomicity And Durability Contract

Every mutation performs this exact sequence while holding the one exclusive
authority lock:

```text
load + validate complete envelope
  -> mutate one typed partition in memory
  -> increment checked revision
  -> serialize body and SHA-256 checksum
  -> create owner-only same-directory stage
  -> write stage
  -> fsync stage
  -> atomic same-directory replace
  -> fsync parent directory
  -> release lock
```

The store publishes no partial closure result. Store failures and
concept-owned operation failures are distinct variants, so a caller cannot
downgrade corruption, lock timeout, serialization failure, or revision
exhaustion into a domain rejection.

The file is a bounded latest-state snapshot rather than an event log.
Cleanup-pending payloads are never compacted or aged by the store and survive
repeated restart; their concept owner must retain them until a fenced release
proof.

Startup runs under the same lock, removes only owned stage/probe leftovers,
exercises a real write/file-sync/replace/parent-sync probe, and validates any
existing envelope before returning authority.

## Exact Crash And Contention Proof

The upper-layer `nimbus-testing` integration uses the NNC0.1a/NNC0.1b real
process harnesses. `nimbus-network` has no normal or dev dependency on
`nimbus-testing`; `nimbus-testing` has a dev-only downward edge to the
feature-gated durability observer.

The crash child is killed only after acknowledging one exact event:

| Crash boundary | Fresh-process required recovery |
| --- | --- |
| `StateFileSynced` | previous complete owner |
| `StateReplaced` | new complete owner |
| `ParentDirectorySynced` | new complete owner |

A separate two-child test opens the same canonical root in two real processes,
releases both contenders through bounded semantic checkpoints, and proves
exactly one durable owner. The loser observes the winner rather than
overwriting it.

```text
timeout 600 cargo test -p nimbus-testing \
  --test network_state_store -- --test-threads=1

2 passed; 0 failed; 1 ignored
```

The ignored case is only the child-process entrypoint explicitly spawned by
the two parent tests.

## Corruption, Version, And Failure Proof

The envelope rejects:

- malformed or truncated JSON with the exact authority path;
- an unexpected format marker;
- an incompatible version as a distinct named error;
- any body whose deterministic checksum does not match;
- a typed partition whose payload schema is invalid;
- group- or world-readable authority state on Unix;
- revision overflow; and
- payload serialization failure.

The formerly expected-red segment and IPAM corruption tests are now ordinary
green tests. They tamper with the shared envelope and prove both torn bytes and
valid-looking changed payloads fail closed instead of reissuing a live segment
or address. Revision exhaustion preserves the authority byte-for-byte, and a
serialization failure publishes no authority file.

## Filesystem And Permission Contract

The host used for this proof was macOS arm64 on APFS. The supported contract is
a same-host local filesystem with:

- process-shared advisory locking;
- atomic same-directory replacement;
- durable file synchronization; and
- durable parent-directory synchronization.

Open classifies the nearest existing ancestor and the created store root,
rejects a mount-type change between them, and fails closed for detectable NFS,
SMB/CIFS, 9p, AFS, Coda, NCP, Ceph, and WebDAV families. Unknown Windows root
shapes, UNC roots, no-root results, and remote drives fail closed. Unsupported
non-Linux/non-Apple/non-BSD Unix targets fail closed rather than compiling an
invalid `statfs` field assumption.

Linux filesystem magic is normalized through `u32` before classification. A
regression constructs negative ILP32 CIFS and SMB2 words and proves they retain
their bit pattern and are rejected rather than sign-extending into the unknown
local fallback.

On Unix, recursive directory creation supplies mode `0700` at `mkdir` time,
then re-enforces it defensively. Authority and lock files are created and
re-enforced as `0600`. Startup refuses an authority file that remains visible
to group or other.

Windows replacement uses `MoveFileExW` with
`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`. Rust canonical verbatim
drive roots are normalized for `GetDriveTypeW`; UNC paths are classified
without lookup. The raw `ERROR_LOCK_VIOLATION` value returned by fs2 0.4.3's
immediate `LockFileEx` path is explicitly treated as bounded contention.

Cross-target `cargo check` attempts reached third-party `ring` and stopped
before compiling Nimbus source because this macOS host lacks
`aarch64-linux-gnu-gcc` and the Windows MSVC headers/toolchain. Those attempts
are environment limitations, not claimed target-build evidence. The
platform-independent Windows root/lock and ILP32 magic classifiers execute as
native unit tests; hosted CI remains the cross-target source of truth.

## Dependency And Ownership Proof

All-feature metadata reports exactly:

```json
{
  "package": "nimbus-network",
  "workspace_edges": ["nimbus-core:normal"]
}
```

There is no `nimbus-network -> nimbus-testing` edge. `fs2`, `serde_json`,
`libc`, and `windows-sys` are third-party implementation dependencies;
`nimbus-core` remains zero-I/O and `nimbus-runtime` remains untouched.

`SingleNodeSegmentAllocator` and IPAM now use the shared store, but the private
allocator trait and concrete caller accessors deliberately remain for NNC2.2.
The static verifier therefore stays truthfully expected-red only for:

- `NNCV005`, the NNC3 port allocation authorities; and
- `NNCV011`, the NNC2.2 allocator contract/concrete-accessor extraction.

Verifier evidence:

```text
timeout 900 bash scripts/verify-nimbus-network-control-plane.sh --self-test
# self-test: 15 passed, 0 failed

timeout 900 bash scripts/verify-nimbus-network-control-plane.sh
# 13 passed, 2 failed; exit 1 exactly as expected
```

No `segments.json`, `ipam_state_path`, or `ipam_lock_path` reference remains
under `nimbus-network` or `nimbus-sandbox`, and `nimbus-sandbox/Cargo.toml` no
longer declares `fs2`.

## Behavioral Verification

```text
timeout 600 cargo test -p nimbus-network --lib -- --test-threads=1
# 59 passed; 0 failed; 0 ignored

timeout 600 cargo test -p nimbus-testing \
  --test network_state_store -- --test-threads=1
# 2 passed; 0 failed; 1 ignored child entrypoint

timeout 900 cargo test -p nimbus-sandbox --lib --no-fail-fast \
  -- --test-threads=1
# 249 passed; 0 failed; 13 ignored

timeout 600 cargo test -p nimbus-testing --lib -- --test-threads=1
# 62 passed; 0 failed; 2 ignored child entrypoints

timeout 1200 cargo clippy \
  -p nimbus-network -p nimbus-sandbox -p nimbus-testing \
  --all-targets --all-features -- -D warnings
# exit 0; pre-existing vendored Brotli warnings remained non-fatal

cargo fmt --all --check
git diff --check
# exit 0
```

The 59 network tests cover atomic ordering, restart, sibling-partition
preservation, checksum/version/truncation failures, closure rejection,
serialization failure, revision exhaustion, permission rejection,
cleanup-pending retention, bounded lock timeout, stale stage/probe cleanup,
network-filesystem classification, Windows path/lock classification, and the
ILP32 CIFS/SMB2 regression in addition to all prior portable state tests.

## Modularity Exception

`crates/nimbus-network/src/state_store.rs` is 1,760 lines at this checkpoint,
which is inside the repository's 1,500-1,999 explicit-justification band.
Roughly the final third is concept-local invariant and failure-path tests. The
file remains one deep module—not a composition root—with one ownership story:
the node-local lock, envelope, durable replacement recipe, platform
classification, error boundary, and tests that access its private crash
events. Keeping those invariants together makes the atomicity and fail-closed
contract locally reviewable; there is no generic helper/common bucket or
second responsibility hidden in it. New provider, allocator, port, or
orchestration logic must live in concept-owned modules rather than extending
this store.

## Independent Review And Disposition

Structured Claude Opus 4.8 maximum-reasoning reviews were run against the
complete local diff. Accepted findings were fixed rather than waived:

1. startup now removes stale durability-probe stages and destinations;
2. Windows verbatim drive paths are normalized while UNC/unknown roots fail
   closed;
3. fs2's raw Windows `ERROR_LOCK_VIOLATION` enters the bounded retry path;
4. non-Linux Unix `statfs` access is limited to targets with the required
   field and other Unix targets fail closed;
5. recursive Unix directory creation applies `0700` at creation time;
6. insecure-permission, revision-exhaustion, and serialization failures have
   exact tests;
7. 32-bit Linux CIFS/SMB2 magic cannot sign-extend past rejection; and
8. Unix-only permission constants are not compiled as unused Windows items.

The final recoverable convergence run reported no accepted/actionable finding,
rated the patch correct at `0.80`, and explicitly verified the Windows cfg
cleanup. Its preceding convergence run also rated the patch correct at `0.80`
and verified the ILP32 fix and regression.

One earlier suggestion to change `single_node_default` was rejected with
source evidence: that helper is `#[cfg(test)]`; every production constructor
propagates the store-open `Result`. The comment was strengthened so it cannot
be mistaken for a production error path.

## Scope Guard

NNC2.1 does not:

- move or publicize the allocator trait;
- replace `SandboxId` with `NetworkAttachmentId`;
- change secondary-block selection or lease-expiry cleanup semantics;
- implement two-phase provider detach/release quarantine;
- allocate host ports;
- perform Netavark, nftables, gvproxy, socket, proxy, DNS, or cluster effects;
  or
- alter tenant admission, service naming, compute choreography, or system
  projections.

Those remain ordered under NNC2.2-NNC9.
