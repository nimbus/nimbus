# NNC6.5f Compose and machine caller substitution audit

Status: `complete; read-only audit and prospective split`

This record owns the read-only NNC6.5f audit. It freezes the remaining Compose,
guest, forwarded-machine, and physical-machine caller seams before product
edits. The audit converts the former omnibus NNC6.5f implementation into three
acceptance-bearing implementation items.

## Objective

NNC6.5f must prove that the next items can replace every remaining Compose and
machine teardown bypass. The items must not create a second saga, provider
journal, port authority, or machine policy owner.

The target remains:

```text
local or forwarded Compose down
  -> one Engine persistence root
  -> EngineWorkloadSagaStore
  -> compute resource retirement facade
  -> exact five-step teardown runtime
  -> provider-owned local or forwarded effects

physical machine stop
  -> compute-owned stop policy
  -> provider-owned durable machine-wide admission barrier under its lock
  -> canonical Engine desired-authority scan after barrier persistence
  -> typed conflict, or machine-owned stop effects

machine workload desire
  -> provider-owned admission guard authenticates barrier absence
  -> Engine desired-intent CAS while the same guard remains held
  -> durable desire, or fail-before without an Engine write
```

## Non-goals

This audit and its three child items do not own:

- failed-provision or failed-restart compensation.
- tenant deletion or final startup-recovery convergence.
- services policy, logical naming, or resolver fencing.
- final removal of unrelated coarse sandbox authority.
- proxy forwarding, egress policy, or certificate ownership.
- cluster membership or transport.
- any socket, process, Machine API, Netavark, nftables, gvproxy, or provider
  effect inside `nimbus-network`.
- a CLI-local workload-saga store or journal.

NNC6.5g retains compensation, tenant retirement, remaining legacy deletion,
and final NNCV035 convergence. NNC6.6 retains service-resolution fencing.

## Acceptance contract

| ID | Verifiable success criterion |
| --- | --- |
| A1 | A source-derived census names every local and forwarded Compose down caller, exact provider-composition root, guest coarse and exact route, physical-machine stop entry point, durable store, provider journal, and port authority in scope. |
| A2 | Current and target call graphs identify the exact owner of policy, durability, coordination, provider effects, and projections. |
| A3 | The audit proves the former NNC6.5f value has three independent failure surfaces and prospectively creates NNC6.5f1-NNC6.5f3 before product edits. |
| A4 | NNC6.5f1 freezes one forwarded composition path that consumes the already-earned exact five-capability registry and one foreground retirement facade. |
| A5 | NNC6.5f2 freezes one Engine-backed local and forwarded Compose down path with no direct backend stop or CLI-local durability. |
| A6 | NNC6.5f2 assigns the coarse guest stop route, client, capability advertisement, and caller removal to the same atomic cutover. |
| A7 | NNC6.5f3 freezes one cross-owner protocol: compute owns stop policy; the provider owns durable barrier persistence and admission authentication; every initial or restart Engine desire CAS holds the same provider admission guard. |
| A8 | NNC6.5f3 returns a typed active-workload conflict before listener, publication, port, process, VMM, or state effects. |
| A9 | The audit preserves the parent and guest durable realms, request-before-send, Inspect-before-retry, complete sibling-batch fencing, and provider-local effect ownership proven by NNC6.5d4. |
| A10 | Stable tenant-qualified workload identity, source and execution generations, attempt, epoch, provider identity, and forwarder generation authenticate every target seam. No IP address, port, PID, unit, path, or provider handle becomes workload identity. |
| A11 | The failure table covers missing, stale, crossed, ambiguous, replayed, concurrent, and fresh-process outcomes with exact byte-preservation and effect bounds. |
| A12 | The behavioral proof matrix covers local Compose, forwarded Compose, guest exact dispatch, physical-machine conflict, concurrent admission, and restart recovery. |
| A13 | Static mutations fail if Compose loses the Engine store, a forwarded composition omits exact teardown registration, a coarse guest stop survives its deletion gate, or machine stop performs an effect before its barrier. |
| A14 | The audit records concept-owned path boundaries and file-complexity decisions for each child item. |
| A15 | `nimbus-network -> nimbus-core` remains its only workspace edge. No audited change moves a provider effect into `nimbus-network`. |
| A16 | The plan, item ledger, recovery header, and routing index contain the same unique item set with exactly one `in_progress` item. |
| A17 | The source-derived teardown helper and aggregate mutation suites pass with the frozen new cases. The live aggregate remains expected red only for unimplemented later owners. |
| A18 | Format, diff, proof lint, docs, site, dependency, and effect scans pass. |
| A19 | One candidate-frozen GPT-5.6 Sol/xhigh/fast item review is fully dispositioned. A narrow review runs only after an accepted executable correction. |
| A20 | This audit changes no product source. Its exact proof, static contract, ledger, and routing close together in one durable item commit. |

