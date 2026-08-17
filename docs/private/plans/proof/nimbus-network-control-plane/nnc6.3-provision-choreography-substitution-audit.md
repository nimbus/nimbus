# NNC6.3 Provision Choreography Substitution Audit

Status: `complete; prospective split frozen; product source unchanged`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.3 began as one large implementation item. It combined executable content,
provider selection, phase-addressable sandbox effects, registry decomposition,
five caller families, and standalone Compose durability. This audit tests
whether those changes form one coherent unit before any effectful caller
changes.

It is not. The existing row requires compute to observe `start -> attach`, but
NNC6.4 is the later item that first separates OCI preparation, attachment, and
activation. The row also treats Cloud Functions as a lazy-provision caller.
Current source instead snapshots already-active bindings and refuses the
runtime service-lookup host operation.

This checkpoint therefore freezes a prospective implementation split. It does
not compile an intent, write the saga store, reserve a resource, start a
sandbox, change a caller, or alter provider behavior.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| A1 | The source census names every current product compile, intent-construction, binding lookup, activation, provider-start, and standalone Compose authority relevant to provision. |
| A2 | Current and target call graphs distinguish read-only naming from effectful activation and distinguish desired, durable, provider, and observed state. |
| A3 | The audit proves from source whether current Container and Krun `start` calls are phase-addressable or combine preparation, reservation, attachment, readiness, and execution. |
| A4 | The audit identifies the exact durable content available after a fresh process reopens only Engine state and the executable content that is missing. |
| A5 | The audit identifies where deployment generation, assigned node identity, exact capability selection, and standalone resource identity are absent before effects. |
| A6 | The audit classifies Convex and Cloud Functions behavior separately rather than projecting one runtime model onto both. |
| A7 | The audit proves that standalone Compose owns no Engine-backed saga composition and that its attachment-only capability registry is intentionally empty. |
| A8 | The target seam preserves `nimbus-network -> nimbus-core` as the only network-crate workspace edge and introduces no provider effect into `nimbus-network`. |
| A9 | The target seam uses small owner-local capabilities, one compute coordinator, one server-owned Engine adapter, and no CLI-local saga store or compatibility shim. |
| A10 | The sequence puts executable durability and pure decisions before NNC6.4; NNC6.4 atomically installs real effect seams, cuts every provision caller over, and deletes every coarse authority. |
| A11 | Every prospective sub-item has an explicit dependency, path boundary, behavioral success criterion, failure proof, and deletion gate. |
| A12 | NNC6.5, NNC6.6, NNC6.1e2, NNC7, and NNC8.3 retain their existing teardown, resolution, recovery, integration, and cleanup authority. |
| A13 | The canonical band table, checkpoint ledger, Recovery Header, and routing index remain mutually recoverable with exactly one `in_progress` item. |
| A14 | Static plan verification, docs gates, proof writing lint, format/diff checks, and the one candidate-frozen structured item review pass with recorded results. |

## Current Source Census

The census is product-source only unless a row explicitly says otherwise.
Tests remain proof consumers, not production authorities.

