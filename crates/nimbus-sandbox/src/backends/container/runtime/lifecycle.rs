use super::support::*;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::network::{
    FixedOciEgressPinProvider, MachinePortPreparationReleaseAuthority, OciEgressPinProvider,
    OciSegmentAllocator, RecordingSegmentAllocator, SegmentAllocatorOperation,
    default_network_attachment_id, panicking_machine_port_proxy_for_test,
};
use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use nimbus_network::{
    NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim, PortLeasePhase,
};

#[path = "tests/absent_runtime_projection.rs"]
mod absent_runtime_projection;
#[path = "tests/attachment_readiness.rs"]
mod attachment_readiness;
#[path = "tests/creator_persistence.rs"]
mod creator_persistence;
#[path = "tests/execute_inspection.rs"]
mod execute_inspection;
mod launch_cleanup;
#[path = "tests/machine_forwarded_readiness.rs"]
mod machine_forwarded_readiness;
#[path = "tests/plan_only_inspection.rs"]
mod plan_only_inspection;
#[path = "tests/provider_cleanup.rs"]
mod provider_cleanup;
#[path = "tests/runner_recovery.rs"]
mod runner_recovery;
#[path = "tests/runner_reliability.rs"]
mod runner_reliability;
#[path = "tests/status_callbacks.rs"]
mod status_callbacks;
#[path = "tests/terminal_finality.rs"]
mod terminal_finality;

#[test]
fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    std::fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

    assert_eq!(
        backend
            .detect_runtime_status(&manifest)
            .expect("status should resolve"),
        SandboxStatus::Failed
    );
}

#[test]
fn pre_netavark_setup_failure_preserves_no_effect_authority() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/false");
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("container-setup-detach-compensation"),
            None,
            None,
        )
        .expect("execute manifest should reserve complete network authority")
        .manifest;
    let claims = backend
        .port_lease_coordinator()
        .claim_netavark_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("test must cross the durable claim boundary");
    std::fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns parent should exist"),
    )
    .expect("netns parent should create");
    std::fs::write(&manifest.network_layout.netns_path, b"owned test netns\n")
        .expect("netns retry handle should exist");

    let error = backend
        .complete_network_setup(
            &manifest,
            manifest
                .network_config
                .as_ref()
                .expect("planned launch should retain network config"),
            None,
            Err(SandboxError::OperationFailed {
                message: "forced netavark setup failure".to_owned(),
            }),
        )
        .expect_err("failed setup must enter the exact detach compensation seam");
    let message = error.to_string();
    assert!(
        message.contains("forced netavark setup failure")
            && !message.contains("detach compensation also failed"),
        "pre-provider failure must preserve the primary error without inventing ambiguity: \
         {message}"
    );
    if cfg!(target_os = "linux") {
        assert!(
            !manifest.network_layout.netns_path.exists(),
            "the separately owned namespace may be removed after Netavark proves no effect"
        );
    }
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should reopen");
    for (request, expected_claim) in manifest.port_leases.iter().zip(claims) {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("claimed lease must remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(
            record.bind_claim(),
            Some(&expected_claim),
            "outer launch compensation still owns each exact unactivated bind claim"
        );
    }
}

#[test]
fn foreign_initial_launch_claim_fails_before_container_provider_effects() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("foreign-container-launch-claim"),
            None,
            None,
        )
        .expect("launch should reserve its complete port batch")
        .manifest;
    let authoritative_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain coordinator authority");
    let foreign_provider: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    manifest.launch_reservation_claim = Some(NetworkReservationClaim::new(
        NetworkProviderHandle::new(foreign_provider, "foreign-container-coordinator")
            .expect("foreign claim should validate"),
    ));
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("execute launch should reserve its PEP")
            .port_lease
            .clone(),
    );

    let error = backend
        .launch_manifest(&mut manifest, true)
        .expect_err("a foreign coordinator must fail before container provider effects");
    assert!(
        error
            .to_string()
            .contains("different launch reservation coordinator"),
        "the preflight rejection must identify the foreign coordinator: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists(),
        "coordinator authentication must precede namespace and Netavark effects"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should reopen");
    for request in &launch_batch {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(record.reservation_claim(), Some(&authoritative_claim));
        assert!(
            record.bind_claim().is_none()
                && record.binding().is_none()
                && record.failure().is_none()
        );
    }
    backend
        .port_lease_coordinator()
        .release_never_bound_requests(&launch_batch, &authoritative_claim)
        .expect("the exact coordinator should clean up the test batch");
}

