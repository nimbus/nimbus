# NNC6.5d4 Forwarded-Machine Teardown Provider

Status: `audit and fail-before complete. implementation not started`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.5d4 adds exact forwarded-machine teardown provider adapters and a strict
private Machine API phase envelope. It proves real substitution for the five
compute-owned teardown capabilities without changing a product caller. The
parent host withdraws its publication before it asks the guest to drain or
stop. The guest executes or inspects one exact phase in its own durable realm.
The parent releases its complete port batch only after exact guest release and
independent provider-absence evidence.

This item does not change Compose down, native service or sandbox stop,
physical-machine stop, tenant retirement, or definition deletion. It does not
delete the coarse Machine API stop route. It does not add a public server
route, a CLI-local saga store, a second port authority, or a second guest
provider-command journal. It does not change tenant policy, service naming,
proxy policy or forwarding ownership, cluster transport, or the dependency or
effect boundary of `nimbus-network`.

The read-only audit ran at `c1c7f1397` from a clean owner worktree. Three
bounded audit packets covered the parent authority, guest and wire protocol,
and compute substitution and recovery contract. Product source stayed
unchanged. The audit corrects the stale path in the parent audit: the current
backend is `crates/nimbus-cli/src/machine/backend.rs`, not a nonexistent
`crates/nimbus-machine/src/machine/backend.rs`.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| K1 | The read-only source audit names the current parent backend, source plan, exact provider identities, confirmed publication journal, port authority, Machine API vocabulary, client, private route, guest facade, Systemd lifecycle, Container runtime, provider journal, compute command, portable receipt, and five-capability registry authorities. |
| K2 | `nimbus-compute` remains the sole saga coordinator. The parent and guest are effect sinks. Neither realm admits desired state, chooses phase order, advances a workload record, retries a later phase, or opens the other realm's durable state. No CLI-local saga store is added. |
| K3 | `ConfirmedWorkloadTeardownCommand` retains the exact ordered prefix of already-committed `WorkloadTeardownReceipt` values from its durable context. Command result construction and callback authentication bind the same prefix. A stale or substituted prefix cannot publish a result. |
| K4 | One portable receipt-prefix validator proves the exact prior evidence required by the current step: none for `WithdrawPublication`. `PublicationAbsent` for drain. plus `ExecutionDrained` for stop. plus `ExecutionStopped` for detach. plus `NetworkDetached` for release. It validates claim, confirmation, subject, generation, attempt, provider role, dispatch epoch, and order. |
| K5 | The admitted forwarded attachment, execution, and ingress provider IDs are pairwise distinct, deterministic, bound into source evidence, and independent of every IP address, port, socket path, process ID, and provider handle. The parent adapter rejects any role or provider substitution without fallback. |
| K6 | One real `ForwardedMachineTeardownAdapter` implements `IngressWithdrawalCapability`, `WorkloadExecutionDrainCapability`, `WorkloadExecutionStopCapability`, `NetworkDetachmentCapability`, and `NetworkReleaseCapability`. It registers only the exact admitted provider IDs and shares the already-composed source, client, parent publication, port, and live-lifetime authorities. |
| K7 | The adapter uses one parent `ProviderCommandAttemptJournal` namespace for all five independent operation streams. Final teardown withdrawal has a teardown-family operation distinct from restart withdrawal. restart claims keep their source-attempt and restart-ordinal rules unchanged. Each step has its own exact claim, epoch, durable result, and compute result CAS. The confirmed publication journal records retained publication progress, not a second command result or saga. |
| K8 | Parent command validation authenticates the complete tenant-qualified key, saga, command, issuing and confirmed revisions and transitions, generation, desired/source/plan digests, capability selection, execution locator and attempt, teardown attempt and epoch, step, mode, subjects, provider target, source plan, forwarder instance and generation, canonical plan, and complete publication member batch before journal mutation, Machine API bytes, port mutation, or provider effect. |
| K9 | `WithdrawPublication` is parent-local. It authenticates the full publication batch, persists withdrawal intent before the first forwarding effect, removes or recovers the exact live lifetime guards, and proves every parent forwarding member absent while every port lease remains retained and non-bindable. Failure or ambiguity causes zero guest drain or stop request. |
| K10 | The confirmed publication authority has an explicit strict progression that distinguishes active, withdrawal-may-exist, withdrawn-and-retained, release-may-exist, and released/retired. Missing, crossed, partial, or corrupt state is ambiguous. A Boolean `retired` value is not enough for the new path. |
| K11 | `nimbus-machine` owns one strict private `MachineApiWorkloadTeardownCommandEnvelope`, request digest, request, response, mode-specific observation, and wire error in `api/teardown.rs`. The internal path is `/v1/machine-api/workload-teardown/phase`. no public `nimbus-server` route or product protocol is added. |
| K12 | The request digest is domain-separated and covers the complete forwarder authority, complete parent command, exact prior receipt prefix, and a closed parent-to-guest provider translation. Strict deserialization rejects missing, null, unknown, noncanonical, stale, skipped, or crossed fields. |
| K13 | The response echoes and validates the request digest, forwarder authority, command and transition IDs, attempt and epoch, parent provider target, step, subjects, mode, and exact observation. A missing, undecodable, oversized, or crossed response is ambiguous because the guest effect can already exist. |
| K14 | The wire keeps closed mode-specific outcomes. Execute can return only `Succeeded`, `DefiniteFailure`, or `Ambiguous`. Inspect can return only `Satisfied`, `NotCompleted`, `DefiniteFailure`, `InProgress`, or `Ambiguous`. Stable `WorkloadFailureEvidence`, success evidence, and owner evidence survive the round trip. |
| K15 | The typed parent-to-guest translation selects guest capabilities from the boot-composed provider bundle. It maps the parent execution role to the exact guest execution composition and the parent attachment role to the exact guest Container attachment owner. It rejects `WithdrawPublication` at the guest and never accepts a free-form caller-selected guest provider ID. |
| K16 | Production guest composition creates Systemd with a deterministic durable teardown-state root and retains the same concrete backend behind `HostLifecycleBackend`, `HostExecutionDrainProvider`, and `HostExecutionStopProvider`. Capability reporting is unavailable when the store or required backend is unavailable. |
| K17 | Guest drain is one explicit composite operation. One extracted teardown-phase journal seam claims the generic guest operation once, then closes new Systemd activation and every Container creator, activation, restart, and provider-dispatch producer. It reports `ExecutionDrained` only after both barriers are exact and every already-admitted operation is settled or conclusively absent. |
| K18 | Guest stop is one explicit composite operation under the same one-time generic claim. It durably sequences the exact Systemd and Container execution owners without marking the generic journal terminal after only one subeffect. It reports `ExecutionStopped` only after exact Systemd-unit absence and exact Container runtime terminality for the same execution attempt. The Container owner inspects before any signal. an already-terminal runtime causes no second stop effect. All network authority remains retained. |
| K19 | Guest detach and release use a forwarded-machine Container adapter that is distinct from the host-managed adapter only at composition authentication. An authenticated Systemd-to-Container terminal-evidence bridge records the exact matching Container stop fence only after runtime inspection proves terminality. it never fabricates evidence or repeats a terminal effect. The adapter reuses the existing Container manifest, `container-runtime` journal, guest machine-publication owner, shared OCI retained-detach/final-release mechanics, IPAM, segment, PEP, listener, and attachment authorities. It does not weaken or fake the host-managed manifest contract. |
| K20 | The forwarded guest attachment adapter authenticates the exact prior `ExecutionStopped` receipt and exact guest machine-publication absence before detach. Release authenticates the prior `NetworkDetached` receipt and complete compound detached proof. NNC6.5d3 host-managed ordering and no-reuse guarantees remain unchanged. |
| K21 | The guest uses the existing Container-rooted `ProviderCommandAttemptJournal` for drain, stop, detach, and release. Execute claims durably before effects. Inspect adopts the exact attempt and is read-only. The guest does not create a second generic journal, parent journal, publication authority, workload store, port authority, or result CAS. |
| K22 | Before a remote Execute, the parent durably records the exact request and request-may-exist boundary. Response loss or either process death forces exact guest Inspect before a retry. A retransmitted Execute cannot overlap an older request that can still commit or publish. |
| K23 | Parent `ReleaseNetwork` first proves exact guest detach, exact guest release, and independent guest provider and publication absence. It then atomically releases the complete parent port batch and marks the retained publication released. No earlier phase can release or rebind a parent port. |
| K24 | Complete-batch fencing is strict. One absent member with one present sibling is `InProgress`. one unknown or crossed sibling is `Ambiguous` or definite crossed failure as applicable. A partial guest result, partial parent observation, partial lifetime recovery, or subset request releases no member and preserves the complete batch byte for byte. |
| K25 | Zero-listener workloads still execute the five portable phases. Parent ingress withdrawal and final release use explicit empty-batch success, not a synthetic port, skipped guest phase, or inferred absence. |
| K26 | Invalid, crossed, stale, skipped, ordering, and identity failures preserve the frozen stable codes. Provider-store corruption, journal corruption, response-correlation failure, unknown effect state, missing authority, and partial evidence remain `Ambiguous`. they are never rewritten as definite absence. |
| K27 | Exact duplicate commands replay with no new effect. Adjacent retry epochs require the prior exact durable absence or retry receipt. Two threads, two subprocesses, and an Inspect contender produce one effect/result winner per phase. Inspect never returns `NotCompleted` while an older exact effect can still start, commit, or publish. |
| K28 | Fresh parent and guest processes recover every frozen two-realm cut from only their own durable roots. Parent-root bytes are unchanged by guest operations. Guest-root bytes are unchanged by parent-only withdrawal and final port release. Neither process receives an in-memory snapshot as authority. |
| K29 | Real compute registry substitution executes and inspects all five phases through the forwarded adapter. Every provider result authenticates the complete confirmed command and receipt prefix before compute's existing result CAS. Registry selection has no fallback, no-op, or compatibility shim. |
| K30 | The new route is available only when the exact guest capabilities, durable Systemd teardown store, Container provider journal, attachment owner, and installed forwarder authority are available. Capability status names precise blockers and never reports aspirational support. |
| K31 | Concept-owned children keep composition roots thin: `machine/backend/teardown.rs`, `machine/client/teardown.rs`, `machine/api/service_workloads/teardown.rs`, `nimbus-machine/api/teardown.rs`, a confirmed-retirement progression child, and a forwarded Container attachment child. Any changed handwritten file at 1,500-1,999 lines gets an explicit owner reason. any file at 2,000 lines is decomposed or has a recorded strong exception. |
| K32 | Coarse `stop_service_sandbox`, Compose, physical-machine stop, native callers, definition deletion, tenant retirement, and legacy cleanup remain unchanged for NNC6.5e-NNC6.5g. The new adapter never calls the coarse stop path. No public route, service-name resolver, tenant-policy seam, cluster transport, provider effect, or workspace dependency enters `nimbus-network`. |
| K33 | Focused wire, parent, guest, node, sandbox, port-batch, compute-substitution, replay, contention, crash, stale/crossed, sibling-batch, zero-listener, and effect-order tests pass. Full affected crates, strict Clippy, warning-denied rustdoc, format, dependency/effect scans, modularity census, NNCV035 arithmetic, proof lint, docs, and site gates pass with exact counts. |
| K34 | The source-derived teardown verifier names the real exact envelope, dispatch, authentication, parent-withdraw-before-guest-stop, and release-after-guest-absence seams. NNCV000-NNCV034 stay green. NNCV035 remains the sole expected red condition because caller cutover and coarse-stop deletion belong to NNC6.5f-NNC6.5g. |
| K35 | Exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast item review runs only after K1-K34 are green. Only an accepted material executable finding permits one narrow correction review. Internal wrapper chunking does not create another review unit. |

