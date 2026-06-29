# CFA0 — Baseline Proof

Starting state for the Cloudflare Adapters plan
(`docs/private/plans/cloudflare-adapters-plan.md`), captured 2026-06-16.

## Starting state (before any CFA band)

- **No Cloudflare adapter.** `crates/nimbus-server/src/adapters/` contains
  `mongodb/`, `dynamodb/`, `firebase/`, `cloud_functions/`, `convex/` — there is
  no `cloudflare/` module. `adapters/mod.rs` does not declare
  `pub mod cloudflare;`.
- **No Cloudflare host-call surface.** `crates/nimbus-runtime/src/host.rs`
  `HostCallOperation` has no `CfKv*` (or Durable-Object) variants.
- **No single-instance DO resource.** `crates/nimbus-services/src/catalog.rs`
  `ServiceBackend`/`SessionTarget` have no per-instance / single-instance
  Durable-Object resource.
- **No verifier, research doc, operator doc, or proof bundle** for this plan
  prior to CFA0.

## Structural template (what CFA reuses)

The existing inbound adapters are the template — CFA slots in beside them on the
same seam:

- Registration: `adapters/mod.rs` (`pub mod <name>;`) + `construction.rs`
  (`ServeOptions::with_*`) + `router.rs` (`build_*_router`) +
  `crates/nimbus-bin/src/start/adapters.rs` (`AdapterEnablement`).
- Runtime binding surface: a per-adapter `HostBridge` implementation translating
  a provider-shaped JS API onto the engine via `HostCallOperation`
  (`crates/nimbus-runtime/src/host.rs`); Convex's `host_bridge/` is the richest
  example.
- Storage: the engine mutation path (`apply_mutation_with_mode*`) + the
  `nimbus-storage` point-read / point-write / range-scan traits.
- Coordination: the `nimbus-services` catalog (named services, sessions) + the
  `nimbus-engine` scheduler + the `nimbus-server` WS transport.

## Ratified decisions (owner, 2026-06-16)

1. **Inbound first.** Nimbus impersonates Cloudflare; outbound (CF storage under
   Nimbus) is a separate future direction, noted but not built here.
2. **Core five researched** — Workers runtime, KV, D1, R2, Durable Objects.
3. **Build wedge = Workers KV first, then Durable Objects.** D1, R2, and full
   Worker-code execution are named follow-on bands, not this wave.
4. **License posture is clear.** `workerd` (Apache-2.0), `miniflare` (MIT),
   `workers-types` (MIT/Apache) are all freely incorporable
   ([[feedback_apache_license_posture]]); preserve LICENSE/NOTICE on lifted code.

## Two overturned assumptions (recorded so they don't resurface)

- **KV `getWithMetadata` returns `{ value, metadata }` only** — there is no
  documented `cacheStatus` field. Any cache-state reporting in Nimbus is an
  explicit Nimbus extension, not the Cloudflare contract.
- **DO `serializeAttachment` limit is 16 KiB**, not 2 KiB. The 2048-character
  limit belongs to `setWebSocketAutoResponse`, a different surface.

## 2026-06-22 re-architecture: primitives-first

After the owner reviewed the storage portfolio, the plan was re-architected from
"the adapter implements KV/DO" to **"build first-class Nimbus primitives, then
thin Cloudflare surfaces over them."** Decisions:

- **Runtime: reimplement, not embed `workerd`.** A second research pass confirmed
  `workerd` is not an embeddable library (subprocess + capnp only), bundles a
  second V8, routes every binding over a process boundary, and — decisively —
  **locks Durable-Object storage to its own local SQLite with no host seam**,
  which would break Nimbus's engine-owned-storage + tenant-isolation invariants.
  The Workers runtime is reimplemented as a V8 profile on `nimbus-runtime`;
  embedding stays a possible future alternate backend. (Research §9b.)
- **KV primitive = `TenantKvStore` seam in `nimbus-storage`, built by NKV0 F2.**
  The metadata-plane trait lives in storage and is owned by the `nimbus-kv`
  program; CFA2 is a prerequisite gate, not a second implementation.
- **Durable-object substrate** = `nimbus-services` single-instance resource +
  engine serialized mutation + per-instance storage namespace + scheduler + WS.
- **Wedge bar raised** to `env.NS` end-to-end inside a real Worker, pulling a
  minimal Workers-runtime slice forward (CFA4).
- **Deferrals:** R2 → NOS Phase 3 (object-storage primitive); cluster-scale
  single-instance DO routing → HS5; D1 over the existing SQLite/libSQL family.

Ledger grew from CFA0..CFA7 to **CFA0..CFA9**; the verifier from 10 to **12
conditions** (the 2026-06-23 plan audit added condition 12, the security-posture
gate — loopback bind guard + auth + DynamoDB-style credential→tenant binding on
the CF KV REST + Workers ingress surfaces).

## Verifier baseline

`bash scripts/verify-cloudflare-adapters.sh` at CFA0 is expected to pass
conditions 1–3 (plan, routing, research doc + this baseline proof) and fail
4–12 until the corresponding bands land. That FAIL-until-built state is the
correct day-one baseline (the verifier ships in CFA0 so `/goal` is checkable
from the start).
