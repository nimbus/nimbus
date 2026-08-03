# NNC6.3b Pure Provision Decision Contract

Status: `complete; item review and sole correction review closed`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Base commit: `ed0560b4e45f7ec571934624962de72d021a71a8`

## Outcome

NNC6.3b adds one pure provision composition seam and one pure provision
decision protocol. The item persists no provider effect, calls no provider,
and changes no product caller.

The workload saga retains a compute-issued portable attempt before an external
effect can start. A provider owner keeps its real handle, journal, and effect
receipt. The saga retains only the exact command fence and closed outcome
state. This split gives a fresh process enough evidence to inspect before a
retry without creating a second provider store.

Every resource-bearing compiled network plan retains the exact provider
selection and a domain-separated digest of the selected source reports.
Resource-free plans retain neither value because selection chooses no provider.
Stable provider IDs alone do not authenticate capability, sovereignty,
forwarding, address, or TLS facts after a process restart.

## Scope

NNC6.3b owns:

- a strict workloads-owned provision source, attempt, result, success, and
  disposition vocabulary.
- an exact provider-report digest bound into the compiled network plan.
- explicit endpoint forwarding and TLS behavior in canonical plan content.
- one compute-owned pure composition constructor.
- one compute-owned exhaustive provision reducer used by ingress and recovery.
- fail-before, serialization, state-machine, dependency, and path proofs.

NNC6.3b does not own:

- a provider trait, provider registry split, provider dispatch, socket,
  namespace, forwarding, proxy, certificate, or sandbox effect.
- a service, sandbox, Compose, Machine API, node, Convex, or Cloud Functions
  caller cutover.
- a second saga store, coordinator, desired-state authority, or compatibility
  decoder.
- teardown, compensation, restart, resolver, projection, cluster transport,
  certificate provider, or service-name ownership.

NNC6.4 retains every real provider command, adapter seam, caller replacement,
and coarse-path deletion. NNC6.5 retains failed-provision compensation.

## Current State

The current call graph is:

```text
tests only
  -> WorkloadNetworkPlanCompiler::compile(...seven parallel inputs...)
     -> validate transient tenant/source values
     -> select_exact(provider IDs, requirements)
     -> CompiledWorkloadNetworkPlan

WorkloadSagaCoordinator::submit_intent
  -> load
  -> create/apply intent
  -> one CAS plus bounded ambiguity read
  -> WorkloadSagaDecision::for_record(record)

WorkloadSagaDecision::for_record
  -> phase-only provision or teardown action
```

The current compiler has no product caller. `submit_intent` and bounded
recovery also have no product caller. NNC6.4 therefore remains the first
effectful cutover.

The source audit found these gaps:

1. The selector accepts no typed effect result.
2. The saga stores no compute-issued attempt before an effect.
3. `WorkloadFailureEvidence` is valid only in `CleanupPending`, so a definite
   provision failure cannot retain its completed phase.
4. `NetworkAttached` selects activation without a distinct prerequisite
   inspection.
5. Assigned node evidence is optional until execution-reference creation.
6. The compiler validates source identity and revision transiently but does not
   persist them.
7. The source model conflates source revision with workload deployment
   generation.
8. The plan stores provider IDs but not the selected source-report digest.
9. HTTPS implies termination, so TLS passthrough is not representable.
10. The compiler infers forwarding from `guest_port` instead of authenticating
    an admitted endpoint value.

## Frozen Architecture Decisions

### D1: One pure submission constructor

`nimbus-compute` owns one parameter-object constructor that returns an exact
`WorkloadSagaKey` and complete `WorkloadSagaIntent`. It does not access a
store, clock, random source, allocator, provider, or socket.

The constructor consumes:

- one `TenantIsolationDecision`.
- one explicit local `NodeIdentity`.
- one closed source snapshot with stable logical identity, independent source
  generation, resource version, and exact sandbox specification.
- one exact source-owned `NetworkCapabilitySelection`.
- one immutable registry snapshot.
- explicit sovereignty, endpoint forwarding, endpoint TLS, activation, and
  publication inputs.

It rebuilds `TenantWorkloadSpec`, validates the local node, and derives the
logical workload key. It then encodes the executable and compiles the plan.
Finally, it constructs the source evidence, admission evidence, and intent.
Callers cannot submit partially composed fields through this seam.

The constructor rejects running `Empty` sources. The only executable encoding is a
canonical `SandboxSpec`, and Cloud Functions remains snapshot-only. A later
real executable family can add a new closed source and encoding together.

### D2: Source evidence is durable and separate from workload generation

