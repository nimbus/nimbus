# NNC6.1b Workload Saga Decision

Status: `complete`

Starting checkpoint: `9cc2330666fb8f1b031337ad35d044604e594c93`

This item freezes the workload saga contract before product implementation.
NNC6.1c through NNC6.1e own the code changes. This item adds no product type,
store adapter, dependency edge, lifecycle effect, or compatibility path.

## Decision

`nimbus-workloads` owns the portable desired-workload record, saga state
machine, and store port. Only `nimbus-compute` decides and writes saga
transitions. `nimbus-server` owns the Engine-backed store adapter and private
table bootstrap.

The adapter uses `Engine::begin_mutation_execution_unit`. The durable record
lives in `_nimbus._workload_sagas`. The table is not a `SystemTable`, and
`nimbus-system` does not define its schema, codec, store, or transition logic.

The target adds these direct workspace edges:

```text
nimbus-workloads -> nimbus-network
nimbus-compute -> nimbus-workloads
```

The `nimbus-network -> nimbus-core` edge remains its only outgoing workspace
edge. The target graph is acyclic.

## Item Boundary

NNC6.1b owns these decisions:

1. The current authority census.
2. The portable identity and record vocabulary.
3. The allowed phase graph.
4. The asynchronous store port and CAS rules.
5. The Engine route, tenant, table, document key, schema, and indexes.
6. The failure and reconciliation contract.
7. The dependency changes and forbidden edges.
8. The later item assignments and exact proof obligations.

NNC6.1b does not implement these decisions. It does not retain a temporary
store, add an optional no-op adapter, or migrate one caller early.

## Current Authority Census

The census is source-derived at the starting checkpoint.

| Evidence | Count | Current fact |
| --- | ---: | --- |
| Reverse dependencies of `nimbus-workloads` | 7 | `nimbus-bridge`, `nimbus-cli`, `nimbus-node`, `nimbus-server`, `nimbus-services`, `nimbus-system`, and `nimbus-testing` |
| `DesiredWorkloadStore` implementations | 1 | `InMemoryDesiredWorkloadStore` |
| Product in-memory authorities | 2 | `ServiceManagerState` and the CLI workload boot planner |
| Physical production upsert sites | 3 | Service activation helper, sandbox create, and sandbox stop |
| `ServiceManager::new` call sites | 54 | Production composition and test fixtures all construct the concrete manager |
| Production recovery readers | 0 | No production path reads desired state to choose recovery work |

The current `DesiredWorkloadStore` is synchronous and infallible. Its upsert
silently replaces stale or divergent generations. Its snapshot restore can
replace the complete map without CAS or tenant fencing.

The current lifecycle order also differs by route:

| Route | Current ordering defect |
| --- | --- |
| Service start | Records in-memory intent before the backend effect, but no durable recovery reader exists. |
| Service stop | Records in-memory intent before stop, but the record disappears on process loss. |
| Sandbox create | Starts the backend before recording Running intent. |
| Sandbox stop | Stops the backend before recording Stopped intent. |
| Lazy activation | `RuntimeServiceRegistry` calls `ServiceManager::start_service_async` without compute. |
| Tenant teardown | Stops and removes resources without a desired-state transition. |

The CLI boot planner creates a second desired-state map. Product execution
uses that map only for counts and logs. NNC6.1c1 converts this code to a pure
plan builder or removes it.

## Frozen Ownership

| Concern | Canonical owner |
| --- | --- |
| Portable desired state, saga record, phase graph, CAS request, and CAS outcome | `nimbus-workloads` |
| Cross-domain decisions and transition ordering | `nimbus-compute` |
| Durable adapter, codec, schema bootstrap, and Engine calls | `nimbus-server` |
| OCC, atomic document/index/journal commit, and durable ambiguity handling | `nimbus-engine` |
| Network plans, leases, attachments, and network provider state | `nimbus-network` |
| Service and sandbox effects and observations | `nimbus-services` and `nimbus-sandbox` |
| Node-local reconcile and inspect execution | `nimbus-node` |
| Rebuildable observed status | `nimbus-system` |
| Cluster transport, membership, and placement leases | The horizontal-scaling owner and future `nimbus-cluster` |