| Concern | Current count | Source-derived result |
| --- | ---: | --- |
| Product `WorkloadSagaCoordinator` construction | 1 | `ComputeState` constructs the sole coordinator from the server-injected durable store. |
| Product `WorkloadNetworkPlanCompiler` call | 0 | The type and implementation exist; all compile calls are test-only. |
| Product `WorkloadSagaIntent` construction | 0 | No caller can create and submit a complete admitted desired generation. The two non-test `ConfirmedWorkloadSagaIntent::new` calls only wrap an already-built intent after store confirmation. |
| Product async lazy service-activation call | 1 | Convex `ctx.service` calls `ensure_service_binding_for_decision_async`. |
| Product sync service-name resolution call | 1 | Convex sync lookup calls `resolve_service_binding_for_decision`; it is read-only. |
| Product binding snapshots | 3 families | Convex invocation, Cloud Functions HTTP/callable, and Cloud Functions trigger execution snapshot already-active services. |
| `RuntimeServiceRegistry` production implementations | 2 | `ServiceInstanceBindingRegistry` is read-only; `ServiceManager` combines lookup, activation, and tenant teardown. |
| ServiceManager provider starts | 2 | Sandbox-backed service activation and standalone sandbox creation call `SandboxBackend::start`. |
| Standalone Compose direct start helper | 1 helper / 2 branches | Local Krun and forwarded Container branches both call the same `start_service_launch`, which calls `SandboxBackend::start` directly. |
| Forwarded Machine API sandbox start adapter | 1 implementation / 1 method | `ForwardedMachineApiSandboxBackend::start` selects image/build transport and enters `start_sync`; it is a host-side provider adapter, not desired-state authority. |
| Machine API start routes | 2 route call sites / 1 production facade | Image and build routes call `GuestNodeWorkloadService::start`, which materializes one PlanOnly bundle and submits one node assignment. |
| Node reconcile start dispatch | 1 product call site | `NodeWorkloadReconciler::reconcile_running` calls its injected `HostLifecycleBackend::start` only after exact inspect reports absent/not-running. |
| Node provider start implementations | 2 | `DirectProcessBackend` and `SystemdTransientUnitBackend` implement the provider-owned `HostLifecycleBackend::start` effect. |
| Product exact capability selection for a workload | 0 | `select_exact` is used by compiler tests and composition tests, not by a provision caller. |
| Product assigned-node source for local native service/sandbox calls | 0 | Native resource contexts carry neither a required local `NodeIdentity` nor a workload location before admission. |
| Product standalone Compose saga store | 0 | Compose receives `EnginePersistenceConfig` but does not open Engine or consume the server-owned saga adapter. |
| Product standalone Compose selectable bundle | 0 | `prepare_attachment_only` deliberately freezes an empty registry because no ingress source is present. |

The direct provider-start census is:

```text
crates/nimbus-services/src/manager/service_start.rs
  sandbox-backed service -> SandboxBackend::start

crates/nimbus-services/src/manager/sandboxes.rs
  standalone sandbox -> SandboxBackend::start

crates/nimbus-cli/src/compose/lifecycle.rs
  local Krun or forwarded Container -> start_service_launch
                                      -> SandboxBackend::start

crates/nimbus-cli/src/machine/backend.rs
  ForwardedMachineApiSandboxBackend::start -> start_sync
    -> Machine API image/build route

crates/nimbus-cli/src/machine/api/service_workloads.rs
  GuestNodeWorkloadService::start -> PlanOnly bundle materialization
                                  -> NodeWorkloadCoordinator

crates/nimbus-node/src/reconciler.rs
  NodeWorkloadReconciler::reconcile_running -> HostLifecycleBackend::start
    -> DirectProcessBackend::start OR SystemdTransientUnitBackend::start
```

The machine and node methods are provider adapter sinks. A future compute
command reaches them through the existing host boundary. They must accept the
exact workload generation and provider attempt, and the guest must not open
another saga store.

### Exact source transcript

The final census used `rg -n` over `crates/**/*.rs`, excluding `tests.rs` and
`tests/` paths, followed by source inspection for inline `#[cfg(test)]`
modules. The exact production-bearing results are:

| Query | Product result |
| --- | --- |
| `WorkloadSagaCoordinator::new`, compiler calls, intent constructors, confirmed wrappers | `state.rs:97` is the one coordinator construction; `ingress.rs:85,98` are two confirmed wrappers; zero compiler calls and zero intent constructors. |
| registry snapshots, resolution, activation, and implementations | Two production implementations in `nimbus-services`; Convex has one snapshot, one async activation, and one sync resolution call; Cloud Functions HTTP/callable and trigger paths each snapshot. Inline registry test hits are excluded. |
| services/Compose direct starts and composition | `service_start.rs:45` and `sandboxes.rs:51` are the two services-owned starts; `compose/lifecycle.rs:142,183,270,305` is one helper with two branches and one direct start; `compose/mod.rs:59,311,647` proves that persistence config is reduced to a control-data path while attachment-only composition is used. |
| Container/Krun phase boundaries and exact selection | Container `runtime.rs:236,585,942,982,1021,1029-1030` and Krun `vm.rs:569,574` plus `vm/lifecycle.rs:537,581,618-620` prove each public start reaches adoption, network configuration, readiness, and execution in one call. The compiler's `select_exact` call has no product compiler caller; CLI selection is composition-only. |
| forwarded Machine/guest node sinks | Host adapter `machine/backend.rs:68,462-492`; image/build routes `machine/api/routes.rs:94-133`; production facade `machine/api/service_workloads.rs:96,128-170`; node dispatch `nimbus-node/src/reconciler.rs:514`; real provider implementations `direct_process.rs:45,61` and `systemd_transient.rs:90,111`. Counts are `1` forwarded adapter, `2` routes, `1` facade, `1` reconcile dispatch, and `2` provider starts. |
| Cloud Functions negative capability | `nimbus-cloud-functions/src/host_bridge.rs:53-54` returns `None`; HTTP/callable `http/invocation.rs:87` and trigger `trigger_executor.rs:89` snapshot bindings; `host_bridge.rs:236-248` tests that `ctx_service_lookup` remains refused. |

