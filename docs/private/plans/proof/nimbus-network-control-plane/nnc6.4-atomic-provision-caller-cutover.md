# NNC6.4 Atomic Provision Caller Cutover

Status: `in progress. preparation durable; portable protocol in progress`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Activation commit: `76c920a12ed21eb8b81c1de088bcad52fd0d81e4`

Dependency commit: `c42c61fb2d97d037069f3b27b9055d6e58f11d1d`

Preparation commit: `eb6adfc5516ae1f7661ff04009ca2bf48c893295`

## Outcome

NNC6.4 replaces every coarse provision authority with one compute-coordinated,
generation-fenced command protocol. The item is intentionally atomic: no
durable candidate may preserve both the old caller authority and the new
dispatcher authority.

The source audit found two protocol facts that NNC6.3b could not yet express:

1. whether this coordinator call won the exact compare-and-swap that installed
   a dispatch claim. and
2. whether a side-effect-free provider inspection proved the exact attempt
   absent, which is the only observation that can authorize a retry.

NNC6.4 must add both facts before it dispatches an effect. A replay, an
ambiguous commit that is later observed, and a fresh-process recovery may only
inspect. Only the direct CAS winner for an exact durable dispatch claim may
execute. An exact absence observation authorizes the same stable attempt at the
next monotonic dispatch epoch.

## Audit Integrity

Three read-only audit lanes covered the item before product edits:

- Container, Krun, native service/sandbox, and Convex activation.
- local and forwarded Compose, Machine API, guest node, DirectProcess, and
  Systemd. and
- portable protocol, coordinator CAS semantics, acceptance proofs, and the
  NNCV033 verifier contract.

The audits changed zero paths and ran no Cargo command, provider effect, or
structured review. They used bounded `rg`, `sed`, `nl`, `wc`, and Git reads.
The owner worktree was clean at `76c920a12ed2` before this proof was created.
The provider-target audit found and corrected one preimplementation design
error: workload execution effects cannot use network capability identity. The
frozen contract now separates exact network Attachment/Ingress targets from a
neutral execution-provider target before either target enters product code.

## Scope

NNC6.4 owns:

- one portable dispatch claim, monotonic dispatch epoch, exact absence
  evidence, and closed inspection-result vocabulary.
- one ephemeral confirmed command that only the compute coordinator can
  construct.
- exact CAS provenance that distinguishes the direct winner from replay and
  ambiguity confirmation.
- one compute dispatcher over small phase capabilities.
- real Container, Krun, server-ingress, forwarded-machine, guest-node,
  DirectProcess, and Systemd substitutions.
- product caller replacement for native service/sandbox and Convex async
  activation.
- product caller replacement for Compose, Machine API, guest node, and the
  hidden node-workload executor.
- separation of non-routable attachment from owner-local publication.
- every behavioral, crash, concurrency, fencing, fresh-process, static,
  dependency, and deletion proof listed here.
- deletion of every coarse provision authority in the same candidate.

NNC6.4 does not own:

- restart policy or restart dispatch, which remain NNC6.4a.
- teardown, withdrawal, compensation, or tenant retirement, which remain
  NNC6.5 and NNC6.1e2.
- resolver fencing, which remains NNC6.6.
- cleanup finalization and reuse, which remain NNC8.3.
- logical service naming, service definitions, sessions, or read-only binding
  projection, which remain `nimbus-services`.
- tenant admission policy, egress PDP, proxy PEP/forwarding, certificate
  providers, system projections, cluster transport, or machine-provider
  selection.
- a provider handle, socket, namespace, IP address, assigned port, or other
  owner-local effect state in the portable saga.
- a saga store or coordinator in the guest, CLI, service manager, sandbox,
  network crate, or provider adapter.
- any effect trait or transport in `nimbus-network`.
- any cloud SDK, Axum, Pingora, Netavark, nftables, gvproxy, Iroh, tenant,
  sandbox, service, server, system, or machine edge in `nimbus-network`.

## Current Ownership And Call Graph

```text
native sandbox route
  -> nimbus-compute sandbox facade
  -> ServiceManager::create_sandbox_resource_*
  -> SandboxBackend::start

native service start / Convex async ctx.service
  -> nimbus-compute service facade OR effectful RuntimeServiceRegistry
  -> ServiceManager activation claim
  -> ServiceManager::start_sandbox_service_async
  -> SandboxBackend::start

local Compose
  -> start_service_launch
  -> SandboxBackend::start

forwarded Compose
  -> ForwardedMachineApiSandboxBackend::start
  -> parent publication journal with a second random attempt identity
  -> coarse Machine API image/build start route
  -> GuestNodeWorkloadService::start
  -> PlanOnly materialization
  -> NodeWorkloadCoordinator
  -> NodeWorkloadReconciler inspect-then-start
  -> DirectProcess or Systemd provider start

hidden node-workload executor
  -> direct admission from CLI flags
  -> NodeWorkloadCoordinator

Container/Krun SandboxBackend::start
  -> prepare + attach + publish + activate + readiness
```

The current product has zero callers of `compose_workload_provision` and zero
callers of `WorkloadSagaCoordinator::submit_intent`. The current shared OCI
attachment state machine installs listener/publication effects before its
`Active` attachment phase. Hiding `SandboxHandle.published_endpoints` until
readiness does not remove already-installed DNAT or forwarding.

## Target Ownership And Call Graph

```text
native, Convex async, Compose, Machine API, or hidden executor intent
  -> source-owner immutable snapshot
  -> nimbus-compute::compose_workload_provision
  -> sole WorkloadSagaCoordinator::submit_intent
  -> pure WorkloadProvisionDecision
  -> exact candidate CAS
     -> direct winner: ConfirmedWorkloadProvisionCommand::Execute
     -> replay/ambiguity/recovery: ConfirmedWorkloadProvisionCommand::Inspect
  -> WorkloadProvisionDispatcher
     -> exact small capability selected by step + provider target
     -> owner-local journal/manifest/unit/lease fence
     -> closed command or inspection result
  -> pure reducer
  -> exact successor CAS

read-only Convex sync lookup / invocation snapshots / Cloud Functions snapshots
  -> ServiceInstanceBindingRegistry or immutable InvocationServices
  -> no intent, store, dispatcher, provider, or activation call
```

Compute remains the sole cross-domain coordinator. Workloads owns portable
saga vocabulary. Provider owners keep effects and opaque handles. The Engine
adapter remains the sole durable product store.

## Frozen Architecture Decisions

### D1: Preserve exact CAS provenance

The coordinator distinguishes:

- `AppliedByThisCall`.
- `ConfirmedAfterAmbiguity`.
- `ConfirmedReplay`.
- `Conflict`. and
- `UnresolvedAmbiguity`.

Only `AppliedByThisCall` for the exact `DispatchPending` candidate can create
an `Execute` command. `ConfirmedAfterAmbiguity` and `ConfirmedReplay` create an
`Inspect` command. `Conflict` and `UnresolvedAmbiguity` create no command.

The current ambiguous-commit resolver returns `Applied` after observing the
candidate. That loses provenance and must change. It may report the candidate
as confirmed, but it may not grant direct-winner authority.

### D2: Retain a stable attempt and monotonic dispatch epoch

`nimbus-workloads` owns a decimal `WorkloadProvisionDispatchEpoch`. A dispatch
claim contains:

- the complete stable `WorkloadProvisionAttempt`.
- the dispatch epoch.
- the exact network or execution provider target.
- `Initial` or `RetryAfterAbsence` authorization. and
- exact absence evidence for a retry.

The attempt ID is stable across an absence-authorized retry. It binds logical
operation identity: saga/key, generation, desired/source/network digests,
required node, selection evidence, source/target phase, step, subjects, and
prerequisite. The dispatch epoch is a separate execution fence. The initial
epoch is zero. A retry increments it by exactly one.

A retry cannot change the attempt ID, skip an epoch, reuse an epoch, change a
provider, or omit exact absence evidence. A delayed lower epoch fails before
owner-local mutation.

### D3: Model side-effect-free inspection explicitly

The closed inspection result distinguishes:

- exact success with step-specific evidence.
- exact definite failure.
- exact absence.
- in progress. and
- unavailable or ambiguous observation.

Only an `Inspect` command can return absence. Absence evidence binds command,
attempt, epoch, provider, step, subjects, and a digest of the provider
observation. It authorizes one same-attempt next-epoch candidate CAS.

`InProgress` and unavailable/ambiguous inspection remain inspection-only.
They never authorize execution. A success or definite failure reduces through
the same pure state machine as a direct command result.

### D4: Commands are ephemeral and unforgeable outside compute

`nimbus-compute` owns `ConfirmedWorkloadProvisionCommand`. Its constructors are
private to `WorkloadSagaCoordinator`. Its domain-separated command ID binds:

- key, saga ID, attempt ID, generation, and desired digest.
- issuing and confirmed revisions plus transition ID.
- source and network-plan digests.
- the exact network or execution provider target.
- source phase, target phase, step, subjects, and prerequisite.
- dispatch epoch. and
- `Execute` or `Inspect` mode.

Every provider result echoes the command ID, attempt ID, dispatch epoch,
provider target, and closed outcome. Crossed results fail before a successor
CAS.

### D5: Use small earned capabilities

The dispatcher depends on these command-specific capabilities:

- network reservation.
- workload preparation.
- non-routable network attachment.
- activation-prerequisite inspection.
- workload activation.
- workload-readiness inspection.
- ingress publication. and
- publication inspection.

The exact names may improve during implementation, but the capabilities stay
step-specific. There is no `NetworkProvider`, `SandboxLifecycleProvider`, or
other god interface.

Container and Krun must substitute for the same preparation, attachment,
activation, and readiness seams. Server ingress and forwarded-machine
publication substitute at the publication seam. DirectProcess and Systemd
substitute as exact activation sinks behind the guest adapter.

### D6: Provider-local journals fence effects

Saga CAS fencing cannot stop a delayed provider call by itself. Every effect
owner retains durable or inspectable attempt state keyed by stable attempt ID
and fenced by dispatch epoch.

- Exact replay adopts or returns existing evidence.
- A lower epoch fails before mutation.
- A higher epoch requires the exact prior absence evidence.
- Concurrent equal claims produce one external effect.
- Opaque provider handles stay in the provider owner.
- Compute persists only typed evidence and its digest.

