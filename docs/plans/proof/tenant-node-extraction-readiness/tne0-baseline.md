# TNE0 Extraction Baseline

## Phase

- Phase ID: TNE0
- Status: done
- Git base: `c2970353` on `main`

## Files Touched

- `docs/plans/tenant-and-node-crate-extraction-readiness-plan.md`
- `docs/plans/proof/tenant-node-extraction-readiness/README.md`
- `docs/plans/proof/tenant-node-extraction-readiness/tne0-baseline.md`

## Requirement IDs

- REQ-EFFECTS
- REQ-VERIFIER
- REQ-TENANT-CRATE
- REQ-ADMIT
- REQ-SYSTEM
- REQ-STATUS
- REQ-HOST
- REQ-NODE-CRATE
- REQ-DOCS

## Behavior Changed

- None. TNE0 is a baseline inventory only.

## Tests Added Or Updated

- None. This phase did not change product behavior.

## Verification Commands

- `git rev-parse --short HEAD`
  - Result: `c2970353`.
- `git rev-parse --abbrev-ref HEAD`
  - Result: `main`.
- `rg --files crates/nimbus-server/src/tenant crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement crates/nimbus-server/src/system_tenant.rs crates/nimbus-server/src/system_tenant | sort`
  - Result: inventoried 42 relevant source files: 5 local-enforcement files,
    8 system-tenant files, and 29 tenant-domain files including tests.
- `cargo tree -p nimbus-server -e normal --depth 1`
  - Result: current `nimbus-server` direct dependency graph includes broad
    server dependencies such as `axum`, `tokio`, `nimbus-engine`,
    `nimbus-machine`, `nimbus-runtime`, and `nimbus-sandbox`. This confirms
    crate extraction must be proven by module-level audits, not by the current
    server crate graph.
- `rg -n "std::process|Command::new|Stdio|std::fs|fs::metadata|metadata\\(|ProcessArtifactVerifierCommandRunner|OfflineVerificationConfig|with_offline_trusted_root|with_runner|Arc::new\\(ProcessArtifactVerifierCommandRunner\\)" crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant`
  - Result: found tenant-domain host-effect blockers in production code:
    `std::process::{Command, Stdio}`, `std::fs::metadata`,
    `ProcessArtifactVerifierCommandRunner`, default
    `Arc::new(ProcessArtifactVerifierCommandRunner)` wiring in
    `ArtifactVerifierCommandBackend`, `CosignVerifierBackend`,
    `SlsaVerifierBackend`, and `SbomVerifierBackend`, plus
    `OfflineVerificationConfig` / `with_offline_trusted_root`. Additional
    `std::fs::write` matches are test-only fixture setup.
- `rg -n "crate::(adapters|http|ws|system_tenant|local_enforcement|service_manager|sandbox|runtime_host|execution|router|state)|axum|tokio|nimbus_engine|nimbus_storage|nimbus_machine|std::process|Command::new|std::fs" crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant --glob '!**/tests.rs' --glob '!tenant/operator_policy/tests.rs'`
  - Result: production tenant-domain matches are the artifact-provenance host
    effects above. No server transport, adapter, system-tenant,
    local-enforcement, service-manager, sandbox, runtime-host, router, state,
    `axum`, `tokio`, `nimbus_engine`, `nimbus_storage`, or `nimbus_machine`
    dependency was found in non-test tenant-domain files.
- `rg -n "^use |^pub use |mod |pub mod" crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant/*.rs crates/nimbus-server/src/tenant/artifact_provenance/*.rs crates/nimbus-server/src/tenant/operator_policy/*.rs`
  - Result: tenant-domain production code is otherwise shaped around
    `nimbus_core`, `nimbus_runtime`, `nimbus_sandbox`, `serde`,
    `serde_json`, `sha2`, `oci_client::Reference`, and `std` collections/net/
    path/sync/thread/time primitives. TNE1 must decide which `std` host-effect
    uses belong outside the tenant candidate.
- `rg -n "ProcessArtifactVerifierCommandRunner|CosignVerifierBackend::new|SlsaVerifierBackend::new|SbomVerifierBackend::new|CompositeArtifactVerifierBackend|ArtifactVerifierCommandBackend" crates/nimbus-server/src crates/nimbus-bin/src crates/nimbus/src --glob '!**/tests/**' --glob '!**/tests.rs'`
  - Result: current concrete artifact verifier construction and re-export
    surface lives in `nimbus-server`'s tenant module and facade exports. No
    `nimbus-bin` or facade crate wiring currently owns the default command
    runner.
