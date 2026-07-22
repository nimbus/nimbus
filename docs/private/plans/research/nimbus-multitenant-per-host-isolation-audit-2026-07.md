# Nimbus Multi-Tenant-Per-Host Isolation Audit (2026-07)

## VERDICT: GO-WITH-PUNCH-LIST

**No LIVE HIGH cross-tenant break exists on today's shipped multi-tenant-per-host path** — the data plane (documents, KV, volumes) is physically per-tenant and collision-infeasible, and every HIGH candidate is genuinely *latent* (blocked by an unwired feature, independently re-verified). Multi-tenant co-location is safe today, but three latent HIGHs MUST be closed before their triggering features ship, and shared-pool DoS/fairness needs node-level aggregate admission.

### Runtime correction — 2026-07-21

The original shared-V8 conclusion was incomplete. A tenant label in the then
current exact partition key covered the default tenant-routing path, but did not
cover `None` routing, unscoped `Script` routing, same-ID tenant recreation, or
the separate optional-tenant Wasmtime retained-Store key. The runtime
tenant-isolation follow-on closes that gap with Engine/storage-derived owner
incarnations, owner-partitioned V8 and Wasmtime retention, common fail-closed
owner admission for every mutable retained backend, and acknowledged
owner/deployment retirement across every worker. The corrected claim is about
exact runtime-owner incarnation, not the presence of a tenant label.

---

## Per-axis summary

| Axis | Status | Key evidence |
| --- | --- | --- |
| **Network + IPAM + shared bridge/PEP** | SAFE (2 non-HIGH residues) | M1 collision structurally gone: identity derived from a globally-unique index over `state.tenants.values().flat_map(indices)` (`segment.rs:306`); per-tenant distinct subnet/bridge/`network_id` (`net.rs from_index`); per-tenant IPAM file+lock (`layout.rs:39`); H1 own-PEP pin fail-closed `policy drop` output chain, siblings rejected (`egress_pin.rs`); cross-tenant L3 blocked twice (netavark `ISOLATE=true` + pin). No cross-tenant read/collision. |
| **Compute (microVM) + shared runtime lanes** | SAFE with structural runtime-owner isolation | Per-sandbox libkrun microVM, globally-unique ULID sandbox ids; manager reads all tenant-checked and fail-closed (`sandboxes.rs:266-285` cross-tenant get = not-found). Mutable V8 and Wasmtime state is partitioned by owner class, stable subject, and Engine/storage incarnation; routing affinity is locality only; every lane worker acknowledges owner/deployment retirement. |
| **Storage + data + filesystem/volumes** | SAFE data plane (1 latent HIGH, DoS/hardening) | Embedded = per-tenant `.redb`/`.sqlite` file; SQL = per-tenant schema (160-bit SHA-256, collision-infeasible); KV = `<tenant_id>0x00<key>` with `TenantId` validated `[A-Za-z0-9_-]` (delimiter unforgeable) + `untenant_key` fail-closed; volumes/artifacts `tenants/<id>/…` traversal-safe. Only gap: shared OCI blob cache. |
| **Secrets + credentials** | SAFE today (inert), 1 latent HIGH | Fully unwired/fail-closed: `CredentialSecretStore` always `empty()` in prod, injection returns forbidden; `nimbus-tenant` holds only opaque handle strings; decision-log unwired; redaction clean. Flat `credential_ref` namespace has no tenant discriminator — safe only because the store is empty. |
| **Services / sandboxes / sessions manager** | SAFE read/enumerate (1 MEDIUM DoS, 1 LOW) | Every cross-tenant read tenant-bound and fail-closed (`None`/`NotFound`/`PermissionDenied`); ids ULID/counter, no slot reuse; per-`TenantServiceKey` activation lock. Gap: no per-tenant count quota on node-wide maps; the one quota (`SandboxTemplateLeaseController`) is unwired. |
| **Admission + scheduling + quota** | SAFE identity binding (multiple MEDIUM DoS/fairness) | Status writes + credential projection identity-bound and fail-closed; per-tenant quota scans only own manifests. Gap: only per-tenant ceilings, no node-level aggregate admission; port TOCTOU; admitted quota fields audit-only (unenforced); scheduler no per-tenant fairness (single-node only today). |