## Current ownership census

| Concern | Current source and behavior | Canonical target |
| --- | --- | --- |
| Compose command routing | `crates/nimbus-cli/src/compose/mod.rs:59-78` receives `EnginePersistenceConfig`, but passes it only to Compose up. `run_compose_down` at lines 228-248 opens no Engine. | Pass the same persistence config to down, open one Engine, and use its `EngineWorkloadSagaStore`. |
| Compose up durability | `compose/mod.rs:195-224` opens the Engine before `ComposeForegroundOwner`. `compose/lifecycle.rs:98-104` creates `EngineWorkloadSagaStore`. | Reuse this durability seam for down. Do not copy its implementation into a second owner. |
| Compose down | `compose/lifecycle.rs:287-399` resolves provider snapshots, calls `SandboxBackend::stop`, inspects again, and derives output from provider state. | Resolve desired service identities, then submit retirement through compute and derive output from durable retirement truth. |
| Local provider composition | `network_composition.rs:360-408` already registers exact Krun execution, attachment, ingress, restart, and teardown capabilities. | Preserve this composition. Add no second local registry. |
| Forwarded provider composition | `network_composition/forwarded.rs:90-146` and `compose/provision.rs:168-225` register provision and restart only. The two roots duplicate forwarded activation. | Use one forwarded composition root. Consume `ForwardedMachineTeardownRegistrations` into one exact registry and pass it to server composition. |
| Foreground runtime facade | `nimbus-server/src/workload_composition.rs:184-220` retains compute but exposes only `ComputeResourceProvisioner`. | Expose the already-owned `ComputeResourceRetirer` from the same runtime. Do not expose raw coordinator, store, or registry authority. |
| Parent forwarded adapter | `machine/backend.rs:69-110` already creates and retains `ForwardedMachineTeardownAdapter`, but product composition does not consume its registrations. | Move the exact registrations into compute composition without reopening their journals or provider facts. |
| Coarse parent stop | `machine/backend.rs:112-137` sends `stop_service_sandbox` before parent publication retirement. `SandboxBackend::stop` at lines 220-224 exposes that path. | Remove the product caller and coarse remote envelope during Compose cutover. Retain only read-only inspection until the remaining trait is deleted in NNC6.5g. |
| Coarse guest route | `machine/api/routes.rs:82-85,334-365`, `machine/client.rs:227-247`, and `nimbus-machine/src/api.rs:76-100` expose `service-sandboxes.stop`. | Delete the route, client verb, wire types, and capability advertisement after exact phase dispatch owns every caller. |
| Exact guest route | `machine/api/service_workloads.rs:126-142,392-398` and its teardown child dispatch one authenticated exact phase. | Preserve this route as the only remote teardown effect ingress. |
| Parent provider journal | `machine/backend/teardown.rs` reuses `ProviderCommandAttemptJournal` and the confirmed parent publication journal. | Keep both authorities. Do not create a Compose journal. |
| Physical-machine stop | `machine/manager/stop.rs:28-146` withdraws parent publication and SSH authority before stopping the VMM and helpers. It does not fence new Engine desire or forwarded provider admission. | Compute owns the stop decision. It first asks the provider barrier capability to persist a machine-wide drain barrier under the provider lock, then scans canonical Engine authority. It returns a typed conflict before effects if exact nonterminal workload authority exists. |
| Durable workload desire | `nimbus-workloads` records exact execution-provider identity in each desired source. The Engine store can enumerate recovery-eligible records, while parent publication witnesses cover provider-backed quiescent executions. | Compute owns the union and the stop decision. No provider or projection store can replace saga truth. |
| Durable forwarded workload witness | `machine/publication_authority/confirmed.rs` stores tenant-qualified workload retirement witnesses, exact forwarder authority, publication progress, and teardown evidence under one process-safe lock. It begins only at the first confirmed provider command. | Extend this provider owner with machine-wide barrier persistence, the process-safe guard, and admission authentication only. Compute retains all policy and desired-state decisions. Engine desire CAS and provider-command admission must authenticate the barrier under this same lock. |
| Machine effects | `machine/manager/stop.rs` owns VMM, helper-process, machine-publication, and SSH-listener effects. | Keep these effects machine-owned after the barrier authorizes them. |

## Load-bearing audit findings

