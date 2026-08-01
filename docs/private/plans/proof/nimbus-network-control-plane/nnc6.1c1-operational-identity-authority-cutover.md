# NNC6.1c1 Operational Identity And Authority Cutover

Status: `acceptance frozen; exact fail-before captured; product implementation not started`

Starting checkpoint: `a0a802ea796e48ffe5431c74d6d08e9c3716ea5c`

NNC6.1c1 makes workloads-owned generation and execution identity canonical in
operational node paths. It also deletes two in-memory objects that look
authoritative but cannot survive a process restart. The item does not add the
durable saga adapter, construct a production coordinator, or reroute service
lifecycle effects.

## Audit Result

The source-derived audit found two coupled problems.

First, node lifecycle code derives `TenantWorkloadId` from workload UID and
admission decision ID. That value omits both assigned node and generation, so
it is not the execution identity frozen by NNC6.1b and implemented by
NNC6.1c. The same operational paths also use `TenantWorkloadGeneration`, a
second counter type with weaker wire semantics than `WorkloadGeneration`.

Second, `nimbus-services` and the CLI each own an
`InMemoryDesiredWorkloadStore`. Neither has a recovery reader. Services writes
its copy after or beside provider effects, and the CLI copy is only a
boot-plan collection. Their snapshots therefore do not prove durability or
restart recovery. Keeping them would create three desired-state authorities
once NNC6.1d adds the Engine-backed saga store.

The clean replacement is:

```text
TenantWorkloadUid + assigned NodeIdentity + WorkloadGeneration
  -> WorkloadExecutionId
  -> node backend keys and systemd unit names

ordered compose intent
  -> Vec<DesiredWorkload>
  -> deterministic placement planning

durable desired state
  -> absent in this checkpoint
  -> server Engine adapter in NNC6.1d
  -> compute-owned lifecycle routing in NNC6.1e
```

Deleting the in-memory copies removes false evidence. It does not remove a
real recovery capability because the current recovery-reader census is zero.

## Current Census

The audit derives this census from the starting checkpoint.

| Evidence | Current result | NNC6.1c1 target |
| --- | --- | --- |
| Rust files naming `TenantWorkloadGeneration` | 7 | 0 |
| Rust files naming node-owned `TenantWorkloadId` | 7 | 0 |
| `DesiredWorkloadStore` implementations | 1 | 0 |
| Product `InMemoryDesiredWorkloadStore` authorities | 2 | 0 |
| Service-manager desired-state upserts | 3 | 0 |
| Service-manager desired-state snapshot readers | 1 API with 3 test consumers | 0 |
| CLI desired-state representation | `DesiredWorkloadSnapshot` backed by `BTreeMap` through `WorkloadController` | ordered `Vec<DesiredWorkload>` |
| Product `WorkloadSagaStore` implementations | 0 | remain 0 |
| Production `WorkloadSagaCoordinator` constructions | 0 | remain 0 |
| NNCV027 implementation gaps | 3 | exactly 2: server adapter and lazy activation |
| New workspace dependency edges needed | 0 | remain 0 |

The seven `TenantWorkloadGeneration` files are:

```text
crates/nimbus-node/src/lib.rs
crates/nimbus-node/src/reconciler.rs
crates/nimbus-node/src/status.rs
crates/nimbus-node/src/tests.rs
crates/nimbus-workloads/src/lib.rs
crates/nimbus-workloads/src/tenant.rs
crates/nimbus-workloads/src/tenant/credential_projection.rs
```

The seven `TenantWorkloadId` files are:

```text
crates/nimbus-cli/src/machine/api/service_workloads.rs
crates/nimbus-node/src/direct_process.rs
crates/nimbus-node/src/host_lifecycle.rs
crates/nimbus-node/src/lib.rs
crates/nimbus-node/src/reconciler.rs
crates/nimbus-node/src/systemd_transient.rs
crates/nimbus-node/tests/zbus_systemd_live.rs
```

The false desired-state authority files are:

```text
crates/nimbus-cli/src/workload_boot.rs
crates/nimbus-services/src/manager.rs
crates/nimbus-services/src/manager/activation.rs
crates/nimbus-services/src/manager/sandboxes.rs
crates/nimbus-services/src/manager/types.rs
crates/nimbus-workloads/src/desired.rs
crates/nimbus-workloads/src/lib.rs
```

