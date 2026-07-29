use super::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

#[path = "tests/post_activation_cleanup.rs"]
mod post_activation_cleanup;
#[path = "tests/readiness.rs"]
mod readiness;
#[path = "tests/registration_failure.rs"]
mod registration_failure;

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn tenant() -> TenantId {
    TenantId::new("tenant-egress").expect("test tenant id should be valid")
}

fn reserve_test_pep_assignment(
    _registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> EgressProxyAssignment {
    let port_lease = port_lease_request(
        tenant_id,
        id,
        "egress-pep",
        OciPortLeaseIntent::host_internal(
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        ),
        nimbus_network::PortRequestMode::ProviderAssigned,
    );
    EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port: 0,
        port_lease,
    }
}

fn start_test_pep(
    registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
    policy: &EgressPolicy,
) -> EgressProxyAssignment {
    let assignment = reserve_test_pep_assignment(registry, tenant_id, id);
    ensure_egress_proxy_running(registry, tenant_id, id, Some(&assignment), policy)
        .expect("test PEP should activate from its exact persisted assignment");
    assignment
}

#[test]
fn ensure_running_registers_is_idempotent_and_stop_deregisters() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-seam-01");
    let policy = EgressPolicy::deny_all();
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);

    assert!(!registry.contains(&tenant, &id).unwrap());
    let assignment = start_test_pep(&registry, &tenant, &id, &policy);
    assert!(registry.contains(&tenant, &id).unwrap());
    assert!(
        trust_anchor_path.is_file(),
        "starting a PEP must publish a workload-scoped trust anchor"
    );

    // idempotent: a second ensure neither errors nor double-registers
    ensure_egress_proxy_running(&registry, &tenant, &id, Some(&assignment), &policy).unwrap();
    assert!(registry.contains(&tenant, &id).unwrap());

    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .unwrap();
    assert!(!registry.contains(&tenant, &id).unwrap());
    assert!(
        !trust_anchor_path.exists(),
        "stopping a PEP must clean up its workload-scoped trust anchor"
    );
    // stop is a no-op when nothing is registered
    registry.stop_with_assignment(&tenant, &id, None).unwrap();
}

#[test]
fn equal_tenant_local_sandbox_ids_own_distinct_pep_registrations() {
    let registry = EgressProxyRegistry::new();
    let tenant_a = TenantId::new("tenant-egress-a").expect("tenant id");
    let tenant_b = TenantId::new("tenant-egress-b").expect("tenant id");
    let id = SandboxId::new("shared-local-sandbox-id");

    let tenant_a_assignment = start_test_pep(&registry, &tenant_a, &id, &EgressPolicy::deny_all());
    let tenant_b_assignment = start_test_pep(&registry, &tenant_b, &id, &EgressPolicy::deny_all());

    assert!(registry.contains(&tenant_a, &id).expect("tenant-a lookup"));
    assert!(registry.contains(&tenant_b, &id).expect("tenant-b lookup"));
    assert_ne!(
        registry
            .local_addr(&tenant_a, &id)
            .expect("tenant-a address lookup"),
        registry
            .local_addr(&tenant_b, &id)
            .expect("tenant-b address lookup"),
        "tenant-qualified registrations must retain independent PEP sockets"
    );

    registry
        .stop_with_assignment(&tenant_a, &id, Some(&tenant_a_assignment))
        .expect("tenant-a PEP should stop");
    assert!(
        registry.contains(&tenant_b, &id).expect("tenant-b lookup"),
        "tenant-a teardown must not deregister tenant-b's equal local sandbox id"
    );
    registry
        .stop_with_assignment(&tenant_b, &id, Some(&tenant_b_assignment))
        .expect("tenant-b PEP should stop");
}