#[test]
fn restart_decision_keeps_failed_container_starting_until_backoff_elapses() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit status should write");
    manifest.next_restart_at_millis = Some(1_500);

    let decision =
        mark_restart_decision_after_exit(&mut manifest, 1_000).expect("restart should evaluate");

    assert_eq!(decision, ContainerRestartDecision::WaitingForBackoff);
    assert_eq!(manifest.last_exit_code, Some(42));
    assert_eq!(manifest.restart_count, 0);
    assert_eq!(manifest.next_restart_at_millis, Some(1_500));
    assert_eq!(manifest.status, SandboxStatus::Starting);
    assert_eq!(manifest.handle.status, SandboxStatus::Starting);
}

#[test]
fn restart_decision_counts_due_failed_container_restart() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 2 }),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit status should write");
    manifest.next_restart_at_millis = Some(0);

    let decision =
        mark_restart_decision_after_exit(&mut manifest, 1_000).expect("restart should evaluate");

    assert_eq!(decision, ContainerRestartDecision::RestartNow);
    assert_eq!(manifest.last_exit_code, Some(42));
    assert_eq!(manifest.restart_count, 1);
    assert_eq!(manifest.next_restart_at_millis, None);
    assert_eq!(manifest.status, SandboxStatus::Starting);
    assert_eq!(manifest.handle.status, SandboxStatus::Starting);
}

/// NNC0.6a fail-before baseline for NNCF20. Inspection owns a stale manifest
/// copy, reaches the provider-launch entry through restart policy, and parks.
/// The coordinator then durably withdraws the workload before releasing that
/// launch. No readiness outcome can satisfy this side-effect assertion.
#[test]
#[ignore = "NNC0.6a expected red until NNC5.6/NNC6.4a make inspect side-effect-free and fence restart"]
fn nnc0_6a_container_inspect_must_not_restart_after_withdrawal() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let restart_probe = RestartLaunchTestProbe::new(Duration::from_secs(1));
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_restart_launch_test_probe(restart_probe.clone());
    let sandbox_id = SandboxId::new("nnc0-6a-container");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute manifest should plan")
        .manifest;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.next_restart_at_millis = Some(0);
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("failed exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("restart-eligible manifest should persist");

    let inspect_backend = backend.clone();
    let inspect_id = sandbox_id.clone();
    let inspect_thread = thread::spawn(move || inspect_backend.inspect_sync(&inspect_id));
    if !restart_probe.wait_until_entered() {
        let inspect_result = inspect_thread
            .join()
            .expect("inspect thread should join after a missing barrier");
        panic!(
            "inspect must reach the provider-launch barrier through restart policy; \
             inspect completed instead with {inspect_result:?}"
        );
    }

    let mut withdrawn = manifest;
    withdrawn.shutdown_requested = true;
    withdrawn.next_restart_at_millis = None;
    withdrawn.status = SandboxStatus::Stopped;
    withdrawn.handle.status = SandboxStatus::Stopped;
    withdrawn.handle.published_endpoints.clear();
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist before launch release");

    restart_probe.release();
    let inspected = inspect_thread
        .join()
        .expect("inspect thread should join")
        .expect("current inspect restart should complete through the test provider")
        .expect("manifest should remain inspectable");
    assert_eq!(
        inspected.status,
        SandboxStatus::Starting,
        "precondition: stale inspection currently reactivates the withdrawn manifest"
    );

    assert_eq!(
        restart_probe.effect_count(),
        0,
        "NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before \
         release must veto the stale container restart provider effect"
    );
}

#[test]
fn plan_only_cleanup_does_not_contact_machine_port_forwarder() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let unavailable_port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.machine_port_forwarder = Some(sample_forwarder(unavailable_port));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan-only manifest should lower")
        .manifest;
    assert!(
        manifest.port_leases.is_empty(),
        "plan-only lowering must not reserve host-global port authority"
    );

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("plan-only cleanup must not contact an effect provider it never activated");
}

#[test]
fn machine_proxy_registry_is_tenant_qualified_for_equal_local_sandbox_ids() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let tenant_a = nimbus_core::TenantId::new("tenant-machine-a").expect("tenant id");
    let tenant_b = nimbus_core::TenantId::new("tenant-machine-b").expect("tenant id");
    let id = SandboxId::new("shared-local-sandbox-id");
    {
        let mut registry = backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock");
        registry.insert(
            (tenant_a.clone(), id.clone()),
            MachinePortProxyEntry::Running(MachinePortProxyRegistration {
                port_bindings: Vec::new(),
                port_leases: Vec::new(),
                routes: Vec::new(),
                proxies: Vec::new(),
                lease_authority: None,
                publication_may_exist: false,
            }),
        );
        registry.insert(
            (tenant_b.clone(), id.clone()),
            MachinePortProxyEntry::Running(MachinePortProxyRegistration {
                port_bindings: Vec::new(),
                port_leases: Vec::new(),
                routes: Vec::new(),
                proxies: Vec::new(),
                lease_authority: None,
                publication_may_exist: false,
            }),
        );
    }

    backend
        .stop_machine_port_proxies(&tenant_a, &id, &[], &[])
        .expect("tenant-a proxy set should stop");
    let registry = backend
        .machine_port_proxies
        .lock()
        .expect("machine proxy registry should lock");
    assert!(!registry.contains_key(&(tenant_a, id.clone())));
    assert!(
        registry.contains_key(&(tenant_b, id)),
        "tenant-a teardown must not remove tenant-b's equal local sandbox id"
    );
}