1. Compose command routing drops the supplied `EnginePersistenceConfig` for
   down. Up opens an Engine. Down prepares only attachment authority.
2. Local and forwarded down both enter `stop_service_target` and then the
   coarse `SandboxBackend::stop` path. The forwarded branch constructs a
   backend without retained provision authority before it sends the coarse
   guest stop.
3. Local exact teardown composition is complete and already passes through
   `with_teardown_capabilities`. Compose down bypasses it through
   attachment-only preparation.
4. Exact forwarded attachment, execution, and ingress registrations already
   exist and are proven. Both forwarded product composition roots omit them.
5. `PreparedForwardedServerWorkload` and
   `PreparedComposeProvision::Forwarded` duplicate activation and provider
   realm construction. NNC6.5f1 must remove this duplication, not add a third
   path.
6. Provider-manifest and guest-list discovery cannot be the durable recovery
   source. A completed or partially retired provider can remove those views
   before a fresh Compose process reopens the workload saga.
7. `SandboxServiceRetirementOutcome` currently contains a definition and an
   optional process-local handle. That shape cannot report a stable terminal
   execution identity after a fresh-process replay. NNC6.5f2 must derive a
   truthful recorded identity and disposition from durable saga state.
8. Physical stop calls provider effects before any complete workload check.
   The confirmed publication journal starts at the first provider command.
   Thus, it provides evidence and can own the admission fence. It is not
   complete desired authority.
9. Direct CLI stop, server stop, restart, bootc restart, and OS-apply restart
   reach the same raw physical stop effect. No caller has a workload fence.
   Every entry point must share one guarded seam.
10. The current Compose and physical-stop NNCV035 checks require invented
    marker names. The child items must satisfy semantic source dataflow,
    ordering, absence, and attributed-test checks. Product code must not add
    marker functions to satisfy the verifier.

## Complexity findings

The original item combines three different concepts:

1. provider-composition validation and registry substitution.
2. user-facing Compose lifecycle orchestration and durable outcome mapping.
3. machine-wide admission fencing before physical provider effects.

They do not share one state machine, one failure surface, one path owner, or
one focused test harness. Reviewing them as one diff would make the review
chunking define the unit of value. The plan therefore splits them before any
product edit.

The audit also identifies one direct duplication pocket. Both
`PreparedForwardedServerWorkload` and `PreparedComposeProvision::Forwarded`
activate the same machine source and rebuild the same server provider realm.
NNC6.5f1 must make one prepared forwarded profile canonical. It must not add a
third helper or compatibility path.

## Target ownership and dataflow

### Forwarded composition

```text
PreparedDefaultMachineProvisionSource
  -> one activated client + ForwardedMachineProvisionAdapter
  -> one ForwardedMachineApiSandboxBackend
  -> retained ForwardedMachineTeardownAdapter
  -> exact attachment + execution + ingress teardown registrations
  -> WorkloadTeardownCapabilityRegistry
  -> ServerWorkloadProviders::with_teardown_capabilities
  -> ExactWorkloadTeardownCapabilityRealm
  -> ComputeState
```

The provider selection and execution-provider identity must match at every
edge. A crossed realm fails before source, saga, journal, Machine API, port, or
provider effects.

### Compose down

```text
ComposeDownCommand
  -> resolved project and requested service identities
  -> Engine::new_with_persistence_config
  -> EngineWorkloadSagaStore
  -> one complete local or forwarded workload composition
  -> ComputeResourceRetirer::submit_service_teardown
  -> durable outcome mapping
  -> Engine::quiesce before return
```

Compose may read source and observed projections. It cannot infer teardown
success from provider inspection or use a raw sandbox handle as authority.
Provider activation cannot substitute another Engine root, network root,
provider selection, execution provider, or forwarder generation.

### Physical-machine stop

```text
authenticated machine lock
  -> compute requests one provider-owned admission guard
  -> under the confirmed-publication lock, authenticate the machine incarnation,
     check provider-backed witnesses, and persist the exact
     forwarder-generation drain barrier
  -> release the provider lock
  -> compute scans canonical Engine durability
     -> matching desire or provider witness exists: typed conflict, zero effect
     -> exact union is empty: barrier persists
  -> withdraw legacy machine publication and SSH listener
  -> stop VMM and helpers
  -> retain or release authority from exact observed absence
  -> record machine state
```

Initial Engine workload desire and restart admission use the provider admission
guard. Each path authenticates barrier absence and holds the same process-safe
provider lock through the Engine CAS. Every forwarded provision, restart, and
publication admission rejects the barrier under that lock before its first
journal or effect boundary. The post-barrier Engine scan then closes the
intent-versus-stop gap without a cross-store transaction.