Tests that assert the false service authority live in:

```text
crates/nimbus-services/src/manager/tests/lifecycle.rs
crates/nimbus-services/src/manager/tests/sandbox_resources.rs
```

## Frozen Ownership

| Concern | Owner after this item |
| --- | --- |
| Admitted workload UID and assigned node | `nimbus-workloads::tenant`, derived from `nimbus-tenant` admission |
| Canonical workload generation and execution identity | `nimbus-workloads::saga` |
| Node execution keys, backend requests, and systemd names | `nimbus-node`, consuming `WorkloadExecutionId` |
| CLI compose intent ordering | `nimbus-cli`, as a non-authoritative value collection |
| Service and sandbox handles, effects, and observations | `nimbus-services` and `nimbus-sandbox` |
| Durable desired state and saga record | NNC6.1d server Engine adapter |
| Cross-domain transition decisions | `nimbus-compute`, first composed in NNC6.1d and routed in NNC6.1e |
| Rebuildable observed status | `nimbus-system` |

`WorkloadExecutionId` is not a logical saga key. It names one admitted
execution. The derivation uses `TenantWorkloadUid`, assigned `NodeIdentity`,
and `WorkloadGeneration`. IP addresses, ports, provider handles, admission
decision IDs, and systemd unit names never become workload identity.

## Substitution Contract

### Generation

Delete `TenantWorkloadGeneration` without an alias or re-export. Use
`WorkloadGeneration` for:

- `TenantWorkloadSpec` and request-identity checks.
- Credential projection requests and bindings.
- Egress reload requests.
- system evidence and node status projections.

`DesiredWorkload` remains a non-durable scheduling value with its current raw
counter in this item. Moving the canonical counter out of the saga module or
making the desired and saga modules mutually dependent would broaden this
cutover. NNC6.1d owns the typed conversion into durable saga intent.

### Execution identity

Delete the node-owned `TenantWorkloadId` type. Do not add an alias,
compatibility constructor, or re-export. `TenantWorkloadSpec::execution_id`
must require an assigned node and derive the canonical value.
`HostLifecyclePlan::from_binding` must consume that method and:

1. Require an assigned node.
2. derive `WorkloadExecutionId` from the workload UID, assigned node, and
   generation.
3. Derive the systemd unit name from that execution ID.

Direct-process and systemd backends, lifecycle requests and status, node
reconcile outcomes, Machine API test adapters, and the live systemd test use
`WorkloadExecutionId` directly. Fields and accessors use `execution_id`, the
systemd journal selector is `NIMBUS_WORKLOAD_EXECUTION_ID`, and unit names use
the complete validated `wex_` value. Same input is stable. Changing UID, node,
or generation changes the ID. Delete the old sanitizer, node-owned SHA-256
derivation, and raw integration-only constructor. Do not add an unchecked
production constructor.

Because execution identity is node-bound, `NodeAgent::reconcile_assignment`
and `NodeAgent::inspect_assignment` must compare the admitted assigned node
with the agent's node before backend validation or inspection. Missing or
crossed assignment produces a typed denial with zero backend and status-write
effects.

Accepted `TenantWorkloadStatus` derives its execution ID from the already
validated UID, writer node, and observed generation. The `_nimbus` observed
projection records that exact `executionId` and writes `observedGeneration` as
canonical decimal text. This preserves `u64::MAX` without IEEE-754 loss while
keeping `nimbus-system` a rebuildable observation rather than an identity or
desired-state authority.

### Desired-state authority deletion

Delete all of these types and surfaces:

```text
DesiredWorkloadStore
InMemoryDesiredWorkloadStore
DesiredWorkloadSnapshot
WorkloadController
ServiceManagerState::desired_workloads
ServiceManager::desired_workload_snapshot
ServiceManager::record_desired_service_workload
```

Delete the three services-owned upsert sites. Service and sandbox lifecycle
behavior remains in its existing owner until NNC6.1e. This item must not add a
replacement local map, cache, event log, file, no-op store, feature flag, or
compatibility shim.