#[test]
fn machine_proxy_rejects_caller_manifest_identity_mismatch_before_effect() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-manifest-owner"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;
    let published = Arc::new(AtomicBool::new(false));
    let published_by_call = Arc::clone(&published);

    let error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &SandboxId::new("machine-substituted-caller"),
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain coordinator claim"),
            ),
            move || {
                published_by_call.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect_err("a substituted caller identity must fail before provider publication");

    assert!(
        error
            .to_string()
            .contains("does not match manifest sandbox"),
        "the rejection must identify the caller/manifest mismatch: {error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "identity validation must precede provider publication"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "identity rejection must not register a provider effect"
    );
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("identity rejection must not bind the requested host port");
    drop(port_probe);
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "identity rejection must precede durable provider adoption"
    );
    backend
        .port_lease_coordinator()
        .release_never_bound_requests(
            &manifest.port_leases,
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("planned launch should retain coordinator claim"),
        )
        .expect("test reservation should release after absence is proven");
}

#[test]
fn machine_proxy_activation_failure_drops_listeners_and_abandons_exact_claims() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-activation-failure"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;

    let error = backend
        .ensure_machine_port_proxies_running_with_activation_failure(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            || {
                Err(SandboxError::OperationFailed {
                    message: "injected machine activation failure".to_owned(),
                })
            },
        )
        .expect_err("injected durable activation failure must fail startup");
    assert!(
        error
            .to_string()
            .contains("injected machine activation failure"),
        "the provider failure must remain primary: {error}"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "failed activation must not register a provider"
    );
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("failed activation must drop every inert listener before compensation");
    drop(port_probe);
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(record.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert!(
        record.bind_claim().is_none(),
        "proven listener absence must abandon the exact durable bind claim"
    );
    assert!(
        record.binding().is_none(),
        "pre-activation failure must retain no observed provider binding"
    );

    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("the exact manifest should retry after claim compensation");
    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the retry provider should stop");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed retry provider absence should release the test lease");
}

#[test]
fn machine_proxy_activation_ack_loss_inspects_active_binding_and_rebinds() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-activation-ack-loss"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;

    let error = backend
        .ensure_machine_port_proxies_running_with_activation_ack_loss(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            || {
                Err(SandboxError::OperationFailed {
                    message: "injected activation acknowledgement loss".to_owned(),
                })
            },
        )
        .expect_err("ambiguous activation acknowledgement loss must fail startup");
    assert!(
        error
            .to_string()
            .contains("injected activation acknowledgement loss"),
        "the ambiguous activation error must remain primary: {error}"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "ambiguous activation must not register a process-local provider"
    );
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("compensation must drop every inert listener before durable inspection");
    drop(port_probe);
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "exact Active inspection plus confirmed provider stop must prepare the lease for rebind"
    );
    assert!(
        record.bind_claim().is_none() && record.binding().is_none(),
        "rebind preparation must clear only obsolete attempt and provider evidence"
    );

    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("the exact manifest should retry after ambiguous-outcome reconciliation");
    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the retry provider should stop");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed retry provider absence should release the test lease");
}

#[test]
fn machine_proxy_reuse_requires_exact_normalized_forwarding_plan() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-route-owner"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first exact forwarding plan should start");
    let published = Arc::new(AtomicBool::new(false));
    let published_by_call = Arc::clone(&published);

    let error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::new(127, 0, 0, 2)],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain coordinator claim"),
            ),
            move || {
                published_by_call.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect_err("a changed provider target must not reuse the prior live proxy");

    assert!(
        error.to_string().contains("exact listener generation"),
        "the rejection must identify mismatched provider evidence: {error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "a stale provider target must be rejected before publication"
    );
    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the original exact provider should stop");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed provider absence should release the test lease");
}

