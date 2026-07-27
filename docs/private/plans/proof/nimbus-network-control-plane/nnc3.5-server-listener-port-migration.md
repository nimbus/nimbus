# NNC3.5 Server Listener Port Migration Proof

Date: 2026-07-27

Status: `complete`

Starting checkpoint:
`5ddee8a1def0062e6bfa84e77b0ef54224eba264`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

The CLI-owned main HTTP/WebSocket bind and the server-owned sibling MongoDB,
DynamoDB, and S3 wire binds now consume the one crash-safe, cross-process
`LocalPortLeaseAuthority`. The socket effects and protocol bytes stay in
`nimbus-cli` and `nimbus-server`; `nimbus-network` still owns only portable
identity, desired binding vocabulary, durable lease state, conflict
authority, and provider evidence.

The migration has one ports-and-adapters boundary:

1. `ServeOptions` creates one process-incarnation listener authority rooted at
   the engine data directory, or at the host-global network state root supplied
   by the CLI composition root;
2. a stable address-free `ListenerId` and `PortLeaseId` derive from that
   process incarnation plus the logical main or wire-listener name;
3. generation `1` and lease epoch `1` are scoped to the fresh incarnation, so
   another server start receives distinct stable IDs without using an IP
   address or numeric port as identity;
4. every Nimbus-owned bind reserves and creates an attempt-unique durable
   `PortBindClaim` before the kernel effect;
5. a failed real bind records terminal no-effect evidence, including the exact
   attempted address shape and `AddrInUse` classification;
6. a successful bind adopts its concrete non-zero endpoint and exact
   provenance, then activates the lease before serving or publishing observed
   listener state; and
7. a confirmed normal Nimbus-owned socket close withdraws and releases its
   exact lease; closing only Nimbus's descriptor for an externally owned
   listener withdraws but retains its external fence; and ambiguous task
   cancellation retains the Active fence for NNC3.8 reconciliation.

No socket, TLS, Axum, router, MongoDB, DynamoDB, S3, systemd, or provider effect
entered `nimbus-network`.

## Ownership And Lifecycle

### Main listener

The ordinary `nimbus start` path preserves CLI ownership of hostname
resolution and `tokio::net::TcpListener::bind`. For each resolved candidate it
now performs:

```text
prepare/reserve/claim -> bind -> adopt/activate -> discovery/startup output
                       \-> durable no-effect failure
```

Port zero remains `ProviderAssigned`; a non-zero request remains `Exact`.
Resolution may try another address only after the failed attempt has truthful
durable evidence. The actual bound endpoint comes from `local_addr()` and is
not inferred from the request.

Systemd activation remains an inherited external effect. The CLI validates and
takes the supplied descriptor as before, then `ServeOptions` records
`ExternallyOwned` provenance before serving. The public `serve` embedder seam
does the same for any already-bound listener. Nimbus-owned callers instead use
`prepare_main_listener` followed by `serve_leased`.

`serve_leased` rejects a listener created by another `ServeOptions`
incarnation. Because that error proves Nimbus's local descriptor is being
closed without serving, it settles the original lease according to provenance:
Nimbus-owned bindings reach `Released`, while external bindings stop at
`Withdrawing` until their owner supplies release evidence. The regression
tests prove both outcomes rather than leaking or prematurely releasing
authority.

### Sibling wire listeners

`nimbus-server` retains each sibling bind and its existing adapter guard. The
sequence is now:

```text
prepare/reserve/claim
  -> bind
  -> guard while Reserved + claimed
  -> adopt/activate
  -> record observed system projection
  -> spawn unchanged adapter protocol tasks
```

A guard rejection first drops the real listener, then abandons the exact bind
claim and releases the proven no-effect lease. A bind rejection records the
original I/O failure against that claim. The system projection is downstream
observation, never desired state or allocation authority.

On a normal main-server return, sibling tasks are aborted and joined before
their leases are withdrawn/released, followed by the main lease. The
pre-existing kth-sibling partial-start unwind baseline remains intentionally
owned by NNC7.1a's structured listener group. Fresh-process reconciliation of
Active or claimed records remains NNC3.8-owned. This item neither hides nor
duplicates those later authorities.