Container and Krun use their durable manifests and network journals. The
forwarded parent uses its publication journal, now bound to canonical command
identity. Server ingress uses durable `PortLease` and listener evidence. The
Systemd sink records the exact fence in inspectable unit properties and uses
fail-if-present creation. DirectProcess keeps the same contract in memory and
does not claim fresh-process durability.

### D7: Attach is non-routable. publish is the first routable effect

The shared OCI attachment state machine currently mixes attachment and
publication. NNC6.4 separates them without creating a second compensation
owner.

Reserve, prepare, attach, activation-prerequisite inspection, activation, and
workload-readiness inspection cannot install an ingress listener, DNAT rule,
gvproxy forwarding path, public route, or resolvable endpoint. Only an exact
`Publish` command after exact workload-readiness evidence can activate those
effects. Publication remains idempotent and inspect-before-retry.

Provider-internal egress PEP setup remains an activation prerequisite, not
ingress publication. PDP, PEP forwarding, certificate, and service-name
ownership do not move.

### D8: Services become source and projection owners only

`nimbus-services` retains service definitions, logical names, sessions,
read-only binding resolution, and observed handle/catalog projection. It loses
activation claims, provider starts, and effectful runtime-registry methods.

`ServiceDefinitionCatalog` must return one immutable source-owned snapshot
that preserves generation and resource version beside the backend definition.
Compose must implement the same source contract. Neither caller may invent a
generation or hash an incomplete backend projection.

Cancellation after accepted intent cancels only the waiting caller. It does
not erase durable desire or create a second activation owner.

### D9: Every product caller enters compute

The atomic cutover covers:

- native standalone sandbox creation.
- explicit native service start.
- Convex async `ctx.service` activation.
- local Compose.
- forwarded Compose.
- Machine API exact phase commands.
- guest-node phase sinks. and
- the hidden node-workload executor.

Convex synchronous lookup and all invocation snapshots remain read-only.
Cloud Functions HTTP, callable, and trigger paths remain snapshot-only with
zero activation, saga-store, dispatcher, or provider call.

The hidden node-workload executor must submit through Engine/compute or be
deleted. It cannot retain direct admission plus a coarse coordinator call.

### D10: Machine transport carries exact phase commands

The two coarse image/build Machine API start request types, routes, handlers,
and client methods are replaced. The wire protocol carries a strict exact
phase command and rejects unknown fields. There is no compatibility route.

The parent-forwarded journal binds canonical attempt, execution ID,
generation, desired/source/network digests, machine-provider generation, and
forwarder authority. It no longer invents a random parallel attempt ID.

The guest accepts the already admitted command. It may materialize artifacts
and invoke node providers, but it cannot re-admit, choose saga order, persist a
saga, or coordinate retries. WSL2 remains fail-closed where host-managed
Netavark/gvproxy capability is required.

### D11: Node effects are exact provider sinks

The guest adapter sends an exact activation command to the node provider.
Systemd uses fail-if-present creation rather than replacement, records the
complete fence in unit properties, and adopts only an exact inspect match.
DirectProcess rejects stale/crossed claims and makes exact replay idempotent.

`NodeWorkloadCoordinator` may remain the compute-owned node adapter. It may not
infer a new provision attempt from desired state. Restart remains NNC6.4a.
Stop remains NNC6.5.

### D12: Definite failure stops exactly. later work compensates

A definite reserve, prepare, attach, activation-prerequisite, activate,
readiness, publish, or observation failure retains the last completed phase
and exact failed claim. It issues no later command and never publishes.

NNC6.4 records recoverable failure evidence when the durable owner is
available. It does not compensate. NNC6.5 remains the sole compensation and
teardown owner.

### D13: Product callers use one compute provision facade

`WorkloadProvisioner` is the product-level compute seam. It owns pure
composition, durable submission, bounded drive/resume, and a keyed tracked
supervisor. Product callers cannot sequence the coordinator, dispatcher, or
driver. Cancellation before entry produces no source/store/provider mutation;
after a tracked submission begins, cancellation ends only that waiter while
the exact durable operation continues or is resumed.

Outer composition injects one canonical `NodeIdentity`, one immutable exact
provider-report registry, one explicit provider selection, one sovereignty
posture, one real capability registry, and one saga store. Compute never picks
the first registered provider and never assumes that a parent
`LocalNetworkManager` describes the forwarded-machine provider realm. The
host-local case may clone its already frozen manager registry; the forwarded
case injects separately authenticated Machine provider facts.

The canonical embedded local-node constructor lives above the portable
`NodeIdentity` type and replaces the isolated CLI string literal. Admission
binds the source-owned generation and that exact node before pure composition.

### D14: Desired source and observed projection are separate

Services stores standalone sandbox desired source before any provider effect:
tenant-qualified stable resource ID, profile, complete spec, generation,
resource version, labels, and timestamps. Provider observation is a separate,
optional, generation-fenced projection. Service definitions are already
desired source and gain the same generation-fenced observed projection.
Stale or crossed observations cannot overwrite a newer generation.

Standalone creation receives its stable resource ID from the product caller;
services never generates it inside the reservation or effect path. The native
create contract therefore carries an explicit client-stable ID. Exact replay
of that ID adopts byte-identical desired source, while crossed profile, spec,
labels, generation, resource version, tenant, or decision identity fails
before source mutation or provider work. Provider-attempt identity remains a
separate owner-local effect fence and is never substituted for product
resource identity.

`ServiceDefinitionCatalog` returns one complete `ServiceDefinition`, not a
backend-only value. Static and Compose definitions start at generation `1`;
their resource version is a source-owned digest of the complete normalized
definition, so a caller never invents generation or hashes an incomplete
projection. Normal synchronous callers may wait for `Observed`; a waiting or
cancelled call exposes truthful accepted/pending state rather than fabricating
a handle.

Definition mutation/deletion synchronization, if still required, receives a
narrow definition-owned gate. The activation set, activation claim, direct
start methods, and effectful runtime-registry methods are deleted rather than
renamed.

### D15: Compose publication and restart behavior are explicit

Standalone Compose opens the canonical Engine-backed saga store. A service
with published port bindings freezes a complete Krun plus real
server-ingress bundle and publishes only through
`ServerIngressPublicationAdapter` after readiness. A service with no published
binding uses explicit `Withheld` publication; an empty registry is not used to
stand in for a publishing provider.

`nimbus compose up` is the foreground process owner documented by the node
lifecycle contract. After it prints the ready result, it retains the Engine,
network manager, sandbox adapter, and live ingress workers until cancellation
or process stop; it cannot return successfully while its published endpoints
still claim routability. Dropping that foreground owner withdraws and settles
its process-bound listeners. A second process cannot claim the same live
network realm, and a later invocation reopens the same Engine-backed saga
rather than creating a CLI journal or a second provider attempt. Detached
Compose ownership would require a deliberately surfaced daemon/provider and
is not invented inside this item.

The native restart stop/start route is deleted in NNC6.4. NNC6.4a later adds
restart as a new durable desired transition; no hidden coarse start authority
is retained as a bridge. The internal hidden node-workload executor is deleted
rather than given a second local saga composition.

### D16: Observed provider state crosses two earned read seams

`WorkloadProvisionRun` remains portable durable truth: its saga record retains
stable references and evidence digests, never a `SandboxHandle`,
`SandboxInspection`, bound socket, assigned port, provider handle, or other
owner-local state. Provider phase evidence therefore cannot double as the
services projection payload.

The one immutable compute capability registry gains two small, exact,
effect-free observation capabilities instead of a second registry or god
provider. An execution observation is keyed by the admitted
`WorkloadExecutionProviderId` and returns an ephemeral, authenticated sandbox
inspection. An ingress endpoint observation is keyed by the admitted ingress
`NetworkProviderId` and returns the actual listener/lease-bound endpoints for
the complete compiled plan witness. The split is mandatory: sandbox inspection
can reconstruct desired guest bindings, but only the ingress owner knows a
provider-assigned host port after bind.

`WorkloadProvisioner` invokes these reads only after the durable run is exactly
`Observed`, inside the retained tracked operation and before completing its
waiters. It validates tenant, source identity, execution ID, generation,
resource version, provider selection, complete listener/lease membership, and
the actual binding fences before asking `nimbus-services` to project. Withheld
publication requires zero ingress endpoints. Missing, stale, crossed,
in-progress, or ambiguous evidence produces truthful pending or rejected state,
mutates no projection, and never restarts, repairs, rebinds, or redispatches a
provider effect. Exact replay repeats only reads plus an idempotent services
projection. IP addresses and ports remain route output, never workload
identity.

### D17: Bind each command to its actual provider authority

`NetworkProviderId` identifies a selected network capability. It does not
identify a Container, Krun, DirectProcess, or Systemd execution authority.
The portable protocol uses one closed `WorkloadProvisionProviderTarget`:

| Step | Target | Exact fence |
| --- | --- | --- |
| Reserve network | Network attachment capability | `NetworkCapabilityRole::Attachment`, `NetworkProviderId`, and the admitted combined `NetworkCapabilitySourceDigest`. |
| Prepare workload | Execution provider | `WorkloadExecutionProviderId` and `WorkloadProvisionSourceDigest`. |
| Attach network | Network attachment capability | `NetworkCapabilityRole::Attachment`, `NetworkProviderId`, and the admitted combined `NetworkCapabilitySourceDigest`. |
| Inspect activation prerequisites | Execution provider | The execution adapter inspects exact attachment, dependency, and execution prerequisites. It excludes selected ingress listeners. |
| Activate workload | Execution provider | `WorkloadExecutionProviderId` and `WorkloadProvisionSourceDigest`. |
| Inspect workload readiness | Execution provider | `WorkloadExecutionProviderId` and `WorkloadProvisionSourceDigest`. |
| Publish or observe publication | Network ingress capability | `NetworkCapabilityRole::Ingress`, `NetworkProviderId`, and the admitted combined `NetworkCapabilitySourceDigest`. |

A resource-free reserve or attach step creates no provider command and no
fabricated provider target. The pure reducer records the no-resource
transition. It cannot publish or make an endpoint reachable.

This split reuses the existing combined network selection digest. It does not
add an attachment-only report digest, a second provider registry, or a third
provider authority for the egress PEP. The PEP remains an attachment-owned
plan prerequisite. The execution target carries a neutral execution-provider
ID rather than overloading the attachment role.

