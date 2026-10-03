//! Cross-backend contracts for one injected OCI network process.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::{Cidr, TenantId};
use nimbus_network::{LocalNetworkManager, LocalNetworkStateStore, PortLeasePhase};
use nimbus_proxy::{WorkloadPep, WorkloadPepConfig};
use nimbus_sandbox::{SandboxId, SandboxPortBinding};
use nimbus_sandbox_container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
};
use nimbus_sandbox_host::egress::{egress_decision_log_root, egress_trust_anchor_root};
use nimbus_sandbox_host::network::OciNetworkProcess;
use nimbus_sandbox_host::port_lease::new_launch_reservation_claim;
use nimbus_sandbox_host::port_lifecycle::SandboxLaunchPortPlan;
use tempfile::TempDir;

use crate::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};

fn fixture_process() -> (TempDir, Arc<OciNetworkProcess>) {
    let root = TempDir::new().expect("network process root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("manager bootstrap should claim the process authority");
    let process = OciNetworkProcess::new(
        bootstrap.authority(),
        Cidr::parse("10.80.0.0/16").expect("fixture super-net should validate"),
        24,
    )
    .expect("the first OCI composition should own process lifetimes");
    drop(bootstrap);
    (root, process)
}

fn injected_container_and_krun(
    root: &TempDir,
    process: &Arc<OciNetworkProcess>,
) -> (
    ContainerSandboxBackend,
    KrunSandboxBackend,
    PathBuf,
    PathBuf,
) {
    let container_root = root.path().join("container-workload");
    let krun_root = root.path().join("krun-workload");
    let container = ContainerSandboxBackend::with_network_process(
        container_process_config(&container_root, root.path(), ContainerStartMode::PlanOnly),
        Arc::clone(process),
    )
    .expect("container should authenticate the process composition");
    let krun = KrunSandboxBackend::with_network_process(
        krun_process_config(&krun_root, root.path(), KrunStartMode::PlanOnly),
        Arc::clone(process),
    )
    .expect("krun should authenticate the process composition");
    (container, krun, container_root, krun_root)
}

fn container_process_config(
    workload_root: &Path,
    network_root: &Path,
    start_mode: ContainerStartMode,
) -> ContainerSandboxBackendConfig {
    let mut config =
        ContainerSandboxBackendConfig::plan_only(workload_root.join("bundles"), workload_root)
            .with_network_state_root(network_root);
    config.node_network_supernet = "10.80.0.0/16".to_owned();
    config.node_tenant_subnet_prefix = 24;
    config.start_mode = start_mode;
    config
}

fn krun_process_config(
    workload_root: &Path,
    network_root: &Path,
    start_mode: KrunStartMode,
) -> KrunSandboxBackendConfig {
    let mut config =
        KrunSandboxBackendConfig::plan_only(workload_root.join("bundles"), workload_root)
            .with_network_state_root(network_root);
    config.node_network_supernet = "10.80.0.0/16".to_owned();
    config.node_tenant_subnet_prefix = 24;
    config.start_mode = start_mode;
    config
}

#[test]
fn oci_network_process_injected_backends_share_one_segment_state_and_revision_stream() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let (root, process) = fixture_process();
    let authority = process.authority();
    let container_root = root.path().join("container-workload");
    let krun_root = root.path().join("krun-workload");

    let mut container_config =
        ContainerSandboxBackendConfig::plan_only(container_root.join("bundles"), &container_root)
            .with_network_state_root(root.path());
    container_config.node_network_supernet = "10.80.0.0/16".to_owned();
    container_config.node_tenant_subnet_prefix = 24;
    container_config.start_mode = ContainerStartMode::PlanOnly;
    let container =
        ContainerSandboxBackend::with_network_process(container_config, Arc::clone(&process))
            .expect("container should authenticate the process composition");

    let mut krun_config =
        KrunSandboxBackendConfig::plan_only(krun_root.join("bundles"), &krun_root)
            .with_network_state_root(root.path());
    krun_config.node_network_supernet = "10.80.0.0/16".to_owned();
    krun_config.node_tenant_subnet_prefix = 24;
    krun_config.start_mode = KrunStartMode::PlanOnly;
    let krun = KrunSandboxBackend::with_network_process(krun_config, process)
        .expect("krun should authenticate the process composition");

    let container_segments = container.segment_allocator_handle_for_test();
    let krun_segments = krun.segment_allocator_handle_for_test();
    assert!(
        Arc::ptr_eq(&container_segments, &krun_segments),
        "both injected facades must retain the exact process-owned segment adapter"
    );

    let before = authority_revision(&authority);
    let container_tenant =
        TenantId::new("segment-through-container").expect("fixture tenant should validate");
    let container_allocation = container_segments
        .segments_for(&container_tenant)
        .expect("container facade should allocate through the retained adapter");
    let after_container = authority_revision(&authority);
    assert_eq!(
        krun_segments
            .inspect_segments(&container_tenant)
            .expect("krun facade should inspect the shared authority")
            .expect("container allocation should be visible through krun"),
        container_allocation
    );

    let krun_tenant =
        TenantId::new("segment-through-krun").expect("fixture tenant should validate");
    let krun_allocation = krun_segments
        .segments_for(&krun_tenant)
        .expect("krun facade should allocate through the retained adapter");
    let after_krun = authority_revision(&authority);
    assert_eq!(
        container_segments
            .inspect_segments(&krun_tenant)
            .expect("container facade should inspect the shared authority")
            .expect("krun allocation should be visible through container"),
        krun_allocation
    );
    assert!(
        before < after_container && after_container < after_krun,
        "both injected facades must advance one manager-owned revision stream: \
         {before} -> {after_container} -> {after_krun}"
    );

    for workload_root in [&container_root, &krun_root] {
        assert!(
            !LocalNetworkStateStore::authority_path_for(workload_root).exists(),
            "portable segment authority must not be recreated under {}",
            workload_root.display()
        );
    }
}