`WorkloadControlBootPlan` stores `Vec<DesiredWorkload>`. Compose's existing
`BTreeMap` iteration supplies deterministic service-name order. The planner
pushes each desired intent and its placement result in the same pass, so both
vectors are one-to-one and order-aligned. Tests assert the exact order without
sorting it after the fact.

## Failure And Recovery Semantics

- Missing assigned node fails before execution-ID or systemd-name materialization.
- Crossed node assignment fails before backend validation, inspection, start,
  stop, status persistence, or any provider effect.
- Raw production input cannot construct an invalid execution ID.
- Service and sandbox effects retain their current error and cleanup behavior.
  Removing an unread in-memory copy cannot alter effect ordering.
- This checkpoint makes no durability or fresh-process recovery claim. The
  absence of a durable store remains explicit and mechanically checked.
- NNC6.1d must commit durable intent before later orchestration can rely on it.
  NNC6.1e remains responsible for routing lazy activation, restart recovery,
  and retirement through compute.
- Rollback means reverting the complete item commit. Runtime compatibility
  paths, dual writes, aliases, and snapshot handoff are forbidden.

## Non-Goals

NNC6.1c1 does not:

- Implement `_nimbus._workload_sagas`, a codec, schema, OCC adapter, or Engine call.
- Construct or inject `WorkloadSagaStore` or `WorkloadSagaCoordinator`.
- change service lazy activation, restart policy, provider effects, network
  lifecycle, publication, naming, or system projection ownership.
- Add a workspace dependency edge. This item removes `nimbus-node`'s now-unused
  `sha2` dependency.
- Alter `nimbus-network -> nimbus-core` or introduce a transport, cluster,
  tenant-policy, service-naming, proxy, sandbox, or server dependency in
  `nimbus-network`.
- Claim that removing the false snapshots completes lifecycle recovery.

## Exact Owned Paths

Only these paths may change for NNC6.1c1:

```text
ARCHITECTURE.md
Cargo.lock
crates/nimbus-cli/src/machine/api/service_workloads.rs
crates/nimbus-cli/src/dev/tests/plan.rs
crates/nimbus-cli/src/node_workload_executor.rs
crates/nimbus-cli/src/workload_boot.rs
crates/nimbus-node/Cargo.toml
crates/nimbus-node/src/direct_process.rs
crates/nimbus-node/src/host_lifecycle.rs
crates/nimbus-node/src/lib.rs
crates/nimbus-node/src/reconciler.rs
crates/nimbus-node/src/status.rs
crates/nimbus-node/src/systemd_transient.rs
crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs
crates/nimbus-node/src/tests.rs
crates/nimbus-node/tests/zbus_systemd_live.rs
crates/nimbus-services/src/manager.rs
crates/nimbus-services/src/manager/activation.rs
crates/nimbus-services/src/manager/sandboxes.rs
crates/nimbus-services/src/manager/tests/lifecycle.rs
crates/nimbus-services/src/manager/tests/sandbox_resources.rs
crates/nimbus-services/src/manager/types.rs
crates/nimbus-workloads/src/desired.rs
crates/nimbus-workloads/src/lib.rs
crates/nimbus-workloads/src/tenant.rs
crates/nimbus-workloads/src/tenant/credential_projection.rs
crates/nimbus-system/src/records/mod.rs
crates/nimbus-system/src/schema.rs
crates/nimbus-system/src/tests.rs
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1c1-operational-identity-authority-cutover.md
scripts/collect-nimbus-machine-service-proof.sh
scripts/nimbus-network-control-plane/compute-node-workload-coordinator-contract.sh
scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
scripts/verify-bootc-default-promotion-gate-helper.sh
scripts/verify-bootc-default-promotion-gate.sh
scripts/verify-nimbus-machine-service-proof-helper.sh
scripts/verify-nimbus-network-source-contract.mjs
scripts/verify-service-sandbox-node-reconciliation.sh
```

This item owns no network, compute, server, engine, sandbox, proxy, cluster,
provider, or listener source. The node manifest may only delete `sha2`. The
lockfile may change only if Cargo's resolved workspace metadata requires it.
If the compiler requires another path, stop and amend this acceptance contract
before editing it.

