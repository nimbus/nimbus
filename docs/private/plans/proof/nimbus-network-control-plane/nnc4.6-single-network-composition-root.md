# NNC4.6 Single Network Composition Root

Date: 2026-07-28

Status: `complete`

Source commit before the item:
`3d4f5ec107663285730eb355be35ff7ba127b5a3`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result Required

NNC4.6 introduces one concrete, transport-free process composition Module:
`LocalNetworkManager`. Its Interface owns exactly one process claim, one
`LocalNetworkStateStore`, one `LocalPortLeaseAuthority` derived from that
store, and one immutable `NetworkCapabilityRegistry`.

This is a façade allowed by Binding Architecture Decision 3, not a
`NetworkProvider` interface. It adds no socket, proxy, policy, forwarding,
Netavark, nftables, gvproxy, Iroh, cluster, cloud, or workload orchestration
implementation. `nimbus-network -> nimbus-core` remains the only workspace
edge.

The manager creates Locality around process composition and gives later
consumers one high-Leverage handle. The underlying store and lease Modules
remain deep multi-handle primitives: deleting their independent-open behavior
would break legitimate transaction, recovery, runner, and cross-process
Adapters. Their existing canonical process mutex plus OS file lock is the
durable authority; the manager claim prevents a second independent
composition, not a second safe handle.

## Audit Finding

The read-only constructor/root audit found:

1. There is no production process network manager.
2. `NetworkCapabilityRegistry` is immutable and well scoped, but production
   currently constructs no complete registry.
3. Repeated same-root `LocalNetworkStateStore::open` and
   `LocalPortLeaseAuthority::open` calls are safe. Existing tests require
   independent handles, symlink aliases, threads, and processes to serialize
   through one store/lock domain.
4. The production root topology is not unified:

   ```text
   nimbus start
     -> server listener leases
        -> <compose-control-data-dir>/networks/control-plane/state.json

     -> compose krun sandbox
        -> <compose-control-data-dir>/services/projects/<project>/
           backends/krun/state/networks/control-plane/state.json
   ```

   Segment allocation, IPAM, PEP, and published sandbox ports follow the
   backend root. Server listeners follow the node control root. They therefore
   can reserve against different durable authorities on one logical node.
5. Container and krun `state_root` currently conflate backend artifacts with
   node-global network authority. Moving the whole root would move the wrong
   ownership. NNC4.6a owns the explicit split.
6. Two unrelated processes configured with genuinely different roots have no
   rendezvous. NNC4.6 does not pretend otherwise. NNC4.6b makes one explicit
   logical-node root a process configuration invariant, and NNC4.6c closes the
   source-derived census. A different explicit root denotes a different
   logical node.

Because manager ownership, backend schema/root fencing, production wiring, and
mechanical closure are independently reviewable units of value, the original
NNC4.6 item was prospectively split before source implementation:

| Item | Unit of value |
| --- | --- |
| NNC4.6 | Process-owned manager, shared handles, typed duplicate diagnostics, and real-process validity. |
| NNC4.6a | Container/krun artifact-root versus node-network-root separation and runner fencing. |
| NNC4.6b | Production process composition and capability-registry wiring. |
| NNC4.6c | Complete constructor/root census and verifier closure. |

This prevents partial autoreview chunks from becoming ad hoc task units. Each
canonical item receives its own acceptance proof and exactly one candidate-
frozen structured review.

## Frozen NNC4.6 Acceptance

| ID | Verifiable success criterion |
| --- | --- |
| M1 | The first `LocalNetworkManager` owns one process composition and exposes the exact immutable capability registry supplied at construction. |
| M2 | Its state-store and port-lease handles derive from the same opened store; a sibling-partition mutation cannot be lost. |
| M3 | A second independent manager for the same direct root returns a typed duplicate-composition error. |
| M4 | An existing symlink or lexical alias of the active root is diagnosed as the same canonical authority. |
| M5 | A second independent manager for a genuinely different root also fails before creating or mutating the attempted authority. The error names active and attempted authority paths and instructs the caller to clone/inject the active manager. |
| M6 | Deliberate reuse is `Arc<LocalNetworkManager>` cloning. Dropping a non-final clone retains the process claim. |
| M7 | Dropping the final clone releases only the process claim; deterministic reopen preserves durable lease state. |
| M8 | A constructor/store failure leaves no stale claim and permits a later valid construction. |
| M9 | Concurrent constructors have exactly one winner and one typed loser without sleep-based coordination. |
| M10 | Two fresh OS processes may each own a manager over the same root; the existing real-process port-lease harness still proves one durable winner and no conflicting active lease. |
| M11 | Raw `LocalNetworkStateStore` and `LocalPortLeaseAuthority` handles remain legal primitives and retain their existing same-process, alias, crash, and recovery proofs. |
| M12 | Metadata, source scans, and tests prove the manager adds no effect implementation or workspace edge beyond `nimbus-core`. |

## Fail-Before Evidence

