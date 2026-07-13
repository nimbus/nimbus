//! Structural regression gate (runtime-guest-trust-global-hardening, HG4/HGx
//! in the plan's terminology).
//!
//! Every finding in this plan (HG0 through HG9) was the SAME bug pattern —
//! a guest-reassignable `globalThis.__nimbus*` trust global or shared
//! global-lexical binding the trusted preamble/Rust host relies on across a
//! warm-pooled realm's invocations — discovered and fixed one property at a
//! time across four review cycles. Per-property red/green exploit tests (see
//! `node_bootstrap.rs`, `capture_ordering.rs`, `nested_dispatch.rs`) prove
//! each FIX, but none of them prevent a FUTURE trust global from being added
//! back in the vulnerable shape (a plain `globalThis.__nimbusX = value`
//! assignment, or `Object.defineProperty` with `configurable:true`).
//!
//! This module is the single gate that closes that class going forward. For
//! each runtime lane, it boots the real runtime through the actual
//! bootstrap+bundle-load path (`invoke_bundle_for_tenant` — the same public
//! entrypoint production traffic uses, not a raw isolate-slot harness),
//! enumerates the post-bootstrap-and-post-bundle-load realm's own globals via
//! `Reflect.ownKeys(globalThis)`, and classifies every `__nimbus*`-prefixed
//! name (plus the explicit non-prefixed allowlist entry, `Deno` — Finding 1)
//! against `docs/private/plans/proof/runtime-guest-trust-globals/
//! structural-gate-allowlist.json`. A future unhardened trust global —
//! whether newly added or accidentally un-hardened by an edit to an existing
//! one — either (a) is not in the fixture at all, and the gate fails naming
//! the offending global and lane, or (b) is in the fixture's
//! `trust_hardened` bucket but its live descriptor is not
//! `{writable:false, configurable:false}`, and the gate fails naming the
//! global, the lane, and the observed descriptor. Adding a new hardened
//! global is a deliberate fixture edit (mirrored in
//! `classification-ledger.md`'s "Structural-test allowlist" section); a
//! silent regression is not.
//!
//! This is a real behavioral assertion against the live V8 realm's property
//! descriptors, not a compile check or a source-text `.contains(...)` grep.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::support::*;
use super::*;

/// Embedded at compile time so the gate never depends on the test process's
/// working directory (unlike a runtime file read, this can't silently pick
/// up a stale copy from a different worktree or fail under `cargo nextest`'s
/// sandboxed cwd).
const FIXTURE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/private/plans/proof/runtime-guest-trust-globals/structural-gate-allowlist.json"
));

/// Shared JS: enumerates every guest-reachable trust-relevant global on
/// `globalThis` after bootstrap and after the bundle's own top-level code
/// (and any queued microtasks) have run, and reports each one's property
/// descriptor. Every lane's bundle below calls this from its
/// `__nimbusInvoke`/`fetch` handler so the inventory reflects the realm's
/// state at the point a real invocation would read these globals, not just
/// immediately post-bootstrap.
const INVENTORY_JS: &str = r#"
function __nimbusStructuralGateInventory() {
  const names = Reflect.ownKeys(globalThis).filter(
    (key) => typeof key === "string" && (key.startsWith("__nimbus") || key === "Deno"),
  );
  const inventory = {};
  for (const name of names) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
    inventory[name] = {
      writable: descriptor.writable === true,
      configurable: descriptor.configurable === true,
      enumerable: descriptor.enumerable === true,
      isAccessor:
        typeof descriptor.get === "function" || typeof descriptor.set === "function",
    };
  }
  return inventory;
}
"#;

#[derive(Deserialize)]
struct Fixture {
    #[serde(default)]
    #[allow(dead_code)]
    non_prefixed_allowlist: Vec<String>,
    base: Buckets,
    #[serde(default)]
    lane_overrides: BTreeMap<String, LaneOverride>,
}

#[derive(Deserialize, Default)]
struct Buckets {
    #[serde(default)]
    trust_hardened: Vec<String>,
    #[serde(default)]
    trust_removed: Vec<String>,
    #[serde(default)]
    intentionally_mutable: Vec<String>,
    #[serde(default)]
    compat: Vec<String>,
}

