use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use nimbus_core::{Cidr, TenantId};
use nimbus_network::{LocalNetworkManager, LocalNetworkStateStore, PortLeasePhase};
use nimbus_proxy::{WorkloadPep, WorkloadPepConfig};
use tempfile::TempDir;

use super::{OciNetworkProcess, OciNetworkProcessError};
use crate::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
};
use crate::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use crate::backends::oci::egress::{egress_decision_log_root, egress_trust_anchor_root};
use crate::backends::oci::network::OciNetworkLayout;
use crate::backends::oci::port_lease::new_launch_reservation_claim;
use crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan;
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

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
fn oci_network_process_contract_has_exactly_one_concurrent_winner() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let root = TempDir::new().expect("network process root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("manager bootstrap should claim the process authority");
    let authority = bootstrap.authority();
    let topology = Cidr::parse("10.80.0.0/16").expect("fixture super-net should validate");

    let authority_before_invalid = fs::read(authority.authority_path()).ok();
    assert!(matches!(
        OciNetworkProcess::new(authority.clone(), topology, 15),
        Err(OciNetworkProcessError::InvalidTenantPrefix {
            node_supernet,
            attempted: 15,
        }) if node_supernet == topology
    ));
    assert_eq!(
        fs::read(authority.authority_path()).ok(),
        authority_before_invalid,
        "invalid topology must not mutate durable network authority"
    );

    let process =
        OciNetworkProcess::new(authority.clone(), topology, 24).expect("first process should open");

    #[cfg(unix)]
    {
        let alias_parent = TempDir::new().expect("alias parent should exist");
        let alias_root = alias_parent.path().join("network-root-alias");
        std::os::unix::fs::symlink(root.path(), &alias_root)
            .expect("canonical network-root alias should create");
        let active_before_alias = fs::read(authority.authority_path()).ok();
        process
            .authenticate_backend_config(&alias_root, "10.80.0.0/16", 24)
            .expect("the process must accept a configured canonical authority alias");
        assert_eq!(
            fs::read(authority.authority_path()).ok(),
            active_before_alias,
            "alias authentication must not mutate durable authority"
        );
    }

    let divergent_parent = TempDir::new().expect("divergent parent should exist");
    let divergent_root = divergent_parent.path().join("uncreated-network-root");
    let active_before_divergence = fs::read(authority.authority_path()).ok();
    let divergence = process
        .authenticate_backend_config(&divergent_root, "10.80.0.0/16", 24)
        .expect_err("a divergent configured authority must fail before creation");
    match divergence {
        OciNetworkProcessError::AuthorityRootMismatch(error) => {
            assert_eq!(error.active_authority_path(), authority.authority_path());
            assert_eq!(
                error.attempted_authority_path(),
                LocalNetworkStateStore::authority_path_for(&divergent_root)
            );
        }
        other => panic!("expected typed divergent-root evidence, got {other}"),
    }
    assert_eq!(
        fs::read(authority.authority_path()).ok(),
        active_before_divergence,
        "divergent-root rejection must not mutate active durable authority"
    );
    assert!(
        !divergent_root.exists(),
        "divergent-root rejection must not create attempted authority"
    );

    let duplicate = OciNetworkProcess::new(authority.clone(), topology, 24)
        .expect_err("a second process composition must fail");
    assert!(matches!(
        duplicate,
        OciNetworkProcessError::DuplicateProcessComposition { .. }
    ));

    let mismatch = OciNetworkProcess::new(
        authority.clone(),
        Cidr::parse("10.81.0.0/16").expect("alternate super-net should validate"),
        25,
    )
    .expect_err("a topology-substituted process composition must fail");
    match mismatch {
        OciNetworkProcessError::DuplicateProcessComposition {
            active_supernet,
            attempted_supernet,
            active_tenant_prefix,
            attempted_tenant_prefix,
            ..
        } => {
            assert_eq!(active_supernet, topology);
            assert_ne!(attempted_supernet, active_supernet);
            assert_eq!(active_tenant_prefix, 24);
            assert_eq!(attempted_tenant_prefix, 25);
        }
        other => panic!("expected typed duplicate composition evidence, got {other}"),
    }

    drop(process);

    let barrier = Arc::new(Barrier::new(3));
    let contenders = ["container", "krun"].map(|_| {
        let authority = authority.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            OciNetworkProcess::new(authority, topology, 24)
        })
    });
    barrier.wait();
    let outcomes = contenders.map(|thread| thread.join().expect("contender should not panic"));
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "concurrent process composition must have one winner"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    Err(OciNetworkProcessError::DuplicateProcessComposition { .. })
                )
            })
            .count(),
        1,
        "the concurrent loser must receive typed duplicate evidence"
    );
    drop(outcomes);

    let reopened = OciNetworkProcess::new(authority, topology, 24)
        .expect("final process drop should permit deterministic reopen");
    drop(reopened);
}

#[test]
fn oci_network_process_retains_ipam_authority_and_rejects_substituted_layout_without_mutation() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let (root, process) = fixture_process();
    let foreign = TempDir::new().expect("foreign network root should exist");
    let tenant = TenantId::new("process-ipam").expect("fixture tenant should validate");
    let sandbox = SandboxId::new("process-ipam");
    let active_layout = OciNetworkLayout::with_roots(
        root.path().join("workloads"),
        root.path(),
        &tenant,
        &sandbox,
    );
    let substituted_layout = OciNetworkLayout::with_roots(
        root.path().join("workloads"),
        foreign.path(),
        &tenant,
        &sandbox,
    );
    let active_authority_path = process.authority().authority_path().to_path_buf();
    let foreign_authority_path = LocalNetworkStateStore::authority_path_for(foreign.path());
    let active_before = fs::read(&active_authority_path).ok();
    assert!(
        !foreign_authority_path.exists(),
        "the substituted root must begin without durable network authority"
    );

    let ipam = process.ipam_authority();
    assert_eq!(
        ipam.state_root(),
        process.authority().state_root(),
        "the process must retain an IPAM adapter derived from its active authority"
    );
    ipam.authenticate_layout(&active_layout)
        .expect("the retained IPAM authority should authenticate its active root");
    let error = ipam
        .authenticate_layout(&substituted_layout)
        .expect_err("a substituted layout root must fail before state access");
    assert!(
        error
            .to_string()
            .contains("rejected network layout authority")
            && error
                .to_string()
                .contains(&foreign_authority_path.display().to_string()),
        "the refusal must preserve substituted-root evidence: {error}"
    );
    assert_eq!(
        fs::read(&active_authority_path).ok(),
        active_before,
        "layout authentication must not mutate the active durable authority"
    );
    assert!(
        !foreign_authority_path.exists(),
        "rejected layout authentication must not create foreign authority"
    );
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