#[test]
fn overlapping_live_registry_teardown_without_provider_evidence_retains_fence_and_anchor() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let decision_log_root = state_root.path().join("decision-logs");
    let trust_anchor_root = state_root.path().join("trust-anchors");
    let tenant = tenant();
    let id = SandboxId::new("egress-restart-retire");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("persisted PEP assignment should reserve");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease,
    };
    let first = EgressProxyRegistry::with_roots_and_network_state(
        &decision_log_root,
        &trust_anchor_root,
        state_root.path(),
    );
    let trust_anchor_path = first.trust_anchor_path_for_test(&tenant, &id);
    ensure_egress_proxy_running(
        &first,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("first process should activate the persisted PEP assignment");
    assert!(trust_anchor_path.is_file());
    let restarted = EgressProxyRegistry::with_roots_and_network_state(
        &decision_log_root,
        &trust_anchor_root,
        state_root.path(),
    );
    assert!(
        !restarted
            .contains(&tenant, &id)
            .expect("fresh registry should inspect"),
        "a fresh process starts without the prior in-memory PEP handle"
    );
    let other_tenant = TenantId::new("tenant-egress-other").expect("tenant id");
    let substitution = restarted
        .stop_with_assignment(&other_tenant, &id, Some(&assignment))
        .expect_err("another tenant must not retire the persisted PEP assignment");
    assert!(
        substitution
            .to_string()
            .contains("does not match the caller"),
        "teardown must reject the substituted tenant at logical ownership: {substitution}"
    );
    let still_active = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("port lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(
        still_active.phase(),
        nimbus_network::PortLeasePhase::Active,
        "rejected teardown must not transition the victim lease"
    );
    let ambiguity = restarted
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect_err("a fresh registry cannot prove that the live PEP provider stopped");
    assert!(
        ambiguity
            .to_string()
            .contains("process-local provider evidence"),
        "the error must identify the missing stop proof: {ambiguity}"
    );
    let restart_ambiguity = restarted
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect_err("restart must also require exact process-local provider-stop evidence");
    assert!(
        restart_ambiguity
            .to_string()
            .contains("process-local provider evidence"),
        "restart must identify the missing stop proof: {restart_ambiguity}"
    );

    let fenced = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("port lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(
        fenced.phase(),
        nimbus_network::PortLeasePhase::Active,
        "missing local provider evidence must fail before withdrawing or releasing"
    );
    assert!(
        trust_anchor_path.exists(),
        "the fresh registry must not delete another live PEP's trust anchor"
    );
    assert!(
        first
            .contains(&tenant, &id)
            .expect("original registry should inspect"),
        "the original process-local provider must remain registered"
    );

    first
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("the registry owning the exact provider handle may stop and release it");
    let released = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("port lease should inspect")
        .expect("released lease should remain durable");
    assert_eq!(released.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(
        !trust_anchor_path.exists(),
        "confirmed local provider shutdown must remove its own trust anchor"
    );
    restarted
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("a released persisted assignment is idempotently absent after restart");
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .expect("confirmed shutdown and release must make the real port reusable");
}

#[test]
fn stop_for_restart_rebinds_exact_active_lease() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-restart-rebind");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("persisted PEP assignment should reserve");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease,
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    ensure_egress_proxy_running(
        &registry,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("initial PEP should activate");

    registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect("restart stop should confirm provider absence");
    assert!(
        !registry
            .contains(&tenant, &id)
            .expect("registry should inspect")
    );
    assert!(
        !trust_anchor_path.exists(),
        "restart stop must withdraw the old trust anchor"
    );
    let retained = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("restart lease should remain durable");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert_eq!(retained.reserved_port().map(NonZeroU16::get), Some(port));
    assert!(
        retained.reservation_claim().is_none()
            && retained.bind_claim().is_none()
            && retained.binding().is_none()
            && retained.failure().is_none(),
        "confirmed provider stop must leave only clean same-generation rebind authority"
    );

    registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect("lost restart acknowledgement must replay from exact clean Reserved authority");
    let replayed = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("restart lease should remain durable");
    assert_eq!(replayed, retained, "the replay must not mutate authority");

    let duplicate_rule = nimbus_egress::EgressRule::new(
        "duplicate",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    );
    let invalid_policy = EgressPolicy::new([duplicate_rule.clone(), duplicate_rule]);
    let preparation_error =
        ensure_egress_proxy_running(&registry, &tenant, &id, Some(&assignment), &invalid_policy)
            .expect_err("invalid restart policy must fail before a replacement PEP bind");
    assert!(
        matches!(preparation_error, SandboxError::InvalidSpec { .. }),
        "the preparation failure must retain its policy cause: {preparation_error}"
    );
    let retryable = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("restart lease should remain durable");
    assert_eq!(
        retryable.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "PEP preparation does not own authority to terminally release a restart-retained request"
    );
    assert_eq!(
        retryable.reserved_port().map(NonZeroU16::get),
        Some(port),
        "a failed restart preparation must preserve the exact numeric fence"
    );

    ensure_egress_proxy_running(
        &registry,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("the exact retained lease should claim, bind, adopt, and activate again");
    assert!(
        registry
            .contains(&tenant, &id)
            .expect("registry should inspect")
    );
    assert!(trust_anchor_path.is_file());
    let rebound = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("rebound lease should remain durable");
    assert_eq!(rebound.phase(), nimbus_network::PortLeasePhase::Active);

    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("test PEP should stop and release");
}

#[test]
fn restart_stop_followed_by_final_stop_releases_exact_confirmed_absent_pep() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-restart-then-release");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("persisted PEP assignment should reserve");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease,
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    ensure_egress_proxy_running(
        &registry,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("initial PEP should activate");

    registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect("restart stop should durably confirm the exact provider absence");
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("final stop should consume the exact durable absence proof");

    let released = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        released.phase(),
        nimbus_network::PortLeasePhase::Released,
        "final stop must terminally release the restart-retained port"
    );
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .expect("the exact confirmed-absent listener port must be reusable");
}

#[test]
fn activation_ack_loss_rebinds_after_confirmed_pre_start_provider_drop() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-activation-ack-loss");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("persisted PEP assignment should reserve");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    )
    .with_post_activation_observer(|| {
        Err(SandboxError::OperationFailed {
            message: "injected durable activation acknowledgement loss".to_owned(),
        })
    });
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);

    let error = registry
        .ensure_running_with_lease(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            (Ipv4Addr::LOCALHOST, port).into(),
            &port_lease,
        )
        .expect_err("lost activation acknowledgement must not publish the PEP");
    assert!(
        error
            .to_string()
            .contains("injected durable activation acknowledgement loss"),
        "the injected activation error remains primary: {error}"
    );
    assert!(
        !registry
            .contains(&tenant, &id)
            .expect("registry should remain inspectable"),
        "a PEP whose activation was not acknowledged must not become reachable"
    );
    assert!(
        !trust_anchor_path.exists(),
        "compensation must withdraw the unpublished attempt's trust anchor"
    );
    let retained = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("listener authority should remain durable");
    assert_eq!(
        retained.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "an exact Active record with a confirmed-dropped prepared socket must return to Reserved"
    );
    assert!(
        retained.bind_claim().is_none() && retained.binding().is_none(),
        "rebind preparation must clear the completed attempt's transient and provider evidence"
    );

    let retry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    retry
        .ensure_running_with_lease(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            (Ipv4Addr::LOCALHOST, port).into(),
            &port_lease,
        )
        .expect("the exact retained request should retry without reallocating identity");
    retry
        .stop_with_assignment(
            &tenant,
            &id,
            Some(&EgressProxyAssignment {
                host: Ipv4Addr::LOCALHOST.to_string(),
                port,
                port_lease,
            }),
        )
        .expect("retry provider should stop cleanly");
}

#[test]
fn post_deregister_cleanup_failure_retains_retryable_evidence() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-stop-retry");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("persisted PEP assignment should reserve");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease,
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    ensure_egress_proxy_running(
        &registry,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("PEP should activate before cleanup failure");
    fs::remove_file(&trust_anchor_path).expect("published anchor should remove");
    fs::create_dir(&trust_anchor_path).expect("directory-shaped anchor should create");
    fs::write(trust_anchor_path.join("blocker"), b"retain")
        .expect("nonempty blocker should force removal failure");

    let first_error = registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect_err("fallible anchor cleanup must preserve retry evidence");
    assert!(
        first_error
            .to_string()
            .contains("failed to remove egress trust anchor"),
        "the first stop must surface the concrete cleanup failure: {first_error}"
    );
    assert!(
        !registry
            .contains(&tenant, &id)
            .expect("registry should inspect"),
        "a stopping tombstone must not publish readiness as a running PEP"
    );
    let withdrawing = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("withdrawing lease should remain durable");
    assert_eq!(
        withdrawing.phase(),
        nimbus_network::PortLeasePhase::Withdrawing
    );
    let port_probe = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .expect("acknowledged PEP shutdown must make the physical port bindable");
    drop(port_probe);

    fs::remove_file(trust_anchor_path.join("blocker")).expect("blocker should remove");
    fs::remove_dir(&trust_anchor_path).expect("directory-shaped anchor should remove");
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("retry must resume exact cleanup evidence and release authority");
    let released = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("released lease should remain durable");
    assert_eq!(released.phase(), nimbus_network::PortLeasePhase::Released);
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .expect("completed cleanup must leave the real port reusable");
}

