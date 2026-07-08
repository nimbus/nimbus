# CP Spec — `nimbus-compute` extraction (AD1, staged CP1→CP2→CP3)

Design authority: `architecture-review-2026-07-plan.md` AD1 + band CP +
the 2026-07-08 CP inventory (post DE/SR/CO refactor). Crate scope: new
`crates/nimbus-compute`, `crates/nimbus-server`, `crates/nimbus`
(facade). Pre-launch: breaking changes preferred. STRICTLY SEQUENTIAL:
CP1 fully merged before CP2, CP2 before CP3. Each is its own PR with
`make ci` green. This lane starts only AFTER de-sweep merges — not
because de-sweep touches the CP1-moved files (it does not), but because
de-sweep rewrites `adapters/cloud_functions/mod.rs` (a CP2
DeploymentState touchpoint) and `http/authz.rs` (a CP3 authz
touchpoint), so ordering it first avoids a same-crate rebase on those.

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
- CP1 candidates are axum-free but NOT all self-contained (verified
  2026-07-08 against the code, correcting the first inventory):
  - `artifact_verifier_effects.rs` + cosign/process/sbom/slsa
    (~2,739 LoC), `machine_lifecycle.rs` (62), and 7 of the 8
    `execution/**` files ARE clean moves.
  - **`execution/subscriptions.rs` is BLOCKED**: it uses
    `crate::state::AppError` (return type, subscriptions.rs:104) and
    `crate::owned_tasks::OwnedTaskSet` (field type, :17/:32/:93).
    `OwnedTaskSet` (owned_tasks.rs:7, `pub(crate)`) is server-only and
    also consumed by transport files that STAY (ws/socket.rs,
    ws/socket/{transport,session}.rs, adapters/convex/subscriptions/
    socket/mod.rs) — co-moving it would make nimbus-server↔compute
    circular. `AppError` is CP2-scheduled. Must be resolved in CP1
    (below) before subscriptions.rs can leave.
  - **`service_manager.rs` body moves; its TEST TREE does not**:
    service_manager/tests.rs (1,729 lines) + tests/{definitions,
    redaction,sandboxes,sessions}.rs depend on server-internal
    `crate::local_server::{...}` + `crate::router::RouterBuildConfig`.
    Only `attach_system_state_engine` needs to become `pub`
    (`SystemTenantServiceEvidenceWriter` can stay private in the moved
    module). `machine_lifecycle` has an EXTERNAL consumer beyond
    http/machines.rs:9 — `nimbus-cli` — so nimbus-server must
    `pub use nimbus_compute::machine_lifecycle` (re-export) to keep
    `nimbus_server::machine_lifecycle::` valid, or nimbus-cli repoints.
- Real CP1 Cargo deps (from the actual `use` statements, correcting the
  guess): [dependencies] nimbus-core, nimbus-runtime, nimbus-artifacts,
  nimbus-engine, nimbus-sandbox, nimbus-services, nimbus-system,
  nimbus-machine, **nimbus-fs**, **nimbus-bridge**, nimbus-provenance;
  serde, serde_json, tokio. [dev-dependencies] nimbus-tenant. There is
  NO reqwest/sha2/ring import anywhere (sha256 is via
  nimbus-runtime/nimbus-artifacts helpers); DROP the guessed
  nimbus-node/-workloads/-operator/adapters.
