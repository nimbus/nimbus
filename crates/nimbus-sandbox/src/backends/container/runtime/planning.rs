use super::support::*;

use std::sync::Arc;

use crate::backends::oci::network::{
    OciMachinePortForwarderConfig, OciNetworkDirectEgress, OciSegmentAllocator,
    RecordingSegmentAllocator, SegmentAllocatorOperation,
};
use nimbus_egress::{EGRESS_CA_BUNDLE_ENV, EGRESS_NODE_EXTRA_CA_CERTS_ENV, EGRESS_PROXY_URL_ENV};
use nimbus_network::LocalPortLeaseAuthority;
use tempfile::TempDir;

fn persisted_claim_observer(
    state_root: PathBuf,
    expected_manifest_path: Option<PathBuf>,
    expected_mode: ContainerStartMode,
    injected_message: &'static str,
) -> impl Fn(&nimbus_network::NetworkReservationClaim) -> Result<()> + Send + Sync {
    move |reservation_claim| {
        let manifest_paths = match expected_manifest_path.as_ref() {
            Some(path) => vec![path.clone()],
            None => crate::artifact_paths::all_manifest_paths(&state_root).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to enumerate claim-only manifests under {}: {error}",
                        state_root.display()
                    ),
                }
            })?,
        };
        assert_eq!(
            manifest_paths.len(),
            1,
            "the claim-only crash cut must publish exactly one canonical manifest before reservation"
        );
        let manifest: ContainerSandboxManifest = serde_json::from_slice(
            &std::fs::read(&manifest_paths[0]).expect("claim-only manifest should read"),
        )
        .expect("claim-only manifest should parse");
        assert_eq!(manifest.start_mode, expected_mode);
        assert_eq!(
            manifest.launch_reservation_claim.as_ref(),
            Some(reservation_claim),
            "the first attachment effect must receive the exact already-durable claim"
        );
        assert!(
            manifest.network_config.is_none()
                && manifest.port_leases.is_empty()
                && manifest.egress_proxy.is_none(),
            "the durable pre-effect manifest must not fabricate placement, listener, or provider evidence"
        );
        Err(SandboxError::OperationFailed {
            message: injected_message.to_owned(),
        })
    }
}

#[test]
fn plan_only_backend_persists_a_container_manifest() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let handle = backend
        .start_sync(sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)))
        .expect("container plan should start");

    assert_eq!(handle.backend, SandboxBackendKind::Container);
    let manifest_path = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &sample_spec().tenant_id,
        &handle.id,
    );
    assert!(manifest_path.is_file(), "manifest should be written");
}

#[test]
fn container_launch_network_config_denies_direct_egress_for_supervised_processes() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let tenant = nimbus_core::TenantId::new("egress-tenant").expect("tenant should parse");
    let network_config = backend
        .network_config(&tenant)
        .expect("network config should resolve");

    assert_eq!(
        network_config.direct_egress,
        OciNetworkDirectEgress::Deny,
        "process-capable container launches must not keep ambient bridge egress"
    );
}

#[test]
fn container_backend_consumes_the_injected_portable_segment_allocator() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let tenant = nimbus_core::TenantId::new("injected-container").expect("tenant should parse");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        tenant.clone(),
        "10.73.0.0/24",
        73,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );

    let network = backend
        .network_config(&tenant)
        .expect("injected allocator should resolve the network");

    assert_eq!(network.network_subnet, "10.73.0.0/24");
    assert_eq!(network.network_name, "nimbus-t-73");
    assert_eq!(network.network_interface, "nb-73");
    assert_eq!(
        recorder.operations(),
        [
            SegmentAllocatorOperation::Reconcile(Default::default()),
            SegmentAllocatorOperation::SegmentFor(tenant),
        ],
        "the container backend must use only the injected capability; startup reconciliation and resolution must not reconstruct or downcast a concrete allocator"
    );
}

#[test]
fn startup_network_reconciliation_failure_blocks_new_container_planning() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let corrupt_owner = SandboxId::new("corrupt-startup-owner");
    let spec = sample_spec();
    let corrupt_manifest_path = crate::artifact_paths::manifest_path(
        &config.workload_state_root,
        &spec.tenant_id,
        &corrupt_owner,
    );
    std::fs::create_dir_all(
        corrupt_manifest_path
            .parent()
            .expect("corrupt manifest parent"),
    )
    .expect("corrupt manifest parent should create");
    std::fs::write(&corrupt_manifest_path, b"{").expect("corrupt manifest should be installed");
    let backend = ContainerSandboxBackend::new(config);

    let error = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("new-work-must-remain-fenced"),
            None,
            None,
        )
        .expect_err("new work must fail closed after startup reconciliation fails");
    assert!(
        error
            .to_string()
            .contains("refuses new durable work because startup reconciliation did not complete")
            && error.to_string().contains("failed to parse manifest"),
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
fn direct_launch_persists_exact_claim_before_first_attachment_reservation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend_root = temp_dir.path().join("direct-owner");
    let state_root = backend_root.join("state");
    let tenant = sample_spec().tenant_id;
    let id = SandboxId::new("direct-manifest-first");
    let expected_manifest_path = crate::artifact_paths::manifest_path(&state_root, &tenant, &id);
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(tenant, "10.74.0.0/24", 74)
            .with_reserve_attachment_observer(persisted_claim_observer(
                state_root.clone(),
                Some(expected_manifest_path),
                ContainerStartMode::Execute,
                "injected direct first-reservation failure",
            )),
    );
    let injected: Arc<OciSegmentAllocator> = recorder;
    let backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(&backend_root),
        injected,
    );

    let error = backend
        .plan_start_with_id(&sample_spec(), &id, None, None)
        .expect_err("the observer should stop the direct launch at its first reservation");
    assert!(
        error
            .to_string()
            .contains("injected direct first-reservation failure"),
        "the first-effect failure must remain primary: {error}"
    );
    let persisted = backend
        .read_manifest(&id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert!(persisted.shutdown_requested);
    assert!(persisted.launch_reservation_claim.is_none());
}

