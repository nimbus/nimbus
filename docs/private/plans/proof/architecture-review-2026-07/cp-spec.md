# CP Spec — `nimbus-compute` extraction (AD1, staged CP1→CP2→CP3)

Design authority: `architecture-review-2026-07-plan.md` AD1 + band CP +
the 2026-07-08 CP inventory (post DE/SR/CO refactor). Crate scope: new
`crates/nimbus-compute`, `crates/nimbus-server`, `crates/nimbus`
(facade). Pre-launch: breaking changes preferred. STRICTLY SEQUENTIAL:
CP1 fully merged before CP2, CP2 before CP3. Each is its own PR with
`make ci` green. This lane starts only AFTER de-sweep merges (both edit
nimbus-server heavily).

## Naming (taste note for the owner)

AD1 named the crate `nimbus-compute` — "the compute plane between the
transport (nimbus-server) and the engine/runtime/sandbox." The
inventory confirms a clean seam. Proceeding with `nimbus-compute` per
AD1; if the owner prefers another name (`nimbus-workload`,
`nimbus-plane`), it is a mechanical rename before CP1 lands — flag in
the CP1 PR.

## Facts this rests on (inventory)

- No `crates/nimbus-compute` exists (greenfield). `crates/nimbus/src/lib.rs:75`
  re-exports `ServeOptions/serve/RouterOptions/build_router` from
  nimbus-server — these MUST stay in nimbus-server.
- 53/324 nimbus-server files touch axum (~16%); the compute plane is the
  other ~271. nimbus-server/Cargo.toml already depends on every
  workspace crate a compute crate needs; axum/tower/hyper/tower-http/
  tokio-rustls stay server-only.
- CP1 candidates (all zero-axum, movable as-is): `execution/**` (8 files,
  ~866 LoC), `artifact_verifier_effects.rs` + cosign/process/sbom/slsa
  (~2,739 LoC), `machine_lifecycle.rs` (62 — a trait + DTOs, imported by
  http/machines.rs:9), `service_manager.rs` (36 — `pub(crate)` today,
  must become `pub`).
- CP2: `AppState` (state.rs:43-51) = 7 fields, exactly ONE transport-side
  (`transport: TransportConfig` = listen_addr/server_shutdown/cors/
  version_check, injected LATE at serve time construction.rs:197-199).
  The other 6 (engine, active_deployment, system_convex_registry,
  control_plane, node_services, runtime) are compute/control-plane, no
  axum types. Single axum coupling: `impl IntoResponse for AppError`
  (state.rs:291) + its import (state.rs:3); the `AppError` enum itself
  (state.rs:267) is transport-free. `RequestCancellationGuard`
  (state.rs:470) + `record_authenticated_usage` (state.rs:491) are
  transport-free. Construction: RouterBuildConfig (router.rs:228) →
  AppState::from_config (router.rs:472); RouterOptions (router.rs:49);
  ServeOptions façade (construction.rs:27).
- CP3: http handlers funnel to state.engine / a manager / system_tenant::*.
  `sandbox_spec.rs` (344, 0 axum) is pure DTO — reference shape.
  `deploy.rs` (588, 1 axum sig, ~95% orchestration) = biggest payoff.
  `sandboxes.rs` (400, most axum) = most transport residue.
  `services.rs` (901, 8 handlers), `scheduling.rs` (118), `machines.rs`
  (359). The http/mod.rs prelude (http/mod.rs:1-4) binds axum into all.

## CP1 — create the crate, move the transport-free modules

1. `crates/nimbus-compute/` with Cargo.toml depending on the workspace
   crates the moved code uses (nimbus-engine, -runtime, -sandbox,
   -services, -machine, -node, -artifacts, -provenance, -workloads,
   -operator, -tenant, -system, -core, adapters as needed) + the
   non-transport third-party (reqwest, sha2, ring, serde, tokio, ...).
   NO axum/tower/hyper. Add to workspace members.
2. `git mv` the 15 files as-is into `crates/nimbus-compute/src/`
   preserving the module tree (`execution/`, `artifact_verifier_effects/`,
   `machine_lifecycle.rs`, `service_manager.rs`). Change `pub(crate)`
   items that are now cross-crate to `pub` (service_manager.rs's
   `SystemTenantServiceEvidenceWriter` + `attach_system_state_engine`).
   Fix intra-move `use crate::` paths to the new crate root; fix
   nimbus-server's references to these modules to `nimbus_compute::`.