`nimbus-workloads` owns `WorkloadProvisionSourceEvidence`. Its closed variants
cover a standalone sandbox and a sandbox-backed service. Each value retains:

- stable source identity and source kind.
- source generation.
- non-empty source resource version.
- source content digest.
- exact required attachment provider ID.

The source digest is domain-separated over these fields and the executable
content digest. The desired digest binds source evidence beside executable,
network, activation, publication, and tenant admission evidence.

Source generation never aliases `WorkloadGeneration`. The pure constructor
checks deployment generation only against tenant admission. It checks source
generation and resource version against the single source snapshot.

That snapshot is the one source-owned composition input. The constructor
validates its generation, resource version, content, and digest together. The
constructor then binds them into desired state. NNC6.3b does not create a
second source catalog or claim
that a durable snapshot is current forever. NNC6.4 owns the pre-dispatch
comparison between this admitted evidence and the current source authority.

`nimbus-services` remains the service-definition and logical-name authority.
The durable source value is an authenticated snapshot for lifecycle recovery,
not a second catalog.

### D3: Intent construction requires an assigned node

A running saga intent requires one assigned `NodeIdentity`. The pure
constructor compares its explicit local node with the tenant decision's
assigned node before executable encoding or intent construction.

`WorkloadAdmissionEvidence` stores the required node. Missing or crossed node
input cannot reach `submit_intent`. The desired digest and execution ID bind
the same node.

NNC6.4 must supply the canonical node value from each outer composition. This
item defines and proves the fail-before contract but does not edit those
callers.

### D4: Selection authenticates exact provider reports

`nimbus-network` adds a portable `NetworkCapabilitySelectionEvidence` value.
It contains the exact `NetworkCapabilitySelection` and a domain-separated
digest of the complete selected attachment and ingress registrations.

`NetworkCapabilityRegistry::select_exact` remains the only selection
authority. A successful selection produces the evidence from the returned
bundle, never from caller-supplied IDs or a forged digest. The compiler stores
that evidence once in canonical plan content. A resource-free plan stores
`None`. Any plan with an attachment, route, listener, or other provider-owned
resource requires `Some(evidence)`. There is no `.first()`, `.next()`,
safe-alternative adoption, or registry-order fallback.

The source-report digest does not create a provider store. Provider owners
still report current facts. NNC6.4 must compare current source evidence with
the admitted digest before dispatch. The strict workload network-plan format
advances from version 1 to version 2. There is no version-1 decoder or alias.

### D5: Forwarding and TLS are explicit endpoint semantics

Each admitted listener has one exact endpoint-semantics value keyed by its
listener name. It contains:

- `None` or `PortForwarded` forwarding intent.
- `Disabled`, `Passthrough`, or `TerminateAtIngress` TLS behavior.

The compiler rejects missing, extra, duplicate, or crossed endpoint semantics.
`PortForwarded` must agree with guest-port shape and requires the forwarding
capability. HTTPS requires either passthrough or termination. TCP and HTTP
require disabled TLS. The exact ingress registration must report the selected
TLS behavior.

The canonical listener content retains both values. The plan digest therefore
binds listener name, protocol, desired address, port request, guest port,
forwarding behavior, and TLS behavior.

TLS termination does not imply a certificate provider. NNC7.6 retains ingress
certificate and interception-CA separation. Provider effects remain in their
current owners.

### D6: The saga proposes a portable attempt for confirmation before effects

`nimbus-workloads` adds these closed provision steps:

1. `ReserveNetwork`
2. `PrepareWorkload`
3. `AttachNetwork`
4. `InspectActivationPrerequisites`
5. `ActivateWorkload`
6. `InspectWorkloadReadiness`
7. `Publish`
8. `ObservePublication`

`WorkloadProvisionAttempt` binds:

- attempt ID, saga key and saga ID.
- issuing revision, generation, and desired digest.
- required node and source digest.
- exact network-plan digest and provider-selection evidence.
- source phase, target phase, provision step, and exact typed subjects.
- prerequisite evidence when the activation attempt follows a successful
  activation-prerequisite inspection.

The attempt ID is domain-separated over the complete canonical attempt
payload. No IP address, assigned port, or provider handle participates in its
identity.

The pure reducer returns a `ProposedWorkloadProvisionTransition`. The
transition contains a candidate record and a symbolic action that is not
dispatchable. The sole compute coordinator must confirm that exact candidate
through the one saga store. Only then may NNC6.4 turn its symbolic action into
a confirmed dispatch command. Neither ingress nor recovery can create a command
from an unconfirmed candidate. NNC6.3b adds no provider effect or dispatcher.