`NodeWorkloadCoordinator` remains the narrow compute adapter for node reconcile
and inspect. NNC6.1c adds a separate compute-owned `WorkloadSagaCoordinator`.

## Frozen Portable Vocabulary

The implementation uses these concept-owned modules:

- `nimbus-workloads/src/saga.rs` owns values and legal transitions.
- `nimbus-workloads/src/store.rs` owns the object-safe store port.
- `nimbus-workloads/src/desired.rs` stops owning persistence and snapshot
  restore authority.

The portable identity ladder is:

| Type | Meaning |
| --- | --- |
| `WorkloadId` | Existing logical routing identity from `nimbus-core`. |
| `WorkloadSagaKey` | Exact `TenantId` plus `WorkloadId` pair. |
| `WorkloadSagaId` | Stable `wsg_` identity derived from the saga key with domain-separated SHA-256. |
| `TenantWorkloadUid` | Admission-incarnation evidence. It is not the logical saga key. |
| `WorkloadExecutionId` | Stable generation-scoped execution identity derived from admitted workload UID, node identity, and desired generation. |
| `TenantWorkloadId` | Current node-local systemd identity. NNC6.1c1 replaces it with a projection of `WorkloadExecutionId`, not a second identity authority. |

The derivation domain is `nimbus.workloads.saga.id.v1`. The derivation hashes
length-delimited tenant and workload bytes. An IP address, socket address,
port, PID, provider handle, manifest path, or node-local name never enters the
derivation.

NNC6.1c1 replaces `TenantWorkloadGeneration` with one serializable
`WorkloadGeneration`. The type allows an explicit `u64` value and exposes
`checked_next`. It never uses saturating arithmetic.

`WorkloadSagaRevision` is a separate serializable `u64` type. It also exposes
`checked_next`. A desired generation names desired content. A saga revision
names one successful store transition. The values are not interchangeable.

`WorkloadDesiredDigest` is a canonical lowercase SHA-256 digest of the
upper-layer desired workload encoding. Equal generations are idempotent only
when this digest and the complete network tuple also match.

The required network tuple contains:

```text
NetworkPlanId
NetworkResourceGeneration
NetworkPlanDigest
```

Every record requires all three fields. A workload with no exposed resource
still carries a valid empty `NetworkPlan`. The saga does not encode a partial
tuple.

The intent values are:

```text
WorkloadActivationIntent = PrepareOnly | ActivateWhenAttached
WorkloadPublicationIntent = Withheld | PublishWhenReady
```

The record also contains one `WorkloadSagaTransitionId`. The workloads crate
derives it from `nimbus.workloads.saga.transition.v1` and a canonical encoding
of the complete semantic transition payload. That payload contains the saga
ID, expected and resulting revisions, source and target phases, active intent,
optional successor intent, phase detail, and redacted failure evidence. The
encoding omits only the transition ID slot that it computes. An exact retry
derives the same ID. Any different next-record content derives a different ID.

## Frozen Saga Record

`WorkloadSagaRecord` contains these required values:

- format version.
- saga ID and saga key.
- workload kind and desired state.
- desired generation and desired digest.
- saga revision and current phase.
- complete network plan tuple.
- activation and publication intent.
- admitted workload evidence.
- exact phase detail.
- last committed transition.

The admitted evidence contains the tenant isolation decision ID,
`TenantWorkloadUid`, and optional assigned `NodeIdentity`. Placement may leave
the node absent. Any execution-bearing phase requires the node.

The record may carry one `successorIntent`. It repeats the complete next
generation workload kind, desired state, generation, desired digest, network
tuple, activation intent, publication intent, and admitted evidence. It cannot
contain only part of that set.

Every record requires `phaseDetail` from this closed tagged enum:

| Tag | Required evidence |
| --- | --- |
| `intent` | No effect reference or owner observation. It is valid only for `IntentCommitted`. |
| `provision` | The exact effect-reference and owner-observation sets in the provision matrix. |
| `teardown` | The withdrawal origin, its retained effect-reference set, and the exact cumulative terminal-observation set in the teardown matrix. |
| `cleanup_pending` | The last safe phase, every retained effect reference, and exactly one inspection requirement per reference. |
| `recorded` | The completed generation, desired digest, terminal-evidence digest, and no retained effect reference. |