There are only two legal orderings. If desire enters the guard first, its
Engine CAS completes before stop can persist the barrier, and the later Engine
scan observes the desire. If stop persists the barrier first, a later desire
guard rejects before any Engine CAS. If a process dies while it holds the
guard, the operating system releases the lock. A committed CAS remains visible
to the scan. An uncommitted CAS created no desire.

The post-barrier scan can find active desire. Compute then asks the provider to
clear only the same unchanged and effect-free barrier under the provider lock.
Compute returns the typed conflict. Unavailable, ambiguous, corrupt, stale, or
crossed authority retains the barrier and fails closed. A snapshot-only
preflight is not sufficient.

The confirmed publication journal cannot prove complete desire by itself. It
starts at the first provider command and would become a second desired-state
authority if physical stop treated it as complete. The compute decision must
join canonical Engine records with provider-backed witnesses. A caller without
the canonical Engine/store authority fails closed before machine effects.

The provider capability owns only the lock, barrier record, persistence, and
admission authentication. Compute owns the stop policy, Engine scan, evidence
join, classification, and clear-versus-retain decision. The barrier stays
fenced after an ambiguous or failed machine stop. A later machine generation
cannot reuse it without an exact generation transition.

## Prospective implementation split

| Item | Value and dependency | Owned product paths | Forbidden paths |
| --- | --- | --- | --- |
| NNC6.5f1 | After NNC6.5f, install one canonical forwarded teardown composition and expose one foreground retirement facade. | `nimbus-cli` forwarded network/Compose preparation, `machine/backend.rs` narrow registry access, `nimbus-server/workload_composition.rs`, focused composition tests. | Compose down caller behavior, coarse guest route deletion, physical-machine stop, compensation, tenant deletion, `nimbus-network` effects. |
| NNC6.5f2 | After NNC6.5f1, replace local and forwarded Compose down with the Engine-backed compute retirement facade and remove the coarse remote stop envelope. | `compose/mod.rs`, `compose/lifecycle.rs`, concept-owned Compose tests, narrow machine client/route/facade/wire capability deletion, source-derived NNCV035 checks. | Physical-machine stop, compensation, tenant deletion, unrelated sandbox authority deletion, service naming or policy. |
| NNC6.5f3 | After NNC6.5f, add the compute-owned desired-authority decision, provider-owned durable machine-wide admission guard/barrier, and typed active-workload conflict before physical stop. | neutral workloads/compute decision and store queries, both Engine desire-admission bodies, server Engine adapter, confirmed machine publication barrier/guard and tests, forwarded provision/restart/publication admissions, machine manager stop and every physical caller, source-derived NNCV035 checks. | Compose behavior, guest phase semantics, workload compensation, tenant deletion, VM effects outside the existing machine owner, provider-owned stop policy, or a machine-owned desired-state registry. |

NNC6.5g depends on NNC6.5f2 and NNC6.5f3. It deletes every remaining legacy
authority after all producers move.

## Child-item acceptance criteria

### NNC6.5f1 forwarded composition

- one source prepares local and forwarded foreground/server profiles.
- the exact retained forwarded adapter supplies all five capability roles.
- `WorkloadTeardownCapabilityRegistry::new` authenticates the three provider
  groups before server composition.
- `ServerWorkloadProviders::with_teardown_capabilities` receives that exact
  registry.
- the foreground runtime exposes `ComputeResourceRetirer`, not raw store or
  dispatcher authority.
- local and forwarded real-substitution tests dispatch each exact provider
  once. They reject crossed selection or execution identity before effects.
- no provider journal, Machine API client, or network manager opens twice.

### NNC6.5f2 Compose cutover

- `run_compose_down` receives the same `EnginePersistenceConfig` as up.
- down opens one Engine and one `EngineWorkloadSagaStore` before compute
  retirement.
- selected and all-service modes submit each stable service identity once.
- replay returns a stable idempotent outcome from durable truth.
- a `Recorded` result exposes stable terminal execution identity and terminal
  disposition even when no process-local sandbox handle survives.
- missing, unstarted, stopped, stopping, cleanup-pending, and ambiguous cases
  return exact typed outcomes without fabricated success.
- local and forwarded paths use the same retirement facade.
- crossed tenant, source generation, execution, plan, provider, or forwarder
  evidence fails before provider effects.
- a fresh process reopens the same Engine root and resumes the exact record.
- process cancellation and lost provider response preserve inspect-before-retry.
- no production Compose code calls `SandboxBackend::stop`.
- no coarse `service-sandboxes.stop` route, client, request, response, or
  capability advertisement remains.
