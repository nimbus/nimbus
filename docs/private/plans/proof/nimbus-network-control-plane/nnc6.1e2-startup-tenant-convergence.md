# NNC6.1e2 Startup And Tenant-Retirement Convergence

Status: `complete; K1-K20 green; item commit contains this record`

Starting checkpoint: `b6cb18f1eb8c70d273b73fdba17a81086d6b7edf`

NNC6.1e2 closes the last NNC6 recovery gap. Durable workload records and
exact effect drivers already exist, but production startup does not consume
the all-phase recovery page. Tenant deletion also retains its intent and
source inventory only in the current process. This item adds one bounded
compute-owned startup supervisor and one portable tenant-retirement record.
It reuses the existing exact provision, restart, and teardown owners.

## Source-Derived Fail-Before

The frozen baseline has four acceptance-critical gaps:

1. `WorkloadSagaCoordinator::plan_recoverable_page` has no production caller.
2. `WorkloadSagaRecord::requires_recovery` excludes an exact durable
   `DefiniteFailure`, so a crash before the `FailedProvision` cause CAS leaves
   compensation undiscoverable.
3. `TenantDeletionLease`, `TenantSourceRetirementClaim`, and its frozen source
   snapshot are process-local. The system records accepted deletion only in
   process-local state. A fresh process cannot restore the source-admission
   barrier.
4. Tenant paging uses only `workloadId > cursor`. Without one mutation epoch
   around the complete scan, an insertion behind the cursor can evade a
   fractured pass.

These are NNC6.1e2 defects. Cleanup finalization, orphan cleanup, release, and
capacity reuse remain exclusively NNC8.3-owned.

## Current And Target Ownership

```text
current

Engine workload-saga durability -> pure recovery decisions -> no startup consumer
                                               |
                                               +-> restart-only retained watch

delete request -> process-local Engine lease + services snapshot -> live driver
process death  -> no durable delete intent or source barrier
```

```text
target

nimbus-workloads
  workload saga + tenant-retirement record/store ports + tenant mutation epoch
          |
          v
nimbus-compute
  one bounded startup supervisor + sole workload/tenant saga coordination
          |
          +-> existing WorkloadProvisioner
          +-> existing WorkloadRestartRuntime
          +-> existing WorkloadTeardownRuntime
          +-> exact fenced successor promotion
          |
          v
nimbus-server
  Engine adapters + schema/codec + fresh-process proof

nimbus-services
  logical source inventory + restorable admission barrier + effect-free finalization
```

Provider effects remain in sandbox, server, node, machine, proxy, and their
existing adapters. `nimbus-network` remains transport-free, effect-free, and
dependent only on `nimbus-core`.

## Frozen Decisions

1. The startup supervisor is a bounded one-shot compute concept. It is not a
   second background reconciler and does not own provider capabilities.
2. Startup submits or joins active restart state through the existing
   `WorkloadRestartRuntime`. Startup never constructs another restart driver.
3. Provision phases and durable definite failures route through the existing
   `WorkloadProvisioner`. Teardown phases route through the existing
   `WorkloadTeardownRuntime`.
4. `Recorded` successor promotion reloads and authenticates the exact saga ID,
   revision, active generation, and successor generation before its CAS.
5. `CleanupPending` produces a typed retained result. NNC6.1e2 cannot release,
   reuse, finalize cleanup, or infer absence.
6. Tenant deletion persists exact intent and source inventory before Engine
   deletion, workload, or provider effects. The adapter resolves ambiguous
   persistence by exact readback.
7. The durable tenant-retirement record binds tenant ID, Engine incarnation,
   stable retirement identity, revision, exact source inventory, and progress.
8. Services may reconstruct its process-local barrier only from an exact
   durable tenant-retirement record. It continues to own logical naming and
   sources. It has no provider effects.
9. Every workload-saga CAS atomically advances a tenant mutation epoch in the
   Engine adapter. Tenant retirement authenticates the same epoch before and
   after each complete paged inventory.
10. Startup readiness fails closed on an unavailable, corrupt, crossed, or
    incomplete store. Normal workload admission cannot pass the readiness
    boundary first.

## Frozen Source Allowlist

Portable vocabulary and ports:

```text
crates/nimbus-workloads/src/lib.rs
crates/nimbus-workloads/src/store.rs
crates/nimbus-workloads/src/tenant_retirement.rs
crates/nimbus-workloads/src/tenant_retirement/tests.rs
crates/nimbus-workloads/src/saga/state.rs
```

