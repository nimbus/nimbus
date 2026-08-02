# NNC6.1e Durable Discovery And Recovery Decisions

Status: `complete; R1-R15 and sole structured review green`

Starting checkpoint: `54ea8a34066d92d688741bd3f5f59cd24bbaf018`

Durable audit checkpoint: `daba135061ed9cfd74a777c3d91a1a3aec7a2a80`

NNC6.1e adds the read side required for durable workload recovery without
starting any workload or network effect. It extends the portable saga-store
port with bounded tenant-scoped enumeration, adds one pure compute-owned
next-action decision, and proves that a distinct process can reopen Engine
durability and reproduce the complete phase/action matrix without receiving a
record snapshot.

This is a prospective split of the former omnibus NNC6.1e row. The split was
made before product implementation or structured review because admitted
intent compilation, effect choreography, lifecycle ingress, and final
convergence have different dependencies and owners.

## Audit Verdict

The durable record and store are sound, but production is not yet a workload
saga reconciler:

1. `WorkloadSagaCoordinator` can load, CAS, and list recoverable records, but
   it cannot choose the next lifecycle action.
2. No production recovery reader consumes `list_recoverable`.
3. `WorkloadSagaStore` cannot enumerate all sagas for one tenant. Its recovery
   query deliberately excludes quiescent `Observed`, prepare-only
   `NetworkAttached`, and terminal `Recorded` records.
4. Runtime lazy activation installs `ServiceManager` itself as
   `RuntimeServiceRegistry`; the manager starts a sandbox directly after a
   process-local activation claim.
5. Explicit service start/stop/restart, force definition deletion, standalone
   sandbox lifecycle, tenant teardown, Compose, and node loops still invoke
   lifecycle effects without a durable compute-issued saga command.
6. A valid `WorkloadSagaIntent` requires admitted generation, desired digest,
   network tuple, activation/publication intent, and admission evidence.
   NNC6.2 owns compiling those values. NNC6.1e must not invent a placeholder
   plan merely to move a call site.

These findings make the prior acceptance genuinely red. They do not authorize
moving effects into compute or network.

## Prospective Split And Execution Order

| Item | Dependency | Unit of value | Excluded owner |
| --- | --- | --- | --- |
| NNC6.1e | NNC6.1d | Bounded tenant discovery, pure action selection, bounded recovery planning, distinct-process proof. | Every product/provider effect. |
| NNC6.2 | NNC6.1e | Compile admitted service/sandbox/tenant intent into an exact `NetworkPlan`. | Lifecycle effects and recovery. |
| NNC6.1e1 | NNC6.2 | Compute-owned ingress for lazy and explicit service/sandbox lifecycle requests. | Provision/restart/teardown choreography. |
| NNC6.3/NNC6.4/NNC6.4a/NNC6.5/NNC6.6 | Earlier named dependencies | Provision, attach/activate, restart, teardown, and service-resolution fencing. | Portable store/discovery ownership. |
| NNC6.1e2 | NNC6.1e1 and NNC6.3-NNC6.6 | Startup recovery and tenant-retirement convergence against the original all-phase acceptance. | NNC8.3 cleanup finalization and reuse. |

Each row is one candidate-frozen review unit. Internal implementation slices,
test batches, or autoreview chunks do not become new items.

## Ownership

```text
nimbus-workloads
  portable record + CAS port + recovery page + tenant page
           |
           v
nimbus-compute
  pure next-action decision + bounded decision pages
           ^
           | Arc<dyn WorkloadSagaStore>
nimbus-server
  private Engine adapter + distinct-process durability proof
           |
           v
nimbus-engine
  durable document/index/journal truth
```

The effect path remains separate:

```text
compute decision (this item: values only)
          |
          +-- later NNC6.3  -> provision owners
          +-- later NNC6.4a -> restart owners
          +-- later NNC6.5  -> teardown owners
```

`nimbus-services` retains logical names, binding snapshots, definitions,
sessions, and service observations. `nimbus-sandbox` and `nimbus-node` retain
workload effects and typed observations. `nimbus-network` retains network
resource lifecycle and still depends only on `nimbus-core`.

## Frozen Scope

NNC6.1e owns:

- a portable tenant cursor, request, page, and store method;
- tenant-page conformance for two test implementations and the one server
  implementation;
