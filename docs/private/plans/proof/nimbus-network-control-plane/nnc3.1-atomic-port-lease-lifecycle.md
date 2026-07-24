# NNC3.1 Atomic Port-Lease Lifecycle Proof

Date: 2026-07-24

Status: `passed`

Source commit:
`b9e6caa6d4011463ce910c89f165b0b2031fa47b`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now owns one crash-safe, host-global port-lease authority over
the same `LocalNetworkStateStore` and cross-process lock domain as segment and
IPAM state. The first lifecycle contract provides:

```text
reserve -> adopt -> activate -> withdraw -> release
```

Every transition is one store transaction. The immutable request carries a
stable `PortLeaseId`, stable `NetworkResourceId` owner, optional tenant
attribution, desired generation, lease epoch, and exact non-zero requested
port. Adoption records an opaque provider handle but performs no provider
effect. Full-request comparison rejects stale or divergent owners, tenants,
ports, generations, and epochs without mutation.

The NNC3.1 conflict domain is deliberately conservative: every non-terminal
lease for an exact port conflicts because protocol, address family, address
specificity, bind realm, range, and provider-assigned modes do not exist until
NNC3.2. Production bind owners remain unmigrated until their later named NNC3
items. This checkpoint therefore creates one authority without introducing a
second production authority.

## Fail-Before

The integration proof was written before the public authority contract:

```text
timeout 300 cargo test -p nimbus-testing --test network_port_lease --no-run
```

It exited `101`. Rust reported unresolved `nimbus_network` imports for
`LocalPortLeaseAuthority`, `PortLeaseError`, and `PortLeaseRequest`. No
production implementation could satisfy the two-process contention test.

The existing NNC0.2 baseline remains the historical production fail-before:
independent sandbox and PEP planners can both claim port `41337`. NNC3.1 does
not mark that baseline green because production owners migrate in NNC3.4
through NNC3.7b and their obsolete authorities are deleted in NNC3.9.

## Atomic Lifecycle And Fencing

`PortLeasePhase` distinguishes:

- `Reserved`, `Binding`, `Active`, and `Withdrawing`;
- `CleanupPending`, reserved for later ambiguous provider cleanup;
- terminal `Released` and `Failed`.

All non-terminal records retain the conflict fence. A different stable lease
can reuse an exact port only after confirmed terminal release. Replays of the
same transition are idempotent; invalid phase changes fail without publishing
partial state. A boxed `PortLeaseFenceMismatch` preserves complete
expected/candidate diagnostics without inflating every public `Result` error
path.

Provider effects remain outside `nimbus-network`:

- `reserve` records authority but does not probe or bind;
- `adopt` records exact binding evidence and its opaque provider handle;
- `activate` permits later publication only after adoption;
- `withdraw` fences new use;
- `release` records confirmed completion but never performs unbind.

NNC3.3 owns real bind/adoption, NNC3.8 owns cleanup-pending reconciliation, and
NNC3.9 owns deletion of legacy allocators and probe/drop decision paths.

## Durable-State Validation

The port payload is stored under the `port-leases` partition inside
`networks/control-plane/state.json`; it does not create another file, lock, or
commit boundary. Authority startup and every read/transaction validate:

1. map key equals the record's stable lease identity;
2. `Reserved` has no provider binding;
3. `Binding` and `Active` have binding evidence;
4. terminal `Failed` retains no provider effect;
5. exact requested and actual ports agree; and
6. no two non-terminal records fence the same exact port.

Tests use the raw store only to write five checksum-valid but semantically
impossible payloads. Every authority open rejects those payloads as
`CorruptAuthority`; a valid checksum can never promote impossible lifecycle
state to trusted authority.

## Thread And Real-Process Contention

The unit proof opens two independent authority handles, releases two threads
through one barrier, and has each contender attempt the entire
reserve/adopt/activate sequence. Exactly one returns `Active`; the other gets a
structured `PortConflict`; restart exposes exactly one durable active record.
The test uses no timing sleep.

The `nimbus-testing` integration proof reuses
`TwoProcessContentionHarness`. Two real child test processes open the same
state root, request the same exact host port under distinct stable listener and
lease identities, and race through reserve/adopt/activate. The parent proves:

- one process reports `Won` and one reports `Lost`;
- the reopened authority contains exactly one record;
- its stable owner matches the harness winner; and
- its durable phase is `Active`.

`nimbus-testing` remains an upper-layer harness consumer. No
`nimbus-network -> nimbus-testing` normal or dev dependency was added.

## Recovery-Ledger Defect And Verifier Repair

While recording this proof, `git rev-parse HEAD` exposed that the Recovery
Header named nonexistent commit
`b9e6caa6d28209dc2d21dbe3b333b82168edda21`; the real NNC2.8 checkpoint is:

```text
b9e6caa6d4011463ce910c89f165b0b2031fa47b
```

`git cat-file -t` failed for the recorded value, yet the aggregate verifier
reported `PASS NNCV008` because it checked only for the words “Last green.”
The ledger was corrected and NNCV008 was deepened to require exactly one
40-hex Recovery Header checkpoint and prove that it resolves to a commit.
A new self-test replaces the checkpoint with forty zeroes and proves exclusive
NNCV008 failure. The self-test suite advances from 15 to 16 conditions.

This closes the compaction-recovery defect instead of preserving a
plausible-looking but unusable recovery anchor.

## Quality-Gate Finding And Fix

The first strict Clippy run rejected the initial rich `StaleFence` enum variant
with `clippy::result_large_err`: it made the largest error at least 172 bytes.
The diagnostics were retained in boxed `PortLeaseFenceMismatch`, which carries
the complete durable expected request and rejected candidate request.
The rerun is clean without lint suppression.

## Verification

```text
cargo test -p nimbus-network --all-features -- --test-threads=1
# 69 passed; 0 failed; 0 ignored
# doc tests: 0 failed

cargo test -p nimbus-testing --test network_port_lease -- --test-threads=1
# 1 passed; 0 failed; 1 ignored child entrypoint

cargo check -p nimbus-network -p nimbus-testing --all-targets --all-features
# exit 0

cargo clippy -p nimbus-network -p nimbus-testing \
  --all-targets --all-features -- -D warnings
# exit 0; only existing vendored Brotli warnings

cargo doc -p nimbus-network --all-features --no-deps
# exit 0

cargo metadata --format-version 1 --no-deps
# nimbus-network workspace edges:
[{"name":"nimbus-core","kind":null}]

cargo fmt --all --check
git diff --check
# both exit 0

bash -n scripts/verify-nimbus-network-control-plane.sh
shellcheck -s bash scripts/verify-nimbus-network-control-plane.sh
# both exit 0

bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 16 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
# only NNCV005, whose production-owner migration remains NNC3.4-NNC3.9

bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

No live external networking provider was required or claimed for this portable
state-machine item. NNC3.3 and later provider-owner items carry the real socket
and adoption proof obligations.

## Worktree Isolation

The implementation was performed only in the dedicated owner worktree and
branch. The original checkout retained its four pre-existing user-owned paths;
no push or pull request was performed.

## Independent Review

The repository autoreview workflow reviewed the complete 100,121-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.8)
```

The reviewer independently checked transition atomicity, pre/post semantic
validation, terminal reuse, full-request fencing, thread/process contention,
dependency direction, provider-effect ownership, test assertions, and the
NNCV008 Git-object check. It reported no correctness, security, atomicity, or
dependency-layering defect requiring a fix.
