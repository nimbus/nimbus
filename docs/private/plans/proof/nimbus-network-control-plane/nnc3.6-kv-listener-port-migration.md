# NNC3.6 Standalone KV Listener Port Migration Proof

Date: 2026-07-27

Status: `complete`

Starting checkpoint:
`856c5834a6d59151aea525688501815cdc80d8de`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Scope And Ownership

`nimbus-kv` retains the standalone RESP socket, accept loop, protocol bytes,
and pre-bound listener seam. It consumes the transport-free
`nimbus-network` port authority through a concept-owned listener adapter.
`nimbus-cli` remains the composition owner for the shared node-local network
state root. NNC3.6 does not move TCP effects into `nimbus-network`, alter RESP
behavior, or implement NNC3.8 restart reconciliation.

The source-derived bind census assigns both existing KV paths to this item:

| Inventory ID | NNC3.6 disposition |
| --- | --- |
| `kv-direct-listener` | `nimbus-kv` keeps the bind and must reserve/claim before the effect, then adopt/activate its exact kernel result. |
| `kv-prebound-listener` | `nimbus-kv` keeps the adoption seam and must record exact `ExternallyOwned` evidence before serving. |

The address-independent listener identity is a process incarnation plus the
logical `resp` listener name. Generation and epoch are `1` within that fresh
incarnation until NNC3.8 owns durable restart handoff.

## Implemented Lifecycle

`NimbusKvListenerConfig` carries the shared node-local network state root,
stable `ListenerId`, generation, and epoch into the `nimbus-kv` adapter. A
direct listener now:

1. opens the host-global authority;
2. reserves and durably claims the requested binding before `TcpListener::bind`;
3. records durable no-effect evidence if the kernel rejects the bind; and
4. adopts the exact kernel address and ownership provenance and atomically
   activates the binding before returning it to the RESP server.

Port zero remains provider-assigned, a fixed direct port is Nimbus-owned, and a
pre-bound listener is externally owned. Pre-bound configuration must match the
descriptor's exact address; mismatch closes the descriptor before any durable
authority is created. Confirmed Nimbus-owned close withdraws and releases the
lease, while confirmed external close only withdraws it. Ordinary cancellation
or `Drop` closes the local descriptor but deliberately retains the Active
fence for NNC3.8 reconciliation.

The CLI composes one shared state root using this precedence:

1. `--control-data-dir`;
2. `NIMBUS_CONTROL_DATA_DIR`;
3. `./data`.

The RESP socket, accept loop, protocol bytes, and provider effects remain in
`nimbus-kv`; no transport effect moved into `nimbus-network`.

## Fail-Before Evidence

The scaffold compiled before any lease behavior was added:

```text
cargo check -p nimbus-kv -p nimbus-cli --all-targets
Finished `dev` profile; exit 0
```

Each written acceptance proof then failed for its intended missing behavior.

### Real separate-process contention

```text
timeout 180 cargo test -p nimbus-kv --test network_listener \
  two_standalone_kv_processes_contend_in_one_authority -- --nocapture
```

Exit: `101`.

The NNC0.1a process harness released two real child roles against one state
root and fixed loopback port. One child completed the kernel bind but failed
because it had no Active durable lease. The other reported only
`Address already in use (os error 48)`. This proves the missing cross-process
authority rather than a synthetic in-process collision.

### Fixed conflict owner identities

```text
timeout 180 cargo test -p nimbus-kv --test network_listener \
  fixed_conflict_reports_both_stable_owner_identities -- --nocapture
```

Exit: `101`.

The second direct bind failed with only `Address already in use (os error 48)`;
the diagnostic did not name either the durable winner or rejected stable owner.

### Pre-bound provenance

```text
timeout 180 cargo test -p nimbus-kv --test network_listener \
  prebound_kv_listener_adopts_exact_external_provenance -- --nocapture
```

Exit: `101`.

The inherited Tokio listener remained usable, but the authority contained zero
records instead of one Active binding with `ExternallyOwned` provenance.