- CP2: `AppState` (state.rs:43-51) = 7 fields, exactly ONE transport-side
  (`transport: TransportConfig`, injected LATE at serve time
  construction.rs:197-199). The other 6 (engine, active_deployment,
  system_convex_registry, control_plane, node_services, runtime) are
  compute/control-plane. **CORRECTION (verified): `AppError` is NOT
  transport-free** — its `Structured(Box<StructuredHttpError>)` variant
  (state.rs:267) wraps `StructuredHttpError`, which stores an axum
  `StatusCode` and IS the `IntoResponse` type. So `AppError` CANNOT
  move to the axum-free compute crate; the "move AppError, keep
  IntoResponse in server via orphan rule" plan is unsound. `AppError`,
  `StructuredHttpError`, and the `IntoResponse` impl all STAY in
  nimbus-server. `RequestCancellationGuard` (state.rs:470) +
  `record_authenticated_usage` (state.rs:491) are transport-free (the
  latter's signature becomes `&ComputeState`). Construction:
  RouterBuildConfig (router.rs:228) → AppState::from_config
  (router.rs:472); RouterOptions (router.rs:49); ServeOptions façade
  (construction.rs:27).
- CP3: http handlers funnel to state.engine / a manager / system_tenant::*.
  `sandbox_spec.rs` (344, 0 axum) is pure DTO — reference shape.
  `deploy.rs` (588, 1 axum sig, ~95% orchestration) = biggest payoff.
  `sandboxes.rs` (400, most axum) = most transport residue.
  `services.rs` (901, 8 handlers), `scheduling.rs` (118), `machines.rs`
  (359). The http/mod.rs prelude (http/mod.rs:1-4) binds axum into all.

## CP1 — create the crate, move the transport-free modules

1. `crates/nimbus-compute/` with the VERIFIED deps: [dependencies]
   nimbus-core, nimbus-runtime, nimbus-artifacts, nimbus-engine,
   nimbus-sandbox, nimbus-services, nimbus-system, nimbus-machine,
   nimbus-fs, nimbus-bridge, nimbus-provenance; serde, serde_json,
   tokio. [dev-dependencies] nimbus-tenant. NO axum/tower/tower-http/
   tokio-rustls, and no reqwest/sha2/ring (not used). Add to workspace
   members.
2. **Resolve the two blocked files BEFORE moving** (do this first, as
   its own reviewable step):
   - `execution/subscriptions.rs`: change its return type from
     `crate::state::AppError` to `nimbus_core::Error` (subscriptions.rs
     is an engine-runtime subscription bridge; nimbus_core::Error is the
     right compute-layer error and the callers already interoperate with
     it). Keep `OwnedTaskSet` SERVER-SIDE: if `subscriptions.rs` truly
     needs an owned-task set, either move `owned_tasks.rs` into the
     move set ONLY IF its other consumers (ws/socket*, convex socket)
     also move (they do NOT in CP1 — so do not), OR restructure so the
     compute-side subscription logic returns a handle the server wraps
     in its OwnedTaskSet. Read subscriptions.rs + owned_tasks.rs and
     pick the smaller correct cut; if subscriptions.rs cannot cleanly
     shed OwnedTaskSet, EXCLUDE it from CP1 and move it in CP3 alongside
     the handler migration — record which you chose.
3. `git mv` the clean set into `crates/nimbus-compute/src/` preserving
   the module tree (`execution/` minus any excluded file,
   `artifact_verifier_effects/`, `machine_lifecycle.rs`,
   `service_manager.rs`). Make ONLY `attach_system_state_engine` `pub`
   (keep `SystemTenantServiceEvidenceWriter` private). For
   `service_manager.rs`: LEAVE its `#[cfg(test)] mod tests` tree in
   nimbus-server (it depends on server-internal `crate::local_server::*`
   + `crate::router::RouterBuildConfig`) — drop the `mod tests;`
   declaration from the moved file and re-home those tests as
   nimbus-server integration tests against the re-exported symbol, or
   note them as a CP3 follow-up. Fix intra-move `use crate::` paths.
4. nimbus-server depends on nimbus-compute; `pub use
   nimbus_compute::machine_lifecycle` (nimbus-cli consumes it) and
   re-export any other moved symbol the facade or external crates use;
   repoint nimbus-server's own references to `nimbus_compute::`. Verify
   the dep graph is ACYCLIC (no compute→server back-edge — the
   subscriptions.rs fix removes the only one).
   Acceptance: `crates/nimbus-compute/Cargo.toml` has ZERO axum/tower/
   tower-http/tokio-rustls deps AND `grep -r "hyper::" src` is empty in
   compute source (hyper may still arrive transitively via nimbus-engine
   — that's fine, only the axum surface must be absent); `cargo check
   -p nimbus-cli -p nimbus` green (facade + cli unchanged); `make ci`
   green; no behavior change.

## CP2 — split `AppState` into `ComputeState` + transport wrapper

CORRECTED DESIGN (the "move AppError via orphan rule" plan is unsound —
`AppError::Structured` wraps the axum-coupled `StructuredHttpError`):

1. In nimbus-compute: define a NEW axum-free error
   `ComputeError { Core(nimbus_core::Error), Unauthorized, Forbidden,
   NotFound }` (the four transport-free variants — DROP the
   `Structured` variant, which is inherently transport-shaped) with
   Display/Error/From<nimbus_core::Error>. `ComputeState` holds the 6
   compute/control-plane fields (engine, active_deployment,
   system_convex_registry, control_plane, node_services, runtime) — zero
   axum. Move `RequestCancellationGuard` +
   `record_authenticated_usage` (its signature becomes `&ComputeState`).
   Expand the move set as needed so ComputeState's field TYPES are all
   axum-free: `DeploymentState`/`ActiveDeployment` (state.rs:202-266),
   `ControlPlaneConfig`, `NodeServicesConfig`, `RuntimeGovernorConfig`;
   split `CloudflareConfig` out of adapters/cloudflare/mod.rs (which
   mixes the config type with an axum Router) so the config half is
   compute-side.
2. In nimbus-server: KEEP `AppError`, `StructuredHttpError`, and
   `impl IntoResponse for AppError` server-side (unchanged). `AppState`
   wraps `ComputeState` + `transport: TransportConfig`. Add the BRIDGE
   `impl From<ComputeError> for AppError` in nimbus-server so a compute
   fn returning `Result<_, ComputeError>` composes with a handler's
   `Result<_, AppError>` via `?`. Handlers reach compute fields via
   `state.compute()` or `Deref<Target = ComputeState>` (pick the
   ergonomic one, document it).
3. Split `construction.rs`/`RouterBuildConfig`:
   `ComputeState::from_config` builds the compute half; the server layer
   adds transport. `RouterOptions`/`ServeOptions`/`serve`/`build_router`
   STAY in nimbus-server with unchanged public signatures (facade
   depends on them).
   Acceptance: ComputeState + ComputeError + every moved type is
   axum-free (grep-proof); the ONLY things referencing axum are the
   server-side AppError/StructuredHttpError/IntoResponse + the
   From<ComputeError> bridge; `make ci` green; facade unchanged.

## CP3 — migrate handler orchestration into compute-owned functions

1. For each of deploy/services/sandboxes/scheduling/machines: extract
   the orchestration body into a compute-owned async fn
   `nimbus_compute::<area>::<verb>(compute: &ComputeState, <parsed
   inputs>) -> Result<Dto, ComputeError>`; the axum handler reduces to
   extract axum inputs → parse → call the compute fn (`?` lifts
   `ComputeError` into the handler's `AppError` via the CP2 bridge) →
   wrap in `Json`/status. `sandbox_spec.rs` is already DTO-only — move
   its conversion into nimbus-compute as the reference. Do `deploy.rs`
   first (biggest, cleanest payoff — ~95% orchestration, 1 axum sig).
   `sandboxes.rs` AND `services.rs` are the two most transport-coupled
   (shared HeaderMap→authz→audit residue): the authz + audit stay in the
   handler, only the manager-orchestration body lifts. Also fold in any
   `execution/subscriptions.rs` remainder deferred from CP1.
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
- nimbus-compute's Cargo.toml lists no axum/tower/tower-http/
  tokio-rustls dep, and its source contains no `axum`/`tower::`/
  `hyper::`/`http::`/`IntoResponse`/`StatusCode` reference (grep-proof
  in each PR). NOTE: `hyper` may still arrive transitively via
  nimbus-engine's dep graph — that is acceptable; only the axum SURFACE
  must be absent, so the constraint is a source-grep, not a
  transitive-dep ban.
- Mutation-path, storage-atomicity, tenant-isolation invariants
  untouched. CP1 is a near-pure move (the only edits are the
  subscriptions.rs error-type swap + path fixes). CP2 introduces the
  new `ComputeError` type + `From<ComputeError> for AppError` bridge —
  a deliberate SEAM addition, not a pure move, but observably
  behavior-preserving (the rendered HTTP error for every path is
  identical: ComputeError → AppError → StructuredHttpError → the same
  status/body as today). CP3 is body-relocation, behavior-preserving.
  Any change to a rendered response, status code, or audit event is out
  of scope and a bug.
- Each stage is observably behavior-preserving and independently
  `make ci`-green before the next starts.

## Verification (per stage, worktree root)

```
cargo fmt --all --check
cargo clippy -p nimbus-compute -p nimbus-server --all-targets -- -D warnings
grep -rE "axum|tower::|hyper::|IntoResponse|StatusCode" crates/nimbus-compute/src && echo "AXUM LEAK" || echo "compute is transport-free"
cargo test -p nimbus-compute
cargo test -p nimbus-server
cargo check -p nimbus-cli -p nimbus
make ci          # required before merging each stage
```

Update ledger rows CP1/CP2/CP3 with evidence per stage.