No product source changed while producing this transcript.

## Current Call Graphs

### Convex runtime service lookup

```text
Convex handler / subscription
  -> RuntimeServiceRegistry snapshot for invocation
  -> ConvexHostBridge
       sync ctx.service  -> read-only resolve
       async ctx.service -> ServiceManager::ensure_service_binding_async
                            -> refresh / activation claim
                            -> service admission
                            -> SandboxBackend::start
                            -> wait for ready
                            -> binding projection
```

The asynchronous branch bypasses the durable workload saga, compiler, shared
network plan, and compute lifecycle coordinator. The synchronous branch is a
naming/read seam and must remain effect-free.

### Cloud Functions service projection

```text
Cloud Functions HTTP/callable or trigger
  -> RuntimeServiceRegistry::snapshot_for_tenant
  -> InvocationRequest.services
  -> runtime execution
```

`CloudFunctionsHostBridge::service_capabilities` returns `None`, and the host
bridge test requires `ctx_service_lookup` to remain an unsupported operation.
Cloud Functions is therefore not a current lazy-activation caller. Its NNC6.3
proof is negative: snapshots stay read-only and produce no provider effect.
Adding Cloud Functions lazy service lookup later requires a deliberate API and
capability decision. This plan does not smuggle it in through a shared trait.

### Native service and sandbox APIs

```text
authorized HTTP handler
  -> nimbus-compute response/orchestration function
  -> ServiceManager
       -> tenant admission
       -> source validation
       -> SandboxBackend::start
       -> in-memory handle/resource state
       -> observed nimbus-system evidence
```

The transport already delegates to compute, but compute immediately hands
lifecycle authority back to `ServiceManager`. Dynamic definitions, active
handles, standalone resources, sessions, and activation claims share one
mutex-backed manager state. That state can remain a definition, naming, and
session source, but it cannot remain the durable lifecycle or effect authority.

### Standalone Compose up

```text
Compose file selection
  -> staged LocalNetworkManager bootstrap
  -> attachment-only registry (empty selectable bundle set)
  -> local Krun or forwarded Machine API backend
  -> direct inspect
  -> direct SandboxBackend::start
  -> render provider handle
```

The command receives the canonical `EnginePersistenceConfig`, so the clean
composition is an embedded invocation of the same Engine-backed control plane.
Opening a CLI-private JSON or in-memory saga store would duplicate authority.

### Forwarded Machine API and guest node

```text
host ForwardedMachineApiSandboxBackend::start
  -> start_sync
  -> MachineApiClient image/build request
  -> one of two Machine API start routes
  -> GuestNodeWorkloadService::start
       -> Container PlanOnly bundle materialization
       -> NodeWorkloadCoordinator::reconcile_assignments
       -> NodeWorkloadReconciler::reconcile_running
       -> HostLifecycleBackend::start
            -> DirectProcessBackend OR SystemdTransientUnitBackend
```

The host adapter and guest facade currently expose coarse start operations.
The node backends are legitimate provider effects. The target keeps those
effect owners, but requires an exact compute-issued phase command at the host
boundary. The guest consumes the command and retains no workload-saga store or
cross-domain transition authority.

## Why The Original Item Cannot Be Implemented Coherently

### Phase ordering depends on NNC6.4

Container Execute `start` calls `plan_start` and then `finish_start`.
`launch_manifest` adopts the reserved attachment, configures networking,
starts the PEP, requires complete attachment readiness, and starts the runtime.
Krun follows the same shape: `start_sync -> finish_start -> execute_start ->
launch_manifest`, which adopts reservation authority, configures networking,
and launches into that network.

Neither backend currently gives compute independent, idempotent commands for
`PrepareWorkload`, `AttachNetwork`, and `ActivateWorkload`. An observer above
`SandboxBackend::start` cannot truthfully prove `start -> attach -> activate`.
it sees one coarse effect. NNC6.4 must therefore land before any effectful
caller cutover claims that order.