#[test]
fn machine_publication_rejects_external_address_substitution_before_proxy_or_forwarder_effect() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(
                SandboxPortBinding::tcp("http", port, 8080)
                    .with_host_address(std::net::IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
            ),
            &SandboxId::new("machine-publication-owner"),
            None,
            None,
        )
        .expect("plan should reserve the exact external publication intent")
        .manifest;
    manifest.spec.port_bindings[0].host_address = std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let published = Arc::new(AtomicBool::new(false));
    let published_by_call = Arc::clone(&published);

    let result = backend.ensure_machine_port_proxies_running_with_publication(
        &manifest.handle.id,
        &[Ipv4Addr::LOCALHOST],
        &manifest,
        MachinePortPreparationReleaseAuthority::FreshLaunch(
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("planned launch should retain coordinator claim"),
        ),
        move || {
            published_by_call.store(true, Ordering::SeqCst);
            Ok(())
        },
    );
    if result.is_ok() {
        let _ = backend.stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        );
    }
    let error = result.expect_err(
        "a substituted external address must fail before proxy bind or forwarder publication",
    );
    assert!(
        error.to_string().contains("does not match the caller"),
        "the rejection must identify divergent durable publication intent: {error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "address substitution must fail before forwarder publication"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "address substitution must not retain a provider effect"
    );
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "address rejection must precede durable provider adoption"
    );
    backend
        .port_lease_coordinator()
        .release_never_bound_requests(
            &manifest.port_leases,
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("planned launch should retain coordinator claim"),
        )
        .expect("test reservation should release after absence is proven");
}

#[test]
fn machine_proxy_restart_rebinds_exact_active_lease() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-restart-owner"),
            None,
            None,
        )
        .expect("plan should reserve the restart listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first provider generation should start");
    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart stop should acknowledge provider absence");

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open");
    let rebound = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("rebound lease should inspect")
        .expect("rebound lease should remain durable");
    assert_eq!(
        rebound.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "acknowledged restart stop must retain the selected port as exact rebind authority"
    );

    backend
        .ensure_machine_port_proxies_running_for_restart(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
        )
        .expect("the same incarnation must claim and restart its retained listener");
    let active = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("active lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(active.phase(), nimbus_network::PortLeasePhase::Active);

    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("final provider stop should succeed");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("final provider absence should release the test lease");
}

