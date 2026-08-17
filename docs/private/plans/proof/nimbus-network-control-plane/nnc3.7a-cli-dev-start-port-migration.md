# NNC3.7a CLI Dev/Start Port Migration Proof

Date: 2026-07-27

Status: `complete; written acceptance 3/3; accepted review finding fixed; clean rerun`

Starting checkpoint:
`77f075750109c56abe0d4086a4e244a4f99ba20e`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Written Acceptance

| Criterion | Required proof | Current state |
| --- | --- | --- |
| No production CLI probe/drop result becomes desired port authority. | Start adapter configuration resolves without consulting a socket-availability callback; source inventory contains no CLI availability-probe owner. | Passing. |
| Conventional conflict uses the shared lease/provider-bind contract without behavior drift. | Default `nimbus start` ports still fail loudly when unavailable, explicit ports still fail at the real bind, and durable conflict/failure evidence names the listener authority. | Passing. |
| Ephemeral adoption uses the shared lease/provider-bind contract without behavior drift. | `nimbus dev` provider-assigned sockets remain bound from durable claim through server adoption; detected conventional fallback, undetected ephemeral selection, `.env.local`, notices, and banner stay truthful. | Passing. |

## Starting Call Graph

```text
nimbus dev
  -> resolve_dev_plan
     -> resolve_wire_plan
        -> load_or_generate credentials
        -> for MongoDB, DynamoDB, S3
           -> resolve_listener_port
              -> detected:
                 TcpListener::bind(conventional)
                 -> drop probe
                 -> choose conventional
                 -> on any bind error call ephemeral_port
              -> undetected:
                 -> ephemeral_port
                    TcpListener::bind(127.0.0.1:0)
                    -> read assigned port
                    -> drop probe
     -> copy three numeric ports into StartCommand
  -> write .env.local and banner from those numbers
  -> run_start_command
     -> resolve_adapter_enablement (explicit numbers, so no second probe)
     -> nimbus-server serve_leased
        -> shared PortLease reserve + claim
        -> later real bind of the advertised number
        -> adopt + activate

nimbus start
  -> run_start_command
     -> resolve_adapter_enablement
        -> for each default-on MongoDB, DynamoDB, S3 listener
           -> bind conventional port
           -> drop probe
           -> treat success as desired port selection
     -> nimbus-server serve_leased
        -> shared PortLease reserve + claim
        -> real bind
        -> adopt + activate
```

The server side already owns the authoritative exact/provider-assigned
reserve → claim → bind → adopt → activate lifecycle from NNC3.5. The remaining
CLI probes are duplicate availability decisions:

- the start probe is unnecessary and racy because the later server lease and
  provider bind are authoritative;
- the dev ephemeral probe cannot simply be deleted because dev must advertise
  the actual provider-assigned port before serving. Its socket and lease must
  stay live and transfer into the same server authority rather than reducing
  back to a number.

## Target Seam

```text
nimbus start
  -> resolve pure adapter desired addresses
  -> nimbus-server shared authority reserve + claim
  -> real provider bind
  -> adopt + activate
  -> fail loud on conventional or explicit conflict

nimbus dev
  -> construct one nimbus-server listener authority for the control-data root
  -> for each wire surface:
     detected: exact conventional prepare/bind/adopt,
               AddrInUse -> provider-assigned prepare/bind/adopt
     undetected: provider-assigned prepare/bind/adopt
  -> retain the concrete pre-bound sockets plus Active leases
  -> advertise only their observed addresses
  -> pass the same authority and sockets into ServeOptions
  -> server validates identity/address, applies guard/projection, then serves
```

The portable crate remains effect-free. `nimbus-server` owns the adapter between
durable leases and sockets; `nimbus-cli` remains the dev composition/effect
owner. No policy, service naming, proxy, tenant, machine, or cluster authority
moves.

## Implemented Seam

- `nimbus-server::PreboundServerListeners` creates one server-listener
  authority incarnation, prepares exact or provider-assigned requests, and
  retains standard-library sockets carrying Active durable leases.
