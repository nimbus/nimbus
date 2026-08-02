# NNC6.1d Durable Workload Saga Store

Status: `acceptance complete; exact item commit pending`

Starting checkpoint: `a098db7b5ca83acae9f6d3ee8046826a4d60bd67`

NNC6.1d adds the first production implementation of the portable
`WorkloadSagaStore`. The implementation is a private server adapter over an
Engine mutation execution unit. It also makes the store a required part of
workload-capable compute composition. This item persists and resolves saga
transitions; it does not execute workload, network, publication, or cleanup
effects.

## Audit Result

The existing seams are sufficient. `nimbus-workloads` owns the validated
record, phase graph, CAS vocabulary, and bounded page contract.
`nimbus-compute` owns the only transition coordinator. `nimbus-server` already
depends on workloads, compute, and Engine, so it can implement the durable
adapter without a new workspace edge. `Engine::begin_mutation_execution_unit`
provides one snapshot, point-read dependencies, whole-record staging, OCC, and
one atomic document/index/journal commit.

The audit found four corrections that must precede product edits:

1. Workload-capable compute currently requires only a network manager. The
   managed profile must also carry `Arc<dyn WorkloadSagaStore>` and construct
   exactly one compute-owned coordinator.
2. The portable recovery rank is private. Workloads must expose one ordered
   phase API for reconciliation priority, but mutable phase cannot participate
   in the storage cursor.
3. `by_phase(phase)` bounds returned rows but can force a provider to scan the
   whole phase before applying the saga cursor. `(phase, sagaId)` still mixes
   recoverable records with quiescent records in the same phase. The corrected
   index is `by_recovery(recoveryEligible, sagaId)`, where the strict codec
   verifies the projection against `requires_recovery` and immutable saga
   identity makes inter-page phase changes duplicate-free.
4. Reserved-tenant predicates are duplicated as `_` and `_nimbus`. The
   canonical pure name classification belongs on `TenantId`: every identifier
   beginning with `_` is Nimbus-reserved. Protocol authorization stays with
   each adapter. The native `/ws` route is protected by local-operator policy
   and remains an explicit operator path.

## Ownership And Composition

```text
nimbus-workloads
  record + transition validation + CAS/page port + recovery order
            |
            v
nimbus-compute
  sole WorkloadSagaCoordinator and ambiguity resolution
            ^
            | Arc<dyn WorkloadSagaStore>
nimbus-server
  private codec + schema + EngineWorkloadSagaStore + composition
            |
            v
nimbus-engine
  execution-unit OCC + atomic document/index/journal commit
```

The dependency shape remains:

```text
nimbus-server -> nimbus-compute -> nimbus-workloads -> nimbus-network
             \-> nimbus-engine                              |
             \-> nimbus-workloads                           v
                                                    nimbus-core
```

`nimbus-network -> nimbus-core` remains the network crate's only outgoing
workspace edge. Neither workloads nor compute gains an Engine, server, system,
storage, or provider dependency.

## Frozen Scope

NNC6.1d owns:

- the server-private Engine store, physical codec, exact schema, and recovery
  query adapter;
- sequential no-churn schema bootstrap and divergent-schema refusal;
- missing/current CAS contention, exact replay, transition rejection,
  atomicity, and ambiguous-outcome proofs;
- a mandatory fresh read in the compute coordinator after an ambiguous CAS;
- managed-versus-protocol-only compute composition;
- the workload-owned public recovery order plus immutable saga-ID cursor and
  physical recovery index;
- one canonical pure reserved-tenant name predicate and application credential
  resolution at the confirmed gaps;
- NNCV027 wiring, item proof, and closeout ledgers.

NNC6.1d does not own:

- service lazy activation or lifecycle call routing;
- workload start, stop, restart, or sandbox inspection;
- network reserve, attach, detach, release, or provider inspection;
- endpoint publication, withdrawal, name resolution, or readiness;
- proxy, egress, listener, Netavark, nftables, gvproxy, or cluster effects;
- system observed projections;
- cleanup, compensation, finalization, or resource reuse;
- a transaction spanning the saga store, network store, and provider effects.