#[test]
fn ensure_running_rejects_a_different_lease_for_an_existing_pep() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-lease-mismatch");
    let first = start_test_pep(&registry, &tenant, &id, &EgressPolicy::deny_all());

    let different = port_lease_request(
        &tenant,
        &id,
        "different-listener",
        OciPortLeaseIntent::host_internal(
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        ),
        nimbus_network::PortRequestMode::ProviderAssigned,
    );
    let error = registry
        .ensure_running_with_lease(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            loopback(),
            &different,
        )
        .expect_err("a running PEP must reject different lifecycle authority");
    assert!(
        error
            .to_string()
            .contains("does not match durable port lease")
            && error.to_string().contains(different.lease_id().as_str()),
        "mismatch must identify the rejected lease without replacing the PEP: {error}"
    );
    assert!(registry.contains(&tenant, &id).expect("registry lookup"));
    registry
        .stop_with_assignment(&tenant, &id, Some(&first))
        .expect("first PEP should cleanly stop");
}

#[test]
fn overlapping_restart_claim_rejection_preserves_the_live_pep_trust_anchor() {
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe.local_addr().expect("probe address");
    drop(port_probe);

    let state_root = tempfile::TempDir::new().expect("state root should exist");
    let decision_log_root = state_root.path().join("decision-logs");
    let trust_anchor_root = state_root.path().join("trust-anchors");
    let tenant = tenant();
    let id = SandboxId::new("egress-overlapping-restart");
    let manager = OciPortLeaseCoordinator::new(state_root.path(), address.port()..=address.port());
    let (_, request) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");

    let first = EgressProxyRegistry::with_roots_and_network_state(
        &decision_log_root,
        &trust_anchor_root,
        state_root.path(),
    );
    first
        .ensure_running_with_lease(&tenant, &id, &EgressPolicy::deny_all(), address, &request)
        .expect("first process should own the live PEP");
    let trust_anchor_path = first.trust_anchor_path_for_test(&tenant, &id);
    let original_anchor =
        fs::read(&trust_anchor_path).expect("live PEP trust anchor should be readable");

    let overlapping = EgressProxyRegistry::with_roots_and_network_state(
        &decision_log_root,
        &trust_anchor_root,
        state_root.path(),
    );
    let error = overlapping
        .ensure_running_with_lease(&tenant, &id, &EgressPolicy::deny_all(), address, &request)
        .expect_err("overlapping restart must lose to the live durable authority");
    assert!(
        error
            .to_string()
            .contains("remains owned by live process lifetime")
            && error.to_string().contains("ProcessBound"),
        "the overlapping restart must stop before a second provider bind: {error}"
    );
    assert_eq!(
        fs::read(&trust_anchor_path).expect("live trust anchor must survive"),
        original_anchor,
        "a caller that never owned the socket must not replace or remove the live PEP anchor"
    );
    assert!(
        first.contains(&tenant, &id).expect("first registry lookup"),
        "the original in-memory PEP must remain registered"
    );

    let assignment = EgressProxyAssignment {
        host: address.ip().to_string(),
        port: address.port(),
        port_lease: request,
    };
    first
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("the original PEP should stop cleanly");
}