#[test]
fn oci_network_process_contract_container_and_krun_share_real_pep_lifecycle_authority() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let (root, process) = fixture_process();
    let (container_backend, krun_backend, container_artifacts, krun_artifacts) =
        injected_container_and_krun(&root, &process);
    let container = container_backend.egress_registry_handle_for_test();
    let krun = krun_backend.egress_registry_handle_for_test();
    let tenant = TenantId::new("process-pep").expect("fixture tenant should validate");
    let sandbox = SandboxId::new("shared-pep");
    let proxy = WorkloadPep::start(WorkloadPepConfig::without_active_policy())
        .expect("test PEP should bind a real process-owned listener");

    assert!(
        container
            .decision_log_path_for_test(&tenant, &sandbox)
            .starts_with(egress_decision_log_root(&container_artifacts))
    );
    assert!(
        container
            .trust_anchor_path_for_test(&tenant, &sandbox)
            .starts_with(egress_trust_anchor_root(&container_artifacts))
    );
    assert!(
        krun.decision_log_path_for_test(&tenant, &sandbox)
            .starts_with(egress_decision_log_root(&krun_artifacts))
    );
    assert!(
        krun.trust_anchor_path_for_test(&tenant, &sandbox)
            .starts_with(egress_trust_anchor_root(&krun_artifacts))
    );
    container
        .insert_running_for_test(&tenant, &sandbox, proxy)
        .expect("container facade should install the real PEP lifecycle");
    let duplicate = WorkloadPep::start(WorkloadPepConfig::without_active_policy())
        .expect("duplicate test PEP should bind before registry admission");
    let duplicate_error = krun
        .insert_running_for_test(&tenant, &sandbox, duplicate)
        .expect_err("krun facade must observe the shared duplicate lifecycle");
    assert!(
        duplicate_error.to_string().contains("already registered"),
        "duplicate diagnostics should preserve the shared workload key: {duplicate_error}"
    );
    assert!(
        krun.contains(&tenant, &sandbox)
            .expect("krun facade should inspect the shared engine"),
        "krun must observe the PEP installed through the container facade"
    );
    krun.stop_with_assignment(&tenant, &sandbox, None)
        .expect("krun facade should stop the exact shared test lifecycle");
    assert!(
        !container
            .contains(&tenant, &sandbox)
            .expect("container facade should inspect the shared engine"),
        "teardown through krun must be visible through container"
    );
    let retry = WorkloadPep::start(WorkloadPepConfig::without_active_policy())
        .expect("retry PEP should bind after exact shared teardown");
    container
        .insert_running_for_test(&tenant, &sandbox, retry)
        .expect("container facade should retry only after shared teardown");
    assert!(
        krun.contains(&tenant, &sandbox)
            .expect("krun facade should inspect the shared retry"),
        "retry through container must be visible through krun"
    );
    container
        .stop_with_assignment(&tenant, &sandbox, None)
        .expect("container facade should stop the shared retry");
    assert_ne!(
        container_artifacts, krun_artifacts,
        "the proof requires distinct backend-local artifact roots"
    );
}