### Durable intent lacks executable content

The saga durably retains:

- desired kind, state, generation, and digest
- complete compiled network plan
- activation and publication intent
- admission evidence.

It does not retain the executable `SandboxSpec`. A digest authenticates
content, but cannot execute it. `ServiceManager` can resolve a current
definition in the live process, and Compose can reread a file. A fresh process
that reopens only Engine durability cannot reconstruct the admitted process,
root, resources, lifecycle policy, mounts, environment, or egress source. An
ingress-local cache would fail the fresh-process contract.

### Placement and provider selection are not admitted by callers

`WorkloadNetworkPlanCompiler` rejects a missing deployment generation, missing
assigned node, missing attachment-bearing capability selection, crossed source,
or sovereignty relaxation. Native service and sandbox contexts do not install
an assigned node. Product callers do not request an exact capability selection.
Choosing the first registered bundle would violate the exact-selection contract
and sovereignty semantics.

The local node identity must be an explicit composition value, persisted or
otherwise exactly reconstructable, and included in tenant admission before
intent compilation. Provider selection must come from the source-owned,
operator-admitted composition and must be authenticated by the durable intent.

### Standalone sandbox identity is learned too late

`ServiceManager::create_sandbox_resource_for_decision_async` receives the
provider-generated `SandboxId` only after `SandboxBackend::start` returns, then
uses that value as the API resource ID and inserts generation `1` into
in-memory state. A tenant-qualified logical workload ID and generation must
exist before reservation or provider effects. The provider sandbox handle
remains opaque provider evidence. It cannot become the logical workload key.

### Capability evidence does not yet describe standalone publication

The first admitted bundle pairs a sandbox-owned attachment registration with a
server-owned main-listener ingress registration. Standalone Compose freezes an
empty registry rather than fabricate the absent server role. Current sandbox
port publication is nevertheless performed inside the sandbox provider through
Netavark or machine forwarding.

The implementation must report this effect honestly. It may add a
sandbox-owned direct-publication ingress registration and explicitly admitted
attachment/publication bundles, or route publication through a real server
ingress adapter. It must not pair an inactive server registration merely to
make selection pass. HTTPS forwarding must distinguish TLS passthrough from
certificate termination. NNC7.6 retains certificate-authority separation.

## Frozen Target Ownership

| State or behavior | Sole owner after the split |
| --- | --- |
| Service definitions, logical names, read-only binding snapshots/resolution, sessions | `nimbus-services` |
| Tenant admission and service/sandbox policy | `nimbus-tenant` |
| Exact executable envelope and workload/network saga vocabulary | `nimbus-workloads` |
| Admitted executable/network compilation, lifecycle decisions, command order, transition CAS | `nimbus-compute` |
| Engine saga codec/store adapter and standalone embedded-store factory | `nimbus-server` |
| Portable network IDs, desired plans, leases, attachment state, capability evidence | `nimbus-network` |
| Container/Krun/Netavark/gvproxy/PEP/provider effects and provider handles | `nimbus-sandbox` |
| HTTP/WebSocket transport and server-owned protocol listeners | `nimbus-server` |
| Rebuildable routes/listeners/ports/service status | `nimbus-system` |

The dependency invariant remains:

```text
nimbus-network -> nimbus-core

nimbus-workloads -> nimbus-network + nimbus-tenant + nimbus-core
nimbus-compute   -> nimbus-workloads + existing upper capability owners
nimbus-server    -> nimbus-compute + nimbus-engine
nimbus-cli       -> nimbus-server + nimbus-compute + provider owners
```

No network-crate socket, sandbox, tenant, service, server, system, Iroh,
Netavark, Axum, Pingora, cloud SDK, or cluster edge is permitted.

## Prospective Implementation Split

### NNC6.3a: Exact executable carrier and desired digest

NNC6.3a adds a strict workloads-owned executable envelope to the saga. It
contains an explicit format/encoding identity and bounded canonical content.
It records the content digest and never exposes content through debug output.
Compute is the only sandbox spec encoder/decoder. `WorkloadSagaIntent` derives
the closed desired digest. The derivation covers kind, state, generation,
executable content, network plan, activation, and publication.