The effect-reference set has three typed subjects:

| Symbol | Stable subject |
| --- | --- |
| `N` | `network`: plan ID, generation, and digest that select the canonical network-manager record. |
| `E` | `execution`: `TenantWorkloadUid`, `NodeIdentity`, `WorkloadExecutionId`, desired generation, and desired digest. |
| `P` | `publication`: sorted stable `PublishedEndpointId` set plus the complete network tuple. |

These fields reference each canonical owner. They never copy provider handles
or observed provider state into the workload saga. Compute persists a reference
before it calls the associated effect. A reference proves the inspection
subject, not effect success.

Owner observations use a closed tag, the exact matching reference, the owner
phase, and a canonical evidence digest. The provision matrix is exhaustive:

| Saga phase | Required references | Required cumulative owner observations | Forbidden evidence |
| --- | --- | --- | --- |
| `IntentCommitted` | none | none | `E`, `P`, and all observations |
| `NetworkReserved` | `N`, `E` | `NetworkReserved(N)` | `P` and later observations |
| `WorkloadPrepared` | `N`, `E` | prior plus `ExecutionPrepared(E)` | `P` and later observations |
| `NetworkAttached` | `N`, `E` | prior plus `NetworkAttached(N)` | `P` and later observations |
| `WorkloadActivated` | `N`, `E` | prior plus `ExecutionActivated(E)` | `P` and later observations |
| `Ready` with `Withheld` | `N`, `E` | prior plus `Ready(N,E)` | `P` and publication observations |
| `Ready` with `PublishWhenReady` | `N`, `E`, `P` | prior plus `Ready(N,E)` | `PublicationPresent(P)` and later observations |
| `Published` | `N`, `E`, `P` | prior plus `PublicationPresent(P)` | missing or extra references and observations |
| `Observed` with `Withheld` | `N`, `E` | the complete Withheld `Ready` set | `P` and publication observations |
| `Observed` with `PublishWhenReady` | `N`, `E`, `P` | the complete `Published` set | missing or extra references and observations |

`Published` is legal only with `PublishWhenReady`. `Ready` moves directly to
`Observed` when publication is `Withheld`. `PrepareOnly` may remain at
`NetworkAttached`. It cannot enter `WorkloadActivated` until a later generation
authorizes activation.

For teardown, `T` is the exact reference set from the withdrawal origin. The
terminal observation set is cumulative and closed:

| Saga phase | Retained references | Required cumulative terminal observations |
| --- | --- | --- |
| `WithdrawalCommitted` | `T` | none |
| `Withdrawn` | `T` | `PublicationAbsent(P)` exactly when `P` is in `T` |
| `Drained` | `T` | prior plus `ExecutionDrained(E)` exactly when `E` is in `T` |
| `WorkloadStopped` | `T` | prior plus `ExecutionStopped(E)` exactly when `E` is in `T` |
| `NetworkDetached` | `T` | prior plus `NetworkDetached(N)` exactly when `N` is in `T` |
| `NetworkReleased` | `T` | prior plus `NetworkReleased(N)` exactly when `N` is in `T` |
| `Recorded` | none | a canonical digest of the complete terminal set plus completed generation and desired digest |

Each terminal observation contains its exact reference, terminal owner phase,
and owner-evidence digest. A phase with no matching reference treats that step
as a proven no-op and forbids the corresponding observation.

`CleanupPending` requires a non-empty retained reference set. Its ordered
inspection set must have the same subject keys as the retained set, with no
duplicates, omissions, or extra subjects. Each `network`, `execution`, or
`publication` requirement repeats the exact reference, expected owner phase,
generation, and digest. Thus fresh recovery inspects every possibly affected
subject, including a planned effect whose acknowledgement never arrived.