Before implementing manager ownership:

1. add `crates/nimbus-network/tests/local_network_manager.rs` against M1-M9;
2. run:

   ```text
   timeout 600 cargo test -p nimbus-network \
     --test local_network_manager --no-run
   ```

3. record exit `101` caused only by the missing manager/error/shared-handle
   Interface;
4. do not manufacture a behavioral failure by making primitive
   `LocalNetworkStateStore::open` fail.

The command exited `101` exactly as required:

```text
error[E0432]: unresolved imports
  nimbus_network::LocalNetworkManager
  nimbus_network::LocalNetworkManagerError
```

No other compile or behavioral failure was present.

The implementation converts the same contract to green. A
single table-driven or explicitly serialized process-claim test must prevent
parallel test cases from competing accidentally.

## Proof Matrix

| Proof | Result |
| --- | --- |
| Manager contract integration test | `1 passed; 0 failed; 0 ignored`; M1-M9 all pass. |
| `nimbus-testing --test network_port_lease` | Manager-backed child roles: `6 passed; 0 failed; 2 ignored`. |
| `nimbus-testing --test network_state_store` | Raw-store positive control: `2 passed; 0 failed; 1 ignored`. |
| Full `nimbus-network` suite | `199 passed; 0 failed; 0 ignored`, including unit, five integration, and doc-test binaries. |
| Affected check/Clippy/rustdoc | `cargo check` and strict `cargo clippy --no-deps -- -D warnings` pass for `nimbus-network` and `nimbus-testing`; warning-denied `nimbus-network` rustdoc passes. |
| Dependency/effect scan | Cargo metadata reports exactly `nimbus-network -> nimbus-core`; static verifier NNCV012 and the full live verifier pass. |
| Formatting/diff/docs gates | `cargo fmt --all --check` and `git diff --check` pass; docs are `108` pages link-clean; site validation is `17/17`. |

## Acceptance Reconciliation

| Criterion | Evidence and disposition |
| --- | --- |
| M1 | The first manager stores and exposes the supplied immutable, empty fail-closed registry. Green. |
| M2 | The manager constructs `LocalPortLeaseAuthority` from the same opened store; the focused test commits a sibling IPAM partition and proves the reserved port remains. Green. |
| M3 | Direct same-root duplicate construction returns `DuplicateProcessComposition`. Green. |
| M4 | Lexical `.`, Unix symlink, and relative-path-with-missing-current-directory cases retain the typed duplicate diagnostic. Green. |
| M5 | A divergent root returns active and attempted paths plus clone/inject guidance before the attempted root exists. Diagnostic formatting is best-effort and cannot replace the known duplicate result. Green. |
| M6 | `Arc::clone` is pointer-identical reuse and a non-final drop retains the claim. Green. |
| M7 | Final drop permits deterministic reopen and the durable lease survives. Green. |
| M8 | Opening through a file fails without retaining a process claim; a later valid open succeeds. Green. |
| M9 | A barrier-coordinated pair produces exactly one manager and one typed duplicate without sleeps. Green. |
| M10 | The upgraded subprocess child roles each own their process manager over one root; the real-process exact/range/crash proofs remain `6/0/2`. Green. |
| M11 | Raw store and port handles remain public and independently openable; unchanged raw-store process proof is `2/0/1`, while the full network suite preserves existing same-process and alias coverage. Green. |
| M12 | Cargo metadata reports only `nimbus-core`; live verifier is `15/15`, self-test is `45/45`, and no provider effect entered the new manager Module. Green. |

## Commands And Exact Evidence

```text
timeout 600 cargo test -p nimbus-network \
  --test local_network_manager -- --nocapture
=> 1 passed; 0 failed; 0 ignored

cargo test -p nimbus-testing \
  --test network_port_lease -- --test-threads=1
=> 6 passed; 0 failed; 2 ignored

cargo test -p nimbus-testing \
  --test network_state_store -- --test-threads=1
=> 2 passed; 0 failed; 1 ignored

timeout 900 cargo test -p nimbus-network \
  --all-features -- --test-threads=1
=> 199 passed; 0 failed; 0 ignored

timeout 900 cargo check \
  -p nimbus-network -p nimbus-testing --all-targets --all-features
=> pass

timeout 1200 cargo clippy \
  -p nimbus-network -p nimbus-testing \
  --all-targets --all-features --no-deps -- -D warnings
=> pass

RUSTDOCFLAGS='-D warnings' timeout 600 cargo doc \
  -p nimbus-network --no-deps --all-features
=> pass

cargo metadata --format-version 1 --no-deps
=> sole nimbus-network workspace dependency: nimbus-core

timeout 900 bash scripts/verify-nimbus-network-control-plane.sh
=> 15 passed; 0 failed

timeout 900 bash scripts/verify-nimbus-network-control-plane.sh --self-test
=> 45 passed; 0 failed

cargo fmt --all --check
git diff --check
=> pass

bash scripts/check-docs.sh
=> 108 pages link-clean

bash scripts/verify-nimbus-docs-site.sh
=> 17/17 conditions green
```