#[test]
fn machine_proxy_accept_worker_panic_reports_then_cleanup_converges_on_retry() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-worker-panic"),
            None,
            None,
        )
        .expect("plan should reserve the listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("provider generation should start");
    {
        let mut registry = backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock");
        let entry = registry
            .get_mut(&(manifest.spec.tenant_id.clone(), manifest.handle.id.clone()))
            .expect("running registration should exist");
        let MachinePortProxyEntry::Running(registration) = entry else {
            panic!("provider generation should still be running");
        };
        let replacement = panicking_machine_port_proxy_for_test(SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            port,
        ));
        let mut original = std::mem::replace(&mut registration.proxies[0], replacement);
        original
            .shutdown()
            .expect("the real provider should stop before failure injection");
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        let provider_is_running = {
            let registry = backend
                .machine_port_proxies
                .lock()
                .expect("machine proxy registry should lock");
            let MachinePortProxyEntry::Running(registration) = registry
                .get(&(manifest.spec.tenant_id.clone(), manifest.handle.id.clone()))
                .expect("injected registration should remain")
            else {
                panic!("injected registration should remain running");
            };
            registration.proxies[0].provider_is_running()
        };
        if !provider_is_running {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "injected provider worker did not exit within one second"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let published = Arc::new(AtomicBool::new(false));
    let publish_probe = Arc::clone(&published);
    let ensure_error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::Retain,
            move || {
                publish_probe.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect_err("an exited retained provider must fence publication");
    assert!(
        ensure_error.to_string().contains("provider worker exited"),
        "the liveness fence should name the failed process-local provider: {ensure_error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "durable Active evidence must not republish an exited process-local provider"
    );

    let first = backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect_err("accept-worker panic must deny restart cleanup");
    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("cleanup retry must consume the joined provider-absence proof");
    assert!(
        first
            .to_string()
            .contains("accept worker panicked during shutdown"),
        "the first attempt must preserve the provider diagnostic: {first}"
    );

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("listener authority should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "joined provider absence must authorize the exact restart rebind"
    );
    let registry = backend
        .machine_port_proxies
        .lock()
        .expect("machine proxy registry should lock");
    assert!(
        !registry.contains_key(&(manifest.spec.tenant_id.clone(), manifest.handle.id.clone())),
        "completed cleanup must retire its generation-qualified tombstone"
    );
}

#[test]
fn machine_proxy_restart_waits_for_external_unexpose_before_rebind() {
    let published_port = unused_loopback_port();
    let listener = TcpListener::bind("127.0.0.1:0").expect("forwarder should bind");
    let forwarder_port = listener
        .local_addr()
        .expect("forwarder address should resolve")
        .port();
    let configured_forwarder = sample_forwarder(forwarder_port);
    let (request_tx, request_rx) = mpsc::channel();
    let (response_tx, response_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut unexpose, _) = listener.accept().expect("unexpose should connect");
        read_complete_http_request(&mut unexpose);
        request_tx
            .send(())
            .expect("unexpose receipt should be observable");
        response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("unexpose response should be released");
        unexpose
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n")
            .expect("native unexpose response should write");
        let (mut inspection, _) = listener
            .accept()
            .expect("absence inspection should connect");
        read_complete_http_request(&mut inspection);
        inspection
            .write_all(
                b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\
                  Content-Length: 2\r\n\r\n[]",
            )
            .expect("native absence list should write");
    });

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(configured_forwarder);
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", published_port, 8080)),
            &SandboxId::new("machine-restart-unexpose"),
            None,
            None,
        )
        .expect("plan should reserve the restart listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("provider generation should start");
    let cleanup = backend
        .begin_machine_port_proxy_restart(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart cleanup should begin")
        .expect("the running provider should yield exact cleanup evidence");
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("active lease should inspect")
            .expect("active lease should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "local provider stop alone must not authorize rebind before external unexpose"
    );
    let disposition_substitution = match backend.begin_machine_port_proxy_release(
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        &manifest.spec.port_bindings,
        &manifest.port_leases,
    ) {
        Ok(_) => panic!("a restart tombstone must reject release-disposition substitution"),
        Err(error) => error,
    };
    assert!(
        disposition_substitution
            .to_string()
            .contains("different exact listener generation or disposition"),
        "the tombstone must authenticate its exact disposition: {disposition_substitution}"
    );

    let unexpose_backend = backend.clone();
    let forwarder = backend
        .config
        .machine_port_forwarder
        .clone()
        .expect("forwarder should remain configured");
    let unexpose_thread = thread::spawn(move || {
        let result =
            unexpose_backend.unexpose_machine_port_proxy_publications(&cleanup, &forwarder);
        (cleanup, result)
    });
    request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("unexpose should reach the external provider");

    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("fenced lease should inspect")
            .expect("fenced lease should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "an in-flight unexpose must retain the exact active generation fence"
    );
    let replacement = backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect_err("a stopping tombstone must reject replacement publication");
    assert!(
        replacement
            .to_string()
            .contains("cleanup is still in progress"),
        "replacement rejection should identify the stopping tombstone: {replacement}"
    );

    response_tx
        .send(())
        .expect("external unexpose acknowledgement should release");
    let (cleanup, unexpose_result) = unexpose_thread.join().expect("unexpose thread should join");
    unexpose_result.expect("external unexpose should be acknowledged");
    server.join().expect("forwarder server should join");
    backend
        .complete_machine_port_proxy_cleanup(&cleanup)
        .expect("acknowledged unexpose may complete the atomic rebind transition");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("rebind lease should inspect")
            .expect("rebind lease should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "only external unexpose acknowledgement may authorize exact restart rebind"
    );
    let manager = backend.port_lease_coordinator();
    manager
        .withdraw_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart authority should withdraw after confirmed provider absence");
    manager
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("withdrawn restart authority should release");
}

#[test]
fn empty_overlapping_machine_proxy_registry_keeps_live_provider_fenced() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-overlap").expect("tenant should validate");
    let id = SandboxId::new("machine-overlap");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-overlap"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let first = ContainerSandboxBackend::new(config.clone());
    let manifest = first
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    first
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first backend should own and activate the machine proxy");

    let overlapping = ContainerSandboxBackend::new(config);
    assert!(
        overlapping
            .machine_port_proxies
            .lock()
            .expect("overlapping registry should lock")
            .is_empty(),
        "a fresh backend has no process-local provider evidence"
    );
    overlapping
        .port_lease_coordinator()
        .withdraw_bindings(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("teardown must fence the listener before attempting stop");
    let stale_fast_path = first
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect_err("a matching local registry must revalidate durable Active authority");
    assert!(
        stale_fast_path
            .to_string()
            .contains("expected exact Active"),
        "the fast path must reject a concurrently withdrawn generation: {stale_fast_path}"
    );
    let ambiguity = overlapping
        .stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect_err("an empty overlapping registry cannot confirm provider shutdown");
    assert!(
        ambiguity.to_string().contains("live process lifetime"),
        "the error must identify the live process-owner fence: {ambiguity}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(&first.config.network_state_root)
        .expect("authority");
    let record = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Withdrawing,
        "ambiguous stop must retain the host-global fence"
    );
    let collision = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect_err("the original provider must still own the real socket");
    assert_eq!(collision.kind(), std::io::ErrorKind::AddrInUse);

    first
        .withdraw_and_stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the exact local registry should resume withdrawal and release its provider");
    first
        .port_lease_coordinator()
        .release_bindings(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed provider stop may release durable authority");
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("confirmed stop and release must make the real port reusable");
}

#[test]
fn independent_machine_backend_cannot_withdraw_another_process_provider() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant = nimbus_core::TenantId::new("tenant-machine-foreign-withdraw")
        .expect("tenant should validate");
    let id = SandboxId::new("machine-foreign-withdraw");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-foreign-withdraw"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let owner = ContainerSandboxBackend::new(config.clone());
    let manifest = owner
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    owner
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("owner backend should start the machine proxy");

    let foreign = ContainerSandboxBackend::new(config);
    let error = foreign
        .withdraw_and_stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect_err("a backend without the provider registration must not withdraw it");
    assert!(
        error.to_string().contains("live process lifetime"),
        "the failure must identify the still-live foreign provider owner: {error}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(&owner.config.network_state_root)
        .expect("authority should open");
    let record = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Active,
        "foreign teardown must prove provider ownership before changing durable authority"
    );
    let collision = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect_err("the owner provider must remain bound");
    assert_eq!(collision.kind(), std::io::ErrorKind::AddrInUse);

    owner
        .withdraw_and_stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the backend with the exact registration may withdraw and stop");
    owner
        .port_lease_coordinator()
        .release_bindings(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed provider stop may release durable authority");
}

#[test]
fn machine_proxy_lifetime_fences_live_owner_and_recovers_after_owner_drop() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-lifetime").expect("tenant should validate");
    let id = SandboxId::new("machine-lifetime");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-lifetime"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let owner = ContainerSandboxBackend::new(config.clone());
    let manifest = owner
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    owner
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("owner backend should start the machine proxy");

    let authority = nimbus_network::LocalPortLeaseAuthority::open(&owner.config.network_state_root)
        .expect("authority");
    let active = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    let lifetime = active
        .active_lifetime()
        .expect("every machine provider effect must retain a process-owner generation");
    assert_eq!(
        lifetime.effect_scope(),
        nimbus_network::PortLeaseEffectScope::ProviderManaged,
        "the external machine publication may outlive its process coordinator"
    );

    let recovery = ContainerSandboxBackend::new(config);
    let live_error = match recovery.begin_machine_port_proxy_release(
        &tenant,
        &id,
        &manifest.spec.port_bindings,
        &manifest.port_leases,
    ) {
        Ok(_) => panic!("a second coordinator must fail closed while the lifetime owner is live"),
        Err(error) => error,
    };
    assert!(
        live_error.to_string().contains("live process lifetime"),
        "live-owner rejection must identify the exact lifetime fence: {live_error}"
    );
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "a live-owner recovery attempt must not mutate durable authority"
    );

    drop(owner);
    let cleanup = recovery
        .begin_machine_port_proxy_release(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("one successor should acquire dead-owner recovery")
        .expect("dead provider-managed authority requires exact cleanup");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::CleanupPending,
        "owner death must quarantine provider-managed authority before inspection"
    );
    recovery
        .confirm_machine_port_proxy_publication_absent(&cleanup)
        .expect("test provider should authenticate exact publication absence");
    recovery
        .complete_machine_port_proxy_cleanup(&cleanup)
        .expect("exact absence should complete dead-owner cleanup");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Released
    );
}

