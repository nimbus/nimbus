# Server Lane Spec — CO2 (operator authz), SR3 (manager verb seam), SR4 (HTTP-mount seam)

Design authority: `architecture-review-2026-07-plan.md` rows + the
2026-07-07 server inventory. Crate scope: `nimbus-server` +
`nimbus-services`. SEQUENCING: this lane starts ONLY after the wave-1
`server-consolidations` branch (CO3/CO4/CO5/DE14/DE15) merges — it
edits the same files (`http/resource_control/*`, `http/services.rs`).
Branch from post-merge main and read the landed consolidation first.
Path correction vs the plan ledger: the resource-control modules live
under `crates/nimbus-server/src/http/resource_control/`.

## Facts this rests on (inventory)

- CO2 true triplicates: `authorize_operator_session_route`
  (sessions.rs:272) / `authorize_operator_service_route`
  (services.rs:276) / `authorize_operator_sandbox_route`
  (sandboxes.rs:128) — identical flow (extract_operator_route_access →
  Authorized ⇒ operator authorization / Missing ⇒ Ok(None) / Err ⇒
  audit + reject), parameterized only by authorization type,
  tenant-context ctor, and the `auth_scope` audit literal. The four
  audit recorders (sessions.rs:463, services.rs:359,383,
  sandboxes.rs:224) are byte-identical except that literal. The
  full-route wrappers (operator gate → application-auth fallback →
  principal-claim check → resource permission check) share a skeleton
  across sessions.rs:179 / services.rs:83,187 / sandboxes.rs:57.
  Scope matchers are GENUINE domain logic and stay separate — but the
  sandbox scope matcher is duplicated a third time inside
  sessions.rs:445 (for `principal_has_sandbox_reach`).
- SR3 ladder: `_async` (system ctx) → `_for_context_async`
  (isolation→decision) → `_for_decision_async` (executes), in
  nimbus-services manager/{sandboxes,activation,handles}.rs.
  Caller census: production HTTP calls ONLY the `_for_context_async`
  rung; `_async` has one prod caller (`manager/registry.rs:59`
  start_service_async runtime auto-activation) + tests;
  `_for_decision_async` has ZERO prod callers beyond its own shim —
  it exists so tests can inject a pre-built decision to assert grant
  enforcement. `reload_service_egress_for_decision_async` has no prod
  caller at all.
- SR4: `router.rs build()` (:534-580) hardcodes 8 merges + 1 fallback.
  Variance axes: ctor shape (`fn()->Router` vs takes `Arc<AppState>` vs
  takes `Arc<Config>`), mount mode (merge vs fallback), gating
  (always-on vs `DeploymentState` option), route_layer middleware.
  The sibling `WireProtocolAdapter` seam (adapters/wire.rs:23, driven
  by construction.rs:248-266) is the template but carries `Arc<Engine>`
  not `Arc<AppState>` — it does not transfer directly.

## Target design (normative)

### CO2 — one operator-route authorization core (do first)

1. In `http/authz.rs` (which already owns the shared primitives):
   - `fn authorize_operator_route<A>(headers, state, tenant:
     &TenantId, surface: &str, scope: OperatorAuthScope, build: impl
     FnOnce(OperatorGrant) -> A) -> Result<Option<A>, AppError>` —
     the single implementation of the extract → authorized/missing/err
     flow. `OperatorAuthScope` is a small enum (SessionPrincipalClass,
     ServicePrincipalClass, ServiceDefinitionPrincipalClass,
     SandboxPrincipalClass) carrying its audit literal via
     `as_str()` — no stringly parameters at call sites.
   - ONE parameterized audit recorder
     `record_operator_authorization_audit(..., scope:
     OperatorAuthScope, ...)` replacing the four copies.
2. The three modules keep their `authorize_*_route` full-route
   wrappers but each becomes: call the shared operator gate → the
   existing application-auth fallback skeleton. If after CO2 the
   fallback skeletons are still near-identical, extract
   `resolve_application_route_auth(...)` into authz.rs too — judge by
   what the diff shows, don't force it.