Compute coordination:

```text
crates/nimbus-compute/src/state.rs
crates/nimbus-compute/src/tenant_retirement.rs
crates/nimbus-compute/src/tenant_retirement/tests.rs
crates/nimbus-compute/src/workload_provisioner.rs
crates/nimbus-compute/src/workload_saga.rs
crates/nimbus-compute/src/workload_saga/startup_recovery.rs
crates/nimbus-compute/src/workload_saga/startup_recovery/tests.rs
crates/nimbus-compute/src/workload_saga/restart_runtime.rs
```

Source barrier:

```text
crates/nimbus-services/src/lib.rs
crates/nimbus-services/src/manager/tenant_retirement.rs
crates/nimbus-services/src/manager/tenant_retirement/tests.rs
```

Engine adapter and serving readiness:

```text
crates/nimbus-server/src/workload_saga_store.rs
crates/nimbus-server/src/workload_saga_store/schema.rs
crates/nimbus-server/src/workload_saga_store/codec.rs
crates/nimbus-server/src/workload_saga_store/recovery.rs
crates/nimbus-server/src/workload_saga_store/tenant_enumeration.rs
crates/nimbus-server/src/workload_saga_store/tenant_retirement.rs
crates/nimbus-server/src/workload_saga_store/tests/*
crates/nimbus-server/src/router.rs
crates/nimbus-server/src/construction.rs
crates/nimbus-server/src/state.rs
crates/nimbus-server/src/workload_composition.rs
crates/nimbus-server/src/workload_composition/tests.rs
```

Exact Engine deletion fencing:

```text
crates/nimbus-engine/src/engine/tenants.rs
crates/nimbus-engine/src/tests.rs
```

Fresh-process retirement must reject a replacement tenant incarnation before
marking that runtime delete-fenced. The pre-existing unqualified deletion
operation cannot provide that proof because the incarnation comparison would
occur after the effect. NNC6.1e2 therefore adds one expected-incarnation Engine
operation and its fail-before regression. Ordinary deletion keeps its existing
operation.

Verifier, this proof, canonical plan, and routing index are also allowed. The
item must record a need outside this list before that edit. `nimbus-network`,
provider implementations, tenant policy, proxy/egress, system projection, and
cluster transport are forbidden.

K19 found directly related test and verifier paths that were absent from the
initial source allowlist. The item also owns these paths:

```text
crates/nimbus-cli/src/compose/tests/lifecycle.rs
crates/nimbus-cli/src/machine/local_server.rs
crates/nimbus-compute/src/workload_saga/provision_decision/tests.rs
crates/nimbus-engine/src/engine/bootstrap.rs
crates/nimbus-engine/src/engine/objects.rs
crates/nimbus-server/src/tests/tenant_isolation_harness.rs
docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json
scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh
scripts/nimbus-network-control-plane/workload-provision-dispatch-self-test.sh
scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs
scripts/nimbus-network-control-plane/workload-teardown-contract.sh
scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs
scripts/verify-nimbus-network-control-plane.sh
scripts/verify-nimbus-network-source-contract.mjs
```

The two Engine edits correct Rustdoc references only. The three test paths use
an explicit large-stack thread for an existing managed-server scenario. The
server serving path also boxes the complete fail-closed readiness future, so
its 125,968-byte state does not stay on each caller's test-thread stack.
Verifier edits replace obsolete source shapes with the shared lifecycle-store
and durable tenant-retirement phase machine. They add no product authority.

## Recovery Routing Matrix

| Durable state | Sole executable owner | Bounded startup result |
| --- | --- | --- |
| Provision phase | `WorkloadProvisioner` | observed, waiting, exact successor settlement, or compensated failure |
| Exact durable provision failure | provisioner compensator -> teardown runtime | recorded, waiting, or cleanup retained |
| Active restart | existing `WorkloadRestartRuntime` | joined/settled or waiting; never a duplicate driver |
| Teardown phase | existing `WorkloadTeardownRuntime` | recorded, waiting, or cleanup retained |
| `Recorded` plus running successor | coordinator exact promotion, then provisioner | exact higher generation only |
| `Recorded` plus stopped successor | coordinator exact promotion | terminal stopped record |
| `Quiescent` | none | counted, no write/effect |
| `CleanupPending` | none in NNC6.1e2 | retained, no release/reuse/finalization |