This proves the negative half of the handoff: symbolic actions cannot
reach an effect and no dispatch authority exists in this item. NNC6.4 owns the
positive half, including the exact confirmed-record token and dispatcher.

### D7: Provision disposition is orthogonal to durable phase

`WorkloadSagaRecord` gains an optional closed
`WorkloadProvisionDisposition`:

- `Ready`
- `AttemptPending(attempt)`
- `InspectionRequired(attempt)`
- `DefiniteFailure { attempt, failure }`

`Some(Ready)` and the three attempt-bearing values are valid only for running
provision phases. Teardown, cleanup, stopped, and recorded state use `None`.
`Ready` is therefore not overloaded as a lifecycle-neutral value. The complete
transition identity includes the option. A proposed attempt changes
the disposition and revision without changing the completed phase.

Only the coordinator's exact prepare-attempt CAS confirmation may authorize
the symbolic action for NNC6.4 dispatch. Reopening `AttemptPending` from
durable state permits only inspection. The prior process can start the effect
before it dies.

An ambiguous result changes only `AttemptPending` to `InspectionRequired` for
the exact attempt. Repeated ambiguity permits only the same inspection.

A definite failure changes only the disposition. It retains the completed
phase, increments the revision, and permits no later provision command.
`requires_recovery` excludes the halted generation. NNC6.5 retains explicit
withdrawal and compensation.

The existing cleanup failure field remains scoped to `CleanupPending`. This
item does not overload it with provision failure semantics. State validation
rejects a higher desired generation while the active generation is
`AttemptPending`, `InspectionRequired`, or `DefiniteFailure`. NNC6.5 must first
resolve or compensate that state. Withdrawal never races an ambiguous
unresolved effect.

Version 3 replaces version 2 of the strict workload-saga format.

Version 3 also replaces the prior desired-digest and transition-identity
domains. No version-2 decoder, default, or compatibility alias exists.

### D8: One closed result vocabulary and one reducer

`WorkloadProvisionEffectResult` has exactly three variants:

- `Succeeded { attempt_id, evidence }`
- `DefiniteFailure { attempt_id, failure }`
- `Ambiguous { attempt_id }`

The success evidence uses closed, step-specific variants. Activation-prerequisite
readiness is distinct from post-activation workload readiness. Publication
presence is distinct from publication observation.

`nimbus-compute` owns one pure reducer with two entry points:

```text
plan(record) -> proposed attempt transition and symbolic action, exact
                inspection, phase-only transition, definite failure, or wait
reduce(record, result) -> proposed next record and optional symbolic action,
                          exact inspection, definite failure, or wait
```

The general recovery selector delegates every provision phase to this reducer.
It does not keep a second provision switch. Teardown selection remains in the
general recovery module for its later owner.

The reducer returns values only. NNC6.4 earns provider interfaces and dispatch.

## Target Call Graph

```text
outer composition snapshot (NNC6.4 caller later)
  -> compose_workload_provision(input)
     -> validate tenant + local node + source snapshot
     -> encode_sandbox_spec
     -> WorkloadNetworkPlanCompiler::compile
        -> select_exact
        -> bind exact provider-report digest + endpoint semantics
     -> WorkloadProvisionSourceEvidence
     -> WorkloadAdmissionEvidence
     -> WorkloadSagaIntent + WorkloadSagaKey

WorkloadSagaCoordinator::submit_intent
  -> exact durable confirmation
  -> WorkloadSagaDecision::for_record
     -> WorkloadProvisionDecision::plan for provision phases

WorkloadProvisionDecision::plan/reduce
  -> pure proposed candidate record and non-dispatchable symbolic action
  -> exact inspection only
  -> terminal definite failure
  -> wait

sole WorkloadSagaCoordinator (confirmation owned by NNC6.4)
  -> CAS the exact proposed candidate through the one saga store
  -> only confirmed state may authorize provider dispatch
```

No arrow in this graph reaches a provider effect.

## Exhaustive Provision Matrix