#[test]
fn same_request_pep_replay_cannot_terminalize_the_claimed_attempt() {
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe.local_addr().expect("probe address");
    drop(port_probe);

    let state_root = tempfile::TempDir::new().expect("state root should exist");
    let decision_log_root = state_root.path().join("decision-logs");
    let trust_anchor_root = state_root.path().join("trust-anchors");
    let tenant = tenant();
    let id = SandboxId::new("egress-same-request-claim");
    let manager = OciPortLeaseCoordinator::new(state_root.path(), address.port()..=address.port());
    let (_, request) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");

    let (claimed_tx, claimed_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    let first = EgressProxyRegistry::with_roots_and_network_state(
        &decision_log_root,
        &trust_anchor_root,
        state_root.path(),
    )
    .with_post_bind_claim_observer({
        let release_rx = Arc::clone(&release_rx);
        move || {
            claimed_tx.send(()).expect("claim observer should signal");
            release_rx
                .lock()
                .expect("claim release receiver should lock")
                .recv_timeout(Duration::from_secs(1))
                .expect("claim observer should release");
        }
    });
    let first_tenant = tenant.clone();
    let first_id = id.clone();
    let first_request = request.clone();
    let first_thread = std::thread::spawn(move || {
        let result = first.ensure_running_with_lease(
            &first_tenant,
            &first_id,
            &EgressPolicy::deny_all(),
            address,
            &first_request,
        );
        (first, result)
    });
    claimed_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first PEP should durably claim before binding");

    let contender = EgressProxyRegistry::with_roots_and_network_state(
        &decision_log_root,
        &trust_anchor_root,
        state_root.path(),
    );
    let assignment = EgressProxyAssignment {
        host: address.ip().to_string(),
        port: address.port(),
        port_lease: request.clone(),
    };
    let restart_error = contender
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect_err("a Reserved lease with an in-flight bind claim is not completed cleanup");
    assert!(
        restart_error.to_string().contains("provider evidence"),
        "restart replay must retain the live claimant's fence: {restart_error}"
    );
    let error = contender
        .ensure_running_with_lease(&tenant, &id, &EgressPolicy::deny_all(), address, &request)
        .expect_err("another process must not acquire the claimed request");
    assert!(
        error
            .to_string()
            .contains("still has a live process-lifetime owner"),
        "the losing replay must report claim contention without binding: {error}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should open");
    let claimed = authority
        .inspect(request.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(claimed.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert!(claimed.bind_claim().is_some());
    assert_eq!(claimed.failure(), None);
    assert_eq!(claimed.binding(), None);

    release_tx
        .send(())
        .expect("first PEP bind claim should release");
    let (first, first_result) = first_thread.join().expect("first PEP thread should join");
    first_result.expect("the exact claimant should bind, adopt, and activate");
    let active = authority
        .inspect(request.lease_id())
        .expect("active lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(active.phase(), nimbus_network::PortLeasePhase::Active);
    assert_eq!(active.bind_claim(), None);
    assert!(first.contains(&tenant, &id).expect("first registry lookup"));
    first
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("the exact claimant should stop cleanly");
}

#[test]
fn pep_bind_collision_records_durable_no_effect_failure() {
    let external =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("external owner bind");
    let address = external.local_addr().expect("external address");
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-bind-collision");
    let manager = OciPortLeaseCoordinator::new(
        registry.network_state_root.as_path(),
        address.port()..=address.port(),
    );
    let (_, request, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("listener target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before the provider bind");

    let error = registry
        .ensure_running_with_lease_and_release_authority(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            address,
            &request,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&reservation_claim),
        )
        .expect_err("the real PEP bind must lose to the external owner");
    assert!(
        error.to_string().contains("failed to bind egress proxy")
            && error.to_string().contains(&address.to_string()),
        "the provider collision should be explicit: {error}"
    );
    assert!(!registry.contains(&tenant, &id).expect("registry lookup"));

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(registry.network_state_root.as_path())
            .expect("authority should restart");
    let record = authority
        .inspect(request.lease_id())
        .expect("lease inspection")
        .expect("failed lease should persist");
    assert_eq!(record.phase(), nimbus_network::PortLeasePhase::Failed);
    let failure = record.failure().expect("failure evidence");
    assert_eq!(
        failure.kind(),
        nimbus_network::PortBindFailureKind::AddrInUse
    );
    assert_eq!(failure.attempt().port(), address.port());
    let assignment = EgressProxyAssignment {
        host: address.ip().to_string(),
        port: address.port(),
        port_lease: request,
    };
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("a failed no-effect assignment is idempotently absent");
    drop(external);
}

#[test]
fn ensure_running_replaces_placeholder_with_public_trust_anchor_only() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let registry = EgressProxyRegistry::with_roots(
        temp_dir.path().join("logs"),
        temp_dir.path().join("trust"),
    );
    let tenant = tenant();
    let id = SandboxId::new("egress-trust-anchor");
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    prepare_egress_trust_anchor_file(&temp_dir.path().join("trust"), &trust_anchor_path)
        .expect("planning should materialize a placeholder trust-anchor file");
    assert!(
        fs::read_to_string(&trust_anchor_path)
            .expect("placeholder should read")
            .contains("placeholder"),
        "planning should create a placeholder at the deterministic mount source"
    );

    let assignment = start_test_pep(&registry, &tenant, &id, &EgressPolicy::deny_all());

    let pem = fs::read_to_string(&trust_anchor_path).expect("trust anchor should read");
    assert!(
        pem.contains("-----BEGIN CERTIFICATE-----")
            && pem.contains("-----END CERTIFICATE-----")
            && !pem.contains("PRIVATE KEY")
            && !pem.contains("placeholder"),
        "workloads must receive only the public CA certificate, never the private key or stale placeholder: {pem}"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("PEP stop should clean up");
    assert!(
        !trust_anchor_path.exists(),
        "trust-anchor cleanup should remove the workload-scoped CA file"
    );
}

#[test]
fn distinct_sandboxes_receive_distinct_ephemeral_cas() {
    // Cross-sandbox isolation invariant: the registry must publish a
    // DIFFERENT ephemeral CA per sandbox. A shared/centralized CA would be a
    // cross-tenant MITM blast radius — the property that distinguishes our
    // per-sandbox PEP from the shared-CA gateway designs.
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let registry = EgressProxyRegistry::with_roots(
        temp_dir.path().join("logs"),
        temp_dir.path().join("trust"),
    );
    let tenant = tenant();
    let first = SandboxId::new("egress-ca-a");
    let second = SandboxId::new("egress-ca-b");
    let first_assignment = start_test_pep(&registry, &tenant, &first, &EgressPolicy::deny_all());
    let second_assignment = start_test_pep(&registry, &tenant, &second, &EgressPolicy::deny_all());

    let first_pem = fs::read_to_string(registry.trust_anchor_path_for_test(&tenant, &first))
        .expect("first trust anchor should read");
    let second_pem = fs::read_to_string(registry.trust_anchor_path_for_test(&tenant, &second))
        .expect("second trust anchor should read");
    assert!(
        first_pem.contains("-----BEGIN CERTIFICATE-----")
            && !first_pem.contains("PRIVATE KEY")
            && !second_pem.contains("PRIVATE KEY"),
        "each published anchor must be public-cert-only"
    );
    assert_ne!(
        first_pem, second_pem,
        "two sandboxes must receive distinct ephemeral CAs, never a shared one"
    );
    registry
        .stop_with_assignment(&tenant, &first, Some(&first_assignment))
        .expect("first stop");
    registry
        .stop_with_assignment(&tenant, &second, Some(&second_assignment))
        .expect("second stop");
}

#[test]
fn readiness_is_none_when_no_proxy_is_registered() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-seam-readiness-absent");

    assert!(
        registry
            .readiness(&tenant, &id)
            .expect("readiness lookup should succeed")
            .is_none(),
        "an unregistered sandbox must report no PEP so the gate denies"
    );
}

#[test]
fn readiness_reports_active_policy_for_a_running_proxy() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-seam-readiness-ready");
    registry
        .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback())
        .unwrap();

    let readiness = registry
        .readiness(&tenant, &id)
        .expect("readiness lookup should succeed")
        .expect("a registered proxy should report readiness");
    assert!(
        readiness.is_ready()
            && readiness.audit_healthy()
            && readiness.policy_generation().is_some(),
        "a PEP started with a compiled policy must be ready with an active generation: {readiness:?}"
    );
}

#[test]
fn readiness_reports_not_ready_for_a_policyless_proxy() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-seam-readiness-policyless");
    let proxy = WorkloadPep::start(WorkloadPepConfig::without_active_policy())
        .expect("a policy-less PEP should still bind and start");
    registry
        .insert_running_for_test(&tenant, &id, proxy)
        .unwrap();

    let readiness = registry
        .readiness(&tenant, &id)
        .expect("readiness lookup should succeed")
        .expect("the registered proxy should report readiness");
    assert!(
        !readiness.is_ready()
            && readiness.audit_healthy()
            && readiness.policy_generation().is_none(),
        "a PEP with no loaded policy must report not-ready so the gate denies: {readiness:?}"
    );
}