#[test]
fn machine_proxy_withdrawal_waits_for_inflight_active_validation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-linearization").expect("tenant should validate");
    let id = SandboxId::new("machine-linearization");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-linearization"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("initial ensure should own and activate the machine proxy");

    let (validated_tx, validated_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let ensuring_backend = backend.clone();
    let ensuring_id = id.clone();
    let ensuring_manifest = manifest.clone();
    let ensure_thread = thread::spawn(move || {
        ensuring_backend.ensure_machine_port_proxies_running_at_validation_barrier(
            &ensuring_id,
            &[Ipv4Addr::LOCALHOST],
            &ensuring_manifest,
            move || {
                validated_tx
                    .send(())
                    .expect("validation barrier should signal");
                release_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("validation barrier should release");
            },
        )
    });
    validated_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("ensure should reach the post-validation barrier");

    let (lock_tx, lock_rx) = mpsc::channel();
    let withdrawing_backend = backend.clone();
    let withdrawing_id = id.clone();
    let withdrawing_manifest = manifest.clone();
    let withdraw_thread = thread::spawn(move || {
        withdrawing_backend.withdraw_and_stop_machine_port_proxies_at_lock_barrier(
            &tenant,
            &withdrawing_id,
            &withdrawing_manifest.spec.port_bindings,
            &withdrawing_manifest.port_leases,
            move || {
                lock_tx
                    .send(())
                    .expect("withdrawal lock barrier should signal");
            },
        )
    });
    lock_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("withdrawal should reach the registry lock barrier");

    let phase_during_validation =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open")
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase();

    release_tx
        .send(())
        .expect("inflight validation should release");
    ensure_thread
        .join()
        .expect("ensure thread should join")
        .expect("the already-validated ensure should complete");
    withdraw_thread
        .join()
        .expect("withdraw thread should join")
        .expect("withdrawal should stop the exact proxy after validation completes");

    assert_eq!(
        phase_during_validation,
        nimbus_network::PortLeasePhase::Active,
        "withdrawal must acquire the same lifecycle lock before changing durable authority"
    );
}