## Current Ownership And Call Graph

The current forwarded retirement path is coarse and ordered incorrectly:

```text
SandboxBackend::stop
  -> ForwardedMachineApiSandboxBackend::retire
     -> publication_journal.retirement_for(sandbox_id)
     -> MachineApiClient::stop_service_sandbox
        -> private coarse /service-sandboxes/{sandbox_id}/stop
        -> GuestNodeWorkloadService::stop
           -> HostLifecycleBackend::stop
           -> ContainerSandboxBackend::stop
              -> guest runtime + machine publication + network cleanup
     -> retire_parent_publication
        -> release complete parent port batch
        -> mark one Boolean retired
```

The defects are observable:

- guest execution stops before parent publication withdrawal.
- one sandbox path ID selects the operation instead of a tenant-qualified
  confirmed command.
- the request carries only `MachineForwarderAuthority`.
- one coarse guest call recombines drain, stop, detach, and release.
- there is no exact teardown request digest, response correlation, parent
  provider journal, or guest phase sink.
- parent retirement has only active versus retired state.
- the caller recovers response loss by issuing the coarse stop again.
- production Systemd composition has no durable teardown store.
- the guest erases the exact drain and stop traits behind
  `Arc<dyn HostLifecycleBackend>`.
- `ProviderCommandOperation::WithdrawPublication` belongs to the restart
  family, so a final teardown claim cannot reuse it without violating restart
  source-attempt and restart-ordinal invariants.