#[test]
fn runner_launch_persists_exact_claim_before_first_attachment_reservation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let state_root = temp_dir.path().join("state");
    let tenant = sample_spec().tenant_id;
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(tenant, "10.75.0.0/24", 75)
            .with_reserve_attachment_observer(persisted_claim_observer(
                state_root.clone(),
                None,
                ContainerStartMode::PlanOnly,
                "injected runner first-reservation failure",
            )),
    );
    let injected: Arc<OciSegmentAllocator> = recorder;
    let backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(temp_dir.path().join("bundles"), &state_root),
        injected,
    );

    let error = backend
        .prepare_plan_only_service_workload(sample_spec())
        .expect_err("the observer should stop the runner launch at its first reservation");
    assert!(
        error
            .to_string()
            .contains("injected runner first-reservation failure"),
        "the first-effect failure must remain primary: {error}"
    );
    let manifest_paths = crate::artifact_paths::all_manifest_paths(&state_root)
        .expect("terminal manifest paths should enumerate");
    assert_eq!(manifest_paths.len(), 1);
    let persisted: ContainerSandboxManifest = serde_json::from_slice(
        &std::fs::read(&manifest_paths[0]).expect("terminal manifest should read"),
    )
    .expect("terminal manifest should parse");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert!(persisted.shutdown_requested);
    assert!(persisted.launch_reservation_claim.is_none());
}

#[test]
fn execute_plan_assigns_bridge_reachable_egress_proxy_and_injects_proxy_env() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);

    let plan = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("execute plan should lower");

    let egress_proxy = plan
        .manifest
        .egress_proxy
        .as_ref()
        .expect("execute launch should assign an egress proxy");
    // First tenant on the node super-net gets 10.0.0.0/24, gateway 10.0.0.1.
    assert_eq!(egress_proxy.host, "10.0.0.1");
    assert_eq!(egress_proxy.port, 15000);
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    assert!(
        env.contains(&"HTTP_PROXY=http://10.0.0.1:15000")
            && env.contains(&"http_proxy=http://10.0.0.1:15000")
            && env.contains(&"NO_PROXY=")
            && env.contains(&"no_proxy="),
        "execute bundle should steer proxy-aware tools through the egress proxy: {env:?}"
    );
    assert!(
        env.contains(&format!("{EGRESS_PROXY_URL_ENV}=http://10.0.0.1:15000").as_str()),
        "execute bundle should expose Nimbus egress proxy metadata: {env:?}"
    );
    for expected in [
        format!("{EGRESS_CA_BUNDLE_ENV}=/run/nimbus/egress/ca.pem"),
        format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/run/nimbus/egress/ca.pem"),
    ] {
        assert!(
            env.contains(&expected.as_str()),
            "execute bundle should expose the workload-scoped trust anchor: {env:?}"
        );
    }
    let trust_anchor_path = temp_dir
        .path()
        .join("state")
        .join("egress-trust-anchors")
        .join("svc-demo")
        .join("db-01.pem");
    assert!(
        trust_anchor_path.is_file(),
        "execute planning must materialize the deterministic trust-anchor mount source"
    );
    assert!(
        std::fs::read_to_string(&trust_anchor_path)
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
        .expect("execute bundle should mount the trust anchor");
    assert_eq!(trust_mount["type"], "bind");
    assert_eq!(
        trust_mount["source"].as_str(),
        Some(trust_anchor_path.to_string_lossy().as_ref())
    );
}

#[test]
fn execute_plan_allocates_egress_proxy_port_after_existing_guest_bindings() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);

    let plan = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 15000, 8080)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("execute plan should lower");

    assert_eq!(
        plan.manifest.egress_proxy.as_ref().map(|proxy| proxy.port),
        Some(15000),
        "loopback publication and bridge-gateway PEP are proven-disjoint bind targets"
    );
}

#[test]
fn plan_only_launches_do_not_materialize_live_proxy_env() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let plan = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan-only launch should lower");

    assert!(
        plan.manifest.egress_proxy.is_none(),
        "plan-only launches should not claim a live egress proxy"
    );
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    assert!(
        env.iter().all(|entry| !entry.starts_with("HTTP_PROXY=")
            && !entry.starts_with("http_proxy=")
            && !entry.starts_with(&format!("{EGRESS_CA_BUNDLE_ENV}="))
            && !entry.starts_with(&format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}="))),
        "plan-only bundles should keep live proxy env absent: {env:?}"
    );
}