#[test]
fn oci_network_process_contract_container_and_krun_share_real_netavark_lifetime_authority() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let (root, process) = fixture_process();
    let (container_backend, krun_backend, _, _) = injected_container_and_krun(&root, &process);
    let container_registry = container_backend.netavark_port_lifetimes_handle_for_test();
    let krun_registry = krun_backend.netavark_port_lifetimes_handle_for_test();
    let coordinator = process.port_lease_coordinator(15_000..=15_001, None);
    let tenant = TenantId::new("process-netavark").expect("fixture tenant should validate");
    let sandbox = SandboxId::new("shared-netavark");

    let first_bindings = [SandboxPortBinding::tcp("container-http", 15_000, 8080)];
    let first_claim = new_launch_reservation_claim().expect("first claim should mint");
    let mut first = coordinator
        .reserve_launch_ports_for_sandbox(
            SandboxLaunchPortPlan::new(&tenant, &sandbox, &first_bindings, &[]),
            &first_claim,
        )
        .expect("first Netavark listener should reserve");
    first
        .confirm_manifest_published()
        .expect("first reservation should publish durably");
    let first_batch = coordinator
        .claim_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &first_bindings,
            &first.published_leases,
        )
        .expect("first Netavark lifetime should claim");
    coordinator
        .activate_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &first_bindings,
            &first.published_leases,
            &first_batch,
        )
        .expect("first Netavark lifetime should activate");

    let second_bindings = [SandboxPortBinding::tcp("krun-http", 15_001, 8081)];
    let second_claim = new_launch_reservation_claim().expect("second claim should mint");
    let mut second = coordinator
        .reserve_launch_ports_for_sandbox(
            SandboxLaunchPortPlan::new(&tenant, &sandbox, &second_bindings, &[]),
            &second_claim,
        )
        .expect("second Netavark listener should reserve");
    second
        .confirm_manifest_published()
        .expect("second reservation should publish durably");
    let second_batch = coordinator
        .claim_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &second_bindings,
            &second.published_leases,
        )
        .expect("second Netavark lifetime should claim");
    coordinator
        .activate_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &second_bindings,
            &second.published_leases,
            &second_batch,
        )
        .expect("second Netavark lifetime should activate");

    container_registry
        .insert(&tenant, &sandbox, first_batch)
        .map_err(|(error, _)| error)
        .expect("container facade should retain the first live batch");
    let (duplicate, second_batch) = krun_registry
        .insert(&tenant, &sandbox, second_batch)
        .expect_err("krun facade must observe the shared duplicate key");
    assert!(
        duplicate.to_string().contains("already owns"),
        "duplicate diagnostics should name shared ownership: {duplicate}"
    );
    drop(
        krun_registry
            .take(&tenant, &sandbox)
            .expect("krun facade should access the shared registry")
            .expect("the first batch should remain retained"),
    );
    krun_registry
        .insert(&tenant, &sandbox, second_batch)
        .map_err(|(error, _)| error)
        .expect("the second batch may install only after exact shared take");

    let authority = process.authority().port_leases();
    assert_eq!(
        authority
            .inspect(second.published_leases[0].lease_id())
            .expect("second lease should inspect")
            .expect("second lease should remain durable")
            .phase(),
        PortLeasePhase::Active
    );
    drop(
        container_registry
            .take(&tenant, &sandbox)
            .expect("container facade should observe krun's insertion")
            .expect("the second batch should remain retained"),
    );
}

fn authority_revision(authority: &nimbus_network::LocalNetworkAuthority) -> u64 {
    let bytes = match fs::read(authority.authority_path()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(error) => panic!("network authority should remain readable: {error}"),
    };
    let envelope: serde_json::Value =
        serde_json::from_slice(&bytes).expect("network authority should remain valid JSON");
    envelope["body"]["revision"]
        .as_u64()
        .expect("network authority should carry a numeric revision")
}