The portable reducer and registry already own the correct order and five small
capability ports:

```text
WithdrawPublication -> DrainExecution -> StopExecution
  -> DetachNetwork -> ReleaseNetwork
```

The current confirmed command carries the current claim, source, compiled
network plan, and execution locator. It does not carry
`WorkloadTeardownDisposition::context().completed()`. This is sufficient for a
single host-managed backend, where one local manifest proves execution stop
before detach. It is not sufficient for the forwarded guest, where
`nimbus-node` owns execution and `nimbus-sandbox` owns attachment state.

## Target Ownership And Call Graph

```text
nimbus-compute: sole workload teardown saga + receipt sequence + result CAS
  -> ForwardedMachineTeardownAdapter (parent provider sink)
     -> WithdrawPublication
        -> parent provider journal claim
        -> confirmed parent publication progression
        -> complete-batch forwarding withdrawal
        -> retain every parent port lease
     -> Drain / Stop / Detach / Release
        -> parent provider journal claim + request-may-exist
        -> private exact Machine API teardown envelope
           -> guest facade validates installed authority and translation
           -> guest container-runtime provider journal
              -> exact Systemd + Container drain/stop composition
              -> exact forwarded Container detach/release composition
           -> durable guest observation -> correlated response
        -> durable parent observation
     -> ReleaseNetwork tail only
        -> prove guest release + provider/publication absence
        -> atomic complete parent port release
        -> confirmed parent publication -> Released
  -> existing compute result CAS
```

The two durable realms stay independent:

| Authority | Canonical state | Must not own |
| --- | --- | --- |
| Compute | Desired workload teardown, ordered receipts, phase claim, result CAS | Provider effects or retry of unconfirmed work |
| Parent provider journal | Exact forwarded command claim and result for each phase | Publication membership, port leases, guest progress, or saga order |
| Parent confirmed publication authority | Exact member batch, forwarder fence, retained-withdrawn/released progression | Generic command result or guest execution state |
| Parent port authority | Host-global port lease, lifetime, complete-batch retained/released state | Workload policy or guest state |
| Guest provider journal | Exact guest-local claim/result for drain, stop, detach, release | Parent state, compute CAS, or a second saga |
| Systemd teardown store | Exact guest unit drain/stop progress and receipts | Container runtime or network authority |
| Container manifest | Exact Container runtime, machine publication, attachment detach/release progress | Systemd unit state or parent port authority |