#[test]
fn substituted_execution_context_preserves_split_runner_roots_and_rejects_redirection() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let workload_state_root = temp_dir.path().join("project-state");
    let network_state_root = temp_dir.path().join("node-network-state");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        &workload_state_root,
    )
    .with_network_state_root(&network_state_root);
    config.published_port_range = 15000..=15002;
    config.buildah_path = "/opt/nimbus/bin/buildah-cleanup".into();
    config.use_buildah_unshare = true;
    config.netavark_path = "/usr/libexec/podman/netavark".into();
    config.aardvark_dns_path = "/usr/libexec/podman/aardvark-dns".into();
    config.machine_port_forwarder = Some(
        OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
            "planning-test-gvproxy",
            nimbus_network::NetworkResourceGeneration::new(1),
        )
        .expect("planning fixture gvproxy identity should validate"),
    );
    let backend = ContainerSandboxBackend::new(config);

    let prepared = backend
        .prepare_plan_only_service_workload(sample_spec())
        .expect("service workload should prepare");

    let manifest_path = crate::artifact_paths::manifest_path(
        &workload_state_root,
        &sample_spec().tenant_id,
        &prepared.handle.id,
    );
    let pointer_path = prepared.bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    assert_eq!(
        std::fs::read_to_string(&pointer_path)
            .expect("runner manifest pointer should be readable")
            .trim(),
        manifest_path.to_string_lossy(),
        "runner should receive an exact manifest pointer scoped to the prepared bundle"
    );

    let manifest_bytes = std::fs::read(&manifest_path).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest should parse");
    assert_eq!(manifest["start_mode"], "plan_only");
    assert_eq!(manifest["egress_proxy"]["host"], "10.0.0.1");
    assert_eq!(manifest["egress_proxy"]["port"], 15000);
    assert_eq!(
        manifest["runner_config"]["buildah_path"],
        "/opt/nimbus/bin/buildah-cleanup"
    );
    assert_eq!(manifest["runner_config"]["use_buildah_unshare"], true);
    assert_eq!(
        manifest["runner_config"]["netavark_path"],
        "/usr/libexec/podman/netavark"
    );
    assert_eq!(
        manifest["runner_config"]["aardvark_dns_path"],
        "/usr/libexec/podman/aardvark-dns"
    );
    assert_eq!(
        manifest["runner_config"]["machine_port_forwarder"]["path_prefix"],
        "/services/forwarder"
    );
    let typed_manifest: ContainerSandboxManifest =
        serde_json::from_slice(&manifest_bytes).expect("typed manifest should parse");
    let reservation_claim = typed_manifest
        .launch_reservation_claim
        .as_ref()
        .expect("runner handoff must persist its exact compensation capability");
    let mut launch_batch = typed_manifest.port_leases.clone();
    launch_batch.push(
        typed_manifest
            .egress_proxy
            .as_ref()
            .expect("runner handoff must reserve its PEP")
            .port_lease
            .clone(),
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&network_state_root)
        .expect("runner authority should reopen");
    for request in &launch_batch {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("runner lease should inspect")
                .expect("runner lease should remain durable")
                .reservation_claim(),
            Some(reservation_claim),
            "every serialized launch request must carry the same durable coordinator claim"
        );
    }
    let mut missing_provenance: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest JSON should parse");
    missing_provenance
        .as_object_mut()
        .expect("manifest should be an object")
        .remove("requested_port_bindings");
    let error = serde_json::from_value::<ContainerSandboxManifest>(missing_provenance)
        .expect_err("runner manifests without canonical binding inputs must fail closed");
    assert!(
        error.to_string().contains("requested_port_bindings"),
        "the missing required field must be explicit: {error}"
    );
    let mut missing_claim: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest JSON should parse");
    missing_claim
        .as_object_mut()
        .expect("manifest should be an object")
        .remove("launch_reservation_claim");
    let error = serde_json::from_value::<ContainerSandboxManifest>(missing_claim)
        .expect_err("runner manifests without coordinator authority must fail closed");
    assert!(
        error.to_string().contains("launch_reservation_claim"),
        "the missing required field must be explicit: {error}"
    );
    let runner_config = typed_manifest.runner_config.to_backend_config();
    assert_eq!(
        runner_config.workload_state_root, workload_state_root,
        "the runner must reopen the exact workload root that prepared its artifacts"
    );
    assert_eq!(
        runner_config.network_state_root, network_state_root,
        "the runner must reopen the exact node authority that prepared its leases"
    );
    assert_eq!(
        typed_manifest.network_layout.workload_state_root,
        runner_config.workload_state_root
    );
    assert_eq!(
        typed_manifest.network_layout.network_state_root,
        runner_config.network_state_root
    );
    assert!(
        !nimbus_network::LocalNetworkStateStore::authority_path_for(
            &runner_config.workload_state_root
        )
        .exists(),
        "split planning must not create a network authority under workload state"
    );
    assert_eq!(
        runner_config.buildah_path,
        PathBuf::from("/opt/nimbus/bin/buildah-cleanup")
    );
    assert!(
        runner_config.use_buildah_unshare,
        "the runner must preserve the exact Buildah execution context used for mounted-rootfs cleanup"
    );
    assert_eq!(
        runner_config.netavark_path,
        PathBuf::from("/usr/libexec/podman/netavark")
    );
    assert_eq!(
        runner_config.aardvark_dns_path,
        PathBuf::from("/usr/libexec/podman/aardvark-dns")
    );
    assert_eq!(runner_config.published_port_range, 15000..=15002);
    let mut executing_manifest = typed_manifest.clone();
    let runner_backend = ContainerSandboxBackend::new(runner_config.clone());
    super::super::runner::persist_runner_execution_ownership(
        &runner_backend,
        &mut executing_manifest,
    )
    .expect("the runner should durably take provider-backed lifecycle ownership");
    assert_eq!(
        executing_manifest.start_mode,
        ContainerStartMode::Execute,
        "a runner-executed handoff must not remain classified as an effect-free preview"
    );
    assert_eq!(
        executing_manifest.launch_reservation_claim, typed_manifest.launch_reservation_claim,
        "taking execution ownership must preserve exact launch compensation until provider adoption"
    );
    assert_eq!(
        runner_backend
            .read_manifest(&prepared.handle.id)
            .expect("persisted runner ownership should inspect")
            .expect("runner manifest should remain durable")
            .start_mode,
        ContainerStartMode::Execute,
        "provider-backed lifecycle ownership must be durable before the first provider effect"
    );
    backend
        .write_manifest(&typed_manifest)
        .expect("test should restore the plan-only cancellation fixture");
    let mut substituted_authority = typed_manifest.clone();
    substituted_authority.runner_config.workload_state_root = temp_dir.path().join("foreign-state");
    let error = super::super::runner::validate_runner_authority_roots(&substituted_authority)
        .expect_err("a substituted runner workload root must fail before backend effects");
    assert!(
        error.to_string().contains("workload root") && error.to_string().contains("does not match"),
        "the workload-root rejection must be explicit: {error}"
    );
    let mut substituted_authority = typed_manifest.clone();
    substituted_authority.runner_config.network_state_root =
        temp_dir.path().join("foreign-network-state");
    let error = super::super::runner::validate_runner_authority_roots(&substituted_authority)
        .expect_err("a substituted runner network root must fail before backend effects");
    assert!(
        error.to_string().contains("network authority root")
            && error.to_string().contains("does not match"),
        "the network-root rejection must be explicit: {error}"
    );
    assert_eq!(
        runner_config
            .machine_port_forwarder
            .expect("machine forwarder should survive runner reconstruction")
            .path_prefix,
        "/services/forwarder"
    );

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(prepared.bundle_dir.join("config.json")).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    assert!(
        env.contains(&"HTTP_PROXY=http://10.0.0.1:15000")
            && env.contains(&"http_proxy=http://10.0.0.1:15000")
            && env.contains(&format!("{EGRESS_PROXY_URL_ENV}=http://10.0.0.1:15000").as_str())
            && env.contains(&format!("{EGRESS_CA_BUNDLE_ENV}=/run/nimbus/egress/ca.pem").as_str())
            && env.contains(
                &format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/run/nimbus/egress/ca.pem").as_str()
            ),
        "service bundle should route proxy-aware tools through the runner-owned egress proxy: {env:?}"
    );
    let trust_anchor_path = workload_state_root
        .join("egress-trust-anchors")
        .join("svc-demo")
        .join(format!("{}.pem", prepared.handle.id.as_str()));
    assert!(
        trust_anchor_path.is_file(),
        "service workload planning must materialize the runner-owned trust-anchor mount source"
    );

    let cancellation_error = backend
        .mark_plan_only_service_workload_stopped(&prepared.handle.id)
        .expect_err("a durable Execute winner must fence stale plan-only cancellation");
    assert!(
        cancellation_error
            .to_string()
            .contains("already decided as Execute"),
        "the losing cancellation must name the durable handoff winner: {cancellation_error}"
    );
    for request in &launch_batch {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("fenced runner lease should inspect")
                .expect("fenced runner lease should remain durable")
                .phase(),
            nimbus_network::PortLeasePhase::Reserved,
            "losing cancellation must not release runner-owned launch authority"
        );
    }
    assert!(
        !backend
            .segment_allocator
            .inspect_segments(&sample_spec().tenant_id)
            .expect("segment authority should inspect after fenced cancellation")
            .unwrap_or_default()
            .is_empty(),
        "losing cancellation must not remove the runner-owned segment allocation"
    );
    assert!(
        trust_anchor_path.exists(),
        "losing cancellation must not remove the runner-owned trust anchor"
    );
}