The last transition contains its transition ID, optional source phase, target
phase, active generation, optional successor generation, and resulting saga
revision. The optional failure object contains only a stable failure code and a
digest of redacted evidence. Free-form provider or credential text stays in
logs and observed projections. It never enters the saga or a decision input.

The codec uses format version `1`. It denies unknown format versions or fields.
It also denies non-canonical counters, out-of-range counters, partial intents,
incomplete evidence, invalid digest text, crossed identities, and inconsistent
phase or transition fields. The Engine table schema alone is not sufficient
because it validates only declared JSON types and required fields.

## Frozen Phase Graph

The provision path is:

```text
IntentCommitted
  -> NetworkReserved
  -> WorkloadPrepared
  -> NetworkAttached
  -> WorkloadActivated
  -> Ready
  -> Published -> Observed  (PublishWhenReady)
  -> Observed               (Withheld)
```

The teardown path is:

```text
WithdrawalCommitted
  -> Withdrawn
  -> Drained
  -> WorkloadStopped
  -> NetworkDetached
  -> NetworkReleased
  -> Recorded
```

A desired stop transition can move any provision phase to
`WithdrawalCommitted`. A failed effect can move an effect-bearing phase to
`CleanupPending`. That phase records the last safe phase and the exact
generation-scoped observation needed for reconciliation.

Generation rollover never resurrects an old execution in place. Before
`Recorded`, the coordinator stores a strictly higher desired generation as the
complete `successorIntent`. Compute moves the active generation to
`WithdrawalCommitted` and finishes its teardown with all old fences retained.
A still-higher intent replaces the pending successor only through CAS. An equal
generation must be an exact replay. A lower generation is stale.

At `Recorded`, one CAS promotes the complete successor intent, clears the
successor slot, and advances the saga revision. A promoted Running intent moves
to `IntentCommitted`. A promoted Stopped intent remains `Recorded` with the new
generation and no acquired evidence. With no pending successor, the same rules
accept a new strictly higher intent directly at `Recorded`. Thus one
logical saga ID spans every generation without a delete or a new workload ID.

`CleanupPending` cannot release a port, segment, attachment, route, listener,
or workload identity. Fresh inspection may prove the intended next phase,
return to the last safe phase, or enter `WithdrawalCommitted`. NNC8.3 retains
cleanup finalization and reuse authority.

The workloads crate exposes an exhaustive transition function. It does not
expose an unchecked phase setter. The function denies these transitions:

- a backward provision or teardown edge.
- a stale generation or revision.
- an equal generation with different desired or network content.
- direct generation replacement before teardown reaches `Recorded`.
- a successor promotion that retains old effect references.
- publication before `Ready`.
- `Published` under `Withheld` or activation under `PrepareOnly`.
- a phase/reference/observation combination outside either evidence matrix.
- release before exact detach evidence.
- a `CleanupPending` exit without current inspection evidence.

## Frozen Store Port

`WorkloadSagaStore` is object-safe, `Send`, and `Sync`. It returns boxed futures
without adding Tokio to `nimbus-workloads`.

```rust
pub trait WorkloadSagaStore: Send + Sync + 'static {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>>;

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit>;

    fn list_recoverable<'a>(
        &'a self,
        page: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage>;
}
```

`WorkloadSagaExpected` is `Missing` or `Revision(WorkloadSagaRevision)`.
`WorkloadSagaCommit` is `Applied` or `Unchanged`. The same transition ID and
identical canonical record returns `Unchanged` without another durable commit.
The same transition ID with different content fails closed.

The error taxonomy distinguishes `Conflict`, `Ambiguous`, `Corrupt`,
`Unavailable`, and `InvalidTransition`. A conflict does not return a stale
snapshot as current truth. The coordinator loads the record again.

The store port does not expose mutable access, whole-map snapshot restore,
unconditional upsert, delete, raw Engine values, or provider effects.

## Frozen Durable Home

The physical home is the `_workload_sagas` table in the reserved `_nimbus`
Engine tenant. The table is a server-owned private control record. It is not a
visible `SystemTable` or a rebuildable projection.

The table fields are:

| Field | JSON type | Required |
| --- | --- | --- |
| `formatVersion` | number | yes |
| `sagaId` | string | yes |
| `tenantId` | string | yes |
| `workloadId` | string | yes |
| `workloadKind` | string | yes |
| `desiredState` | string | yes |
| `desiredGeneration` | string | yes |
| `desiredDigest` | string | yes |
| `sagaRevision` | string | yes |
| `phase` | string | yes |
| `phaseDetail` | object | yes |
| `networkPlanId` | string | yes |
| `networkGeneration` | string | yes |
| `networkPlanDigest` | string | yes |
| `activationIntent` | string | yes |
| `publicationIntent` | string | yes |
| `admission` | object | yes |
| `successorIntent` | object | no |
| `lastTransition` | object | yes |
| `failure` | object | no |

Every `u64` counter uses its canonical unsigned decimal string. The codec
allows `0` or a non-zero digit followed by digits, rejects leading zeroes and
values above `u64::MAX`, and round-trips every portable value exactly. This rule
also covers counters nested in `successorIntent`, `phaseDetail`, and
`lastTransition`. `formatVersion` remains the fixed small number `1`.

The strict codec enforces the nested objects defined in the record section.
It rejects unknown nested fields and tags. It also cross-checks phase detail,
active and successor intents, admission and execution identities, transition
payload, and top-level network tuple before returning a portable record.

The exact index set is:

```text
by_tenantId_and_workloadId(tenantId, workloadId)
by_phase(phase)
by_tenantId_and_phase(tenantId, phase)
by_desiredState_and_phase(desiredState, phase)
```

The Engine document ID equals the canonical `WorkloadSagaId`. The adapter
validates agreement among the document ID, saga ID, tenant ID, and workload
ID on every read.

Reserved `_nimbus` routing is the primary access boundary. Requiring the
authenticated system principal in table policy adds defense in depth.
`PrincipalContext::system()` currently uses an ordinary claim. The plan does
not treat that claim as an unforgeable capability.

NNC6.1d must prove that every application protocol and credential binding
rejects reserved tenant selection. Explicit local operator access may inspect
the record. Application principals cannot read, create, update, delete, or
replace its schema.

The single reserved tenant avoids application schema pollution and supports a
bounded cross-tenant recovery scan. Workload transitions are control-plane
writes. NNC6.1d measures their contention under the concurrent saga proof
instead of claiming throughput from design alone.

## Frozen Engine Route And CAS

The server adapter uses `Engine::begin_mutation_execution_unit` with the
reserved tenant and `PrincipalContext::system()`. This is one of the three
canonical Engine-owned mutation paths: queued journal, direct mutation, and
execution unit. The adapter implements a domain store port, but Engine retains
the commit, OCC, index, and journal authority. It does not call raw storage or
create a fourth mutation path.

The queued journal and direct mutation paths remain canonical for their
existing callers. Their public APIs do not bind the saga point read and
conditional write in one unit. Therefore, they cannot implement this CAS.
Rejecting them here does not reject Nimbus's single Engine-owned mutation
authority.

Each CAS attempt follows this sequence:

1. Open one fresh mutation execution unit.
2. Read the canonical document through the unit.
3. Decode and validate the complete current record.
4. Return `Unchanged` for an exact transition replay.
5. Validate the expected revision, active and successor intents, exact phase
   detail, complete transition digest, and legal edge.
6. Stage one whole-record set with `exists(false)` or current `update_time`.
7. Commit the execution unit once.

The point read records the OCC dependency. The write precondition rejects a
concurrent create or update. The Engine commits the document, indexes, and
journal entry in one storage transaction.

The adapter never retries a domain conflict inside the store. The compute
coordinator reloads and recomputes the transition.

The current Engine can classify durable failures internally, but its public
error does not expose a stable ambiguity variant for this adapter. NNC6.1d
conservatively reports any non-conflict error returned from `commit` as
`Ambiguous`. It does not parse error text or roll back an uncertain write.

After `Ambiguous`, a fresh load decides the result:

| Fresh result | Decision |
| --- | --- |
| Exact next record and transition ID | Treat the CAS as applied. |
| Exact old expected record | Retry the same transition ID from a fresh unit. |
| Another revision or transition | Fence the caller and recompute. |
| Missing, corrupt, or unavailable | Fail closed with zero provider effects. |