- a pure exhaustive compute action enum and selector;
- bounded recovery-page planning with stable cursor propagation;
- exact active/successor and cleanup-retention decisions;
- a distinct-process Engine-backed phase/action proof;
- item-local verifier mode, mutation cases, proof, and ledger closeout.

NNC6.1e does not own:

- constructing a product `WorkloadSagaIntent`;
- service names, definitions, readiness, or binding resolution;
- tenant admission or policy;
- sandbox/node start, stop, restart, inspect, or artifact cleanup;
- network reserve, attach, detach, release, or provider inspection;
- publication, forwarding, proxy, PEP, certificate, listener, Netavark, nft,
  gvproxy, Iroh, cluster, machine, or cloud-provider effects;
- startup command dispatch or tenant deletion cutover;
- cleanup-result mutation, finalization, fence release, or capacity reuse;
- a transaction spanning Engine, the network store, and effects.

NNC8.3 remains the only later owner allowed to resolve `CleanupPending` into
final cleanup/release/reuse. This item may return only an inspection-and-retain
decision for that phase.

## Frozen Source Allowlist

### Portable discovery

```text
crates/nimbus-workloads/src/lib.rs
crates/nimbus-workloads/src/store.rs
crates/nimbus-workloads/src/store/tests.rs
```

### Pure compute decision plane

```text
crates/nimbus-compute/src/workload_saga.rs
crates/nimbus-compute/src/workload_saga/recovery.rs
crates/nimbus-compute/src/workload_saga/tests.rs
crates/nimbus-compute/src/workload_saga/recovery/tests.rs
```

The child module names may be refined only before their first edit and must
remain concept-owned. A generic `helpers.rs`, `utils.rs`, or god provider is
not admitted.

### Server adapter and process proof

```text
crates/nimbus-server/src/workload_saga_store.rs
crates/nimbus-server/src/workload_saga_store/recovery.rs
crates/nimbus-server/src/workload_saga_store/tenant_enumeration.rs
crates/nimbus-server/src/workload_saga_store/tests/mod.rs
crates/nimbus-server/src/workload_saga_store/tests/recovery.rs
crates/nimbus-server/src/workload_saga_store/tests/composition.rs
```

The existing `by_tenantId_and_workloadId(tenantId, workloadId)` index is the
first-choice physical seam. `schema.rs` is not admitted unless a fail-before
query/index proof demonstrates that this existing index cannot bound the
tenant page. If such evidence appears, amend this proof before editing schema.

### Verification and recovery state

```text
scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
scripts/verify-nimbus-network-control-plane.sh
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/README.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e-durable-discovery-recovery-decisions.md
```

Initially forbidden product paths include all `nimbus-services` manager
lifecycle modules, `nimbus-sandbox`, `nimbus-node`, `nimbus-machine`,
`nimbus-network`, `nimbus-system`, proxy/egress, CLI lifecycle, and provider
implementations. A discovered need outside the allowlist requires an explicit
audit amendment before the edit.

## Tenant Discovery Contract

The recovery cursor and tenant cursor are intentionally different:

| Cursor | Purpose | Includes quiescent records | Stable order key |
| --- | --- | --- | --- |
| `WorkloadSagaRecoveryCursor` | Global crash/startup recovery work. | No. | Immutable `WorkloadSagaId`. |
| `WorkloadSagaTenantCursor` | Complete tenant retirement/inventory input. | Yes. | Tenant-qualified `WorkloadSagaKey` / `WorkloadId`. |

The portable additions are:

```text
WorkloadSagaTenantCursor
WorkloadSagaTenantPageRequest
WorkloadSagaTenantPage
WorkloadSagaStore::list_for_tenant(tenant_id, request)
```

Required invariants:

1. Limits are in `1..=MAX_WORKLOAD_SAGA_PAGE_SIZE`.
2. Every returned record belongs to the requested tenant.
3. Records are strictly ordered by immutable logical workload key.
4. A cursor belongs to the requested tenant and is strictly before every
   returned record.
5. Duplicate, unsorted, crossed-tenant, regressing, over-limit, empty-with-more,
   and malformed pages fail closed.
6. `has_more` creates a cursor only from the final returned record.
7. `Observed`, prepare-only `NetworkAttached`, `Recorded`, and
   `CleanupPending` records remain visible.