#[test]
fn reload_fails_closed_when_no_proxy_is_running() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-seam-missing");
    let err = registry
        .reload(&tenant, &id, CompiledEgressPolicy::deny_all())
        .unwrap_err();
    assert!(matches!(err, SandboxError::OperationFailed { .. }));
}

#[test]
fn reload_advances_generation_used_by_running_pep() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-seam-reload");
    let assignment = start_test_pep(&registry, &tenant, &id, &EgressPolicy::deny_all());
    let before = registry
        .readiness(&tenant, &id)
        .expect("pre-reload readiness should inspect")
        .expect("running PEP should report readiness");
    registry
        .reload(
            &tenant,
            &id,
            EgressPolicy::new([nimbus_egress::EgressRule::new(
                "reload-generation-proof",
                nimbus_egress::EgressProtocol::Https,
                "example.com",
                443,
            )])
            .compile()
            .expect("distinct reload policy should compile"),
        )
        .unwrap();
    let after = registry
        .readiness(&tenant, &id)
        .expect("post-reload readiness should inspect")
        .expect("running PEP should remain registered");
    assert_eq!(
        (
            before.is_ready(),
            before.audit_healthy(),
            before
                .policy_generation()
                .map(|generation| generation.get()),
        ),
        (true, true, Some(1)),
        "initial deny-all policy must be the first active generation"
    );
    assert_eq!(
        (
            after.is_ready(),
            after.audit_healthy(),
            after.policy_generation().map(|generation| generation.get()),
        ),
        (true, true, Some(2)),
        "reload must advance the policy generation observed by the running PEP"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .unwrap();
}

#[test]
fn egress_proxy_env_entries_emit_container_shape_for_every_backend() {
    let entries = egress_proxy_env_entries("http://10.89.0.1:15000");

    for expected in [
        "NIMBUS_SANDBOX_EGRESS_PROXY_URL=http://10.89.0.1:15000",
        "HTTP_PROXY=http://10.89.0.1:15000",
        "http_proxy=http://10.89.0.1:15000",
        "HTTPS_PROXY=http://10.89.0.1:15000",
        "https_proxy=http://10.89.0.1:15000",
        "ALL_PROXY=http://10.89.0.1:15000",
        "all_proxy=http://10.89.0.1:15000",
        "NO_PROXY=",
        "no_proxy=",
    ] {
        assert!(
            entries.iter().any(|entry| entry == expected),
            "expected shared proxy env entry {expected:?} in {entries:?}"
        );
    }
    // NO_PROXY/no_proxy must stay empty so nothing is exempt from the PEP.
    assert!(
        entries
            .iter()
            .all(|entry| entry != "NO_PROXY=http://10.89.0.1:15000"),
        "NO_PROXY must remain empty so no destination bypasses the PEP: {entries:?}"
    );
}

#[test]
fn egress_trust_anchor_env_entries_emit_additive_trust_shape() {
    let entries = egress_trust_anchor_env_entries(EGRESS_TRUST_ANCHOR_GUEST_PATH);

    for expected in [
        format!("{EGRESS_CA_BUNDLE_ENV}={EGRESS_TRUST_ANCHOR_GUEST_PATH}"),
        format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}={EGRESS_TRUST_ANCHOR_GUEST_PATH}"),
    ] {
        assert!(
            entries.contains(&expected),
            "expected shared trust-anchor env entry {expected:?} in {entries:?}"
        );
    }
    assert!(
        entries
            .iter()
            .all(|entry| !entry.starts_with("SSL_CERT_FILE=")
                && !entry.starts_with("CURL_CA_BUNDLE=")
                && !entry.starts_with("REQUESTS_CA_BUNDLE=")),
        "trust env must be additive and not replace system roots: {entries:?}"
    );
}

#[test]
fn egress_proxy_assignment_renders_ipv4_proxy_url() {
    let assignment = EgressProxyAssignment::for_test("10.89.0.1", 15000);
    assert_eq!(
        assignment.proxy_url().expect("ipv4 url renders"),
        "http://10.89.0.1:15000"
    );
    assert_eq!(
        assignment.bind_addr().expect("ipv4 bind addr"),
        "10.89.0.1:15000".parse().unwrap()
    );
}

#[test]
fn egress_proxy_assignment_brackets_ipv6_proxy_url() {
    // Rendering through SocketAddr is what makes an IPv6 gateway safe: a raw
    // `format!("http://{host}:{port}")` would emit the malformed
    // `http://::1:15000`. Guard the IPv6-bracket behavior directly.
    let assignment = EgressProxyAssignment::for_test("::1", 15000);
    assert_eq!(
        assignment.proxy_url().expect("ipv6 url renders"),
        "http://[::1]:15000"
    );
}

#[test]
fn egress_proxy_assignment_rejects_non_ip_host() {
    let assignment = EgressProxyAssignment::for_test("gateway.example.com", 15000);
    let error = assignment
        .bind_addr()
        .expect_err("a non-IP gateway host must fail closed");
    assert!(matches!(error, SandboxError::InvalidSpec { .. }));
    assert!(assignment.proxy_url().is_err());
}

#[test]
fn ensure_egress_proxy_running_denies_when_assignment_absent() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-no-assignment");
    let error =
        ensure_egress_proxy_running(&registry, &tenant, &id, None, &EgressPolicy::deny_all())
            .expect_err("a missing assignment must fail closed");
    assert!(matches!(error, SandboxError::OperationFailed { .. }));
    assert!(
        !registry.contains(&tenant, &id).unwrap(),
        "a denied launch must register no PEP"
    );
}