No workload, network, naming, proxy, or provider effect can run until this
check resolves the intent write.

## Failure And Reconciliation Contract

| Failure point | Required behavior |
| --- | --- |
| Admission or plan compilation fails | Make zero store, lease, and provider calls. |
| CAS conflicts | Reload and recompute before any effect. |
| Intent write fails before a durable result | Make zero effects. |
| Intent write returns an ambiguous result | Fail closed and perform no same-process effect or blind retry. |
| Process dies after intent and before reserve | A fresh process resumes from the durable record. |
| Provider effect succeeds before phase commit | Inspect by stable ID, generation, and digest before adopt or compensation. |
| A stale generation calls the coordinator | Reject it with zero new effects. |
| The store is unavailable | Do not activate, restart, or publish. |
| Cleanup evidence is incomplete | Retain `CleanupPending` and all resource fences. |
| The system projection is missing | Rebuild observation from the saga and provider evidence. Do not infer desire from the projection. |

There is no transaction across the saga table, network node store, and provider
effects. Compute commits intent, calls one idempotent generation-scoped
operation, and commits the resulting phase.

## Dependency And Authority Guard

The target graph adds both edges in NNC6.1c:

```text
nimbus-workloads -> nimbus-network
nimbus-compute -> nimbus-workloads
```

These edges remain forbidden:

```text
nimbus-network   -X-> nimbus-workloads|nimbus-compute|nimbus-server|nimbus-system
nimbus-workloads -X-> nimbus-engine|nimbus-server|nimbus-system
nimbus-compute   -X-> nimbus-server|nimbus-storage
```

`nimbus-server` already depends on Engine, compute, workloads, and system. The
adapter needs no new workspace edge.

The source guard must reject these authority leaks:

- saga vocabulary or a store implementation in network or system.
- `_workload_sagas` in `SystemTable` or `nimbus-system` source.
- raw storage writes from the server adapter.
- a queued or direct mutation route for saga CAS.
- a second production transition writer.
- a production in-memory desired store.
- workload identity derived from an address, port, PID, or provider handle.

NNC6.1c must narrow NNCV026 before it adds the planned compute dependency and
coordinator. Its `early-workloads-dependency` mutation becomes obsolete. Its
second-coordinator rule must continue to reject duplicate node coordinators
without rejecting the distinct compute-owned saga coordinator.

## Implementation Assignment

| Proof obligation | Owning item |
| --- | --- |
| Portable IDs, complete transition-payload digest, phases, generation rollover, exhaustive legal edges, serialization, overflow, and in-memory conformance adapter | NNC6.1c |
| Portable `WorkloadExecutionId` | NNC6.1c |
| Removal of node-local identity authority | NNC6.1c1 |
| Closed active/successor intent, admission, phase-detail, effect-reference, owner-observation, and inspection-requirement variants | NNC6.1c |
| Both direct dependency edges and cycle proof | NNC6.1c |
| Removal of service and CLI in-memory authorities | NNC6.1c1 |
| Server codec, lossless decimal-counter wire form, schema bootstrap, reserved access boundary, execution-unit CAS, atomic commit, and required production store injection | NNC6.1d |
| Missing and current-revision contention | NNC6.1d |
| Exact replay, divergent replay, stale generation, and ambiguous commit recovery | NNC6.1d |
| Fresh Engine and fresh-process recovery from every tagged phase detail without snapshot handoff | NNC6.1d and NNC6.1e |
| Higher-generation successor withdrawal and exact promotion at `Recorded` | NNC6.1e |
| Lazy activation and restart decisions through compute | NNC6.1e and NNC6.4a |
| Tenant teardown withdrawal before effects | NNC6.1e and NNC6.5 |
| Desired intent before sandbox start or network reserve | NNC6.1d and NNC6.3 |
| Provider-effect recovery through exact inspection | NNC6.1e and NNC6.3 through NNC6.5 |

## Fail-Before Handoff

The item-local contract command is:

```bash
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh decision
```