Those remain with NNC6.1e and the later named owners.

## Frozen Source Allowlist

### Portable order and composition

```text
crates/nimbus-workloads/src/saga.rs
crates/nimbus-workloads/src/store.rs
crates/nimbus-workloads/src/store/tests.rs
crates/nimbus-compute/src/state.rs
crates/nimbus-compute/src/workload_saga.rs
crates/nimbus-compute/src/workload_saga/tests.rs
```

### Server adapter

```text
crates/nimbus-server/src/lib.rs
crates/nimbus-server/src/state.rs
crates/nimbus-server/src/router.rs
crates/nimbus-server/src/workload_saga_store.rs
crates/nimbus-server/src/workload_saga_store/codec.rs
crates/nimbus-server/src/workload_saga_store/schema.rs
crates/nimbus-server/src/workload_saga_store/recovery.rs
crates/nimbus-server/src/workload_saga_store/tests/mod.rs
crates/nimbus-server/src/workload_saga_store/tests/ambiguity.rs
crates/nimbus-server/src/workload_saga_store/tests/codec.rs
crates/nimbus-server/src/workload_saga_store/tests/store.rs
crates/nimbus-server/src/workload_saga_store/tests/durability.rs
crates/nimbus-server/src/workload_saga_store/tests/composition.rs
crates/nimbus-server/src/workload_saga_store/tests/recovery.rs
crates/nimbus-server/src/tests/local_server_security.rs
crates/nimbus-server/src/tests/runtime_owner_conformance.rs
```

### Reserved application tenants

```text
crates/nimbus-core/src/types.rs
crates/nimbus-system/src/identity.rs
crates/nimbus-cloud-functions/src/http/tenant_binding.rs
crates/nimbus-convex/src/silo_auth.rs
crates/nimbus-convex/src/tenancy.rs
crates/nimbus-dynamodb/src/tenant.rs
crates/nimbus-firebase/src/project_tenant_registry.rs
crates/nimbus-kv/src/server.rs
crates/nimbus-mongodb/src/credential_registry.rs
crates/nimbus-mongodb/src/commands/tenant.rs
crates/nimbus-s3/src/auth.rs
```

### Verification and recovery state

```text
scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
scripts/verify-nimbus-network-control-plane.sh
scripts/verify-nimbus-network-source-contract.mjs
docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1b-workload-saga-vocabulary-store-durable-home.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1d-durable-workload-saga-store.md
docs/private/plans/README.md
```

No Engine, storage, network, system-schema, services-lifecycle, sandbox,
proxy, egress, cluster, CLI, or provider source is in the allowlist. A source
need outside this list requires an audit amendment here before the edit.

The source-contract helper and bind inventory are a narrow convergence
amendment discovered by the aggregate gate after product composition. NNCV025
and NNCV026 must assert the stronger `ProtocolOnly`/`Managed` profile rather
than their deleted optional-manager syntax, while NNCV006 must refresh only the
five source-derived occurrence lines moved by the reserved-tenant edits. This
amendment changes no listener classification or prior ownership decision.

The two existing server integration tests are a second narrow convergence
amendment for the already-merged trusted-tenant PRs #238 and #239. The full
NNC6.1d affected suite proved that their old fixtures still omitted the now
mandatory Cloud Functions tenant binding and Convex deploy silo. Updating only
those fixtures is required to preserve their original security assertions; it
does not alter either trust boundary or add product behavior to NNC6.1d.

## Compute Composition Contract

`ComputeStateConfig` uses one explicit profile value:

```text
ProtocolOnly

Managed {
  LocalNetworkManager,
  Arc<dyn WorkloadSagaStore>,
}
```

The managed form is the only form that can construct a
`WorkloadSagaCoordinator`. The protocol-only form has neither manager nor
coordinator. Node services that require workload lifecycle still fail before
capability use when paired with `ProtocolOnly`. Server constructs the concrete
Engine adapter at the `AppState` to `ComputeState` handoff. Compute sees only
the portable store port and constructs the sole coordinator.

The coordinator resolves an `Ambiguous` CAS by performing exactly one fresh
point load before returning:

- exact next record: return an applied durable outcome;
- exact old record or missing record: preserve `Ambiguous` and permit no
  downstream effect;
- competing valid revision: return a typed conflict with observed revision;
- corrupt or unavailable truth: return the corresponding fail-closed error.

It does not retry the CAS. NNC6.1e must consume this coordinator path rather
than the raw store.

## Physical Schema And Codec

The record lives in Engine tenant `_nimbus`, table `_workload_sagas`. It is not
a `SystemTable`. The document ID is the canonical `WorkloadSagaId`.

The exact format-v1 fields are:

```text
formatVersion:number
sagaId:string
tenantId:string
workloadId:string
workloadKind:string
desiredState:string
desiredGeneration:string
desiredDigest:string
sagaRevision:string
phase:string
recoveryEligible:boolean
phaseDetail:object
networkPlanId:string
networkGeneration:string
networkPlanDigest:string
activationIntent:string
publicationIntent:string
admission:object
successorIntent?:object
lastTransition:object
failure?:object
```

The exact indexes are:

```text
by_tenantId_and_workloadId(tenantId, workloadId)
by_recovery(recoveryEligible, sagaId)
by_tenantId_and_phase(tenantId, phase)
by_desiredState_and_phase(desiredState, phase)
```

The table policy requires the authenticated identity claim `sub=system` for
read, create, update, and delete. This policy is defense in depth, not an
unforgeable capability. Reserved routing remains the primary boundary.

The codec explicitly maps the flat physical fields to the nested portable
record. It does not serialize `WorkloadSagaRecord` directly as the physical
document. Decode rejects:

- unknown or missing physical and nested fields;
- versions other than `1`;
- non-canonical, leading-zero, negative, fractional, or overflowing counters;
- invalid or non-canonical digests and IDs;
- document ID, saga ID, tenant, or workload disagreement;
- partial or crossed network, admission, execution, publication, successor,
  transition, evidence, inspection, and failure values;
- a top-level desired or network projection that differs from active intent;
- an invalid phase detail or transition.

Every counter remains decimal text and round-trips through `u64::MAX`.

## Schema Bootstrap

Every store operation verifies the exact logical schema. Managed server start
also performs the same preparation eagerly. Preparation follows:

1. Ensure the reserved Engine tenant is ready.
2. Read the current table schema.
3. If absent, install the exact schema once and reread it.
4. Reconcile generated index IDs before comparing logical definitions.
5. If exact, return without another schema write.
6. If divergent, fail closed without replacement.

This proves zero schema churn for sequential exact preparation. Concurrent
processes that both observe an absent table may each submit the same schema
because Engine exposes no create-only schema CAS. They must converge on one
exact logical schema and preserve index identities; NNC6.1d does not claim an
exactly-once cross-process schema commit. Divergent schema is never repaired or
overwritten implicitly.

## CAS Contract

Each CAS performs one blocking execution-unit attempt off the async runtime:

1. Verify exact schema readiness.
2. Open one fresh mutation execution unit for `_nimbus` with
   `PrincipalContext::system()`.
3. Point-read the canonical document and strictly decode it.
4. Return `Unchanged` for exact record replay before testing the stale expected
   revision.
5. Reject a divergent claim of the same transition ID.
6. Verify `Missing` or the exact loaded revision and validate the legal
   successor.
7. Stage one whole-record `Set` with `exists(false)` or the loaded document's
   exact `update_time`.
8. Call `commit()` exactly once.

An Engine conflict maps to domain `Conflict`; a failed fresh observation may
leave `observed=None` rather than fabricate evidence. Every other error returned
from `commit()` maps to `Ambiguous`. Read, codec, validation, and staging errors
remain definitive `Unavailable`, `Corrupt`, or `InvalidTransition`. The adapter
does not parse error text, roll back uncertain truth, retry a domain conflict,
or invoke an external effect.

## Recovery Paging

`nimbus-workloads` publishes the single ordered phase list and recovery rank
for reconciliation priority. It includes the special `Recorded` phase because
`Recorded + successorIntent` requires recovery. The store cursor deliberately
contains only the immutable `WorkloadSagaId`: the server queries one
`(recoveryEligible, sagaId)` window, strictly decodes every returned document,
filters only through `WorkloadSagaRecord::requires_recovery`, and delegates
identity-order validation to `WorkloadSagaPage::new`.