Success requires strict missing/unknown/duplicate/crossed/oversized/content-
digest rejection, exact replay, successor validation, fresh-process Engine
round-trip, no `nimbus-workloads -> nimbus-sandbox` edge, and zero provider
effects. Existing saga-v2 records are changed cleanly in place. No compatibility
decoder is added because Nimbus is pre-launch.

### NNC6.3b: Pure provision decisions and exact composition

After NNC6.3a, NNC6.3b adds only pure decision vocabulary and composition
inputs. A closed selector consumes confirmed saga state and one typed effect
result. It emits the next command or a terminal wait/failure decision without
calling a provider.

The admitted input includes deployment generation, local node identity, exact
source-owned provider selection, publication mode, forwarding mode, address
semantics, sovereignty evidence, and TLS behavior. Missing or crossed input
fails before intent persistence. The selector never chooses the first available
provider.

NNC6.3b promotes no product provider trait. It does not split the registry,
change a caller, retain a new compatibility adapter, or execute an effect.
Provider capabilities earn their seams only when real adapters and their
callers replace the old authority atomically in NNC6.4.

The result vocabulary distinguishes success, definite failure, and ambiguous
outcome. A definite failure leaves the last completed phase current and permits
no later action. An ambiguous outcome permits only inspection of the exact
generation and provider attempt.

### NNC6.4: Atomic provider-command and caller replacement

NNC6.4 depends on NNC6.3a and NNC6.3b. One candidate and one item commit add
the real provider commands, compute dispatcher, every caller cutover, and every
old-path deletion. No checkpoint preserves both an old and new provision
authority.

This item is intentionally one atomic breaking cutover. Splitting its effectful
parts would require a compatibility bridge or two active authorities. The
smaller NNC6.3a-NNC6.3b items absorb durable-content and pure-decision work
before that cutover. NNC6.4 may use bounded fail-before and implementation
chunks during development, but only its complete acceptance-bearing unit can
receive review or completion status.

The dispatcher persists this order:

```text
admit -> compile -> persist intent
      -> reserve -> persist NetworkReserved
      -> prepare -> persist WorkloadPrepared
      -> attach -> persist NetworkAttached
      -> inspect activation prerequisites
      -> activate -> persist WorkloadActivated
      -> inspect workload readiness
      -> publish -> persist Published
      -> observe
```

Attachment plus required PEP evidence authorizes activation. Workload
readiness authorizes publication. Prepare, attach, and activate cannot install
a host-routable endpoint. Only the publication command can do that.

Container, Krun, server ingress, Netavark publication, and forwarded-machine
publication keep their current effect owners. Small capability traits require
two real adapters or materially different consumers. `nimbus-network` owns
portable endpoint and lease state, but no socket, forwarding, or publication
effect.

Each command carries the logical workload ID, execution ID, generation,
provider attempt, desired digest, and exact predecessor evidence. Exact replay
is idempotent. A stale or crossed command fails before mutation. An ambiguous
effect inspects the exact attempt before retry.

A definitive reserve, prepare, attach, activation-prerequisite, activate,
workload-readiness, or publish error issues no later command. The coordinator
records recoverable failure evidence when the result can be persisted. If that
persistence is ambiguous, it reads the exact record before any decision.
NNC6.5 remains the sole compensation owner.

The same item cuts native service/sandbox creation, Convex async activation,
local and forwarded Compose, host Machine API, and guest-node provision paths
to compute. Convex sync resolution and all invocation snapshots remain
read-only. Cloud Functions HTTP, callable, and trigger execution remains
snapshot-only with zero activation, store, or provider calls.

Standalone Compose opens the canonical embedded Engine through the server-owned
saga adapter. A fresh command after process death inspects exact provider
evidence before it acts. The CLI owns no desired map or saga store.

The Machine API receives exact phase commands. Its guest node materializes
provider artifacts and delegates exact execution to the existing node
reconciler. DirectProcess and Systemd starts remain provider sinks. The guest
owns no second saga store or cross-domain transition coordinator.

The atomic deletion gate removes every coarse provision authority. That set
includes `SandboxBackend::start`, coarse Machine API start routes,
`start_service_launch`, direct services provider starts, ServiceManager
activation claims, and effectful registry methods. ServiceManager retains
definitions, logical names, read-only binding snapshots, resolution, and
sessions.

## Prospective Item Contract Matrix