- Engine quiescence and provider/network lifetime settlement complete before
  the CLI returns.

### NNC6.5f3 physical-machine barrier

- one compute-owned decision joins matching canonical Engine records with
  provider-backed witnesses for the exact execution provider and machine
  incarnation.
- one strict journal record binds provider instance, forwarder generation,
  barrier epoch, state, and checksum.
- the provider-witness check and barrier claim are atomic under the existing
  confirmed-publication lock.
- initial workload desire and restart admission use the provider admission
  guard.
- each path authenticates barrier absence and holds the same lock through the
  Engine CAS and its result.
- stop persists the barrier under that lock before its canonical Engine scan.
  The scan closes intent-versus-stop admission races without a cross-store
  transaction.
- any active, retiring, ambiguous, partial, or corrupt authority returns a
  typed conflict. Unavailable, stale, or crossed authority does the same. Each
  result makes zero machine or network effect.
- every forwarded provision, restart, and publication admission rechecks the
  barrier before its first durable request/effect boundary.
- a caller without the canonical Engine/store decision authority fails closed.
- a concurrent admission and stop produce exactly one winner.
- active Engine desire clears only the unchanged effect-free barrier under the
  provider lock. Unavailable or ambiguous authority retains the barrier.
- a process death after barrier persistence and before VMM stop reopens fenced
  and cannot admit a workload.
- a failed or ambiguous stop retains the barrier and all unresolved authority.
- exact terminal provider absence permits the existing machine owner to
  settle its publication and SSH authority.
- a later machine generation needs an explicit exact barrier transition. It
  cannot overwrite a stale or unresolved generation.

## Failure and recovery table

| Case | Required result |
| --- | --- |
| Compose cannot open Engine durability | Return store failure before provider teardown or projection mutation. |
| Compose source or requested service is missing | Return the existing typed input or not-found result with no saga or provider effect. |
| Exact saga is absent and source is unstarted | Finalize the source stop through compute. Do not fabricate an execution identity. |
| Exact saga is already `Recorded` | Return stable replay outcome with no provider effect. |
| Saga, source generation, plan, selection, or execution identity is crossed | Reject before provider journal or Machine API bytes. Preserve every durable realm. |
| Forwarded provider realm is incomplete | Composition fails before runtime construction. No fallback to coarse stop. |
| Execute response is lost | Persist ambiguity, retain all parent and guest authority, then Inspect the exact command. |
| Guest has a partial sibling outcome | Return in-progress or ambiguous. Release no parent port. |
| Compose process dies after command claim | A fresh process reopens Engine durability and adopts or inspects the same attempt. |
| Physical stop sees an active workload witness | Return typed active-workload conflict before listener, port, VMM, helper, or state effects. |
| Physical stop cannot enumerate canonical desired authority after barrier persistence | Retain the barrier and return typed authority-unavailable conflict before any machine effect. Missing desired evidence is never treated as absence. |
| Initial or restart Engine desire and physical stop contend | Both use the provider admission guard. Desire-first holds the lock through its Engine CAS, so the later stop scan sees it. Barrier-first makes the later desire fail before its Engine CAS. |
| Process dies while a desire guard is held | The operating-system lock releases. If the Engine CAS committed, the stop scan sees it. If the CAS did not commit, no desired authority exists. |
| Active desire is found after the barrier | Compute requests conditional clearing of only the unchanged effect-free barrier under the provider lock, then returns typed conflict. |
| Process dies after machine barrier persistence | Reopen with admission still fenced. Inspect machine/provider state before any retry or release. |
| Machine stop is ambiguous or incomplete | Retain the barrier, workload witnesses, publications, ports, and provider evidence. |
| Barrier or witness storage is corrupt | Fail closed. Do not treat missing or unreadable evidence as absence. |
| Later machine generation crosses an unresolved barrier | Reject before machine start or forwarded workload admission. |

## Behavioral and static proof matrix

