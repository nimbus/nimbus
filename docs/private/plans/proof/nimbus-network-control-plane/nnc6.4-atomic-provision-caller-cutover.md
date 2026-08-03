# NNC6.4 Atomic Provision Caller Cutover

Status: `in progress. product-free preparation checkpoint ready to commit`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Activation commit: `76c920a12ed21eb8b81c1de088bcad52fd0d81e4`

Dependency commit: `c42c61fb2d97d037069f3b27b9055d6e58f11d1d`

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

### D13: Bind each command to its actual provider authority

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
20. retry lacks absence evidence.
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

The audit identified these complexity pockets:

- shared OCI attachment lifecycle: about 1,465 production lines and currently
  mixes attachment with publication.
- Container runtime composition root: about 1,378 lines.
- Krun VM composition root: about 877 lines.
- `ServiceManager`: definitions, activation claims, effects, handle cache,
  teardown, and projections in one owner.
- guest Machine API start: prepare, activation, status, and publication in one
  method.
- two coarse Machine API routes and open request payloads.
- node inspect-then-start concurrency and Systemd replacement semantics. and
- `crates/nimbus-workloads/src/saga/tests.rs`: 2,275 lines.

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
| E22 | Native sandbox, native service, Convex async activation, local Compose, forwarded Compose, Machine API, guest node, and hidden node executor all enter the compute saga. |
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
| E33 | NNCV033 passes 40/40 direct checks and 48/48 mutations. the aggregate passes 34/34 and 325/325. |
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

## Status Ledger

| Checkpoint | State | Evidence | Next exact action |
| --- | --- | --- | --- |
| Source audit | `done` | Three read-only lanes changed zero paths. Current/target graphs, caller census, protocol gap, provider-target correction, complexity pockets, and deletion gates are frozen above. | Preserve the frozen census while product implementation proceeds. |
| Expected-red contract | `done` | NNCV033 passes `48/48` mutations. Current product passes `4/40` direct groups and fails the exact 36 NNC6.4 implementation groups. NNCV032 passes `32/32` and `36/36` from its pinned completion tree. The live aggregate is expected red at `33/34`; its bounded self-test passes `325/325`. Docs pass `108`; site passes `17/17`. | Force-add the ignored proof, stage the exact product-free preparation paths, inspect the staged tree, and commit the checkpoint. |
| Portable protocol | `todo` | E1-E10. | Add CAS provenance, dispatch claim/epoch, inspection, absence evidence, command fence, reducer, and strict state transitions. |
| Dispatcher and composition | `todo` | E11-E15. | Add exact registry, small capabilities, source/report freshness checks, managed ComputeState composition, and deterministic fakes. |
| Provider phase split | `todo` | E13-E21. | Split Container/Krun attach from publish. add provider journals and server/forwarded/node substitutions. |
| Caller cutover and deletions | `todo` | E22-E32. | Replace every inventoried caller and delete every coarse authority atomically. |
| Acceptance convergence | `todo` | E1-E34. | Run focused, affected, crash, process, static, quality, and docs gates. freeze exact candidate identity. |
| Item review | `todo` | E35. | Run one Sol/xhigh/fast review only after E1-E34 are green. |
| Item commit | `todo` | Exact reviewed tree plus final ledger. | Commit one complete NNC6.4 item. Do not push or open a PR. |

## Recovery Record

This proof, the plan Recovery Header, and the checkpoint ledger preserve the
source census and frozen contract. The dirty worktree preserves the verified
preparation checkpoint until its commit. During implementation, append one row
per meaningful checkpoint. Record the exact HEAD, dirty paths, last green
command/count, current red condition, finding dispositions, blocker, and next
exact command.

No structured review is authorized until the entire item satisfies E1-E34.