- `rg -n "^use |^pub use |mod |pub mod" crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement/*.rs crates/nimbus-server/src/system_tenant.rs crates/nimbus-server/src/system_tenant/*.rs`
  - Result: local-enforcement production imports are `std` collections/future/
    pin/sync, `nimbus_core`, `serde`, `sha2`, and `crate::tenant`; system
    tenant imports server persistence dependencies such as `nimbus_engine`,
    `nimbus_machine`, `nimbus_sandbox`, and `crate::local_enforcement`.
- `rg -n "crate::(adapters|http|ws|system_tenant|service_manager|runtime_host|execution|router|state)|axum|nimbus_engine|nimbus_storage|nimbus_machine|std::process|Command::new|std::fs" crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement --glob '!**/tests.rs'`
  - Result: pass; no matches. Local-enforcement production code has no server
    transport, adapter, concrete storage provider, process-launch, or
    system-tenant persistence dependency today.
- `rg -n "LocalEnforcementBinding::from_decision|LocalEnforcementBinding::from_spec|TenantWorkloadStatus|TenantSystemEvidenceProjection|HostLifecycleBackend|HostLifecyclePlan|DirectProcessBackend|SystemdTransientUnitBackend" crates/nimbus-server/src --glob '!local_enforcement/**' --glob '!local_enforcement.rs'`
  - Result: production binding/status consumers are runtime host, Convex
    HostBridge, sandbox service-manager activation/launch, and
    `system_tenant/records.rs`; concrete backend start/stop/inspect callers
    remain inside local-enforcement tests.
- `rg -n "DirectProcessBackend::|SystemdTransientUnitBackend::|HostLifecycleBackend::|\\.validate\\(|\\.start\\(plan\\)|\\.stop\\(workload_id|\\.inspect\\(workload_id|impl HostLifecycleBackend" crates/nimbus-server/src --glob '!**/tests.rs' --glob '!tests/**'`
  - Result: concrete `DirectProcessBackend` and `SystemdTransientUnitBackend`
    implementations are production code, but construction and
    `validate/start/stop/inspect` calls appear only after local module
    `mod tests` lines. No production node reconciler exists yet.
- `rg -n "pub(crate)? async fn record|pub(crate)? fn record|write_document|delete_document|ensure_system_tenant|prepare_system_tenant|workload_status|TenantSystemEvidenceProjection|SystemEvidence" crates/nimbus-server/src/system_tenant.rs crates/nimbus-server/src/system_tenant`
  - Result: `_nimbus` persistence is server/system-tenant-owned. The
    workload-status projection writer exists as
    `record_tenant_workload_status_async`, but is currently `#[cfg(test)]`.
- `rg -n "record_tenant_workload_status|workload_status|TenantWorkloadStatus" crates/nimbus-server/src --glob '!target/**'`
  - Result: workload-status production DTOs and schema exist, local
    host-lifecycle backends can produce `TenantWorkloadStatus`, and the live
    persistence writer remains test-scoped until a real node/control-plane
    caller is introduced.
- `find crates -maxdepth 1 -type d \\( -name 'nimbus-tenant' -o -name 'nimbus-node' \\) -print`
  - Result: pass; no `crates/nimbus-tenant` or `crates/nimbus-node`
    directories exist.
- `rg -n "\"crates/nimbus-(tenant|node)\"|name = \"nimbus-(tenant|node)\"|nimbus_(tenant|node)" Cargo.toml Cargo.lock crates/nimbus-server crates/nimbus-bin crates/nimbus`
  - Result: no crate/member/package references; the only match is the existing
    runtime claim key `nimbus_tenant_id` in tenant context code.