| Completed phase | Ready decision | Success result | Definite failure | Ambiguous or reopened attempt |
| --- | --- | --- | --- | --- |
| `IntentCommitted` | Propose reserve attempt plus symbolic action; dispatch requires later store confirmation. | Propose `NetworkReserved`. | Retain `IntentCommitted`; halt. | Inspect exact reserve attempt. |
| `NetworkReserved` | Persist prepare attempt, then return prepare command. | Persist `WorkloadPrepared`. | Retain `NetworkReserved`; halt. | Inspect exact prepare attempt. |
| `WorkloadPrepared` | Persist attach attempt, then return attach command. | Persist `NetworkAttached`. | Retain `WorkloadPrepared`; halt. | Inspect exact attach attempt. |
| `NetworkAttached`, prepare-only | Wait. | Reject. | Reject. | Reject. |
| `NetworkAttached`, activate | Persist prerequisite-inspection attempt, then return inspect command. | Prerequisite success persists an exact activation attempt before returning activate. Activation success persists `WorkloadActivated`. | Retain `NetworkAttached`; halt. | Inspect the exact prerequisite or activation attempt. |
| `WorkloadActivated` | Persist workload-readiness attempt, then return inspect command. | Persist `Ready`. | Retain `WorkloadActivated`; halt. | Inspect exact workload-readiness attempt. |
| `Ready`, withheld | Persist the pure `Observed` transition. | Reject. | Reject. | Reject. |
| `Ready`, publish | Persist publish attempt, then return publish command. | Persist `Published`. | Retain `Ready`; halt. | Inspect exact publish attempt. |
| `Published` | Persist observation attempt, then return inspect command. | Persist `Observed`. | Retain `Published`; halt. | Inspect exact observation attempt. |
| `Observed` | Wait. | Reject. | Reject. | Reject. |

Every success advances at most one durable phase. The prerequisite success is
the sole same-phase success and it replaces the inspection attempt with an
activation attempt that retains exact predecessor evidence.

## Fail-Before Composition Matrix

The pure constructor rejects these rows before intent persistence or effect:

| Input defect | Required result |
| --- | --- |
| Missing or crossed local node | Typed composition error; no submission value. |
| Missing stable source ID | Typed source error. |
| Crossed source tenant, kind, name, backend, owner, or profile | Typed source error. |
| Missing or crossed source generation/resource version | Typed source error. |
| Crossed executable/source digest | Typed source error. |
| Missing or crossed workload generation | Typed admission error. |
| Missing or crossed exact selection | Typed selection error. |
| Known providers in an unadmitted pair | Unregistered composition error. |
| Satisfying diagnostic alternative | Original selection still fails. |
| Crossed provider-report digest | Typed source-evidence error. |
| Sovereignty relaxation or unsupported source report | Typed capability error. |
| Missing, extra, duplicate, or crossed listener semantics | Typed endpoint-semantics error. |
| Forwarding behavior disagrees with guest-port shape | Typed forwarding error. |
| Address family, realm, or exposure unsupported | Typed capability error. |
| TLS behavior disagrees with protocol | Typed TLS error. |
| Exact ingress lacks passthrough or termination evidence | Typed selection error. |
| Activation or publication disagrees with plan content | Typed intent error. |
| Running empty source | Typed unsupported-source error. |

All constructor failure tests use store, lease, and provider spies fixed at
zero. The constructor API itself has no effect-capable parameter.

## Result And Recovery Failure Matrix

| Result defect or boundary | Required result |
| --- | --- |
| Unknown result or success variant/field | Strict decode failure. |
| Crossed attempt ID | Reject without candidate record or command. |
| Crossed key, saga ID, revision, generation, digest, node, source, plan, selection, step, phase, or subject | Reject without candidate record or command. |
| Wrong success evidence for step | Reject without state change. |
| Duplicate or out-of-order observation | Reject without state change. |
| Process dies before attempt CAS | No attempt and no command is recoverable. |
| Process dies after attempt CAS and before effect | Fresh process inspects the exact attempt. |
| Process dies after effect and before result CAS | Fresh process inspects the exact attempt. |
| Definite failure result | Same completed phase plus terminal provision disposition. |
| Ambiguous result | Same completed phase plus exact inspection disposition. |
| Reopen definite failure | No provision command. |
| Reopen ambiguous/pending attempt | Exact inspection only. |
| Prepare, attach, or activate result | No publication action. |
| Publish before workload readiness | Typed rejection. |

## Path Allowlist

Product paths may include only:

- `crates/nimbus-network/src/capability.rs`
- `crates/nimbus-network/src/capability_registry.rs`
- `crates/nimbus-network/src/plan.rs` and
  `crates/nimbus-network/tests/readiness_dependency.rs` only for deterministic
  digest expectation replacements caused by the canonical content change.
- network concept-owned tests and public exports.
- `crates/nimbus-workloads/src/network_plan.rs` and a concept-owned child if
  needed.
- `crates/nimbus-workloads/src/saga.rs` only for narrow wiring.
- `crates/nimbus-workloads/src/saga/provision.rs` and its child tests.
- `crates/nimbus-workloads/src/saga/test_support.rs`, compiled only under
  `cfg(test)`, for reducer-driven exact-history fixtures.