#[test]
fn ensure_egress_proxy_running_starts_pep_for_assignment() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-with-assignment");
    let assignment = start_test_pep(&registry, &tenant, &id, &EgressPolicy::deny_all());
    assert!(registry.contains(&tenant, &id).unwrap());
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .unwrap();
}

#[test]
fn live_sandbox_pep_path_uses_decision_logger_not_noop() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let registry = EgressProxyRegistry::with_decision_log_root(temp_dir.path().join("logs"));
    let tenant = tenant();
    let id = SandboxId::new("egress-live-audit");
    let assignment = start_test_pep(&registry, &tenant, &id, &EgressPolicy::deny_all());
    let local_addr = registry
        .local_addr(&tenant, &id)
        .expect("registry lookup should succeed")
        .expect("a PEP should be registered");
    let mut stream = TcpStream::connect(local_addr).expect("client should connect to PEP");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
            .write_all(
                b"GET http://blocked.test:80/secret?token=test-token-placeholder HTTP/1.1\r\nHost: blocked.test\r\nAuthorization: Bearer test-auth-token\r\n\r\n",
            )
            .expect("client should write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("client should read response");
    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden"),
        "deny-all live PEP should reject the request, got: {response}"
    );

    let log_path = registry.decision_log_path_for_test(&tenant, &id);
    let log_text = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("decision log {} should read: {error}", log_path.display()));
    let lines = log_text.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        1,
        "live PEP decision_logger must emit exactly one terminal event, not use noop: {log_text:?}"
    );
    let event: serde_json::Value =
        serde_json::from_str(lines[0]).expect("decision log line should be JSON");
    assert_eq!(event["tenant_id"], tenant.as_str());
    assert_eq!(event["workload_id"], id.as_str());
    assert_eq!(event["policy_generation"], 1);
    assert_eq!(event["decision"], "deny");
    assert_eq!(event["reason_class"], "default_deny");
    assert_eq!(event["protocol"], "http");
    assert_eq!(event["canonical_host"], "blocked.test");
    assert_eq!(event["port"], 80);
    let rendered_event = event.to_string();
    let redacted_query = ["token", "=<redacted>"].concat();
    assert!(
        rendered_event.contains(&redacted_query),
        "live audit event must redact query values: {rendered_event}"
    );
    assert!(
        !rendered_event.contains("test-token-placeholder"),
        "live audit event must omit query values: {rendered_event}"
    );
    assert!(
        !rendered_event.contains("test-auth-token"),
        "live audit event must omit bearer tokens: {rendered_event}"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .unwrap();
}

#[test]
fn trust_anchor_writer_rejects_paths_outside_root() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let root = temp_dir.path().join("trust");

    for escape in [
        temp_dir.path().join("elsewhere/ca.pem"),
        root.join("../escaped.pem"),
        root.join("tenant/../../escaped.pem"),
        root.clone(),
    ] {
        let error = prepare_egress_trust_anchor_file(&root, &escape)
            .expect_err("a target outside the trust-anchor root must fail closed");
        assert!(
            matches!(error, SandboxError::OperationFailed { .. }),
            "path {} must be rejected",
            escape.display()
        );
        assert!(
            !escape.is_file(),
            "no file may be created at the rejected target {}",
            escape.display()
        );
    }
}

#[test]
fn trust_anchor_writer_rejects_symlinked_directory_escape() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let root = temp_dir.path().join("trust");
    let outside = temp_dir.path().join("outside");
    fs::create_dir_all(&root).expect("trust root should create");
    fs::create_dir_all(&outside).expect("outside dir should create");
    // A tenant directory under the root is a symlink pointing outside it:
    // the lexical component check passes, but the canonical parent escapes.
    symlink(&outside, root.join("tenant-a")).expect("symlink should create");

    let escaped = root.join("tenant-a/sandbox-a.pem");
    let error = prepare_egress_trust_anchor_file(&root, &escaped)
        .expect_err("a symlinked directory escape must fail closed");
    assert!(matches!(error, SandboxError::OperationFailed { .. }));
    assert!(
        !outside.join("sandbox-a.pem").exists(),
        "no trust-anchor bytes may be written through an escaping symlink"
    );
}

#[test]
fn trust_anchor_writer_publishes_atomically_with_explicit_permissions() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let root = temp_dir.path().join("trust");
    let path = root.join("tenant-a/sandbox-a.pem");

    prepare_egress_trust_anchor_file(&root, &path).expect("placeholder should publish");
    // Overwrite (placeholder -> real content) must also go through the
    // temp+rename path and leave no temp residue beside the target.
    prepare_egress_trust_anchor_file(&root, &path).expect("re-publish should succeed");

    let entries: Vec<String> = fs::read_dir(path.parent().unwrap())
        .expect("trust-anchor directory should list")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["sandbox-a.pem".to_owned()],
        "the writer must leave only the published file, no temp residue"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path)
            .expect("published trust anchor should stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o644,
            "trust anchor must be world-readable for the guest bind mount and writable only by the owner"
        );
    }
}

#[test]
fn ensure_running_fails_closed_when_trust_anchor_root_is_unwritable() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let trust_root = temp_dir.path().join("trust");
    fs::create_dir_all(&trust_root).expect("trust root should create");
    fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o555))
        .expect("trust root should become read-only");

    let registry =
        EgressProxyRegistry::with_roots(temp_dir.path().join("logs"), trust_root.clone());
    let tenant = tenant();
    let id = SandboxId::new("egress-unwritable-trust-root");
    let result = registry.ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback());

    // Restore permissions before asserting so TempDir cleanup succeeds
    // even if an assertion fails.
    fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o755))
        .expect("trust root permissions should restore");

    let error = result.expect_err("an unwritable trust-anchor root must fail the PEP start");
    assert!(matches!(error, SandboxError::OperationFailed { .. }));
    assert!(
        !registry.contains(&tenant, &id).unwrap(),
        "a failed trust-anchor publish must register no PEP"
    );
    let records =
        nimbus_network::LocalPortLeaseAuthority::open(registry.network_state_root.as_path())
            .expect("portable authority should reopen")
            .list()
            .expect("PEP leases should list");
    assert_eq!(records.len(), 1, "the failed PEP owns one durable request");
    assert_eq!(
        records[0].phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "PEP preparation may remove its effects but only the launch coordinator owns release"
    );
}