Each route reloads current durable truth. A stale page decision cannot write or
call an effect. The existing owner inspects ambiguous issued effects before
retry.

## Tenant-Retirement Recovery Order

```text
capture exact source inventory under services barrier
  -> persist/confirm tenant-retirement intent
  -> acquire/authenticate Engine deletion fence
  -> retire runtime owner
  -> fence/join workload submissions
  -> epoch-before
  -> enumerate and drive every durable child
  -> enumerate exact terminal inventory
  -> epoch-after (must equal epoch-before)
  -> services effect-free finalization
  -> finish Engine deletion
  -> record terminal retirement
  -> release source barrier
  -> remove terminal retirement record
```

On restart, startup lists durable tenant retirements and restores every
services barrier. It does this before workload effects or admission. It then
resumes the same order. Startup accepts an absent target only with exact
terminal evidence and no different tenant incarnation.

## Deterministic Fail-Before Matrix

| ID | Baseline defect to expose |
| --- | --- |
| F1 | Fresh managed startup lists no all-phase workload recovery work. |
| F2 | A durable provision `DefiniteFailure` is absent from recovery enumeration. |
| F3 | No production consumer routes exact provision, teardown, successor, quiescent, and cleanup decisions. |
| F4 | Startup and the restart watch have no explicit shared exact-key handoff. |
| F5 | Tenant deletion has no portable durable intent or bounded active-retirement page. |
| F6 | A fresh services manager cannot restore the old incarnation's source barrier. |
| F7 | A process cut after accepted tenant retirement loses its source inventory. |
| F8 | An insertion behind a tenant cursor is not detected by an authenticated scan epoch. |

The fail-before tests must fail for these named reasons. Compile errors,
unrelated timeouts, sleeps, and provider-environment skips are not evidence.

## Acceptance Ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| K1 | The source-derived ownership graph, four defects, decisions, allowlist, and fail-before matrix are frozen before product edits. | pass |
| K2 | Exact fail-before behavior proves F1-F8 and is recorded before correction. | pass |
| K3 | Portable tenant-retirement identity, revision, source inventory, progress, CAS, delete, and bounded active-page contracts reject crossed/corrupt/stale input. | pass |
| K4 | Exact durable provision failure remains recovery-eligible until the `FailedProvision` cause is committed. | pass |
| K5 | One Engine transaction atomically applies each workload-saga CAS and advances its tenant mutation epoch. | pass |
| K6 | Complete tenant paging authenticates an unchanged mutation epoch; behind-cursor insertion fails closed. | pass |
| K7 | Server Engine adapters use exact schemas, system principal, bounded indexed pages, canonical codecs, and inspect-after-ambiguity. | pass |
| K8 | Services captures or restores one exact incarnation-bound source barrier and rejects source, definition, session, and workload admission until terminal release. | pass |
| K9 | One bounded compute startup supervisor enumerates every recovery page once and returns typed per-key and aggregate outcomes. | pass |
| K10 | Provision, failed-provision compensation, teardown, and successor routes reuse their existing exact owners and generation fences. | pass |
| K11 | Active restart recovery joins/defers to the existing restart runtime and cannot dispatch a duplicate restart effect. | pass |
| K12 | `CleanupPending` survives startup and tenant retirement with all fences retained and zero release, reuse, or finalization. | pass |
| K13 | Store unavailable/corrupt/ambiguous truth fails startup readiness closed without partial admission or inferred state. | pass |
| K14 | Tenant-retirement intent is durable before Engine deletion or provider effects; every named crash cut resumes from exact record/source truth. | pass |
| K15 | Fresh-process recovery receives only durable roots and fixed configuration; it receives no record, snapshot, manager map, handle, or `Arc` handoff. | pass |
| K16 | Server serving and managed foreground use cannot pass their workload readiness boundary before one bounded recovery attempt completes. | pass |
| K17 | Existing service naming, policy, PDP/PEP, provider-effect, certificate, projection, cluster, and cleanup ownership remains unchanged. | pass |
| K18 | `nimbus-network -> nimbus-core` remains its only workspace edge and no effect or transport enters the crate. | pass |
| K19 | Focused and full affected behavior, strict quality, dependency/effect, verifier, proof-lint, docs, and site gates pass with exact evidence. | pass |
| K20 | After K1-K19 are green, one full GPT-5.6 Sol/xhigh/fast review runs; accepted executable corrections get affected proofs and at most one narrow review; the exact item is committed once. | pass |