## Staged Implementation

1. Extend the standalone workload-saga authority helper with `cutover` mode
   and record the exact 12-condition fail-before result.
2. Replace `TenantWorkloadGeneration` with `WorkloadGeneration` across tenant,
   credential, status, and observed-projection surfaces.
3. Replace `TenantWorkloadId` with `WorkloadExecutionId` through node and
   Machine API paths. Add missing/cross-node zero-effect tests.
4. Delete the desired store/snapshot/controller types and both product
   authorities. Replace the CLI map with an ordered intent vector.
5. Run focused behavior tests, then full affected suites and static gates.
6. Candidate-freeze the complete item only after R1-R14 are green. Run the one
   full GPT-5.6 Sol/xhigh/fast item review. A materially accepted executable
   finding permits affected proof reruns and one narrow correction review.
7. Record exact evidence, transition the ledger, and commit the complete item.

## Fail-Before Contract

Before product edits, `workload-saga-authority-contract.sh cutover` must report
exactly `0 passed, 12 failed`, one diagnostic for each condition:

1. `TenantWorkloadGeneration` remains.
2. Node-owned `TenantWorkloadId` remains.
3. `DesiredWorkloadStore` remains.
4. `InMemoryDesiredWorkloadStore` remains.
5. `DesiredWorkloadSnapshot` remains.
6. `WorkloadController` remains.
7. `ServiceManager` desired state, snapshot API, or upsert authority remains.
8. The CLI planner is not a pure ordered `Vec<DesiredWorkload>`.
9. Host lifecycle and observed status do not carry `WorkloadExecutionId` with
   exact `execution_id` naming.
10. `TenantWorkloadSpec` and observed status do not use lossless
    `WorkloadGeneration` evidence.
11. The node crate retains the legacy SHA-256/sanitizer/raw-test identity path,
    old journal selector, or old unit-name convention.
12. Node reconcile and inspect do not both fence assigned-node mismatch before
    backend effects.

After implementation, the same mode must report `1 passed, 0 failed`.
`implementation` mode remains expected-red at exactly `0 passed, 2 failed`,
with only the NNC6.1d server adapter and NNC6.1e lazy-activation diagnostics.

### Captured fail-before

The acceptance checkpoint added only `cutover` mode. It did not edit product,
manifest, lockfile, operational-proof, or legacy-verifier source. The result
matched the frozen contract exactly:

```text
FAIL workload-saga-authority legacy TenantWorkloadGeneration remains
FAIL workload-saga-authority node-owned TenantWorkloadId remains
FAIL workload-saga-authority DesiredWorkloadStore remains
FAIL workload-saga-authority InMemoryDesiredWorkloadStore remains
FAIL workload-saga-authority DesiredWorkloadSnapshot remains
FAIL workload-saga-authority WorkloadController remains
FAIL workload-saga-authority ServiceManager desired-state field, snapshot, or write authority remains
FAIL workload-saga-authority CLI planner is not a pure ordered Vec<DesiredWorkload>
FAIL workload-saga-authority host lifecycle and observed status do not carry WorkloadExecutionId
FAIL workload-saga-authority tenant spec and observed status do not use lossless WorkloadGeneration
FAIL workload-saga-authority legacy node identity derivation, selector, or unit convention remains
FAIL workload-saga-authority node reconcile and inspect need exactly two pre-effect assigned-node fences, observed 0
Summary: 0 passed, 12 failed
```

Control modes remained exact:

| Mode | Result |
| --- | --- |
| `decision` | `1 passed, 0 failed`; census `8/1/2/3/54/0` |
| `implementation` | `0 passed, 3 failed`; old authority plus the NNC6.1d and NNC6.1e gaps |
| Bash syntax and ShellCheck | pass |

## Acceptance Matrix