The initial combined check/Clippy/rustdoc shell invocation placed
`RUSTDOCFLAGS` after `timeout`, so only that rustdoc command invocation exited
`127`. The correctly ordered warning-denied rustdoc command above then passed;
this was an invocation error, not a source or documentation failure.

## Structured Review And Correction

The one full item-level structured review ran after M1-M12 and all gates were
green:

```text
autoreview --mode local --engine codex \
  --model gpt-5.6-sol --thinking xhigh --codex-speed fast
=> one pass; actual model gpt-5.6-sol; xhigh; service_tier="fast"
=> one accepted P2 finding at confidence 0.93
```

The accepted finding was concrete and inside M3-M5: after an active manager
existed, a second construction with a relative attempted root could let
`current_dir()` failure escape as `LocalNetworkManagerError::Store`, replacing
the required `DuplicateProcessComposition`.

A deterministic Unix regression case entered a directory, removed it, and
then attempted the relative duplicate construction. Before the correction:

```text
cargo test -p nimbus-network --test local_network_manager -- --nocapture
=> 0 passed; 1 failed; 0 ignored
=> expected DuplicateProcessComposition, got Store("No such file or directory")
```

The narrow correction made attempted-authority rendering infallible and
best-effort: it retains the relative path if the current directory cannot be
resolved and still canonicalizes when possible. It performs no attempted-root
mutation. The test restores the process-wide current directory explicitly and
also has a drop fallback.

Affected proofs after the correction:

```text
manager contract => 1 passed; 0 failed; 0 ignored
full nimbus-network => 199 passed; 0 failed; 0 ignored
nimbus-network check => pass
nimbus-network strict Clippy => pass
nimbus-network warning-denied rustdoc => pass
format and diff checks => pass
```

Exactly one narrow correction review then ran with the same actual
`gpt-5.6-sol`, `xhigh`, fast settings and the accepted defect as its sole
focus:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.99)
```

No further structured review is warranted: all subsequent closeout changes
are ledger/proof wording and mechanical verification only.

## Candidate-Frozen Review Scope

- Request and item: canonical NNC4.6 only, M1-M12 above.
- Base: committed NNC4.5 checkpoint
  `3d4f5ec107663285730eb355be35ff7ba127b5a3`.
- Owner boundary: portable process composition inside `nimbus-network`, plus
  the existing real-process port proof adapter in `nimbus-testing`.
- Executable paths: `lib.rs`, new `manager.rs`, the smallest
  `LocalPortLeaseAuthority::from_store` hook, new focused manager test, and
  manager-backed port child roles.
- Explicitly deferred canonical items: backend root separation (NNC4.6a),
  production root/capability wiring (NNC4.6b), and mechanical census closure
  (NNC4.6c).
- Forbidden expansion: provider effects, sockets, policy, transport, workload
  orchestration, compatibility shims, or a speculative provider interface.

## Modularity Constraints

- Add the manager in a new concept-owned module. Do not grow
  `state_store.rs`, currently 1,957 lines.
- Do not add manager logic to `port_lease.rs`, currently 1,808 lines.
- If canonical authority identity must be shared, extract one concept-owned
  local-root child rather than adding another switchboard.
- The manager is concrete. No new provider trait is earned here.
- `Arc` is the reuse Interface; do not add a compatibility shim or silent
  singleton lookup.
- Failed duplicate construction performs no attempted-root filesystem effect.
- Existing raw-handle tests remain positive controls, not exceptions to hide.

## Review Cadence

No structured review runs during fail-before, implementation, cleanup, or
acceptance convergence. After M1-M12 and every affected gate are green on one
candidate-frozen diff, run exactly one GPT-5.6 Sol, xhigh, fast structured
review for NNC4.6. Only an accepted material executable finding permits one
narrow correction review after its affected proofs rerun.

## Current Checkpoint

| Field | Value |
| --- | --- |
| Owned paths | Canonical plan/routing/proof; manager Module/export/test; smallest port-authority-from-store hook; manager-backed subprocess port proof. |
| Source edits | New manager Module/export/test; smallest `LocalPortLeaseAuthority::from_store` hook; real-process port child roles now open through the manager. |
| Last green | M1-M12; manager 1/0/0; manager-backed port 6/0/2; raw store 2/0/1; full network 199/0/0; affected check/Clippy/rustdoc; exact core-only edge; verifier 15/15 plus self-test 45/45; format/diff; docs 108 pages; site 17/17; accepted P2 correction proof; narrow correction review clean at 0.99. |
| Review disposition | Full Sol/xhigh/fast item review accepted one M3-M5 P2; deterministic fail-before reproduced it; the narrow correction is proven and its one permitted correction review is clean. No rejected or unresolved findings. |
| Next action | Commit the exact NNC4.6 item, then begin NNC4.6a's read-only backend-root substitution audit. |
| Blocker | None. |