#[test]
fn fresh_launch_capability_releases_claimed_preparation_failure() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe
        .local_addr()
        .expect("probe address should resolve");
    drop(port_probe);
    let tenant = tenant();
    let id = SandboxId::new("egress-fresh-launch-release");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), address.port()..=address.port());
    let (_, port_lease, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("fresh launch should reserve its PEP listener");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        temp_dir.path().join("logs"),
        temp_dir.path().join("trust"),
        temp_dir.path(),
    );
    let duplicate_rule = nimbus_egress::EgressRule::new(
        "duplicate",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    );
    let invalid_policy = EgressPolicy::new([duplicate_rule.clone(), duplicate_rule]);

    let error = registry
        .ensure_running_with_lease_and_release_authority(
            &tenant,
            &id,
            &invalid_policy,
            address,
            &port_lease,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&reservation_claim),
        )
        .expect_err("invalid policy must fail before provider bind");
    assert!(
        matches!(error, SandboxError::InvalidSpec { .. }),
        "preparation must retain its policy cause: {error}"
    );
    let record = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("fresh launch request should remain auditable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Released,
        "only the explicit non-persisted fresh-launch capability may retire proven no-effect authority"
    );
    assert!(
        record.bind_claim().is_none() && record.binding().is_none(),
        "fresh-launch compensation must retain no attempt or provider evidence"
    );
}

#[test]
fn retained_restart_bind_failure_returns_to_reserved_and_retries() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe
        .local_addr()
        .expect("probe address should resolve");
    drop(port_probe);

    let tenant = tenant();
    let id = SandboxId::new("egress-retained-bind-retry");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), address.port()..=address.port());
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("restart authority should reserve before bind");
    let blocker =
        std::net::TcpListener::bind(address).expect("external conflict should occupy the endpoint");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        temp_dir.path().join("logs"),
        temp_dir.path().join("trust"),
        temp_dir.path(),
    );

    registry
        .ensure_running_with_lease_and_release_authority(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            address,
            &port_lease,
            PepPreAdoptionReleaseAuthority::Retain,
        )
        .expect_err("the transient address conflict must fail this attempt");
    let retained = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("retained request should remain");
    assert_eq!(
        retained.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "a no-effect restart bind miss must remain retryable rather than terminal"
    );
    assert!(
        retained.bind_claim().is_none(),
        "the failed attempt must relinquish only its exact bind claim"
    );

    drop(blocker);
    registry
        .ensure_running_with_lease_and_release_authority(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            address,
            &port_lease,
            PepPreAdoptionReleaseAuthority::Retain,
        )
        .expect("the same retained request should bind after the conflict clears");
    let active = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("active request should remain");
    assert_eq!(active.phase(), nimbus_network::PortLeasePhase::Active);
}

#[test]
fn pep_pre_adoption_cleanup_keeps_prepared_socket_exclusive() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let trust_root = temp_dir.path().join("trust");
    fs::create_dir_all(&trust_root).expect("trust root should create");
    fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o555))
        .expect("trust root should become read-only");
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe
        .local_addr()
        .expect("probe address should resolve");
    drop(port_probe);

    let tenant = tenant();
    let id = SandboxId::new("egress-pre-adoption-exclusion");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), address.port()..=address.port());
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");
    let observed_bind_error = Arc::new(std::sync::Mutex::new(None));
    let observer_result = Arc::clone(&observed_bind_error);
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        temp_dir.path().join("logs"),
        trust_root.clone(),
        temp_dir.path(),
    )
    .with_pre_adoption_cleanup_observer(move || {
        *observer_result.lock().expect("observer result should lock") =
            std::net::TcpListener::bind(address)
                .err()
                .map(|error| error.kind());
    });

    let result = registry.ensure_running_with_lease(
        &tenant,
        &id,
        &EgressPolicy::deny_all(),
        address,
        &port_lease,
    );

    fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o755))
        .expect("trust root permissions should restore");
    result.expect_err("an unwritable trust-anchor root must fail before PEP adoption");
    assert_eq!(
        *observed_bind_error
            .lock()
            .expect("observer result should lock"),
        Some(std::io::ErrorKind::AddrInUse),
        "attempt-owned cleanup must run while the prepared socket still excludes a replacement binder"
    );
}

#[test]
fn pep_pre_adoption_cleanup_precedes_explicit_coordinator_release() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let trust_root = temp_dir.path().join("trust");
    fs::create_dir_all(&trust_root).expect("trust root should create");
    fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o555))
        .expect("trust root should become read-only");
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe
        .local_addr()
        .expect("probe address should resolve");
    drop(port_probe);

    let tenant = tenant();
    let id = SandboxId::new("egress-pre-adoption-release-order");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), address.port()..=address.port());
    let (_, port_lease, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");
    let replacement_tenant =
        TenantId::new("tenant-egress-replacement").expect("replacement tenant should validate");
    let replacement_id = SandboxId::new("egress-pre-adoption-replacement");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        temp_dir.path().join("logs"),
        trust_root.clone(),
        temp_dir.path(),
    );

    let result = registry.ensure_running_with_lease(
        &tenant,
        &id,
        &EgressPolicy::deny_all(),
        address,
        &port_lease,
    );

    fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o755))
        .expect("trust root permissions should restore");
    result.expect_err("an unwritable trust-anchor root must fail before PEP adoption");
    let replacement_manager =
        OciPortLeaseCoordinator::new(temp_dir.path(), address.port()..=address.port());
    assert!(
        replacement_manager
            .reserve_internal_listener(
                &replacement_tenant,
                &replacement_id,
                "egress-pep",
                target_for_ip(address.ip()).expect("replacement target should validate"),
                PortExposure::Private,
            )
            .is_err(),
        "PEP cleanup must retain the request until its launch coordinator authorizes release"
    );
    let replacement_bind = std::net::TcpListener::bind(address)
        .expect("PEP compensation must drop its prepared socket before returning");
    drop(replacement_bind);

    manager
        .release_never_bound_requests(std::slice::from_ref(&port_lease), &reservation_claim)
        .expect("the explicit fresh-launch coordinator may release after provider absence");
    let (_, replacement_lease, replacement_claim) = replacement_manager
        .reserve_internal_listener_for_coordinator(
            &replacement_tenant,
            &replacement_id,
            "egress-pep",
            target_for_ip(address.ip()).expect("replacement target should validate"),
            PortExposure::Private,
        )
        .expect("replacement authority may reserve after explicit coordinator release");
    let replacement_bind = std::net::TcpListener::bind(address)
        .expect("replacement provider may bind after explicit coordinator release");
    drop(replacement_bind);
    replacement_manager
        .release_never_bound_requests(std::slice::from_ref(&replacement_lease), &replacement_claim)
        .expect("replacement test reservation should release");
}