It verifies every frozen census count, the frozen plan terms, the core-only
network edge, and the negative network/system ownership guard. The target
command must fail:

```bash
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh implementation
```

The expected result is seven named failures:

1. Missing workloads-owned saga vocabulary.
2. Missing workloads-owned store port.
3. Missing workloads to network dependency.
4. Missing compute to workloads dependency.
5. Remaining production in-memory authority.
6. Missing server-owned durable adapter.
7. Lazy activation still bypasses compute.

NNC6.1c through NNC6.1d evolve this helper into NNCV027. Before marking it
green, they add exclusive mutation cases for each authority and route rule.

## Acceptance Ledger

| ID | Verifiable success criterion | Candidate result |
| --- | --- | --- |
| R1 | The proof records 7 reverse dependencies, 1 store implementation, 2 product in-memory authorities, 3 production upsert sites, 54 manager constructors, and 0 recovery readers. | green |
| R2 | The plan freezes the saga key, saga ID, execution ID, typed desired generation, typed revision, desired digest, and complete-payload transition ID. | green |
| R3 | The plan freezes the complete network tuple and activation and publication intent. | green |
| R4 | The plan freezes every provision, teardown, cleanup, and higher-generation rollover phase rule. | green |
| R5 | The plan freezes one object-safe async store port and its exact outcomes and errors. | green |
| R6 | The plan names `_nimbus._workload_sagas`, format version 1, every field, nested evidence contract, lossless counter form, every index, and the document key. | green |
| R7 | The plan selects `Engine::begin_mutation_execution_unit` and rejects the other two mutation routes for CAS. | green |
| R8 | The plan freezes exact replay, OCC conflict, ambiguous result, and fresh-load behavior. | green |
| R9 | The plan assigns every crash, restart, stale-generation, cleanup, and effect-order proof to a later item. | green |
| R10 | The plan assigns both direct dependency edges to NNC6.1c and records every forbidden edge. | green |
| R11 | Network and system contain no workload saga authority, and network retains only its core workspace edge. | green |
| R12 | The plan keeps service naming, policy, proxy, provider effects, system projection, and cluster transport in their current owners. | green |
| R13 | The decision contract passes, and its implementation mode fails with exactly seven named target gaps. | green: decision `1/1`; implementation `0/7` |
| R14 | Plan structure, docs, format, script syntax, and ShellCheck pass with exact results. | green: verifier `27/27`; docs `108`; site `17/17`; lint, syntax, ShellCheck, format, and diff pass |
| R15 | One candidate-frozen Sol/xhigh/fast item review is complete and every finding is dispositioned. | green: full plus sole narrow reviews complete; all findings dispositioned; no third review |

## Structured Review Disposition

The sole full review used GPT-5.6 Sol with xhigh reasoning and fast service
tier. It reported six findings and rated the original candidate incorrect at
confidence `0.96`.

| Finding | Disposition | Correction or evidence |
| --- | --- | --- |
| P1: execution-unit CAS creates a second mutation path. | Rejected. | The current repository contract names queued journal, direct mutation, and execution unit as the three Engine-owned paths. The server adapter calls the Engine API, not storage. The decision now states this distinction explicitly. |
| P1: no later-generation transition exists after `Observed` or `Recorded`. | Accepted. | The phase graph now stores one complete successor intent, withdraws the active generation, and promotes only at `Recorded`. Direct terminal Running and Stopped advances are explicit. |
| P1: optional unstructured phase detail cannot support fresh recovery. | Accepted. | `phaseDetail` is required. Closed tags and exhaustive provision/teardown matrices define every required and forbidden reference and owner observation. Cleanup inspection is exact and provider state stays with its owner. |
| P2: transition IDs omit semantic fields. | Accepted. | The ID now hashes the canonical complete semantic transition payload, excluding only its own output slot. |
| P2: unrestricted `u64` counters cannot round-trip through Float64 JSON numbers. | Accepted. | Every durable `u64` counter uses canonical decimal text with exact parse and range rules. |
| P3: the helper does not verify every census fact. | Accepted. | The helper verifies the exact reverse dependency set, product authority paths, and allowed non-recovery read call set in addition to the existing counts. |