Returned pages and provider index work are bounded by the requested limit plus
one lookahead row. Results are stable, duplicate-free across inter-page phase
changes, and strictly after the input saga identity. A record that becomes
recoverable behind a cursor waits for the next full reconciliation sweep; it is
never recovered twice in the current sweep. `Observed`, prepare-only attached,
and recorded-without-successor records do not appear.

## Reserved-Tenant Contract

`TenantId::is_nimbus_reserved()` is the canonical pure predicate. It returns
true for every identifier beginning with `_`. It does not authorize access.

Application resolution must reject such a tenant in native HTTP, Convex
authenticated and anonymous selection, Cloud Functions, Firebase, DynamoDB,
Cloudflare, S3, MongoDB bound and unbound modes, and standalone KV. The native
`/ws` and Convex `_nimbus` registry are local-operator surfaces guarded by
server-access policy; they remain the explicit inspection exception. No
application credential can authenticate to a reserved tenant.

## Failure And Reconciliation Matrix

| Cut or observation | Required result |
| --- | --- |
| Missing record, valid initial CAS | One applied record. |
| Two missing-record contenders | One `Applied`; one `Conflict`; one durable revision. |
| Two current-revision contenders | One winner; one `Conflict`; no lost update. |
| Exact replay | `Unchanged`; no journal advance. |
| Divergent same transition ID | Typed invalid transition; no write. |
| Stale revision or illegal edge | Typed conflict or invalid transition; no write. |
| Before durable persistence | `Ambiguous`; fresh Engine sees old or missing truth. |
| Durable before publish | `Ambiguous`; fresh Engine sees exact next truth. |
| After publish before fanout | `Ambiguous`; fresh Engine sees exact next truth. |
| Corrupt durable record | `Corrupt`; no overwrite or effect. |
| Unavailable read | `Unavailable`; no write or effect. |
| Divergent schema | Fail closed; no schema replacement or saga effect. |
| Process restart | Fresh Engine reconstructs exact record and recovery page from disk. |

## Fail-Before Contract

Before product corrections, extend NNCV027 with a `durable-store` mode. Its
initial run must fail on the absent concrete adapter, absent managed compute
profile, absent production coordinator, absent strict schema/codec, missing
public recovery order, single-field phase index, missing fresh ambiguity read,
and reserved KV/Mongo gaps.

The pre-existing commands remain:

```bash
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh decision
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh cutover
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh implementation
```

At NNC6.1d completion:

- `decision`, `cutover`, and `durable-store` pass;
- `implementation` remains the exact `0/1` expected red for NNC6.1e's lazy
  activation route;
- product store implementations equal one;
- production coordinator constructions equal one;
- no production in-memory saga store exists.