| Item | Dependency | Owned paths | Required failure proof | Deletion gate |
| --- | --- | --- | --- | --- |
| NNC6.3a | NNC6.3 | workloads saga vocabulary; compute executable codec; server Engine codec/round-trip tests; verifier and item docs | Missing, unknown, duplicate, crossed, oversized, or digest-divergent content rejects before store/effects; fresh process reconstructs exact content | Delete digest-only desired acceptance, any executable cache authority, and compatibility decoding. |
| NNC6.3b | NNC6.3a | workloads/compute pure result and decision vocabulary; exact compute/server composition inputs; capability requirements; verifier and item docs | Missing/crossed generation, node, selection, source, publication, forwarding, address, sovereignty, or TLS evidence rejects before persistence/effects; definite failure stops; ambiguity yields inspect only | No product provider interface, effect, caller cutover, registry split, or compatibility adapter may exist in this item. |
| NNC6.4 | NNC6.3a-NNC6.3b | compute dispatcher; workloads transitions; network lease/endpoint state; sandbox Container/Krun/publication adapters; services catalog; server Convex/Engine/ingress composition; Cloud Functions negative tests; CLI Compose/Machine API; node command adapter; verifier and item docs | Exact crash cut after every effect; stale/crossed and definitive failure matrix; ambiguous inspect-before-retry; no routability before publish; native/runtime/Compose concurrency; fresh host and guest processes; Cloud Functions zero-effect proof | In the same candidate, delete `SandboxBackend::start`, coarse Machine API starts, `start_service_launch`, direct services starts, ServiceManager activation/effectful registry authority, CLI-local desired/store state, and every other coarse provision path. |

NNC6.4a depends on NNC6.4. NNC6.5, NNC6.6, and NNC6.1e2 remain later
convergence owners rather than cleanup escape hatches for these items.

## Failure And Proof Matrix

| Boundary | Required proof |
| --- | --- |
| Before exact executable encoding | No saga write, reservation, or provider call. |
| Executable content crossed under a valid outer record | Strict rejection before effect; record bytes unchanged. |
| Missing deployment generation, node, or provider selection | Admission/compile failure with zero store and effect calls. |
| Intent CAS ambiguous | Fresh read; no command unless exact durability is confirmed. |
| Crash after intent persistence | Fresh process reconstructs exact executable/network content and deterministic next action. |
| Crash after each provider effect before phase CAS | Inspect exact generation/provider attempt before retry; no duplicate effect. |
| Definite reserve/prepare/attach/prerequisite/activate/readiness/publish failure | Exact last completed phase remains current; recoverable failure evidence is recorded when persistence is available; no later command or publication runs. |
| Stale or crossed command | Provider rejects before mutation; saga remains unchanged. |
| Cancellation before durable intent | No store or effect call. |
| Cancellation after durable intent | Desire remains recoverable; no rollback fiction. |
| Container/Krun prepare | No tenant instruction and no routable publication. |
| Attach without current prepare | Typed rejection; no activation or publication. |
| Attach/activate before publish | No host-routable endpoint exists. |
| Activate without current attachment and PEP evidence | Typed rejection; no tenant instruction or publication. |
| Publish without exact workload readiness | Typed rejection; no listener, forwarding, route, or endpoint effect. |
| Ambiguous publish | Inspect exact owner-local publication attempt before retry; never allocate or forward twice. |
| Native service concurrent activation | One durable logical generation and at most one provider attempt. |
| Standalone sandbox create | Logical tenant-qualified ID precedes every provider effect. |
| Convex async lookup | Uses compute activation; concurrent requests converge on one generation. |
| Convex sync lookup | Read-only; zero store/provider calls. |
| Cloud Functions HTTP/callable/trigger | Snapshot-only; zero activation/store/provider calls. |
| Compose process death | Reopen Engine and inspect; no CLI-local desired map or duplicate start. |
| Crossed Machine API or guest-node command | Reject before bundle, node, forwarding, or execution effects; guest opens no saga store. |
| Provider capability substitution | Exact request fails closed; no first/available fallback. |
| HTTPS direct publication | Passthrough and termination evidence cannot be substituted. |
| Atomic cutover deletion | No `SandboxBackend::start`, coarse Machine API start route, `start_service_launch`, direct services start, or ServiceManager effectful registry authority remains. |

## Retained Later Owners