Capability satisfaction applies lifecycle requirements to the provider role
that owns them. Attachment requires `DurableInspect`, `Reconcile`, and
`Delete`; NNC6.4 ingress requires only the honestly earned
`DurableInspect` and `Reconcile` publication behavior. Ingress withdrawal and
absence-proven deletion remain NNC6.5 and cannot be advertised early merely
to satisfy a shared set. The requirements value therefore carries explicit
attachment and ingress lifecycle sets; there is no uniform compatibility
constructor.

HTTP or HTTPS endpoint protocol does not imply an L7 ingress provider. A
transparent TCP publisher forwards admitted bytes without owning HTTP
streaming, WebSocket framing, path routing, or TLS termination, so the network
compiler must not inject those L7 features from endpoint protocol alone. An
explicit source requirement for any such feature remains authoritative and
must reject an ingress report that does not offer it.

## Exact Phase Protocol

| Durable phase/disposition | Durable action | Authorized command | Success | Failure or uncertainty |
| --- | --- | --- | --- | --- |
| `IntentCommitted / Ready` | CAS reserve claim at epoch 0. | Direct winner executes. all other confirmations inspect. | CAS `NetworkReserved / Ready`. | Definite failure retains `IntentCommitted`. ambiguity requires inspection. |
| `NetworkReserved / Ready` | CAS prepare claim at epoch 0. | Execute or inspect exact preparation. | CAS `WorkloadPrepared / Ready`. | Retain `NetworkReserved`. no attach. |
| `WorkloadPrepared / Ready` | CAS attach claim at epoch 0. | Execute non-routable attach or inspect. | CAS `NetworkAttached / Ready`. | Retain `WorkloadPrepared`. no activation. |
| `NetworkAttached / Ready`, prepare-only | No CAS. | None. | Wait. | No activation or publication. |
| `NetworkAttached / Ready`, activate | CAS prerequisite-inspection claim. | Read-only inspection. | CAS distinct activation claim with prerequisite evidence. | In progress waits. definite failure halts. ambiguity inspects. |
| `NetworkAttached / DispatchPending(Activate)` | Claim already durable. | Direct winner activates. others inspect. | CAS `WorkloadActivated / Ready`. | Retain `NetworkAttached`. no readiness/publish. |
| `WorkloadActivated / Ready` | CAS workload-readiness inspection claim. | Read-only inspection. | CAS `Ready / Ready`. | In progress waits. definite failure halts. ambiguity inspects. |
| `Ready / Ready`, withheld | CAS pure `Observed / Ready`. | None. | `Observed`. | No publication call. |
| `Ready / Ready`, publish | CAS publish claim at epoch 0. | Direct winner publishes. others inspect. | CAS `Published / Ready`. | Retain `Ready`. ambiguity inspects. |
| `Published / Ready` | CAS publication-observation claim. | Read-only inspection. | CAS `Observed / Ready`. | Retain `Published`. absence is not observed success. |
| Any reopened `DispatchPending` | Load and authenticate exact record. | Inspect only. | Reduce exact observation. | Never execute from replay. |
| Any `InspectionRequired` | Load and authenticate exact record. | Inspect only. | Success advances. absence proposes same attempt at epoch + 1. | In progress/unavailable remains inspection-only. |
| Exact absence | CAS retry claim with bound absence evidence. | Direct winner executes at next epoch. others inspect. | Normal step success. | Ambiguous/replay inspects. |
| Any definite failure | None. | None. | None. | NNC6.5 alone compensates. |

Every effect follows a durable dispatch-claim CAS. Every success or definite
failure receives an exact successor CAS. An ambiguous successor CAS performs
one exact read before any later decision.

## Source-Derived Caller Census

| Surface | Current authority | Required replacement | Deletion gate |
| --- | --- | --- | --- |
| Native sandbox | Services calls `SandboxBackend::start` before stable logical resource identity and durable intent. | Compute composes, submits, dispatches, then services projects observed resource state. | Remove effectful standalone sandbox creation methods. |
| Native service | ServiceManager activation claim and direct sandbox start. | Compute drives the exact service incarnation. services supplies immutable source and projection. | Remove activation claim, in-progress set, start methods, and direct provider call. |
| Convex async service lookup | Effectful runtime registry refreshes/starts service. | Server calls a compute-owned provision capability, then reads the binding projection. | Remove async effect methods from `RuntimeServiceRegistry`. |
| Convex sync and invocation snapshots | Read-only resolution. | Retain unchanged as read-only. | Static negative proof forbids store/provider calls. |
| Container | Coarse start plans, attaches, publishes, activates, and checks readiness. | Exact phase adapters over durable manifest and existing lower effects. | Remove coarse `SandboxBackend::start`, `start_sync`, and `finish_start` authority. |
| Krun | Same coarse lifecycle. production identity is provider-generated. | Exact caller-selected preparation plus phase adapters. | Remove coarse start authority and provider-generated provision identity. |
| Local Compose | `start_service_launch` calls a sandbox backend directly. | Open canonical Engine store/coordinator and submit the exact source snapshot. | Delete `start_service_launch` and provisioning backend field. |
| Forwarded Compose parent | Coarse forwarded backend and a second random publication attempt. | Compute command plus a canonical-fence-bound provider journal. | Remove forwarded `SandboxBackend::start` authority and random attempt creation. |
| Machine API | Image/build start routes carry open `SandboxSpec`. | Strict phase-command request/response and inspect protocol. | Delete both coarse request types, client methods, and routes. |
| Guest node | `start` combines prepare, activate, status, and publication and re-admits generation zero. | Narrow phase sinks consuming the parent command. no saga store/coordinator. | Delete coarse guest start and local admission. |
| Systemd | Raceable inspect-then-start with replace mode. | Exact fail-if-present claim plus inspect/adopt. | Remove replacement as provision behavior. |
| DirectProcess | Replay creates another process and overwrites state. | Exact in-memory claim, replay adoption, and stale rejection. | Remove unfenced provision start behavior. |
| Hidden node executor | CLI flags create desire/admission and invoke node directly. | Submit through Engine/compute or delete the command. | No direct product coordinator construction remains. |
| Cloud Functions | Immutable invocation service snapshots. | Retain snapshot-only behavior. | Static proof forbids activation/store/provider calls. |

## Required Legacy Deletions

The final NNCV033 deletion census must find no production authority for:

- `SandboxBackend::start`.
- Container and Krun coarse start composition.
- `start_service_launch`.
- direct ServiceManager provider starts.
- ServiceManager activation claims and `activations_in_progress`.
- effectful `RuntimeServiceRegistry` methods.
- coarse Machine API image/build start routes and request types.
- forwarded-machine random parallel attempt identity.
- guest `MachineApiNodeWorkloadFacade::start`.
- guest saga-store or saga-coordinator construction.
- CLI-local desired-state or saga-store authority.
- the hidden node-executor provision bypass.
- first-available provider selection.
- provider handles in portable saga values.
- a general `NetworkProvider` or coarse lifecycle trait. or
- an effect interface or forbidden dependency in `nimbus-network`.

The final census explicitly retains:

- Convex synchronous service resolution.
- Convex and Cloud Functions invocation snapshots.
- DirectProcess and Systemd start effects only as exact guest provider sinks.
- service definitions, logical names, sessions, and read-only binding
  projection. and
- owner-local sandbox, forwarding, ingress, network-manager, and provider
  journals.

## Behavioral And Failure Proof Matrix

| Proof | Required observation |
| --- | --- |
| Unconfirmed candidate | Cannot form a provider command. |
| Direct CAS winner | Executes the exact attempt once. |
| Confirmed replay | Inspects without execute. |
| Ambiguous CAS then exact read | Inspects without execute. |
| Unresolved ambiguity | Emits no command. |
| Crossed command fence | Rejects before provider call. |
| Crossed result fence | Rejects before successor CAS. |
| Exact absence | Authorizes same attempt at epoch + 1. |
| In progress | Never retries or executes. |
| Retry without absence | Rejects before CAS/effect. |
| Concurrent dispatchers | Produce one owner-local external effect. |
| Stale provider epoch | Rejects before provider mutation. |
| Crash after claim CAS, before provider | Fresh process inspects exact attempt. |
| Crash after owner claim, before effect | Fresh process inspects/adopts exact owner state. |
| Crash after effect, before result CAS | Fresh process inspects and advances once. |
| Crash after result CAS | Fresh process resumes the next phase. |
| Definite failure at each step | Retains exact completed phase and emits no later command. |
| Prepare/attach/activate | Cannot publish or become host-routable. |
| Publish | Requires exact workload-readiness evidence. |
| Ambiguous publication | Inspects before retry. |
| Publication replay | Is idempotent under one owner-local journal. |
| Current source mismatch | Rejects before attempt CAS. |
| Provider-report mismatch | Rejects before effect. |
| Fresh Engine process | Reopens exact durable attempt without handed-over state. |
| Portable saga | Contains no provider handle, IP identity, socket, or assigned port identity. |
| Cancellation after submission | Cancels waiter only. durable desire remains. |
| Non-observed run | Performs zero execution observation, ingress observation, or services projection. |
| Observed withheld run | Reads the exact execution provider once, reads no ingress provider, and projects no endpoint. |
| Observed published run | Reads the exact execution and ingress providers, validates complete listener/lease membership, and projects the actual nonzero bound endpoint rather than desired port `0`. |
| Missing live ingress owner after restart | Reports pending without restart, repair, rebind, provider dispatch, or services mutation. |
| Crossed projection evidence | Wrong tenant, source, execution ID, backend, generation, resource version, plan, listener, lease, lifetime, protocol, target, or duplicate member rejects before services mutation. |
| Projection response loss and replay | Repeats only effect-free reads and one idempotent services projection. |
| Projection portability | `SandboxHandle`, `SandboxInspection`, `PortLeaseBinding`, and provider handles remain ephemeral and absent from `nimbus-workloads` and the saga record. |
| Foreground Compose ownership | After `compose up` reports ready, the actual endpoint remains connectable while the command is live; a second owner is rejected; cancellation withdraws the process-bound listener before the command returns; reopening uses the same Engine saga and does not create a second effect. |
| WSL2 for host-managed requirement | Fails before Machine API/provider effect. |
| Cloud Functions | Snapshot-only with zero activation/store/provider calls. |

Every provider-bearing happy path also proves the full observer order:

```text
admit
-> compile
-> persist intent
-> reserve
-> prepare
-> attach
-> activation prerequisites ready
-> activate
-> workload ready
-> publish
-> observe
```