| Proof family | Required evidence |
| --- | --- |
| Forwarded composition | Local and forwarded profile tests prove exact registry substitution, same-realm identity, one journal open, and crossed fail-before behavior. |
| Compose happy path | Local and forwarded selected-service tests observe exact withdraw, drain, stop, detach, release, record order. |
| Compose all-services | Deterministic service ordering and one exact submission per stable identity. |
| Compose replay | Same process and fresh process return the same durable terminal result with zero duplicate provider effect. |
| Compose failures | Missing store, source, capability, provider, and ambiguous result never call the coarse backend or fabricate `Stopped`. |
| Guest transport | Only exact teardown phase requests remain. Crossed requests and responses fail before guest effects or parent release. |
| Physical conflict | Active and ambiguous witnesses return typed conflict before every machine-effect log. |
| Physical contention | Thread and process contenders prove that both Engine desire CAS paths hold the provider guard through commit and that stop waits for an in-flight commit before it can persist the barrier and scan. Exactly one admission-or-drain ordering wins. |
| Physical crash | Subprocess cuts after barrier, publication withdrawal, provider request, and observed absence reopen to exact fenced state. |
| Static dataflow | Source checks require persistence config to reach down; the exact Engine store to reach the activated retirement runtime; stable terminal identity to reach the returned Compose outcome; one canonical forwarded registry to reach both consumers; guest validation and journal claim to precede remote I/O; both Engine desire CAS operations to remain inside the provider guard; every provider admission to authenticate the barrier; and all five physical callers to reach authorization before effects. |
| Static negative cases | Mutations remove each required edge, disconnect the Engine store, discard terminal identity, duplicate or bypass forwarded composition, skip guest validation, send before journal claim, retain coarse route/wire/capability artifacts, release a desire guard before CAS, bypass provision/restart/publication admission, move the barrier after an effect, bypass any physical caller, introduce a CLI store, or add a network-crate effect. Every mutation fails only its assigned contract. |

### Exact Compose behavior roster

NNC6.5f2 owns these nine attributed tests:

1. `compose_down_local_uses_engine_saga_and_compute_teardown`
2. `compose_down_forwarded_uses_engine_saga_and_exact_machine_phases`
3. `compose_down_unresolved_submission_makes_zero_provider_calls`
4. `compose_down_replay_is_idempotent_and_reports_durable_outcome`
5. `compose_down_crossed_or_stale_identity_fails_before_provider_effects`
6. `compose_down_ambiguous_result_reopens_with_inspection_only`
7. `compose_down_process_reopen_resumes_same_attempt_without_duplicate_effect`
8. `compose_down_partial_sibling_failure_preserves_completed_and_unissued_services`
9. `compose_down_cancellation_after_submission_is_replayable`

The assertions must prove the five exact provider phases followed by
`Recorded`. Each exact phase gets one Execute. Five claims and five results
produce at least ten saga CAS operations.

An ambiguous result reuses the same attempt in Inspect mode. Confirmed
durability precedes every provider command.
A partial sibling failure has exact per-service counts. Fresh-process proof
reopens the same Engine and provider-state roots in a subprocess.

### Exact physical-machine behavior roster

NNC6.5f3 owns these fourteen attributed tests:

1. `machine_stop_rejects_active_workload_saga_authority`
2. `standalone_machine_stop_fails_closed_without_engine_drain_authority`
3. `machine_stop_exact_empty_fence_precedes_publication_and_vmm_effects`
4. `machine_stop_active_authority_makes_zero_publication_ssh_vmm_or_state_effects`
5. `machine_stop_stale_or_crossed_machine_generation_makes_zero_effects`
6. `machine_stop_ambiguous_unavailable_or_corrupt_authority_fails_closed`
7. `machine_stop_reopen_rediscovers_active_durable_authority`
8. `machine_stop_and_concurrent_admission_linearize_at_one_fence`
9. `machine_workload_desire_commit_holds_admission_guard_through_engine_cas`
10. `machine_stop_barrier_waits_for_inflight_engine_desire_commit`
11. `machine_restart_cannot_bypass_active_workload_fence`
12. `machine_os_restart_cannot_bypass_active_workload_fence`
13. `stopped_machine_with_active_durable_authority_returns_typed_conflict`
14. `machine_stop_ignores_observed_projection_and_address_identity`

The contention result is binary. Admission can win while it holds the provider
guard through the Engine CAS. The later stop scan then returns the typed
conflict. The stop barrier can also win. Later desire admission then fails
before its Engine CAS. The proof uses thread and two-process contenders and
crash cuts before each physical effect.

### Frozen semantic verifier mutations

The green fixture and source scanner must cover the real seams, not marker
functions. NNC6.5f freezes these later-owner mutations:

- Compose down omits the persistence config.
- The retirement flow omits Engine or `EngineWorkloadSagaStore`.
- The activated retirement runtime uses an unrelated Engine store.
- The flow discards the correctly activated runtime and gets the retirer from
  a second activation.
- The flow omits `resource_retirer` or `submit_service_teardown`.
- The flow discards the terminal execution reference or omits its exact bound
  value from the returned durable `Recorded` result.
- The canonical forwarded composition omits exact teardown registration.
- Either forwarded consumer bypasses or discards the canonical result.
- Guest dispatch skips validation or discards the validated value.
- Guest execute sends a known direct or aliased remote effect before its
  journal claim.