- NNC6.4a owns eligible Container/Krun restart and explicit service restart.
- NNC6.5 owns withdrawal, stop, failed-provision compensation, force deletion,
  Compose down, and tenant-retirement caller cutover.
- NNC6.6 owns resolver fencing while withdrawal is in progress.
- NNC6.1e2 owns final fresh-process startup recovery and tenant-retirement
  convergence after all caller paths exist.
- NNC7 owns protocol-listener integration, service endpoint generations,
  portable handle projection, system projection, and certificate/telemetry
  guardrails.
- NNC8.3 owns cleanup finalization, orphan removal/quarantine, release, and
  capacity reuse.
- `horizontal-scaling-plan.md` continues to own cluster membership, node
  identity distribution, placement replication, and fenced super-net leases.
- `service-identity-provider-auth-plan.md` continues to own provider credential
  minting. Executable content stores references, not a second credential
  authority.

## Path Boundaries

This audit checkpoint may change only:

- this proof
- `docs/private/plans/nimbus-network-control-plane-plan.md`
- `docs/private/plans/README.md`
- `scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh`
- `scripts/verify-nimbus-network-control-plane.sh`.

The script allowance is narrow. The NNC6.1e1 contract may classify later
canonical `nnc*.md` proof records as documentation rather than source. Its
unexpected product-path mutation must remain red. The aggregate script may
change only the exact self-test count.

NNC6.3a may change workloads saga vocabulary, compute encoding, the server
codec/adapter tests, the static contract, and item docs. NNC6.3b may change
pure workloads/compute decision vocabulary, exact compute/server composition
inputs, portable capability requirements, tests, the static contract, and item
docs. It may not change a provider or caller.

NNC6.4 owns the atomic cutover named in its contract row. Its paths span
compute, workloads, network state, sandbox, services, server, CLI, Machine API,
and the node command adapter. It also owns Cloud Functions negative tests. No
split item may edit a later teardown, restart, resolution, projection, cleanup,
or cluster owner merely to make its local proof pass.

## Structured Review Disposition

The sole complete-item review used engine `codex`, model `gpt-5.6-sol`,
thinking `xhigh`, and fast service tier. It reported four findings and rated the
candidate incorrect at `0.98`.

| Finding | Disposition | Correction |
| --- | --- | --- |
| P1: split preserves legacy activation paths and deletes them at impossible times | Accepted | Removed NNC6.3c-NNC6.3e and every temporary adapter/bridge. NNC6.3b is pure. NNC6.4 now adds replacements and deletes all coarse provision paths in one candidate and commit. |
| P1: no owner or command for routable publication | Accepted | NNC6.4 now gives publication an idempotent owner-local command, exact readiness predecessor, ambiguity inspection, and negative gates that keep prepare/attach/activate unroutable. |
| P2: no definite provider-failure proof | Accepted | Pure vocabulary and the NNC6.4 matrix now distinguish definite failure from ambiguity. Every named definite failure preserves the exact phase, records recoverable evidence when possible, and issues no later command. |
| P2: machine/node sinks are unenumerated | Accepted | The census and call graph now name and count the forwarded adapter, two Machine API routes, guest facade, node dispatch, and two real host-backend start implementations. |

The accepted corrections change documents only. Per owner cadence, they require
affected docs/static proof but no narrow review by themselves. The later gate
found one executable verifier-classification defect, so that isolated script
correction receives the cadence's sole narrow review.

That sole narrow review used engine `codex`, model `gpt-5.6-sol`, thinking
`xhigh`, fast service tier, and no web search. It reported no findings and rated
the correction correct at `0.99`. The review independently confirmed the
anchored canonical-proof matcher and the arbitrary-path fail-closed boundary.
It also confirmed exact
`13/13` and `228/228` arithmetic, both aggregate self-test repairs, the four
accepted planning corrections, and zero product-source changes. No further
NNC6.3 review is warranted.

## Verifier Classification Correction

Fail-before aggregate verification passed NNCV000-NNCV029 and failed NNCV030.
The exact diagnostic named this proof as an NNC6.1e1 source-diff escape, so the
aggregate result was `30/31`.

The corrected classifier admits canonical
`docs/private/plans/proof/nimbus-network-control-plane/nnc*.md` records. It does
not admit arbitrary private docs or product paths. The focused contract passes
`10/10`. Its self-test passes one positive later-proof case and all 12 existing
fail-closed mutations (`13/13`). Bash syntax passes. The aggregate exact count
changes from `227` to `228`.