There is one exact durable CAS after each effect. No routability appears before
`publish`.

## NNCV033 Contract

The concept-owned contract is
`scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh`.
It must pass these 40 direct checks:

1. required inputs.
2. routing, proof, and completion baseline.
3. NNC6.3b completion checkpoint pin.
4. closed confirmed-command vocabulary.
5. private confirmed-command construction.
6. complete confirmed-record identity.
7. revision and transition fence.
8. generation and digest fence.
9. closed network/execution provider target and subject fence.
10. domain-separated command ID.
11. dispatch epoch and authorization.
12. effect-result command correlation.
13. closed inspection-result vocabulary.
14. complete absence-evidence fence.
15. portable disposition retry state.
16. explicit disposition transition graph.
17. same-attempt monotonic retry.
18. exhaustive command/result reducer.
19. CAS confirmation provenance.
20. direct-winner-only execute.
21. replay-and-ambiguity inspect-only.
22. bounded ambiguous-store read.
23. current source before dispatch.
24. current provider report before dispatch.
25. exact provider-registry routing.
26. small real capability seams.
27. provider-local attempt idempotency.
28. one store and one coordinator.
29. required managed-compute dispatch composition.
30. reserve command mapping.
31. prepare command mapping.
32. attach command mapping.
33. activation-prerequisite command mapping.
34. activate command mapping.
35. workload-readiness command mapping.
36. publish, observe, and withheld mapping.
37. definite-failure and ambiguity behavior.
38. crash, concurrency, and fresh-process proof.
39. positive and read-only caller census. and
40. legacy deletion, path, dependency, and effect contract.

Green direct output is exactly:

```text
NNC6.4 provider dispatch contract: 40 checks passed
```

The self-test applies 48 non-no-op mutations:

1. missing command vocabulary.
2. extra command mode.
3. forgeable command constructor.
4. missing confirmed transition ID.
5. missing confirmed revision.
6. missing attempt ID.
7. missing generation.
8. missing desired digest.
9. missing provider-target ID.
10. missing provider-target source digest.
11. missing subject fence.
12. missing command-ID domain.
13. missing dispatch epoch.
14. result loses command fence.
15. unknown inspection result.
16. missing inspection absence.
17. missing inspection in-progress.
18. retry changes attempt ID.
19. retry reuses dispatch epoch.
20. retry accepts crossed absence revision evidence.
21. fixed revision-offset retry.
22. ambiguous CAS executes.
23. unchanged CAS executes.
24. execute before attempt CAS.
25. source mismatch effects.
26. provider-report mismatch effects.
27. missing reserve command.
28. missing prepare command.
29. missing attach command.
30. missing prerequisite inspection.
31. missing activate command.
32. missing readiness inspection.
33. publish before ready.
34. missing publication observation.
35. definite failure emits later command.
36. ambiguity retries without inspection.
37. in-progress retries.
38. concurrent provider duplicate.
39. missing effect crash cut.
40. fresh-process snapshot handoff.
41. duplicate coordinator.
42. duplicate store.
43. god provider trait.
44. network effect interface.
45. portable provider handle.
46. old provision authority remains.
47. caller-family bypass. and
48. Cloud Functions effect.

Green self-test output is exactly:

```text
NNC6.4 provider dispatch contract self-test: 48 passed, 0 failed
```

NNCV032 remains the historical NNC6.3b proof. Its no-provider and no-caller
checks must be pinned to
`c42c61fb2d97d037069f3b27b9055d6e58f11d1d`, not weakened against current
source. The aggregate target becomes 34/34 live checks and 325/325 retained
plus NNCV033 mutations.

The frozen current-state run passes 4 of 40 direct groups and fails the exact
36 unimplemented NNC6.4 groups. Required inputs, routing/proof ownership, the
NNC6.3b completion pin, and the one-store/one-coordinator census pass. Every
product command, provider, caller, behavior, and deletion group remains red.
The 48-mutation self-test passes with no no-op mutation.

## Frozen Path Allowlist

Protocol and coordinator paths:

- `crates/nimbus-workloads/src/saga/provision.rs` and concept-owned dispatch,
  inspection, validation, and test children.
- `crates/nimbus-workloads/src/saga/state.rs` and its concept-owned provision
  validation child.
- `crates/nimbus-workloads/src/saga/tests.rs` only to move the intact provision
  group before new tests.
- `crates/nimbus-workloads/src/lib.rs`.
- `crates/nimbus-compute/src/workload_saga.rs`.
- `crates/nimbus-compute/src/workload_saga/provision_decision.rs`.
- new compute provision-command, dispatch, registry, adapter, and concept-owned
  test modules.
- `crates/nimbus-compute/src/state.rs`, configuration, native service/sandbox
  facades, and source-composition modules.
- compute and workloads manifests only if a source-derived dependency is
  required. and
- server workload-saga codec, schema, composition, ambiguity, contention,
  durability, and subprocess proof paths.

Service, server, sandbox, and provider paths:

- service catalog snapshots, manager definition/projection paths, registry
  split, and focused tests.
- server state/composition, Convex host-call adapter, read-only negative tests,
  ingress adapter, router/native caller tests, and Cloud Functions negative
  tests.
- sandbox backend trait, Container and Krun composition roots, and shared OCI
  attachment/publication state machine.
- sandbox manifests, journals, and focused unit, adapter, concurrency, and
  crash tests.
- sandbox public exports and manifest only for the earned phase seam.
- node coordinator/reconciler, HostLifecycle command fence, DirectProcess,
  Systemd, and focused tests.
- machine strict wire protocol/provider-mode vocabulary and tests.
- CLI Compose, Engine composition, forwarded-machine journal/client/backend,
  Machine API route/facade, hidden node executor, and focused tests.
- `packages/nimbus` canonical SDK source/self-tests, generated distribution,
  embedded `nimbus-assets` package artifacts, `nimbus-system` route inventory,
  and the public resource/API docs plus source map only to remove the stale
  service-restart surface after NNC6.4 deletes its server route. NNC6.4a owns
  reintroducing explicit restart through the fenced saga; NNC6.4 must not leave
  a callable client method or observed route for an endpoint that is absent.

Control paths:

- this proof.
- the canonical plan and routing index.
- NNCV033 contract and self-test.
- the NNCV032 historical completion pin.
- the aggregate verifier. and
- source-derived census JSON only if the existing census schema is extended
  rather than duplicated.

No other product or control path is authorized without first recording the
source-derived need and ownership disposition in this proof and the Recovery
Header.

## Modularity Disposition

The final product-source census covers `299` changed Rust files. `27` meet a
modularity threshold. The prior complexity pockets converged as follows:
shared OCI attachment lifecycle is `1,522` lines, the Container runtime root is
`1,577`, the Krun VM root is `912`, service activation/effect ownership left
`ServiceManager`, the guest Machine API consumes exact phase commands, coarse
Machine routes and payloads are deleted, and node activation no longer uses an
inspect-then-start or Systemd replacement path.

Three files exceed 2,000 lines. They retain strong ownership exceptions rather
than receiving a review-driven mechanical split:

| Path | Lines | Strong ownership exception |
| --- | ---: | --- |
| `crates/nimbus-network/src/port_lease/lifetime.rs` | 2,738 | One durable lease-lifetime state machine owns lock acquisition, exact recovery authentication, claim/activate/settle transitions, and batched atomic lifetime advancement. Complete-plan batching and tests already live in concept-owned children. Splitting a transition half from its lock and fencing invariants would create a less local authority seam. |
| `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs` | 2,086 | One process-handoff and runner-ownership state machine owns prepared-manifest authentication, exact execute/inspection locking, durable handoff decisions, launch-result convergence, and bounded cleanup. Identity, recovery storage, probes, and tests already live in children. A further NNC6.4 split would separate adjacent crash states without introducing an independent capability. |
| `crates/nimbus-sandbox/src/backends/oci/port_lease.rs` | 2,044 | One OCI adapter translates the transport-free lease authority into authenticated reserve, claim, activate, fail, inspect, rebind, withdraw, and release transitions for scalar and complete-plan batches. The repeated entrypoints preserve one adapter authority and one error mapping; extracting one transition family during atomic caller cutover would risk duplicated lifecycle policy. |

The remaining 1,500–1,999-line files have these explicit dispositions:

| Path | Lines | Ownership disposition |
| --- | ---: | --- |
| `crates/nimbus-sandbox/src/backends/container/runtime/launch_cleanup.rs` | 1,993 | One launch-compensation and cleanup-convergence state machine; effect ordering and crash-cut invariants remain colocated. |
| `crates/nimbus-network/src/port_lease.rs` | 1,979 | Validated public lease vocabulary and authority entrypoints; durable lifetime and complete-plan behavior already live in concept children. |
| `crates/nimbus-sandbox/src/backends/oci/egress/tests.rs` | 1,951 | One OCI egress readiness/failure matrix over shared fixtures; test-only. |
| `crates/nimbus-workloads/src/saga/tests.rs` | 1,931 | Shared provision/teardown/successor/wire fixture matrix. Provision-state cases moved intact to `tests/provision_state.rs`; no new NNC6.4 state-machine cases remain inline. |
| `crates/nimbus-server/src/tests.rs` | 1,898 | Server integration test composition and shared fixtures; concept suites live in child modules and this file adds no product authority. |
| `crates/nimbus-compute/src/workload_network_plan/tests.rs` | 1,791 | One compiler-to-portable-plan behavioral matrix over a shared source fixture; test-only. |
| `crates/nimbus-sandbox/src/backends/container/runtime/planning.rs` | 1,723 | Pure Container launch/attachment planning decisions and validation; provider effects remain outside this owner. |
| `crates/nimbus-server/src/tests/service_manager.rs` | 1,714 | Shared service/sandbox route fixture and backend witness for concept-owned child suites; test-only. |
| `crates/nimbus-sandbox/src/backends/oci/egress.rs` | 1,703 | OCI egress lifecycle owner; readiness and tests are already concept children and policy remains outside the adapter. |
| `crates/nimbus-sandbox/src/backends/krun/vm/tests/launch_compensation.rs` | 1,694 | One Krun launch-compensation crash/fencing matrix; test-only. |
| `crates/nimbus-node/src/reconciler.rs` | 1,655 | One host-local reconciliation state machine. Exact DirectProcess/Systemd effects remain behind narrow sinks; no provision coordinator remains here. |
| `crates/nimbus-cli/src/machine/backend/provision/tests.rs` | 1,635 | One forwarded-machine provision, retirement, replay, and identity-fencing matrix over shared fixtures; test-only. |
| `crates/nimbus-workloads/src/network_plan.rs` | 1,608 | Transport-free validated network-plan vocabulary and canonical digest rules; no I/O or provider effects. |
| `crates/nimbus-sandbox/src/backends/krun/vm/tests.rs` | 1,596 | Shared Krun fixture composition with concept-owned test children; test-only. |
| `crates/nimbus-sandbox/src/backends/oci/port_lifecycle.rs` | 1,589 | One OCI port transition state machine; machine-specific behavior already lives in its child. |
| `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs` | 1,576 | One external-publication journal and exact command/authority authentication state machine; machine transport and provider selection remain outside. |
| `crates/nimbus-sandbox/src/backends/container/runtime.rs` | 1,577 | Thin Container composition root relative to its provider scope. Provision, cleanup, planning, runner, publication, and tests live in concept-owned children. |
| `crates/nimbus-server/src/listener_lease.rs` | 1,556 | One server-owned listener lease/rebind/adoption state machine; complete-plan member operations preserve sibling authority atomically. |
| `crates/nimbus-workloads/src/store/tests.rs` | 1,523 | One store-port conformance and paging/recovery matrix shared across implementations; test-only. |
| `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs` | 1,522 | One attachment lifecycle owner with authority, readiness, reconciliation, release, and tests in concept children. Publication stays outside. |
| `crates/nimbus-cli/src/machine/backend/provision.rs` | 1,575 | One forwarded-machine provider adapter and parent-owned journal/fencing state machine; guest transport and compute coordination remain outside. |
| `crates/nimbus-cli/src/network_composition.rs` | 1,524 | CLI composition root for assembling existing network capabilities; it owns no lease, provider effect, or saga state. |
| `crates/nimbus-workloads/src/saga/state.rs` | 1,514 | Portable saga state vocabulary and validation root; provision-specific validation lives in its concept child. |
| `crates/nimbus-workloads/src/saga.rs` | 1,507 | Portable saga coordination vocabulary and transition graph; provision dispatch/state are in concept children and provider effects remain absent. |