#[test]
fn pep_pre_adoption_cleanup_failure_keeps_reserved_lease_fenced() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let trust_root = temp_dir.path().join("trust");
    let tenant = tenant();
    let id = SandboxId::new("egress-cleanup-fence");
    let port_probe =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let address = port_probe
        .local_addr()
        .expect("probe address should resolve");
    drop(port_probe);
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), address.port()..=address.port());
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        temp_dir.path().join("logs"),
        &trust_root,
        temp_dir.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    fs::create_dir_all(&trust_anchor_path)
        .expect("directory-shaped anchor should force write and removal failure");

    let error = registry
        .ensure_running_with_lease(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            address,
            &port_lease,
        )
        .expect_err("trust-anchor write and cleanup failure must fail PEP preparation");
    assert!(
        error
            .to_string()
            .contains("pre-adoption compensation also failed"),
        "the original failure must retain cleanup evidence: {error}"
    );
    let record = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "failed trust-anchor removal must retain the never-adopted listener fence for reconciliation"
    );
}

#[test]
fn concurrent_ensure_running_registers_exactly_one_pep() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let registry = EgressProxyRegistry::with_roots(
        temp_dir.path().join("logs"),
        temp_dir.path().join("trust"),
    );
    let tenant = tenant();
    let id = SandboxId::new("egress-concurrent-start");
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    let assignment = reserve_test_pep_assignment(&registry, &tenant, &id);

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let registry = registry.clone();
                let tenant = tenant.clone();
                let id = id.clone();
                let assignment = assignment.clone();
                scope.spawn(move || {
                    ensure_egress_proxy_running(
                        &registry,
                        &tenant,
                        &id,
                        Some(&assignment),
                        &EgressPolicy::deny_all(),
                    )
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .expect("ensure_running thread should not panic")
                .expect("every concurrent ensure_running must succeed");
        }
    });

    assert!(registry.contains(&tenant, &id).unwrap());
    let pem = fs::read_to_string(&trust_anchor_path)
        .expect("the winning PEP must have published its trust anchor");
    assert!(
        pem.contains("-----BEGIN CERTIFICATE-----") && !pem.contains("PRIVATE KEY"),
        "published trust anchor must be the public certificate: {pem}"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("PEP stop should clean up");
    assert!(
        !trust_anchor_path.exists(),
        "stop must remove the published trust anchor"
    );
}

#[test]
fn concurrent_cross_listener_lease_cannot_borrow_pep_registration() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-concurrent-lease-mismatch");
    let requests = ["egress-pep", "different-listener"].map(|listener_name| {
        let request = port_lease_request(
            &tenant,
            &id,
            listener_name,
            OciPortLeaseIntent::host_internal(
                target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
                PortExposure::Private,
            ),
            nimbus_network::PortRequestMode::ProviderAssigned,
        );
        reserve_provider_assigned(
            registry
                .port_authority()
                .expect("same-process PEP port authority should remain available"),
            request,
        )
        .expect("provider-assigned identity should reserve")
    });
    let barrier = Arc::new(std::sync::Barrier::new(2));

    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = requests
            .iter()
            .map(|request| {
                let registry = registry.clone();
                let tenant = tenant.clone();
                let id = id.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    registry.ensure_running_with_lease(
                        &tenant,
                        &id,
                        &EgressPolicy::deny_all(),
                        loopback(),
                        request,
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("registration thread should not panic"))
            .collect::<Vec<_>>()
    });

    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "exactly one lifecycle authority may own the registered PEP: {results:?}"
    );
    let rejected = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .expect("one divergent lease must be rejected");
    let rejected = rejected.to_string();
    assert!(
        rejected.contains("does not match durable port lease")
            || rejected.contains("does not match the caller"),
        "the cross-listener candidate must fail at either durable lease validation or the \
         atomically observed registration fence, never borrow the winner: {rejected}"
    );
    let winner = results
        .iter()
        .position(|result| result.is_ok())
        .expect("one exact lifecycle authority should win");
    let local_addr = registry
        .local_addr(&tenant, &id)
        .expect("winning PEP address should inspect")
        .expect("winning PEP should remain registered");
    let assignment = EgressProxyAssignment {
        host: local_addr.ip().to_string(),
        port: local_addr.port(),
        port_lease: requests[winner].clone(),
    };
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("winning PEP should cleanly stop");
}

#[test]
fn scrub_reserved_egress_env_removes_only_reserved_keys_and_keeps_others() {
    let mut env = vec![
        "PATH=/usr/bin".to_owned(),
        "HTTP_PROXY=http://attacker:1".to_owned(),
        "https_proxy=http://attacker:2".to_owned(),
        format!("{EGRESS_CA_BUNDLE_ENV}=/tmp/attacker-ca.pem"),
        format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/tmp/attacker-node-ca.pem"),
        "MALFORMED".to_owned(),
        "API_KEY=keep-me".to_owned(),
    ];
    scrub_reserved_egress_env(&mut env);

    for reserved in EGRESS_RESERVED_ENV_KEYS {
        assert!(
            !env.iter().any(|entry| env_key(entry) == Some(reserved)),
            "reserved key {reserved} must be scrubbed: {env:?}"
        );
    }
    assert!(
        env.contains(&"PATH=/usr/bin".to_owned())
            && env.contains(&"API_KEY=keep-me".to_owned())
            && env.contains(&"MALFORMED".to_owned()),
        "non-reserved entries must be preserved: {env:?}"
    );
}