| Gate | Verifiable success criterion | State |
| --- | --- | --- |
| R1 | Dirty paths are a subset of the exact allowlist; the node manifest only drops `sha2`; no new dependency edge or network, compute, server, engine, sandbox, proxy, cluster, provider, or listener source change exists. | pending |
| R2 | `TenantWorkloadGeneration` is absent from Rust source and public exports; affected admission, credential, egress, status, and evidence values use `WorkloadGeneration`. | pending |
| R3 | `TenantWorkloadId` is absent from Rust source and public exports; direct process, systemd, node reconcile, Machine API tests, and live systemd tests use fields, accessors, and values typed as `WorkloadExecutionId`. | pending |
| R4 | Operational ID tests prove same-input stability and changed UID/node/generation separation; missing assignment and wrong-agent-node cases deny before backend or status effects. | pending |
| R5 | Systemd names use the full validated `wex_` ID and `NIMBUS_WORKLOAD_EXECUTION_ID`; direct/systemd happy, stop, inspect, absent, error, external-restart-denial, and proof-collector behavior passes. | pending |
| R6 | `DesiredWorkloadStore`, `InMemoryDesiredWorkloadStore`, `DesiredWorkloadSnapshot`, `WorkloadController`, all three services upserts, the manager field, and snapshot API are absent without replacement authority or compatibility code. | pending |
| R7 | CLI boot planning owns only an ordered `Vec<DesiredWorkload>`; exact compose order and one-to-one placement alignment pass without test-side sorting. | pending |
| R8 | Service start/stop and sandbox create/stop/cleanup tests continue to prove actual handles, effects, observations, idempotency, and error behavior without asserting local desired-state persistence. System status records exact derived `executionId` and lossless decimal `observedGeneration`. | pending |
| R9 | Product `WorkloadSagaStore` implementations and production `WorkloadSagaCoordinator` constructions remain exactly zero; NNCV027 implementation mode has only its two later-owned failures. | pending |
| R10 | `cutover` mode records exact fail-before `0/12` and final `1/0`; decision mode is updated to the post-cutover census and passes. | pending |
| R11 | Full `nimbus-workloads`, `nimbus-node`, `nimbus-services`, `nimbus-cli`, and `nimbus-system` affected suites pass with exact test and skip counts recorded. | pending |
| R12 | All-target/all-feature checks, strict Clippy, and warning-denied rustdoc pass for all five affected crates. | pending |
| R13 | Cargo metadata remains acyclic; `nimbus-network` has exactly `nimbus-core`; workloads and compute forbidden edges remain absent; portable effect scans remain empty. | pending |
| R14 | Live network verifier, script syntax/ShellCheck, format/diff, technical-writing lint, docs, and site gates pass with exact counts. | pending |
| R15 | After R1-R14 are green, exactly one full GPT-5.6 Sol/xhigh/fast review is dispositioned. Only a materially accepted executable defect permits one narrow correction review. | pending |

## Required Verification

The implementation proof records exact counts and artifacts for:

```text
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh cutover
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh decision
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh implementation

timeout 1800 cargo nextest run -p nimbus-workloads
timeout 1800 cargo nextest run -p nimbus-node
timeout 1800 cargo nextest run -p nimbus-services
timeout 1800 cargo nextest run -p nimbus-cli
timeout 1800 cargo nextest run -p nimbus-system

cargo check -p nimbus-workloads -p nimbus-node -p nimbus-services -p nimbus-cli -p nimbus-system --all-targets --all-features
cargo clippy -p nimbus-workloads -p nimbus-node -p nimbus-services -p nimbus-cli -p nimbus-system --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-workloads -p nimbus-node -p nimbus-services -p nimbus-cli -p nimbus-system --no-deps --all-features

bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/verify-nimbus-network-control-plane.sh --self-test
cargo fmt --all --check
git diff --check
bash -n scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
shellcheck -x scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
bash scripts/verify-bootc-default-promotion-gate-helper.sh
bash scripts/verify-nimbus-machine-service-proof-helper.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Focused fail-before and corrected tests must name each critical behavior. These
are execution-ID derivation, missing/cross-node zero effects, direct/systemd
lifecycle, CLI order alignment, service lifecycle, and sandbox lifecycle.
Shared Cargo artifacts and repository single-flight conventions remain
mandatory.

## Review Cadence

The item, not an intermediate diff, is the review unit. Audit, fail-before,
implementation, cleanup, and acceptance convergence use owner inspection,
focused tests, affected suites, and static checks. Structured autoreview runs
only after the complete item is candidate-frozen and R1-R14 are green.

Do not push or open a pull request.
