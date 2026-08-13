# NNC6.6 Service Resolution Fencing

Status: `complete`

Owner branch: `codex/nimbus-network-architecture-audit`

Starting checkpoint: `78569693e4c764abd4573e238d04647120926eae`

## Outcome

Logical service resolution stops returning a routable handle before compute
awaits teardown or restart publication withdrawal. Teardown stays fenced
through terminal projection. Restart stays fenced through durable publication
observation. Services owns naming and the resolver state. Compute remains the
sole workload coordinator.

## Source-Derived Call Graph

1. Managed composition passes one `Arc<ServiceManager>` through
   `NodeServicesConfig` as the `RuntimeServiceRegistry`.
2. Runtime callers use `snapshot_for_tenant` or `resolve_service_binding`.
3. Both methods derive bindings from services-owned observed sandbox handles.
4. `ComputeResourceRetirer::submit_service_teardown` installs a
   `WorkloadSourceRetirementClaim` before it persists the stopped successor or
   awaits the first provider command.
5. Tenant retirement installs `TenantSourceRetirementBarrier` under the same
   `ServiceManagerState` lock before it captures or retires tenant sources.
6. The current resolver reads the observation but does not read either fence.
   A ready cached handle therefore remains routable during withdrawal.
7. Terminal service projection changes the handle to `Stopped`, clears its
   endpoints, and removes the source claim in one state-lock operation.
8. Restart previously dispatched `WithdrawPublication` without a logical
   resolution fence. A cached ready handle could remain routable during the
   complete restart.
9. Restart completion is durable before the driver re-observes the exact
   target execution and ingress through the existing projection orchestrator.
   Only that truthful projection can release the services fence. A replay of
   the completed record can retry projection and release without repeating a
   restart provider command.

## Frozen Seam

The production correction stays in `nimbus-services`:

- `crates/nimbus-services/src/manager/registry.rs` owns the atomic resolver
  predicate, runtime projection, and exact restart-fence transitions.
- `crates/nimbus-services/src/manager/catalog.rs` applies the same predicate to
  the public read-only service-instance catalog.
- `crates/nimbus-services/src/manager/types.rs` retains one process-local
  source-attempt to target-attempt chain. Terminal service and tenant
  retirement remove it.

Compute adds one narrow services adapter and injects it into the existing
restart driver:

- `crates/nimbus-compute/src/workload_saga/restart_resolution.rs` maps only a
  sandbox-backed service source to the services-owned fence and composes the
  existing exact projection orchestrator before release.
- `restart_driver.rs`, `restart_runtime.rs`, and `state.rs` install the fence
  before provider withdrawal and release it after durable publication
  observation.

Deterministic proofs stay with existing concept-owned test modules:

- `crates/nimbus-compute/src/resource_retirement/tests/support.rs` adds a
  test-only semantic gate at the first teardown or restart provider step.
- `crates/nimbus-compute/src/resource_retirement/tests/lifecycle.rs` resolves
  the service while the test parks the provider effect.
- `restart_driver/tests.rs` proves exact ordering, failure behavior, and
  release-only retry.
- `manager/tests/source_projection.rs` proves tenant, source, attempt-chain,
  replay, and crossed-evidence behavior.
- `scripts/verify-nimbus-network-source-contract.mjs` follows the atomic
  resolver helper names after the implementation stopped reading the observed
  projection through a separate lock acquisition.

The item does not change durable workload-saga state, provider effects,
sockets, naming, endpoint identity, restart policy, or `nimbus-network`.

## Race Contract

The services state mutex is the linearization boundary. A service source
claim, tenant barrier, or active restart withdrawal makes the matching cached
observation non-routable. A resolver that starts after that transition returns
no binding. The observed provider status remains available for status and
recovery.

If compute releases an unadvanced claim after a definite pre-effect rejection,
the unchanged ready observation becomes routable again. Ambiguous or pending
retirement keeps the claim and remains fail closed. Terminal retirement keeps
the stopped observation and removes the claim atomically.

A restart claim authenticates source generation, resource version, and the
exact source and target execution attempts. Exact replay is idempotent.
Release requires the services observation to name the exact target attempt.

Completion retains that target as an inactive chain fence. A second restart
must extend that attempt. If the first release fails, the exact successor can
replace the still-active chain atomically. A stale first release cannot reopen
the successor.

Definite failure does not release the active fence. A durable completion can
retry only the exact release without provider replay.

The fence is process-local because the service observations that it gates are
also process-local. A process crash cannot expose the old in-memory handle.
NNC6.1e2 retains fresh-process rehydration ordering from durable saga truth.