Two handwritten verifier files also meet a threshold:

| Path | Lines | Ownership disposition |
| --- | ---: | --- |
| `scripts/verify-nimbus-network-control-plane.sh` | 2,072 | Strong exception. This file is the single 34-condition aggregate orchestration root. Concept contracts and mutation suites remain in owned child scripts. A split at this gate would duplicate summary arithmetic or command routing without creating a domain seam. |
| `scripts/verify-nimbus-network-source-contract.mjs` | 1,958 | Explicit source-scanner owner. It contains the shared Rust source model and the closed source-boundary checks that consume it. The aggregate and mutation harness remain outside this file. |

Before adding NNC6.4 cases, move the intact portable provision-state test
group to a concept-owned child. Split the shared OCI attachment/publication
logic by state-machine ownership. Keep Container and Krun composition roots
thin. new phase behavior belongs in concept-owned children. Split services
activation effects away completely rather than renaming a mixed manager.

Do not split files mechanically. Every retained file from 1,500 through 1,999
lines needs an explicit ownership justification here. Every file at or above
2,000 lines must be decomposed or receive a strong ownership exception before
candidate freeze.

## Acceptance Criteria

| ID | Criterion |
| --- | --- |
| E1 | Portable state has a strict dispatch epoch, authorization, an exact closed network/execution provider target, and complete absence evidence without provider handles. |
| E2 | The stable attempt ID remains identical across an absence-authorized retry. the dispatch epoch increases by exactly one. |
| E3 | The ephemeral command binds complete record, revision, transition, generation, digest, provider target, subject, prerequisite, epoch, and mode identity. Network targets bind role, ID, and admitted network-source digest. execution targets bind a neutral execution ID and workload-source digest. |
| E4 | Only the coordinator can construct a confirmed command. |
| E5 | Direct CAS provenance is distinct from replay, ambiguity confirmation, conflict, and unresolved ambiguity. |
| E6 | Only the direct winner of an exact dispatch-claim CAS can execute. Replay and ambiguity confirmation inspect only. |
| E7 | An unresolved store ambiguity emits no command. every ambiguous successor CAS performs one bounded exact read. |
| E8 | Exact inspection success/failure/absence/in-progress/unavailable outcomes are closed and strictly correlated to the command and provider-target fence. |
| E9 | Only exact absence authorizes one same-attempt next-epoch retry. In-progress, unavailable, replay, and ambiguity do not. |
| E10 | The reducer and state validator accept every legal repeated inspection/retry history and reject skipped/reused/crossed epochs. |
| E11 | Current source generation/resource version/content and current provider-report digest are checked before attempt CAS/effect. |
| E12 | The dispatcher selects the exact admitted network or execution provider target and has no first-available or safe-alternative fallback. Resource-free network steps fabricate no target or command. |
| E13 | Real Container and Krun adapters substitute for narrow prepare, attach, activate, and readiness capabilities. |
| E14 | Real server-ingress and forwarded-machine adapters substitute for the publication capability. |
| E15 | Provider-local journals/manifests/units fence attempt plus epoch, adopt exact replay, reject stale/crossed claims before mutation, and produce one effect under concurrency. |
| E16 | Reserve, prepare, attach, activation-prerequisite, activate, and readiness steps cannot make an endpoint host-routable. |
| E17 | Only owner-local publish after exact workload-readiness evidence may activate ingress. ambiguous publication inspects before retry. |
| E18 | The observer proves admit→compile→persist→reserve→prepare→attach→activation-ready→activate→workload-ready→publish→observe with an exact durable CAS after every effect. |
| E19 | Every definite failure retains the exact completed phase, records exact evidence where durable ownership exists, issues no later command, and leaves compensation solely to NNC6.5. |
| E20 | Crash cuts after claim CAS, owner claim, effect, and result CAS recover through a genuinely fresh Engine process without handed-over state. |
| E21 | Concurrent coordinators and concurrent provider calls produce one external effect and one monotonic successor. |
| E22 | Native sandbox, native service, Convex async activation, local Compose, forwarded Compose, and Machine API parent callers all enter the compute saga. The hidden node executor is absent; the guest remains only an exact fenced phase sink. |
| E23 | Compose uses the canonical Engine-backed saga store and a complete source-owned snapshot. it creates no CLI journal or invented generation. |
| E24 | Machine API uses a strict exact phase-command protocol. coarse image/build start requests/routes/clients are deleted without compatibility aliases. |
| E25 | The forwarded parent journal binds canonical command identity and machine-provider generation. WSL2 host-managed gaps fail before effects. |
| E26 | The guest has no saga store/coordinator or local re-admission. it remains a fenced provider adapter. |
| E27 | DirectProcess and Systemd are exact activation sinks. Systemd uses fail-if-present claim/adoption rather than replacement. |
| E28 | Convex sync lookup and all invocation snapshots remain read-only. Cloud Functions HTTP/callable/trigger have zero activation/store/provider calls. |
| E29 | Services retain source, naming, session, and projection ownership but no activation claim, direct provider start, or effectful runtime-registry method. |
| E30 | `SandboxBackend::start`, `start_service_launch`, coarse Machine API/guest starts, every direct caller bypass, and every other legacy provision authority are absent. |
| E31 | The portable saga contains no provider handle, socket, namespace, IP identity, assigned-port identity, or second store/coordinator. |
| E32 | `nimbus-network` retains only the `nimbus-core` workspace edge and gains no effect interface. |
| E33 | NNCV033 passes 40/40 direct checks and 50/50 mutations. the aggregate passes 34/34 and 327/327. |
| E34 | Full affected behavioral suites, strict Clippy, warning-denied rustdoc, format/diff, dependency/effect scans, docs 108, and site 17/17 pass. |
| E35 | One complete GPT-5.6 Sol/xhigh/fast structured review runs only after E1-E34 are green and the item is candidate-frozen. every finding has an evidence-backed disposition. |

## Verification Commands

Expected-red preparation:

```bash
bash -n scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh
bash -n scripts/nimbus-network-control-plane/workload-provision-dispatch-self-test.sh
shellcheck scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh scripts/nimbus-network-control-plane/workload-provision-dispatch-self-test.sh
bash scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
```

During implementation, use focused fail-before tests and affected-crate suites.
At candidate freeze, record exact commands, test counts, skips, timeouts,
environment capabilities, static counts, and artifact identities here before
the one structured review.

## Preparation Verification Evidence

The product-free checkpoint has this exact evidence:

| Gate | Result |
| --- | --- |
| NNCV033 direct | Expected red: `4/40` groups pass and the exact 36 unimplemented NNC6.4 groups fail. Exit status is `1`. |
| NNCV033 mutation suite | `48/48` pass with no no-op mutation. |
| Pinned NNCV032 direct and mutation suites | `32/32` and `36/36` pass from NNC6.3b completion commit `c42c61fb2`. |
| Live aggregate | Expected red: `33/34` pass and only NNCV033 fails. |
| Aggregate self-test | `325/325` pass. The previously affected NNCV020 `missing-pre-crash-witness` case passes exclusively. |
| Script checks | Bash syntax passes for the five affected scripts. Scoped ShellCheck passes for NNCV032, NNCV033, and the aggregate. |
| Documentation checks | Proof technical-writing lint exits `0` with advisory warnings only. Docs pass `108` pages. Site verification passes `17/17`. |
| Repository hygiene | `git diff --check` passes. No product source is changed. |

The aggregate harness keeps at most two independent full-repository verifier
scans active. This limit follows measured behavior, not a weakened assertion:

1. A serial `timeout 3600` self-test reached NNCV026 and then timed out with
   exit `124`. It was not recorded as green.
2. A four-lane trial exposed scanner starvation in NNCV006 while NNCV020 ran
   the `missing-pre-crash-witness` mutation. That trial ended at `32` passed
   and `2` failed and was rejected.
3. The final two-lane `timeout 5400` run completed at `325/325`. It executed
   the same aggregate child commands and exclusive failure assertions. Only
   independent read-only scans overlapped.

No verifier process remained after the final run. No structured review ran.
The item-level review remains prohibited until E1-E34 are green.

## Portable Protocol Checkpoint Evidence

E1-E10 are green at dirty checkpoint HEAD
`e281a948a739a161ebaf63058e00eb654e0f0d29`. The checkpoint is intentionally
uncommitted because NNC6.4 remains one atomic caller replacement and deletion
item.

