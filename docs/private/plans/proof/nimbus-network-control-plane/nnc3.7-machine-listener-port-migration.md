# NNC3.7 Machine SSH/Forwarding Listener Migration Proof

Date: 2026-07-27

Status: `complete; acceptance, review, and repository gates pass`

Starting checkpoint:
`9ded055bf386ab8ccc6dc9c36d569707e2755f89`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Scope And Ownership

NNC3.7 migrates the host TCP port through which gvproxy forwards SSH to a
managed machine. `nimbus-cli` remains the machine lifecycle coordinator and
gvproxy process adapter; gvproxy remains the provider effect that creates the
real TCP listener. `nimbus-network` owns only the stable listener identity,
range allocation, durable bind claim, exact binding evidence, and fencing.

The SSH-based machine API forwarder creates a Unix-domain listener through the
`ssh -L` process. It is part of machine forwarding lifecycle but not a
host-TCP-port allocation authority, so it does not receive a `PortLease`.
Provider-managed networking remains a separate capability mode; NNC3.7 does
not add WSL2 effects or preempt NNC4.4 capability work.

## Fail-Before Ownership And Complexity Audit

The fail-before call graph was:

```text
machine start
  -> MachineLaunchPlan::build
     -> allocate_machine_ssh_port
        -> fs2 lock over machine-local port-alloc.lck
        -> load machine-local port-alloc.dat
        -> TcpListener::bind(127.0.0.1, candidate)
        -> drop probe listener
        -> persist numeric claim in port-alloc.dat
     -> build gvproxy -ssh-port <selected>
  -> persist MachineRuntimeState.ssh_port
  -> pre_start_networking
     -> spawn gvproxy
     -> wait for gvproxy Unix socket
  -> wait for localhost SSH readiness
```

This has two commit authorities:

1. the machine-local JSON allocation map serializes only machine callers; and
2. the network state store serializes server, sandbox, PEP, and KV callers.

The machine lock prevents two cooperating machine commands from selecting one
port, but it cannot conflict with a server lease and cannot close the external
binder window between probe/drop and gvproxy bind. `MachineRuntimeState` is an
observed/runtime record; its `ssh_port` field is not a lease authority.

Removal currently deletes the machine-local map entry. Stop deliberately keeps
it so an ordinary restart can reuse the numeric port. The replacement must
preserve stable restart intent using the shared lease lifecycle, not retain a
second allocation file.

## Implemented Seam

The implementation uses a concept-owned machine SSH port adapter in
`nimbus-cli`:

- a stable address-independent `ListenerId` derives from the node-local managed
  machine identity and logical `ssh-forward` listener name;
- generation and epoch fence the request;
- one `PortRequestMode::Range(10000..=65535)` request allocates in the same
  node store as server, sandbox, PEP, and KV listeners;
- the request is reserved and claimed before gvproxy starts;
- the provider-selected numeric port is passed to gvproxy only from the durable
  reservation;
- exact loopback TCP binding evidence activates only after provider
  observation;
- confirmed provider stop retains or releases the exact lease according to
  restart versus removal intent; ambiguous process loss stays fenced for
  NNC3.8;
- standalone machine commands resolve the same control-data root contract as
  other CLI entrypoints, while `nimbus start` injects its already-resolved
  control-data root into the host machine lifecycle manager; and
- `port-alloc.dat`, `port-alloc.lck`, probe/drop selection, and their root
  accessors are deleted rather than retained as compatibility state.

No socket, gvproxy, process, machine policy, Axum, or provider effect moves
into `nimbus-network`.

The production lifecycle now has one host-global authority:

```text
machine start
  -> install startup signal monitor
  -> build every fallible VMM/helper command input
  -> PreparedMachineSshPortLease::prepare
     -> open the shared control-data network store
     -> reuse a nonterminal listener identity or mint a new incarnation
     -> reserve Range(10000..=65535)
     -> durably claim one attempt-unique gvproxy bind
  -> persist listener ID plus selected observed port in MachineRuntimeState
  -> spawn gvproxy with only the durably claimed port
  -> observe exact localhost SSH readiness
  -> adopt and activate the exact provider binding
  -> start the Unix-domain API forwarder

machine stop
  -> withdraw/fence the active listener
  -> stop VMM/API forwarder/gvproxy
  -> only an exact gvproxy pid receipt plus successful stop is confirmed absence
  -> retain the exact port as Reserved with confirmed-stopped binding evidence
  -> preserve Failed/Stale runtime evidence and Withdrawing authority on ambiguity

machine remove
  -> require stopped/terminal machine lifecycle
  -> release only a confirmed-stopped retained lease
  -> remove machine records and runtime artifacts
```