---

## Punch list

### Latent HIGH — NOT launch-blocking for co-location today, but LAUNCH-BLOCKING for the feature that triggers them (fix before that feature ships)

1. **HIGH — Flat `credential_ref` namespace (M1-class).** `CredentialSecretStore` (`crates/nimbus-proxy/src/credentials.rs:4`) is keyed by a bare `credential_ref` with no tenant discriminator; policy validation only checks non-empty (`nimbus-egress/src/policy.rs:382`). Blocked today: prod PEP `backends/oci/egress.rs::ensure_running` never calls `with_credential_store` → `empty()` → injection fails closed; `from_entries`/`with_credential_store` have only test callers. **Becomes a live cross-tenant credential read** the moment secret-management populates a shared/host-wide store cloned into per-sandbox proxies. *Blocking for: secret-management wiring.* Fix: key lookup by `(tenant/sandbox, credential_ref)` or build each per-sandbox store only from the owning tenant's resolved secrets; promote `prove.rs:295` tenant-prefix check from advisory to fail-closed gate.

2. **CLOSED — Mutable runtime retention previously treated routing/labels as ownership.** The fix is intentionally deeper than rejecting one `None` routing case: mutable retention requires a live manager-issued owner lease under every routing mode, V8 and Wasmtime share explicit owner partitions, the owner includes canonical subject plus Engine/storage incarnation, and deletion/redeployment revokes and retires matching state across every worker. Ownerless fresh/startup-snapshot and immutable compiled-artifact paths remain valid.

3. **HIGH — Shared OCI blob cache cross-tenant read.** `materializer.rs:71` hardcodes `blob_cache_dir = state_root/image-cache/oci` shared across all tenants (used by both container and krun/vm paths); `pull_blob_to_cache` (`:462`) returns a cache HIT with no per-tenant authorization/digest re-check. Blocked today: `RegistryAuth::Anonymous` hardcoded (`:340`), public-only. **Becomes a live cross-tenant private-layer read** the moment authenticated registry pulls land; is *already* a dedup timing side-channel and unbounded shared-disk DoS. *Blocking for: authenticated/private registry pulls.* Fix: scope cache per tenant OR gate hits on a per-tenant pull entitlement.

### MEDIUM — live today, all fail-closed (availability/DoS/enumeration); not launch-blocking for correctness, prioritize before scaling co-location density

4. **MEDIUM/H2-adjacent — runtime egress authorization can fail open on absent tenant label.** `nimbus-bridge/src/egress.rs:106-135` rejects tenant mismatch only when `EgressRequest.tenant_label` is `Some`; if a runtime/adapter caller constructs egress with no tenant label, the host bridge can authorize a tenant-bound egress decision without proving the tenant. Blocked today: shipped server/runtime paths normally supply the tenant label via `RuntimeInvocationHostCallBinding`, but `tenant_label` is optional in `nimbus-runtime/src/runtime/bootstrap/state.rs:80-100` and the fetch hook forwards that optional value (`extensions.rs:378-393`). Fix now with the same principle as H2: fail closed when the egress decision is tenant-bound and the request tenant label is absent, with any local/tooling exception modeled as an explicit separate principal/mode.