| Gate | Result |
| --- | --- |
| Portable dispatch wire and target behavior | `8/8` pass. Strict decimal epochs, unknown inspection variants/fields, crossed providers, crossed revisions, and resource-free targeting are covered. |
| Portable provision-state behavior | `13/13` pass. Exact absence, stable attempt, repeated retries through epoch 8, and reused/skipped/crossed authorization are covered. |
| Compute confirmation and command behavior | `19/19` pass. Direct/replay/ambiguity/conflict provenance, one-read ambiguity, durable recovery load, exact command correlation, and absence-only retry are covered. |
| Full affected behavior | `nimbus-workloads` passes `136/136`; `nimbus-compute` passes `166`, with one declared child-process-only ignore. |
| NNCV033 | The strengthened mutation suite passes `48/48`. The direct contract is expected red at `23/40`; the exact 17 remaining groups are E11+ dispatcher, provider, caller, crash, census, and deletion obligations. |
| Affected quality | Affected all-target check and strict Clippy pass. Format, diff, and both NNCV033 Bash syntax checks pass. Proof lint exits `0` with advisory warnings only. Docs pass `108`; site verification passes `17/17`. Third-party Brotli warnings remain dependency-owned and do not fail strict workspace lint. |
| Manual convergence audit | Two P1 findings were accepted and fixed: nested absence revision authorization is strict on deserialize, and fresh recovery loads exact durable store truth. Two directly related P2 cleanups were also accepted: conflict/unresolved outcomes expose no candidate as confirmed truth, and NNCV033 now proves the corrected seams. |
| Review cadence | No structured review ran. E35 remains prohibited until E1-E34 are green and the complete item is candidate-frozen. |

## Dispatcher Checkpoint Evidence

E11-E12 are green in the same intentionally uncommitted NNC6.4 item tree.
They extend the E1-E10 protocol without adding provider effects to
`nimbus-network` or granting a second coordinator.

| Gate | Result |
| --- | --- |
| Exact dispatcher behavior | `18/18` pass. Source and provider-report drift reject before claim CAS/effect; exact network and execution targets dispatch without fallback; all eight step mappings, resource-free behavior, effect separation, registry uniqueness, and fresh-store inspection recovery are covered. |
| Authenticated command behavior | `19/19` pass. The private command constructor carries the exact confirmed executable and compiled network plan, and only the direct claim-CAS winner receives execute mode. |
| Full affected behavior | `nimbus-workloads` passes `136/136`; `nimbus-compute` passes `184`, with one declared child-process-only ignore. |
| NNCV033 | The strengthened mutation suite passes `48/48`. The direct contract is expected red at `33/40`; the exact seven remaining groups cover real provider substitutions and idempotency, managed composition, failure/crash/process proofs, caller census, and legacy deletion. |
| Affected quality | Affected all-target check and strict Clippy pass. Format and both NNCV033 Bash syntax checks pass. Third-party Brotli warnings remain dependency-owned and do not fail strict target lint. |
| Review cadence | No structured review ran. E13-E34 implementation and convergence remain before the single E35 item review. |

## Sole Full Review Disposition Ledger

The sole full Sol/xhigh/fast item review produced `41` findings. Source and
plan verification classifies `33` as accepted NNC6.4 corrections, `4` as
false-premise rejections, and `4` as real teardown risks owned atomically by
NNC6.5. Deferred rows are not treated as resolved; NNC6.5 must consume them.

| # | Priority | Disposition | Evidence and correction boundary |
| ---: | --- | --- | --- |
| 1 | P1 | `accepted; correction active` | Forwarded retirement dropped exact parent publication cleanup while retaining the caller. Restore exact guest-absence authentication and parent batch settlement without adding a second teardown coordinator. |
| 2 | P1 | `accepted; correction active` | Guest readiness reused activation mapping and treated `Running` as ready. Only `Ready` succeeds readiness; earlier phases remain in progress. |
| 3 | P1 | `accepted; correction active` | Guest inspection lost live node-lifecycle evidence. Combine plan-only sandbox state with read-only exact lifecycle observation; never restart. |
| 4 | P1 | `accepted; correction active` | Fresh forwarded adapters could confirm durable `Active` publication without reclaiming non-cloneable live lifetimes. Success must retain the exact recovered batch. |
| 5 | P2 | `deferred; NNC6.5` | Definite guest publication failure retains fenced authority by E19. NNC6.5 owns compensation; adapter-local release would duplicate compensation authority. |
| 6 | P2 | `accepted; correction active` | `ObservePublication` cached `Absent` as terminal. Process-bound publish and observe absence must remain live-reconcilable. |
| 7 | P2 | `rejected` | STOP already inherits `service_execution_blockers`; the cited lifecycle-unavailable state is reported unsupported. Add only a pinning assertion if useful. |
| 8 | P1 | `accepted; correction active` | Forwarded start/dev composition still failed closed despite an exact machine-owned prepared source. Add the missing prepared forwarded profile and consume it only after Engine construction. |
| 9 | P1 | `deferred; NNC6.5` | Published ingress will require exact Delete when NNC6.5 adds the real deletion capability and cutover. Requiring it now would advertise an unimplemented effect. |
| 10 | P2 | `accepted; correction active` | Stop response validation lost tenant and complete binding-set authentication. Reject missing, extra, duplicate, reordered, crossed, or foreign receipts before parent cleanup. |
| 11 | P2 | `accepted; correction active` | Confirmed publication members were only self-consistent, not tied one-to-one to the canonical command plan. Authenticate complete membership, tenant, plan, generation, listener, lease, and binding. |
| 12 | P1 | `accepted; correction active` | Machine wire validation did not correlate executable bytes with admitted source evidence. Use one portable canonical executable/source authentication seam. |
| 13 | P2 | `accepted; correction active` | Execute mode accepted revisions later than the unique claim revision. Execute requires equality; later confirmed revisions are inspection-only. |
| 14 | P2 | `rejected` | `EndpointProtocol` has TCP/HTTP/HTTPS only; UDP is rejected by the compiler. All admitted application protocols intentionally materialize a TCP lease. |
| 15 | P2 | `accepted; corrected, focused green` | First definite failure returned a provider code while replay returned the normalized code. Both paths now normalize identically; focused replay passes `2/2`. |
| 16 | P3 | `accepted; correction active` | One Tokio yield did not prove the second waiter joined. Replace it with a bounded semantic wait-boundary signal. |
| 17 | P3 | `accepted; correction active` | Real Container/Krun adapter tests only proved construction. Invoke each registered capability and assert exact outcomes and crossed-backend rejection. |
| 18 | P1 | `rejected` | The real zbus adapter maps systemd `NoSuchUnit` to inactive/dead status; broad `NotFound` also represents provider/interface failure and must remain fail closed. |
| 19 | P1 | `accepted; correction active` | Direct-process exact inspection compared an unfenced lifecycle projection with the retained activation fence. Ignore only the unavailable fence while retaining all lifecycle identity checks. |
| 20 | P1 | `accepted; correction active` | Scalar and batch rebind could overwrite a crossed retained bind claim. Require exact claim and effect scope before lifetime mutation. |
| 21 | P1 | `accepted; correction active` | Container activation inspection treated every present runtime state as success. Only running succeeds; non-running states classify explicitly. |
| 22 | P2 | `accepted; correction active` | Machine-ingress absence checked only reserved/no-binding. Authenticate the exact request/plan/claim and prove every effect-bearing field empty. |
| 23 | P1 | `accepted; correction active` | Krun PEP attach dropped compiler plan membership through legacy `FreshLaunch`. Use exact `FreshPlannedLaunch` authority. |
| 24 | P2 | `rejected` | The cited claim-only manifest is `Reserved`, whose cleanup already uses the persisted provision-plan attachment ID. `attachment_recovery` handles only post-config `Adopting`. |
| 25 | P2 | `accepted; correction active` | Krun attach rejected dead planned PEP ownership before rebind recovery. Authenticate owner death and recover the exact planned lease before fresh-only checks. |
| 26 | P2 | `accepted; correction active` | Krun reservation inspection trusted manifest flags without expected plan or live allocator/lease evidence. Authenticate the entire durable authority set. |
| 27 | P2 | `accepted; correction active` | OCI attachment state retained a sandbox-derived plan fallback. Require the compiler-issued durable plan and fail before mutation when absent. |
| 28 | P2 | `accepted; correction active` | Build provenance hashed a mutable context before later COPY reads. Build and hash from one private snapshot so recorded digest and copied bytes cannot diverge. |
| 29 | P2 | `accepted; correction active` | Krun activation inspection treated every present runtime as success. Share the explicit running/non-running classifier. |
| 30 | P1 | `accepted; correction active` | Server ingress reported source absence before consulting an exact live listener batch. A live exact effect cannot reconcile to absence. |
| 31 | P1 | `deferred; NNC6.5` | Per-workload ingress withdrawal/release is real missing teardown work. NNC6.5 must cancel/join workers, settle leases, and observe exact absence. |
| 32 | P1 | `accepted; correction active` | Ingress spawned unbounded untracked native connection workers. Add bounded tracked workers, deadlines, cancellation, and join-before-settlement. |
| 33 | P2 | `accepted; correction active` | Live ingress observation omitted exact execution identity. Retain and authenticate the execution subject in the key, batch, and query. |
| 34 | P2 | `accepted; correction active` | Fixed composition checked provider presence but not sovereignty satisfaction. Use canonical registry satisfaction before managed composition. |
| 35 | P1 | `accepted; corrected, focused green` | Retry and absence construction accepted `DispatchPending`. Both now require durable `InspectionRequired`; focused history/rejection passes `3/3`. |
| 36 | P1 | `deferred; NNC6.5` | Definition deletion can race an in-flight provision result. NNC6.5 must cancel/join the saga, drain late results, re-retire, then remove desired source. |
| 37 | P2 | `accepted; correction active` | Unfenced service observation could establish the first provider identity. Delete it or make it update-only after exact first-write fencing. |
| 38 | P2 | `accepted; correction active` | Unfenced standalone observation duplicated first-write authority. Preserve the exact compute-facing source/execution-fenced path as sole first writer. |
| 39 | P2 | `accepted; correction active` | Cancellation during definition retirement stranded the mutation claim. Use cancellation-safe RAII ownership. |
| 40 | P2 | `accepted; correction active` | Session commit did not revalidate exact definition/sandbox source and ready observation under the final lock. Add exact commit gates for both branches. |
| 41 | P2 | `accepted; correction active` | Provider-local verifier tokens could be satisfied by server/shared fixtures with real providers absent. Require independent Container and Krun evidence plus independent mutations. |