- `nimbus dev` claims before each real conventional/provider-assigned bind,
  durably records a no-effect `AddrInUse` receipt before fallback, observes the
  actual provider-assigned port, and retains the exact descriptor rather than
  handing a number to a later bind.
- `StartCommand` carries the private listener bundle into startup. Startup
  selects its authority before preparing the main listener and transfers the
  bundle only after all earlier fallible work succeeds.
- `ServeOptions` validates the same incarnation, exact configured address, and
  adapter guard before converting the retained descriptor to Tokio and serving
  the unchanged adapter.
- Every explicit rejection/error path closes the socket and settles the lease.
  Dropping a bundle before transfer is also cancellation-safe: it closes and
  best-effort settles every retained listener; a settlement error is logged
  and its durable fence remains for later reconciliation.
- Direct `nimbus start` resolution is now pure desired-state configuration.
  The shared server authority and real provider bind decide availability.
  Default conventional conflicts preserve the existing adapter-specific
  recovery guidance; explicit conflicts remain loud real-bind failures.
- The source-derived inventory replaces both CLI probe/drop owners with the
  retained provider-bind/consumer seam and records the deleted start
  availability callback.

`nimbus-network` remains unchanged and effect-free. No socket, protocol,
provider, CLI, server, policy, naming, proxy, machine, or cluster dependency
enters the portable crate.

## Expected-Red Evidence

### Provider-assigned dev socket is stealable

```text
timeout 180 cargo test -p nimbus-cli \
  provider_assigned_wire_port_stays_held_until_server_adoption \
  -- --ignored --nocapture
```

Exit: `101`.

The test resolved and advertised provider-assigned port `60959`, then a
competing real listener acquired it before server adoption:

```text
the advertised provider-assigned port 60959 must remain bound until the server
adopts the listener, but a competing owner acquired it
```

The numeric port is intentionally environment-dependent. The proof is the
successful second bind after the first descriptor was dropped.

### Start desired-state resolution invokes three probes

```text
timeout 180 cargo test -p nimbus-cli \
  start_adapter_resolution_does_not_consult_kernel_availability \
  -- --ignored --nocapture
```

Exit: `101`.

The injected counter observed exactly three availability decisions:

```text
left: 3
right: 0
```

This proves configuration resolution still asks the kernel to decide desired
ports independently of the shared authority.

## Behavioral Evidence

The two expected-red tests are now ordinary passing tests. The complete
NNC3.7a focused lane also covers conventional preference, fallback truth,
same-incarnation adoption without rebind, cross-incarnation rejection,
pre-start failure cleanup, bundle-drop cleanup, and recovery guidance:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  timeout 300 cargo nextest run -p nimbus-server -p nimbus-cli \
  -E 'test(/nnc3_7a|dropping_prebound_bundle|startup_error_closes_and_releases_untransferred_prebound_listeners|conventional_port_conflict_fails_through_shared_authority_with_guidance|dev_plan_retains_every_advertised_wire_socket_until_start_handoff|provider_assigned_wire_port_stays_held_until_server_adoption|detected_surface_prefers_conventional_port|port_conflict_fallback_updates_nimbus_owned_key|undetected_surfaces_take_ephemeral_ports|start_adapter_resolution_does_not_consult_kernel_availability/)' \
  --test-threads 1 --status-level fail --final-status-level all