#[test]
fn machine_proxy_withdrawal_waits_for_inflight_publication() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant = nimbus_core::TenantId::new("tenant-machine-publish-linearization")
        .expect("tenant should validate");
    let id = SandboxId::new("machine-publish-linearization");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-publish-linearization"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;

    let (publishing_tx, publishing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let publishing_backend = backend.clone();
    let publishing_id = id.clone();
    let publishing_manifest = manifest.clone();
    let publish_thread = thread::spawn(move || {
        publishing_backend.ensure_machine_port_proxies_running_at_publication_barrier(
            &publishing_id,
            &[Ipv4Addr::LOCALHOST],
            &publishing_manifest,
            move || {
                publishing_tx
                    .send(())
                    .expect("publication barrier should signal");
                release_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("publication barrier should release");
                Ok(())
            },
        )
    });
    publishing_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("ensure should reach the publication barrier");

    let withdrawing_backend = backend.clone();
    let lock_probe_backend = backend.clone();
    let withdrawing_id = id.clone();
    let withdrawing_manifest = manifest.clone();
    let (at_lock_tx, at_lock_rx) = mpsc::channel();
    let (withdrawn_tx, withdrawn_rx) = mpsc::channel();
    let withdraw_thread = thread::spawn(move || {
        let result = withdrawing_backend.withdraw_and_stop_machine_port_proxies_at_lock_barrier(
            &tenant,
            &withdrawing_id,
            &withdrawing_manifest.spec.port_bindings,
            &withdrawing_manifest.port_leases,
            move || {
                assert!(
                    matches!(
                        lock_probe_backend.machine_port_proxies.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ),
                    "publication must hold the exact registry mutex immediately before withdrawal tries to acquire it"
                );
                at_lock_tx
                    .send(())
                    .expect("withdrawal lock barrier should signal");
            },
        );
        withdrawn_tx
            .send(())
            .expect("withdrawal completion should signal");
        result
    });
    at_lock_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("withdrawal should reach the registry-lock boundary");

    let phase_during_publication =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open")
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase();

    release_tx
        .send(())
        .expect("inflight publication should release");
    publish_thread
        .join()
        .expect("publication thread should join")
        .expect("publication should complete");
    withdrawn_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("withdrawal should complete after publication releases the registry");
    withdraw_thread
        .join()
        .expect("withdraw thread should join")
        .expect("withdrawal should stop only after publication completes");

    assert_eq!(
        phase_during_publication,
        nimbus_network::PortLeasePhase::Active,
        "publication and withdrawal must share one lifecycle lock"
    );
}

#[test]
fn absent_machine_registry_accepts_only_an_entire_terminal_no_effect_batch() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let first_probe =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("first port probe should bind");
    let first_port = first_probe
        .local_addr()
        .expect("first port should resolve")
        .port();
    let second_probe =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("second port probe should bind");
    let second_port = second_probe
        .local_addr()
        .expect("second port should resolve")
        .port();
    assert_ne!(first_port, second_port);
    drop((first_probe, second_probe));

    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    config.published_port_range = first_port.min(second_port)..=first_port.max(second_port);
    let backend = ContainerSandboxBackend::new(config);
    let manager = backend.port_lease_coordinator();
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-terminal").expect("tenant should validate");
    let id = SandboxId::new("machine-terminal");
    let bindings = [
        SandboxPortBinding::tcp("released", first_port, 8080),
        SandboxPortBinding::tcp("failed", second_port, 8081),
    ];
    let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("terminal batch launch claim should mint");
    let mut reservations = manager
        .reserve_launch_ports_for_sandbox(
            crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan::new(
                &tenant,
                &id,
                &bindings,
                &[],
            ),
            &reservation_claim,
        )
        .expect("terminal batch should reserve atomically");
    reservations
        .confirm_manifest_published()
        .expect("fixture should publish its exact launch request set");
    manager
        .release_never_bound_requests(
            std::slice::from_ref(&reservations.published_leases[0]),
            &reservations.reservation_claim,
        )
        .expect("first never-bound listener should release");
    let failed_claim = manager
        .claim_machine_bindings(
            &tenant,
            &id,
            std::slice::from_ref(&bindings[1]),
            std::slice::from_ref(&reservations.published_leases[1]),
        )
        .expect("failed listener should claim its provider attempt")
        .pop()
        .expect("one listener should return one claim");
    manager
        .record_machine_proxy_bind_failure(
            &reservations.published_leases[1],
            &failed_claim,
            SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), second_port),
            std::io::ErrorKind::AddrInUse,
        )
        .expect("second listener should record terminal no-effect failure");

    manager
        .classify_machine_cleanup_batch(&tenant, &id, &bindings, &reservations.published_leases)
        .expect("a Failed/Released batch must classify as uniformly terminal without effect");
    backend
        .stop_machine_port_proxies(&tenant, &id, &bindings, &reservations.published_leases)
        .expect("an absent registry is idempotent when every exact listener is terminal");

    let phases = reservations
        .published_leases
        .iter()
        .map(|request| {
            nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
                .expect("authority should open")
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("terminal record should persist")
                .phase()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            nimbus_network::PortLeasePhase::Released,
            nimbus_network::PortLeasePhase::Failed,
        ],
        "idempotent cleanup must preserve exact terminal evidence"
    );
}