No second broad review is authorized. After all accepted executable corrections
and affected proofs are green, one narrow correction review may inspect only
the accepted defect set.

## Post-Review Correction Checkpoint

GPT-5.6 Sol reviewed the correction once with xhigh and fast modes. The review
compared synthetic base
`4eec9cfbc8e452c7abad99107f37f73c56b4a300` with correction
`09f91a55263c566fa3ccc855941fdbd17c8c46a9`. It reported six findings at
confidence `0.96`. The integration owner accepted and corrected all six:

1. stopped Container and Krun runtimes now classify as ambiguous instead of
   authorizing an absence retry.
2. retired machine publication retries finish locally before guest contact.
3. an existing exact ingress listener preserves an ambiguous source result.
4. real Container and Krun substitution tests invoke all six registered
   provision capabilities.
5. the compiler and sandbox use the same source-owned `egress-pep` listener
   dependency.
6. NNCV033 proves the concrete provider-journal connector instead of accepting
   a decoy token.

The material corrections pass their focused proofs. The final affected suites
pass non-CLI `2,502/2,502` with `79` skips, CLI `936/936` with one skip,
`nimbus-system` `73/73`, and listener lease `18/18`. NNCV033 passes `40/40`
direct checks. Its mutation suite reports `50 passed, 0 failed`. The aggregate
passes `34/34` and its
bounded adversarial suite passes `327/327`. The plan authorizes no further
NNC6.4 review.

## Status Ledger

| Checkpoint | State | Evidence | Next exact action |
| --- | --- | --- | --- |
| Source audit | `done` | Three read-only lanes changed zero paths. Current/target graphs, caller census, protocol gap, provider-target correction, complexity pockets, and deletion gates are frozen above. | Preserve the frozen census while product implementation proceeds. |
| Expected-red contract | `done` | Commit `eb6adfc5516ae1f7661ff04009ca2bf48c893295` makes the source census and contract durable. NNCV033 passes `48/48` mutations. Current product passes `4/40` direct groups and fails the exact 36 NNC6.4 implementation groups. NNCV032 passes `32/32` and `36/36` from its pinned completion tree. The live aggregate is expected red at `33/34`; its bounded self-test passes `325/325`. Docs pass `108`; site passes `17/17`. | Preserve the exact expected-red behavior while implementation converges. |
| Portable protocol | `done` | E1-E10 pass at dirty checkpoint HEAD `e281a948a739a161ebaf63058e00eb654e0f0d29`: focused `8 + 13 + 19`; full workloads `136`; full compute `166` plus one declared ignore; affected check, strict Clippy, format, diff, Bash syntax, proof lint, docs `108`, site `17/17`, NNCV033 `48/48`, and direct expected-red `23/40`. The convergence audit's two P1 and two related P2 findings are fixed. | Preserve the portable protocol while dispatcher/provider composition lands. |
| Dispatcher and composition | `done` | E11-E13 and E15 are green: dispatcher `18/18`; IPAM `15/15`; orphan evidence `32/32` plus one child-only ignore; provider journal `7/7` plus one child-only ignore; compute provider adapter and real registry substitution `4/4 + 2/2`; full compute `184` plus one ignore; NNCV033 `48/48`; direct expected-red `35/40`. Exact compiler-selected attachment/listener/lease identities reach Container/Krun, and provider-local journaling proves replay, fencing, tamper rejection, and one effect under thread/process contention. | Preserve the exact provider seam while publication and caller cutover land. |
| Provider phase split | `done` | E13-E21 and D17 are green. Exact role-specific requirements, provider targets, private attach, provider-owned publication, artifact adoption, lifetime recovery, and complete request authentication are proven. Network passes `252` plus one child-only ignore; workloads `136`; sandbox passes `980/980` with `46` skips; Machine passes `32/32`; server composition/listener/protocol proofs pass `7 + 4 + 1`. Linux live-provider tests remain explicitly capability-gated. | Preserve the provider seam through review. |
| Caller cutover and deletions | `done` | E22-E30 are green. Native, server/Convex, node, sandbox, start/dev, Machine-forwarded, and Compose callers use the compute-owned provision facade and exact provider phases. The coarse host/sandbox starts, hidden node executor, mixed service activation, legacy restart route, and all classified provision bypasses are deleted. NNCV033 passes `40/40` direct checks and `48/48` mutations. | Preserve single coordinator/effect ownership through review. |
| Acceptance convergence | `done` | E1-E34 are green after correction. Affected non-CLI behavior passes `2,502/2,502` with `79` skips; CLI passes `936/936` with one skip; system passes `73/73`; listener proof passes `18/18`; live aggregate passes `34/34`; self-test passes `327/327`. All-target check, strict affected Clippy, warning-denied rustdoc, Rustfmt, Prettier, diff, Bash syntax, scoped ShellCheck, dependency/effect/static scans, SDK build/typecheck/test and `24`-route parity, package closure, docs `108`, site `17/17`, and the exact `299`-file/`27`-threshold Rust modularity census pass. | Record the final candidate and commit the complete item. |
| Item review | `done` | The sole full Sol/xhigh/fast review completed as eight internal passes with `41` findings and overall confidence `0.98`. The one permitted narrow correction review reported six accepted findings at confidence `0.96`; all are fixed and proven. No further review is authorized. | Preserve the accepted corrections and final green evidence. |
| Item commit | `ready` | E1-E35 and the post-review correction proofs are green. The final pre-ledger candidate has `326` staged paths, tree `c2bbd4ff3cbb9e5c49d59a1d737333cc3621fe9f`, and full patch SHA-256 `1e899c0a2b29f93552bf0402414cddd94f70792157dd9aa83a82484c53d71b66`. | Commit one complete NNC6.4 item. Do not push or open a PR. |

## Recovery Record

This proof, the plan Recovery Header, and the checkpoint ledger preserve the
source census and frozen contract at preparation commit
`eb6adfc5516ae1f7661ff04009ca2bf48c893295`. During implementation, append one
row per meaningful checkpoint. Record the exact HEAD, dirty paths, last green
command/count, current red condition, finding dispositions, blocker, and next
exact command.

No structured review is authorized until the entire item satisfies E1-E34.

Recovery checkpoint date: 2026-08-03.

HEAD is `e281a948a739a161ebaf63058e00eb654e0f0d29`. The dirty tree contains the
portable protocol, exact dispatcher, provider-local attempt journal, real
Container/Krun capability adapters, services-owned source authority, managed
composition, concept-owned tests, affected fixtures, NNCV033 changes, proof,
and owner plan. Exact compiler-selected attachment, listener, port-lease, plan,
tenant, and generation identities now reach both real sandbox providers. IPAM
passes `15/15`; orphan evidence passes `32/32` plus one child-only ignore;
provider journaling passes `7/7` plus one child-only ignore; compute adapter and
real registry substitutions pass `4/4 + 2/2`; format passes. Direct NNCV033 is
expected red at `35/40`; five E14+ groups remain.

Owner inspection proves the attach phase is genuinely private and cannot add
host publication by invoking Netavark a second time against the existing
container namespace. E14 therefore requires a server-owned transparent TCP
listener/proxy adapter for local Container/Krun ingress, while forwarded-machine
publication continues through its existing machine forwarding owner. The local
server adapter, exact Container/Krun phases, strict Machine/guest transport,
DirectProcess/Systemd sinks, and four-cut fresh-process proof are now present.
The owner reran the lost guest closeout and fixed two exact reds: planned
publication inspection now authenticates compiler-owned full-plan membership,
and crossed extended plan witnesses reject as `PlanMembershipConflict` before
lookup or mutation. Exact sandbox phases pass `11/11`; full network passes
`243` plus one child-only ignore; node passes `59/59`; strict Machine route,
guest validation, and capability advertisement pass `1 + 1 + 1`; the E20
parent passes `1/1`; NNCV033 passes `48/48` and is expected red at `38/40`.
The direct contract's remaining groups are positive/read-only caller proofs
and legacy deletion. The forwarded parent
adapter and its exact journal are now implemented and manually inspected. A
same-attempt old-epoch replay defect found during that inspection is corrected,
but the seven focused tests wait on the atomic Compose caller migration that
restores CLI compilation; E14 therefore remains open. A later manual source
audit reopened E13/E15/E16 for five behavioral gaps not represented by those
static groups: partial reserve-manifest crash recovery, ambiguous OCI artifact
adoption without destructive replacement for referenced images and Dockerfile
build results, dead planned PEP lifetime recovery,
dead planned publication-listener lifetime recovery, and complete
binding/request authentication before durable mutation. Blocker: none. Next
command-bearing work is those exact corrections and proofs, followed by
E22-E30 atomic caller cutover and the deferred forwarded tests. The expanded
NNCV033 source census passes Bash syntax, scoped ShellCheck, diff check, and
`48/48` mutations; direct remains expected red at `38/40`. D13-D16 now have a
single compute facade, separate services desired/observed state, and exact
execution plus ingress endpoint reads after `Observed`. That shared seam is
green: projection passes `11`, retained provisioner integration passes `8`,
full services passes `99` with one declared ignore, full compute passes `225`
with one declared ignore, and strict compute/services Clippy passes. The same
immutable registry owns effect and observation substitution; no provider-local
state enters the portable saga. Full sandbox is green after the lint cleanup:
`979` tests execute with zero failures, comprising library `970/0/27`,
guest-user-switch `2/0`, capability registration `2/0`, and production network
composition `5/0`; Linux-only targets and doctests contain zero tests on this
host. Six post-refactor focused suites execute `162` passes with five
intentional child-process ignores. Strict sandbox/network Clippy, scoped
Rustfmt over all 30 lane paths, and `git diff --check` pass. E13/E15/E16 have
no remaining lane-owned finding. E14/E25 still awaits its seven focused tests
after caller compilation.

