# Nimbus Capability Segregation Readiness Audit

Date: 2026-06-01
Plan: docs/plans/nimbus-capability-segregation-plan.md
Baseline reviewed: post-BPD HEAD reported as `db30ddac`

This note records the factual anchors checked during plan readiness review. It is
not implementation evidence for CB0-CB9; it is the pre-execution grounding proof
that the plan's control surface matches the live tree.

## Verified Anchors

| Claim | Verified anchor |
| --- | --- |
| `RuntimeGrants.service` is an exact service-name list and defaults empty. | `RuntimeGrants` / `service: Vec<String>` in `crates/nimbus-runtime/src/limits/grants.rs`. |
| Tenant service authorization has a decision seam. | `TenantIsolationDecision::service_access(...)` and `TenantServiceGrantPolicyDecision` in `crates/nimbus-tenant/src/decision.rs`. |
| Runtime host-call grants reject unknown service names before bridge dispatch. | `enforce_host_call_grants` in `crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs`; service check compares exact `grants.service` entries and returns `runtime service grant denied`. |
| Convex bridge re-checks the tenant decision before resolving service bindings. | `service_access(&payload.service_name)` in `crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs`. |
| HTTP service lifecycle routes are currently local-admin mounted. | `/api/tenants/{tenant_id}/services/{service_name}/start|stop|restart` under `build_local_admin_router()` in `crates/nimbus-server/src/router.rs`. |
| Tenant application auth verifier lives in the extracted auth crate. | `pub trait ApplicationAuthVerifier` in `crates/nimbus-auth/src/lib.rs`; nimbus-server `application_auth.rs` owns helper resolution only. |
| BPD is now the JS packaging baseline. | `docs/plans/archive/binary-embedded-package-distribution-plan.md`; package roots and closure gates in Makefile, `scripts/stage-embedded-packages.mjs`, `scripts/check-package-closure.mjs`, `scripts/build-js-package.mjs`, and `crates/nimbus-bin/src/embedded_packages.rs`. |
| Tenant runtime bundle admission for `nimbus/rest` is not already present. | Existing codegen classification is `MANAGED_PACKAGE_NAMES` / external-package handling in `packages/codegen/src/module_specifiers.mjs`; CB7a adds operator-only import rejection. |

## Design Consequences

- CB4/CB5 are hardening existing service checks, not adding the first service
  authorization check.
- CB2 is retired because BPD made `@nimbus/core` a new embedded root and closure
  edge while the JS package split remains ergonomic, not authoritative.
- CB7a must add tenant bundle admission for operator-only entries; it is not a
  pre-existing gate.
- CB8 intentionally widens HTTP service lifecycle reachability from operator-only
  to operator plus scoped tenant/spawned callers with own-tenant + exact service
  grant.

