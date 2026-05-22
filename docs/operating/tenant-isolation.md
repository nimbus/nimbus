# Tenant Isolation Runbook

Use this runbook when a tenant workload is rejected, when tenant-isolation
drift is reported, or before changing runtime, sandbox, storage, HostBridge,
network, image, secret, quota, cleanup, or audit code.

The architecture posture is documented in
[tenant isolation](../tenant-isolation.md).

## First Response

1. Capture the `TenantIsolationEvent` for the failed request or drift finding.
2. Record `decision_id`, `tenant_id`, `surface`, `principal_class`,
   `workload_stable_id`, `result`, `reason_code`, and correlation IDs.
3. Do not copy bearer tokens, cookies, raw credentials, or secret handles into
   incident notes. The event schema redacts sensitive fields; keep that
   property intact when exporting or summarizing logs.
4. Decide whether this is a tenant request problem, an operator policy problem,
   a product bug, or state drift.

## Rejected Admission

Admission rejection is expected when tenant-controlled intent tries to widen
authority. Use the `reason_code` and event kind to route investigation.

| Rejection Shape | Likely Cause | Check |
| --- | --- | --- |
| `application_principal_tenant_mismatch` | Bearer/session tenant claim does not match the route tenant. | Compare event tenant ID with the authenticated principal's tenant claim digest, not with raw bearer contents. |
| Runtime policy routes away from `in_process_untrusted`. | Production runtime requested broad filesystem, network, env, subprocess, FFI, worker, inspector, package-loading, or privileged grants. | Inspect the admitted `TenantRuntimePolicyDecision` and configure a stronger execution tier instead of loosening production mode. |
| Service lookup denied. | Workload requested a service not present in the decision-derived service grants. | Check deployment service bindings and `TenantServiceGrantPolicyDecision`. |
| Sandbox launch mismatch. | Catalog/backend returned a tenant, service, or backend different from the admitted decision. | Treat as a catalog or state integrity bug; do not launch the returned handle. |
| Image denied. | Production image is tag-only, unsigned under a required signature policy, missing provenance/SBOM, wrong identity, or local-build-backed without explicit policy. | Check `TenantImagePolicyDecision` and the provider evidence. |
| Volume/mount denied. | Compose requested host bind mounts, undeclared named volumes, invalid destinations, or too many mounts. | Rewrite to Nimbus-owned named volumes under tenant scope. |
| `_nimbus` denied. | Application or unauthenticated caller tried to access operator system data. | Require local admin/operator authority for system-control data. |

Safe response:

- Return the structured error to the caller with the reason code.
- Keep production mode enabled.
- Add or repair the explicit policy input if the request is legitimate.
- Re-run the conformance gate before treating the change as safe.

Unsafe response:

- Do not change expected tenant IDs to match the request.
- Do not bypass `TenantIsolationDecision`.
- Do not grant generic localhost, wildcard filesystem, raw secret, or host bind
  access as a quick fix.
- Do not switch production surfaces to local-development mode.

## Drift Findings

The drift scanner is read-only. It reports violations; it does not repair or
delete state.

Current code API:

- `scan_tenant_isolation_drift_async(...)`
- `TenantIsolationDriftScanConfig`
- `TenantIsolationDriftReport`
- `TenantIsolationDriftViolation`

When a finding appears:

1. Capture the report and keep the full violation code, surface, location, and
   message.
2. Stop or quarantine the affected sandbox/service before deleting artifacts.
3. Compare the finding with `_nimbus.services`, `_nimbus.ports`,
   `_nimbus.routes`, sandbox manifests, tenant volume roots, and any required
   decision/audit anchors.
4. Repair through normal Nimbus lifecycle operations when possible: redeploy,
   stop service, delete tenant, or rebuild the sandbox state.
5. If manual host cleanup is unavoidable, remove only the exact tenant-owned
   root named in the finding after confirming no active service references it.
6. Re-run the drift scanner or the focused drift test fixture after repair.

Escalate as a product bug when drift shows:

- A sandbox manifest tenant ID differs from the tenant path.
- A service handle or port record points to another tenant.
- A non-loopback service port exists without explicit public exposure policy.
- A volume root is outside the tenant volume tree.
- A decision/audit anchor is missing for active tenant service state when the
  scanner is configured to require it.

## Verification Gates

Run the focused tenant-isolation lane before and after any isolation-sensitive
change:

```sh
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture
cargo test -p nimbus-server audit_events -- --nocapture
make verify-tenant-isolation-conformance
cargo fmt --all --check
cargo clippy -p nimbus-server --all-targets
```

On macOS, the conformance gate may need to run outside a restricted execution
sandbox because the fixture binds local listeners.

For Linux cgroup v2 memory-limit proof:

```sh
bash scripts/prove-linux-cgroup-memory-limit.sh
```

For a remote Debian proof host:

```sh
ssh nimbus@<host> 'bash -s' < scripts/prove-linux-cgroup-memory-limit.sh
```

## Evidence To Preserve

For each incident or enterprise review, preserve:

- The `TenantIsolationEvent` JSON with redactions intact.
- The conformance command output, including scenario counts.
- Drift scanner report JSON or test output.
- Image admission decision and provider evidence, if image policy was involved.
- Runtime policy admission result and fallback tier, if runtime grants were
  involved.
- Sandbox manifest path, service handle, port record, and tenant volume root,
  if sandbox state was involved.

## Review Checklist

Before closing an incident:

- The original reason code is understood.
- The fix changes policy, deployment intent, or state through a Nimbus-owned
  seam rather than through raw host edits.
- No redacted field was expanded into incident notes or customer-facing logs.
- Conformance and focused tests pass.
- Any residual risk is recorded in
  [tenant isolation](../tenant-isolation.md#residual-risks) or in the owning
  follow-on plan.