The next dirty checkpoint completes C1 and D17 without changing the atomic
item boundary. C1's native facade passes focused `3 + 9 + 4 + 2 + 9`, full
compute `231/0/1`, strict no-deps Clippy, check, format, diff, and no-bypass
scans. Its source-aware behavior proof provisions one standalone sandbox and
one sandbox-backed service through the real provisioner, records two durable
sagas, executes six provider phases per workload, authenticates exact observed
projection, and repeats no provider effect on replay. D17 passes network
`252/0/1`, workloads `136`, compute network-plan `21/0/1`, sandbox `979/0/27`,
Machine `32`, strict affected Clippy, all `31` constructor census,
role/wire/digest/effect/dependency scans, and scoped format/diff. Its server
integration waits only for the active server owner to remove three stale
product references; the Linux real-provider pair remains Linux-host evidence.
The strengthened NNCV033 binds positive checks to production caller tokens
plus source-specific behavior tests and scans previously omitted compute,
server, node, and composition paths. It passes `48/48` mutations and remains
expected red at `38/40` only for remaining caller-proof and legacy-deletion
groups. Server/Convex, node/hidden-executor, and sandbox coarse-start deletion
proceed as disjoint lanes. Blocker: none.

The node lane has now closed its provision-side acceptance: the coarse
`HostLifecycleBackend::start` authority and inspect-then-start reconciler path
are deleted, DirectProcess and Systemd expose only exact activate/inspect
sinks, and `31/31` focused tests plus library check, format, diff, and static
scans pass. The sandbox lane deleted the host-applicable
`SandboxBackend::start`, `start_sync`, and `finish_start` paths. Its source
audit found and migrated `25` cfg-Linux smoke-test calls to the same exact
phase protocol. Three accidentally overlapping early full-suite attempts did
not complete. Owner `sample` evidence showed live shared
`nimbus-egress-proxy` worker threads, but the exact suspected test later passed
`1/1` in `7.82s` under a `75s` bound and left no child. Source inspection
confirmed those process-static workers are not sufficient evidence of a
lifetime leak. The interrupted runs remain uncounted; the durable JUnit run
below supplies the authoritative full-suite result.

Server production composition now checks after deleting obsolete provider
reports, the service Restart route, and all three late service-manager
composition shims. The stale cfg-test fixtures now use complete managed
composition, and the two exact Convex activation and cancellation tests pass
`2/0/0` with `597` filtered. The cancellation test proves the host-visible
`NimbusRuntimeError::Cancelled` boundary while retained dispatch continues.
All `25` cfg-Linux sandbox smoke callers are also migrated to exact phases;
the zero-hit source census, host-neutral precondition test, and macOS
all-integration no-run compile pass. Removing only their crate-level Linux cfg
gates makes every ignored live-provider source type-check on every host; all
live cases remain ignored. The Linux cross-target check is unverified because
the installed target lacks `aarch64-linux-gnu-gcc` before Nimbus code. A
JUnit-backed diagnostic sandbox run reports `979` passed, one failed, and `46`
skipped in `133.113s`. Its sole red was
`plan_only_backend_does_not_charge_manifest_only_port_previews`: a second
manifest-only preview observed quota retained by the first. The mechanical
test migration called the real reserve-and-prepare helper in a test whose
contract is pure preview. The test-only correction restores the existing
stable-ID `plan_start_with_id` seam and asserts that the preview manifest
carries no port lease; no production quota or lease behavior changed. The
exact failing test, the adjacent re-reservation regression, and all `48/48`
`plan_only_` tests pass. The authoritative post-correction run uses the same
bounded no-retry nextest command and reports `980/980` passed, `46` skipped,
one slow, and zero retries in `124.267s`; durable JUnit is
`target/nextest/ci-nightly/junit.xml`. Package all-target check and strict
`-D warnings` Clippy also pass. Machine/guest exact command transport and
hidden-executor deletion remain active; local and forwarded Compose still
require the canonical compute-backed foreground lifetime. NNCV033 therefore
remains honestly expected red at `38/40`. Blocker: none.

The next integration checkpoint closes the server-owned construction slice and
the exact Machine transport slice without claiming outer caller completion.
`ServerWorkloadProfile::Managed` now boxes only its complete composition, so
strict Clippy no longer requires the protocol-only profile to carry the large
managed payload. Serialized composition tests pass `7/7`; exact
manager-derived listener integration passes `4/4`; the protocol-only
prepared-authority constructor passes `1/1`; server check and strict all-target
Clippy pass. The reusable CLI prepared profile retains one exact frozen manager,
service manager, Krun backend, node identity, source reports, selection, and
requirements; source/static checks pass, but its `13` CLI tests remain
unexecuted until the outer start/Compose compilation wall is removed, and six
of those tests require Linux. The Machine crate checks and passes `32/32`.
After its owned compile corrections, the full CLI check reports zero
Machine-owned errors: one intentional unused forwarded-provider re-export and
six start/dev/Compose callers remain. The pure prepared-default-machine
source/activation seam, start/dev consumption, and foreground Compose runtime
are active as disjoint packets. Blocker: none.

No structured review ran.
The original checkout remains untouched with its user-owned
`AGENTS.md`, concurrent-write benchmark, and browser bundle changes.

The final acceptance-convergence fail-before run executes after every product
caller and deletion is present. NNCV033 is green at `40/40`, its standalone
mutation suite is green at `48/48`, the affected behavioral suites are green at
`2,475/2,475` plus CLI `927/927`, and the listener lint correction is green at
`18/18`. The aggregate correctly stays red until its older control artifacts
recognize the same authorized NNC6.4 replacements: NNCV006 and NNCV015 need
source-derived inventory/census reconciliation; NNCV008 needs the final
Recovery Header; NNCV018, NNCV020, NNCV021, and NNCV024–NNCV028 still name
pre-NNC6.4 exports, hard line thresholds, service activation, manager fields,
the deleted hidden executor, old durable-store symbol locations, or the live
worktree as an NNC6.2 historical baseline. These are explicit control-path
updates inside NNC6.4's existing aggregate-verifier authority. Each update must
replace the stale assertion with equal or stronger evidence for the current
canonical seam; no check may be skipped, broadly exempted, or converted into a
success-by-absence condition. The aggregate fail-before result is `23/34`, and
E33 remains red until the live aggregate is `34/34` and its retained mutation
suite is `325/325`. Blocker: none. No structured review ran.

The final documentation gate then exposed one source-derived cross-layer
correction. The server restart route was deliberately deleted with the coarse
service activation authority, while the canonical JS SDK, generated embedded
package, system route inventory, public resource/API references, and node/CLI
architecture prose still advertised the deleted path or hidden node executor.
Those surfaces are now explicitly owned for narrow removal/correction in this
item. NNC6.4a remains the sole owner of a future explicit restart cutover
through the fenced saga; NNC6.4 will not restore it through a stop/start shim.
Blocker: none. No structured review ran.

Final acceptance convergence is green before review. Direct NNCV033 passes
`40/40`; its mutation suite passes `48/48`; the live aggregate passes `34/34`;
and the bounded aggregate self-test passes `325/325`. Affected non-CLI behavior
passes `2,475/2,475` with `79` declared skips, the refreshed CLI suite passes
`927/927` with one skip, `nimbus-system` passes `73/73` with the documented
external-fixture guard, and the focused listener proof passes `18/18`. The
canonical SDK build, typecheck, capability segregation, `24`-route parity,
embedded-package staging, and package-closure checks pass. Docs pass `108` and
site verification passes `17/17`. Strict affected Clippy and warning-denied
rustdoc, Rustfmt, Prettier, diff, Bash syntax, scoped ShellCheck, dependency,
effect, source, and exact `290`-changed-Rust-file/`27`-threshold modularity
checks pass. The candidate-frozen executable/script patch SHA-256 is
`744fe56d40dbbb418d4450700f9a0662471fa93afa4814ec7ca9dd5a301fb0dd`; the
pre-ledger staged tree is `44008a58a544e6023ddc0baf625bf35d13204c22` and
its full patch SHA-256 is
`43cb1fcb6cd15f3e15537eabf2c376bfc9619c2c23743321e477caafee294276`.
Exactly `322` paths were staged with zero unstaged paths at candidate freeze.
E1-E34 are complete; E35 is now authorized exactly once.

The sole full E35 review then completed as eight internal bundle passes with
`41` findings and overall confidence `0.98`. This is one item review, not eight
review cycles. Its Codex thread IDs are
`019fcb20-fe72-7a72-81f6-c57070a53202`,
`019fcb27-eb7f-7092-8dd2-1036a9f4e808`,
`019fcb2c-1933-7e71-bc6c-7e14319bbcef`,
`019fcb31-db62-7b31-a700-5bdfdb461e9e`,
`019fcb37-5dd3-7b72-a571-9492ffd68ce9`,
`019fcb3d-1919-7461-bbe5-b465d985b17d`,
`019fcb43-2b26-7a21-b4a8-ef695bfbe8e7`, and
`019fcb48-237f-7952-9bfb-b140ca4b7bc8`. Evidence disposition is active.
The first accepted corrections require durable `InspectionRequired` state
before absence-authorized retry and normalize a definite provider failure
identically on first result and exact replay; focused proofs pass `3/3` and
`2/2`. No second broad review is authorized.

GPT-5.6 Sol completed the one permitted narrow correction review against the
accepted executable defect set. It used xhigh and fast modes. The Post-Review
Correction Checkpoint lists the six accepted findings and their corrections.
The correction also exposed and fixed the source-owned
`egress-pep` dependency mismatch between the compute compiler and both OCI
providers.

The final post-correction run passes affected non-CLI `2,502/2,502` with `79`
skips, CLI `936/936` with one skip, system `73/73`, and listener lease `18/18`.
NNCV033 passes `40/40` direct checks and `50/50` mutations. The aggregate
passes `34/34` and `327/327` adversarial cases.

All-target check, strict Clippy, warning-denied rustdoc, Rustfmt, Prettier,
diff, Bash syntax, and scoped ShellCheck pass. Dependency, effect, source, and
exact `299`-Rust-file and `27`-threshold modularity checks also pass. The
unchanged candidate SDK evidence remains green for build, typecheck, tests,
24-route parity, and package closure. This proof records the final docs and
candidate identity before the item commit. The review cadence is complete.

The final pre-ledger candidate stages `326` owned paths with no unstaged or
untracked path. Its staged tree is
`c2bbd4ff3cbb9e5c49d59a1d737333cc3621fe9f`. Its full binary patch SHA-256 is
`1e899c0a2b29f93552bf0402414cddd94f70792157dd9aa83a82484c53d71b66`.
The executable and script patch SHA-256 is
`0486001f13ad2d72919784fdee9d1e68830669786102db284289ad40619691e4`.
