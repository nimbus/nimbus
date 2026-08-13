# NNC7.1a Structured Listener Group

Status: `complete`

## Outcome

`nimbus-server` owns one structured group for sibling wire listeners. The
group owns task creation, supervision, cancellation, joining, and lease
settlement. A partial startup or unexpected child completion cannot leave a
sibling accepting traffic after `serve_leased` returns.

## Frozen ownership

- `adapters/wire.rs` returns named, unspawned task futures. It retains the
  private adapter registration seam and contains no socket bind or lease
  effect.
- `listener_group.rs` owns spawned sibling tasks and their active listener
  leases. It reports setup unwind, normal shutdown, and unexpected task death.
- `construction.rs` keeps bind, security guard, observed projection, and main
  server composition. It delegates group lifecycle after each adapter is
  ready.
- MongoDB, DynamoDB, and S3 retain protocol, authentication, router, and
  background-task behavior.
- `listener_lease` and `nimbus-network` retain durable port authority and
  provenance-aware settlement. The group calls the existing settlement seam.
  It does not duplicate it.

Owned product paths:

- `crates/nimbus-server/src/listener_group.rs`
- `crates/nimbus-server/src/listener_group/tests.rs`
- `crates/nimbus-server/src/construction.rs`
- `crates/nimbus-server/src/adapters/wire.rs`
- `crates/nimbus-server/src/adapters/mongodb/mod.rs`
- `crates/nimbus-server/src/adapters/mongodb/listener.rs`
- `crates/nimbus-server/src/adapters/dynamodb/mod.rs`
- `crates/nimbus-server/src/adapters/dynamodb/listener.rs`
- `crates/nimbus-server/src/adapters/dynamodb/ttl_sweeper.rs`
- `crates/nimbus-server/src/adapters/s3/mod.rs`
- `crates/nimbus-server/src/adapters/s3/listener.rs`
- `crates/nimbus-server/src/lib.rs`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`

## Failure matrix

| ID | Boundary | Required result |
| --- | --- | --- |
| F1 | kth bind fails | Abort and join every prior child before settling every prior owned lease. No prior socket accepts after return. Preserve the bind error. |
| F2 | kth guard fails | Close and settle the refused listener, then unwind every prior group member. The guard error remains primary. |
| F3 | kth projection fails | Close and settle the projected listener, then unwind every prior group member. The projection error remains primary. |
| F4 | adapter task construction fails or returns no listener task | Close the untransferred listener, settle its lease, then unwind every prior group member. No task starts for the rejected member. |
| F5 | a spawned child exits successfully while the main server runs | Stop the main serve future, abort and join every sibling, settle every owned lease, and report the named unexpected exit. |
| F6 | a spawned child returns an error or panics | Apply the same complete shutdown and report the adapter/task plus returned or join error. |
| F7 | more than one cleanup fails | Attempt every task join and every lease settlement. Return the primary failure with each cleanup result attached in stable group order. |
| F8 | main listener is externally owned or inherited | Close the local descriptor and withdraw its Nimbus adoption. Never release the external provider authority. Sibling settlement remains unchanged. |

## Acceptance contract

- The inherited `nnc0_7_kth_adapter_failure_must_not_leave_prior_listener_live`
  test becomes ordinary green evidence.
- Focused tests cover F2-F8, including multiple cleanup failures and a child
  that exits without an error.
- Adapter implementations do not spawn listener or background tasks.
- Every adapter supplies at least one listener task. DynamoDB can also
  supply its TTL background task.
- The group aborts all children before it awaits any join.
- The group attempts every join and lease settlement after an earlier cleanup
  error.
- `serve_leased` selects between the main server and sibling supervision. An
  unexpected child completion is a server error, not a log-only event.
- Existing HTTP/WebSocket/MongoDB/DynamoDB/S3 tests preserve protocol and
  security behavior.
- Source checks prove `nimbus-network` still has only `nimbus-core` as a
  workspace dependency and owns no socket, task, protocol, projection, or
  provider effect.

## Non-goals

- Do not change projection schema or make observed state authoritative.
- Do not move protocol parsing, authentication, TLS, sockets, or tasks into
  `nimbus-network`.
- Do not change RESP or `nimbus-kv` ownership.
- Do not add service identity, portable status handles, projection rebuild,
  or TLS/telemetry work owned by NNC7.2-NNC7.6.
- Do not add a public listener-provider abstraction. The group is private
  server composition.

## Verification

During implementation, run exact listener-group tests and the affected adapter
tests. At candidate freeze, run the complete `nimbus-server` suite and the
all-target check. Also run strict Clippy and Rustdoc, format and diff checks,
the live network verifier, documentation gates, and proof lint. Run one
GPT-5.6 Sol/xhigh/fast item review only after all acceptance conditions are
green. Run one narrow correction review only if an accepted finding changes
executable code.

## Evidence ledger

| Check | Result |
| --- | --- |
| Source audit | The existing `WireProtocolAdapter::spawn` returns detached `JoinHandle<()>` values. `serve_leased` drops them on synchronous setup failure and does not supervise child completion while the main server runs. |
| F1 fail-before | `nnc0_7_kth_adapter_failure_must_not_leave_prior_listener_live` exits `101`: the first listener still accepts after the kth bind fails. |
| Worktree checkpoint | NNC7.1 item `e2af495c39996c0692ca4756391e0d705629dfad`; `0 behind / 157 ahead`; product tree clean before this proof. |
| Focused implementation | `cargo test -q -p nimbus-server --lib listener_group::tests:: -- --test-threads=1` passes `10/10`. The ordinary exact NNCF17 regression passes `1/1`. F1-F8 now cover kth bind/guard/projection/construction failure, successful/error/panic child death, complete ordered cleanup reporting, and inherited-listener withdrawal without external release. |
| Adapter preservation | Wire trait `6/6`, MongoDB `10/10`, DynamoDB `7/7`, and S3 `8/8` pass. The focused item total is `42/42`. Adapter implementations return unspawned listener/background futures. MongoDB retains only its existing per-connection task spawn inside the protocol listener. |
| Full affected behavior | `cargo test -q -p nimbus-server -- --test-threads=1` passes `752` tests with `35` declared ignores and zero failures. |
| Compiler quality | All-target check, strict all-target Clippy, and warning-denied Rustdoc pass. The final type-only census correction was followed by another strict Clippy pass. Unchanged vendored Brotli warnings remain dependency output, not Nimbus diagnostics. |
| Authority census | The first live verifier correctly rejected the old `spawn` coordinates, four unclassified group handoffs, and a private socket type alias. The alias was removed, all `67/67` observed authority occurrences were classified under the existing server sibling-listener site, and the bind and composition censuses pass independently. The live aggregate then passes `36/36`. |
| Dependency and effect boundary | Cargo metadata resolves only `nimbus-network -> nimbus-core`. Direct source scans find no socket, task, Axum, Pingora, Netavark, nftables, gvproxy, Iroh, or provider effect in `nimbus-network`. |
| Formatting and documentation | `cargo fmt --all --check` and `git diff --check` pass. Docs pass `108` pages, and site verification passes `17/17`. |
| Modularity | `listener_group.rs` is `343` lines, its concept-owned test child is `698`, and `construction.rs` is `1,429`. The composition root remains below the `1,500`-line coherence threshold. |
| Review cadence | No structured review ran on partial work. One full GPT-5.6 Sol/xhigh/fast item review and one narrow correction review ran. No further review is authorized. |
| Full item review | The full review used staged tree `0fde7421f16511a10b9f2a4f603843fb73db0e58`, patch SHA-256 `3c6a3cbce4cc5bc948b507f2f8ac3f3e3d0a80606bf0b95d56236262c5bacb1e`, and 15 paths. It reported one P3 at confidence `0.93`: F7 used one scheduler yield before shutdown, so cancellation could hide a completed-task error. The finding is accepted. |
| Accepted correction | The test-only group seam now waits with a bounded, diagnostic semantic barrier until every registered task reports completion. Focused listener-group `10/10`, strict Clippy, format, and diff checks pass after correction. |
| Narrow review | The one narrow GPT-5.6 Sol/xhigh/fast review used staged tree `935fb35b45a36b44e22fb794c01b49dce41f0914`, patch SHA-256 `c52db4bb8fa140792922a6ad19ec984317cdeea961e8e6b657c657dc438cf8e0`, and 15 paths. It is clean at confidence `0.98` and confirms the barrier waits for both tasks before F7 shutdown. Review cadence is exhausted. |

## Acceptance disposition

| ID | Result | Evidence |
| --- | --- | --- |
| F1 | pass | The former NNCF17 expected-red is ordinary green. A kth bind failure returns only after prior listener futures are dropped and prior leases settle. |
| F2 | pass | A kth guard refusal closes the rejected socket and every prepared predecessor, preserves the guard error kind/message, and releases the prior active lease. |
| F3 | pass | A deterministic post-guard projection fault unwinds every prepared listener and preserves the projection failure. |
| F4 | pass | Build error, build panic, and listener-factory panic close and settle the untransferred listener. The task-set type requires exactly one listener factory. |
| F5 | pass | A child that returns `Ok(())` stops main supervision, names the unexpected exit, cancels siblings, and settles leases. |
| F6 | pass | Returned errors and panics retain adapter/task identity and converge through the same shutdown path. |
| F7 | pass | Every task receives cancellation before joins begin. Task and lease cleanup failures are all attempted and appended in stable registration order. |
| F8 | pass | An inherited main socket ends in `Withdrawing` with `ExternallyOwned` provenance. The external descriptor continues to fence the host port. |

## Candidate recovery checkpoint

| Field | Value |
| --- | --- |
| Last durable item | NNC7.1 at `e2af495c39996c0692ca4756391e0d705629dfad` |
| Divergence | `0 behind / 157 ahead` of `origin/main` |
| Candidate state | F1-F8, focused `42/42`, full server `752 + 35 ignored`, strict affected quality, live architecture `36/36`, docs `108`, and site `17/17` are green. The accepted F7 correction is focused-green and narrow-review clean. |
| Next action | Commit this completed proof with the exact product and ledger diff, then continue the read-only NNC7.2 seam audit. |
| Blocker | none |
