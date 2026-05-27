# TSB0 Baseline Inventory

Date: 2026-05-27

## Status

Status: `done`

This is a baseline checkpoint for
`docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`. It records the
current code and documentation shape before TSB1 starts moving module paths or
introducing local-enforcement types. It is not a completion claim for later
requirement IDs.

## Git Base

- Branch: `main`
- Base revision: `90a44f51`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb0-baseline.md`

No Rust source, generated files, or unrelated proof artifacts were changed.

## Requirement IDs Touched

- `REQ-DOCS`: satisfied for TSB0 by this proof note and docs validation.
- `REQ-ADMIT`, `REQ-RAW`, `REQ-SYSTEM`, `REQ-STORAGE`, `REQ-STATUS`,
  `REQ-CREDS`, `REQ-LIFECYCLE`, `REQ-TRUST`, `REQ-HOST`, `REQ-ARTIFACT`,
  `REQ-DELETE`, `REQ-QUOTA`, `REQ-CRATE`: reviewed as baseline inventory only.
  These IDs are intentionally not marked satisfied by TSB0; later phases must
  provide their required tests or dependency audits.

## Behavior Changed

None. TSB0 changed plan/proof documentation only.

## Tests Added Or Updated

None. This phase did not change product behavior.

## Symbol Inventory

The current root tenant module is
`crates/nimbus-server/src/tenant_isolation.rs`. It is already a thin re-export
root over concept-owned children:

| Child module | Classification | Current role |
| --- | --- | --- |
| `artifact_provenance` and `artifact_provenance/{admission,cosign,sbom,slsa}.rs` | tenant-domain plus artifact admission | Runtime bundle, guest executable, OCI image, signature, SLSA, SBOM, offline-root, composite-verifier, and command-runner evidence. |
| `audit_events.rs` | tenant-domain evidence | `TenantIsolationEvent` schema plus OCSF/OpenTelemetry projections and redaction behavior. |
| `authority.rs` | tenant-domain | `TenantIsolationMode` and authority decision shape. |
| `context.rs` | tenant-domain admission boundary | `TenantIsolationContext` and `admit_runtime_invocation_decision`; crate-private by design. |
| `decision.rs` | tenant-domain admitted artifact | `TenantIsolationDecision`, `TenantIsolationDecisionId`, `TenantStorageAccessDecision`, `TenantServiceAccessDecision`, and audit record shape. |
| `evidence.rs` | tenant-domain evidence | Canonical event names and evidence reason codes. |
| `identity.rs` | tenant-domain identity | `TenantWorkloadIdentity`, `TenantWorkloadKind`, `TenantWorkloadLocation`, and `TenantWorkloadStableIdentity`. |
| `image_admission.rs` | tenant-domain plus artifact admission | Tenant image admission source, request, provider, signature/provenance/SBOM evidence, and OCI reference parsing helpers. |
| `operator_policy.rs` and `operator_policy/*` | tenant-domain policy compiler | Operator policy document/default/runtime/sandbox/service/network/storage/volume/image/secret/quota/audit shapes, validation, prove, diff, draft, reload, external-policy, egress, formatting, and explanation. |
| `policy_input.rs` | tenant-domain policy input | Decision inputs for service grants, network endpoints, storage namespace, volumes, images, secrets, quotas, audit redaction, and full `TenantIsolationPolicyInput`. |
| `runtime_admission.rs` | tenant-domain runtime PEP | `RuntimeIsolationTier`, `RuntimePolicyAdmission`, `TenantRuntimePolicyAdmission`, `TenantRuntimePolicyDecision`, and test-only `RuntimeIsolationRoute`. |

The server facade currently re-exports the tenant boundary from
`crates/nimbus-server/src/lib.rs`:

- artifact verification/admission symbols:
  `ArtifactAdmission`, `ArtifactVerificationRequest`,
  `ArtifactVerificationPolicy`, `ArtifactVerificationEvidence`,
  `ArtifactVerificationSubject`, `ArtifactVerifierBackend`,
  `ArtifactImageVerificationProvider`, `CompositeArtifactVerifierBackend`,
  `ArtifactVerifierCommandBackend`, `ArtifactVerifierCommandRunner`,
  `CosignVerifierBackend`, `SlsaVerifierBackend`, `SbomVerifierBackend`,
  `OfflineVerificationConfig`, `ArtifactVerifierError`,
  `ArtifactVerifierErrorKind`, `ArtifactVerifierResult`,
  `admit_runtime_bundle_artifact`, `admit_guest_executable_artifact`,
  `redact_artifact_verifier_output`, and provenance/SBOM/signature evidence
  structs.
- admitted tenant authority symbols:
  `TenantIsolationDecision`, `TenantIsolationDecisionId`,
  `TenantIsolationMode`, `TenantIsolationAuthorityDecision`,
  `TenantIsolationPolicyInput`, `TenantStorageAccessDecision`,
  `TenantServiceAccessDecision`, all `Tenant*PolicyDecision` inputs,
  `RuntimeIsolationTier`, `TenantRuntimePolicyAdmission`, and
  `TenantRuntimePolicyDecision`.
- identity and evidence symbols:
  `TenantWorkloadIdentity`, `TenantWorkloadKind`, `TenantWorkloadLocation`,
  `TenantWorkloadStableIdentity`, `TenantIsolationAuditRecord`,
  `TenantIsolationEvent`, `TenantIsolationEventKind`,
  `TenantIsolationEventResult`, `TenantIsolationEventValue`, and
  `TENANT_ISOLATION_EVENT_SCHEMA_VERSION`.
- operator policy symbols:
  `OperatorPolicyDocument`, metadata/default/workload/runtime/sandbox/service/
  network/storage/volume/image/secret/quota/audit policy structs, policy diff,
  lifecycle, proof, advisory, draft, reload, external-policy, egress, and
  `OPERATOR_POLICY_SCHEMA_VERSION`.

The current `_nimbus` system-tenant module is
`crates/nimbus-server/src/system_tenant.rs`. It is crate-private and owns
operator/system evidence, not application tenant APIs:

| Symbol group | Classification | Current role |
| --- | --- | --- |
| `SYSTEM_TENANT_ID`, `system_tenant_id`, `is_reserved_tenant_id`, `is_system_tenant_id`, `user_tenant_id` | system-tenant evidence | Reserve `_nimbus` and keep user tenant construction from silently claiming it. |
| `route_inventory`, `RouteInventoryEntry`, `adapter_capability_inventory`, `AdapterCapabilityEntry` | system-tenant evidence | Write adapter/route inventory records. |
| `install_table_projection_observer` | system-tenant evidence plus storage/API PEP | Projects table identity state into `_nimbus` evidence. |
| `prepare_system_tenant_async`, `ensure_system_tenant_async`, `record_*_async`, `delete_*_state_async`, `sync_scheduler_state_for_tenant_async`, `RunRecord`, `sandbox_backend`, `sandbox_status`, `endpoint_protocol` | system-tenant evidence | Prepare schemas and record service, machine, listener, subscription, scheduler, run, table, and system-event state. |
| `stable_key_segment`, `*_document_id` helpers | system-tenant evidence plus raw-value sanitization | Derive stable evidence document IDs instead of accepting raw tenant-controlled keys. |
| `system_table_schemas` | system-tenant evidence | Declares system table schemas. |

The current storage/API PEP inventory relevant to this plan is:

| Symbol group | Classification | Current role |
| --- | --- | --- |
| `TenantLifecycle`, `TenantPointRead`, `TenantPointWrite`, `TenantRangeScan`, `DurableJournal`, `SchedulerStore`, `ControlPlaneUsage`, `KeyProviderSurface`, `StorageEngine` | storage/API PEP | Storage trait surface split across tenant data, journal, scheduler, usage, and key-provider concerns. |
| `DEFAULT_TABLE_NAMESPACE`, `hidden_table_namespace`, `deleting_table_namespace`, `TableLifecycleTransition`, `TableLifecycleStateMachine`, `apply_table_lifecycle_transition` | storage/API PEP | Stable table lifecycle naming and state transitions. |
| `TableCatalogKey`, `TableCatalogEntry`, `TenantTableCatalog`, `TableIdentitySnapshotEntry`, `TableBackendLayout`, `TableSummaryStatus`, `TableIdentityDiagnostic` | storage/API PEP | Tenant/table identity snapshots, backend-owned layout diagnostics, and table status classification. |
| `resolve_table_id_in_read_txn`, `resolve_table_id_in_write_txn`, `export_table_identities_in_read_txn`, `resolve_or_create_table_id_in_write_txn`, `ensure_table_id_in_write_txn`, `ensure_default_table_id_in_write_txn`, `stage_hidden_table_identity_in_write_txn`, `mark_default_table_deleting_in_write_txn`, `activate_hidden_table_identity_in_write_txn`, `hard_delete_deleting_table_identity_in_write_txn` | storage/API PEP | Stable `TableId` lookup, lifecycle, replacement, and deletion helpers. |

Current lower-layer consumers already use decision-derived authority:

| Consumer | Classification | Current role |
| --- | --- | --- |
| `crates/nimbus-server/src/runtime_host/mod.rs` | runtime primitive consumer | `RuntimeHostScope` stores a `TenantIsolationDecision` and derives runtime storage access from it. |
| `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs` | adapter shim plus HostBridge PEP | `ConvexHostBridgeScope` requires a `TenantIsolationDecision`; `ConvexHostBridge` stores `TenantStorageAccessDecision` from the decision. |
| `crates/nimbus-server/src/service_manager/activation.rs` | sandbox primitive consumer | Service activation builds or accepts a `TenantIsolationDecision` before sandbox service launch. |
| `crates/nimbus-server/src/service_manager/launch.rs` | sandbox primitive consumer | Sandbox launch checks service access, backend match, egress policy, image admission, returned tenant, and returned service name against the decision-derived binding. |
| `crates/nimbus-server/src/ws/mod.rs`, MongoDB adapter commands, Convex runtime-backed invocation context/subscriptions, async scheduling, and tests | server transport / adapter shim | Callers import tenant context or decision types from the current `tenant_isolation` path and are rename targets for TSB1. |

Current local-enforcement shape is planned, not implemented as production code:

| Source | Classification | Baseline finding |
| --- | --- | --- |
| `docs/plans/firecracker-snapshot-invocation-backend-plan.md` | local-enforcement consumer plan | Snapshot invocation flows from `TenantIsolationDecision` to invocation spec, pool restore, cleanup, and `TenantWorkloadStatus`. It names `nimbus-node / local_enforcement` as owner of host lifecycle and invocation-pool evidence. |
| `docs/plans/computer-use-sandbox-plan.md` | local-enforcement consumer plan | Session lifecycle, tenant binding, status, and evidence belong to `local_enforcement` / future `nimbus-node`; libkrun process details stay out of `nimbus-runtime`. |
| `docs/plans/gpu-accelerated-sandbox-plan.md` | local-enforcement consumer plan | Benchmark gate and evidence shape belong to `local_enforcement` / future `nimbus-node`; GPU backend primitives stay in sandbox-owned modules. |
| `docs/plans/service-identity-provider-auth-plan.md` | credential projection plan | Credentials must derive from `TenantWorkloadStableIdentity` and an admitted decision, not raw tenant strings, caller claims, process context, session IDs, or sandbox metadata. |

Current container-image contract is operator/runtime deployment context for
TSB8-TSB10, not dynamic tenant workload execution. `docs/operating/container-image.md`
states the default Nimbus image runs `nimbus` directly in the foreground, does
not run systemd in the container, uses UID/GID `10001:10001`, exposes `/health`,
keeps state under `/var/lib/nimbus`, and publishes digest, SBOM, vulnerability,
and attestation evidence.

## Comparative Pattern Disposition

| Source pattern | Disposition for TSB0 | Next owned phase |
| --- | --- | --- |
| OpenShell gateway/supervisor split | Intentionally deferred; plan keeps `tenant` as admission truth, `local_enforcement` as node-local coordinator, and workload-local `supervisor` as backend-specific process-local enforcement. | TSB3-TSB5 |
| Kubernetes NodeRestriction and status/lease/subresource model | Intentionally deferred; baseline has no node-status writer yet. The plan requires node identity, assigned-workload, UID, generation, decision, and observed-only status checks. | TSB3, TSB4, TSB11, TSB14 |
| Kubernetes bound service-account token shape | Intentionally deferred; service identity plan already rejects raw subjects and requires admitted workload identity. | TSB3, TSB4, TSB11, TSB14 |
| Kubernetes `observedGeneration`, conditions, deletion timestamps, finalizers, owner UID | Intentionally deferred; current code has system-tenant evidence records but no `TenantWorkloadStatus` type yet. | TSB3, TSB4, TSB11 |
| Kubernetes quota hard/used split | Intentionally deferred; current tenant policy has quota decision inputs, while hard-limit versus observed-usage accounting lands with local enforcement. | TSB4, TSB11-TSB13 |
| CockroachDB system tenant and typed capabilities | Partially implemented through reserved `_nimbus`, crate-private system tenant records, and current operator/system evidence posture; typed broad-target capabilities remain future work. | TSB2-TSB4, TSB11-TSB13 |
| Convex qualified storage/table namespace lessons | Partially implemented through stable `TableId`, table catalog lifecycle, table identity diagnostics, and table projection into `_nimbus`; system/user/virtual/orphaned namespace policy remains future-proofing evidence. | TSB2-TSB4, TSB11-TSB13 |
| workerd isolate-group trust monotonicity | Intentionally deferred; runtime warm-pool cross-tenant tests exist, but this plan still needs explicit trust classification and monotonic reuse evidence across runtime/sandbox/pool reuse. | TSB3-TSB5, TSB11, TSB14 |
| Podman Quadlet generator constraints | Intentionally deferred; container-image docs define the default app image and Podman example, but node installation, dynamic tenant D-Bus units, and `compose export quadlet` are not implemented yet. | TSB8-TSB10 |

## Verification Commands

Commands already run while building this baseline:

```sh
git rev-parse --short HEAD
```

Result: `90a44f51`.

```sh
git rev-parse --abbrev-ref HEAD
```

Result: `main`.

```sh
git status --short
```

Result before this proof note: only the TSB0 plan-row status edit was present.

```sh
rg --files crates/nimbus-server/src/tenant_isolation crates/nimbus-server/src/system_tenant crates/nimbus-storage/src
```

Result: inventoried the current tenant-domain, system-tenant, and storage
source files, including the split tenant modules and storage table-identity
helpers.

```sh
rg --count-matches "pub (struct|enum|trait|type|const|fn)|pub\(crate\) (struct|enum|trait|type|const|fn)|pub\(super\) (struct|enum|trait|type|const|fn)" crates/nimbus-server/src/tenant_isolation crates/nimbus-server/src/system_tenant crates/nimbus-storage/src/traits/mod.rs crates/nimbus-storage/src/table_identity.rs crates/nimbus-storage/src/store/table_catalog.rs
```

Result: symbol-count inventory by file. Highest-count touched files were
`tenant_isolation/artifact_provenance.rs` with 90 public or crate-visible
symbols, `policy_input.rs` with 40, `decision.rs` and `audit_events.rs` with
35 each, `table_identity.rs` with 27, and `operator_policy/external.rs` with
28. This proof classifies the public/root security surface by module and
records the lower-layer consumers that matter for TSB1-TSB14.

```sh
rg -n "tenant_isolation|TenantIsolationDecision|local_enforcement|system_tenant|HostLifecycle|TenantWorkloadStatus" crates/nimbus-server/src crates/nimbus-storage/src crates/nimbus-sandbox/src crates/nimbus-runtime/src docs/plans/*.md docs/architecture docs/operating docs/tenant-isolation.md
```

Result: 465 matched lines. The relevant production consumers are runtime host,
Convex HostBridge, service manager activation/launch, WebSocket and adapter
imports, and system-tenant record paths. The relevant local-enforcement
references are currently active platform plans, not production modules.

Closeout validation:

```sh
git diff --check -- docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb0-baseline.md
npm run docs:validate-refs:strict
git status --short
```

Results:

- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (212 working-tree Markdown files)`.
- `git status --short`: only
  `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md` and
  `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb0-baseline.md`
  were modified or untracked.

## Remaining Risks

- TSB0 did not run Rust tests because it changed only documentation. TSB1 must
  run the focused tenant-isolation, drift, and audit-event tests before and
  after the module-path rename.
- The local-enforcement and host-lifecycle concepts are currently plan-level.
  No production `local_enforcement` module, `TenantWorkloadStatus`,
  `HostLifecycleBackend`, `NodeStatusAuthorizer`, node lease, or credential
  projection writer exists yet.
- The comparative pattern review is a scope filter, not an implementation
  shortcut. Each deferred pattern still needs Nimbus-specific tests before its
  requirement ID can close.

## Next Resumable Action

Commit the TSB0 checkpoint, then start TSB1 by running the pre-rename focused
tenant-isolation tests.