This separation permits deterministic tests. A process can reopen only its own
realm, inspect the exact attempt, and prove that it cannot duplicate an effect
or release another realm's authority.

## Exact Provider And Phase Mapping

The source plan already freezes three distinct parent identities:

| Role | Provider identity source | Exact teardown use |
| --- | --- | --- |
| Attachment | `nimbus-machine.forwarded-container-attachment` | Parent attachment capability. translates to the guest Container attachment owner. |
| Execution | `nimbus-machine.forwarded-container-execution` | Parent drain and stop capabilities. translates to the composed guest Systemd plus Container execution owner. |
| Ingress | `MachineForwarderAuthority.provider_instance().provider_id()`. currently the gvproxy forwarder registration | Parent-only publication withdrawal. |

The request binds a closed translation chosen by trusted composition. It does
not make the guest provider ID a caller-controlled string.

| Portable step | Required prior receipt prefix | Parent action | Guest action |
| --- | --- | --- | --- |
| `WithdrawPublication` | empty | Withdraw the exact complete parent publication batch and retain ports. | Reject before journal access. |
| `DrainExecution` | `PublicationAbsent` | Send or inspect the exact remote command. | Close and prove the Systemd and Container admission barriers. |
| `StopExecution` | `PublicationAbsent`, `ExecutionDrained` | Send or inspect the exact remote command. | Prove exact Systemd-unit absence and Container runtime terminality. |
| `DetachNetwork` | preceding receipts plus `ExecutionStopped` | Send or inspect the exact remote command. | Prove guest machine publication absence and retained compound network detach. |
| `ReleaseNetwork` | preceding receipts plus `NetworkDetached` | Send or inspect guest release. after success, prove absence and release the parent batch. | Final-release guest listener, PEP, IPAM, segment, and attachment authority. |

An IP address, port, socket path, PID, systemd unit name, or provider handle can
appear only as observed or provider-local evidence. It never selects workload
authority.

## Wire Contract

`crates/nimbus-machine/src/api/teardown.rs` owns portable private transport
vocabulary only. It has no socket, route, provider effect, workload store, or
saga coordinator. The restart wire contract is the exemplar because it has a
domain-separated request digest and strict response correlation.

The command envelope and digest bind:

- command ID, saga key and ID, issuing and confirmed revisions, and issuing
  and confirmed transition IDs.
- workload generation, desired digest, required node, admitted source and
  source digest.
- complete compiled network plan, plan digest, and capability selection.
- execution locator and execution attempt.
- teardown attempt, dispatch epoch, current claim, step, mode, typed subjects,
  and parent provider target.
- exact ordered prior receipt prefix.
- complete forwarder authority and machine provider generation.
- closed parent-to-guest provider translation.

The response correlates every field that can select an effect or result. It
uses separate Execute and Inspect outcome enums. Wire validation errors describe
which fence is invalid. They do not invent a provider `DefiniteFailure` after
the request might reach the guest.

The internal Unix-socket route authenticates the boot-installed forwarder
authority before facade lookup. It then passes exactly one validated envelope
to the effect sink. It does not accept a path-derived sandbox identity and does
not open the compute store.

## Parent Publication And Port State

The existing confirmed publication journal is exact retained authority, but
its terminal marker is one `retired` Boolean. The new concept-owned retirement
child must use a strict progression:

```text
Active
  -> WithdrawalMayExist
  -> WithdrawnRetained
  -> ReleaseMayExist
  -> Released
```

Each transition binds the exact command, forwarder, canonical plan, and member
batch.

- `WithdrawalMayExist` is durable before a lifetime or forwarding effect can
  stop.
- `WithdrawnRetained` requires an exact complete-batch observation.
- `ReleaseMayExist` requires the exact guest `NetworkReleased` response and an
  independent guest provider and publication absence check.
- `Released` requires the atomic complete-batch port release.
- Exact terminal port inspection recovers a crash after port release and
  before the final journal write.

The parent provider journal remains the command result authority. The
confirmed publication progression records only publication and lease effect
progress. These stores bind one another by exact command and member digest.
they do not duplicate the same state.

For an empty member batch, the same progression completes without a port
effect. It cannot skip guest drain, stop, detach, or release.

## Guest Execution And Attachment Composition

The guest workload is Systemd-wrapped Container execution. Exact stop cannot
declare success after only one owner is terminal. The guest teardown child
therefore composes two existing narrow owners behind one guest-local state
machine:

```text
DrainExecution
  -> durable composite drain intent
  -> Systemd exact admission barrier
  -> Container exact admission barrier
  -> both drained -> one guest result

StopExecution
  -> require exact composite drain proof
  -> durable Systemd stop may-exist -> exact unit absence
  -> durable Container stop may-exist -> exact runtime terminality
  -> both terminal -> one guest result
```

The generic `container-runtime` journal has one claim and one terminal result
for the portable guest operation. A concept-owned teardown-phase seam passes
one authenticated execution claim to the composed substeps instead of trying
to claim the same stream again. Provider-owned substep progress lives with the
Systemd teardown store and Container manifest. A crash after the first
subeffect reopens those stores and resumes the second subeffect.

