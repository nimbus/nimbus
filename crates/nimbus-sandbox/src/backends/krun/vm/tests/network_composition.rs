//! Krun network composition, startup fencing, and teardown contracts.

use std::sync::Arc;

use nimbus_egress::{EGRESS_CA_BUNDLE_ENV, EGRESS_NODE_EXTRA_CA_CERTS_ENV, EGRESS_PROXY_URL_ENV};
use nimbus_network::NetworkSegmentAllocator;

use super::{env_from_config, support::*};
use crate::backends::oci::network::{
    OciSegmentAllocator, RecordingSegmentAllocator, SegmentAllocatorOperation,
    allocate_container_ips, default_network_attachment_id,
};

#[test]
fn launch_network_config_denies_direct_bridge_egress() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));

    let tenant = nimbus_core::TenantId::new("deny-tenant").expect("tenant should parse");
    let network_config = backend
        .network_config(&tenant)
        .expect("network config should resolve");
    assert_eq!(
        network_config.direct_egress,
        crate::backends::oci::network::OciNetworkDirectEgress::Deny,
        "krun VMMs must run inside a deny-by-default bridge with no ambient egress route"
    );
    assert_eq!(
        network_config.provider_kind_label(),
        "krun",
        "the real krun composition path must persist krun-owned provider evidence"
    );
}

#[test]
fn krun_backend_consumes_the_injected_portable_segment_allocator() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant = TenantId::new("injected-krun").expect("tenant should parse");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        tenant.clone(),
        "10.74.0.0/24",
        74,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf()),
        injected,
    );

    let network = backend
        .network_config(&tenant)
        .expect("injected allocator should resolve the network");

    assert_eq!(network.network_subnet, "10.74.0.0/24");
    assert_eq!(network.network_name, "nimbus-t-74");
    assert_eq!(network.network_interface, "nb-74");
    assert_eq!(
        recorder.operations(),
        [SegmentAllocatorOperation::SegmentFor(tenant)],
        "the krun backend must use only the injected capability; evidence-aware startup inspection and resolution must not reconstruct or downcast a concrete allocator"
    );
}

#[test]
fn startup_network_reconciliation_failure_blocks_new_krun_planning() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    let corrupt_owner = SandboxId::new("corrupt-krun-startup-owner");
    let spec = sample_spec();
    let corrupt_manifest_path = crate::artifact_paths::manifest_path(
        &config.workload_state_root,
        &spec.tenant_id,
        &corrupt_owner,
    );
    fs::create_dir_all(
        corrupt_manifest_path
            .parent()
            .expect("corrupt manifest parent"),
    )
    .expect("corrupt manifest parent should create");
    fs::write(&corrupt_manifest_path, b"{").expect("corrupt manifest should be installed");
    let backend = KrunSandboxBackend::new(config);

    let error = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("new-krun-work-must-remain-fenced"),
            None,
            None,
        )
        .expect_err("new work must fail closed after startup reconciliation fails");
    assert!(
        error
            .to_string()
            .contains("refuses new network work because startup reconciliation did not complete")
            && error.to_string().contains("unmatched artifact")
            && error
                .to_string()
                .contains(&corrupt_manifest_path.display().to_string()),
        "admission must preserve the exact observable startup failure: {error}"
    );
    assert_eq!(
        crate::artifact_paths::all_manifest_paths(&backend.config.workload_state_root)
            .expect("manifest paths should inspect"),
        [corrupt_manifest_path],
        "rejected planning must not create a second launch authority"
    );
}

#[test]
fn plan_only_stop_does_not_invent_attachment_cleanup_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-stop-order", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.75.0.0/24",
        75,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ),
        injected,
    );

    let handle =
        block_on(backend.start(spec.clone())).expect("plan-only krun start should succeed");
    let before_stop = recorder.operations();
    block_on(backend.stop(&handle.id)).expect("plan-only krun stop should clean local artifacts");

    assert_eq!(
        recorder.operations(),
        before_stop,
        "an authority-free plan-only preview must not fabricate an attachment hold in order to clean it"
    );
}

