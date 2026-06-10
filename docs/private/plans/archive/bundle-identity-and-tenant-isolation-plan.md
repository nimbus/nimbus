# Plan: Bundle Identity Rename and Tenant Isolation

Rename `bundle_identity()` → `identity()` on `RuntimeBundle`, and add tenant
isolation to `RuntimeBundleIdentity` so warm pool entries cannot be shared
across tenants even when bundles have identical content.

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Prerequisite:** warm-pool-default-and-retained-pool-deprecation-plan.md (done)
- **Motivation:** `bundle.bundle_identity()` is redundant naming. More
  importantly, `RuntimeBundleIdentity` is currently `(entrypoint, sha256)` with
  no tenant dimension — warm pool entries for one tenant can be served to
  another if they share the same bundle. This is a cross-tenant isolation
  violation for module-level state and violates the security model of every
  production multi-tenant runtime platform (Cloudflare Workers, Deno Deploy,
  Convex).

## Control Plan Rules

- `todo`: not started
- `in_progress`: actively being implemented
- `done`: acceptance criteria met and verification recorded

### Recovery loop

1. Reread this plan's Phase Status Ledger and Execution Log.
2. Inspect the current git worktree and reconcile.
3. Resume any `in_progress` phase first.

---

## Why Tenant-Isolated Bundle Identity

### Current state

`RuntimeBundleIdentity` is `(canonical_entrypoint, expected_sha256)`. The warm
pool matches on this identity — if two tenants submit bundles that resolve to
the same path and hash, they share warm runtimes. Module-level side effects
(`let counter = 0`) persist across warm reuse, so tenant-A's counter state
leaks to tenant-B.

### What production platforms do

| Platform | Isolation key | Share across tenants? |
|----------|--------------|----------------------|
| Cloudflare Workers | `(account_id, script_id)` | **Never** |
| Deno Deploy | `(project_id, deployment_id)` | **Never** |
| Convex | `(team_id, project_id, deployment_hash)` | **Never** |
| AWS Lambda | `(account_id, function_name, version)` | **Never** |

Every platform keys warm/cached isolates by tenant + code identity. Content
equality is never sufficient for sharing.

### What changes

`RuntimeBundleIdentity` gains an optional `tenant_label`. When present, warm
pool matching requires both content identity AND tenant identity to match.

The warm pool fallback path (bundle-only match, any affinity) still operates
within the tenant boundary — it can reuse a runtime from a different function
of the same tenant, but never from a different tenant.

---

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies |
|-------|--------|---------|-------------------|
| BI0 | `done` | Rename `bundle_identity()` → `identity()` across codebase | None |
| BI1 | `done` | Add `tenant_label: Option<String>` to `RuntimeBundleIdentity` | BI0 |
| BI2 | `done` | Thread tenant label through warm pool take/return paths | BI1 |
| BI3 | `done` | Add cross-tenant isolation test | BI2 |

## Recommended Delivery Order

1. BI0 — pure rename, no behavior change
2. BI1 — add field, update constructors
3. BI2 — update warm pool matching and affinity
4. BI3 — test that proves tenant isolation

## Implementation Details

### BI0: Rename `bundle_identity()` → `identity()`

Files to change:
- `crates/nimbus-runtime/src/runtime/bundle.rs` — method rename
- `crates/nimbus-runtime/src/affinity.rs` — call sites
- `crates/nimbus-runtime/src/runtime/bootstrap/snapshot/retained_pool.rs` — call sites
- `crates/nimbus-runtime/src/runtime.rs` — test call sites
- Any other `bundle_identity()` call sites found via grep

This is a mechanical rename. The internal field name `identity` is already
correct — only the public accessor changes.