Container
inspection can record an exact externally-stopped terminal fence after Systemd
unit absence proves the parent process boundary is dead. If the runtime is
still live, the state stays in progress or ambiguous until the one authorized
Container stop path settles it. The composition does not publish a partial
generic result and does not create another generic journal.

The NNC6.5d3 host-managed attachment adapter correctly rejects machine-forwarded
composition and requires its local execution state. The guest must not weaken
that contract. A new forwarded composition child authenticates the prior
portable receipt prefix and exact guest machine-publication absence. It also
authenticates the same Container manifest before it calls the shared
retained-detach/final-release mechanics. It reuses every existing effect and
authority owner.

Production guest composition must retain one concrete
`SystemdTransientUnitBackend` behind the lifecycle, drain, and stop trait
objects. The Linux factory must construct it with a deterministic teardown
root below the guest control-data directory. Non-Linux and unavailable
composition stay fail closed and report the exact blocker.

## Frozen Failure Roster

| Case | Required result and proof |
| --- | --- |
| Wrong step, subject kind, mode, or guest mapping | `DefiniteFailure(sandbox_teardown_command_invalid)` before path derivation, journal access, Machine API bytes, or effect. |
| Parent versus guest provider substitution | `DefiniteFailure(machine_teardown_provider_crossed)` and both realms remain byte stable. |
| Crossed tenant, saga, command, execution attempt, node, source, desired, plan, selection, member batch, or forwarder | Typed crossed or stale failure before effect. no fallback. |
| Stale workload or machine generation | `DefiniteFailure(machine_teardown_forwarder_stale)` or the frozen sandbox stale code, with successor bytes unchanged. |
| Stale/skipped dispatch epoch or crossed transition | Frozen stale/epoch-invalid failure. never start or adopt another attempt. |
| Missing or substituted prior receipt | `DefiniteFailure(machine_teardown_order_invalid)` or `sandbox_teardown_order_invalid`. run no later-phase effect. |
| Receipt prefix valid in shape but crossed in claim, subject, generation, or provider | Definite crossed failure. preserve every store and lease. |
| Missing, corrupt, or partial parent publication authority | `Ambiguous`. release no port and send no guest request. |
| Missing or corrupt guest manifest, node receipt, or journal | `Ambiguous` unless an exact terminal guest journal observation can be replayed. Missing files never prove absence. |
| Exact duplicate Execute | Exact replay or adoption. no second parent request or guest effect. |
| Exact Inspect with live older request | `InProgress` or `Ambiguous`, never `NotCompleted`. |
| Lost or crossed guest response | `Ambiguous`. retain request-may-exist and every parent port. Inspect the exact guest command before retry. |
| Parent withdrawal partial, present sibling, or unknown sibling | `InProgress` for exact live evidence or `Ambiguous` for unknown state. Preserve the complete batch. |
| Guest drain incomplete | `InProgress` or `Ambiguous`. send no stop request. |
| Systemd terminal but Container runtime live, or the inverse | `InProgress` or `Ambiguous`. do not publish `ExecutionStopped`. |
| Detach before execution terminality or guest publication absence | `DefiniteFailure(sandbox_teardown_order_invalid)` and no network mutation. |
| Release before compound detached proof | `DefiniteFailure(sandbox_teardown_order_invalid)` and no authority release. |
| Parent release before exact guest release and independent absence | `DefiniteFailure(machine_teardown_order_invalid)` and all parent ports remain fenced. |
| Stale callback after successor generation or provider replacement | Reject the callback. It cannot record a result, release a port, or mutate the successor. |
| Address, port, socket path, PID, unit name, or provider handle offered as workload identity | `DefiniteFailure(sandbox_teardown_identity_invalid)` before lookup. |

## Two-Realm Crash, Restart, And Concurrency Matrix

Every capability has an independent compute claim, parent provider claim,
parent progress record, parent observation, and compute result CAS. Each remote
guest phase also has an exact request, guest claim, guest provider progress,
guest observation, and correlated response.

The outer matrix is:

1. compute persists the exact phase claim and prior receipt prefix.
2. the parent validates the complete command.
3. the parent provider claim is durable.
4. parent request ID and request-may-exist evidence are durable before send.
5. the parent dies before send, after send, or after response loss.
6. the guest validates the complete digest, authority, translation, and
   receipt prefix before journal access.
7. the guest claim is durable before any effect.
8. provider substep progress is durable before each effect.
9. the guest dies before, during, or after an effect.
10. a fresh guest process inspects exact durable state before retry.
11. the guest observation is durable before response construction.
12. the transport delivers, loses, truncates, or crosses the response.
13. a fresh parent process adopts the exact request and sends Inspect before
    any new Execute.
14. the parent persists the exact correlated observation.
15. compute dies before or after the result CAS and a fresh process replays it.
16. only the committed receipt can authorize the next phase.

Parent withdrawal adds cuts after withdrawal intent, each member stop, complete
batch observation, and retained-state publication. Parent final release adds
cuts after guest response validation, independent guest-absence observation,
release intent, atomic batch release, and the final released journal state.

Guest drain adds cuts after each admission barrier and each already-admitted
operation settlement. Guest stop adds cuts after each Systemd and Container
may-exist boundary, effect, and terminal observation. Guest detach and release
reuse all NNC6.5d3 provider, namespace, listener, PEP, IPAM, segment, and
portable-attachment cuts with the forwarded composition prerequisites.