### Concept-owned module

`crates/nimbus-server/src/listener_lease.rs` is the effect adapter between
portable durable authority and Tokio listener observations. It contains the
claim/adopt/failure translation, binding-target and exposure normalization,
provenance mapping, and confirmed-close release behavior. The server
composition root only sequences those capabilities around its existing binds.

Final source-derived sizes are below the repository's 1,500-line threshold:

| Owner | Lines |
| --- | ---: |
| `crates/nimbus-server/src/listener_lease.rs` | 695 |
| `crates/nimbus-server/src/construction.rs` | 884 |
| `crates/nimbus-cli/src/start/boot.rs` | 1,070 |

## Source Census

The NNC0.1 machine-readable inventory assigns exactly four relevant entries:

| Inventory ID | NNC3.5 disposition |
| --- | --- |
| `cli-main-direct-listener` | CLI keeps the bind; composition prepares and activates its lease. |
| `cli-main-systemd-listener` | Existing NNC3.3 external adoption records `ExternallyOwned`. |
| `server-sibling-wire-listeners` | Server keeps each bind; each consumes one claimed lease before its unchanged guard/protocol task. |
| `server-main-listener-consumer` | Server accepts only an Active generation-scoped leased listener, or explicitly adopts an external one. |

The live NNCV006 source classifier passes. Its production results identify only
the CLI main bind and the server sibling bind in this scope, and both are
immediately adjacent to the claim-first adapter call. Other raw bind matches
under these crates are test fixtures or the later-owned CLI dev, start-adapter,
and machine probe/drop sites; NNC3.7, NNC3.7a, and NNC3.9 retain those
dispositions.

## Fail-Before Evidence

Both acceptance regressions were installed before the production migration.
Each exact command exited `101` at the intended new invariant:

```text
cargo test -p nimbus-server \
  construction::tests::nnc3_5_main_listener_is_active_in_port_authority_while_serving \
  -- --nocapture

NNC3.5: the main server accepted a listener without an Active durable port lease
```

```text
cargo test -p nimbus-server \
  construction::tests::nnc3_5_sibling_bind_is_claimed_before_guard_and_serves_identical_bytes \
  -- --nocapture

NNC3.5: the sibling kernel bind reached its guard without an exact durable claim
```

The corrected focused run passes all four `nnc3_5_` cases, including the
owner-incarnation and synchronous sibling-failure cleanup paths:

```text
test result: ok. 4 passed; 0 failed
```

## Acceptance Matrix

| Written criterion | Executable evidence | Result |
| --- | --- | --- |
| Existing main listener behavior remains available while every production main bind owns or adopts a lease. | `nnc3_5_main_listener_is_active_in_port_authority_while_serving`; CLI provider-assigned activation test; external-adoption provenance test; complete CLI suite. | Pass |
| Existing sibling guard ordering is preserved and protocol bytes are identical. | `nnc3_5_sibling_bind_is_claimed_before_guard_and_serves_identical_bytes` observes a Reserved claimed record inside the unchanged guard and reads the exact `lease-owned` bytes from the spawned adapter. Existing MongoDB, DynamoDB, S3, TLS, HTTP, and WebSocket tests pass in the complete server suite. | Pass |
| Success, collision, ownership mismatch, and confirmed close have durable, fenced outcomes. | Provider-assigned bind reaches `Active` with the exact actual port then `Released`; a real external collision reaches `Failed/AddrInUse`; external listeners record `ExternallyOwned` and retain their fence at `Withdrawing` after Nimbus closes only its descriptor; cross-incarnation consumption settles according to provenance. | Pass |

Written NNC3.5 acceptance: **3/3 pass**.

## Behavioral Verification

Focused ownership tests:

```text
cargo test -p nimbus-server nnc3_5_ -- --nocapture
4 passed, 0 failed

cargo test -p nimbus-server listener_lease::tests:: -- --nocapture
6 passed, 0 failed

cargo test -p nimbus-cli start::boot::listener_tests:: -- --nocapture
6 passed, 0 failed

cargo test -p nimbus-server \
  cleanup_error_is_aggregated_without_hiding_the_primary_failure -- --nocapture
1 passed, 0 failed
```

Complete affected suites at the final production bytes:

```text
cargo nextest run -p nimbus-cli
862 passed, 0 failed, 2 skipped

cargo nextest run -p nimbus-server \
  --filter-expr \
  'not test(tests::local_server_security::deploy_admin_requires_local_admin_header_even_with_deploy_bearer) & not test(tests::runtime_owner_conformance::cloud_functions_passes_runtime_owner_lifecycle_conformance)'
584 passed, 0 failed, 28 skipped
```

The disjoint final affected result is **1,446/1,446 passed**, with 30 expected
skips.

Before the final one-test cleanup, an unfiltered combined run executed 1,436
tests and reported 1,434 passes plus exactly these two failures:

- `deploy_admin_requires_local_admin_header_even_with_deploy_bearer` expected
  HTTP 200 and received 400;
- `cloud_functions_passes_runtime_owner_lifecycle_conformance` expected HTTP
  200 and received 409.

Both reproduce individually and are the exact pre-existing server baselines
already recorded in
`nnc0.7-orphan-listener-baselines.md`. They do not enter listener construction
or port authority. They remain explicit baseline exclusions; no assertion,
filter, or implementation was changed to make them green. The final correction
tests are included in the 584/584 server result above.

## Architecture And Quality Verification

Completed gates:

```text
cargo check -p nimbus-server -p nimbus-cli --all-targets
exit 0

cargo clippy -p nimbus-server -p nimbus-cli --all-targets --no-deps -- -D warnings
exit 0

cargo clippy -p nimbus-server --all-targets --no-deps -- -D warnings
exit 0 after the final error-path cleanup

RUSTDOCFLAGS='-D warnings' cargo doc --no-deps \
  -p nimbus-server -p nimbus-cli
exit 0
```

Warnings emitted by those commands are confined to unchanged vendored Brotli
sources; both affected crates are warning-clean under `-D warnings`.

Dependency and effect proofs:

- `cargo tree -p nimbus-network --edges normal` shows `nimbus-core` as its
  only Nimbus workspace dependency;
- NNCV004, NNCV007, and NNCV012 pass, proving the dependency contract,
  profile acyclicity, and forbidden dependency/effect scan;
- NNCV006 passes, proving no new unclassified production bind entered;
- the verifier self-test passes 16/16;
- the live verifier passes 14 conditions and fails only NNCV005 at the
  later-owned CLI dev/machine/legacy sandbox allocation authorities scheduled
  for NNC3.7a/NNC3.9.

Final format, diff, documentation, review disposition, and ledger results are
recorded below.

## Structured Review

The first frozen candidate was tree
`d802f126da9e60c7c0152a28f5c2ce4dac6e7e92`, represented by synthetic commit
`8bae395a8479939990d2a89ed20a11b36ad2d975`. The structured review completed
with actual `gpt-5.6-sol`, `xhigh` reasoning, and fast mode in review thread
`019fa4af-910b-73a1-9913-3015ce2a7f92`. Its six findings were all accepted:

| Finding | Disposition and proof |
| --- | --- |
| P1: external/systemd local descriptor close released external authority | Preserve provenance in `ActiveServerListenerLease`; an external close now withdraws only. The external-adoption test proves `Withdrawing` plus retained `ExternallyOwned` evidence. |
| P1: synchronous server setup failure stranded the main Active lease | All pre-serve returns now converge through confirmed-local-close settlement. `nnc3_5_synchronous_sibling_failure_closes_and_releases_owned_main_listener` proves the main port is reusable and its record is `Released`. |
| P1: discovery setup failure stranded the CLI-owned Active listener | CLI startup explicitly settles after `local_addr` or discovery-acquisition failure. `startup_error_closes_and_releases_cli_owned_listener` proves durable release and kernel reuse. |
| P2: sibling shutdown awaited one task before aborting the rest | Normal shutdown aborts every sibling in one pass before awaiting any task. Atomic partial-start unwind remains explicitly NNC7.1a-owned. |
| P2: failed durable bind receipt was treated as candidate collision | `RecordedListenerBindFailure` distinguishes a durable no-effect receipt from authority failure; hostname fallback stops on the latter. Both corrupt-store adapter and CLI fallback tests prove the branch. |
| P2: cleanup failures disappeared behind a serving error | `append_cleanup_error` preserves the primary error kind and aggregates every settlement failure. Its focused regression proves both diagnostics remain visible. |

