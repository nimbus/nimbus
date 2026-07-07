# GR2 Spec — egress posture is a named constructor argument

Design authority: `docs/private/plans/architecture-review-2026-07-plan.md`
GR2 + the 2026-07-06 wiring inventory. Pre-launch rules: breaking change,
no compat shims.

## Facts this rests on

- `NimbusRuntime` holds `egress_gateway: RuntimeEgressGatewayBinding`
  (runtime.rs:49,56); `with_policy`/`new`/`with_limits`
  (runtime/facade.rs:15-30) silently default it to `CoarsePermissions`;
  `with_egress_gateway` (facade.rs:32) is an optional builder nothing
  forces.
- The isolate fetch hook (bootstrap/extensions.rs:360-413) branches on
  the binding; `CoarsePermissions` = coarse deno_permissions net check —
  the implicit fallback GR2 eliminates as a *default* (it stays available
  as an explicit, named choice).
- nimbus-server already pairs correctly via
  `runtime_for_host_with_egress_gateway` (execution/invocations/
  mod.rs:131-141, bound `H: HostBridge + EgressGateway`); its helper
  `runtime_for_host` (mod.rs:122) is only called by the pairing fn.
- `ConvexHostBridge` and `CloudFunctionsHostBridge` implement both traits
  (both delegate to `nimbus_bridge::egress::authorize_runtime_egress`).
  `CloudflareHostBridge` (adapters/cloudflare/host_bridge.rs) implements
  only `HostBridge`, is `dead_code` outside tests (CFA5 pending), and its
  test builds the runtime via bare `with_policy` — the concrete gap.

## Changes (normative)

### 1. nimbus-runtime — mandatory egress posture

Add a PUBLIC posture type in `crates/nimbus-runtime/src/egress.rs`:

```rust
/// How an isolate's outbound fetch is authorized. Every runtime
/// construction must name one — there is no implicit default.
pub enum RuntimeEgressPosture {
    /// Per-request authorization through an EgressGateway (production).
    Gateway(Arc<dyn EgressGateway>),
    /// Coarse deno_permissions net checks only. For runtimes whose
    /// profile carries no tenant egress policy; names the fallback the
    /// old constructors applied silently.
    CoarsePermissions,
}
```

Change the constructors (BREAKING, no deprecated shims):

```rust
impl NimbusRuntime {
    pub fn new(host: Arc<dyn HostBridge>, limits: RuntimeLimits, posture: RuntimeEgressPosture) -> Self;
    pub fn with_limits(...same addition...);
    pub fn with_policy(host: Arc<dyn HostBridge>, policy: Arc<RuntimePolicy>, posture: RuntimeEgressPosture) -> Self;
}
```

- Map `RuntimeEgressPosture` into the existing `RuntimeEgressGatewayBinding`
  (which stays `pub(crate)`; `Missing` arm may now be deleted if genuinely
  unreachable — check and remove dead arms rather than keep them).
- DELETE `with_egress_gateway` (its job moved into construction). If any
  call site legitimately needs post-construction override, it doesn't —
  update it to construct correctly instead.
- Update every in-crate test call site to name a posture explicitly
  (most become `RuntimeEgressPosture::CoarsePermissions`; the gateway
  tests pass their gateway at construction).

### 2. nimbus-server — collapse the helper pair

`runtime_for_host` + `runtime_for_host_with_egress_gateway` become ONE
function (keep the `with_egress_gateway` name and the
`H: HostBridge + EgressGateway` bound) that passes
`RuntimeEgressPosture::Gateway(...)` at construction. No server path may
construct a runtime with `CoarsePermissions`.

### 3. Cloudflare — explicit posture + the missing test

- Implement `EgressGateway` for `CloudflareHostBridge` IF its state can
  reach `nimbus_bridge::egress::authorize_runtime_egress` the same way
  Convex/CloudFunctions do (inspect what state that helper needs — if the
  KV-only bridge lacks it, do NOT fake it).
- If a real impl is not honestly wireable yet: the bridge stays
  HostBridge-only, and its (test) wiring constructs the runtime with
  `RuntimeEgressPosture::Gateway(Arc::new(DenyAllEgressGateway))` — the
  deny is now NAMED at the wiring site with a comment pointing at the
  Cloudflare adapters plan (CFA), not implied by a default.
- Either way, ADD the missing test: a Cloudflare-profile guest `fetch`
  is denied end-to-end (today untested). Assert the denial error, not
  just is_err.

### 4. Ripple

Update every other `NimbusRuntime::{new,with_policy,with_limits}` call
site across the workspace (nimbus-bridge tests, nimbus-testing fixtures,
nimbus-system dev-deps, benches — grep exhaustively). Tests name
`CoarsePermissions` unless they are exercising gateway behavior. No call
site may be left on an old signature (it won't compile — that is the
point).

## Tests (required)

1. Cloudflare-profile fetch denied end-to-end (new; see §3).
2. Existing gateway tests green with gateway-at-construction.
3. A doc/compile-level proof that construction without a posture is
   impossible (the signature change itself enforces this; add a
   `compile_fail` doctest showing the two-arg `with_policy` no longer
   compiles).
4. Existing egress tests (isolate_fetch_consults_egress_gateway_*,
   fail-closed proxy-enforced, extensions hook-ordering) all green.

## Verification gates (worktree root, in order — blast radius: this is a
fail-closed construction change touching a public constructor)

```
cargo fmt --all --check
cargo clippy -p nimbus-runtime -p nimbus-server --all-targets -- -D warnings
cargo test -p nimbus-runtime
cargo test -p nimbus-server
cargo test -p nimbus-bridge -p nimbus-testing -p nimbus-system
cargo check -p nimbus-cli
```

Report real per-suite counts. CI's full workspace suite is the final
oracle.