#[test]
fn reload_egress_policy_updates_running_container_proxy() {
    let first = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst");
    let second = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecond");
    let temp_dir = TempDir::new().expect("tempdir should build");
    let proxy_port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_egress_policy(allow_loopback_http_policy(first.addr.port())),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("manifest should persist before reload");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("egress proxy should start on loopback test subnet");
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("running manifest should publish post-launch authority");
    let proxy_addr = manifest
        .egress_proxy
        .as_ref()
        .expect("proxy assignment should exist")
        .bind_addr()
        .expect("proxy bind address should parse");

    let allowed_first = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            first.addr.port()
        ),
    );
    assert!(
        allowed_first.starts_with("HTTP/1.1 200 OK") && allowed_first.contains("first"),
        "initial policy should allow first upstream, got: {allowed_first}"
    );

    backend
        .reload_egress_policy(
            &manifest.handle.id,
            allow_loopback_http_policy(second.addr.port()),
        )
        .expect("egress policy reload should update live proxy");
    let denied_old = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            first.addr.port()
        ),
    );
    let allowed_new = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            second.addr.port()
        ),
    );

    assert!(
        denied_old.starts_with("HTTP/1.1 403 Forbidden"),
        "old upstream should be denied after reload, got: {denied_old}"
    );
    assert!(
        allowed_new.starts_with("HTTP/1.1 200 OK") && allowed_new.contains("second"),
        "new upstream should be allowed after reload, got: {allowed_new}"
    );
    let reloaded_manifest = backend
        .read_manifest(&manifest.handle.id)
        .expect("manifest read should succeed")
        .expect("manifest should remain");
    assert_eq!(
        reloaded_manifest.spec.egress.rules()[0].port,
        second.addr.port()
    );
}

fn allow_loopback_http_policy(port: u16) -> EgressPolicy {
    EgressPolicy::new([
        EgressRule::new("loopback-test", EgressProtocol::Http, "127.0.0.1", port)
            .allow_internal_ips(true),
    ])
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral loopback listener should bind")
        .local_addr()
        .expect("ephemeral listener should expose address")
        .port()
}

fn read_complete_http_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("request read timeout should set");
    let mut request = Vec::new();
    let mut expected_len = None;
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).expect("request should read");
        assert!(read > 0, "request closed before its complete body arrived");
        request.extend_from_slice(&chunk[..read]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..header_end])
                .expect("request headers should be UTF-8");
            let content_len = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("valid content length"))
                })
                .unwrap_or(0);
            expected_len = Some(header_end + 4 + content_len);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            return;
        }
    }
}

fn proxy_request(proxy_addr: SocketAddr, request: String) -> String {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .write_all(request.as_bytes())
        .expect("client should write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("client should read response");
    response
}

struct TestHttpServer {
    addr: SocketAddr,
}

impl TestHttpServer {
    fn start(response: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self { addr }
    }
}

/// MTN5 DNS-off posture: the container backend disables the in-subnet
/// aardvark-dns resolver (`enable_dns=false`), matching the krun backend. Under
/// the H1 pin gateway:53 is unreachable, so the resolver is dead weight and a
/// cross-tenant DNS-leak surface; names resolve host-side through the egress PEP.
#[test]
fn container_network_config_disables_bridge_dns_resolver() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));

    let tenant = nimbus_core::TenantId::new("dns-tenant").expect("tenant should parse");
    assert!(
        !backend
            .network_config(&tenant)
            .expect("network config should resolve")
            .enable_dns,
        "the container backend must disable the bridge DNS resolver (enable_dns=false)"
    );
}