## Acceptance Ledger

| ID | Criterion | Evidence |
| --- | --- | --- |
| K1 | A ready service resolves before retirement starts. | Focused lifecycle test precondition. |
| K2 | A lookup after teardown `WithdrawPublication` starts returns no binding before the provider effect finishes. | Deterministic semaphore-gated lifecycle test. |
| K3 | A tenant snapshot at the same boundary omits the retiring service. | The same deterministic lifecycle test. |
| K4 | A service-specific claim fences only its exact tenant-qualified service key. | Services unit test with another service and tenant. |
| K5 | A tenant retirement barrier fences every service for that tenant and no service for another tenant. | Services unit test. |
| K6 | Fence checks and observation reads share one manager lock. | Source inspection plus focused concurrency test. |
| K7 | The fence does not delete or rewrite observed status needed for recovery. | Lifecycle test checks the observation while withdrawal is parked. |
| K8 | Terminal stop remains `Stopped` with no published endpoints and removes restart chain state. | Existing lifecycle behavior plus source inspection. |
| K9 | Restart fencing precedes the first provider withdrawal command and lasts through durable publication observation. | Driver ordering and parked-provider integration tests. |
| K10 | A failed publication withdrawal stays fenced. | Driver definite-failure test. |
| K11 | Exact claim and release replays are idempotent; release requires the exact target observation; stale or crossed attempts fail closed; an active second restart extends only the prior target attempt without reopening. | Services attempt-chain and active-handoff tests. |
| K12 | A restore failure after durable completion retries exact target projection and release without repeating restart provider work. | Driver restoration-retry and lifecycle target-observation tests. |
| K13 | A fresh process cannot inherit an old routable in-memory handle; durable rehydration order remains with NNC6.1e2. | State ownership inspection and explicit later-owner route. |
| K14 | No provider, socket, network-crate, policy, or naming authority moves. | Diff and dependency/effect scans. |
| K15 | Focused tests and complete affected crate suites pass. | Exact commands and counts below. |
| K16 | Formatting, strict affected Clippy, Rustdoc, diff, plan verifier, docs gates, one full Sol review, and its permitted narrow correction review pass. | Exact results below. |

## Evidence

The pre-correction test reproduced the teardown race. Direct resolution
returned a routable binding after compute installed the source claim and the
test parked provider withdrawal. The corrected focused set passes:

- services exact release and active-handoff tests: `2/2`.
- compute teardown and restart concurrency tests: `2/2`.
- restart fence ordering: `1/1`.
- completed-release retry: `1/1`.
- failed-withdrawal retention passed before review and remains covered by the
  full suite.

Complete affected suites pass:

- `cargo test -p nimbus-services`: `93/93`.
- `cargo test -p nimbus-compute`: `450` passed and `1` ignored child
  entrypoint.

Affected all-target compilation, strict all-target Clippy, warning-denied
Rustdoc, `cargo fmt --all --check`, `git diff HEAD --check`, and Node syntax
pass. The vendored Brotli sources emit inherited warnings. The primary-crate
lint gates pass. The live architecture verifier passes `36/36`.

## Item Review Disposition

The one full GPT-5.6 Sol/xhigh/fast item review found four issues:

1. Accepted: durable completion alone could reopen the stale source-attempt
   observation. The correction reuses the exact execution/ingress projection
   orchestrator and releases only after Services observes the target attempt.
2. Accepted: a failed first release could reject an exact successor restart.
   The correction permits only the adjacent active attempt-chain handoff and
   keeps stale release fenced.
3. Accepted as proof truth only: K16 still described verifier and docs work as
   pending after those gates advanced. This record now states exact live
   results.
4. Accepted: the lifecycle proof used an anonymous yield loop. It now uses a
   named semantic wait with last durable restart state and provider call
   counts in its timeout diagnostic.

The accepted executable findings changed the candidate, so the plan cadence
permitted one narrow correction review. The actual reviewer was Codex
`gpt-5.6-sol` with `xhigh` reasoning and fast service in one pass, with no
fallback. It found one P2 in the test harness: the named wait stopped at
durable restart completion before resolution restore could finish. The
corrected helper now waits for durable inactivity, the exact target-attempt
observation, and a routable released binding. Its timeout reports all three
states plus provider call counts.

The post-review lifecycle boundary and no-provider-replay proofs pass `1/1`
each. The test-only correction passes strict Compute Clippy, format, and diff
checks. The cadence permits no third review. K1-K16 are complete.