#[derive(Deserialize, Default)]
struct LaneOverride {
    #[serde(default)]
    move_to_trust_hardened: Vec<String>,
    #[serde(default)]
    move_to_trust_removed: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bucket {
    TrustHardened,
    TrustRemoved,
    IntentionallyMutable,
    Compat,
}

impl Fixture {
    fn load() -> Self {
        serde_json::from_str(FIXTURE_JSON)
            .expect("structural-gate-allowlist.json should parse as the documented schema")
    }

    /// Classification for `name` on `lane`, applying that lane's override
    /// (if any) before falling back to the base classification. Returns
    /// `None` when `name` is not listed anywhere — the "new unhardened
    /// global" failure case.
    fn classify(&self, lane: &str, name: &str) -> Option<Bucket> {
        if let Some(overrides) = self.lane_overrides.get(lane) {
            if overrides.move_to_trust_hardened.iter().any(|n| n == name) {
                return Some(Bucket::TrustHardened);
            }
            if overrides.move_to_trust_removed.iter().any(|n| n == name) {
                return Some(Bucket::TrustRemoved);
            }
        }
        if self.base.trust_hardened.iter().any(|n| n == name) {
            return Some(Bucket::TrustHardened);
        }
        if self.base.trust_removed.iter().any(|n| n == name) {
            return Some(Bucket::TrustRemoved);
        }
        if self.base.intentionally_mutable.iter().any(|n| n == name) {
            return Some(Bucket::IntentionallyMutable);
        }
        if self.base.compat.iter().any(|n| n == name) {
            return Some(Bucket::Compat);
        }
        None
    }
}

#[derive(Deserialize, Debug, Clone, Copy)]
struct GlobalDescriptor {
    writable: bool,
    configurable: bool,
    #[allow(dead_code)]
    enumerable: bool,
    #[serde(rename = "isAccessor")]
    #[allow(dead_code)]
    is_accessor: bool,
}

fn parse_inventory(value: &Value) -> BTreeMap<String, GlobalDescriptor> {
    serde_json::from_value(value.clone())
        .expect("bundle should return the __nimbusStructuralGateInventory() JSON shape")
}

/// The core gate assertion. Every observed global on `lane` must classify;
/// every `trust_hardened` global that is present must be fully slot-hardened
/// (`writable:false, configurable:false`); every `trust_removed` global must
/// not be present at all. `intentionally_mutable` and `compat` globals carry
/// no hardening requirement — their presence (mutable or not) is documented
/// as safe in the ledger.
fn assert_structural_gate(lane: &str, inventory: &BTreeMap<String, GlobalDescriptor>) {
    let fixture = Fixture::load();
    for (name, descriptor) in inventory {
        match fixture.classify(lane, name) {
            None => panic!(
                "structural gate FAILED: guest-reachable global '{name}' on lane '{lane}' is not \
                 classified in structural-gate-allowlist.json (observed writable={}, \
                 configurable={}). A new trust global must be a deliberate allowlist edit \
                 (docs/private/plans/proof/runtime-guest-trust-globals/structural-gate-allowlist.json \
                 + classification-ledger.md's \"Structural-test allowlist\" section), not a silent \
                 addition.",
                descriptor.writable, descriptor.configurable
            ),
            Some(Bucket::TrustHardened) => assert!(
                !descriptor.writable && !descriptor.configurable,
                "structural gate FAILED: trust global '{name}' on lane '{lane}' is NOT fully \
                 slot-hardened (observed writable={}, configurable={}; expected writable:false, \
                 configurable:false). A guest can reassign or redefine this slot and poison a \
                 later same-tenant invocation on a warm-pooled realm.",
                descriptor.writable,
                descriptor.configurable
            ),
            Some(Bucket::TrustRemoved) => panic!(
                "structural gate FAILED: '{name}' must be ABSENT on lane '{lane}' but was found \
                 present (observed writable={}, configurable={}).",
                descriptor.writable, descriptor.configurable
            ),
            Some(Bucket::IntentionallyMutable) | Some(Bucket::Compat) => {
                // No hardening requirement -- documented app-singleton /
                // bootstrap-transient surface, not a trust decision.
            }
        }
    }
}

/// Completeness check for the small set of trust globals a lane's own
/// bundle setup here guarantees are wired: catches a hardening regression
/// that manifests as the global silently disappearing (e.g. a rename that
/// left the fixture and the bootstrap script out of sync) rather than
/// appearing unhardened.
fn assert_present(lane: &str, inventory: &BTreeMap<String, GlobalDescriptor>, names: &[&str]) {
    for name in names {
        assert!(
            inventory.contains_key(*name),
            "structural gate FAILED: expected trust global '{name}' to be present on lane \
             '{lane}' but it was not found in the inventory -- the fixture and the bootstrap \
             wiring have drifted out of sync"
        );
    }
}

fn assert_absent(lane: &str, inventory: &BTreeMap<String, GlobalDescriptor>, names: &[&str]) {
    for name in names {
        assert!(
            !inventory.contains_key(*name),
            "structural gate FAILED: expected '{name}' to be absent on lane '{lane}' but it was \
             found present (this lane must never define it)"
        );
    }
}

fn default_lane_bundle_source() -> String {
    // Mirrors packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs's
    // real HG0 install shape -- Object.defineProperty with configurable:false,
    // writable:false, as the first statement -- rather than a plain
    // `globalThis.__nimbusInvoke = ...` assignment. A hand-rolled test bundle
    // that used the plain-assignment shape would install __nimbusInvoke
    // UNHARDENED itself and make this gate fail on every lane regardless of
    // what the real codegen preamble does, which is not the regression this
    // gate is meant to catch.
    format!(
        r#"{INVENTORY_JS}
Object.defineProperty(globalThis, "__nimbusInvoke", {{
  value: function () {{
    return __nimbusStructuralGateInventory();
  }},
  configurable: false,
  enumerable: false,
  writable: false,
}});

export {{}};
"#
    )
}

fn default_lane_request() -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

// ---------------------------------------------------------------------
// web (default/WebStandardIsolate) lane
// ---------------------------------------------------------------------

#[tokio::test]
async fn structural_gate_web_lane_trust_globals_are_hardened_or_classified() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, default_lane_bundle_source()).expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &default_lane_request(),
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    let inventory = parse_inventory(&result);
    assert_absent("web", &inventory, &["Deno"]);
    assert_present(
        "web",
        &inventory,
        &[
            "__nimbusInvoke",
            "__nimbusCreateContext",
            "__nimbusSyncHostValue",
        ],
    );
    assert_structural_gate("web", &inventory);
}