5. **MEDIUM DoS — no node-level aggregate admission on shared pools.** Every shared pool is bounded only by per-tenant ceilings whose sum exceeds host capacity: published-port window ~1001 / 128-per-tenant ≈ 8 tenants exhaust it (`port_manager.rs:14,96`); sandbox 128 vCPU / 256 GiB **per tenant** → ~2 tenants oversubscribe (`spec.rs:20-24`, `resource_quota.rs`); node super-net `/16` shared + grow-only placement burns `/24`s → ~3-4 scaling tenants starve new-tenant segments (`placement.rs:36`, `segment.rs:329`); uncapped `state.sessions`/`state.definitions` (`sessions.rs:51`, `definitions.rs:84`); unbounded never-pruned OCI blob cache (`materializer.rs`). The one per-tenant quota (`SandboxTemplateLeaseController::max_instances_per_tenant`) is **unwired**. Fix: sum-across-tenants admission; wire the existing quota controller.

6. **MEDIUM collision (TOCTOU, fails closed at bind) — host port double-pick.** `read_used_host_ports` (`container/runtime.rs:429`) reads manifests with no lock; `next_available_host_port` (`port_manager.rs:91`) picks first-free; `plan_start` runs unlocked (only Mutex `runtime.rs:62` guards an unrelated map). Two concurrent cross-tenant launches can pick the same port — but only one binds, the other gets `EADDRINUSE`. No redirect/read. Fix: serialize allocation under a lock or reserve-then-commit.

7. **MEDIUM wiring gap — admitted quota is audit-only.** `TenantQuotaPolicyDecision.runtime_budget`/`sandbox_charge` (`policy_input.rs:236`) is admitted/audited/projected (`tenant.rs:554`) but has no getters and no consumer; real enforcement uses a disconnected hardcoded `SandboxResourceQuotaPolicy::default()` (`runtime/config.rs:95`, `krun/vm.rs:163`). Operator quotas not tunable end-to-end.

8. **MEDIUM DoS — placement never revisits partially-free grown blocks.** `placement.rs:36` always starts from the primary block then grows a fresh `/24` per exhaustion, hitting `MAX_BLOCKS_PER_TENANT=64` after ~63 sandboxes and burning ~16k addresses to host ~316; accelerates the aggregate-admission super-net pressure. Fail-closed. Fix: iterate `entry.indices` before growing.

### LOW — hardening

9. **LOW — H1 pin conditional on `egress_proxy.is_some()`** (`container/runtime.rs:797`, `krun/vm/lifecycle.rs:250`). Cross-tenant reach still blocked by isolated bridges; only exposed reach is a same-tenant sibling PEP. Add an assertion binding "execute-mode netns goes live" to "a pin was applied."
10. **LOW — OCI cache hit trusts existing blob without SHA-256 re-verify** (`materializer.rs:462`); download temp `<digest>.download` in shared dir races on concurrent same-digest pulls (fail-closed by post-download verify). Add defensive re-verify + tenant/pid-unique temp name.
11. **LOW — `sandbox_resources` keyed by bare backend id** (`nimbus-services/manager/types.rs:30`), relying on backend ULID uniqueness; key by `(tenant, id)` to make isolation intrinsic. Same shape for admission `admit_if_principal_claim_absent_or_matching` (`context.rs:167`) admitting an Application principal with no tenant claim — use `require_matching_principal_claim`.
12. **LOW — object-store credential resolver flat `id`** (`nimbus-object-storage/src/credentials.rs`), fail-closed today, same flat-namespace shape if it becomes per-tenant. OCI/crun backend silently drops `TenantVolume` mounts (functional, not security).

---

## The one-tenant-per-host fallback

A large fallback is **not required** — the verdict is GO-WITH-PUNCH-LIST, not NO. But if you choose to defer the punch list and run **one-tenant-per-host**, every remaining concern collapses because the shared-resource threat surface is what the punch list targets:

- **The two remaining latent HIGHs become non-issues.** A shared credential store and a shared OCI blob cache holding another tenant's private layer are cross-*tenant* by definition — with one tenant per host there is no second tenant to read. Runtime owner-incarnation isolation is now structural and remains required in either topology.
- **Most MEDIUM DoS/fairness items become non-issues.** Port-window, CPU/mem, super-net, session/definition, and blob-cache exhaustion only harm *co-located* tenants; a single tenant exhausting its own host is its own capacity problem, not a cross-tenant break. The runtime-egress missing-tenant guard is still worth fixing because it protects future multi-tenant runtime/adapter egress callers regardless of host density.
- **The port TOCTOU collision** persists intra-tenant but is already fail-closed at bind (`EADDRINUSE`) — no correctness impact.
- **What stays load-bearing even under one-tenant-per-host:** the per-tenant data-plane separation (per-tenant `.redb`/schema/KV-prefix/volume paths) and the H1 own-PEP netns pin remain the isolation boundary against the tenant's own escaped/compromised workload reaching the host or other hosts, and must not regress.

Recommendation: ship multi-tenant co-location now (no live HIGH), but treat punch-list items 1–3 as hard gates on their respective triggering features (secret-management, multi-tenant public-facade use, private-registry pulls), and land item 4 (node-level aggregate admission) before increasing co-location density.

---

## Adversarial pass

## Adversarial synthesis — multi-tenant-per-host cross-tenant break attempts

Root read: `nimbus-worktrees/egress-audit` @ `7a2ae9466` (branch `mtn6-grow-egress-fix`), read-only. I independently re-verified the load-bearing code behind every HIGH candidate across the six axes and tried to chain them into a live break as an attacker controlling one tenant's workload on a shared host.

### Bottom line
**No LIVE HIGH (cross-tenant read or same-slot collision) is reachable on today's shipped multi-tenant-per-host path.** Every HIGH candidate is genuinely *latent* — it stays closed only because a deferred feature is unwired — and for each I confirmed the exact unwired guard. The exploitable-today breaks are all availability/DoS or fail-closed collisions (MEDIUM). I could not construct a concrete cross-tenant read.

### HIGH candidates — all confirmed LATENT (attack blocked today)

**1. Secrets: flat `credential_ref` namespace (M1-class shared identifier) — BLOCKED today.**
Attempt: tenant A's egress policy names a `credential_ref` owned by tenant B; the per-sandbox PEP injects B's secret into A's request. Blocked: `CredentialSecretStore::from_entries` / `EgressProxyConfig::with_credential_store` have **only** `crates/nimbus-proxy/src/tests.rs` callers. The sole production PEP instantiation (`backends/oci/egress.rs::ensure_running`) never sets a store → every proxy runs `CredentialSecretStore::empty()` → injection hits the fail-closed branch. But the store is keyed by a bare `credential_ref` with no tenant discriminator (`crates/nimbus-proxy/src/credentials.rs:4`), and the only cross-tenant handle guard (`nimbus-tenant/.../prove.rs:295` `prove_cross_tenant_regressions`) is advisory-only and skips any handle not prefixed `tenant-`. So this flips to a live HIGH the instant deferred secret-management populates a shared/host-wide store and clones it into per-sandbox proxies.

**2. Compute: runtime-owner isolation — CLOSED by the 2026-07-21 correction.**
The original attack also works through unscoped script routing, tenant
recreation, and the retained Wasmtime Store, so checking only the missing-label
case was insufficient. All mutable retained admission now requires an active
runtime-owner lease; the owner is independent of routing and includes the
canonical subject plus incarnation; V8/Wasmtime partitions and executor-wide
retirement enforce it before guest entry, at checkout, and on return.

**3. Storage: shared OCI blob cache cross-tenant read — BLOCKED today (anonymous-only).**
Attempt: tenant B references a digest tenant A already pulled and reads A's private layer straight from the shared cache without the registry authorizing B. Blocked: `RegistryAuth::Anonymous` is hardcoded (`materializer.rs:340`) with no private-auth config path — pulls are public-only, so there is nothing to bypass. But the cache dir is hardcoded shared (`materializer.rs:71` `state_root/image-cache/oci`) and used by **both** the container backend and the Linux-production krun microVM path (`krun/vm/start.rs:195`), and `pull_blob_to_cache` returns a cache HIT with no authorization and no digest re-verification (`materializer.rs:462`). This becomes a live cross-tenant private-layer read the moment authenticated registry pulls land. It is *already* a live dedup timing side-channel (hit/miss reveals sibling image digests) and an unbounded shared-disk DoS.