#[test]
fn runner_handoff_failures_compensate_network_pointer_and_launch_artifacts() {
    for (failure, expected) in [
        (
            RunnerHandoffFailure::Manifest,
            "injected runner manifest handoff failure",
        ),
        (
            RunnerHandoffFailure::Pointer,
            "injected runner pointer handoff failure",
        ),
    ] {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let state_root = temp_dir.path().join("state");
        let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            &state_root,
        ))
        .with_runner_handoff_failure(failure);

        let error = backend
            .prepare_plan_only_service_workload(sample_spec())
            .expect_err("the injected handoff boundary must fail preparation");
        assert!(
            error.to_string().contains(expected),
            "the primary handoff failure must remain visible: {error}"
        );

        let manifest_paths = crate::artifact_paths::all_manifest_paths(&state_root)
            .expect("compensated manifest paths should enumerate");
        assert_eq!(
            manifest_paths.len(),
            1,
            "compensation must persist exactly one terminal manifest"
        );
        let manifest: ContainerSandboxManifest = serde_json::from_slice(
            &std::fs::read(&manifest_paths[0]).expect("terminal manifest should read"),
        )
        .expect("terminal manifest should parse");
        assert_eq!(manifest.status, SandboxStatus::Stopped);
        assert!(manifest.shutdown_requested);
        assert!(manifest.launch_reservation_claim.is_none());
        assert!(manifest.launch_artifact.is_none());
        assert!(
            !manifest
                .bundle_layout
                .bundle_dir
                .join(RUNNER_MANIFEST_POINTER_FILE)
                .exists(),
            "failed handoff must withdraw its runner pointer"
        );
        assert!(
            !egress_trust_anchor_root(&state_root)
                .join(manifest.spec.tenant_id.as_str())
                .join(format!("{}.pem", manifest.handle.id.as_str()))
                .exists(),
            "failed handoff must remove its unactivated trust anchor"
        );
        let authority = LocalPortLeaseAuthority::open(&state_root)
            .expect("port authority should reopen after compensation");
        let records = authority.list().expect("leases should list");
        assert!(
            !records.is_empty()
                && records
                    .iter()
                    .all(|record| record.phase() == nimbus_network::PortLeasePhase::Released),
            "failed handoff must release every never-bound listener"
        );
        assert!(
            backend
                .segment_allocator
                .inspect_segments(&manifest.spec.tenant_id)
                .expect("segment authority should inspect")
                .unwrap_or_default()
                .is_empty(),
            "failed handoff must release IPAM and its exact segment reservation"
        );
    }
}