## Acceptance Matrix

| Written criterion | Executable evidence | Current result |
| --- | --- | --- |
| The NNC0 process harness proves separate processes contend in one authority. | `two_standalone_kv_processes_contend_in_one_authority` | Pass |
| Fixed conflict reports both owner identities. | `fixed_conflict_reports_both_stable_owner_identities` | Pass |
| Pre-bound path is tested. | `prebound_kv_listener_adopts_exact_external_provenance` | Pass |

Written acceptance: `3/3` pass.

## Behavioral Evidence

```text
cargo test -p nimbus-kv --test network_listener -- --nocapture
8 passed; 0 failed; 1 ignored child-only entrypoint

cargo nextest run -p nimbus-kv
22 passed; 0 failed; 2 skipped

cargo nextest run -p nimbus-cli
863 passed; 0 failed; 2 skipped
```

The focused suite also proves:

- provider-assigned port zero activates the exact kernel port and releases on
  confirmed close;
- external kernel collision records durable no-effect failure evidence;
- pre-bound address mismatch creates no authority and closes the descriptor;
- synchronous setup failure releases its exact Nimbus-owned lease; and
- ambiguous cancellation closes the local descriptor but retains the Active
  durable fence and rejects a new contender.

The complete `nimbus-kv` suite retains the RESP happy, edge, protocol-error,
pre-bound, and graceful-shutdown behavior. The complete `nimbus-cli` suite
includes the pure control-data-root precedence proof.

## Quality And Structural Evidence

```text
cargo check -p nimbus-kv -p nimbus-cli --all-targets
PASS

cargo clippy -p nimbus-kv -p nimbus-cli --all-targets --no-deps -- -D warnings
PASS

RUSTDOCFLAGS='-D warnings' cargo doc --no-deps -p nimbus-kv -p nimbus-cli
PASS

cargo fmt --all --check
PASS

git diff --check
PASS

bash scripts/verify-nimbus-network-control-plane.sh --self-test
16/16 PASS

bash scripts/verify-nimbus-network-control-plane.sh
14 PASS; 1 expected later-owned failure solely at NNCV005

bash scripts/check-docs.sh
108 pages; PASS

bash scripts/verify-nimbus-docs-site.sh
17/17 PASS
```

NNCV006 passes: `nimbus-network` retains exactly one workspace dependency,
`nimbus-core`, and contains no socket, protocol, provider-effect, policy,
service-name, proxy, projection, or cluster-transport dependency. Production
module sizes are below the repository threshold: `listener.rs` is 366 lines,
`server.rs` is 1,073 lines, and the CLI composition module is 184 lines.

## Structured Review

One candidate-complete structured review ran with the owner-selected reviewer:

```text
autoreview --mode local --engine codex --model gpt-5.6-sol \
  --thinking xhigh --codex-speed fast \
  --prompt 'Review only NNC3.6 standalone nimbus-kv listener migration ...'
```

The reported engine was Codex, model `gpt-5.6-sol`, reasoning `xhigh`, and fast
service tier; no fallback reviewer was accepted. The review found two P2
issues, both accepted:

| Finding | Disposition |
| --- | --- |
| The proof still described only expected-red/in-progress state. | Corrected by this final proof and the canonical ledger transition. |
| Ambiguous cancellation/`Drop` retained its fence by implementation but lacked an executable proof. | Added `ambiguous_listener_drop_retains_active_fence_for_reconciliation`; it passes in the focused and full suites. |

No production or architecture bytes changed after review. Per the owner's
one-review closeout rule, the test-and-proof-only corrections do not trigger a
repeat review.

## Closeout

NNC3.6 owns no later restart inference. Fresh-process handoff and convergence
remain NNC3.8 obligations. Machine SSH/forwarding listeners begin at NNC3.7;
CLI dev/start allocation decisions begin at NNC3.7a; census closure and
old-authority deletion remain NNC3.7b and NNC3.9.