- `cargo test -p nimbus-server tenant::artifact_provenance -- --nocapture`
  - Result: pass; 41 passed, 0 failed, 0 ignored, 839 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`
  - Result: pass; 22 passed, 0 failed, 0 ignored, 858 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `git diff --check -- docs/plans/tenant-and-node-crate-extraction-readiness-plan.md docs/plans/proof/tenant-node-extraction-readiness/README.md docs/plans/proof/tenant-node-extraction-readiness/tne0-baseline.md`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; docs reference validation covered 213 working-tree Markdown
    files.

## Current Evidence

### Classification

| Surface | Classification | Baseline finding |
| --- | --- | --- |
| `tenant/context.rs`, `tenant/decision.rs`, `tenant/identity.rs`, `tenant/policy_input.rs`, `tenant/authority.rs`, `tenant/runtime_admission.rs` | pure tenant contract candidate | Authority, workload identity, admission context, admitted decision, runtime policy admission, and projection types are suitable `nimbus-tenant` candidates once imports are rewritten to the new crate. |
| `tenant/audit_events.rs`, `tenant/evidence.rs` | pure tenant evidence candidate | Event schema, OCSF/OpenTelemetry projection, reason-code canonicalization, and redaction behavior are tenant evidence contracts. |
| `tenant/image_admission.rs` | tenant image-policy contract with pure parsing dependency | Uses `oci_client::Reference` for OCI reference parsing and no server transport or host lifecycle dependencies. Candidate for tenant crate if `oci_client` remains an accepted pure utility dependency or is replaced by a smaller parser. |
| `tenant/operator_policy/*` | pure tenant/operator policy compiler candidate | Depends on `nimbus_core`, `nimbus_runtime`, `nimbus_sandbox`, `serde`, `sha2`, and internal tenant modules. No server transport or persistence imports were found in production files. |
| `tenant/artifact_provenance.rs` and `artifact_provenance/{cosign,sbom,slsa}.rs` | mixed pure artifact contract plus verifier host effects | Request/policy/evidence/error/trait shapes are tenant candidates. `ProcessArtifactVerifierCommandRunner`, default process-runner constructors, `std::process`, and trusted-root filesystem metadata validation must move to server/operator wiring before extraction. |
| `nimbus-server` facade re-exports for tenant symbols | server wiring | Re-exports currently include pure tenant types and concrete verifier command runners together. TNE1/TNE2 must split intentional public re-exports from server-owned verifier-effect wiring. |
| `local_enforcement.rs` and `local_enforcement/{host_lifecycle,direct_process,systemd_transient}.rs` | node enforcement candidate | Production code has clean local dependencies today: tenant contracts, core types, serde, sha2, and std primitives. It cannot be extracted before `nimbus-tenant` because it currently imports `crate::tenant`. |
| Runtime host, Convex HostBridge, and sandbox service-manager consumers | server wiring / lower-layer PEP consumers | These production paths already derive local enforcement from `TenantIsolationDecision` or narrow projections. They are the current REQ-ADMIT evidence lanes. |
| `system_tenant/records.rs`, `projection.rs`, `schema.rs`, `keys.rs` | server/system persistence | Owns `_nimbus` schemas and writes. This must remain outside `nimbus-node`; future node code should depend on writer traits implemented by server wiring. |

### Blockers For TNE1/TNE2

- Tenant artifact provenance still owns process execution:
  `ProcessArtifactVerifierCommandRunner`, `std::process::Command`, and
  `Stdio`.
- Pure-looking verifier constructors still default to concrete process runners:
  `ArtifactVerifierCommandBackend::new`, `CosignVerifierBackend::new`,
  `SlsaVerifierBackend::new`, and `SbomVerifierBackend::new`.
- Offline trusted-root validation currently probes host filesystem metadata in
  the tenant artifact module. TNE1 should move that validation to
  server/operator verifier wiring or make it an injected verifier effect.
- `nimbus-server` currently re-exports both pure tenant contracts and concrete
  process-runner/verifier backend symbols from one tenant facade.

### Blockers For TNE3/TNE4

- No production `NodeWorkloadReconciler` exists.
- No production code outside local-enforcement tests calls
  `HostLifecycleBackend::validate`, `start`, `stop`, or `inspect`.
- `_nimbus.workload_status` schema/status DTOs exist, but the async writer is
  test-scoped; TNE3 must introduce a production writer trait and server-owned
  implementation deliberately.
- `local_enforcement` depends on `crate::tenant`, so `nimbus-node` extraction
  must wait until `nimbus-tenant` exists or would otherwise depend back into
  `nimbus-server`.

### Requirements

- REQ-EFFECTS and REQ-VERIFIER are not satisfied yet; TNE1 owns the blocker
  removal.
- REQ-TENANT-CRATE and REQ-NODE-CRATE are not satisfied yet; no extracted
  crates exist.
- REQ-ADMIT, REQ-SYSTEM, REQ-STATUS, and REQ-HOST have current baseline tests,
  but extraction phases must reprove them after moving code.
- REQ-DOCS is satisfied for TNE0 by this proof note and passing strict docs
  reference validation.

## Remaining Risks

- TNE0 is a snapshot. It does not remove blockers or claim extraction
  readiness.
- The artifact verifier split must be careful not to accidentally weaken
  fail-closed behavior, redaction, or offline-root validation.
- The node reconciler phase must not make `_nimbus.workload_status` writable by
  node code directly; persistence must stay inverted through server-owned
  traits.

## Next Resumable Action

- Start TNE1 by splitting artifact verifier host effects out of the tenant
  domain while preserving the existing artifact-provenance test behavior.