#[test]
fn pointer_acknowledgement_failure_cannot_compensate_after_execute_wins() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let state_root = temp_dir.path().join("state");
    let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        &state_root,
    ))
    .with_runner_handoff_failure(RunnerHandoffFailure::PointerAfterExecuteDecision);

    let error = backend
        .prepare_plan_only_service_workload(sample_spec())
        .expect_err("post-publication acknowledgement loss must fail preparation");
    assert!(
        error.to_string().contains("already decided as Execute"),
        "compensation must lose to the durable Execute decision: {error}"
    );

    let manifest_paths = crate::artifact_paths::all_manifest_paths(&state_root)
        .expect("fenced manifest paths should enumerate");
    assert_eq!(manifest_paths.len(), 1);
    let manifest: ContainerSandboxManifest = serde_json::from_slice(
        &std::fs::read(&manifest_paths[0]).expect("fenced manifest should read"),
    )
    .expect("fenced manifest should parse");
    assert_eq!(manifest.start_mode, ContainerStartMode::PlanOnly);
    assert!(!manifest.shutdown_requested);
    assert!(manifest.launch_reservation_claim.is_some());
    assert!(
        manifest
            .bundle_layout
            .bundle_dir
            .join(RUNNER_MANIFEST_POINTER_FILE)
            .is_file(),
        "the Execute winner's published pointer must remain available"
    );
    let authority =
        LocalPortLeaseAuthority::open(&state_root).expect("port authority should reopen");
    let mut requests = manifest.port_leases.clone();
    requests.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("prepared runner should retain its PEP")
            .port_lease
            .clone(),
    );
    assert!(
        requests.iter().all(|request| {
            authority
                .inspect(request.lease_id())
                .expect("fenced lease should inspect")
                .is_some_and(|record| record.phase() == nimbus_network::PortLeasePhase::Reserved)
        }),
        "losing compensation must retain every Execute-owned reservation"
    );
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .is_some_and(|segments| !segments.is_empty()),
        "losing compensation must retain the Execute-owned segment"
    );
}

#[test]
fn plan_only_backend_stop_uses_exact_runner_cancellation_authority() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = sample_spec();
    let prepared = backend
        .prepare_plan_only_service_workload(spec.clone())
        .expect("runner handoff should prepare");
    let manifest = backend
        .read_manifest(&prepared.handle.id)
        .expect("manifest lookup should succeed")
        .expect("prepared manifest should exist");
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("runner handoff should reserve its PEP")
            .port_lease
            .clone(),
    );
    let authority = LocalPortLeaseAuthority::open(temp_dir.path().join("state"))
        .expect("runner authority should reopen");

    backend
        .stop_sync(&prepared.handle.id)
        .expect("generic backend stop must use exact unadopted runner compensation");

    for request in &launch_batch {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("stopped runner lease should inspect")
                .expect("released runner lease should remain durable")
                .phase(),
            nimbus_network::PortLeasePhase::Released
        );
    }
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "generic plan-only stop must not route a Reserved attachment through adopted teardown"
    );
    assert!(
        !egress_trust_anchor_root(&temp_dir.path().join("state"))
            .join(spec.tenant_id.as_str())
            .join(format!("{}.pem", prepared.handle.id.as_str()))
            .exists(),
        "generic plan-only stop must remove the never-activated trust anchor"
    );
}

#[test]
fn plan_only_backend_scopes_container_artifacts_by_tenant_for_same_service_name() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let tenant_a = sample_spec_for_tenant("tenant-a", "api");
    let tenant_b = sample_spec_for_tenant("tenant-b", "api");

    let handle_a = backend
        .start_sync(tenant_a.clone())
        .expect("tenant-a container plan should start");
    let handle_b = backend
        .start_sync(tenant_b.clone())
        .expect("tenant-b container plan should start");

    let manifest_a = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &tenant_a.tenant_id,
        &handle_a.id,
    );
    let manifest_b = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &tenant_b.tenant_id,
        &handle_b.id,
    );
    let bundle_a = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_a.tenant_id,
        &handle_a.id,
    )
    .join("config.json");
    let bundle_b = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_b.tenant_id,
        &handle_b.id,
    )
    .join("config.json");

    assert_ne!(
        manifest_a, manifest_b,
        "tenant-scoped container manifests must not collide for the same service name"
    );
    assert_ne!(
        bundle_a, bundle_b,
        "tenant-scoped container bundles must not collide for the same service name"
    );
    assert!(manifest_a.is_file(), "tenant-a manifest should be written");
    assert!(manifest_b.is_file(), "tenant-b manifest should be written");
    assert!(bundle_a.is_file(), "tenant-a bundle should be written");
    assert!(bundle_b.is_file(), "tenant-b bundle should be written");

    futures::executor::block_on(crate::backend::SandboxBackend::remove_tenant_artifacts(
        &backend,
        tenant_a.tenant_id.clone(),
    ))
    .expect("tenant-a artifacts should be removed");

    assert!(
        !manifest_a.exists(),
        "tenant-a manifest should be removed by tenant cleanup"
    );
    assert!(
        !bundle_a.exists(),
        "tenant-a bundle should be removed by tenant cleanup"
    );
    assert!(manifest_b.exists(), "tenant-b manifest should remain");
    assert!(bundle_b.exists(), "tenant-b bundle should remain");
}