## Acceptance Ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| R1 | Exact format-v1 schema contains only 21 frozen fields and four frozen indexes; `_workload_sagas` is absent from `SystemTable`. | pass |
| R2 | Repeated exact bootstrap adds no second schema commit, preserves index IDs, and divergent schema fails without replacement. | pass |
| R3 | Strict codec round-trips every phase/detail/evidence shape and `u64::MAX`. | pass |
| R4 | Unknown, malformed, partial, crossed, or inconsistent physical and nested values return `Corrupt`. | pass |
| R5 | Two missing-record contenders yield one applied winner and one conflict. | pass |
| R6 | Two current-revision contenders yield one winner and no lost update. | pass |
| R7 | Exact replay is `Unchanged` and journal-commit-free. | pass |
| R8 | Divergent transition-ID replay, stale generation/revision, and illegal edges fail before commit. | pass |
| R9 | One transition atomically produces its document, four maintained index projections, and one journal commit; pre-persist failure produces none. | pass |
| R10 | Every declared pre/post durable commit cut returns `Ambiguous` without an external effect. | pass |
| R11 | The compute coordinator performs one fresh load and classifies exact-next, exact-old, competing, missing, corrupt, and unavailable ambiguity outcomes. | pass |
| R12 | A fresh Engine/process recovers the exact record and every stored tagged phase detail without snapshot handoff. | pass: the child has an explicit deadline, concurrent output drains, kill-on-drop fallback, and explicit terminate/reap diagnostics; the stalled-child regression passes |
| R13 | Recovery pages use the codec-verified `recoveryEligible` projection and are physically bounded, deterministic by immutable saga identity, cursor-strict, duplicate-free across inter-page phase changes, and include `Recorded + successor`. | pass: one `by_recovery(recoveryEligible, sagaId)` window and an exact phase-transition-between-pages regression prove the immutable fence |
| R14 | Workload-capable compute retains exactly one server store/coordinator; omission is unrepresentable or rejected before effects. | pass |
| R15 | Protocol-only compute has neither manager nor coordinator and does not bootstrap the saga table. | pass |
| R16 | Every application protocol and credential resolver rejects the canonical reserved prefix; local-operator inspection remains available. | pass |
| R17 | Store/coordinator source contains no network, sandbox, service, listener, proxy, provider, or raw-storage effect path. | pass |
| R18 | Dependency metadata preserves all forbidden edges and the exact network core-only edge. | pass |
| R19 | NNCV027 closes only the durable-store gap; lazy activation remains the exact NNC6.1e red. | pass |
| R20 | Focused/full affected tests, check, strict Clippy, rustdoc, format, static/mutation verifiers, and docs gates pass before candidate freeze. | pass: affected `1,539/1,539`, verifier `28/28`, exact-count guard, aggregate `188/188`, quality gates, docs `108`, and site `17/17` |

No criterion may be marked complete from compilation alone. Each behavioral
criterion records its happy, edge, failure, restart, or fencing result and exact
test count in the closeout table below.

## Verification Plan

Focused implementation loop:

```bash
cargo nextest run -p nimbus-workloads workload_saga
cargo nextest run -p nimbus-compute workload_saga
cargo nextest run -p nimbus-compute state
cargo nextest run -p nimbus-server workload_saga_store
cargo nextest run -p nimbus-convex reserved
cargo nextest run -p nimbus-kv reserved
cargo nextest run -p nimbus-mongodb reserved
```

Candidate closeout:

```bash
cargo nextest run -p nimbus-workloads -p nimbus-compute -p nimbus-server -p nimbus-convex -p nimbus-kv -p nimbus-mongodb -p nimbus-dynamodb -p nimbus-firebase -p nimbus-s3 -p nimbus-cloud-functions
cargo check -p nimbus-workloads -p nimbus-compute -p nimbus-server -p nimbus-convex -p nimbus-kv -p nimbus-mongodb -p nimbus-dynamodb -p nimbus-firebase -p nimbus-s3 -p nimbus-cloud-functions --all-targets --all-features
cargo clippy -p nimbus-workloads -p nimbus-compute -p nimbus-server -p nimbus-convex -p nimbus-kv -p nimbus-mongodb -p nimbus-dynamodb -p nimbus-firebase -p nimbus-s3 -p nimbus-cloud-functions --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-workloads -p nimbus-compute -p nimbus-server -p nimbus-convex -p nimbus-kv -p nimbus-mongodb -p nimbus-dynamodb -p nimbus-firebase -p nimbus-s3 -p nimbus-cloud-functions --all-features --no-deps
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh decision
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh cutover
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh durable-store
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh implementation
bash scripts/verify-nimbus-network-control-plane.sh
cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Run exactly one structured GPT-5.6 Sol/xhigh/fast review as a post-acceptance
closeout gate. Start it only after R1-R20 are green and the complete item is
candidate-frozen. An accepted executable finding permits affected proof reruns
and one narrow defect-focused correction review. Documentation wording,
formatting, elapsed time, or internal diff chunking do not justify another
review.

## Closeout Evidence

| Evidence | Result |
| --- | --- |
| Fail-before | `durable-store` exits `1` at `0/35`: all missing adapter, schema/codec, public recovery order, managed composition, fresh ambiguity read, exact authority-count, and canonical reserved-consumer conditions fail. |
| Codec/schema | The 23-test server store lane passes. It covers the 21-field/four-index schema, strict physical and nested codecs, all tagged phase details, maximum counters, sequential no-churn bootstrap, preserved index IDs, and divergent-schema refusal. The final index-ID assertion passes `1/1`. |
| CAS/replay/atomicity | The store lane proves missing and current contention, exact replay, illegal and stale refusal, one document plus four index projections plus one journal commit, and zero effects before persistence. |
| Ambiguity/restart/recovery | The store lane proves all three declared commit cuts plus a post-durability task panic. Fresh Engine and bounded OS-process reopen tests pass; the stalled-child proof terminates and reaps its child. Recovery traverses all 16 phases in stable `4/4/4/3` pages and does not repeat a saga that advances phase between pages. |
| Compute composition | The focused compute lane passes `15/15`. It proves all six ambiguity classifications, one retained managed coordinator, and no CAS retry. The affected suite covers both protocol-only rejection tests. |
| Reserved protocols | The canonical-prefix lane passes `21/21` across core, system, Cloud Functions, Convex, DynamoDB, Firebase, KV, MongoDB, and S3. Cloudflare uses the guarded DynamoDB registry. |
| Dependency/effect census | Live NNCV000-NNCV027 passes `28/28`. Direct source scans find one production store implementation, one production coordinator construction, no forbidden store/coordinator effects, and `nimbus-network -> nimbus-core` as the sole network workspace edge. |
| Focused and affected suites | Server store `23/23`, full workloads `67/67`, compute `15/15`, reserved-prefix `21/21`, and the complete affected matrix `1,539/1,539` with 32 declared skips pass. The two stale PR #238/#239 fixtures pass `2/2` after their trust-boundary-preserving updates. |
| Check/Clippy/rustdoc | Exact affected packages pass `cargo check`, warning-denied Clippy, and warning-denied rustdoc with all targets/features required by each command. Existing vendored Brotli warnings remain non-fatal and unchanged. |
| Verifier/mutations | Live verifier `28/28`; direct `decision`, `cutover`, and `durable-store` each pass `1/1`; `implementation` remains the required NNC6.1e red at `0/1`; the final aggregate verifier self-test passes `188/188`. The NNCV005 mutation now requires the exact `27 passed, 1 failed` child summary and rejects a synthetic `26/1` result. The first final attempt exposed the missing `Last green` recovery field through 134 contaminated exclusivity failures; restoring that required ledger field made a representative NNCV015 child exact at `27/1` and the complete rerun green without weakening a verifier. |
| Format/docs | Rustfmt and `git diff --check` pass. Docs are link-clean at 108 pages, and the site verifier passes `17/17`. |
| Structured review | The sole full GPT-5.6 Sol/xhigh/fast review ran with one P2 and two P3 findings at overall confidence `0.95`; all three are accepted and corrected with the evidence above. The one permitted narrow GPT-5.6 Sol/xhigh/fast correction review is clean with zero findings at confidence `0.97`. No further review is warranted. |
| Durable item commit | pending |

## Recovery Checkpoint

- The starting commit is durable at `a098db7b5`.
- Every visible dirty path and the ignored proof path remain inside the frozen
  allowlist. No Engine, storage, network, system-schema, services-lifecycle,
  sandbox, proxy, egress, cluster, CLI, or provider source changed.
- The direct `durable-store` fail-before remains recorded at the exact `0/35`.
  The corrected implementation passes NNCV000-NNCV027 at `28/28`.
- The sole full GPT-5.6 Sol/xhigh/fast review ran against the R1-R20-green
  candidate. It accepted one P2 recovery-cursor defect, one P3 exact verifier
  cardinality gap, and one P3 unbounded child-process proof.
- R12, R13, and R20 are green. Pagination and its physical index use immutable
  saga identity; the inter-page phase-transition regression passes; the restart
  child is bounded, terminated, and reaped; the verifier child count is exact;
  and affected/final gates pass.
- The corrected item is candidate-frozen. Its one narrow correction review is
  clean with zero findings at confidence `0.97`; all three accepted defects are
  confirmed corrected. Close and commit the exact item. Do not run another
  review.
- There is no blocker. Do not push or open a PR.