## Review Cadence

No structured review runs during implementation. Run one complete item review
only after K1-K19 are green and the candidate is frozen. Run one narrow
correction review only if an accepted finding materially changes executable
code. Proof wording, formatting, ledger edits, or elapsed time never authorize
another review.

## Current Evidence

The fail-before commands ran from checkpoint `b6cb18f1e` with the shared target.

- F1 ran `compute_state_retains_the_exact_managed_workload_composition`.
  It exited `101`: `0 passed`, `1 failed`, and `451 filtered out`.
  Fresh managed construction reported `all_phase_reads=0`.
- F2 ran
  `durable_definite_provision_failure_stays_discoverable_for_compensation`.
  It exited `101`: `0 passed`, `1 failed`, and `451 filtered out`.
  The crash hid durable compensation work before the `FailedProvision` CAS.
- F3 ran `fresh_managed_startup_consumes_each_planned_recovery_key`.
  It exited `101`: `0 passed`, `1 failed`, and `452 filtered out`.
  Exact failed-provision and cleanup-pending records received zero all-phase
  reads.
- F4 used the same managed-construction fixture. It reported
  `all_phase_reads=0` and `restart_reads=1`. Restart discovery ran before the
  shared startup handoff.
- F5 ran `server_preparation_installs_durable_tenant_retirement_authority`.
  It exited `101`: `0 passed`, `1 failed`, and `672 filtered out`.
  `prepare_for_server` left `_tenant_retirements` absent with `SchemaNotFound`.
- F6 ran
  `fresh_manager_restores_accepted_tenant_source_barrier_before_admission`.
  It exited `101`: `0 passed`, `1 failed`, and `94 filtered out`.
  A fresh manager admitted a new source after the process cut.
- F7 ran `fresh_manager_recovers_exact_accepted_source_inventory`.
  It exited `101`: `0 passed`, `1 failed`, and `94 filtered out`.
  The fresh manager reconstructed an empty source list instead of three
  accepted sources.
- F8 ran
  `tenant_driver_rejects_concurrent_child_insertion_without_duplicate_effects`.
  It exited `101`: `0 passed`, `1 failed`, and `451 filtered out`.
  The driver detected the inserted key only after `10` provider effects.

Focused corrections produced this evidence:

- Portable tenant-retirement contracts pass `5/5`.
- Exact definite-provision-failure recovery passes `1/1`.
- Services capture, restore, admission, finalization, and rejection pass
  `11/11`.
- Compute tenant retirement first passed `10/10`. The final expanded set passes
  `15/15`.
- The K6 behind-cursor regression passes `1/1`. The driver fails before
  teardown, and provider calls remain at zero.
- Server Engine retirement and epoch behavior passes `6/6`. Both workload CAS
  operations apply, and the epoch advances to `2`.
- Schema and system-policy proof passes `1/1`.
- Exact Engine replacement fencing passes `1/1`. A mismatched incarnation
  fails before delete fencing. The replacement remains enterable.
- The fresh-process matrix passes one parent over five writer/recovery pairs.
  Recovery receives only the durable root and fixed cut configuration.
- Bounded startup recovery passes `5/5`. Store failures occur before restart
  discovery and provider calls.
- Exact restart handoff passes `1/1`. Startup waiters join the retained task.
  Periodic discovery starts after the all-phase pass.
- Server workload composition passes `16/16` with one child-only ignore.
- CLI lifecycle passes `3/3`. Compose retirement passes `9/9`.
- Source inspection confirms reuse of the existing provision, restart,
  teardown, and successor owners. It finds no second effect driver.
- K17 finds no edit in forbidden owners. Services still owns logical naming
  and readiness.
- K18 confirms that `nimbus-network` depends only on `nimbus-core`. The source
  scan finds no transport, cloud SDK, upper-crate, or provider effect.

### Full Review Disposition

The one full item review used GPT-5.6 Sol at xhigh reasoning in fast mode over
staged tree `7941c57e6525df094965e8480cbab9619efcc4bd` and patch SHA-256
`2432c38c5b7af056bdff18d2baa8a5b02f0887e50409a089f973595c6e4d49a3`.
It accepted six findings. Each correction has direct behavior proof:

| Finding | Disposition and proof |
| --- | --- |
| Retained tenant-retirement retry entered an already fenced tenant before loading retained progress. | Accepted. The driver now loads and resumes the retained record first. The deterministic retry regression passes `1/1`. |
| Startup recovery did not authenticate the complete successor intent. | Accepted. The decision carries and compares the full successor intent. The crossed-successor regression passes in the `13/13` startup-recovery module. |
| Tenant retirement collapsed typed lifecycle failures into `Internal`. | Accepted. Core error classifications now survive the compute facade, and the focused classification regression passes. |
| Startup route proof did not substitute every real owner. | Accepted as a proof defect. Seven exact private owner substitutions cover provision, compensation, teardown, successor, restart join, cleanup retention, and failure closure; the full module passes `13/13`. |
| The fresh-process fixture persisted no sources or child sagas. | Accepted as a proof defect. Each of five process cuts now persists one real observed source and one exact child saga through legal CAS transitions. Recovery converges the child to `Recorded` with the exact stopped successor, while all provider effects remain forbidden. The parent matrix and retired-incarnation replacement tests each pass `1/1`. |
| Restart joining observed only one scheduler yield. | Accepted. The test now crosses the semantic retained-task join boundary and passes `1/1`. |

These executable and proof corrections authorize one narrow correction review.
They do not authorize a second full item review.

### Narrow Correction Review

The sole narrow review used the Nimbus autoreview wrapper with Codex
`gpt-5.6-sol`, xhigh reasoning, fast service, one pass, and no fallback. It
reviewed staged tree `5a916c51873853a5010c754c7ae0b30a4fb7d80a`, patch
SHA-256 `a0ad51a6c78e7fa7a2ad7c11214b42d356adea8ab61c3f65124f27caaaca263d`,
`60` paths, and `47` Rust paths. TruffleHog was clean. The review reported no
accepted or actionable finding and rated the corrected scope correct at
confidence `0.92`. The review cadence is complete. We will not run a third
review.

### K19 Candidate Gates

Full affected behavior passes `3,210` tests with `46` intentional ignores.

| Crate | Result |
| --- | --- |
| Workloads | `229` |
| Engine | `668 + 5 ignored` |
| Compute | `469 + 1 ignored` |
| Services | `96` |
| Server | `741 + 36 ignored` |
| CLI | `1,007 + 4 ignored` |

Engine used
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-engine -- --test-threads=1`.
Server and CLI used serialized full-crate invocations.

- The first all-feature check refused before linking. The shared V8 target did
  not use pointer compression. The requested feature enables it. The item did
  not clean the shared target or create a competing target. Hosted CI remains
  the pointer-compression source of truth. The authoritative affected check is
  `cargo check --all-targets -p nimbus-workloads -p nimbus-engine -p nimbus-compute -p nimbus-services -p nimbus-server -p nimbus-cli`,
  which exits `0`.
- Strict affected Clippy with `-D warnings` exits `0`. Strict Rustdoc with
  `RUSTDOCFLAGS='-D warnings'` exits `0` after two comment-only stale-link
  corrections. `cargo fmt --all --check` and `git diff --check` exit `0`.
- Shell and Node syntax checks pass for every changed verifier script.
- The live architecture verifier passes `36/36`.
- The aggregate fail-closed mutation suite passes `564/564`. It includes
  `180/180` teardown mutations and eight new tenant-retirement mutations.
- `bash scripts/check-docs.sh` passes `108` pages. The docs-site verifier passes
  `17/17` conditions.
- Proof lint passes with zero diagnostics.

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Current item | NNC6.1e2 |
| Last durable commit | The commit containing this completed record is the NNC6.1e2 item commit. |
| Current owned paths | Canonical plan/proof; `nimbus-workloads` portable retirement contracts; `nimbus-server` Engine adapters and serving readiness; `nimbus-services` exact barrier restoration; `nimbus-compute` bounded startup, restart handoff, retirement coordination, and tests; required `nimbus-cli` managed foreground readiness call sites and tests. |
| Last green | K1-K19; full affected behavior `3,210 + 46 ignored`; affected check, strict Clippy/Rustdoc, format/diff; live verifier `36/36`; aggregate mutations `564/564`; docs `108`; site `17/17`; proof lint zero. All six accepted full-review corrections have focused proof. |
| Next action | Reconcile current main at the clean item checkpoint, then begin the read-only NNC7.1 ownership and protocol-parity audit. |
| Blocker | none |