#[test]
fn plan_only_backend_lowers_tenant_volume_mounts_under_tenant_state_root() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let tenant_a = sample_spec_for_tenant("tenant-a", "api")
        .with_mount(SandboxMountSpec::tenant_volume("shared", "/var/lib/app").read_only(false));
    let tenant_b = sample_spec_for_tenant("tenant-b", "api")
        .with_mount(SandboxMountSpec::tenant_volume("shared", "/var/lib/app").read_only(true));

    let handle_a = backend
        .start_sync(tenant_a.clone())
        .expect("tenant-a container plan should start");
    let handle_b = backend
        .start_sync(tenant_b.clone())
        .expect("tenant-b container plan should start");

    let volume_a = temp_dir
        .path()
        .join("state")
        .join("tenants")
        .join("tenant-a")
        .join("volumes")
        .join("shared");
    let volume_b = temp_dir
        .path()
        .join("state")
        .join("tenants")
        .join("tenant-b")
        .join("volumes")
        .join("shared");
    assert!(volume_a.is_dir(), "tenant-a volume directory should exist");
    assert!(volume_b.is_dir(), "tenant-b volume directory should exist");
    assert_ne!(
        volume_a, volume_b,
        "same named volume in different tenants must not share host storage"
    );

    let bundle_a = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_a.tenant_id,
        &handle_a.id,
    )
    .join("config.json");
    let bundle_b = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_b.tenant_id,
        &handle_b.id,
    )
    .join("config.json");
    let rendered_a =
        std::fs::read_to_string(&bundle_a).expect("tenant-a bundle should be readable");
    let rendered_b =
        std::fs::read_to_string(&bundle_b).expect("tenant-b bundle should be readable");
    assert!(
        rendered_a.contains(&volume_a.to_string_lossy().to_string()),
        "tenant-a bundle should bind only the tenant-a volume path: {rendered_a}"
    );
    assert!(
        rendered_b.contains(&volume_b.to_string_lossy().to_string()),
        "tenant-b bundle should bind only the tenant-b volume path: {rendered_b}"
    );

    futures::executor::block_on(crate::backend::SandboxBackend::remove_tenant_artifacts(
        &backend,
        tenant_a.tenant_id.clone(),
    ))
    .expect("tenant-a artifacts should be removed");

    assert!(
        !volume_a.exists(),
        "tenant-a volume should be removed by tenant cleanup"
    );
    assert!(
        volume_b.exists(),
        "tenant-b volume should remain after tenant-a cleanup"
    );
}

#[test]
fn plan_only_backend_scopes_network_state_by_tenant_for_same_sandbox_id() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let sandbox_id = SandboxId::new("api-01");

    let tenant_a_plan = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-a", "api"),
            &sandbox_id,
            None,
            None,
        )
        .expect("tenant-a plan should lower");
    let tenant_b_plan = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-b", "api"),
            &sandbox_id,
            None,
            None,
        )
        .expect("tenant-b plan should lower");

    assert_ne!(
        tenant_a_plan.manifest.network_layout.netns_path,
        tenant_b_plan.manifest.network_layout.netns_path,
        "same sandbox id in different tenants must not share network namespaces"
    );
    assert_eq!(
        tenant_a_plan.manifest.network_layout.network_state_root,
        temp_dir.path().join("state")
    );
    assert_eq!(
        tenant_b_plan.manifest.network_layout.network_state_root,
        temp_dir.path().join("state"),
        "all network resources on one node share one authority root"
    );
    assert_ne!(
        tenant_a_plan.manifest.network_layout.tenant_id,
        tenant_b_plan.manifest.network_layout.tenant_id,
        "tenant IPAM payloads remain distinct typed partitions"
    );
}

#[test]
fn plan_only_backend_auto_assigns_exposed_ports_from_published_range() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15001;
    let backend = ContainerSandboxBackend::new(config);

    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &sandbox_id(),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect("plan should lower image-exposed ports");

    assert_eq!(plan.manifest.spec.port_bindings.len(), 1);
    let binding = &plan.manifest.spec.port_bindings[0];
    assert_eq!(binding.name, "tcp-8080");
    assert_eq!(binding.host_port, 15000);
    assert_eq!(binding.guest_port, 8080);
}

#[test]
fn plan_only_range_exhaustion_creates_no_durable_segment_allocation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15000;
    let backend = ContainerSandboxBackend::new(config);
    let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("explicit", 15000, 5432));

    let error = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("plan-only-range-exhaustion"),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect_err("the image-derived port must not fit in the exhausted preview range");
    assert!(
        error.to_string().contains("published port range")
            && error.to_string().contains("exhausted"),
        "the original preview failure must remain primary: {error}"
    );

    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "authority-free plan preview must fail before creating a segment allocation: {segments:?}"
    );
}

#[test]
fn runner_handoff_rereserves_previewed_automatic_ports_as_ranges() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-auto-reselection"),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect("plan-only image port should preview");
    assert_eq!(plan.manifest.spec.port_bindings[0].host_port, 15000);
    assert_eq!(
        plan.manifest.requested_port_bindings,
        Vec::<SandboxPortBinding>::new(),
        "the canonical operator input remains distinct from the image-derived preview"
    );

    let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("competing launch claim should mint");
    backend
        .port_lease_coordinator()
        .reserve_launch_ports_for_sandbox(
            crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan::new(
                &sample_spec().tenant_id,
                &SandboxId::new("other-sandbox"),
                &[SandboxPortBinding::tcp("occupied", 15000, 9090)
                    .with_host_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))],
                &[],
            ),
            &reservation_claim,
        )
        .expect("another sandbox should claim the previewed number");

    let reservations = backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect("runner handoff should select live authority, not pin the preview");

    assert_eq!(plan.manifest.spec.port_bindings[0].host_port, 15001);
    assert!(matches!(
        reservations.published_leases[0].binding().port(),
        nimbus_network::PortRequestMode::Range(_)
    ));
    assert_eq!(
        plan.manifest.handle.published_endpoints[0].address.port(),
        15001,
        "the visible handle must follow the selected durable port"
    );
    assert_eq!(
        plan.manifest
            .egress_proxy
            .as_ref()
            .expect("runner should own its PEP")
            .port,
        15001,
        "disjoint loopback and bridge-gateway targets may share one numeric port"
    );
}