The accepted P3 changes executable verification logic. Therefore, the item
received exactly one narrow review focused on these accepted defects.

The narrow review reported two findings at confidence `0.97`:

| Finding | Disposition | Correction |
| --- | --- | --- |
| P1: the closed tags still do not define required evidence for each concrete phase. | Accepted. | The proof and plan now contain exhaustive provision and teardown matrices. They define exact references, cumulative owner and terminal observations, forbidden evidence, publication branches, and cleanup's one-to-one inspection rule. NNC6.1c-e criteria own exhaustive conformance and recovery tests. |
| P3: the zero-reader search can hide scan failures and misses UFCS/controller reads. | Accepted. | The helper scans the full old store read surface, UFCS forms, and controller reads. It compares the exact two allowed non-recovery projection calls and treats status above `1` as a hard failure. |

The cadence permits no third review. The owner reruns every affected proof and
closes only if all written criteria pass. Documentation-only ledger closeout
after those proofs does not trigger another review.

## First Correction Verification

| Gate | Exact corrected result |
| --- | --- |
| Census | Reverse dependencies `7`; store implementations `1`; product in-memory authorities `2`; production upserts `3`; manager constructors `54`; recovery readers `0`. |
| Decision contract | `1 passed, 0 failed` |
| Target implementation contract | Expected red: `0 passed, 7 failed`; the same seven handoff diagnostics remain exact. |
| Live control-plane verifier | `27 passed, 0 failed` |
| Technical-writing lint | `1` file passed with `0` diagnostics. |
| Script quality | Bash syntax and ShellCheck pass. |
| Rust format | `cargo fmt --all --check` passes. |
| Diff hygiene | `git diff --check` passes. |
| Docs | `108` pages pass the docs gate. |
| Docs site | `17/17` conditions pass. |

## Final Correction Verification

| Gate | Exact final result |
| --- | --- |
| Census | Reverse dependencies `7`; store implementations `1`; product in-memory authorities `2`; production upserts `3`; manager constructors `54`; recovery readers `0`. |
| Decision contract | `1 passed, 0 failed` |
| Target implementation contract | Expected red: `0 passed, 7 failed`; the same seven handoff diagnostics remain exact. |
| Live control-plane verifier | `27 passed, 0 failed` |
| Technical-writing lint | `1` file passed with `0` diagnostics. |
| Script quality | Bash syntax and ShellCheck pass. |
| Rust format | `cargo fmt --all --check` passes. |
| Diff hygiene | `git diff --check` passes. |
| Docs | `108` pages pass the docs gate. |
| Docs site | `17/17` conditions pass. |

| Artifact | SHA-256 |
| --- | --- |
| Full-review staged diff | `47ccadf4bc1ef764d3e782f8be59b6d2e48dd4494cf60e54f84b8af51123d7d7` |
| Sole narrow-review staged diff | `ca12262f7a73b974c0f8f7e1234cb33a01abee89d7d7485699ccc65bd22813bc` |
| Final decision helper | `2452eee9ee9e8e00da441f09d16f4b2ce4dc85ae4d4fffaf3db47b0c2635816c` |

No product source, manifest, dependency edge, provider effect, push, or pull
request entered this item.

## Pre-Review Verification

| Gate | Exact result |
| --- | --- |
| Decision contract | `1 passed, 0 failed` |
| Target implementation contract | Expected red: `0 passed, 7 failed`; all seven diagnostics match the frozen handoff. |
| Live control-plane verifier | `27 passed, 0 failed` |
| Technical-writing lint | `1` file passed with `0` diagnostics. |
| Script quality | Bash syntax and ShellCheck pass. |
| Rust format | `cargo fmt --all --check` passes. |
| Diff hygiene | `git diff --check` passes. |
| Docs | `108` pages pass the docs gate. |
| Docs site | `17/17` conditions pass. |

## Worktree Integrity

All three delegated agents changed zero paths. The owner worktree was clean at
the starting checkpoint before this item began. This item excludes the original
checkout, the `machine-os` companion, pushes, and pull requests.