For every phase, two synchronized Execute contenders, two process contenders,
and one Inspect contender prove one result/effect winner. A second workload
with the same local sandbox string in another tenant cannot observe, claim, or
alter the first workload's attempt.

## Complete-Batch Proof Matrix

| Parent members | Guest evidence | Required classification | Port result |
| --- | --- | --- | --- |
| All withdrawn | Exact current phase | Continue. | Retain all until final release. |
| One present sibling | Exact current phase | `InProgress`. | Retain all. |
| One unknown sibling | Exact current phase | `Ambiguous`. | Retain all. |
| One crossed or missing member | Any | Definite crossed failure or `Ambiguous` for missing authority. | Byte-stable complete batch. |
| All withdrawn | Partial guest receipt prefix | Definite order/crossed failure. | Retain all. |
| All withdrawn | Lost guest response | `Ambiguous`. exact Inspect next. | Retain all. |
| All withdrawn | Exact guest detach only | Continue to guest release, not parent release. | Retain all. |
| All withdrawn | Exact guest release, absence unknown | `Ambiguous`. | Retain all. |
| All withdrawn | Exact guest release and exact absence | Final release authorized. | Atomically release all. |
| Empty canonical batch | Exact phase evidence | Explicit empty success. | No synthetic lease. complete remaining phases. |

## Frozen Path Ownership

Primary product ownership:

- `crates/nimbus-workloads/src/saga/teardown.rs` and focused state/wire tests
  for the validated ordered receipt-prefix contract.
- `crates/nimbus-compute/src/workload_saga/teardown_command.rs`, callback
  authentication, registry composition, and real substitution tests.
- `crates/nimbus-machine/src/api/teardown.rs` and focused wire tests, with
  narrow exports from `api.rs`.
- `crates/nimbus-cli/src/machine/backend/teardown.rs` and concept-owned tests
  for the parent five-capability adapter. Narrow shared-authority accessors may
  stay beside `ForwardedMachineProvisionAdapter`. Put teardown behavior only
  in the child.
- `crates/nimbus-cli/src/machine/client/teardown.rs`,
  `machine/api/service_workloads/teardown.rs`, and focused tests.
- narrow private route, guest facade, capability reporting, and production
  composition changes in `machine/api/routes.rs`, `machine/api.rs`, and
  `machine/api/service_workloads.rs`.
- a concept-owned confirmed-publication retirement progression child and
  exact complete-batch port-lifetime tests.
- one forwarded Container attachment teardown composition child. Put focused
  tests beside this owner. Reuse existing machine-publication, exact
  execution, attachment, and provider-journal owners.
- a concept-owned shared confirmed-teardown-to-provider-journal seam and one
  teardown-family final-withdraw operation. Restart withdrawal semantics and
  its existing operation remain unchanged.
- this proof, canonical plan and ledger, routing index, and only the
  source-derived verifier coordinates required by real new paths.

Forbidden paths and seams:

- Compose down, native service or sandbox callers, definition deletion,
  tenant retirement, physical-machine stop, and coarse stop deletion.
- public `nimbus-server` routes or application protocol changes.
- tenant admission, quota, service name, DNS, certificate, proxy policy, or
  forwarding ownership.
- a CLI-local saga store, second guest provider journal, or second parent port
  authority. The parent cannot access guest files. The guest cannot access
  parent files.
- a god teardown provider, optional no-op, compatibility decoder, feature
  flag, IP-address identity, or speculative abstraction.
- any socket, Axum, Pingora, Netavark, nft, gvproxy, Iroh, cluster transport,
  cloud SDK, provider effect, or new workspace dependency in `nimbus-network`.

## Complexity And Pattern Decisions

| File | Audit lines | Disposition |
| --- | ---: | --- |
| `crates/nimbus-cli/src/machine/backend/provision.rs` | 1,632 | Existing source-plan and exact provision composition root. Put parent teardown in `backend/teardown.rs`. expose only narrow shared-authority methods. |
| `crates/nimbus-cli/src/machine/client.rs` | 1,405 | Near the explicit-reason threshold. Put teardown transport in `client/teardown.rs`. |
| `crates/nimbus-cli/src/machine/publication_authority/confirmed.rs` | 1,240 | Keep retained publication identity and storage here. put the new strict retirement progression and tests in concept children. |
| `crates/nimbus-sandbox/src/provider_command.rs` | 1,561 | Existing deep provider-command journal. Reuse it without adding forwarded composition logic. |
| `crates/nimbus-sandbox/src/backends/container/runtime.rs` | 1,601 | Existing composition root. Register a child. do not inline guest teardown. |
| `crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs` | 1,508 | Existing exact machine-publication adapter. Add only narrow visibility or composition seams. keep new teardown logic in a child. |
| `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs` | 1,606 | Existing machine-publication state machine. Do not duplicate it. Add focused tests beside its owner. |
| `crates/nimbus-node/src/systemd_transient.rs` | 1,369 | Add a narrow durable-root factory or builder. keep teardown behavior in its existing child. |
| `crates/nimbus-node/src/host_lifecycle.rs` | 1,490 | At the threshold edge. Reuse the exact teardown traits. do not add guest composition logic. |
| `crates/nimbus-machine/src/api.rs` | 1,033 | Export the child wire module only. |
| `crates/nimbus-cli/src/machine/api/service_workloads.rs` | 521 | Keep this as facade and composition. Put exact guest state and dispatch in a child. |