8. The server query performs at most the requested page plus one lookahead row
   of indexed work.
9. A phase or generation change cannot change the tenant-page order key.
10. Store unavailability or corrupt physical records returns a typed error and
    no partial authoritative page.

The method is read-only. It cannot acquire an effect authority, transition a
record, or infer desired state from a service manager, manifest, IP address,
provider handle, or system projection.

## Pure Recovery Decision Contract

`WorkloadSagaAction` is a closed compute-owned value. Selection is pure: one
validated `WorkloadSagaRecord` in, one exact action or quiescent result out.
It performs no store call, wall-clock read, randomness, logging side effect,
provider inspection, or mutation.

| Current phase | Exact decision |
| --- | --- |
| `IntentCommitted` | reserve the exact network reference derived from active intent |
| `NetworkReserved` | prepare the exact execution reference |
| `WorkloadPrepared` | attach the exact network reference |
| `NetworkAttached` | activate exact execution, or quiesce for `PrepareOnly` |
| `WorkloadActivated` | inspect exact execution/network readiness |
| `Ready` | publish exact endpoints, or advance-without-effect to `Observed` when publication is withheld |
| `Published` | observe the exact publication reference |
| `Observed` | quiesce; a higher intent must first CAS the record to withdrawal |
| `WithdrawalCommitted` | withdraw exact publication, or advance-without-effect when none was retained |
| `Withdrawn` | drain exact execution, or advance-without-effect when none was retained |
| `Drained` | stop exact execution, or advance-without-effect when none was retained |
| `WorkloadStopped` | detach exact network, or advance-without-effect when none was retained |
| `NetworkDetached` | release exact network, or advance-without-effect when none was retained |
| `NetworkReleased` | record the exact terminal evidence digest |
| `Recorded` | promote only the exact queued higher-generation successor, otherwise quiesce |
| `CleanupPending` | inspect every exact retained reference and retain all fences |

The selector must expose the target phase for every advance and the exact
typed reference/inspection requirement for every owner action. It cannot
return a bare string or untyped provider payload.

Special rows:

- a stale or equal-divergent intent is rejected by the existing record API
  before selection;
- a queued successor exists only after active-generation withdrawal has been
  durably committed;
- promotion is legal only at `Recorded` and consumes the exact queued intent;
- `CleanupPending` cannot activate, publish, release, record, promote, or
  replace a successor;
- unknown or unavailable effect evidence selects inspection/retention, never
  blind retry;
- an IP address is never a workload or action identity.

## Bounded Recovery Reader

The compute coordinator gains a read-only method that consumes one
`WorkloadSagaPageRequest`, calls `list_recoverable` once, and returns an ordered
page of `(record identity, revision, action)` decisions plus the same stable
next cursor.

It must:

- make exactly one bounded store page call;
- validate every record before selection;
- preserve store order and cardinality;
- return store unavailable/corrupt errors without partial actions;
- produce no CAS or provider effect;
- treat quiescent records as absent from global recovery by the existing
  `requires_recovery` contract, while the pure selector remains total for
  explicit loads and tenant pages.

Production startup dispatch remains NNC6.1e2-owned. This item creates a
substitutable read/decision seam, not an unbounded background loop.

## Distinct-Process Proof

The Engine-backed proof lives in `nimbus-server`, where the private adapter is
available. It uses the existing bounded subprocess crash harness rather than a
new dependency from `nimbus-network` or `nimbus-workloads` to
`nimbus-testing`.

Required protocol:

1. A writer child opens a fresh Engine root, prepares the exact schema, and
   persists the independently declared phase/variant fixture matrix.
2. The child acknowledges a named post-durability boundary and parks.
3. The parent kills and reaps that exact child under a bounded deadline.
4. A separately spawned recovery child opens only the same Engine root and
   constructs a fresh store/coordinator.
5. Fixed fixture keys are defined in test code in both roles. No serialized
   record, action, snapshot, `Arc`, service-manager map, handle, or store object
   crosses argv, environment, stdin, stdout, or a temp handoff file.
6. The recovery child loads/plans every named phase/variant, returns only an
   ordered count and digest, and exits.
7. The parent checks distinct process identities, exact count/digest, bounded
   wait, successful reap, and diagnostic capture.

Minimum matrix:

- all 16 phases;
- `NetworkAttached` activate and prepare-only branches;
- `Ready` publish and withheld branches;
- `Recorded` quiescent and exact-successor promotion branches;
- every provision-origin higher-generation update, which must first select
  active-generation withdrawal;
- `CleanupPending` with network, execution, publication, and complete combined
  retained-reference sets;
- unavailable/unknown inspection rows that retain rather than retry/release.

## Failure And Reconciliation Table

| Failure point | Required result |
| --- | --- |
| Tenant cursor tenant differs from request | Reject before query/effect. |
| Tenant page is duplicated, unsorted, crossed, or over limit | Reject the entire page as corrupt/invalid. |
| Store is unavailable | Return typed unavailable; no partial actions. |
| Physical record is corrupt | Return typed corrupt; no inferred action. |
| Page advances phase between calls | Immutable logical cursor prevents duplicate/regressing identity. |
| Record is quiescent | Tenant enumeration includes it; global recovery excludes it; explicit selection returns quiescent. |
| Higher generation arrives | Existing CAS rules queue/replace the successor and withdraw active generation; selector never promotes early. |
| `CleanupPending` evidence is incomplete or unknown | Inspect/retain every fence; no release or reuse. |
| Writer dies after durability | Distinct recovery process derives the same decision from Engine. |
| Writer dies before durability | No record is discoverable and no action is emitted. |
| Child stalls | Harness terminates and reaps it with bounded diagnostics. |
| Parent attempts snapshot handoff | Static/behavior proof fails. |

## Fail-Before Contract

Add `recovery-decisions` mode to
`scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh`.
Before product implementation it must report exactly:

```text
FAIL workload-saga-authority missing tenant-scoped workload-saga paging
FAIL workload-saga-authority missing pure compute workload-saga action selector
FAIL workload-saga-authority missing bounded compute recovery decision reader
FAIL workload-saga-authority missing distinct-process all-phase recovery proof
Summary: 0 passed, 4 failed
```

The historical `implementation` mode remains independently red for NNC6.1e1's
lazy-activation cutover. Current NNC6.1e does not weaken or claim it.

Behavioral fail-before tests land before production code and must demonstrate:

- tenant paging currently does not compile/exist;
- every phase/variant currently lacks an action decision;
- no bounded coordinator page produces decisions;
- the Engine-backed child cannot run the required all-phase recovery path.

At completion, `recovery-decisions` is `1 passed, 0 failed`, NNCV027 includes
it, and exclusive mutations prove that each of the four seams plus cursor
ordering, quiescent inclusion, cleanup retention, successor promotion, and
snapshot-handoff guards fails NNCV027 alone.

### Recorded Fail-Before Evidence

The behavioral red was captured before the corresponding implementation in
each disjoint owner lane:

| Seam | Exact red | Intended missing behavior |
| --- | --- | --- |
| Portable tenant page | `24 passed, 2 failed` | Both portable test stores returned an empty placeholder rather than the tenant's durable saga inventory. |
| Pure compute decisions | `15 passed, 5 failed` | The selector returned incorrect/empty phase actions and the bounded reader did not return the required decision page. |
| Engine tenant adapter | `0 passed, 1 failed` | The durable record existed but tenant enumeration returned an empty page. |
| Distinct-process digest | `0 passed, 1 failed` | The complete recovered matrix produced the exact digest while the frozen expectation still named `matrix-30-pending`. |

The first process-proof compile attempt used the wrong public import path and
is not counted as behavioral red. After that path was corrected, the digest
mismatch was the intended behavioral proof: writer PID `27449` was killed and
reaped at the named post-durability boundary, recovery PID `27450` reopened the
same Engine root, and the child emitted the stable recovered digest without a
record or snapshot handoff.

## Implemented Seams

### Portable tenant discovery

- `WorkloadSagaTenantCursor` owns an immutable tenant-qualified
  `WorkloadSagaKey`; it is deliberately distinct from the global recovery
  cursor and is stable across phase or generation transitions.
- `WorkloadSagaTenantPageRequest` validates the bounded limit and rejects a
  cursor from another tenant before an adapter query.
- `WorkloadSagaTenantPage` revalidates every record and rejects crossed,
  malformed, duplicate, unsorted, regressing, over-limit, empty-with-more, and
  partial-with-more results as one failed page.