### BI1: Add tenant label to `RuntimeBundleIdentity`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleIdentity {
    tenant_label: Option<String>,
    entrypoint: PathBuf,
    expected_sha256: Option<String>,
}
```

Construction paths:
- `RuntimeBundle::new()` and `RuntimeBundle::with_expected_sha256()` — no tenant
  (backward compatible for tests and dev tools)
- New: `RuntimeBundle::with_tenant()` or `RuntimeBundle::for_tenant()` — sets
  the tenant label. The server/engine layer sets this when creating bundles for
  tenant invocations.

`RuntimeBundleIdentity::tenant_label()` accessor returns `Option<&str>`.

### BI2: Thread tenant through warm pool

The warm pool match in `take_warm_pool_entry` already compares
`&entry.bundle_identity == bundle_identity`. Since `PartialEq` is derived and
`tenant_label` is now part of the struct, the match automatically enforces
tenant isolation — no additional matching logic needed.

The return path in `return_runtime_with_affinity` already stores
`bundle.identity().clone()`, which now includes the tenant label.

Key invariant: the `RuntimeBundle` passed to `take_runtime_for_invocation` and
`return_runtime_for_invocation` must have the same tenant label. This is
naturally true because the same bundle object is used for both the take and
return paths within a single invocation lifecycle.

The `RuntimeAffinityKey::Script` variant should also include the tenant label
for consistency, though the `Tenant` and `Function` variants already key by
tenant.

### BI3: Cross-tenant isolation test

A test that:
1. Creates two `RuntimeBundle` instances with identical entrypoint and SHA256
   but different tenant labels
2. Invokes tenant-A's bundle, returns the warm runtime to the pool
3. Attempts to take a warm runtime for tenant-B
4. Asserts cold miss (no warm hit) because the tenant labels differ
5. Invokes tenant-A again and asserts warm hit

This test goes in `crates/nimbus-runtime/src/runtime/tests/cooperative.rs` or
a new `warm_pool.rs` test file.

## Verification Contract

| Phase | Required verification |
|-------|---------------------|
| BI0 | `cargo test -p nimbus-runtime --lib` passes; no references to `bundle_identity()` remain; `make clippy` clean |
| BI1 | Compiles; existing tests pass (they use `None` tenant label) |
| BI2 | Existing warm pool tests pass; warm pool metrics unchanged for single-tenant scenarios |
| BI3 | New cross-tenant isolation test passes; warm_pool_hits == 0 for cross-tenant attempts |

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|-----------|-----------|
| BI0 | done | — |
| BI1 | done | — |
| BI2 | done | — |
| BI3 | done | — |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-08 | BI0 | done | Renamed `bundle_identity()` → `identity()` in bundle.rs, affinity.rs, retained_pool.rs, runtime.rs tests | `cargo test -p nimbus-runtime --lib` 71 pass; grep confirms zero `.bundle_identity()` call sites in crates/ | BI1 |
| 2026-04-08 | BI1 | done | Added `tenant_label: Option<String>` to `RuntimeBundleIdentity`; added `RuntimeBundle::for_tenant()` constructor; `new()` and `with_expected_sha256()` pass `None` | Workspace compiles clean; all 71 existing tests pass | BI2 |
| 2026-04-08 | BI2 | done | `RuntimeAffinityKey::Script` now includes `tenant_label: Option<String>`; warm pool matching via derived `PartialEq` automatically enforces tenant isolation | `make clippy` clean; all tests pass | BI3 |
| 2026-04-08 | BI3 | done | Added `bundle_identity_includes_tenant_label` (unit) and `warm_pool_cross_tenant_isolation` (integration) tests proving cross-tenant warm pool entries are never shared | Both tests pass; warm_pool_hits==0 for cross-tenant, warm_pool_hits==1 for same-tenant | — |
| 2026-04-08 | cleanup | done | Removed 14 dead `reset_main_realm` test functions (~690 lines) left over from D2 deprecation; removed dead `load_bundle_for_bypass_repro_without_post_return_settle` method | 71 tests pass; `make clippy` clean; `cargo fmt --all --check` clean | — |
