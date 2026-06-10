# MBA3 Registration Seam Decision

Date: 2026-05-27

posture: explicit_typed_registry
allowed_boundaries: `PersistenceProvider`, `TenantProviderBootstrapPlan`, `ProviderBackgroundTask`, adapter route/capability inventory owned by `crates/nimbus-server/src/system_tenant/inventory.rs`

## Current Inventory Use

Nimbus does not currently use the Rust `inventory` crate as an adapter or
storage-backend registration primitive. `inventory` appears in `Cargo.lock`
through `deno_core`; it is not a direct dependency of the Nimbus crates. The
`inventory` name in `crates/nimbus-server/src/system_tenant/inventory.rs` is a
manual system-tenant route/capability listing, not linker-section
self-registration.

Current storage provider selection is intentionally typed:

- `crates/nimbus-engine/src/persistence/provider.rs` owns
  `PersistenceProvider` and provider-specific opened-tenant/background-task
  shapes.
- `crates/nimbus-engine/src/persistence_config.rs` owns
  `TenantProviderBootstrapPlan`, where topology, dialect, routing, and
  control-plane selection become concrete provider plans.
- `crates/nimbus-engine/src/persistence.rs` keeps provider dispatch explicit
  through the `match_persistence_provider!` macro.

## Decision

Keep the explicit typed registry for built-in Nimbus storage providers and
compatibility adapters. Adding a backend should require touching a small,
documented set of typed boundaries rather than relying on linker-section
registration.

The ExtendDB `inventory::submit!` pattern remains useful source evidence for a
future plugin-style architecture where backends live in separate crates and need
to self-register without a central product binary knowing every provider. That
is not the current Nimbus shape. Today, the engine needs typed ownership over
opened tenant handles, background worker hooks, topology selection, and
capability reporting.

## Guardrail

Do not add a direct `inventory` dependency under this plan unless a later ADR
or proof file changes the posture to `inventory_registration` and names the
cross-crate plugin boundary that makes self-registration necessary.
