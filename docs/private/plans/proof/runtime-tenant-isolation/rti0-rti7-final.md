# Nimbus Runtime Tenant Isolation Final Proof

Date: 2026-07-21

Implementation branch: `codex/review-runtime-tenant-isolation-plan`

Plan baseline: `93124d87e` (`origin/main` when the plan was promoted)

Final implementation baseline: `c789db2fd` (`origin/main` after rebasing the
completed implementation, including PRs #225, #226, and #229)

Pull request: #227

This proof closes RTI0-RTI7. It supplements the fail-before predicates in
`rti0-fail-before.md`; deliberately red tests were not committed.

## Shipped Ownership Model

- Engine/storage remain the only authority for tenant existence, the durable
  tenant incarnation, and tenant operation/delete fencing.
- `nimbus-compute::RuntimeManager` lowers an Engine-issued tenant runtime lease
  into an opaque `RuntimeOwnerLease`, owns canonical runtime configuration and
  lane executors, and tracks deployment authority separately from owner
  lifetime.
- `nimbus-runtime` owns backend-neutral retained-state admission, exact reuse
  authority, owner partitions, revocation, retirement, caps, and diagnostics.
  It still has zero workspace dependencies.
- Routing affinity is only worker locality. `None` and unscoped `Script`
  routing cannot remove, synthesize, or replace runtime-owner authority.
- Convex and Cloud Functions select adapter semantics and artifacts, but use
  manager-selected Nimbus runtime lanes and the same owner lifecycle contract.

## Band Evidence

### RTI0 - Reproduction And Guard

- `rti0-fail-before.md` records the baseline source predicates for `None`,
  unscoped `Script`, same-label recreation, optional-tenant Wasmtime Stores,
  missing V8 owner caps, and adapter-owned executors.
- `runtime-warm-pool-owner-incarnation-isolation` exercises `None`, unscoped
  `Script`, different subjects with equal numeric incarnations, and the same
  subject with a new incarnation using module-global sentinels.
- Retained Wasmtime tests reject missing, mismatched, and revoked owners.
- The mandatory common retained-state guard replaced the temporary fail-closed
  scaffold; there is no compatibility or downgrade mode.

### RTI1 - Canonical Owner And Reuse Authority

- `RuntimeOwnerId` equality contains owner class, opaque stable subject, and a
  nonzero incarnation; the human audit label is excluded.
- `RuntimeOwnerLease` carries revocation. The owning
  `RuntimeManagerInvocationLease` retains the Engine operation lease through
  background drain; cloneable host authority cannot pin tenant deletion after
  the invocation lifecycle ends.
- Reuse authority is a closed projection separate from routing locality.
- Runtime key/property tests vary owner, deployment, bundle, runtime shape,
  permissions, capabilities, services, and construction mode independently.
- The Engine suite passed `549` tests (`2` ignored) with implicit external
  provider fixtures disabled; compute tests prove the Engine-issued identity
  changes after same-ID recreation and cannot be minted locally.

### RTI2 - Owner-Partitioned Backends

- V8 warm runtimes and context-recycle isolates are stored in explicit owner
  partitions with owner and worker caps, local/global eviction, exact
  authority matching, and post-execution revocation checks.
- Wasmtime retained Stores use the same owner partition and actual bundle
  provenance hash; compiled components remain shareable.
- Bun/JSC calls common retained-state admission before backend entry, while its
  retained implementation remains non-product-selectable.
- The final required runtime lane passed `516` tests (`134` ignored). Earlier
  focused evidence also passed the embedded-anchor integration test, all `8`
  locker tests, and the compile-fail doctest (`4` ignored).

### RTI3 - Retirement And Races

- Owner and deployment-authority retirement broadcast to every worker and
  return acknowledgement reports.
- Admission rechecks revocation after invocation registration. Direct executor
  invocation also participates in the retirement registry.
- Owner retirement removes queued work, cancels active work, performs scoped
  pre/post-drain affinity purges, and condemns returned state. Deployment
  retirement preserves the owner while condemning the old generation.
- `executor::tests::retirement` passed `7/7`, covering queued, checkout-before-
  guest, active, response-ready/background drain, return-after-revoke,
  simultaneous owner/deployment, direct invocation, routing locality, and
  pressure/retirement behavior.

### RTI4 - Compute-Owned Runtime Manager

- `RuntimeGovernorConfig` is the canonical base configuration. Startup and
  deploy project its limits into adapter execution requirements; adapters
  preserve their guest semantics but do not own executors or the canonical
  governor/policy.
- Lanes are keyed by complete backend/profile/guest-semantics/construction
  requirements and safely shared across adapters.
- Deployment generations use checked monotonic advancement; overflow fails
  closed rather than aliasing authority.
- Deployment authority is scoped by owner class, so rotating an application
  deployment cannot revoke the system tenant's same-numbered generation.
- Compute tests passed `67/67`, including canonical startup limits, lane
  sharing, system owner class, generation separation, and overflow denial.

### RTI5 - Lifecycle And Adapter Migration

- Compute tenant deletion orders Engine fencing, owner retirement and worker
  acknowledgement, service teardown, remaining Engine-operation drain, and
  storage deletion. A partial teardown failure stays fenced and retryable.
- System evidence writes first use the already-created `_nimbus` tenant and do
  not wait on an unrelated tenant's deletion fence; the bounded regression test
  passed `1/1`.
- Same-ID recreation receives a different canonical owner; recreation remains
  denied until deletion completes.
- The shared served conformance harness passed for Convex and Cloud Functions:
  `2` parent tests passed and `2` subprocess-only tests were ignored by the
  parent runner after being executed in isolated child processes.
- Convex passed `45` tests (`3` ignored); Cloud Functions passed `35` tests.

### RTI6 - Fairness, Diagnostics, And Benchmarks

- Live owner partitions enforce per-owner and per-worker caps while global
  pressure eviction remains balanced across owners.
- Low-cardinality metrics cover checkout, exact hits/misses, owner mismatch,
  revocation, return-after-revoke discard, purge, and acknowledgement failure.
  Diagnostics aggregate by lane/profile and owner class without tenant IDs or
  incarnations as metric labels.
- Criterion owner-partition lookup at `4096` operations measured
  `[89.431, 90.405, 90.648] us` (about `45.307 Melem/s` at the center).
- Retirement fanout measured `[6.1925, 6.3488, 6.3879] us` for `1` worker,
  `[17.159, 17.496, 17.581] us` for `4`, and
  `[52.249, 55.629, 56.475] us` for `16`.

### RTI7 - Cleanup, Documentation, And Static Proof

- Duplicate adapter executor/authority ownership was removed. Architecture,
  operations, glossary, profile-runtime records, and the July isolation audit
  now describe runtime owners, routing locality, retirement, and residual
  same-process limits accurately. `IsolateGroup` remains a separately gated
  density question rather than an isolation boundary.
- `scripts/verify-runtime-tenant-isolation.sh` passed `19/19`, including
  compute manager ownership, adapter governor/policy exclusion, owner identity,
  pool admission, routing/bundle non-authority, Bun/JSC gating, and tenant-free
  startup artifacts.
- `make verify-harness SURFACE=runtime` passed its required runtime test and all
  `6` named cases, including owner-incarnation isolation.
- `scripts/check-docs.sh` passed `108` pages; the docs-site verifier passed
  `17/17` conditions.
- Final formatting, clippy, and required local CI results are recorded below.

## Final Gates

- `mise exec node@24 -- make ci` passed end to end: formatting, workspace
  clippy, dependency policy, runtime anchor consistency, the required runtime
  lane (`516` passed, `134` ignored), the workspace lane (`4662` passed, `33`
  skipped, `2` reported leaky), doc tests, the required verification harness,
  JavaScript package/UI builds and tests (`51` files / `336` UI tests), and
  release/install proof helpers.
- `scripts/verify-runtime-tenant-isolation.sh` passed `19/19` static checks.
- `make verify-harness SURFACE=runtime` passed `1/1`, exercising all `6` named
  runtime isolation and liveness cases.
- `scripts/check-docs.sh` passed `108` pages; the docs-site verifier passed all
  `17` conditions.
- Focused lifecycle regressions passed: the served tenant-isolation conformance
  harness completed `21` scenarios (`12` allowed, `9` denied), the persistent
  deploy/restart case passed `1/1`, and the system-evidence/delete-fence case
  passed `1/1`.