Every fallible setup step that can run after reservation but before gvproxy
spawn has explicit no-effect compensation. Provider-attempt construction and
startup-signal installation run before reservation. A failed durable bind
claim attempts exact compensation of the already-reserved slot; if the receipt
is ambiguous and compensation cannot be proved, the durable record stays
fenced rather than being inferred absent.

`MachineRuntimeState.ssh_listener_id` is the address-independent authority
identity; `ssh_port` remains an observed/runtime projection. An ordinary
confirmed stop/restart preserves the listener ID and exact port. A terminal
failed or released record causes the next listener incarnation to receive a
fresh identity.

The production composition files remain small: the machine port adapter is 461
lines, `manager.rs` is 581, `launch.rs` is 261, `readiness.rs` is 556,
`stop.rs` is 519, and `vmm.rs` is 285. No file crosses the repository's
1,500-line justification threshold.

## Fail-Before Evidence

### External owner after probe/drop

```text
timeout 180 cargo test -p nimbus-cli --lib \
  machine::manager::tests::ports_state::external_binder_after_probe_blocks_provider_while_machine_state_claims_port \
  -- --exact --ignored --nocapture
```

Exit: `101`.

A real child process bound the exact machine-selected port after the probe
descriptor was dropped. The faithful provider bind failed with
`io::ErrorKind::AddrInUse`, but `port-alloc.dat` still claimed the port:

```text
left: Some(10001)
right: Some(10001)
```

The concrete port is environment-dependent; the proof identity is the machine
record plus the child acknowledgement, not the number.

### Machine versus server authority

```text
timeout 180 cargo test -p nimbus-cli --lib \
  machine::manager::tests::ports_state::machine_ssh_reservation_conflicts_with_server_listener_authority \
  -- --exact --ignored --nocapture
```

Exit: `101`.

The real machine allocator selected and persisted port `10001` in its isolated
map. A real `ServeOptions::prepare_main_listener` using the same filesystem root
then successfully reserved the same port in the network authority, reaching:

```text
server listener authority must reject machine-owned SSH port 10001
```

This proves duplicated authority rather than a kernel-only collision.

### Ordinary focused baseline

```text
timeout 180 cargo test -p nimbus-cli --lib \
  machine::manager::tests::ports_state:: -- --nocapture
```

Result: `4 passed; 0 failed; 3 ignored`.

The ignores are the two expected-red parents and their child-role entrypoint.
No ordinary behavior test was weakened.

## Written Acceptance Matrix

| Criterion | Executable/static proof | Final result |
| --- | --- | --- |
| Probe-then-drop allocator is deleted. | Source scan rejects `machine_port_is_available`, `TcpListener::bind` in production `ports.rs`, and machine-local allocation state/lock accessors. | Pass. The old function, JSON/lock paths, fs2 map, and production probe are absent; the only machine `TcpListener::bind` sites are explicit test collision/probe harnesses. |
| Machine/server conflict proof passes. | `machine_ssh_reservation_conflicts_with_server_listener_authority` | Pass. A real server reservation is rejected by the same store and the diagnostic names both stable authorities. |

Written NNC3.7 acceptance: **2/2 pass**.

## Behavioral Evidence

Focused machine authority and lifecycle suites:

```text
cargo test -p nimbus-cli --lib \
  machine::manager::tests::ports_state:: -- --nocapture
8 passed; 0 failed; 1 ignored child-only entrypoint

cargo test -p nimbus-cli --lib \
  machine::manager::tests::stop_cleanup:: -- --nocapture
4 passed; 0 failed

cargo test -p nimbus-network \
  port_lease::tests::confirmed_stop_rebind_transition_is_exact_fenced_and_idempotent \
  -- --exact --nocapture
1 passed; 0 failed

cargo test -p nimbus-cli \
  provider_managed_backend_is_rejected_before_host_listener_authority \
  -- --nocapture
1 passed; 0 failed
```

The focused cases prove:

- range reservations from two machine incarnations select distinct ports;
- a durable bind claim exists before any provider effect;
- exact localhost evidence activates the lease;
- a proven pre-provider failure abandons, withdraws, and releases its claim;
- a real external child wins the exact kernel port, the faithful provider bind
  receives `AddrInUse`, and the failure/cleared claim are durable;
- machine and server owners conflict in the same authority before a bind;
- confirmed stop/restart reuses the exact listener identity and port;
- confirmed-stopped removal reaches terminal `Released`;
- ambiguous gvproxy stop preserves provider artifacts, marks machine state
  `Failed/Stale`, and retains `Withdrawing` exact binding evidence; and