3. Fold the third sandbox scope-matcher copy (sessions.rs:445): move
   `sandbox_permission_scope_allows` to sandboxes.rs pub(crate) (or
   authz.rs) and call it from sessions' reach check — signatures
   reconciled (&str vs Option<&str>).
4. Scope matchers otherwise UNTOUCHED (domain logic).

### SR3 — one entry per manager verb

1. `_for_context_async` becomes the seam (it is what production
   calls): rename nothing; instead
   - Demote every `_for_decision_async` to PRIVATE
     (`fn ..._for_decision_async` without pub, or inline into the
     context rung where trivial). `reload_service_egress_for_decision_async`:
     give it a `_for_context_async` public rung only if something will
     call it; otherwise make it private and note the dormant verb in
     the ledger evidence.
   - `_async` (system-context) rungs: keep ONLY
     `start_service_async` (real prod caller in registry.rs) —
     re-implement it as a thin private-context call at its call site or
     keep the public method; delete
     `create_sandbox_resource_async` (tests-only) and migrate its 7
     test sites to the context rung with an explicit system context.
2. The grant-enforcement tests that inject pre-built decisions
   (lifecycle.rs:294,527,609 etc.): relocate their assertions behind a
   `#[cfg(test)] pub(crate)` decision-injection hook or re-express them
   through the context rung with a fixture decision — the ENFORCEMENT
   assertions must survive with equal strength; do not delete a single
   one.
3. Workspace-wide caller migration; no wrapper aliases left.

### SR4 — HTTP-adapter mount seam

1. New `adapters/http_mount.rs`: 
   `trait HttpProtocolAdapter { fn name(&self) -> &'static str;
   fn enabled(&self, deployment: &DeploymentState) -> bool;
   fn mount(&self, router: Router<Arc<AppState>>, state: &Arc<AppState>)
   -> Router<Arc<AppState>>; }`
   — `mount` receives the router and returns it (subsumes merge vs
   fallback and lets the adapter attach its own route_layer
   middleware); adapters needing config Arcs capture them at
   registration.
2. `build()` iterates a registration list
   (`fn http_protocol_adapters(state) -> Vec<Box<dyn HttpProtocolAdapter>>`)
   for the ADAPTER routers: convex, firebase, cloudflare,
   cloud_functions (fallback — document that at most one fallback
   adapter may be enabled; assert it). The non-adapter routers
   (public/ui/local-admin/service-control/deploy) are the server's own
   surface and STAY as direct merges — this seam is for protocol
   adapters, mirroring `WireProtocolAdapter`'s scope.
3. Mount order must be preserved exactly (route shadowing); encode the
   order in the registration list and add a test asserting the built
   router serves a representative route per adapter exactly as before
   (reuse existing router tests; add gating tests: firebase absent ⇒
   404, present ⇒ mounted).

## Hard constraints

- Zero authorization-behavior change: every operator/application
  auth decision, audit event (incl. auth_scope strings, reasons,
  origins), and rejection status must be byte-identical. The existing
  resource_control test suites pass unmodified except imports/helper
  names.
- Blast radius: authz is fail-closed surface — run the FULL
  nimbus-server + nimbus-services suites, not filtered subsets.
- No public-API aliases left behind (pre-launch).
- CP coordination: this lane deliberately precedes CP1-CP3; do not
  move files across crates here.

## Required tests

1. CO2: audit-parity test — for each scope, drive the shared gate and
   assert the emitted LocalServerAuditEvent equals the pre-refactor
   shape (literal auth_scope strings pinned).
2. SR3: per-verb — context rung enforces grants (the relocated
   decision-injection assertions); tests-only rungs gone (compile
   proof).
3. SR4: adapter gating on/off per adapter; fallback uniqueness
   assertion; route-parity smoke for each mounted adapter.

## Verification gates (worktree root, report real counts)

```
cargo fmt --all --check
cargo clippy -p nimbus-server -p nimbus-services --all-targets -- -D warnings
cargo test -p nimbus-server
cargo test -p nimbus-services
cargo check -p nimbus-cli
```

Update ledger rows CO2/SR3/SR4 with evidence on completion.