The first retained aggregate rerun exposed two stale self-test expectations in
the aggregate script itself. Its checkpoint fixture assumed the commit hash
immediately followed the table label, and its legacy-port mutation still
expected the former 30-condition summary. The fixture now replaces the hash
within the exact ledger row and fails loudly if it cannot build. The summary
expects `30 passed, 1 failed`. A retained rerun then passed all `228` mutation
proofs with exit status zero.

## Evidence Ledger

| Checkpoint | Evidence |
| --- | --- |
| Read-only audit base | `26df5075d7dab582a4c9602e248993eabd8eab49`; owner worktree clean; original checkout untouched. |
| Source census | Exact non-test `rg -n` queries plus inline-`cfg(test)` inspection produce the counts and paths above: coordinator `1`, compiler calls `0`, intent constructors `0`, confirmed wrappers `2`, async activation `1`, sync resolution `1`, snapshot families `3`, registry implementations `2`, services starts `2`, Compose helper/branches `1/2`, forwarded adapter `1`, Machine routes `2`, guest facade `1`, node dispatch `1`, and real node provider starts `2`. |
| Plan split | NNC6.3 plus NNC6.3a-NNC6.3b and revised atomic NNC6.4 occur once in both task and checkpoint ledgers; NNCV008 must accept the exact bijection, one active row, current header, and full checkpoint hash. |
| Quality/docs | New-proof writing lint passes with zero errors and 10 advisory warnings; `cargo fmt --all --check`, `git diff --check`, docs `108`, and site `17/17` pass. The legacy plan/index retain a pre-existing document-wide writing-lint backlog, so their item-owned sections receive manual inspection plus the repository docs gates rather than a false whole-file pass. |
| Candidate identity | The complete substantive three-document candidate reviewed first had staged tree `842f6d4881633074660b01bc9bf82e8359870783` and binary patch SHA-256 `de017851d7d6f39fe72d34cdddad76277542c6ed4734830e74a0718d7bc1c9d0`. The corrected five-path substantive candidate before identity wording has staged tree `ec67da6dcf74b5ed0ac1daac3aad404da0534bc5` and binary patch SHA-256 `78941ed4e8036bc92648ccff6d75fe77a2bfd027280cb06b8e13b8f4898b3c9b`. |
| Structured review | Sole full actual GPT-5.6 Sol/xhigh/fast review: two P1 plus two P2, all accepted, overall `0.98`. The sole narrow Sol/xhigh/fast verifier-correction review is clean with no findings at `0.99`; no further review is warranted. Its reviewed staged tree before closeout wording is `313c351d2c980cae97c15d1e28d94fceb91dd5b4`, with binary patch SHA-256 `f90e11fdbaf7df832a25a068e5aefb8e909e5dbf402b126f60a0c70cafc41b2c`. |
| Verifier fail-before/correction | Aggregate expected red `30/31`, with only NNCV030 rejecting this canonical proof path. Corrected focused contract `10/10`, self-test `13/13`, live verifier `31/31`, retained aggregate mutations `228/228`, Bash syntax, helper ShellCheck, format, and diff checks pass. The first retained aggregate attempt correctly found and drove the two stale self-test expectation repairs described above. |
| Final commit | This proof, routing edit, plan checkpoint, and two verifier corrections commit together as the NNC6.3 item. Resolve the exact durable commit with `git log -1 --format=%H -- docs/private/plans/proof/nimbus-network-control-plane/nnc6.3-provision-choreography-substitution-audit.md`; the NNC6.3a checkpoint promotes that hash on its first edit. |

## Acceptance Traceability

| Clause | Candidate evidence |
| --- | --- |
| A1-A7 | Correction complete: exact source transcript, machine/node counts, and current call graphs above. |
| A8-A12 | Correction complete: no compatibility bridge, atomic deletion order, explicit publication owner, definite-error behavior, target ownership, retained-owner map, and failure matrix. |
| A13 | Canonical plan/ledger/routing correction is complete; NNCV008 passes in live `31/31` and aggregate `228/228` verification. |
| A14 | The sole full review and sole narrow correction review are dispositioned; live `31/31`, aggregate `228/228`, focused `10/10` and `13/13`, docs `108`, site `17/17`, proof lint, syntax, ShellCheck, format, and diff gates are green. |