- confirmed gvproxy stop retains an exact stopped-binding receipt before
  ordinary restart; and
- provider-managed WSL2 is rejected before the host-managed launch plan can
  create network-authority or machine-runtime state.

The pre-review broad lane was run twice to distinguish configuration and
parallel-load failures rather than hide them:

```text
timeout 900 cargo nextest run \
  -p nimbus-network -p nimbus-machine -p nimbus-cli \
  -p nimbus-server -p nimbus-system
1651 passed; 5 failed; 27 skipped
```

Three failures were the explicit PostgreSQL/MySQL/libSQL projection fixtures
rejecting a missing live-provider environment. Their diagnostics require
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` for the ordinary local
lane; they are not claimed as provider evidence. The other two are the exact
pre-existing NNC0.7 server baselines documented by NNC3.5:

- `deploy_admin_requires_local_admin_header_even_with_deploy_bearer`;
- `cloud_functions_passes_runtime_owner_lifecycle_conformance`.

The corrected ordinary-local lane used the mandated provider-fixture setting
and excluded only those two already-reproduced baselines:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  timeout 900 cargo nextest run \
  -p nimbus-network -p nimbus-machine -p nimbus-cli \
  -p nimbus-server -p nimbus-system \
  --filter-expr \
  'not test(tests::local_server_security::deploy_admin_requires_local_admin_header_even_with_deploy_bearer) & not test(tests::runtime_owner_conformance::cloud_functions_passes_runtime_owner_lifecycle_conformance)'
1652 passed; 2 failed; 29 skipped
```

Both failures were 30-second conflict-retry harness checkpoints under the
heavily parallel lane. Each passed immediately in exact isolation at the same
production bytes:

```text
cargo test -p nimbus-server \
  tests::convex_runtime::conflict_retry::forced_conflict_integration_test \
  -- --exact --nocapture
1 passed; 0 failed

cargo test -p nimbus-server \
  tests::convex_runtime::conflict_retry::wait_before_retry_test \
  -- --exact --nocapture
1 passed; 0 failed
```

The structured reviewer correctly rejected adding those two isolated passes to
the failed broad aggregate. The authoritative closeout lane therefore reran the
same affected scope at bounded concurrency after resolving the accepted
provider-mode finding:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  timeout 1200 cargo nextest run \
  -p nimbus-network -p nimbus-machine -p nimbus-cli \
  -p nimbus-server -p nimbus-system \
  --filter-expr \
  'not test(deploy_admin_requires_local_admin_header_even_with_deploy_bearer) & not test(cloud_functions_passes_runtime_owner_lifecycle_conformance)' \
  --test-threads 4 --status-level fail --final-status-level fail
1655 passed; 0 failed; 29 skipped
```

This single aggregate is the final affected result: **1,655/1,655 pass**. The
29 declared skips include the explicitly disabled external-provider fixtures;
no live PostgreSQL, MySQL, or libSQL provider execution is claimed.

## Quality And Structural Evidence

```text
cargo check \
  -p nimbus-network -p nimbus-machine -p nimbus-cli \
  -p nimbus-server -p nimbus-system --all-targets
PASS

cargo clippy \
  -p nimbus-network -p nimbus-machine -p nimbus-cli \
  -p nimbus-server -p nimbus-system \
  --all-targets --no-deps -- -D warnings
PASS

RUSTDOCFLAGS='-D warnings' cargo doc --no-deps \
  -p nimbus-network -p nimbus-machine -p nimbus-cli \
  -p nimbus-server -p nimbus-system
PASS

cargo fmt --all --check
PASS

git diff --check
PASS

jq empty \
  docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
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

NNCV005 now names only the later-owned CLI development resolver and retained
sandbox `PortManager` deletion gate. NNCV006 passes with the refreshed bind
inventory, proving that NNC3.7 introduced no unclassified production bind or
probe. `cargo tree -p nimbus-network --edges normal` confirms
`nimbus-network` still has exactly one Nimbus workspace dependency,
`nimbus-core`; no socket, gvproxy, machine, server, or other upper-layer
dependency entered the portable crate.

The source-derived inventory now records the current machine allocation,
deleted production probe, and gvproxy provider-bind truth. The API forwarder
remains a machine-owned Unix-domain effect and is explicitly outside host
TCP/UDP `PortLease` authority.

## Migration Constraints

- Promote `nimbus-network` from a `nimbus-cli` dev dependency to a direct
  production dependency; do not route portable authority through the `nimbus`
  facade.