Those accepted findings materially changed the lifecycle implementation, so
the plan permitted exactly one bounded repeat review. It ran with actual
`gpt-5.6-sol`, `xhigh` reasoning, and fast mode against tree
`66f3ddba509b043afa6e42dcb4f5075fbf1ab8e8`, represented by synthetic commit
`92c9408c1f7c98eca8d202080db8fb7e84dc2f7e`, in thread
`019fa4c9-a825-7171-ad4d-2c35c7590984`. No fallback result was accepted. The
three reported P1 findings received one owner disposition each:

| Repeat-review finding | Final disposition and proof |
| --- | --- |
| Fresh `ServeOptions` cannot re-adopt a retained externally owned fence under a new process-incarnation ID. | Valid recovery requirement, routed to NNC3.8 rather than implemented here. NNC3.5 deliberately scopes generation/epoch to a fresh incarnation, never releases an external effect from local-descriptor evidence, and does not own fresh-process generation handoff or authenticated external release. NNC3.8 already requires active leases and ambiguous cleanup to survive a genuinely fresh-process restart. |
| Hostname fallback could continue after `reserve` succeeded but the separate bind-claim receipt failed. | Accepted and fixed. Pure claim/attempt construction now precedes reservation; a failed claim performs exact no-effect compensation, and only a proven durable `PortConflict` is classified `AddrInUse` and allowed to try another hostname candidate. Every other preparation failure aborts fallback. `authority_preparation_failure_aborts_candidate_fallback`, `durable_port_conflict_may_try_the_next_hostname_candidate`, and the 862-test CLI suite pass. |
| Failure while adopting an already-bound listener stranded its prepared claim. | Accepted and fixed. Adoption plus activation is now one atomic authority transaction. Any pre-adoption or transaction failure first closes the concrete socket, then abandons the authenticated never-bound claim and settles according to provenance; an ambiguous cleanup receipt remains fenced. `adoption_failure_closes_socket_and_releases_never_bound_owned_claim` proves socket closure and durable `Released` state. |

Per the owner instruction, this was the one repeat review, not the start of a
new campaign. The two direct corrections were verified by focused and complete
affected suites, strict lint/rustdoc, format/diff, and static verifier gates;
no additional autoreview was launched. No Claude Opus 4.8 or alternate-model
result was used.

## Scope And Later Owners

This item deliberately does not:

- migrate standalone `nimbus-kv` (NNC3.6);
- migrate machine SSH/forwarding listeners (NNC3.7);
- change CLI dev conventional/ephemeral or start-adapter availability
  decisions (NNC3.7a);
- close the complete bind census (NNC3.7b);
- infer fresh-process socket absence or recycle ambiguous claims (NNC3.8);
- delete remaining probe/drop or legacy authority (NNC3.9); or
- replace the existing sibling-task unwind with the structured listener group
  (NNC7.1a).

Those boundaries keep NNC3.5 testable and closeable without duplicating later
authority.

## Candidate Changed Paths

```text
Cargo.lock
crates/nimbus-cli/Cargo.toml
crates/nimbus-cli/src/start/boot.rs
crates/nimbus-server/Cargo.toml
crates/nimbus-server/src/construction.rs
crates/nimbus-server/src/lib.rs
crates/nimbus-server/src/listener_lease.rs
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc3.5-server-listener-port-migration.md
```

No push or PR is authorized.