#[test]
fn execute_plan_hides_preview_and_projects_only_selected_durable_endpoint_when_ready() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::Execute;
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);
    let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("competing launch claim should mint");
    backend
        .port_lease_coordinator()
        .reserve_launch_ports_for_sandbox(
            crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan::new(
                &sample_spec().tenant_id,
                &SandboxId::new("execute-preview-competitor"),
                &[SandboxPortBinding::tcp("occupied", 15000, 9090)
                    .with_host_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))],
                &[],
            ),
            &reservation_claim,
        )
        .expect("another sandbox should claim the previewed number");

    let sandbox_id = SandboxId::new("execute-auto-reselection");
    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &sandbox_id,
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect("execute planning should select live port authority");

    assert_eq!(
        plan.manifest.spec.port_bindings[0].host_port, 15001,
        "the selected binding must follow durable authority rather than the occupied preview"
    );
    assert!(
        plan.manifest.handle.published_endpoints.is_empty(),
        "execute-mode Starting state must not publish even the selected endpoint before readiness"
    );
    let persisted = backend
        .read_manifest(&sandbox_id)
        .expect("execute manifest should inspect")
        .expect("execute manifest should be durable");
    assert_eq!(persisted.spec.port_bindings[0].host_port, 15001);
    assert!(
        persisted.handle.published_endpoints.is_empty(),
        "the durable Starting projection must remain withdrawn"
    );

    let mut ready = persisted;
    synchronize_handle_status(&mut ready, SandboxStatus::Ready);
    assert_eq!(
        ready
            .handle
            .published_endpoints
            .iter()
            .map(|endpoint| (endpoint.address.port(), endpoint.guest_port))
            .collect::<Vec<_>>(),
        [(15001, Some(8080))],
        "readiness must project only the selected durable binding, never the stale preview"
    );
}

#[test]
fn runner_handoff_rejects_truncated_automatic_port_provenance() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-truncated-provenance"),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect("plan-only image port should preview");
    plan.manifest.spec.port_bindings.clear();

    let error = backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect_err("truncated automatic-port provenance must fail closed");

    assert!(
        error.to_string().contains("port binding provenance"),
        "the handoff error must identify the corrupted authority boundary: {error}"
    );
}

#[test]
fn runner_handoff_rejects_explicit_port_forged_as_automatic() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("api", 15432, 5432)),
            &SandboxId::new("runner-forged-provenance"),
            None,
            None,
        )
        .expect("plan-only explicit port should preview");
    plan.manifest.requested_port_bindings.clear();

    let error = backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect_err("an explicit operator port forged as automatic must fail closed");

    assert!(
        error.to_string().contains("port binding provenance"),
        "the handoff error must identify the forged authority boundary: {error}"
    );
}

#[test]
fn later_container_planning_failure_compensates_never_bound_port_batch() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let blocked_bundle_root = temp_dir.path().join("bundle-root-is-a-file");
    std::fs::write(&blocked_bundle_root, b"not a directory").expect("obstacle should write");
    let state_root = temp_dir.path().join("state");
    let mut config = ContainerSandboxBackendConfig::under_root(&state_root);
    config.bundle_root = blocked_bundle_root;
    config.start_mode = ContainerStartMode::Execute;
    config.published_port_range = 15000..=15001;
    let authority_root = config.network_state_root.clone();
    let backend = ContainerSandboxBackend::new(config);
    let spec = sample_spec();

    backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("planning-compensation"),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect_err("bundle materialization should fail after the atomic reservation");

    let leases = nimbus_network::LocalPortLeaseAuthority::open(&authority_root)
        .expect("authority should reopen")
        .list()
        .expect("authority should list");
    assert_eq!(
        leases.len(),
        2,
        "published and PEP requests should be recorded"
    );
    assert!(
        leases
            .iter()
            .all(|lease| lease.phase() == nimbus_network::PortLeasePhase::Released),
        "known no-effect planning failure must leave no port fence: {leases:?}"
    );
    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "later planning compensation must finalize the unrealized segment hold: {segments:?}"
    );
}

#[test]
fn port_quota_failure_after_placement_releases_the_segment_hold() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::Execute;
    config.max_published_ports_per_tenant = Some(0);
    let backend = ContainerSandboxBackend::new(config);
    let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

    let error = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("post-placement-port-quota"),
            None,
            None,
        )
        .expect_err("port admission must fail after execute-mode placement");
    assert!(
        error.to_string().contains("port quota"),
        "the original port-admission failure must remain primary: {error}"
    );

    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "failed port admission must finalize the unrealized segment hold: {segments:?}"
    );
}

#[test]
fn plan_only_backend_does_not_charge_manifest_only_port_previews() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15005;
    config.max_published_ports_per_tenant = Some(1);
    let state_root = config.network_state_root.clone();
    let backend = ContainerSandboxBackend::new(config);

    backend
        .start_sync(sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 15432, 5432)))
        .expect("first same-tenant plan should render");

    let second = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("api-01"),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect("a manifest-only preview must not consume durable tenant quota");
    assert!(
        second.manifest.port_leases.is_empty(),
        "plan-only rendering must not claim host-global port authority"
    );
    let durable = LocalPortLeaseAuthority::open(&state_root)
        .expect("durable port authority should open")
        .list()
        .expect("durable port authority should list");
    assert!(
        durable.is_empty(),
        "manifest-only previews must not be charged as durable tenant-published leases: {durable:?}"
    );
}

#[test]
fn successful_plan_only_previews_do_not_consume_the_node_segment_pool() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.node_network_supernet = "10.251.0.0/30".to_owned();
    config.node_tenant_subnet_prefix = 30;
    let backend = ContainerSandboxBackend::new(config);

    for (tenant, sandbox) in [("preview-a", "app-a"), ("preview-b", "app-b")] {
        let spec = sample_spec_for_tenant(tenant, sandbox);
        backend
            .plan_start_with_id(&spec, &SandboxId::new(sandbox), None, None)
            .expect("an authority-free preview must not exhaust the one-slot node pool");
        assert!(
            backend
                .segment_allocator
                .inspect_segments(&spec.tenant_id)
                .expect("preview segment authority should inspect")
                .unwrap_or_default()
                .is_empty(),
            "a successful preview must not create attachment-less segment authority"
        );
    }
}

