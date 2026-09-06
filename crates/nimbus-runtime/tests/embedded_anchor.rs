//! Exercises the EMBEDDED NodeFull(Node22) anchor snapshot install — the serving path the cfg(test)
//! cage oracle cannot reach.
//!
//! Under cfg(test), `snapshot_extensions` adds a test-only extension, so the bootstrap snapshot's
//! provenance no longer matches the generated production blob and `try_embedded` correctly falls back
//! to a runtime build. As an INTEGRATION test, nimbus-runtime is linked NON-cfg(test): provenance is
//! the production value, it matches the generated `.bin`/`.pc.bin`, and the anchor installs FROM the
//! embedded blob (a ~19ms deserialize instead of a ~4.18s build).
//!
//! Run feature-off (default lane) this installs a per-isolate read-only heap; run feature-on
//! (`--features v8-pointer-compression`, the `rust-runtime-ptrcomp-check` CI job) it installs the
//! embedded NodeFull superset into the real shared cage. Either way the generated blob must
//! deserialize into a working NodeFull isolate — if it were stale or V8-incompatible, this aborts
//! loud (provenance `Err`, or a V8 read-only-heap `V8_Fatal`). The same process then builds and
//! restores the service-bearing NodeFull snapshot. Running this executable under a filesystem
//! sandbox that denies Deno source checkouts proves both service-snapshot source consumers use the
//! packaged table.

use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use nimbus_runtime::{
    HostBridge, HostCallRequest, InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle,
    RuntimeLimits, RuntimeOwnerClass, RuntimeOwnerId, RuntimeOwnerLeaseIssuer, RuntimePolicy,
};
use serde_json::json;

#[derive(Debug)]
struct NoopHost;

impl HostBridge for NoopHost {
    fn call(&self, _request: HostCallRequest) -> nimbus_runtime::Result<serde_json::Value> {
        Err(nimbus_runtime::NimbusRuntimeError::Contract(
            "packaged service-snapshot smoke must not call the host".to_string(),
        ))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_nodefull_anchor_installs_from_generated_blob() {
    nimbus_runtime::smoke_install_generated_embedded_anchor().expect(
        "the generated embedded NodeFull(Node22) anchor snapshot should deserialize and install a \
         working isolate on the serving path",
    );

    let temporary = tempfile::tempdir().expect("service-snapshot smoke directory should build");
    let bundle_path = temporary.path().join("service-snapshot-smoke.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  return { serviceSnapshot: true };
};

export {};
"#,
    )
    .expect("service-snapshot smoke bundle should write");
    let mut limits = RuntimeLimits::application_node22();
    limits.service_capability_enabled = true;
    limits.grants.service = vec!["release-smoke-service".to_string()];
    let runtime = NimbusRuntime::with_policy(
        Arc::new(NoopHost),
        Arc::new(RuntimePolicy::new(limits)),
        nimbus_runtime::RuntimeEgressPosture::CoarsePermissions,
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "release:serviceSnapshot".to_string(),
        args: json!({}),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };
    let owner_id = RuntimeOwnerId::trusted_session(
        RuntimeOwnerClass::Tooling,
        "packaged-service-snapshot-smoke",
        NonZeroU64::new(1).expect("service-snapshot owner incarnation is nonzero"),
        Some("packaged-service-snapshot-smoke"),
    )
    .expect("service-snapshot owner should build");
    let (owner, _) = RuntimeOwnerLeaseIssuer.issue(owner_id);

    let value = runtime
        .invoke_bundle_for_tenant_with_owner(
            &RuntimeBundle::new(&bundle_path),
            &request,
            "packaged-service-snapshot-smoke",
            owner,
        )
        .await
        .expect("the service-bearing NodeFull runtime should invoke from packaged sources");
    assert_eq!(value, json!({ "serviceSnapshot": true }));
}