#[test]
fn restart_network_teardown_retains_exact_segment_hold() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-restart-hold", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.76.0.0/24",
        76,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ),
        injected,
    );
    let handle =
        block_on(backend.start(spec)).expect("plan-only krun start should render the manifest");
    let mut manifest = backend
        .read_manifest(&handle.id)
        .expect("manifest read should succeed")
        .expect("planned manifest should exist");
    let network_config = backend
        .network_config(&manifest.spec.tenant_id)
        .expect("execute-shaped network config should resolve");
    let reservation_claim = network_config.reservation_claim.clone();
    let segment_id = network_config
        .segment_id
        .parse()
        .expect("execute-shaped segment identity should parse");
    manifest.network_config = Some(network_config);
    allocate_container_ips(
        &backend.ipam_authority,
        &manifest.network_layout,
        manifest
            .network_config
            .as_ref()
            .expect("execute-shaped config should remain"),
        &manifest.handle.id,
    )
    .expect("execute-shaped fixture should persist its generation-fenced IPAM");
    recorder
        .reserve_attachment_for_coordinator(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &reservation_claim,
        )
        .expect("execute-shaped fixture should reserve its exact attachment");
    recorder
        .bind_reserved_attachment_to_segment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &segment_id,
            &reservation_claim,
        )
        .expect("execute-shaped fixture should bind its exact segment");
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &reservation_claim,
        )
        .expect("execute-shaped fixture should adopt its exact segment association");
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    let before_restart = recorder.operations().len();

    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Restart,
        )
        .expect("restart teardown should remove only provider artifacts");

    assert_eq!(
        &recorder.operations()[before_restart..],
        [SegmentAllocatorOperation::InspectAttachment(
            manifest.spec.tenant_id.clone(),
            default_network_attachment_id(&manifest.handle.id),
        )],
        "restart teardown may authenticate but must not mutate the exact segment hold while the \
         persisted network config will be reused"
    );
    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect("final teardown should quarantine and release the retained hold");
    assert_eq!(
        &recorder.operations()[before_restart..],
        [
            SegmentAllocatorOperation::InspectAttachment(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::InspectAttachment(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::Quarantine(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::Quarantine(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::Release(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::FinalizeRelease(
                manifest.spec.tenant_id.clone(),
                vec!["netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned()],
            ),
        ],
        "final teardown must release the exact hold that restart preserved"
    );
}

#[test]
fn execute_egress_proxy_binds_bridge_gateway_after_published_ports() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.published_port_range = 15000..=15002;
    let backend = KrunSandboxBackend::new(config);

    let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("extra", 15000, 8080));
    let sandbox_id = SandboxId::new("egress-port-order");
    let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("published binding launch claim should mint");
    backend
        .port_lease_coordinator()
        .reserve_launch_ports_for_sandbox(
            crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan::new(
                &spec.tenant_id,
                &sandbox_id,
                &spec.port_bindings,
                &[],
            ),
            &reservation_claim,
        )
        .expect("published binding should reserve before egress");
    let network_config = backend
        .network_config(&spec.tenant_id)
        .expect("primary network config resolves");
    let proxy = backend
        .allocate_egress_proxy(&network_config, &sandbox_id, &spec)
        .expect("execute launches should assign a bridge-reachable egress proxy");

    assert_eq!(
        proxy
            .bind_addr()
            .expect("egress proxy bind address should resolve"),
        "10.0.0.1:15000"
            .parse()
            .expect("bridge gateway socket address should parse"),
        "loopback publication and bridge-gateway PEP may share one numeric port"
    );
}

#[test]
fn execute_plan_injects_proxy_env_and_workload_scoped_trust_anchor() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.published_port_range = 15000..=15002;
    let backend = KrunSandboxBackend::new(config);
    let sandbox_id = SandboxId::new("db-01");

    let plan = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("execute krun plan should lower");

    let egress_proxy = plan
        .manifest
        .egress_proxy
        .as_ref()
        .expect("execute krun plan should assign an egress proxy");
    assert_eq!(egress_proxy.host, "10.0.0.1");
    assert_eq!(egress_proxy.port, 15000);
    let config: serde_json::Value = serde_json::from_slice(
        &fs::read(&plan.manifest.bundle_layout.config_path).expect("bundle config should read"),
    )
    .expect("bundle config should parse");
    let env = env_from_config(&config);
    for expected in [
        format!("{EGRESS_PROXY_URL_ENV}=http://10.0.0.1:15000"),
        "HTTP_PROXY=http://10.0.0.1:15000".to_owned(),
        "HTTPS_PROXY=http://10.0.0.1:15000".to_owned(),
        "NO_PROXY=".to_owned(),
        format!("{EGRESS_CA_BUNDLE_ENV}=/run/nimbus/egress/ca.pem"),
        format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/run/nimbus/egress/ca.pem"),
    ] {
        assert!(
            env.contains(&expected.as_str()),
            "execute krun bundle should carry proxy and trust env {expected:?}: {env:?}"
        );
    }

    let trust_anchor_path = temp_dir
        .path()
        .join("state")
        .join("egress-trust-anchors")
        .join("tenant")
        .join("db-01.pem");
    assert!(
        trust_anchor_path.is_file(),
        "execute krun planning must materialize the deterministic trust-anchor mount source"
    );
    assert!(
        fs::read_to_string(&trust_anchor_path)
            .expect("trust-anchor placeholder should read")
            .contains("placeholder"),
        "the planner writes a placeholder that the live PEP overwrites before launch"
    );
    let mounts = config["mounts"]
        .as_array()
        .expect("mounts should be an array");
    let trust_mount = mounts
        .iter()
        .find(|mount| mount["destination"] == "/run/nimbus/egress/ca.pem")
        .expect("execute krun bundle should mount the trust anchor");
    assert_eq!(trust_mount["type"], "bind");
    assert_eq!(
        trust_mount["source"].as_str(),
        Some(trust_anchor_path.to_string_lossy().as_ref())
    );
}

#[test]
fn plan_scopes_network_namespace_path_by_tenant() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));

    let tenant_a_plan = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-a", "db"),
            &SandboxId::new("db-a"),
            None,
            None,
        )
        .expect("tenant-a plan should lower");
    let tenant_b_plan = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-b", "db"),
            &SandboxId::new("db-b"),
            None,
            None,
        )
        .expect("tenant-b plan should lower");

    assert!(
        tenant_a_plan
            .manifest
            .network_layout
            .netns_path
            .starts_with(
                temp_dir
                    .path()
                    .join("state")
                    .join("tenants")
                    .join("tenant-a")
            ),
        "the sandbox netns must be rooted under the owning tenant"
    );
    assert_ne!(
        tenant_a_plan.manifest.network_layout.netns_path,
        tenant_b_plan.manifest.network_layout.netns_path,
        "the same service name in different tenants must not share a network namespace"
    );
    assert!(
        tenant_a_plan.manifest.egress_proxy.is_none(),
        "plan-only launches must not claim a live egress proxy"
    );
}