#[test]
fn execute_manifest_without_attachment_config_fails_before_network_effects() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("missing-network-authority"),
            None,
            None,
        )
        .expect("execute plan should reserve attachment authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("execute plan should carry compensation authority");
    manifest.network_config = None;

    let error = backend
        .configure_network(
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(&claim),
        )
        .expect_err("missing attachment config must fail before Netavark");
    assert!(
        error.to_string().contains("no reserved network attachment"),
        "the failure must name the missing attachment authority: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "a manifest without attachment authority must not create a network namespace"
    );
}

#[test]
fn plan_only_backend_rejects_same_tenant_resource_quota_exhaustion() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    // Disk is left unlimited: sandbox specs can no longer carry an (unenforceable)
    // disk_limit_bytes, so each sandbox is charged the default per-sandbox disk
    // accounting. This test isolates the vCPU constraint, so disk must not bind first.
    config.resource_quota_policy = SandboxResourceQuotaPolicy::default()
        .with_max_active_sandboxes_per_tenant(Some(2))
        .with_max_vcpus_per_tenant(Some(2))
        .with_max_memory_bytes_per_tenant(Some(1024))
        .with_max_disk_bytes_per_tenant(None)
        .with_max_log_bytes_per_tenant(Some(512));
    let backend = ContainerSandboxBackend::new(config);

    backend
        .start_sync(
            sample_spec_for_tenant("tenant-a", "db").with_resource_limits(
                SandboxResourceLimits::default()
                    .with_cpu_count(1)
                    .with_memory_limit_bytes(512)
                    .with_log_limit_bytes(256),
            ),
        )
        .expect("first sandbox should fit within tenant quota");

    let error = backend
        .start_sync(
            sample_spec_for_tenant("tenant-a", "api").with_resource_limits(
                SandboxResourceLimits::default()
                    .with_cpu_count(2)
                    .with_memory_limit_bytes(512)
                    .with_log_limit_bytes(256),
            ),
        )
        .expect_err("second sandbox should exceed same-tenant vCPU quota");

    assert!(
        error.to_string().contains("sandbox vCPU quota exceeded")
            && error.to_string().contains("tenant-a"),
        "expected tenant vCPU quota error, got: {error}"
    );

    backend
        .start_sync(
            sample_spec_for_tenant("tenant-b", "api").with_resource_limits(
                SandboxResourceLimits::default()
                    .with_cpu_count(2)
                    .with_memory_limit_bytes(512)
                    .with_log_limit_bytes(256),
            ),
        )
        .expect("other tenant should have an independent quota bucket");
}

#[test]
fn image_backed_plan_uses_direct_conmon_launch_for_materialized_rootfs() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let rootfs_path = temp_dir.path().join("materialized-rootfs");

    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &sandbox_id(),
            Some(&sample_launch_defaults(rootfs_path.clone())),
            Some(sample_rootfs_artifact(rootfs_path)),
        )
        .expect("image-backed plan should lower");

    assert_eq!(
        plan.manifest.conmon_launch.create_command.program,
        PathBuf::from("conmon")
    );
    assert_eq!(
        plan.manifest.conmon_launch.start_command.program,
        PathBuf::from("crun")
    );
    assert!(
        plan.manifest
            .conmon_launch
            .create_command
            .args
            .first()
            .map(String::as_str)
            != Some("unshare"),
        "materialized rootfs launches should not be wrapped in buildah unshare"
    );
}

#[test]
fn image_backed_plan_merges_image_env_with_process_overrides() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let rootfs_path = temp_dir.path().join("materialized-rootfs");
    let mut defaults = sample_launch_defaults(rootfs_path.clone());
    defaults.process = SandboxProcessSpec::new(["/bin/app"]).with_env([
        "PATH=/image/bin",
        "IMAGE_ONLY=yes",
        "OVERRIDE_ME=image",
    ]);
    let mut spec = sample_spec();
    spec.process = SandboxProcessSpec::new(Vec::<String>::new())
        .with_env(["OVERRIDE_ME=compose", "APP_ENV=dev"]);

    let plan = backend
        .plan_start_with_id(
            &spec,
            &sandbox_id(),
            Some(&defaults),
            Some(sample_rootfs_artifact(rootfs_path)),
        )
        .expect("image-backed plan should lower with merged process env");

    assert_eq!(
        plan.manifest.spec.process.env,
        vec![
            "PATH=/image/bin".to_owned(),
            "IMAGE_ONLY=yes".to_owned(),
            "OVERRIDE_ME=compose".to_owned(),
            "APP_ENV=dev".to_owned(),
        ],
        "container image env should survive while process env keeps override precedence"
    );

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    for expected in [
        "PATH=/image/bin",
        "IMAGE_ONLY=yes",
        "OVERRIDE_ME=compose",
        "APP_ENV=dev",
    ] {
        assert!(
            env.contains(&expected),
            "bundle env should include merged entry {expected:?}: {env:?}"
        );
    }
    assert!(
        !env.contains(&"OVERRIDE_ME=image"),
        "bundle env should replace image env with process override: {env:?}"
    );
}

#[test]
fn rootfs_plan_resolves_entrypoint_command_and_user_without_image_defaults() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut spec = sample_spec();
    spec.process = SandboxProcessSpec::new(Vec::<String>::new())
        .with_entrypoint(["/bin/sh", "-lc"])
        .with_command(["exec app"])
        .with_user("1001:1002");

    let plan = backend
        .plan_start_with_id(&spec, &sandbox_id(), None, None)
        .expect("rootfs plan should lower entrypoint/command without image defaults");

    assert_eq!(
        plan.manifest.spec.process.args,
        vec!["/bin/sh", "-lc", "exec app"],
        "rootfs entrypoint and command must become runtime process args"
    );

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    assert_eq!(
        config["process"]["args"],
        serde_json::json!(["/bin/sh", "-lc", "exec app"])
    );
    assert_eq!(config["process"]["user"]["uid"], serde_json::json!(1001));
    assert_eq!(config["process"]["user"]["gid"], serde_json::json!(1002));
}