- `crates/nimbus-workloads/src/saga/state.rs` and focused state tests.
- `crates/nimbus-workloads/src/lib.rs`.
- `crates/nimbus-compute/src/workload_network_plan.rs` and focused tests.
- `crates/nimbus-compute/src/workload_provision_composition.rs` and child
  tests.
- `crates/nimbus-compute/src/workload_saga.rs` only for narrow wiring.
- `crates/nimbus-compute/src/workload_saga/provision_decision.rs` and child
  tests.
- `crates/nimbus-compute/src/workload_saga/test_support.rs`, compiled only
  under `cfg(test)`, for reducer-driven exact-history fixtures.
- `crates/nimbus-compute/src/workload_saga/recovery.rs` and focused delegation
  tests.
- `crates/nimbus-server/src/network_capabilities.rs` and its tests only for
  the source-owned portable TLS report.
- `crates/nimbus-workloads/src/store/tests.rs`,
  `crates/nimbus-workloads/src/saga/tests.rs`, and
  `crates/nimbus-workloads/src/saga/network/tests.rs` for strict portable-state
  fixtures.
- `crates/nimbus-compute/src/workload_saga/tests.rs`,
  `crates/nimbus-compute/src/workload_saga/ingress.rs`, and its child tests for
  the sole-coordinator delegation proof.
- `crates/nimbus-server/src/workload_saga_store/codec.rs`,
  `crates/nimbus-server/src/workload_saga_store/schema.rs`, and their focused
  tests for the exact physical `source` and `provisionDisposition` fields.
- `crates/nimbus-server/src/workload_saga_store/tests/provision_fixture.rs`,
  compiled only under `cfg(test)`, and sibling store tests that replace forged
  phase shortcuts with complete reducer-produced revision history.
- `crates/nimbus-server/tests/network_capability_registration.rs` only for the
  portable TLS-report registration assertion.

Control paths may include this proof, the canonical plan, and the routing
index. They may include the NNCV032 contract, self-test helpers, and aggregate
verifier. They may also include the narrow NNCV031 completion checkpoint pin.
That pin scopes the predecessor gate to its own durable item range. Narrow

NNCV027 and NNCV029 corrections preserve their original recovery and
compiled-plan authority. They use the canonical reducer and strict saga-v3
vocabulary. The exact NNC4.6f census correction updates the line anchor for the
edited source-owned ingress registration.

The changed-path census fails on sandbox effects, services catalogs or
managers, machine/node/proxy/egress/system/CLI paths, server routers, protocol
adapters, manifests, or any provider caller. NNC6.4 owns those paths.

## Modularity Disposition

Four changed files require an explicit ownership disposition:

- `crates/nimbus-workloads/src/network_plan.rs` is 1,607 lines. It is one
  portable-plan deep module: strict wire decoding, canonical content
  validation, and content/reference digest identity must agree. A concept-owned
  child holds its behavioral matrix.

  One production invariant remains. A split
  would create two interpretations of canonical plan identity. This is a
  deliberate 1,500–1,999-line exception. Provider
  effects, compilation policy, and orchestration may not enter it.
- `crates/nimbus-compute/src/workload_network_plan/tests.rs` is 1,685 lines.
  It is one test-only compiler behavior matrix. The same private seam owns its
  fixtures, fail-before crossings, and exact compile assertions. Process
  mechanics already live in `tests/child_process.rs`. It is a
  deliberate test-band exception and may receive no production logic or
  generic fixture authority. The next coherent proof group must move intact
  before 2,000 lines.
- `crates/nimbus-workloads/src/saga/tests.rs` is 1,990 lines. It is the
  portable `WorkloadSagaRecord` cross-phase state-machine matrix. Provision
  vocabulary and reducer-history support already have concept-owned children.

  The remaining cases share exact multi-phase fixtures and transition
  identity. Splitting them during this item would duplicate the canonical
  lifecycle matrix. This is a final 1,500–1,999-line exception. An intact proof
  group must move to a named child before another inline group enters this file.
- `scripts/verify-nimbus-network-control-plane.sh` is 2,016 lines. This is a
  strong ownership-based script exception, not a production module. It is the
  single aggregate router for the concept-owned verifiers. It preserves exact
  bounded-prefix and continuation mutation arithmetic. It also emits one
  fail-closed summary. NNCV032 logic remains in its own contract and self-test

  helpers. Splitting the aggregate arithmetic would create competing closeout
  authorities. Unrelated checks cannot enter it.