- Keep one provider-attempt identity distinct from stable listener identity.
- Claim before gvproxy spawn; never infer authority from
  `MachineRuntimeState.ssh_port`.
- Do not classify an ambiguous gvproxy exit as confirmed absence. NNC3.8 owns
  fresh-process/provider-generation reconciliation.
- Preserve machine lifecycle error precedence while retaining cleanup/fencing
  failures in diagnostics.
- Preserve provider-managed versus host-managed networking semantics.
- Do not start CLI dev/start port work, census closure, or old-authority
  deletion outside the machine-owned allocator; those remain NNC3.7a,
  NNC3.7b, and NNC3.9.

## Structured Review

One candidate-complete review ran with the required reviewer and item scope:

```text
autoreview --mode local --engine codex --model gpt-5.6-sol \
  --thinking xhigh --codex-speed fast --stream-engine-output \
  --prompt 'Review only NNC3.7, its written acceptance criteria, lifecycle correctness, authority boundaries, and directly related cleanup.'
```

The review reported engine `codex`, model `gpt-5.6-sol`, reasoning `xhigh`,
service tier `fast`, one pass, and thread
`019fa526-b611-74a1-aeaf-acc4a734a1e7`. It produced two findings:

| Finding | Disposition | Resolution and proof |
| --- | --- | --- |
| P1, 0.98: the launch-plan shape admitted provider-managed networking while unconditionally creating a host SSH lease, but the no-gvproxy branch had no matching stop settlement. | Accepted at the current supported-state boundary. | WSL2 is not implemented and remains NNC4.4-owned. The current `MachineVmmBackend` seam now admits only host-managed krunkit/vfkit backends; `MachineLaunchPlan` structurally requires a gvproxy command, so it cannot combine a host lease with no host networking effect. `provider_managed_backend_is_rejected_before_host_listener_authority` proves WSL2 fails before durable network or machine-runtime state. |
| P2, 0.99: two isolated passes cannot be added to a failed 1,652-pass broad aggregate and called 1,654/1,654. | Accepted. | The proof no longer performs disjoint arithmetic. One bounded-concurrency canonical lane passes 1,655/1,655 with 29 declared skips, at the final production bytes. |

No repeat review was run. The accepted changes only remove an invalid optional
state and correct the proof method; they do not implement a new provider mode,
move an ownership seam, or otherwise materially alter the reviewed
architecture.

## Candidate Changed Paths

```text
Cargo.lock
crates/nimbus-cli/Cargo.toml
crates/nimbus-cli/src/machine/handlers.rs
crates/nimbus-cli/src/machine/local_server.rs
crates/nimbus-cli/src/machine/manager.rs
crates/nimbus-cli/src/machine/manager/launch.rs
crates/nimbus-cli/src/machine/manager/ports.rs
crates/nimbus-cli/src/machine/manager/readiness.rs
crates/nimbus-cli/src/machine/manager/stop.rs
crates/nimbus-cli/src/machine/manager/tests.rs
crates/nimbus-cli/src/machine/manager/tests/launch_image.rs
crates/nimbus-cli/src/machine/manager/tests/ports_state.rs
crates/nimbus-cli/src/machine/manager/tests/provider_bootstrap.rs
crates/nimbus-cli/src/machine/manager/tests/readiness_startup.rs
crates/nimbus-cli/src/machine/manager/tests/ssh_scp.rs
crates/nimbus-cli/src/machine/manager/tests/stop_cleanup.rs
crates/nimbus-cli/src/machine/manager/vmm.rs
crates/nimbus-cli/src/machine/server_control.rs
crates/nimbus-cli/src/machine/stub/manager.rs
crates/nimbus-cli/src/machine/tests/records_state.rs
crates/nimbus-cli/src/start/boot.rs
crates/nimbus-machine/src/lib.rs
crates/nimbus-machine/src/roots.rs
crates/nimbus-machine/src/state.rs
crates/nimbus-network/src/port_lease/rebind.rs
crates/nimbus-network/src/port_lease/tests.rs
crates/nimbus-server/src/tests/machine_lifecycle.rs
crates/nimbus-system/src/tests.rs
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
docs/private/plans/proof/nimbus-network-control-plane/nnc3.7-machine-listener-port-migration.md
```

All paths are owned by NNC3.7. The proof file is intentionally force-tracked at
the item checkpoint because the private proof directory is ignored by default.

## Next Proof Step

Commit these exact 32 owned paths as the NNC3.7 checkpoint with the plan and
routing index activating NNC3.7a. Then begin NNC3.7a from its read-only CLI
dev/start resolver call graph and fail-before behavior; do not push or
open/update a PR.