12 passed; 0 failed
```

The passing cases are:

1. every advertised dev wire port is the exact continuously held handoff
   socket;
2. detected surfaces prefer a free conventional address;
3. a real conventional `AddrInUse` receipt permits provider-assigned fallback
   and keeps `.env.local` plus the fallback notice truthful;
4. provider-assigned sockets remain unstealable before server adoption;
5. undetected surfaces use retained provider-assigned sockets without claiming
   a fallback;
6. start adapter resolution performs zero kernel availability decisions;
7. an early startup error closes sockets and leaves durable leases Released;
8. a shared-authority conventional conflict fails loud with the existing
   adapter-specific recovery flags;
9. a foreign authority incarnation rejects and settles every selected and
   unconsumed pre-bound listener;
10. the same authority serves the retained descriptor without a second bind
    and with an Active lease visible to the adapter guard;
11. dropping pre-serve bundle ownership closes its socket and settles its
    Active lease;
12. replacing or dropping `ServeOptions` retains the drop-capable bundle until
    consumption, closes every unconsumed socket, and leaves its lease Released.

The full CLI suite at the candidate bytes before the final bundle-drop safety
test passed `873/873` with one declared skip. The final affected aggregate and
quality-gate results are recorded below after their candidate-complete run.

The first candidate-complete affected lane passed without the earlier
non-reproducible leak diagnostic:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  timeout 1200 cargo nextest run \
  -p nimbus-network -p nimbus-cli -p nimbus-server -p nimbus-system \
  --filter-expr \
  'not test(deploy_admin_requires_local_admin_header_even_with_deploy_bearer) & not test(cloud_functions_passes_runtime_owner_lifecycle_conformance)' \
  --test-threads 4 --status-level fail --final-status-level fail
1647 passed; 0 failed; 29 skipped
```

An earlier pre-safety-net aggregate passed `1646/1646` but reported one
nextest leak diagnostic without naming the test. A diagnostic rerun of
`nimbus-network`, `nimbus-server`, and `nimbus-system` passed `773/773` with no
leak; the final aggregate above passed `1647/1647` with no leak. No leak is
claimed as accepted or hidden evidence.

The structured review found one material lifecycle path: transferring the
bundle into a raw map removed its drop guard before server consumption.
`ServeOptions` now retains `Option<PreboundServerListeners>` itself. Removing a
matched listener is the only way to take it out of the guard; cancellation,
early main-listener setup failure, option drop, and builder replacement all
close and settle every still-unconsumed listener. The focused lane then passed
`12/12`, and the complete affected lane reran at the fixed bytes:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  timeout 1200 cargo nextest run \
  -p nimbus-network -p nimbus-cli -p nimbus-server -p nimbus-system \
  --filter-expr \
  'not test(deploy_admin_requires_local_admin_header_even_with_deploy_bearer) & not test(cloud_functions_passes_runtime_owner_lifecycle_conformance)' \
  --test-threads 4 --status-level fail --final-status-level fail
1648 passed; 0 failed; 29 skipped
```

## Static Evidence

```text
bash scripts/verify-nimbus-network-control-plane.sh --self-test
16/16 PASS

bash scripts/verify-nimbus-network-control-plane.sh
14 PASS; 1 expected later-owned failure solely at NNCV005
```

NNCV005 now names only
`crates/nimbus-sandbox/src/backends/oci/port_manager.rs:41`; NNC3.9 owns its
deletion. NNCV006 passes, including its source-derived inventory check, so no
production CLI availability probe or unclassified production bind remains.

## Quality And Documentation Evidence

```text
timeout 900 cargo check \
  -p nimbus-network -p nimbus-cli -p nimbus-server -p nimbus-system \
  --all-targets
PASS

timeout 1200 cargo clippy \
  -p nimbus-network -p nimbus-cli -p nimbus-server -p nimbus-system \
  --all-targets --no-deps -- -D warnings
PASS

RUSTDOCFLAGS='-D warnings' timeout 900 cargo doc --no-deps \
  -p nimbus-network -p nimbus-cli -p nimbus-server -p nimbus-system
PASS

cargo fmt --all --check
PASS

git diff --check
PASS

jq empty \
  docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
PASS

cargo tree -p nimbus-network --edges normal
PASS: nimbus-core remains the only Nimbus workspace dependency

bash scripts/check-docs.sh
108 pages; PASS

bash scripts/verify-nimbus-docs-site.sh
17/17 PASS
```

The changed production composition roots remain below the repository's
modularity threshold:

```text
996  crates/nimbus-server/src/listener_lease.rs
1313 crates/nimbus-server/src/construction.rs
520  crates/nimbus-cli/src/dev/wire.rs
1214 crates/nimbus-cli/src/start/boot.rs
```

The small final lifecycle cleanup increased the first two counts only within
their existing coherent listener-lease and server-construction ownership; no
file reaches the 1,500-line justification threshold.

## Structured Review

The required candidate-complete review ran as one bundle pass:

```text
AUTOREVIEW_ALLOW_NESTED_CODEX=1 \
  /Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine codex --model gpt-5.6-sol \
  --thinking xhigh --codex-speed fast --stream-engine-output \
  --prompt '<NNC3.7a written-acceptance and ownership scope>'