### Live-today breaks I confirmed (all MEDIUM, fail-closed)

- **Port TOCTOU collision (M1-shaped, mitigated).** `read_used_host_ports` (`container/runtime.rs:429+`) reads manifests off disk with **no lock**; `next_available_host_port` (`port_manager.rs:91`) picks first-free; `plan_start` runs `ensure_launch_quota` + `allocate_missing_bindings_for_tenant` unlocked. The only `Mutex` (`runtime.rs:62`) guards the unrelated in-memory `machine_port_proxies` map. Two concurrent cross-tenant launches can pick the same host port — but at actual container bind only one succeeds, the other gets `EADDRINUSE` and fails. No traffic redirect, no read. Fail-closed collision.

- **Shared-pool DoS across axes (no node-level aggregate admission).** Every shared node pool is bounded only by per-tenant ceilings whose sum vastly exceeds host capacity: published-port window (~1001 / 128-per-tenant ≈ 8 tenants exhaust it), sandbox CPU/mem (128 vCPU / 256 GiB **per tenant**, `spec.rs:20-24` → ~2 tenants oversubscribe the host), node super-net (/16 shared, grow-only placement in `placement.rs`/`segment.rs` burns /24s → ~3-4 scaling tenants starve new-tenant segment assignment), and uncapped `state.sessions`/`state.definitions` in `nimbus-services` (`sessions.rs:51`, `definitions.rs:84`, no admission context). The one real per-tenant instance quota (`SandboxTemplateLeaseController::max_instances_per_tenant`) is **unwired** — no caller outside its own file. Also the operator-admitted `TenantQuotaPolicyDecision` (`policy_input.rs:236`) is audit-only: no code consumes its fields; enforcement uses a disconnected hardcoded `SandboxResourceQuotaPolicy::default()`. All fail closed (later tenants get errors, not reads).

### Cross-axis chaining attempts — none produced a live HIGH
- Chaining "force `None` tenant" (compute gap) with the DoS-bypass or warm-pool reuse requires reaching the `None`-tenant facade, which no multi-tenant server path does. Dead end.
- Chaining port TOCTOU into a redirect fails at OS bind (`EADDRINUSE`). Dead end.
- The data plane itself is solid: embedded = per-tenant `.redb`/`.sqlite` file, SQL = per-tenant schema (160-bit SHA-256, collision-infeasible), KV = `<tenant_id>0x00<key>` with `TenantId` validated to `[A-Za-z0-9_-]` (so the `0x00` delimiter is unforgeable) and `untenant_key` fail-closed. Volumes/artifacts are `tenants/<tenant_id>/…` with traversal-safe validated ids. No cross-tenant document/KV/volume read is constructible.

### Recommendation ordering
1. Close the two remaining latent HIGHs *before* their triggering features ship: (a) tenant-scope the credential store lookup + fail-closed ownership check (promote `prove.rs` from advisory to gate); (b) scope the OCI blob cache per tenant OR gate cache hits on a per-tenant pull entitlement. Keep the now-landed runtime-owner static and behavioral gates green. Also fix the runtime-egress missing-tenant guard before that wiring expands.
2. Add node-level aggregate admission (sum-across-tenants) for the shared pools and wire the existing `SandboxTemplateLeaseController` / `TenantQuotaPolicyDecision` quotas that are currently unenforced.
3. Serialize port allocation under a lock (or reserve-then-commit) to remove the TOCTOU; key `sandbox_resources` by `(tenant, id)` so cross-tenant separation stops depending on backend id-uniqueness.