## Acceptance Criteria

| ID | Criterion |
| --- | --- |
| E1 | One parameter-object constructor returns an exact key and intent without an effect-capable parameter. |
| E2 | Required local node and source snapshot evidence are desired-digest bound and survive strict fresh-process Engine round-trip. |
| E3 | Source generation/resource version remain distinct from workload generation. |
| E4 | Every resource-bearing compiled plan binds exact provider IDs and the exact selected source-report digest; resource-free plans bind neither. |
| E5 | Explicit forwarding and TLS semantics are listener-name bound and capability checked. |
| E6 | Missing/crossed node, single source-owned snapshot generation/resource version/content, selection, sovereignty, forwarding, address, publication, or TLS input rejects before submission/effect; current-source comparison remains NNC6.4. |
| E7 | Safe alternatives remain diagnostic; no iteration-order fallback exists. |
| E8 | The reducer proposes an exact portable but non-dispatchable attempt; no dispatch authority exists in NNC6.3b, and NNC6.4 alone may derive one from the sole coordinator's exact store confirmation. |
| E9 | Attempt identity binds every named fence and never uses IP address, assigned port, or provider handle as workload identity. |
| E10 | Result vocabulary contains exactly success, definite failure, and ambiguous variants with strict decoding. |
| E11 | The provision reducer exhaustively covers every provision phase, branch, step, and result. |
| E12 | Activation-prerequisite readiness and post-activation workload readiness are distinct. |
| E13 | Definite failure retains the completed phase and emits no later provision command after replay or reopen. |
| E14 | Pending or ambiguous reopen emits exact inspection only. |
| E15 | Crossed result fences and evidence produce no candidate record or command. |
| E16 | Publication is unreachable before exact workload readiness; earlier steps emit no publication. |
| E17 | Recovery and ingress delegate to the same provision reducer. |
| E18 | One saga store and one compute coordinator remain; no provider interface, effect, caller cutover, or compatibility path appears. |
| E19 | Dependency and effect scans preserve the existing graph, including `nimbus-network -> nimbus-core` as its only workspace edge. |
| E20 | NNCV032, focused behavior, affected suites, quality gates, docs gates, dependency/effect/path scans, and candidate identity pass with exact evidence. |

The structured review is a post-acceptance closeout gate. It reviews the
candidate-frozen E1-E20 unit once. It is not part of the evidence required to
freeze that candidate.

## Acceptance Evidence

| ID | State | Evidence |
| --- | --- | --- |
| E1 | `green` | The composition constructor accepts one parameter object. Focused composition and NNCV032 API-shape checks prove that it returns one exact key and intent without an effect-capable parameter. |
| E2 | `green` | Strict workload, physical-codec, durability, and fresh-process recovery tests preserve the exact node, source snapshot, desired digest, attempt, result, and disposition fields. |
| E3 | `green` | `source_generation_changes_source_and_desired_digests_without_changing_deployment_generation` proves independent source and workload generations. |
| E4 | `green` | Network-plan and compiler matrices prove exact selected IDs and source-report digest for resource-bearing plans. They also prove that resource-free plans retain neither value. |
| E5 | `green` | Network, compiler, and server registration tests prove listener-name-bound forwarding and all three portable TLS behaviors. |
| E6 | `green` | Composition crossing matrices and NNCV032 substitutions reject every named missing or crossed input before submission or effect. |
| E7 | `green` | Registry and compiler tests retain satisfying alternatives as diagnostics. Structural checks reject `.first()`, `.next()`, or registry-order adoption. |
| E8 | `green` | Reducer tests return only portable symbolic actions. NNCV032 proves that no dispatcher, provider trait, or effect authority exists in this candidate. |
| E9 | `green` | Attempt-correlation matrices bind every named fence. Structural checks reject address, assigned port, and provider handle as workload identity. |
| E10 | `green` | Strict workload and physical decoding accept only success, definite failure, and ambiguous results. Mutation cases reject unknown and legacy shapes. |
| E11 | `green` | `every_provision_phase_and_result_is_exhaustive` covers every provision phase, branch, step, and result. Exact revision-history checks cover each resulting record shape. |
| E12 | `green` | `activation_prerequisite_success_prepares_activation_attempt` proves distinct prerequisite and post-activation readiness evidence. Direct portable-state regressions prove that prerequisite inspection cannot complete activation and that a generic `Ready -> AttemptPending` transition cannot bypass the retained prerequisite inspection. |
| E13 | `green` | Definite-failure and reopen tests retain the completed phase and return no later command. |
| E14 | `green` | Pending and ambiguous reopen tests reconstruct the exact attempt and return inspection only. Fresh-process recovery retains the same correlation. |
| E15 | `green` | Crossed attempt, result, subject, source, plan, selection, node, generation, revision, and digest matrices return no candidate or command. |
| E16 | `green` | Publication reachability tests reject publication before exact workload readiness. Every earlier step lacks a publication action. |
| E17 | `green` | `ingress_and_recovery_delegate_to_same_provision_reducer` and NNCV027 prove one provision switch. |
| E18 | `green` | The 55-path candidate census contains no provider caller, effect owner, manifest, or compatibility path. One saga store and one compute coordinator remain. |
| E19 | `green` | Cargo metadata reports `nimbus-core` as the only `nimbus-network` workspace dependency. No Cargo manifest or lockfile changed, and NNCV032 effect scans pass. |
| E20 | `green` | Affected suites, checks, strict Clippy, warning-denied rustdoc, format, diff, Bash syntax, scoped ShellCheck, proof lint, docs `108`, site `17/17`, NNCV032 `32/32`, its mutations `36/36`, live aggregate `33/33`, and bounded aggregate mutations `277/277` pass. |