#[test]
fn plan_writes_bundle_joining_the_planned_network_namespace() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));

    let plan = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new("db-01"), None, None)
        .expect("plan-only launch should lower");

    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&plan.manifest.bundle_layout.config_path)
            .expect("bundle config should be readable"),
    )
    .expect("bundle config should parse");
    let env = env_from_config(&config);
    assert!(
        env.iter().all(|entry| {
            !entry.starts_with("HTTP_PROXY=")
                && !entry.starts_with(&format!("{EGRESS_CA_BUNDLE_ENV}="))
                && !entry.starts_with(&format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}="))
        }),
        "plan-only krun bundles must not inject live proxy or trust env: {env:?}"
    );
    let namespaces = config["linux"]["namespaces"]
        .as_array()
        .expect("linux.namespaces should be an array");
    let network_namespace = namespaces
        .iter()
        .find(|namespace| namespace["type"] == "network")
        .expect("krun bundle must carry the deny-by-default network namespace");

    assert_eq!(
        network_namespace["path"],
        plan.manifest
            .network_layout
            .netns_path
            .to_string_lossy()
            .as_ref(),
        "the bundle network namespace entry must point at the planned tenant-scoped netns"
    );
}

/// KME5 hardening: the krun microVM network must be resolver-free. The
/// deny-by-default guest resolves names through the host PEP (`HTTP_PROXY`),
/// so the backend's `network_config` turns `enable_dns` off, which stops
/// netavark from binding an in-subnet aardvark-dns stub on the bridge gateway
/// `:53` — closing the residual DNS-exfil channel and removing the
/// `10.89.0.1:53` collision between two krun sandboxes.
#[test]
fn krun_network_config_disables_bridge_dns_resolver() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));

    let tenant = nimbus_core::TenantId::new("dns-tenant").expect("tenant should parse");
    assert!(
        !backend
            .network_config(&tenant)
            .expect("network config should resolve")
            .enable_dns,
        "the krun backend must disable the bridge DNS resolver stub (enable_dns=false)"
    );
}