- A Compose direct backend stop survives NNC6.5f2.
- A coarse guest route, operation, path, request, response, response wire, or
  capability survives NNC6.5f2.
- Physical stop claims its barrier after a machine or network effect.
- The required authentication and effect order exists only outside the exact
  `stop_machine` body.
- Active authority returns an untyped error or permits an effect.
- Observed projection, address, or provider handle substitutes for identity.
- Unavailable, ambiguous, corrupt, stale, or crossed authority permits stop.
- Direct CLI stop, server stop, restart, bootc restart, or OS-apply restart
  bypasses the gate.
- Initial or restart desire omits the provider admission guard or releases it
  before the Engine CAS.
- The guard invokes the CAS closure outside the locked mutation.
- Barrier claim or admission authentication occurs outside the provider lock.
- Forwarded provision, restart, or publication admission bypasses barrier
  authentication or crosses any required journal/effect boundary before it.
- Stop policy moves out of compute, or barrier persistence/authentication
  definitions move out of the confirmed-publication provider owner.
- Stop scans Engine before barrier persistence or omits its post-barrier scan.
- Stop fails to conditionally clear an unchanged effect-free barrier after
  active desire wins.

Each of the `138` fixture mutations must produce exactly one assigned
diagnostic. The live
NNCV035 contract becomes the exact expected `0/7`: the stronger audit exposes
the two omitted forwarded composition roots as their own later-owner group.
The aggregate still has only NNCV035 red. No audit-only marker may make a
product diagnostic green.

## Complexity and module boundaries

Current handwritten source sizes include:

- `network_composition.rs`: `1,547` lines. Production logic ends before the
  inline tests. Keep its existing composition-root exception and move new
  forwarded behavior to `network_composition/forwarded.rs`.
- `machine/publication_authority/confirmed.rs`: `1,721` lines. It is a deep
  provider-evidence owner. Put the new machine drain barrier in a concept child
  and keep its invariants next to the envelope validation. Do not add Engine
  enumeration or desired-state decisions to this file.
- `machine/handlers.rs`: `886` lines. Keep handlers thin.
- `machine/manager/stop.rs`: `604` lines. Keep provider effects here. Compute
  owns barrier policy and the stop decision. A confirmed-publication concept
  child owns only the provider lock, durable barrier persistence, conditional
  clearing, and admission authentication.
- `compose/mod.rs`: `669` lines and `compose/lifecycle.rs`: `474` lines. Put
  durable Compose retirement behavior in a concept-owned child when needed.
  Do not turn either root into a switchboard.
- `nimbus-server/workload_composition.rs`: `427` lines. Add only the narrow
  retirement facade that its retained `ComputeState` already owns.

No child item may grow a second provider composition root or a generic
`MachineTeardownProvider` interface.

## Structured review disposition

The one full item review ran against staged tree
`ab5fe42ade24b1d9a14128d45998279274368e36` and staged patch SHA-256
`447827338ea5415ead384b6eda93d881bdc8ac58f0bb553debad7bf03e1ea71c`.
The actual reviewer was GPT-5.6 Sol with xhigh reasoning and fast service tier.
Thread `019ff191-98c1-7f11-b174-da79b11cf340` reported eight findings at
overall confidence `0.98`. We accept all eight.

| Finding | Disposition and correction |
| --- | --- |
| Engine desire can commit after stop's scan | Accept. Both initial desire and restart admission now have exact static and behavioral obligations to hold the provider admission guard through the Engine CAS. Stop persists the barrier under that lock before it scans Engine. |
| Both forwarded consumers construct registries | Accept. One concept-owned canonical composition constructs the registry. Server and Compose consumers only delegate to it. Independent mutations remove each delegation. |
| Machine checks count marker names | Accept. The scanner extracts both Engine CAS bodies, three provider admissions, five real physical callers, and the real effect body. It checks body-local dataflow and order. |
| Compose constructs but does not use the Engine store | Accept. The scanner requires the exact store to reach prepared runtime activation before the retirement facade. A disconnected-store mutation fails. |
| Compose discards terminal execution identity | Accept. The scanner requires the bound terminal reference to reach the returned durable recorded outcome. A discard mutation fails. |
| Guest validation and journal order are not connected | Accept. The scanner extracts dispatch, validate, and execute. It requires validation before execute and journal claim before remote I/O. Two order mutations fail. |
| Coarse guest absence is incomplete | Accept. The deletion census covers the function, operation, path, request, response, response wire, and capability tokens. Route, wire, and capability mutations fail. |
| Barrier policy is assigned to the provider | Accept. Compute owns policy, evidence classification, and the stop decision. The provider owner retains only lock, barrier persistence, conditional clearing, and admission authentication. |