- `WorkloadSagaStore::list_for_tenant` is object safe and implemented by the
  two portable conformance stores plus the private Engine adapter.

### Pure compute recovery decisions

- `WorkloadSagaAction` is a closed provider-neutral value enum. The exhaustive
  selector covers all 16 durable phases and the prepare-only, withheld
  publication, no-reference teardown, running/stopped successor, quiescent,
  and cleanup-retention branches.
- `WorkloadSagaDecision` binds the action to tenant/workload key, saga ID,
  revision, active generation, and exact target phase. Provider payloads and IP
  addresses never become workload identity.
- `plan_recoverable_page` performs exactly one bounded store read, preserves
  order/cardinality/cursor, and has no CAS, provider, clock, randomness, or
  other effect capability.

### Engine adapter and process proof

- The private adapter uses the existing
  `by_tenantId_and_workloadId(tenantId, workloadId)` composite index with exact
  tenant equality, optional workload greater-than cursor, ascending workload
  order, system principal, and limit-plus-one lookahead. It rejects any
  physical over-read or decoded crossed/malformed/unsorted result.
- A writer child persists 30 independently named phase/variant histories,
  proves every transition was newly applied, reaches
  `workload-saga.phase-matrix-durable`, and is killed and reaped.
- A distinct recovery child receives only the Engine state root and role mode,
  constructs a fresh store/coordinator, loads the fixed keys, and reproduces
  `matrix-30-44ad8f3b626c2b503441c96a2bbb88181f1f3887040947e7c0323e533f2a9ea3`.
  The matrix covers every phase, prepare-only/withheld branches, both successor
  kinds, every provision-origin higher-generation withdrawal, and retained
  cleanup references. No record, action, snapshot, `Arc`, handle, or manager
  map crosses the process boundary.

## Acceptance Ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| R1 | The source-derived caller/effect census and prospective split are recorded before product edits. | green |
| R2 | Tenant paging uses a distinct tenant-qualified immutable cursor and includes every quiescent phase. | green |
| R3 | Portable page validation rejects every crossed, duplicate, unsorted, regressing, malformed, and over-limit case. | green |
| R4 | Two test store implementations and the server Engine adapter pass the same tenant-page contract. | green |
| R5 | The server tenant query is indexed and bounded to limit plus one without schema churn when the existing index suffices. | green |
| R6 | One pure closed compute action selector covers all 16 phases and every activation/publication/successor/cleanup branch. | green |
| R7 | Every action carries exact typed identity/reference, revision/generation context, and target phase; no IP or provider payload becomes workload identity. | green |
| R8 | `CleanupPending` selects complete inspection/retention only and cannot release, reuse, activate, publish, record, or promote. | green |
| R9 | Higher generations withdraw the active generation and promote only the exact queued successor at `Recorded`; stale/equal-divergent inputs make zero actions/effects. | green |
| R10 | One bounded coordinator page returns ordered decisions and the exact store cursor with no CAS or effect capability. | green |
| R11 | Store unavailable/corrupt/ambiguous inputs fail closed without partial actions or inferred truth. | green |
| R12 | A killed writer and distinct recovery child reopen Engine durability with no record/snapshot handoff and reproduce the exact phase/action digest. | green |
| R13 | Effects remain in their current owners; network remains core-only/effect-free; no new dependency cycle or `nimbus-network -> nimbus-testing` edge appears. | green |
| R14 | `recovery-decisions`, live NNCV027, exclusive mutations, script syntax/ShellCheck, and plan/static verifiers pass exact recorded counts. | green |
| R15 | Focused/full affected tests, check, strict Clippy, warning-denied rustdoc, format/diff, and docs gates pass with exact recorded evidence. | green |

No row becomes green from compilation alone. R2-R12 require happy, edge,
error, and relevant crash/fencing behavior. The structured review is a
subsequent item-closeout gate, not a self-referential prerequisite for starting
that same review.

## Candidate Verification Evidence