// ---------------------------------------------------------------------
// node22 lane
// ---------------------------------------------------------------------

fn node22_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22()))
}

#[tokio::test]
async fn structural_gate_node22_lane_trust_globals_are_hardened_or_classified() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, default_lane_bundle_source()).expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        node22_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &default_lane_request(),
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    let inventory = parse_inventory(&result);
    assert_present(
        "node22",
        &inventory,
        &[
            "__nimbusInvoke",
            "__nimbusCreateContext",
            "Deno",
            "__nimbusRefreshNodeProcessCwd",
        ],
    );
    assert_structural_gate("node22", &inventory);
}

/// Fresh-realm-recycle variant: the same lane, but on `WarmContextRecycle` +
/// `CooperativeLocker` instead of the default startup-snapshot pool, so the
/// gate also covers a realm that gets torn down and rebuilt between
/// invocations rather than only the warm-reused main context.
#[tokio::test]
async fn structural_gate_node22_lane_survives_fresh_realm_recycle() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, default_lane_bundle_source()).expect("bundle should write");

    let limits = RuntimeLimits {
        runtime_pool_kind: crate::limits::RuntimePoolKind::WarmContextRecycle,
        execution_model: crate::limits::RuntimeExecutionModel::CooperativeLocker,
        // WarmContextRecycle on a Node target requires an explicit proof that
        // the recycled realm is same-owner/exact-authority (limits/axes.rs);
        // otherwise RuntimePolicy::new rejects the combination outright.
        node_full_realm_reuse_policy:
            crate::limits::RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority,
        ..RuntimeLimits::application_node22()
    };
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &default_lane_request(),
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    let inventory = parse_inventory(&result);
    assert_present("node22", &inventory, &["__nimbusInvoke", "Deno"]);
    assert_structural_gate("node22", &inventory);
}

// ---------------------------------------------------------------------
// cloudflare lane
// ---------------------------------------------------------------------

fn cloudflare_worker_bundle_source() -> String {
    format!(
        r#"{INVENTORY_JS}
export default {{
  async fetch(request, env, ctx) {{
    const inventory = __nimbusStructuralGateInventory();
    return new Response(JSON.stringify(inventory), {{ status: 200 }});
  }},
}};
"#
    )
}