These executable verifier corrections authorize exactly one narrow correction
review after all affected proofs are green. The cadence permits no second full
review.

Corrected pre-ledger candidate:

- staged tree: `33dedcca7cc77590364e06edb364addf603ea060`.
- staged binary patch SHA-256:
  `aeb70292a748969795878d6caf914f571bdd3e736c8da3bd8223056faeaf7137`.
- scope: exactly seven audit-owned paths and no unstaged path.

The sole narrow correction review used the final staged tree
`9ea703bcedc89bf9e5c01a4083feb8a0736db651`. The actual reviewer was GPT-5.6
Sol with xhigh reasoning and fast service tier. Thread
`019ff1e1-f033-7d91-852b-eaa6c9a8ac31` reported nine P2 findings at overall
confidence `0.99`. We accept all nine:

1. invoke the Engine CAS closure inside the same provider-lock mutation.
2. authenticate before every required provider journal or effect boundary.
3. check physical effect order inside the exact stop function.
4. bind the exact Engine store activation to the runtime that supplies the
   retirer.
5. pass the bound terminal execution reference into the recorded return.
6. return each canonical forwarded composition result from its consumer.
7. pass the validated guest value into execute and reject every known remote
   send before the journal claim.
8. prove compute and confirmed-publication ownership in separate source files.
9. delete the concrete coarse-stop capability advertisement.

The final corrections extract the exact locked closure, every provider
boundary, and the exact physical-stop body. They also bind Compose dataflow,
forwarded consumer returns, guest request flow, and separate owner files. The
capability check names the concrete advertisement. Twelve new mutations fail
closed with only their assigned diagnostic. The cadence forbids a third
review.

Final executable verifier identity:

- four script paths and zero product-source paths.
- binary patch SHA-256:
  `f952fee2b597fe8a097d5f04ccc23e7afcab6340ce4a89e8d094aadb3d9b14e5`.
- helper mutation suite: `138/138`.
- complete aggregate mutation suite: `552/552`.
- live NNCV035: exact expected `0/7`.
- live aggregate: exact expected `35/36`, with only NNCV035 red.

## Acceptance ledger

| Criterion | Status | Evidence |
| --- | --- | --- |
| A1-A15 architecture and split | `pass` | Three independent source audits freeze the Compose, guest/forwarded, and physical-machine callers, current and target owners, f1-f3 split, failure matrix, exact test rosters, and semantic mutation set. The accepted corrections preserve compute policy and bind every real admission/caller seam. Product source is unchanged. |
| A16 ledger and routing | `pass` | Plan and ledger contain `115/115` unique IDs with no mismatch and one `in_progress` row. NNCV008 and NNCV009 pass. Routing names f1-f3 and preserves NNC6.5g authority. |
| A17 static proof | `pass` | The strengthened helper passes `138/138`; the aggregate passes `552/552`; live NNCV035 is exact `0/7`; and the live aggregate is exact `35/36` with only NNCV035 red. Each of the twelve new mutations fails with only its assigned diagnostic. |
| A18 quality and docs | `pass` | Rust format, Prettier, diff, Node/Bash syntax, focused ShellCheck, strict proof lint, NNCV004/NNCV008/NNCV009/NNCV012, docs `108`, and site `17/17` pass. The aggregate ShellCheck retains only pre-existing unused-helper and dynamic-source-follow warnings outside the changed teardown helper. |
| A19 one item review | `pass` | The sole full review and sole narrow correction review are complete. All eight full-review and nine narrow-review findings are accepted, corrected, and proven. The cadence forbids another review. |
| A20 durable audit checkpoint | `pass` | The item commit that contains this row closes exactly the read-only audit, proof, static contract, ledger, and routing. Four script paths changed; no product source changed. |

## Recovery state

- owner worktree: `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`.
- owner branch: `codex/nimbus-network-architecture-audit`.
- dependency checkpoint: NNC6.5e item commit `18377b1a2`.
- product source changed by this audit: none.
- owned paths: proof, plan, routing, NNCV035 helper, and aggregate count.
- acceptance: A1-A20 pass.
- full item review: complete.
- full-review corrections: all eight findings pass their proof.
- narrow correction review: complete.
- narrow-review corrections: all nine findings pass their proof.
- remaining review work: none. The cadence forbids another structured review.
- next item: NNC6.5f1 canonical forwarded composition and foreground retirement
  facade.
- blocker: none.