## Verifier Contract

NNCV032 is `workload-provision-decision-contract`. The direct helper must pass
32 named conditions. The self-test must apply 36 non-no-op mutations. The
mutations cover result variants, every correlation field, every composition
dimension, definite failure, ambiguity, fallback, and duplicate authority.
They also cover forbidden effects, dependencies, paths, compatibility, and
missing behavior proof. Test-support containment and exact revision-history
validation complete the mutation set.

The expected closeout arithmetic is:

```text
NNCV032 direct: 32/32
NNCV032 mutations: 36/36
live aggregate: 33/33
retained plus new mutations: 241 + 36 = 277/277
```

The helper must verify each fixture substitution changed its input before it
runs the mutated contract.

## Verification Plan

Before product edits:

1. Record the clean base and current live `32/32` verifier result.
2. Install NNCV032 and record its expected-red diagnostics.
3. Run all 36 contract mutations against the expected-red fixture.

During implementation:

1. Run focused strict wire and state-machine tests.
2. Run the composition crossing matrix with zero-call spies.
3. Run success, definite-failure, ambiguity, and crossed-fence matrices.
4. Run existing compiler, ingress, recovery, and durability regressions.
5. Run NNCV032 after each seam converges.

To freeze the candidate:

1. Run full affected `nimbus-network`, `nimbus-workloads`, and
   `nimbus-compute` suites.
2. Run `nimbus-server` when nested durability fixtures change.
3. Run affected checks, strict Clippy, warning-denied rustdoc, format, and
   dependency/effect/path scans.
4. Run NNCV032 direct, self-test, live aggregate, and aggregate mutations.
5. Run docs and site gates with exact counts.

After candidate freeze:

1. Run one GPT-5.6 Sol/xhigh/fast structured review over the complete item.
2. Rerun affected proofs and one narrow defect review only if an accepted
   finding materially changes executable code.

## Review And Correction Disposition

The complete candidate received exactly one GPT-5.6 Sol/xhigh/fast item review
after E1-E20 were green. Its reviewed tree was
`eba9b42d97de4d1713c1122878fc2668b9bccba5`; its patch SHA-256 was
`b849013b7b49aef78b05044e1fa9fc2aa6b2477f2cb756f65fdd747b8e3993b7`.
The review produced ten findings:

- Eight executable findings were accepted. The corrections enforce exact
  attempt target-phase completion, exact initial promotion disposition,
  prerequisite/current-intent/subject/attempt correlation, kind/source
  correlation, durable publication-observation evidence, evidence-free empty
  TLS semantics, truthful aggregate local-ingress cleartext plus termination
  reporting, and fail-closed Git path/digest census execution.
- The claim that admission was absent from the desired digest was rejected
  from `WorkloadDesiredDigestPayload`, its constructor, validation, and an
  exact mutation regression.
- The pre-review candidate-identity placeholder was an expected closeout
  ledger state, not an executable defect; this checkpoint replaces it.

Each accepted executable finding has fail-before evidence and corrected
behavior. The Git census mutation, for example, proved that a fake Git command
could exit 73 while the historical contract returned success. The portable
transition, publication, and TLS cases each failed directly before their
correction.

Because those corrections materially changed executable code, exactly one
narrow GPT-5.6 Sol/xhigh/fast correction review ran over tree
`41e234bf0fb9389217a6476babe953e0aacb52c0`, patch SHA-256
`4d79b482e77b5c8dd338c21c949c4119125687e727e221fcfaab79c8084bf4a3`.
It produced two findings:

- A generic `Ready -> AttemptPending` transition could bypass retained
  activation-prerequisite inspection. This was accepted. The exact direct
  fail-before was `0/1`; the corrected transition guard and regression pass
  `1/1`, and the final workloads suite passes `125/125`.
- The claim that the intent constructor omitted kind/source validation was
  rejected: `WorkloadSagaIntent::new` immediately calls `validate`, which
  performs that exact correlation. Both invalid variants already have direct
  constructor coverage.

An initial mistyped `--exact` invocation selected zero tests and is explicitly
excluded from evidence; the unfiltered named command above is the retained
fail-before proof. No further structured review is warranted or permitted by
the item-value review cadence. After the accepted narrow-review correction and
before ledger-only closeout, the staged tree was
`00d64578012338e23c17b506447fbe5a38bc08b0`, the full patch SHA-256 was
`df042085bb132faae91a3141f6a644a1fea5199b62ff47c2928a82c7c7aa9417`,
and the executable/non-private-doc patch SHA-256 was
`ff0551ba284866427b2a62fc147e94ab1695f68f1089f2238bb97f0c81be1de3`.

## Recovery Ledger

| Field | Value |
| --- | --- |
| Base and last completed commit | `ed0560b4e45f7ec571934624962de72d021a71a8` (`NNC6.3a`) |
| Current item | `NNC6.3b` closeout; `NNC6.4` is next after the exact item commit. |
| Current state | Complete. E1-E20, the item review, its one permitted correction review, all accepted corrections, and retained verification are green. Only the exact ledger-bearing item commit remains. |
| NNCV032 fixture | Known-good `32/32`; `36/36` non-no-op mutations fail closed. |
| NNCV032 product result | Exit 1 with 138 expected diagnostics before product edits. |
| Last green | Before NNCV032 installation, the live aggregate passed `32/32`. |
| Current live verifier | The aggregate is green at `33/33`, including NNCV032 at `32/32` direct conditions. |
| Retained aggregate mutation proof | `241/241` from NNC6.3a plus NNCV032 `36/36` pass as an exact bounded `277/277` run. |
| Predecessor proof | NNCV031 passes `25` direct checks and `13/13` mutations after pinning its path census to the durable NNC6.3a range. |
| Predecessor convergence | NNCV027 follows the canonical provision reducer. NNCV029 recognizes strict saga-v3 durability, and the NNC4.6f census line anchor matches the edited source-owned ingress report. |
| Affected behavior | Network passes `239` with one ignore. Workloads passes `125`. Compute passes `147` with one child-only ignore. Server passes `645` with 30 declared skips. |
| Quality and docs | Affected check, strict Clippy, warning-denied rustdoc, format, diff, Bash syntax, scoped ShellCheck, and proof lint pass. Docs pass `108` pages, and the site passes `17/17`. |
| Dependency and path proof | The exact candidate contains 55 paths and no manifest or lockfile. Cargo metadata reports only `nimbus-core` as a `nimbus-network` workspace dependency. |
| Dirty paths | Proof, plan/routing, NNCV032 contract/self-test, aggregate routing, predecessor corrections, and only the expanded frozen NNC6.3b product/test allowlist recorded above. |
| Blocker | None. |
| Next command | Run the ledger-only format/diff/docs gates, restage the exact 55-path candidate, and create the one NNC6.3b item commit. Then begin NNC6.4 with its read-only substitution audit. |
| Review | Complete. One full item review and, after accepted executable corrections, one narrow correction review ran with GPT-5.6 Sol/xhigh/fast. Every finding is dispositioned above; no further review is warranted. |
| Push or PR | Not authorized. |

## Linked Owners

- NNC4.3 and NNC4.6 own immutable exact registry composition.
- NNC6.2 owns the pure admitted network-plan compiler.
- NNC6.2a owns complete compiled-plan durability.
- NNC6.1e1 owns confirmed durable intent submission.
- NNC6.3a owns the strict executable carrier and closed desired digest.
- NNC6.4 owns all provider interfaces, commands, effects, and caller cutover.
- NNC6.5 owns failed-provision compensation and teardown.
- NNC7 owns listener integration, endpoint projection, and TLS authority
  guardrails.
- The horizontal-scaling plan owns distributed node identity and placement.
- The service-identity plan owns provider credential minting.
- `nimbus-tenant`, `nimbus-services`, `nimbus-egress`, and `nimbus-proxy`
  retain policy, logical names, PDP, and PEP ownership.