```

The helper confirmed engine `codex`, model `gpt-5.6-sol`, reasoning `xhigh`,
service tier `fast`, one review pass, and thread
`019fa569-a7d3-7b93-8fd3-04143b54f9f2`.

| Finding | Disposition | Resolution and proof |
| --- | --- | --- |
| P2, 0.92: converting the bundle to a raw map in `ServeOptions` removed drop cleanup before consumption; cancellation, early setup failure, or builder replacement could close sockets while leaving false Active leases. | Accepted as an in-scope lifecycle defect. | `ServeOptions` retains the drop-capable bundle. It removes only a matched listener; all still-unconsumed listeners settle on explicit error or ownership drop. `nnc3_7a_serve_options_drop_and_replacement_settle_prebound_bundles` proves both paths; focused acceptance passes 12/12 and the fixed affected lane passes 1,648/1,648. |

Because the accepted finding materially changed production lifecycle ownership,
the review contract requires one clean rerun of the same Sol/xhigh/fast review.
The rerun used the same engine/model/reasoning/tier as one bundle pass, thread
`019fa579-56ac-7053-a225-7bdba402950b`, and returned:

```text
findings: []
overall_correctness: patch is correct
overall_confidence: 0.93
autoreview clean: no accepted/actionable findings reported
```

The reviewer explicitly confirmed that the bundle remains drop-capable until
individual synchronous consumption; unconsumed sockets and leases settle on
option drop, replacement, startup failure, rejection, unmatched handoff, and
future cancellation. Review stops at this clean result. No additional review
was run.

## Candidate Changed Paths

```text
crates/nimbus-cli/src/dev/plan.rs
crates/nimbus-cli/src/dev/tests/adoption.rs
crates/nimbus-cli/src/dev/tests/plan.rs
crates/nimbus-cli/src/dev/wire.rs
crates/nimbus-cli/src/start/adapters/dynamodb.rs
crates/nimbus-cli/src/start/adapters/mod.rs
crates/nimbus-cli/src/start/adapters/mongodb.rs
crates/nimbus-cli/src/start/adapters/s3.rs
crates/nimbus-cli/src/start/boot.rs
crates/nimbus-cli/src/start/mod.rs
crates/nimbus-cli/src/start/tests/adapters.rs
crates/nimbus-server/src/construction.rs
crates/nimbus-server/src/lib.rs
crates/nimbus-server/src/listener_lease.rs
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/README.md
docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
docs/private/plans/proof/nimbus-network-control-plane/nnc3.7a-cli-dev-start-port-migration.md
```

All 18 paths are NNC3.7a-owned. The item proof is intentionally force-tracked
because the private proof directory is ignored by default.

## Migration Constraints

- Preserve detected conventional preference plus `AddrInUse` fallback in dev.
- Preserve undetected provider-assigned ports and truthful pre-serve
  `.env.local`/banner values.
- Preserve start's fail-loud conventional and explicit-port behavior.
- A fallback may continue only after the exact no-effect bind failure receipt
  is durable; authority failure stops fallback.
- Pre-bound sockets must belong to the same server authority incarnation that
  serves them; cross-incarnation adoption fails closed.
- Any pre-serve error must close and explicitly settle every socket whose
  absence is confirmed. Ambiguous drops retain fencing for NNC3.8.
- Existing sibling partial-start unwind remains NNC7.1a-owned.
- Do not introduce a new allocator, numeric-port handoff, compatibility shim,
  or socket effect in `nimbus-network`.

## Next Step

Commit these exact 18 owned paths as the NNC3.7a checkpoint with the plan
activating NNC3.7b. Then close the source-derived bind/allocation census from a
read-only inventory/verifier reconciliation before any new production edit.
Do not start restart convergence (NNC3.8), old-authority deletion (NNC3.9), or
structured sibling-task unwind (NNC7.1a); do not push or open/update a PR.