| Gate | Exact result |
| --- | --- |
| Portable focused store | `26 passed, 0 failed` |
| Compute saga/recovery focused | `20 passed, 0 failed` |
| Server workload-saga store | `28 passed, 0 failed, 1 ignored` (the ignored child is spawned only by the parent process proof) |
| Full `nimbus-workloads` | `73 passed, 0 failed` |
| Full `nimbus-compute` | `94 passed, 0 failed` |
| Total affected behavior | `195 passed, 0 failed, 1 intentional child-only ignore` |
| Affected all-target/all-feature check | pass |
| Affected all-target/all-feature strict Clippy | pass; item-owned code has zero warnings |
| Affected no-dependency warning-denied rustdoc | pass |
| Direct `recovery-decisions` | `1 passed, 0 failed` |
| Live network verifier | `28 passed, 0 failed` |
| Focused NNC6.1e mutations | `10 passed, 0 failed` |
| Full aggregate mutations | `198 passed, 0 failed`; an earlier continuously progressing run expired at its insufficient 300-second bound and a later 1,800-second bound expired during NNCV021, neither was classified as a failure |
| Bash syntax | pass for both changed scripts |
| ShellCheck | NNC6.1e contract passes directly; aggregate passes with its documented inherited `SC2034,SC1091` exclusions |
| Cargo format and diff check | pass |
| Docs and site | `108` link-clean pages; `17/17` site conditions |

## Verification Plan

Fail-before and focused loop:

```bash
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh recovery-decisions
cargo test -p nimbus-workloads store
cargo test -p nimbus-workloads saga
cargo test -p nimbus-compute workload_saga
cargo test -p nimbus-server workload_saga_store
```

Candidate closeout:

```bash
cargo test -p nimbus-workloads
cargo test -p nimbus-compute
cargo test -p nimbus-server workload_saga_store
cargo check -p nimbus-workloads -p nimbus-compute -p nimbus-server --all-targets --all-features
cargo clippy -p nimbus-workloads -p nimbus-compute -p nimbus-server --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-workloads -p nimbus-compute -p nimbus-server --no-deps
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/verify-nimbus-network-control-plane.sh --self-test
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
cargo fmt --all --check
git diff --check
```

Commands are bounded where they can hang, use the shared Cargo target, and are
not piped through commands that hide the real exit code. Exact executed/pass/
skip counts are recorded at closeout; a zero-test or skipped provider lane is
not passing evidence.

## Review Cadence

No structured autoreview runs during audit, fail-before, implementation,
cleanup, or acceptance convergence. After R1-R15 are green and the complete
NNC6.1e item is candidate-frozen, run exactly one full structured review with
GPT-5.6 Sol, xhigh reasoning, fast mode. If and only if an accepted finding
materially changes executable code, rerun affected proofs and exactly one
narrow correction review focused on that defect. Documentation wording,
formatting, elapsed time, or internal review chunks do not justify a repeat.

### Review Outcome

Exactly one full structured item review ran against staged tree
`8c9d522263522a874848e6a9516c86fcad931e86`. The actual reviewer was Codex
with `gpt-5.6-sol`, `xhigh` reasoning, and `service_tier="fast"`; web search was
off, the bundle was `187672` bytes, and the helper executed one pass with no
fallback. It reported zero findings, `patch is correct`, and confidence
`0.90`. No finding was accepted because none was reported, so no executable
correction and no narrow correction review are warranted.

The executable/script patch SHA-256 is
`27e31fb78c4c66c4b49940920842d775ce038c01f2bc183e666215480239af5e`.
The post-review edits are ledger/routing closeout only and do not justify a
repeat review under the frozen cadence.

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Current item | NNC6.1e |
| Last durable commit | `2204fa8d7a886b3557709932f02944961c629c4b` (NNC6.1e exact reviewed item) |
| Current owned paths | Frozen NNC6.1e allowlist: portable store paging, pure compute decisions, private Engine adapter/process proof, verifier scripts, and canonical plan/proof ledgers. |
| Last green | Product behavior `195/195` with one child-only ignore; live verifier `28/28`; focused mutations `10/10`; aggregate mutations `198/198`; affected check/Clippy/rustdoc; syntax/ShellCheck; format/diff. |
| Current expected red | None for NNC6.1e. Historical `implementation` mode remains red solely for NNC6.1e1's later-owned runtime lazy-activation cutover. |
| Next action | Commit the exact reviewed item, record its durable hash in the routing transition, then begin NNC6.2's read-only admitted-plan compiler audit. |
| Blocker | none |
| Review | one full Sol/xhigh/fast pass; zero findings; confidence `0.90`; no correction review |