Small capability traits, ports-and-adapters, explicit state machines, durable
command journals, request/response fencing, and inspect-before-retry are the
canonical patterns. The implementation must not merge those seams into a
`NetworkProvider` or `MachineTeardownProvider` god interface.

## Fail-Before Baseline

All checks ran at clean `c1c7f1397` before a product edit.

| Check | Exit | Expected-red result |
| --- | ---: | --- |
| `test -f crates/nimbus-cli/src/machine/backend/teardown.rs` | 1 | No parent exact teardown adapter exists. |
| `test -f crates/nimbus-cli/src/machine/api/service_workloads/teardown.rs` | 1 | No guest exact phase sink exists. |
| `test -f crates/nimbus-machine/src/api/teardown.rs` | 1 | No strict teardown wire vocabulary exists. |
| `rg -q ForwardedMachineTeardownAdapter crates/nimbus-cli/src crates/nimbus-compute/src` | 1 | No real five-capability substitution exists. |
| `rg -q MachineApiWorkloadTeardownCommandEnvelope crates/nimbus-machine/src crates/nimbus-cli/src` | 1 | No exact remote command envelope exists. |
| `rg -q MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH crates/nimbus-machine/src crates/nimbus-cli/src` | 1 | No private teardown phase path exists. |
| `rg -q MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION crates/nimbus-machine/src crates/nimbus-cli/src` | 1 | Capability reporting cannot name exact teardown. |
| `rg -q stop_service_sandbox crates/nimbus-cli/src/machine/backend.rs` | 0 | The coarse product backend still calls the coarse guest stop as expected for later cutover. |

Source inspection supplies the remaining fail-before evidence:

- `ConfirmedWorkloadTeardownCommand` has no prior receipt prefix.
- parent retirement calls guest stop before parent withdrawal.
- the coarse request contains only forwarder authority.
- guest stop recombines Systemd and Container stop plus network cleanup.
- production Systemd composition has `teardown_store: None`.
- the host-managed Container attachment adapter rejects forwarded composition.
- confirmed parent retirement has only a Boolean terminal state.
- final `WithdrawPublication` is currently classified as a restart-family
  provider command and cannot accept a teardown-domain claim.
- the static source contract lacks the exact d4 seams.

Focused implementation tests must encode these red observable boundaries before
the related behavior changes. A fail-before rejection must preserve durable
bytes and make zero provider effect.

## Implementation Bands

These bands are dependency ordered inside one canonical item. They are not
review units. The canonical ledger records no partial band completion.

1. Add the validated portable receipt-prefix contract. Carry it through
   confirmed command and result authentication. Add deterministic crossed,
   missing, stale, and out-of-order tests.
2. Add one teardown-family final-withdraw provider operation and extract the
   narrow confirmed-teardown journal seam. Prove that restart withdrawal stays
   unchanged. Prove that all five teardown streams use correct claim rules.
3. Add strict Machine API teardown wire vocabulary, complete digest, closed
   outcomes, response correlation, and pure wire tests.
4. Add durable Systemd production composition and retain the exact lifecycle,
   drain, and stop trait objects. Add capability and unavailable-mode tests.
5. Add the guest composite execution drain/stop state machine and exact phase
   sink. Reuse the Container journal and node/Container progress stores. Add
   replay, contention, and fresh-process cuts.
6. Add the forwarded Container attachment composition. Prove receipt-prefix
   order, guest machine-publication absence, retained detach, final release,
   and no regression to NNC6.5d3.
7. Add the private route and client child. Prove strict fail-before request and
   response correlation and exact Inspect-before-retry after ambiguity.
8. Add parent publication progression and `ForwardedMachineTeardownAdapter`.
   Implement all five real capabilities and complete-batch port fencing.
9. Add real compute registry substitution, two-realm crash/concurrency tests,
   sibling and empty-batch matrices, and source-derived verifier cases.
10. Run every item gate and freeze the complete candidate. Run one full item
   review. Resolve accepted findings under the frozen cadence, update the
   ledger, and commit the exact completed item.

## Acceptance Commands

The final evidence ledger records exact counts and declared platform or child
process ignores.