fn cloudflare_worker_request() -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::CloudflareWorkerFetch,
        function_name: "worker:fetch".to_string(),
        args: serde_json::json!({
            "request": { "url": "https://example.com/gate", "method": "GET", "headers": [], "body": null },
            "env": {},
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

async fn invoke_cloudflare_gate(policy: Arc<RuntimePolicy>) -> BTreeMap<String, GlobalDescriptor> {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(&bundle_path, cloudflare_worker_bundle_source())
        .expect("worker bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        policy,
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_request(),
            "tenant-a",
        )
        .await
        .expect("Cloudflare Worker fetch should execute");

    let body = result["body"]
        .as_str()
        .expect("worker response body should be text");
    parse_inventory(&serde_json::from_str(body).expect("worker response body should be JSON"))
}

#[tokio::test]
async fn structural_gate_cloudflare_lane_trust_globals_are_hardened_or_classified() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let inventory = invoke_cloudflare_gate(run_to_completion_snapshot_runtime_test_policy()).await;

    assert_absent("cloudflare", &inventory, &["Deno"]);
    assert_present(
        "cloudflare",
        &inventory,
        &["__nimbusInvokeCloudflareWorkerFetch"],
    );
    assert_structural_gate("cloudflare", &inventory);
}

/// Fresh-realm-recycle variant, matching `capture_ordering.rs`'s Cloudflare
/// fresh-realm coverage: the gate must hold for a recycled realm, not only
/// the warm-reused main context.
#[tokio::test]
async fn structural_gate_cloudflare_lane_survives_fresh_realm_recycle() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let inventory = invoke_cloudflare_gate(Arc::new(RuntimePolicy::new(
        cooperative_context_recycle_runtime_test_limits(),
    )))
    .await;

    assert_absent("cloudflare", &inventory, &["Deno"]);
    assert_present(
        "cloudflare",
        &inventory,
        &["__nimbusInvokeCloudflareWorkerFetch"],
    );
    assert_structural_gate("cloudflare", &inventory);
}

// ---------------------------------------------------------------------
// cloud_functions lane (codegen's second __nimbusInvoke emit site)
// ---------------------------------------------------------------------

fn cloud_functions_bundle_source() -> String {
    format!(
        r#"{INVENTORY_JS}
// Mirrors packages/codegen/src/cloud_functions/runtime_sources.mjs's
// createInvocationDispatcher() install shape (the plan's stated "Cloud
// Functions codegen" coverage axis, second HG0 emit site) -- app-singleton
// state via `??=`, dispatcher built by a factory function, installed with
// Object.defineProperty {{configurable:false, writable:false}}.
globalThis.__nimbusCloudFunctionsState ??= {{ targets: [] }};
globalThis.__nimbusAdminApps ??= [];
const __nimbusCollectedTargets = [];
function createInvocationDispatcher(targets) {{
  return async function () {{
    return __nimbusStructuralGateInventory();
  }};
}}
Object.defineProperty(globalThis, "__nimbusInvoke", {{
  value: createInvocationDispatcher(__nimbusCollectedTargets),
  configurable: false,
  enumerable: false,
  writable: false,
}});

export {{}};
"#
    )
}

#[tokio::test]
async fn structural_gate_cloud_functions_lane_trust_globals_are_hardened_or_classified() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, cloud_functions_bundle_source()).expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &default_lane_request(),
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    let inventory = parse_inventory(&result);
    assert_absent("cloud_functions", &inventory, &["Deno"]);
    assert_present(
        "cloud_functions",
        &inventory,
        &[
            "__nimbusInvoke",
            "__nimbusCloudFunctionsState",
            "__nimbusAdminApps",
        ],
    );
    assert_structural_gate("cloud_functions", &inventory);
}

// ---------------------------------------------------------------------
// convex_default lane (RuntimeGuestSemantics::ConvexDefault)
// ---------------------------------------------------------------------

fn convex_default_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(RuntimeLimits {
        guest_semantics: crate::RuntimeGuestSemantics::ConvexDefault,
        ..run_to_completion_snapshot_runtime_test_limits()
    }))
}

#[tokio::test]
async fn structural_gate_convex_default_lane_trust_globals_are_hardened_or_classified() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(&default_lane_bundle_source());

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        convex_default_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &default_lane_request(),
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    let inventory = parse_inventory(&result);
    assert_absent("convex_default", &inventory, &["Deno"]);
    assert_present(
        "convex_default",
        &inventory,
        &[
            "__nimbusInvoke",
            "__nimbusCreateContext",
            "__nimbusBeginGuestInvocation",
        ],
    );
    assert_structural_gate("convex_default", &inventory);
}