3. nimbus-server depends on nimbus-compute and re-exports any moved
   symbol that its own public API or the facade exposed (so
   `crates/nimbus/src/lib.rs:75` and all callers compile unchanged).
   Acceptance: `crates/nimbus-compute/Cargo.toml` has ZERO
   axum/tower/http-transport deps (grep proof); `make ci` green; server
   re-exports compile; no behavior change (pure move).
4. Tests move with their modules; run nimbus-compute's suite +
   nimbus-server's.

## CP2 — split `AppState` into `ComputeState` + transport wrapper

1. In nimbus-compute: `ComputeState` holding the 6 compute/control-plane
   fields (engine, active_deployment, system_convex_registry,
   control_plane, node_services, runtime) — zero axum. Move `AppError`
   (the enum + Display/Error/From) to nimbus-compute; move
   `RequestCancellationGuard` + `record_authenticated_usage` too.
2. In nimbus-server: `AppState` wraps `ComputeState` + the
   `transport: TransportConfig`. The `impl IntoResponse for AppError`
   STAYS in nimbus-server (orphan rule — AppError is now foreign, axum
   is foreign; nimbus-server owns the bridge via the existing
   `StructuredHttpError::from_app_error(...).into_response()`). Handlers
   reach compute fields via `state.compute()` (or Deref to ComputeState
   — choose the ergonomic one, document it).
3. Split `construction.rs`/`RouterBuildConfig` the same way:
   `ComputeState::from_config` builds the compute half; the server layer
   adds transport. `RouterOptions`/`ServeOptions`/`serve`/`build_router`
   STAY in nimbus-server with unchanged public signatures (facade
   depends on them). Late-injected listen_addr/server_shutdown stay
   transport-side.
   Acceptance: ComputeState has no axum type; the ONLY axum-touching
   thing referencing AppError is the server-side IntoResponse impl;
   `make ci` green; facade unchanged.

## CP3 — migrate handler orchestration into compute-owned functions

1. For each of deploy/services/sandboxes/scheduling/machines: extract
   the orchestration body into a compute-owned async fn
   `nimbus_compute::<area>::<verb>(compute: &ComputeState, <parsed
   inputs>) -> Result<Dto, AppError>`; the axum handler reduces to
   extract axum inputs → parse → call the compute fn → wrap in
   `Json`/status. `sandbox_spec.rs` is already DTO-only — move its
   conversion into nimbus-compute as the reference. Do `deploy.rs`
   first (biggest, cleanest payoff), `sandboxes.rs` last (most transport
   residue — leave the genuinely-transport bits in the handler).
2. Coordinate with the already-landed SR3 (`_for_context_async` verb
   seam), SR4 (http-adapter mount), CO2 (authz), CO3/CO4 (pagination/
   lifecycle helpers) — the compute fns call the consolidated seams; do
   NOT re-introduce per-handler duplication.
3. Optionally point `nimbus-cli` start/dev wiring at nimbus-compute
   directly if it reduces coupling — judge, report.
   Acceptance: each axum handler is a thin extract→call→respond; the
   orchestration is compute-owned and unit-testable without axum; every
   existing http/adapter/e2e test passes unchanged; `make ci` green.

## Hard constraints (all stages)

- The facade (`crates/nimbus/src/lib.rs:75`) keeps re-exporting
  `ServeOptions/serve/RouterOptions/build_router` from nimbus-server
  with unchanged signatures.
- nimbus-compute never depends on axum/tower/hyper/tower-http/
  tokio-rustls (grep-proof in each PR).
- Mutation-path, storage-atomicity, tenant-isolation invariants
  untouched — this is a MOVE + SEAM, no logic change. Any behavior
  change is out of scope and a bug.
- Each stage is behavior-preserving and independently `make ci`-green
  before the next starts.

## Verification (per stage, worktree root)

```
cargo fmt --all --check
cargo clippy -p nimbus-compute -p nimbus-server --all-targets -- -D warnings
grep -rE "axum|tower::|hyper::" crates/nimbus-compute/src && echo "AXUM LEAK" || echo "compute is transport-free"
cargo test -p nimbus-compute
cargo test -p nimbus-server
cargo check -p nimbus-cli -p nimbus
make ci          # required before merging each stage
```

Update ledger rows CP1/CP2/CP3 with evidence per stage.