```sh
cargo test -p nimbus-workloads teardown -- --test-threads=1
cargo test -p nimbus-machine workload_teardown_phase_wire -- --test-threads=1
cargo test -p nimbus-node teardown -- --test-threads=1
cargo test -p nimbus-sandbox provider_command -- --test-threads=1
cargo test -p nimbus-sandbox container_teardown -- --test-threads=1
cargo test -p nimbus-sandbox attachment_lifecycle -- --test-threads=1
cargo test -p nimbus-cli machine_api_workload_teardown -- --test-threads=1
cargo test -p nimbus-cli guest_workload_teardown -- --test-threads=1
cargo test -p nimbus-cli forwarded_machine_teardown -- --test-threads=1
cargo test -p nimbus-network port_lease::lifetime -- --test-threads=1
cargo test -p nimbus-compute teardown_sandbox -- --test-threads=1
cargo test -p nimbus-compute forwarded_machine_teardown -- --test-threads=1
cargo test -p nimbus-workloads
cargo test -p nimbus-machine
cargo test -p nimbus-node
cargo test -p nimbus-sandbox
cargo test -p nimbus-cli
cargo test -p nimbus-compute
cargo clippy -p nimbus-workloads -p nimbus-machine -p nimbus-node -p nimbus-network -p nimbus-sandbox -p nimbus-cli -p nimbus-compute --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-workloads -p nimbus-machine -p nimbus-node -p nimbus-network -p nimbus-sandbox -p nimbus-cli -p nimbus-compute --no-deps
cargo fmt --all --check
git diff --check
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Closeout also runs the network dependency/effect scan and changed-file
modularity census. It runs source-derived inventories, ledger bijection,
NNCV035 arithmetic, proof lint, and technical-writing lint. Structured
autoreview is forbidden until K1-K34 are green and the whole item is candidate
frozen.

## Static Seam Checklist

- [ ] `nimbus-network` still has only the `nimbus-core` workspace edge.
- [ ] No socket, HTTP route, provider effect, machine DTO, or workload receipt
      enters `nimbus-network`.
- [ ] Compute is the only saga coordinator and result-CAS owner.
- [ ] Exactly one parent provider journal exists for forwarded teardown.
- [ ] Exactly one guest provider journal exists for Container guest effects.
- [ ] Parent publication state and command-result state are not duplicated.
- [ ] Parent and guest durable roots are independent and inaccessible across
      the Machine API boundary.
- [ ] Attachment, execution, and ingress provider IDs are pairwise distinct.
- [ ] Every request and response binds stable tenant-qualified identity and
      generation/epoch fences.
- [ ] No IP address, port, path, PID, unit name, or provider handle selects a
      workload.
- [ ] Parent publication is absent before guest drain or stop.
- [ ] The guest drains execution before stop and stops it before detach.
- [ ] The guest detaches its attachment before guest and parent release.
- [ ] Complete parent port authority stays retained through detach.
- [ ] Partial or unknown sibling state releases no port.
- [ ] Guest Execute persists claim and may-exist evidence before effects.
- [ ] Parent ambiguity causes exact guest Inspect before retry.
- [ ] Inspect cannot write, effect, repair, release, or claim a new Execute.
- [ ] Coarse stop remains only for later callers and is not used by the new
      adapter.
- [ ] NNCV035 remains the only planned red aggregate condition.
- [ ] The item has one review unit and one exact completion commit.

## Retained Later Owners And Non-Goals

- NNC6.5e owns native service, sandbox, and definition caller cutover.
- NNC6.5f owns Compose, forwarded composition callers, and physical-machine
  workload-stop boundaries.
- NNC6.5g owns failed-provision compensation, tenant retirement, coarse-stop
  deletion, and final NNCV035 convergence.
- NNC6.6 owns service-resolution fencing during withdrawal.
- NNC6.1e2 owns final startup recovery and tenant-retirement convergence.
- NNC8.3 owns orphan-cleanup finalization and capacity reuse.

Logical service names stay in `nimbus-services`. Tenant admission stays in
`nimbus-tenant`. Proxy forwarding and policy stay in their current PDP/PEP
owners. Machine provider capability semantics stay explicit. Cluster
membership, transport, routing, and super-net fencing remain separate.

## Evidence Ledger

| Evidence | Current result |
| --- | --- |
| Source base and owner worktree | `c1c7f1397`. clean before the audit and fail-before capture. |
| Parent audit | Current guest-first retirement order, exact provider identities, confirmed publication state, complete-batch port authority, and parent recovery gaps recorded. Zero changed paths and zero tests. |
| Guest and wire audit | Missing envelope/route/client/facade, exact restart exemplar, unavailable Systemd teardown store, erased exact traits, and composite guest execution risks recorded. Zero changed paths and zero tests. |
| Compute and shared audit | Receipt-prefix transport prerequisite, five-capability registry substitution, two-realm recovery, forwarded attachment composition, and static-verifier requirements recorded. Zero changed paths and zero tests. |
| Complexity census | Audit line counts and concept-owned dispositions are frozen above. |
| Fail-before | Seven absent seam checks exit `1`. the retained coarse-stop check exits `0`. product source unchanged. |
| Audit checkpoint verification | Format and diff pass. Technical-writing lint reports zero diagnostics. NNCV008 passes. NNCV035 self-test is `55/55`; direct remains exact expected red at `0/7`; aggregate is `35/36` with only NNCV035 red. Docs pass `108`; site passes `17/17`. |
| Structured review | Not run. Audit and fail-before are partial item work. One review is allowed only after K1-K34 are green. |
| Durable audit checkpoint | The commit containing this proof, plan recovery header, and routing index is the exact NNC6.5d4 audit/fail-before checkpoint. It is not the item completion commit. |

## Current Acceptance State

K1-K2 path and ownership audit evidence is complete. The item has not run
K3-K35. Those criteria remain frozen and red. The audit changed no product
source. The next action is
implementation band 1: add and prove the exact prior receipt-prefix contract,
then carry it through confirmed command and result authentication. NNC6.5d4
remains the sole `in_progress` canonical item. There is no blocker.
